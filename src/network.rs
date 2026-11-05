use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    io::Write,
    process::{Command, Stdio},
    sync::{Arc, Mutex},
};
use url::Url;

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct FtpSource {
    #[serde(default = "default_scheme")]
    pub scheme: String,
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: String,
    pub root: String,
    #[serde(default)]
    pub rediscover: Option<bool>,
    #[serde(default)]
    pub recent_hosts: Vec<String>,
    #[serde(default)]
    pub hardware_ids: Vec<String>,
    pub username: String,
    #[serde(skip)]
    pub password: String,
}
fn default_scheme() -> String {
    "ftp".into()
}
impl FtpSource {
    pub fn smartphone() -> Self {
        Self {
            scheme: "ftp".into(),
            id: format!("{:016x}", rand::random::<u64>()),
            name: "Smartphone".into(),
            rediscover: Some(true),
            port: "2121".into(),
            root: "/device/Music/".into(),
            ..Default::default()
        }
    }
    pub fn discovery_enabled(&self) -> bool {
        self.rediscover.unwrap_or_else(|| {
            self.name.to_ascii_lowercase().contains("smartphone")
                || self.root.starts_with("/device/")
        })
    }
    pub fn url(&self) -> Result<String, String> {
        let port = self
            .port
            .parse::<u16>()
            .ok()
            .filter(|&p| p > 0)
            .ok_or("Porta: usa un numero da 1 a 65535")?;
        if self.host.contains(['/', '@', ':', ' ', '\n', '\r']) || self.host.is_empty() {
            return Err("Inserisci IP o nome del server FTP".into());
        }
        if self.username.contains(['\n', '\r', ':']) || self.password.contains(['\n', '\r']) {
            return Err("Credenziali non valide".into());
        }
        let scheme = if self.scheme.is_empty() {
            "ftp"
        } else {
            &self.scheme
        };
        if !matches!(scheme, "ftp" | "ftps") {
            return Err("Protocollo FTP/FTPS richiesto".into());
        }
        let mut u = Url::parse(&format!("{scheme}://{}:{port}/", self.host))
            .map_err(|_| "Server FTP non valido")?;
        let root = self.root.trim_matches('/');
        u.set_path(&if root.is_empty() {
            "/".to_string()
        } else {
            format!("/{root}/")
        });
        Ok(u.into())
    }
    pub fn set_url(&mut self, input: &str) -> Result<(), String> {
        let parsed = Url::parse(input.trim()).map_err(|_| "Percorso di rete non valido")?;
        if !matches!(parsed.scheme(), "ftp" | "ftps")
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err(
                "Usa ftp://host:porta/cartella/ e inserisci le credenziali nei campi separati"
                    .into(),
            );
        }
        let mut changed = self.clone();
        changed.host = parsed.host_str().ok_or("Host mancante")?.into();
        changed.port = parsed
            .port()
            .unwrap_or(if parsed.scheme() == "ftps" { 990 } else { 21 })
            .to_string();
        changed.scheme = parsed.scheme().into();
        changed.root = percent_encoding::percent_decode_str(parsed.path())
            .decode_utf8()
            .map_err(|_| "Percorso non UTF-8")?
            .into_owned();
        changed.url()?;
        *self = changed;
        Ok(())
    }
}
#[derive(Clone, Default)]
pub struct AuthStore(Arc<Mutex<HashMap<String, (String, String)>>>);
impl AuthStore {
    pub fn insert(&self, source: &FtpSource) -> Result<(), String> {
        let url = Url::parse(&source.url()?).map_err(|_| "URL non valido")?;
        if let Ok(mut map) = self.0.lock() {
            map.insert(
                authority(&url),
                (source.username.clone(), source.password.clone()),
            );
        }
        Ok(())
    }
    pub(crate) fn get(&self, source: &str) -> Option<(String, String)> {
        let u = Url::parse(source).ok()?;
        self.0.lock().ok()?.get(&authority(&u)).cloned()
    }
}
fn authority(u: &Url) -> String {
    format!(
        "{}://{}:{}",
        u.scheme(),
        u.host_str().unwrap_or(""),
        u.port_or_known_default().unwrap_or(0)
    )
}

/// A unique filename prevents overwriting another client's cover; rename only after upload.
pub fn save_album_cover(
    source: &str,
    bytes: &[u8],
    auth: &AuthStore,
    cancelled: &dyn Fn() -> bool,
) -> Result<String, String> {
    use std::{
        io::Write,
        os::unix::fs::OpenOptionsExt,
        process::{Command, Stdio},
        time::Duration,
    };
    let root = Url::parse(source)
        .ok()
        .and_then(|u| u.join(".").ok())
        .ok_or("Cartella album non disponibile")?;
    if !matches!(root.scheme(), "ftp" | "ftps")
        || !root.username().is_empty()
        || root.password().is_some()
    {
        return Err("Salvataggio disponibile per cartelle locali, NAS e FTP".into());
    }
    if cancelled() {
        return Err("Salvataggio annullato".into());
    }
    let id = rand::random::<u128>();
    let filename = format!("cover.MusicCraft-{id:032x}.png");
    let temporary = format!(".musiccraft-{id:032x}.part");
    let target =
        child_url(root.as_str(), &temporary, false).ok_or("Cartella album non disponibile")?;
    let local = std::env::temp_dir().join(format!("musiccraft-cover-{id:032x}.png"));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&local)
        .map_err(|_| "File temporaneo non disponibile")?;
    let result = (|| {
        file.write_all(bytes)
            .map_err(|_| "Copertina non salvabile")?;
        let mut command = Command::new("/usr/bin/curl");
        command
            .args([
                "--config",
                "-",
                "--silent",
                "--fail",
                "--connect-timeout",
                "3",
                "--max-time",
                "30",
                "--proto",
                "=ftp,ftps",
                "--upload-file",
            ])
            .arg(&local)
            .args([
                "--quote",
                &format!("-RNFR {temporary}"),
                "--quote",
                &format!("-RNTO {filename}"),
                "--url",
                &target,
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = authenticated_spawn(&mut command, auth, source)?;
        loop {
            if cancelled() {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Salvataggio annullato".into());
            }
            match child.try_wait() {
                Ok(Some(status)) => {
                    return if status.success() {
                        Ok(filename)
                    } else {
                        Err(
                            "Impossibile salvare la copertina su FTP; conservata nella cache"
                                .into(),
                        )
                    };
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(80)),
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("Salvataggio FTP interrotto".into());
                }
            }
        }
    })();
    drop(file);
    let _ = std::fs::remove_file(local);
    result
}

/// Authentication goes through an anonymous stdin pipe, never argv, files or logs.
pub fn authenticated_spawn(
    command: &mut Command,
    auth: &AuthStore,
    url: &str,
) -> Result<std::process::Child, String> {
    // These URLs identify one literal file. curl's default URL globbing treats
    // album names such as [2003 Remaster] as ranges and fails before connecting.
    command.arg("--globoff").stdin(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|_| "Trasferimento di rete non disponibile".to_string())?;
    if let Some((user, pass)) = auth.get(url)
        && !user.is_empty()
    {
        let escape = |s: String| s.replace('\\', "\\\\").replace('"', "\\\"");
        let config = format!("user = \"{}:{}\"\n", escape(user), escape(pass));
        if child
            .stdin
            .take()
            .is_none_or(|mut stdin| stdin.write_all(config.as_bytes()).is_err())
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Autenticazione non inviata".into());
        }
    } else {
        child.stdin.take();
    }
    Ok(child)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub name: String,
    pub directory: bool,
    pub modified: Option<String>,
}

pub fn parse_mlsd(text: &str) -> Vec<Entry> {
    text.lines()
        .filter_map(|line| {
            let (facts, name) = line.trim_end_matches('\r').split_once(' ')?;
            // MLSD has exactly one separator; further spaces belong to the filename.
            let kind = facts.split(';').find_map(|f| {
                f.to_ascii_lowercase()
                    .strip_prefix("type=")
                    .map(str::to_owned)
            })?;
            if name.is_empty()
                || !matches!(kind.as_str(), "file" | "dir")
                || matches!(name, "." | "..")
                || name.contains('/')
            {
                return None;
            }
            Some(Entry {
                name: name.into(),
                directory: kind == "dir",
                modified: facts.split(';').find_map(|f| {
                    let (k, v) = f.split_once('=')?;
                    (k.eq_ignore_ascii_case("modify") && !v.is_empty()).then(|| v.to_owned())
                }),
            })
        })
        .collect()
}
fn parse_list(text: &str) -> Vec<Entry> {
    text.lines()
        .filter_map(|line| {
            if !line.starts_with(['d', '-']) {
                return None;
            }
            let mut rest = line;
            for _ in 0..8 {
                rest = rest.trim_start();
                let index = rest.find(char::is_whitespace)?;
                rest = rest.get(index..)?;
            }
            let name = rest
                .strip_prefix(' ')
                .or_else(|| rest.strip_prefix('\t'))?
                .trim_end_matches('\r');
            if name.is_empty() || matches!(name, "." | "..") || name.contains('/') {
                return None;
            }
            Some(Entry {
                name: name.into(),
                directory: line.starts_with('d'),
                modified: None,
            })
        })
        .collect()
}

/// One handle per sweep reuses the authenticated control connection across directories.
/// Credentials stay in memory; verbose/debug output is never enabled.
pub struct ListingSession {
    handle: curl::easy::Easy,
    mlsd: bool,
    timeout: std::time::Duration,
}
impl ListingSession {
    pub fn new() -> Self {
        Self {
            handle: curl::easy::Easy::new(),
            mlsd: true,
            timeout: std::time::Duration::from_secs(10),
        }
    }
    pub fn with_timeout(timeout: std::time::Duration) -> Self {
        Self {
            timeout,
            ..Self::new()
        }
    }
    pub fn set_timeout(&mut self, timeout: std::time::Duration) {
        self.timeout = timeout;
    }
    fn raw(
        &mut self,
        target: &str,
        auth: &AuthStore,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Result<String, String> {
        use std::{sync::atomic::Ordering, time::Duration};
        let url = Url::parse(target).map_err(|_| "Indirizzo FTP non valido")?;
        if !matches!(url.scheme(), "ftp" | "ftps")
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err("Indirizzo FTP non valido".into());
        }
        let options = (|| -> Result<(), curl::Error> {
            self.handle.url(target)?;
            self.handle
                .connect_timeout(Duration::from_secs(3).min(self.timeout))?;
            self.handle.timeout(self.timeout)?;
            self.handle.progress(true)?;
            self.handle
                .custom_request(if self.mlsd { "MLSD" } else { "LIST" })?;
            self.handle.follow_location(false)?;
            let (user, password) = auth
                .get(target)
                .unwrap_or_else(|| ("anonymous".into(), "aurea@localhost".into()));
            self.handle
                .username(if user.is_empty() { "anonymous" } else { &user })?;
            self.handle.password(&password)?;
            Ok(())
        })();
        options.map_err(|_| "Configurazione FTP non disponibile")?;
        let mut bytes = Vec::new();
        let result = {
            let mut transfer = self.handle.transfer();
            transfer
                .write_function(|data| {
                    if cancel.load(Ordering::Relaxed) || bytes.len() + data.len() > 4 * 1024 * 1024
                    {
                        return Ok(0);
                    }
                    bytes.extend_from_slice(data);
                    Ok(data.len())
                })
                .map_err(|_| "Lettura FTP non disponibile")?;
            transfer
                .progress_function(|_, _, _, _| !cancel.load(Ordering::Relaxed))
                .map_err(|_| "Controllo FTP non disponibile")?;
            transfer.perform()
        };
        result.map_err(|_| "FTP non raggiungibile: controlla IP, porta, credenziali e percorso")?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
    pub fn list(
        &mut self,
        url: &str,
        auth: &AuthStore,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Result<Vec<Entry>, String> {
        self.list_with_coverage(url, auth, cancel)
            .map(|(entries, _)| entries)
    }
    pub fn list_with_coverage(
        &mut self,
        url: &str,
        auth: &AuthStore,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Result<(Vec<Entry>, bool), String> {
        let text = match self.raw(url, auth, cancel) {
            Ok(text) => text,
            Err(_)
                if self.mlsd
                    && matches!(self.handle.response_code(), Ok(500 | 501 | 502 | 504)) =>
            {
                self.mlsd = false;
                self.raw(url, auth, cancel)?
            }
            Err(error) => return Err(error),
        };
        if self.mlsd {
            let entries = parse_mlsd(&text);
            if entries.is_empty()
                && !text.trim().is_empty()
                && !text
                    .lines()
                    .all(|line| line.to_ascii_lowercase().contains("type="))
            {
                self.mlsd = false;
                return self
                    .raw(url, auth, cancel)
                    .map(|text| coverage(&text, parse_list(&text), false));
            }
            Ok(coverage(&text, entries, true))
        } else {
            Ok(coverage(&text, parse_list(&text), false))
        }
    }
}
fn coverage(text: &str, entries: Vec<Entry>, mlsd: bool) -> (Vec<Entry>, bool) {
    // Parsers deliberately ignore unsupported rows. Such a listing can add
    // tracks but cannot prove that an existing entry disappeared.
    let rows = text
        .lines()
        .filter(|line| {
            let line = line.trim();
            if line.is_empty() {
                return false;
            }
            if mlsd {
                !line.split_once(' ').is_some_and(|(facts, _)| {
                    facts.split(';').any(|fact| {
                        fact.eq_ignore_ascii_case("type=cdir")
                            || fact.eq_ignore_ascii_case("type=pdir")
                    })
                })
            } else {
                !line
                    .strip_prefix("total ")
                    .is_some_and(|n| n.parse::<u64>().is_ok())
            }
        })
        .count();
    let complete = rows == entries.len() && !text.contains('\u{fffd}');
    (entries, complete)
}

pub fn list(url: &str, auth: &AuthStore) -> Result<Vec<Entry>, String> {
    ListingSession::new().list(url, auth, &std::sync::atomic::AtomicBool::new(false))
}
pub fn child_url(parent: &str, name: &str, directory: bool) -> Option<String> {
    let mut url = Url::parse(parent).ok()?;
    {
        let mut parts = url.path_segments_mut().ok()?;
        parts.pop_if_empty().push(name);
        if directory {
            parts.push("");
        }
    }
    Some(url.into())
}
pub fn rebase(source: &str, old: &str, new: &str) -> Option<String> {
    source
        .strip_prefix(old)
        .map(|suffix| format!("{new}{suffix}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn directory_listings_preserve_spaces_that_belong_to_file_names() {
        let rows = parse_mlsd("type=file;size=2;     01.flac\r\n");
        assert_eq!(rows[0].name, "    01.flac");
        let rows = parse_list("-rw-r--r-- 1 user group 2 Jan 01 12:00     01.flac\r\n");
        assert_eq!(rows[0].name, "    01.flac");
    }
    #[test]
    fn ftp_listing_supports_spaces_unicode_and_skips_parent_links() {
        let parsed = parse_mlsd(
            "type=cdir; .\r\ntype=pdir; ..\r\ntype=dir;modify=20260101; Un album\r\ntype=file;size=100; 01 Città.flac\r\ntype=OS.unix=slink; loop\r\n",
        );
        assert_eq!(parsed.len(), 2);
        assert!(parsed[0].directory);
        assert_eq!(parsed[1].name, "01 Città.flac");
        assert_eq!(parse_list("drwxr-xr-x 1 user group 20 Oct 16 12:00 Un album\n-rw-r--r-- 1 user group 20 Oct 16 12:00 01 Città.flac").len(), 2);
        assert!(
            child_url("ftp://nas/Music/", "01 # è.flac", false)
                .unwrap()
                .contains("%23")
        );
    }
    #[test]
    fn configurable_network_root_is_atomic_and_keeps_identity_and_credentials() {
        let mut source = FtpSource::smartphone();
        source.username = "fixture".into();
        source.password = "fixture-only".into();
        let id = source.id.clone();
        source
            .set_url("ftp://phone.local:8736/device/Music%20%C3%A8/")
            .unwrap();
        assert_eq!(source.root, "/device/Music è/");
        assert_eq!(
            source.url().unwrap(),
            "ftp://phone.local:8736/device/Music%20%C3%A8/"
        );
        assert_eq!(source.id, id);
        assert_eq!(source.password, "fixture-only");
        let before = source.url().unwrap();
        for bad in [
            "sftp://phone.local/Music/",
            "ftp://fixture:fixture-only@phone.invalid/Music/",
            "ftp://phone.local:0/Music/",
            "ftp://phone.local/Music/?secret=1",
        ] {
            assert!(source.set_url(bad).is_err());
            assert_eq!(source.url().unwrap(), before);
        }
        source.set_url("ftps://phone.local/").unwrap();
        assert_eq!(source.url().unwrap(), "ftps://phone.local:990/");
        let mut json = serde_json::to_value(&source).unwrap();
        json.as_object_mut().unwrap().remove("scheme");
        assert_eq!(
            serde_json::from_value::<FtpSource>(json).unwrap().scheme,
            "ftp"
        );
    }
    #[test]
    fn source_validation_and_safe_persistence() {
        let mut s = FtpSource::smartphone();
        s.host = "192.168.1.10".into();
        s.username = "user".into();
        s.password = "secret".into();
        assert_eq!(s.url().unwrap(), "ftp://192.168.1.10:2121/device/Music/");
        assert!(!serde_json::to_string(&s).unwrap().contains("secret"));
        s.port = "0".into();
        assert!(s.url().is_err());
        assert_eq!(
            rebase(
                "ftp://old/Music/album/1.flac",
                "ftp://old/Music/",
                "ftp://new/Music/"
            ),
            Some("ftp://new/Music/album/1.flac".into())
        );
        assert_eq!(
            rebase("ftp://old/MusicOther/a", "ftp://old/Music/", "ftp://new/"),
            None
        );
    }
}

#[cfg(test)]
mod integration {
    use super::*;
    #[test]
    #[ignore = "requires scripts/test_ftp.py fixture server"]
    fn live_ftp_literal_brackets_and_braces_download_and_prefetch_exact_files() {
        use crate::{cache, jobs, library::Track};
        use std::sync::{atomic::AtomicU64, mpsc};
        let base = std::env::var("AUREA_TEST_FTP_URL").unwrap();
        let u = Url::parse(&base).unwrap();
        let auth = AuthStore::default();
        auth.insert(&FtpSource {
            host: u.host_str().unwrap().into(),
            port: u.port().unwrap().to_string(),
            root: "/".into(),
            username: "fixture".into(),
            password: "fixture-only".into(),
            name: "Fixture".into(),
            id: "fixture".into(),
            scheme: "ftp".into(),
            ..Default::default()
        })
        .unwrap();
        let fixture_root = std::path::PathBuf::from(std::env::var("AUREA_TEST_FTP_ROOT").unwrap());
        let generation = Arc::new(AtomicU64::new(7));
        for (album, file) in [
            ("Literal [2003 Remaster]", "01 Track [Live].wav"),
            ("Literal [Disc 1]", "02 Track.wav"),
            ("Literal {Mono,Stereo}", "03 Track {one,two}.wav"),
        ] {
            let dir = child_url(&base, album, true).unwrap();
            let source = child_url(&dir, file, false).unwrap();
            let expected = std::fs::read(fixture_root.join(album).join(file)).unwrap();
            let cache_dir = tempfile::tempdir().unwrap();
            let path = cache::materialize(&source, cache_dir.path(), &generation, 7, &auth, |_| {})
                .unwrap_or_else(|e| panic!("literal URL {source}: {e}"));
            assert_eq!(std::fs::read(path).unwrap(), expected);
            // Directory discovery must recognize a legacy-encoded spelling as already indexed.
            let known = std::collections::HashSet::from([crate::library::source_key(&source)]);
            let report = crate::sync::scan(
                crate::sync::Target::Ftp {
                    id: "fixture".into(),
                    root: dir.clone(),
                },
                &auth,
                &known,
                &mut crate::sync::Index::default(),
                true,
                &std::sync::atomic::AtomicBool::new(false),
            );
            assert_eq!(report.stats.errors, 0);
            assert!(
                report.tracks.is_empty(),
                "equivalent FTP spelling must not become a new track"
            );
            // A separate cache forces the exact same transfer through the next-track path.
            let next_cache = tempfile::tempdir().unwrap();
            let (tx, rx) = mpsc::channel();
            jobs::prefetch(
                Track::placeholder(source.clone()),
                next_cache.path().into(),
                auth.clone(),
                generation.clone(),
                7,
                tx,
                jobs::PrepareOptions::default(),
            );
            match rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap() {
                jobs::Job::Ready(track, _, ticket, _) => {
                    assert_eq!(track.source, source);
                    assert_eq!(track.rate, 44100);
                    assert_eq!(ticket, 7);
                }
                _ => panic!("next-track audio must be ready"),
            }
            // Drain the prefetch job before its private cache is removed.
            while rx.recv_timeout(std::time::Duration::from_secs(10)).is_ok() {}
        }
    }
    #[test]
    #[ignore = "requires scripts/test_ftp.py fixture server"]
    fn live_ftp_scan_cache_metadata_and_artwork_prefetch() {
        use crate::{
            audio, cache, jobs,
            library::{self, Track},
        };
        use rodio::Source;
        use std::sync::{
            atomic::{AtomicBool, AtomicU64},
            mpsc,
        };
        let base = std::env::var("AUREA_TEST_FTP_URL").unwrap();
        let u = Url::parse(&base).unwrap();
        let ftp = FtpSource {
            host: u.host_str().unwrap().into(),
            port: u.port().unwrap().to_string(),
            root: "/".into(),
            username: "fixture".into(),
            password: "fixture-only".into(),
            name: "Fixture".into(),
            id: "fixture".into(),
            scheme: "ftp".into(),
            ..Default::default()
        };
        let auth = AuthStore::default();
        auth.insert(&ftp).unwrap();
        let entries = list(&base, &auth).unwrap();
        assert!(
            entries
                .iter()
                .any(|e| e.directory && e.name == "Album Città")
        );
        let dir = child_url(&base, "Album Città", true).unwrap();
        let items = list(&dir, &auth).unwrap();
        assert!(items.iter().any(|e| e.name == "01 Opening.wav"));
        let (tx, rx) = mpsc::channel();
        jobs::ftp_scan(
            base.clone(),
            auth.clone(),
            tx,
            Arc::new(AtomicBool::new(false)),
        );
        let mut tracks = Vec::new();
        loop {
            match rx.recv_timeout(std::time::Duration::from_secs(30)).unwrap() {
                jobs::Job::Tracks(batch) => tracks.extend(batch),
                jobs::Job::Done(errors) => {
                    assert_eq!(errors, 0);
                    break;
                }
                jobs::Job::Problem(e) => panic!("{e}"),
                _ => {}
            }
        }
        assert_eq!(tracks.len(), 4);
        let root = tempfile::tempdir().unwrap();
        let gen_id = Arc::new(AtomicU64::new(7));
        let first = tracks
            .iter()
            .find(|t| t.source.ends_with("Opening.wav"))
            .unwrap()
            .clone();
        let cached =
            cache::materialize(&first.source, root.path(), &gen_id, 7, &auth, |_| {}).unwrap();
        let fixture = std::path::Path::new(&std::env::var("AUREA_TEST_FTP_ROOT").unwrap())
            .join("Album Città/01 Opening.wav");
        assert_eq!(
            std::fs::read(&cached).unwrap(),
            std::fs::read(&fixture).unwrap()
        );
        let mut metadata = first.clone();
        metadata.update_metadata(&cached);
        assert_eq!(metadata.rate, 44100);
        assert_eq!(metadata.bits, 16);
        assert_eq!(metadata.channels, 2);
        let art = jobs::artwork(&cached, &first.source, root.path(), &auth, &gen_id, 7).unwrap();
        assert!(art.size[0] > 0 && art.size[1] > 0);
        let mut synced = tracks.clone();
        synced[0].title = "Existing enriched metadata".into();
        let original_order: Vec<_> = synced.iter().map(|t| t.source.clone()).collect();
        let expected_pcm = crate::integer::DecodedPcm::read(&fixture).unwrap();
        for track in tracks {
            let path =
                cache::materialize(&track.source, root.path(), &gen_id, 7, &auth, |_| {}).unwrap();
            if track.source.ends_with(".mp3") {
                let pcm = crate::integer::DecodedPcm::read_mode(&path, || false, false).unwrap();
                assert!(pcm.floating);
                assert_eq!(
                    pcm.frames(),
                    expected_pcm.frames(),
                    "MP3 encoder delay/padding must be removed"
                );
            }
            if !track.source.ends_with(".mp3") {
                let pcm = crate::integer::DecodedPcm::read(&path).unwrap();
                assert_eq!(pcm.samples, expected_pcm.samples);
                assert_eq!(pcm.rate, expected_pcm.rate);
            }
            let mut prepared_track = track.clone();
            let mut progressive = crate::streaming::Stream::prepare(
                &mut prepared_track,
                root.path(),
                &auth,
                crate::streaming::Cancellation::new(
                    std::sync::Arc::new(AtomicU64::new(7)),
                    7,
                    None,
                ),
                !track.source.ends_with(".mp3"),
                None,
            )
            .unwrap();
            let mut progressive_pcm = Vec::new();
            loop {
                match progressive.pull() {
                    crate::streaming::Pull::Frame(frame) => {
                        progressive_pcm.extend_from_slice(&frame[..2])
                    }
                    crate::streaming::Pull::Pending => std::thread::yield_now(),
                    crate::streaming::Pull::End => break,
                    crate::streaming::Pull::Failed => panic!("progressive decoder failed"),
                }
            }
            if track.source.ends_with(".mp3") {
                assert_eq!(
                    progressive_pcm.len(),
                    expected_pcm.samples.len(),
                    "progressive MP3 delay/padding"
                );
            } else {
                assert_eq!(
                    progressive_pcm, expected_pcm.samples,
                    "progressive lossless PCM must match source exactly"
                );
            }
            let decoder = rodio::Decoder::try_from(std::fs::File::open(&path).unwrap()).unwrap();
            assert_eq!(decoder.channels().get(), 2);
            assert!(decoder.take(2048).any(|s| s.abs() > 0.001));
        }
        // Prefetch completes while the preceding track is still considered current.
        let next = Track::placeholder(child_url(&dir, "02 Interlude.flac", false).unwrap());
        let (tx, rx) = mpsc::channel();
        jobs::prefetch(
            next,
            root.path().into(),
            auth.clone(),
            gen_id,
            7,
            tx,
            jobs::PrepareOptions {
                exclusive: Some(true),
                ..Default::default()
            },
        );
        match rx.recv_timeout(std::time::Duration::from_secs(20)).unwrap() {
            jobs::Job::Ready(_, pcm, _, _) => assert!(pcm.is_some()),
            _ => panic!("audio must be prepared before artwork"),
        };
        match rx.recv_timeout(std::time::Duration::from_secs(20)).unwrap() {
            jobs::Job::Prepared(track, cover, _) => {
                assert!(cover.is_some());
                assert_eq!(track.rate, 44100);
            }
            _ => panic!("prefetch result missing"),
        }
        // A newly copied phone track appears on refresh. Existing order and tags
        // survive repeated scans; network clients still have read-only permissions.
        std::fs::copy(&fixture, fixture.parent().unwrap().join("05 New song.wav")).unwrap();
        for expected_added in [1, 0] {
            let (tx, rx) = mpsc::channel();
            jobs::ftp_scan(
                base.clone(),
                auth.clone(),
                tx,
                Arc::new(AtomicBool::new(false)),
            );
            let mut added = 0;
            loop {
                match rx.recv_timeout(std::time::Duration::from_secs(20)).unwrap() {
                    jobs::Job::Tracks(batch) => added += library::merge_tracks(&mut synced, batch),
                    jobs::Job::Done(errors) => {
                        assert_eq!(errors, 0);
                        break;
                    }
                    jobs::Job::Problem(e) => panic!("{e}"),
                    _ => {}
                }
            }
            assert_eq!(added, expected_added);
            assert_eq!(synced.len(), 5);
            assert_eq!(
                synced[..4]
                    .iter()
                    .map(|t| t.source.clone())
                    .collect::<Vec<_>>(),
                original_order
            );
            assert_eq!(synced[0].title, "Existing enriched metadata");
        }
        // Bad credentials fail without persisting a partial audio file.
        let mut wrong = ftp;
        wrong.password = "wrong".into();
        let bad = AuthStore::default();
        bad.insert(&wrong).unwrap();
        assert!(list(&base, &bad).is_err());
        let _ = audio::devices;
    }
}
