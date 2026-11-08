//! Bounded, read-only library tag reads. Audio playback and discovery never wait for this worker.
use crate::{jobs::Job, library::Track, network::AuthStore};
use curl::easy::Easy;
use lofty::file::TaggedFileExt;
mod ftp_path;
mod index;
mod metadata;
pub use index::Library;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Completion {
    Indexed,
    FileError,
    SourceError,
    Interrupted,
}

use std::{
    io::{self, Read, Seek, SeekFrom},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::Sender,
    },
    time::{Duration, Instant},
};

const CHUNK: usize = 16 * 1024;
const BUDGET: usize = 1024 * 1024;
const REQUESTS: usize = 32;

struct Ranged {
    easy: Easy,
    position: u64,
    length: u64,
    chunk_start: u64,
    chunk: Vec<u8>,
    transferred: usize,
    requests: usize,
    deadline: Instant,
    epoch: Arc<AtomicU64>,
    ticket: u64,
    failure: Option<io::ErrorKind>,
    literal: Option<(String, AuthStore)>,
}
fn error(message: impl ToString) -> io::Error {
    io::Error::other(message.to_string())
}
fn parse_error(e: lofty::error::FileParseError) -> io::Error {
    let mut message = e.to_string();
    let mut cause = std::error::Error::source(&e);
    for _ in 0..6 {
        let Some(c) = cause else { break };
        message.push_str(" / ");
        message.push_str(&c.to_string());
        cause = c.source();
    }
    error(message)
}
fn transfer_error(e: curl::Error) -> io::Error {
    let kind = if e.code() == 78 {
        io::ErrorKind::NotFound
    } else if e.is_couldnt_connect() || e.is_couldnt_resolve_host() {
        io::ErrorKind::NotConnected
    } else if e.is_login_denied() {
        io::ErrorKind::PermissionDenied
    } else if e.is_operation_timedout() {
        io::ErrorKind::TimedOut
    } else if e.is_aborted_by_callback() {
        io::ErrorKind::ConnectionAborted
    } else {
        io::ErrorKind::Other
    };
    io::Error::new(kind, e.to_string())
}
impl Ranged {
    fn open(
        source: &str,
        auth: &AuthStore,
        epoch: Arc<AtomicU64>,
        ticket: u64,
    ) -> io::Result<Self> {
        crate::library::validate_url(source).map_err(error)?;
        let mut easy = Easy::new();
        easy.url(source).map_err(error)?;
        easy.connect_timeout(Duration::from_secs(3))
            .map_err(error)?;
        easy.timeout(Duration::from_secs(5)).map_err(error)?;
        easy.signal(false).map_err(error)?;
        easy.max_recv_speed(256 * 1024).map_err(error)?;
        auth.configure(&mut easy, source).map_err(error)?;
        let deadline = Instant::now() + Duration::from_secs(15);
        easy.progress(true).map_err(error)?;
        let cancellation = epoch.clone();
        easy.progress_function(move |_, _, _, _| {
            cancellation.load(Ordering::Relaxed) == ticket
                && Instant::now() < deadline
                && !crate::cache::audio_transfer_pending()
        })
        .map_err(error)?;
        if ftp_path::needed(source) {
            let mut reader = Self {
                easy,
                position: 0,
                length: 0,
                chunk_start: 0,
                chunk: Vec::new(),
                transferred: 0,
                requests: 0,
                deadline,
                epoch,
                ticket,
                failure: None,
                literal: Some((source.to_owned(), auth.clone())),
            };
            let header = match reader.literal_transfer(4096, true) {
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    // Android FTP servers may require an absolute path in no-CWD mode.
                    // Try the normal login-relative URL first to preserve other servers' homes.
                    let u = url::Url::parse(source).map_err(error)?;
                    let absolute = format!(
                        "{}/%2F{}",
                        &u[..url::Position::BeforePath],
                        u.path().strip_prefix('/').unwrap_or(u.path())
                    );
                    reader.literal = Some((absolute, auth.clone()));
                    reader.literal_transfer(4096, true)?
                }
                result => result?,
            };
            let text = String::from_utf8_lossy(&header);
            reader.length = text
                .lines()
                .find_map(|line| {
                    line.split_once(':')
                        .filter(|(k, _)| k.eq_ignore_ascii_case("Content-Length"))
                        .and_then(|(_, n)| n.trim().parse::<u64>().ok())
                })
                .filter(|n| *n > 0 && *n <= 1 << 30)
                .ok_or_else(|| error("Remote size unavailable"))?;
            return Ok(reader);
        }
        easy.nobody(true).map_err(error)?;
        easy.perform().map_err(transfer_error)?;
        let length = easy.content_length_download().map_err(error)?;
        if !length.is_finite() || length <= 0.0 || length > (1_u64 << 30) as f64 {
            return Err(error("Remote size unavailable or outside metadata limits"));
        }
        easy.nobody(false).map_err(error)?;
        easy.progress(true).map_err(error)?;
        Ok(Self {
            easy,
            position: 0,
            length: length as u64,
            chunk_start: 0,
            chunk: Vec::new(),
            transferred: 0,
            requests: 0,
            deadline,
            epoch,
            ticket,
            failure: None,
            literal: None,
        })
    }
    fn check(&mut self) -> io::Result<()> {
        let result = if self.epoch.load(Ordering::Relaxed) != self.ticket
            || crate::cache::audio_transfer_pending()
        {
            // Interrupted means "retry immediately" to Read::read_exact/read_to_end.
            // Cancellation is permanent for this reader: terminate the parser instead.
            Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "Metadata read cancelled",
            ))
        } else if Instant::now() >= self.deadline {
            Err(error("Metadata read deadline exhausted"))
        } else {
            Ok(())
        };
        if let Err(e) = &result {
            // Lofty wraps I/O failures; retain cancellation for the scheduler so a
            // playback/sync pause never becomes a malformed-file retry penalty.
            self.failure = Some(e.kind());
        }
        result
    }
}
impl Read for Ranged {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        self.check()?;
        if into.is_empty() || self.position >= self.length {
            return Ok(0);
        }
        let end = self.chunk_start + self.chunk.len() as u64;
        if self.position < self.chunk_start || self.position >= end {
            if self.transferred >= BUDGET || self.requests >= REQUESTS {
                return Err(error("Metadata byte/request budget exhausted"));
            }
            let start = self.position;
            let size = CHUNK
                .min(BUDGET - self.transferred)
                .min((self.length - start) as usize);
            if self.literal.is_some() {
                let bytes = self.literal_transfer(size, false)?;
                if bytes.len() != size {
                    return Err(error("Incomplete metadata range"));
                }
                self.transferred += bytes.len();
                self.requests += 1;
                self.chunk_start = start;
                self.chunk = bytes;
                return self.read(into);
            }
            self.easy
                .range(&format!("{start}-{}", start + size as u64 - 1))
                .map_err(error)?;
            let epoch = self.epoch.clone();
            let ticket = self.ticket;
            let deadline = self.deadline;
            let mut bytes = Vec::with_capacity(size);
            {
                let mut transfer = self.easy.transfer();
                transfer
                    .progress_function(move |_, _, _, _| {
                        epoch.load(Ordering::Relaxed) == ticket
                            && Instant::now() < deadline
                            && !crate::cache::audio_transfer_pending()
                    })
                    .map_err(error)?;
                transfer
                    .write_function(|part| {
                        if part.len() > size.saturating_sub(bytes.len()) {
                            return Ok(0);
                        }
                        bytes.extend_from_slice(part);
                        Ok(part.len())
                    })
                    .map_err(error)?;
                if let Err(e) = transfer.perform() {
                    let e = if e.is_aborted_by_callback() && Instant::now() >= self.deadline {
                        error("Metadata read deadline exhausted")
                    } else {
                        transfer_error(e)
                    };
                    self.failure = Some(e.kind());
                    return Err(e);
                }
            }
            if bytes.len() != size {
                return Err(error("Incomplete metadata range"));
            }
            self.transferred += bytes.len();
            self.requests += 1;
            self.chunk_start = start;
            self.chunk = bytes;
        }
        let offset = (self.position - self.chunk_start) as usize;
        let n = into.len().min(self.chunk.len() - offset);
        into[..n].copy_from_slice(&self.chunk[offset..offset + n]);
        self.position += n as u64;
        Ok(n)
    }
}
impl Seek for Ranged {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        self.check()?;
        let position = match from {
            SeekFrom::Start(n) => i128::from(n),
            SeekFrom::Current(n) => i128::from(self.position) + i128::from(n),
            SeekFrom::End(n) => i128::from(self.length) + i128::from(n),
        };
        if position < 0 || position > i128::from(self.length) {
            return Err(error("Metadata seek outside file"));
        }
        self.position = position as u64;
        Ok(self.position)
    }
}

pub(crate) fn read(
    mut track: Track,
    auth: &AuthStore,
    epoch: Arc<AtomicU64>,
    ticket: u64,
) -> io::Result<Track> {
    let mut reader = Ranged::open(&track.source, auth, epoch, ticket)?;
    let length = reader.length;
    if let Some(tag) = metadata::read(&mut reader, length)? {
        if let Some(tag) = tag {
            track.apply_tag(&tag);
        }
        track.metadata_loaded = true;
        return Ok(track);
    }
    let result = (|| {
        lofty::probe::Probe::new(&mut reader)
            .guess_file_type()
            .map_err(error)?
            .options(
                lofty::config::ParseOptions::new()
                    .read_properties(false)
                    .read_cover_art(false),
            )
            .read()
            .map_err(parse_error)
    })();
    let parsed =
        result.map_err(|e| io::Error::new(reader.failure.unwrap_or(e.kind()), e.to_string()))?;
    if let Some(tag) = parsed.primary_tag().or_else(|| parsed.first_tag()) {
        track.apply_tag(tag);
    }
    // A successfully parsed tagless file is indexed too; do not fetch it forever.
    track.metadata_loaded = true;
    Ok(track)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn leading_ftp_names_use_literal_transfer_only_when_needed() {
        assert!(ftp_path::needed("ftp://phone:2121/device/%20%20Track.flac"));
        assert!(!ftp_path::needed("ftp://phone:2121/device/Track.flac"));
        assert!(!ftp_path::needed("https://host/%20Track.flac"));
    }
    // Keep the old failure bounded: read_exact retries Interrupted forever otherwise.
    struct CountedRead {
        reader: Ranged,
        calls: usize,
    }
    impl Read for CountedRead {
        fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
            self.calls += 1;
            if self.calls > 8 {
                return Err(error("regression guard: repeated cancellation"));
            }
            self.reader.read(into)
        }
    }
    fn idle_reader() -> Ranged {
        Ranged {
            easy: Easy::new(),
            position: 0,
            length: 16,
            chunk_start: 0,
            chunk: vec![0; 16],
            transferred: 0,
            requests: 0,
            deadline: Instant::now() + Duration::from_secs(15),
            epoch: Arc::new(AtomicU64::new(0)),
            ticket: 0,
            failure: None,
            literal: None,
        }
    }
    #[test]
    fn cancelled_metadata_read_exact_exits_without_retrying() {
        let mut counted = CountedRead {
            reader: idle_reader(),
            calls: 0,
        };
        counted.reader.epoch.store(1, Ordering::Relaxed);
        let result = counted.read_exact(&mut [0; 4]).unwrap_err();
        assert_eq!(
            counted.calls, 1,
            "cancellation must terminate standard read_exact"
        );
        assert_eq!(result.kind(), io::ErrorKind::ConnectionAborted);
        assert_eq!(
            counted.reader.failure,
            Some(io::ErrorKind::ConnectionAborted)
        );
        assert_eq!(counted.reader.requests, 0);
    }
    #[test]
    fn expired_metadata_deadline_exits_without_retrying() {
        let mut counted = CountedRead {
            reader: idle_reader(),
            calls: 0,
        };
        counted.reader.deadline = Instant::now() - Duration::from_secs(1);
        let result = counted.read_exact(&mut [0; 4]).unwrap_err();
        assert_eq!(counted.calls, 1);
        assert_ne!(result.kind(), io::ErrorKind::Interrupted);
        assert_eq!(counted.reader.failure, Some(result.kind()));
        assert_eq!(counted.reader.requests, 0);
    }
    #[test]
    fn cancelled_curl_transfer_is_not_a_retryable_read_interruption() {
        let error = transfer_error(curl::Error::new(42)); // CURLE_ABORTED_BY_CALLBACK
        assert_eq!(error.kind(), io::ErrorKind::ConnectionAborted);
    }
    #[test]
    #[ignore = "Synthetic FTP server provided by scripts/test_ftp.py"]
    fn live_ftp_album_tags_read_without_audio_downloads() {
        let root = std::env::var("AUREA_TEST_FTP_URL").unwrap();
        let mut source = crate::network::FtpSource::smartphone();
        source.set_url(&root).unwrap();
        source.username = "fixture".into();
        source.password = "fixture-only".into();
        let auth = AuthStore::default();
        auth.insert(&source).unwrap();
        for (name, title) in [
            ("02 Interlude.flac", "Interlude"),
            ("03 Finale.m4a", "Finale"),
            ("04 Encore.mp3", "Encore"),
        ] {
            let folder = crate::network::child_url(&root, "Album Città", true).unwrap();
            let url = crate::network::child_url(&folder, name, false).unwrap();
            let result = read(
                Track::placeholder(url),
                &auth,
                Arc::new(AtomicU64::new(1)),
                1,
            )
            .unwrap();
            assert_eq!(result.title, title);
            assert_eq!(result.artist, "Aurea Test Ensemble");
            assert_eq!(result.album, "Network Sessions");
            assert!(result.metadata_loaded);
            assert_eq!(result.rate, 0);
        }
        let url = crate::network::child_url(&root, ".range-probe.bin", false).unwrap();
        let mut reader = Ranged::open(&url, &auth, Arc::new(AtomicU64::new(1)), 1).unwrap();
        let parsed = lofty::probe::Probe::new(&mut reader)
            .guess_file_type()
            .unwrap()
            .options(
                lofty::config::ParseOptions::new()
                    .read_properties(false)
                    .read_cover_art(false),
            )
            .read()
            .unwrap();
        use lofty::tag::Accessor;
        assert_eq!(
            parsed.primary_tag().unwrap().title().as_deref(),
            Some("Range Title")
        );
        assert!(reader.length > 5_000_000);
        assert!(
            reader.transferred < 256 * 1024,
            "cover/audio must be skipped: {} bytes",
            reader.transferred
        );
        reader.epoch.store(2, Ordering::Relaxed);
        assert!(reader.read(&mut [0; 4]).is_err());
        assert!(reader.seek(SeekFrom::Start(u64::MAX)).is_err());
    }
    #[test]
    #[ignore = "Synthetic FTP server provided by scripts/test_ftp.py"]
    fn live_ftp_metadata_edges_skip_images_and_preserve_leading_names() {
        use lofty::tag::Accessor;
        let root = std::env::var("AUREA_TEST_FTP_URL").unwrap();
        let mut source = crate::network::FtpSource::smartphone();
        source.set_url(&root).unwrap();
        source.username = "fixture".into();
        source.password = "fixture-only".into();
        let auth = AuthStore::default();
        auth.insert(&source).unwrap();
        for (name, title) in [
            ("    Leading.flac", "Interlude"),
            ("Large Picture.mp3", "After Picture"),
        ] {
            let url = crate::network::child_url(&root, name, false).unwrap();
            let mut reader = Ranged::open(&url, &auth, Arc::new(AtomicU64::new(1)), 1).unwrap();
            let length = reader.length;
            let tag = metadata::read(&mut reader, length)
                .unwrap()
                .unwrap()
                .unwrap();
            assert_eq!(tag.title().as_deref(), Some(title));
            assert!(
                reader.transferred < 128 * 1024,
                "skip artwork: {}",
                reader.transferred
            );
            assert!(reader.requests < 10);
            if name.starts_with(' ') {
                let old = crate::network::child_url(&root, name.trim_start(), false).unwrap();
                assert_eq!(super::index::verified_space_alias(&old, &auth), Some(url));
            }
        }
    }
    #[test]
    #[ignore = "Explicit private USB-header audit against a local synthetic FTP endpoint"]
    fn real_remote_tag_audit() {
        let root = std::env::var("MUSICCRAFT_TAG_AUDIT_URL").unwrap();
        let reference = std::env::var("MUSICCRAFT_TAG_AUDIT_REFERENCE").unwrap();
        let rows: Vec<serde_json::Value> =
            serde_json::from_slice(&std::fs::read(reference).unwrap()).unwrap();
        let mut source = crate::network::FtpSource::smartphone();
        source.set_url(&root).unwrap();
        source.username = "fixture".into();
        source.password = "fixture-only".into();
        let auth = AuthStore::default();
        auth.insert(&source).unwrap();
        for row in &rows {
            let name = std::path::Path::new(row["file"].as_str().unwrap())
                .file_name()
                .unwrap()
                .to_str()
                .unwrap();
            let url = crate::network::child_url(&root, name, false).unwrap();
            let track = read(
                Track::placeholder(url),
                &auth,
                Arc::new(AtomicU64::new(1)),
                1,
            )
            .unwrap();
            assert_eq!(track.title, row["tags"]["TITLE"][0].as_str().unwrap());
            assert_eq!(track.artist, row["tags"]["ARTIST"][0].as_str().unwrap());
            assert_eq!(track.album, row["tags"]["ALBUM"][0].as_str().unwrap());
            assert_eq!(
                track.number,
                row["tags"]["TRACKNUMBER"][0]
                    .as_str()
                    .unwrap()
                    .parse::<u32>()
                    .unwrap()
            );
        }
        println!(
            "{} real USB header tag records match through the ranged FTP reader",
            rows.len()
        );
    }
}
