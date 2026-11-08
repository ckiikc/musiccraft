//! Seekable compressed bytes. Only the decoder worker touches the network or disk.
use super::Cancellation;
use crate::{cache, network::AuthStore};
use curl::easy::Easy;
use std::{
    fs::{self, File, OpenOptions},
    hash::{Hash, Hasher},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Mutex,
    time::Duration,
};
const BLOCK: usize = 256 * 1024;
const FTP_BLOCK: usize = 1024 * 1024;
const MAX_STREAM_FILE: u64 = 64 * 1024 * 1024 * 1024;
fn error(message: impl ToString) -> io::Error {
    io::Error::other(message.to_string())
}

pub(super) fn ftp_source(source: &str) -> bool {
    matches!(
        source.split_once(':').map(|(scheme, _)| scheme),
        Some("ftp" | "ftps")
    )
}

pub(super) fn input(
    source: &str,
    root: &Path,
    auth: &AuthStore,
    cancel: Cancellation,
) -> Result<Box<dyn symphonia::core::io::MediaSource>, String> {
    if !crate::library::is_remote(source) {
        return File::open(source)
            .map(|f| Box::new(f) as _)
            .map_err(|_| "File locale/NAS non disponibile".into());
    }
    crate::library::validate_url(source)?;
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hash);
    let target = root.join(format!("{:016x}.audio", hash.finish()));
    if target.is_file()
        && target
            .metadata()
            .is_ok_and(|m| m.len() > 0 && m.len() <= cache::MAX_FILE)
    {
        return File::open(target)
            .map(|f| Box::new(f) as _)
            .map_err(|_| "Cache audio non leggibile".into());
    }
    Remote::open(source, root, target, auth, cancel)
        .map(|r| Box::new(r) as _)
        .map_err(|e| format!("Lettura audio dalla rete: {e}"))
}

struct Remote {
    easy: Mutex<Easy>,
    source: String,
    auth: AuthStore,
    cancel: Cancellation,
    length: u64,
    position: u64,
    block_start: u64,
    block_size: usize,
    block: Vec<u8>,
    file: File,
    part: PathBuf,
    target: PathBuf,
    root: PathBuf,
    blocks: Vec<bool>,
    literal: bool,
}
impl Remote {
    fn open(
        source: &str,
        root: &Path,
        target: PathBuf,
        auth: &AuthStore,
        cancel: Cancellation,
    ) -> io::Result<Self> {
        let _priority = cache::AudioTransfer::begin();
        cancel.check()?;
        let mut easy = Easy::new();
        easy.url(source).map_err(error)?;
        easy.connect_timeout(Duration::from_secs(3))
            .map_err(error)?;
        easy.timeout(Duration::from_secs(12)).map_err(error)?;
        easy.signal(false).map_err(error)?;
        easy.fail_on_error(true).map_err(error)?;
        auth.configure(&mut easy, source).map_err(error)?;
        easy.progress(true).map_err(error)?;
        let cancellation = cancel.clone();
        easy.progress_function(move |_, _, _, _| !cancellation.cancelled())
            .map_err(error)?;
        let literal = url::Url::parse(source).is_ok_and(|u| {
            matches!(u.scheme(), "ftp" | "ftps")
                && u.path_segments()
                    .and_then(|mut p| p.next_back())
                    .is_some_and(|s| {
                        percent_encoding::percent_decode_str(s)
                            .decode_utf8_lossy()
                            .starts_with(char::is_whitespace)
                    })
        });
        let mut effective = source.to_owned();
        let length = if literal {
            let mut result = literal_request(&effective, auth, &cancel, None);
            if result
                .as_ref()
                .is_err_and(|e| e.kind() == io::ErrorKind::NotFound)
            {
                let u = url::Url::parse(source).map_err(error)?;
                effective = format!(
                    "{}/%2F{}",
                    &u[..url::Position::BeforePath],
                    u.path().strip_prefix('/').unwrap_or(u.path())
                );
                result = literal_request(&effective, auth, &cancel, None);
            }
            String::from_utf8_lossy(&result?)
                .lines()
                .find_map(|line| {
                    line.split_once(':')
                        .filter(|(k, _)| k.eq_ignore_ascii_case("Content-Length"))
                        .and_then(|(_, n)| n.trim().parse::<u64>().ok())
                })
                .ok_or_else(|| error("Dimensione remota non disponibile"))?
        } else {
            easy.nobody(true).map_err(error)?;
            easy.perform().map_err(error)?;
            let n = easy.content_length_download().map_err(error)?;
            if !n.is_finite() || n <= 0. || n > MAX_STREAM_FILE as f64 {
                return Err(error(
                    "Dimensione audio non disponibile o oltre il limite di streaming",
                ));
            }
            easy.nobody(false).map_err(error)?;
            n as u64
        };
        if length == 0 || length > MAX_STREAM_FILE {
            return Err(error("Dimensione audio oltre il limite di streaming"));
        }
        fs::create_dir_all(root)?;
        let part = target.with_extension(format!("{:016x}.stream-part", rand::random::<u64>()));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&part)?;
        // Ordinary FTP opens a data connection for every range. Larger
        // windows reduce REST/RETR round trips without reading the whole file.
        let block_size = if !literal && ftp_source(source) {
            FTP_BLOCK
        } else {
            BLOCK
        };
        Ok(Self {
            easy: Mutex::new(easy),
            source: effective,
            auth: auth.clone(),
            cancel,
            length,
            position: 0,
            block_start: 0,
            block_size,
            block: vec![],
            file,
            part,
            target,
            root: root.to_owned(),
            blocks: vec![false; length.div_ceil(block_size as u64) as usize],
            literal,
        })
    }
    fn fetch(&mut self, start: u64, size: usize) -> io::Result<Vec<u8>> {
        let _priority = cache::AudioTransfer::begin();
        self.cancel.check()?;
        if self.literal {
            let bytes =
                literal_request(&self.source, &self.auth, &self.cancel, Some((start, size)))?;
            self.cancel.transferred(bytes.len());
            return Ok(bytes);
        }
        let result = self.fetch_range(start, size);
        if ftp_source(&self.source)
            && result.as_ref().is_err_and(|err| {
                matches!(
                    err.kind(),
                    io::ErrorKind::TimedOut
                        | io::ErrorKind::ConnectionReset
                        | io::ErrorKind::UnexpectedEof
                )
            })
        {
            self.cancel.check()?;
            self.fetch_range(start, size)
        } else {
            result
        }
    }

    fn fetch_range(&mut self, start: u64, size: usize) -> io::Result<Vec<u8>> {
        let easy = self.easy.get_mut().map_err(error)?;
        easy.range(&format!("{start}-{}", start + size as u64 - 1))
            .map_err(error)?;
        let mut bytes = Vec::with_capacity(size);
        let mut content_range = None;
        {
            let mut transfer = easy.transfer();
            transfer
                .header_function(|line| {
                    if let Ok(line) = std::str::from_utf8(line)
                        && let Some((k, value)) = line.split_once(':')
                        && k.eq_ignore_ascii_case("Content-Range")
                    {
                        content_range = Some(value.trim().to_owned());
                    }
                    true
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
            transfer.perform().map_err(|err| {
                let kind = if err.is_operation_timedout() {
                    io::ErrorKind::TimedOut
                } else if err.is_couldnt_connect()
                    || err.is_send_error()
                    || err.is_recv_error()
                    || err.is_got_nothing()
                {
                    io::ErrorKind::ConnectionReset
                } else if err.is_partial_file() {
                    io::ErrorKind::UnexpectedEof
                } else {
                    io::ErrorKind::Other
                };
                io::Error::new(kind, err)
            })?;
        }
        if self.source.starts_with("http")
            && (easy.response_code().map_err(error)? != 206
                || content_range.as_deref()
                    != Some(&format!(
                        "bytes {start}-{}/{}",
                        start + size as u64 - 1,
                        self.length
                    )))
        {
            return Err(error(
                "Il server non supporta intervalli audio verificabili",
            ));
        }
        self.cancel.check()?;
        if bytes.len() != size {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "Intervallo audio incompleto",
            ));
        }
        self.cancel.transferred(bytes.len());
        Ok(bytes)
    }
}
impl Read for Remote {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        self.cancel.check()?;
        if into.is_empty() || self.position == self.length {
            return Ok(0);
        }
        if self.position < self.block_start
            || self.position >= self.block_start + self.block.len() as u64
        {
            let index = self.position / self.block_size as u64;
            let start = index * self.block_size as u64;
            let size = self.block_size.min((self.length - start) as usize);
            self.block = if self.blocks[index as usize] {
                let mut bytes = vec![0; size];
                self.file.seek(SeekFrom::Start(start))?;
                self.file.read_exact(&mut bytes)?;
                bytes
            } else {
                let bytes = self.fetch(start, size)?;
                if self.length <= cache::MAX_FILE {
                    self.file.seek(SeekFrom::Start(start))?;
                    self.file.write_all(&bytes)?;
                    self.blocks[index as usize] = true;
                }
                bytes
            };
            self.block_start = start;
        }
        let offset = (self.position - self.block_start) as usize;
        let count = into.len().min(self.block.len() - offset);
        into[..count].copy_from_slice(&self.block[offset..offset + count]);
        self.position += count as u64;
        Ok(count)
    }
}
impl Seek for Remote {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        self.cancel.check()?;
        let n = match from {
            SeekFrom::Start(n) => n as i128,
            SeekFrom::Current(n) => self.position as i128 + n as i128,
            SeekFrom::End(n) => self.length as i128 + n as i128,
        };
        if n < 0 || n > self.length as i128 {
            return Err(error("Seek fuori dal file audio"));
        }
        self.position = n as u64;
        Ok(self.position)
    }
}
impl symphonia::core::io::MediaSource for Remote {
    fn is_seekable(&self) -> bool {
        true
    }
    fn byte_len(&self) -> Option<u64> {
        Some(self.length)
    }
}
impl Drop for Remote {
    fn drop(&mut self) {
        if self.blocks.iter().all(|b| *b)
            && self.file.sync_all().is_ok()
            && fs::rename(&self.part, &self.target).is_ok()
        {
            cache::trim(&self.root, &self.target);
        } else {
            let _ = fs::remove_file(&self.part);
        }
    }
}
fn literal_request(
    source: &str,
    auth: &AuthStore,
    cancel: &Cancellation,
    range: Option<(u64, usize)>,
) -> io::Result<Vec<u8>> {
    let size = range.map_or(4096, |(_, size)| size);
    let mut command = Command::new("/usr/bin/curl");
    command.args([
        "--config",
        "-",
        "--silent",
        "--fail",
        "--ftp-method",
        "nocwd",
        "--connect-timeout",
        "3",
        "--max-time",
        "12",
        "--proto",
        "=ftp,ftps",
    ]);
    if let Some((start, size)) = range {
        command.args(["--range", &format!("{start}-{}", start + size as u64 - 1)]);
    } else {
        command.arg("--head");
    }
    command
        .args(["--url", source])
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child =
        crate::network::authenticated_spawn(&mut command, auth, source).map_err(error)?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| error("Dati FTP non disponibili"))?;
    let reader = std::thread::spawn(move || {
        let mut bytes = vec![];
        stdout
            .take(size as u64 + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let result = loop {
        if let Err(e) = cancel.check() {
            let _ = child.kill();
            let _ = child.wait();
            break Err(e);
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                break if status.success() {
                    Ok(())
                } else {
                    Err(io::Error::new(
                        if status.code() == Some(78) {
                            io::ErrorKind::NotFound
                        } else {
                            io::ErrorKind::Other
                        },
                        "Trasferimento FTP interrotto",
                    ))
                };
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(e);
            }
        }
    };
    let bytes = reader
        .join()
        .map_err(|_| error("Lettura FTP interrotta"))??;
    result?;
    if (range.is_some() && bytes.len() != size) || bytes.len() > size {
        return Err(error("Intervallo FTP non valido"));
    }
    Ok(bytes)
}
