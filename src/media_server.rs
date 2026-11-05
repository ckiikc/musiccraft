//! Opt-in LAN media origin. Only the selected library is exposed, with stable session IDs.
//! Original bytes are served with HTTP byte ranges; no path or credential is accepted in a route.
use crate::{cache, library::Track, network::AuthStore};
use std::{
    collections::HashMap,
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    net::{IpAddr, SocketAddr, TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

pub struct Server {
    pub address: SocketAddr,
    token: String,
    shared: Arc<Shared>,
    stop: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
    thread: Option<std::thread::JoinHandle<()>>,
    pub count: usize,
}
struct Shared {
    catalog: RwLock<Catalog>,
    root: PathBuf,
    auth: AuthStore,
    stop: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
    token: String,
    active: AtomicUsize,
}
#[derive(Default)]
struct Catalog {
    tracks: HashMap<usize, Arc<Track>>,
    ids: HashMap<String, usize>,
    playlist: Arc<str>,
    next_id: usize,
}
impl Catalog {
    fn update(
        &mut self,
        tracks: Vec<Track>,
        aliases: &[(String, String)],
        address: SocketAddr,
        token: &str,
    ) -> bool {
        let ids: HashMap<_, _> = self
            .ids
            .iter()
            .map(|(source, id)| {
                let canonical = aliases.iter().fold(source.clone(), |source, (old, new)| {
                    crate::network::rebase(&source, old, new).unwrap_or(source)
                });
                (canonical, *id)
            })
            .collect();
        let mut next_tracks = HashMap::with_capacity(tracks.len());
        let mut next_ids = HashMap::with_capacity(tracks.len());
        let mut playlist = String::from("#EXTM3U\n");
        for track in tracks.into_iter().take(crate::library::MAX_TRACKS) {
            if next_ids.contains_key(&track.source) {
                continue;
            }
            let id = ids.get(&track.source).copied().unwrap_or_else(|| {
                let id = self.next_id;
                self.next_id += 1;
                id
            });
            let title = format!("{} - {}", track.artist, track.title).replace(['\r', '\n'], " ");
            playlist.push_str(&format!(
                "#EXTINF:{},{}\nhttp://{address}/{token}/track/{id}.{}\n",
                track.duration as u64,
                title,
                extension(&track)
            ));
            next_ids.insert(track.source.clone(), id);
            next_tracks.insert(id, Arc::new(track));
        }
        let changed = self.playlist.as_ref() != playlist || self.ids != next_ids;
        self.tracks = next_tracks;
        self.ids = next_ids;
        if changed {
            self.playlist = playlist.into();
        }
        changed
    }
}
impl Server {
    pub fn start(
        bind: SocketAddr,
        tracks: Vec<Track>,
        root: PathBuf,
        auth: AuthStore,
    ) -> Result<Self, String> {
        if !private(bind.ip()) {
            return Err("Scegli un indirizzo privato della rete locale".into());
        }
        if tracks.is_empty() {
            return Err("Nessun brano nella sorgente selezionata".into());
        }
        let listener = TcpListener::bind(bind).map_err(|_| "Porta server non disponibile")?;
        listener
            .set_nonblocking(true)
            .map_err(|_| "Server non disponibile")?;
        let address = listener
            .local_addr()
            .map_err(|_| "Server non disponibile")?;
        let token = format!("{:032x}", rand::random::<u128>());
        let mut catalog = Catalog::default();
        catalog.update(tracks, &[], address, &token);
        let stop = Arc::new(AtomicBool::new(false));
        let generation = Arc::new(AtomicU64::new(1));
        let count = catalog.tracks.len();
        let shared = Arc::new(Shared {
            catalog: RwLock::new(catalog),
            root,
            auth,
            stop: stop.clone(),
            generation: generation.clone(),
            token: token.clone(),
            active: AtomicUsize::new(0),
        });
        let worker_shared = shared.clone();
        let thread = std::thread::spawn(move || {
            let shared = worker_shared;
            let mut workers = Vec::new();
            while !shared.stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, peer)) => {
                        // Darwin inherits O_NONBLOCK from the listening socket.
                        // A large write would otherwise stop at the socket buffer
                        // with WouldBlock, truncating the media response.
                        if stream.set_nonblocking(false).is_err() {
                            continue;
                        }
                        let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
                        if !private(peer.ip()) || shared.active.load(Ordering::Relaxed) >= 4 {
                            let _ = reply(&mut stream, 503, &[], b"Unavailable", false);
                            continue;
                        }
                        shared.active.fetch_add(1, Ordering::Relaxed);
                        let state = shared.clone();
                        workers.push(std::thread::spawn(move || {
                            let _ = serve(&mut stream, &state);
                            state.active.fetch_sub(1, Ordering::Relaxed);
                        }));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(40))
                    }
                    Err(_) => break,
                }
                let mut i = 0;
                while i < workers.len() {
                    if workers[i].is_finished() {
                        let _ = workers.swap_remove(i).join();
                    } else {
                        i += 1;
                    }
                }
            }
            for worker in workers {
                let _ = worker.join();
            }
        });
        Ok(Self {
            address,
            token,
            shared,
            stop,
            generation,
            thread: Some(thread),
            count,
        })
    }
    pub fn playlist(&self) -> Arc<str> {
        self.shared.catalog.read().unwrap().playlist.clone()
    }
    /// Atomically replace the visible catalog. Requests already serving a file
    /// keep their Arc<Track>; no socket, session token or cancellation is changed.
    pub fn update(&mut self, tracks: Vec<Track>, aliases: &[(String, String)]) -> bool {
        let mut catalog = self.shared.catalog.write().unwrap();
        let changed = catalog.update(tracks, aliases, self.address, &self.token);
        self.count = catalog.tracks.len();
        changed
    }
    pub fn playlist_url(&self) -> String {
        format!("http://{}/{}/library.m3u", self.address, self.token)
    }
    #[cfg(test)]
    pub fn shared_track_for_test(&self, id: usize) -> Track {
        self.shared.catalog.read().unwrap().tracks[&id]
            .as_ref()
            .clone()
    }
    #[cfg(test)]
    pub fn track_url(&self, index: usize) -> String {
        format!("http://{}/{}/track/{index}", self.address, self.token)
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.generation.store(0, Ordering::Relaxed);
        // An in-flight remote fetch can outlive the UI for its bounded network timeout.
        // Do not block the audio/UI thread while it cancels.
        self.thread.take();
    }
}
fn private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => v.is_private() || v.is_loopback(),
        IpAddr::V6(v) => v.is_loopback() || v.is_unique_local(),
    }
}
fn equal(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0, |sum, (a, b)| sum | (a ^ b))
            == 0
}
fn serve(stream: &mut TcpStream, s: &Shared) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut request = Vec::new();
    let mut byte = [0_u8];
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while request.len() < 16 * 1024
        && std::time::Instant::now() < deadline
        && stream.read(&mut byte)? == 1
    {
        request.push(byte[0]);
        if request.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    if !request.ends_with(b"\r\n\r\n") {
        return reply(stream, 400, &[], b"Invalid request", false);
    }
    let request = String::from_utf8_lossy(&request);
    let mut lines = request.split("\r\n");
    let first = lines
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>();
    if first.len() != 3 || !matches!(first[0], "GET" | "HEAD") {
        return reply(
            stream,
            405,
            &[("Allow", "GET, HEAD".into())],
            b"Method not allowed",
            false,
        );
    }
    let head = first[0] == "HEAD";
    let path = first[1].split('?').next().unwrap_or_default();
    let parts: Vec<_> = path.split('/').collect();
    if parts.len() < 3 || !parts[0].is_empty() || !equal(parts[1], &s.token) {
        return reply(stream, 404, &[], b"Not found", head);
    }
    if parts.as_slice().get(2) == Some(&"library.m3u") && parts.len() == 3 {
        let playlist = s.catalog.read().unwrap().playlist.clone();
        return reply(
            stream,
            200,
            &[("Content-Type", "audio/x-mpegurl; charset=utf-8".into())],
            playlist.as_bytes(),
            head,
        );
    }
    if parts.len() != 4 || parts[2] != "track" {
        return reply(stream, 404, &[], b"Not found", head);
    }
    let Some(track) = parts[3]
        .split('.')
        .next()
        .unwrap_or_default()
        .parse::<usize>()
        .ok()
        .and_then(|i| s.catalog.read().unwrap().tracks.get(&i).cloned())
    else {
        return reply(stream, 404, &[], b"Not found", head);
    };
    if s.stop.load(Ordering::Relaxed) {
        return reply(stream, 503, &[], b"Stopped", head);
    }
    let path = match cache::materialize(&track.source, &s.root, &s.generation, 1, &s.auth, |_| {}) {
        Ok(p) => p,
        Err(_) => return reply(stream, 503, &[], b"Source unavailable", head),
    };
    let mut file = match File::open(&path) {
        Ok(f) => f,
        Err(_) => return reply(stream, 503, &[], b"Source unavailable", head),
    };
    let size = file.metadata()?.len();
    let ranges: Vec<_> = lines
        .filter_map(|line| line.split_once(':'))
        .filter(|(key, _)| key.eq_ignore_ascii_case("range"))
        .collect();
    let span = if ranges.is_empty() {
        Some((0, size.saturating_sub(1)))
    } else if ranges.len() == 1 {
        range(ranges[0].1.trim(), size)
    } else {
        None
    };
    let Some((start, end)) = span.filter(|_| size > 0) else {
        return reply(
            stream,
            416,
            &[("Content-Range", format!("bytes */{size}"))],
            b"Invalid range",
            head,
        );
    };
    let length = end - start + 1;
    let content = match track.codec.to_ascii_lowercase().as_str() {
        "flac" => "audio/flac",
        "wav" => "audio/wav",
        "mp3" => "audio/mpeg",
        "m4a" | "mp4" | "alac" => "audio/mp4",
        "aac" => "audio/aac",
        "aif" | "aiff" => "audio/aiff",
        "ogg" | "oga" => "audio/ogg",
        _ => "application/octet-stream",
    };
    let mut headers = vec![
        ("Content-Type", content.into()),
        ("Content-Length", length.to_string()),
        ("Accept-Ranges", "bytes".into()),
    ];
    if !ranges.is_empty() {
        headers.push(("Content-Range", format!("bytes {start}-{end}/{size}")));
    }
    write_header(stream, if ranges.is_empty() { 200 } else { 206 }, &headers)?;
    if head {
        return Ok(());
    }
    file.seek(SeekFrom::Start(start))?;
    let mut remaining = length;
    let mut buffer = [0_u8; 64 * 1024];
    while remaining > 0 && !s.stop.load(Ordering::Relaxed) {
        let count = file.read(&mut buffer[..remaining.min(64 * 1024) as usize])?;
        if count == 0 {
            break;
        }
        stream.write_all(&buffer[..count])?;
        remaining -= count as u64;
    }
    Ok(())
}
fn extension(track: &Track) -> &str {
    match track.codec.as_str() {
        "FLAC" => "flac",
        "MP3" => "mp3",
        "M4A" | "MP4" | "ALAC" => "m4a",
        "AAC" => "aac",
        "AIFF" | "AIF" => "aiff",
        "OGG" | "OGA" => "ogg",
        "WAV" => "wav",
        _ => "bin",
    }
}
fn range(header: &str, size: u64) -> Option<(u64, u64)> {
    if size == 0 {
        return None;
    }
    let value = header.strip_prefix("bytes=")?;
    if value.contains(',') {
        return None;
    }
    let (a, b) = value.split_once('-')?;
    if a.is_empty() {
        let suffix = b.parse::<u64>().ok()?.min(size);
        return (suffix > 0).then_some((size - suffix, size - 1));
    }
    let start = a.parse::<u64>().ok()?;
    let end = if b.is_empty() {
        size - 1
    } else {
        b.parse::<u64>().ok()?.min(size - 1)
    };
    (start < size && start <= end).then_some((start, end))
}
fn write_header(
    stream: &mut TcpStream,
    status: u16,
    headers: &[(&str, String)],
) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        206 => "Partial Content",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        416 => "Range Not Satisfiable",
        _ => "Service Unavailable",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nConnection: close\r\nCache-Control: private, no-store\r\nX-Content-Type-Options: nosniff\r\n"
    )?;
    for (key, value) in headers {
        write!(stream, "{key}: {value}\r\n")?;
    }
    write!(stream, "\r\n")
}
fn reply(
    stream: &mut TcpStream,
    status: u16,
    headers: &[(&str, String)],
    body: &[u8],
    head: bool,
) -> std::io::Result<()> {
    let mut headers = headers.to_vec();
    headers.push(("Content-Length", body.len().to_string()));
    write_header(stream, status, &headers)?;
    if !head {
        stream.write_all(body)?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_ids_survive_reordering_removal_and_phone_relocation() {
        let address = "127.0.0.1:9042".parse().unwrap();
        let a = Track::placeholder("ftp://old.local/Music/a.flac".into());
        let b = Track::placeholder("ftp://old.local/Music/b.flac".into());
        let c = Track::placeholder("ftp://old.local/Music/c.flac".into());
        let mut catalog = Catalog::default();
        assert!(catalog.update(vec![a.clone(), b.clone()], &[], address, "token"));
        assert!(!catalog.update(vec![a.clone(), b.clone()], &[], address, "token"));
        assert!(catalog.update(vec![c, b.clone(), a.clone()], &[], address, "token"));
        assert_eq!(catalog.ids[&a.source], 0);
        assert_eq!(catalog.ids[&b.source], 1);
        assert_eq!(catalog.tracks[&2].title, "c");
        let aliases = vec![(
            "ftp://old.local/Music/".into(),
            "ftp://new.local/Music/".into(),
        )];
        let mut relocated = a;
        relocated.source = "ftp://new.local/Music/a.flac".into();
        assert!(catalog.update(vec![relocated.clone()], &aliases, address, "token"));
        assert_eq!(catalog.ids[&relocated.source], 0);
        assert_eq!(catalog.tracks.len(), 1);
        assert!(!catalog.tracks.contains_key(&1));
        assert!(catalog.playlist.contains("/track/0.flac"));
        assert!(catalog.update(vec![relocated, b.clone()], &[], address, "token"));
        assert!(
            catalog.ids[&b.source] > 2,
            "retired URLs must never refer to a different track"
        );
    }
    #[test]
    #[ignore = "explicit live HTTP catalog replacement on loopback"]
    fn updates_keep_urls_and_inflight_original_bytes_without_rebinding() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("before.wav");
        let bytes: Vec<_> = (0..8 * 1024 * 1024).map(|n| (n % 251) as u8).collect();
        std::fs::write(&path, &bytes).unwrap();
        let a = Track::placeholder(path.to_string_lossy().into_owned());
        let b = Track::placeholder(temp.path().join("new.wav").to_string_lossy().into_owned());
        std::fs::write(&b.source, b"new original bytes").unwrap();
        let mut server = Server::start(
            "127.0.0.1:0".parse().unwrap(),
            vec![a.clone()],
            temp.path().into(),
            AuthStore::default(),
        )
        .unwrap();
        let address = server.address;
        let playlist_url = server.playlist_url();
        let track_url = server.track_url(0);
        let mut stream = TcpStream::connect(address).unwrap();
        write!(
            stream,
            "GET {} HTTP/1.1\r\nHost: localhost\r\n\r\n",
            url::Url::parse(&track_url).unwrap().path()
        )
        .unwrap();
        let mut header = Vec::new();
        let mut byte = [0];
        while !header.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).unwrap();
            header.push(byte[0]);
        }
        assert!(header.starts_with(b"HTTP/1.1 200"));
        assert!(server.update(vec![b, a], &[]));
        assert_eq!(server.address, address);
        assert_eq!(server.playlist_url(), playlist_url);
        let mut body = Vec::new();
        stream.read_to_end(&mut body).unwrap();
        assert_eq!(body, bytes, "in-flight transfer retains original file");
        let fetch = |url: &str| {
            let mut body = Vec::new();
            let mut easy = curl::easy::Easy::new();
            easy.url(url).unwrap();
            {
                let mut tx = easy.transfer();
                tx.write_function(|b| {
                    body.extend_from_slice(b);
                    Ok(b.len())
                })
                .unwrap();
                tx.perform().unwrap();
            }
            body
        };
        assert_eq!(fetch(&track_url), bytes);
        assert_eq!(fetch(&server.track_url(1)), b"new original bytes");
        let list = String::from_utf8(fetch(&playlist_url)).unwrap();
        assert!(list.find("/track/1.").unwrap() < list.find("/track/0.").unwrap());
        assert_eq!(server.count, 2);
    }
    #[test]
    fn range_limits_and_lan_validation() {
        assert_eq!(range("bytes=5-9", 100), Some((5, 9)));
        assert_eq!(range("bytes=-5", 100), Some((95, 99)));
        assert_eq!(range("bytes=99-", 100), Some((99, 99)));
        assert_eq!(range("bytes=100-", 100), None);
        assert_eq!(range("bytes=1-2,4-5", 100), None);
        assert_eq!(range("bytes=-0", 100), None);
        assert!(!private("0.0.0.0".parse().unwrap()));
        assert!(!private("8.8.8.8".parse().unwrap()));
    }
    #[test]
    #[ignore = "binds an explicit loopback fixture listener"]
    fn loopback_server_preserves_bytes_ranges_and_rejects_unpublished_paths() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("fixture.flac");
        // Exceed Darwin's socket send buffer to catch inherited O_NONBLOCK.
        let bytes: Vec<_> = (0..2 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
        std::fs::write(&path, &bytes).unwrap();
        let server = Server::start(
            "127.0.0.1:0".parse().unwrap(),
            vec![Track::placeholder(path.to_string_lossy().into_owned())],
            temp.path().into(),
            AuthStore::default(),
        )
        .unwrap();
        let fetch = |path: &str, headers: &str| {
            let mut s = TcpStream::connect(server.address).unwrap();
            write!(s, "GET {path} HTTP/1.1\r\nHost: localhost\r\n{headers}\r\n").unwrap();
            let mut b = Vec::new();
            s.read_to_end(&mut b).unwrap();
            b
        };
        let url = url::Url::parse(&server.track_url(0)).unwrap();
        let full = fetch(url.path(), "");
        let split = full.windows(4).position(|b| b == b"\r\n\r\n").unwrap() + 4;
        assert_eq!(&full[split..], bytes);
        let partial = fetch(url.path(), "Range: bytes=12-31\r\n");
        let split = partial.windows(4).position(|b| b == b"\r\n\r\n").unwrap() + 4;
        assert_eq!(&partial[split..], &bytes[12..32]);
        assert!(partial.starts_with(b"HTTP/1.1 206"));
        assert!(fetch("/wrong/track/0", "").starts_with(b"HTTP/1.1 404"));
        assert!(
            fetch(&format!("/{}/track/../state.ron", server.token), "")
                .starts_with(b"HTTP/1.1 404")
        );
        assert!(fetch(url.path(), "Range: bytes=999999999-\r\n").starts_with(b"HTTP/1.1 416"));
        let playlist = fetch(url::Url::parse(&server.playlist_url()).unwrap().path(), "");
        assert!(String::from_utf8_lossy(&playlist).contains("#EXTM3U"));
    }
}
