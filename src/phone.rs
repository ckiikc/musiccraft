//! MusicCraft Android protocol v1. Pairing credentials are never part of media URLs.
mod address;
mod pairing_code;
use crate::{
    library::{self, Track},
    network::{AuthStore, FtpSource},
};
pub use pairing_code::{Nearby, nearby, pair};
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use url::Url;

pub fn valid_pin(pin: &str) -> bool {
    pin.len() == 44
        && pin.ends_with('=')
        && pin
            .bytes()
            .take(43)
            .all(|c| c.is_ascii_alphanumeric() || c == b'+' || c == b'/')
}
fn valid_id(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(i, c)| {
            if [8, 13, 18, 23].contains(&i) {
                c == b'-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}
pub fn pairing(link: &str) -> Result<FtpSource, String> {
    let u = Url::parse(link.trim()).map_err(|_| "Invalid MusicCraft pairing link")?;
    if u.scheme() != "musiccraft"
        || u.host_str() != Some("pair")
        || !u.path().is_empty()
        || u.fragment().is_some()
        || !u.username().is_empty()
        || u.password().is_some()
        || u.port().is_some()
    {
        return Err("Invalid MusicCraft pairing link".into());
    }
    let mut q = HashMap::new();
    for (k, v) in u.query_pairs() {
        if q.insert(k.into_owned(), v.into_owned()).is_some() {
            return Err("Duplicate pairing field".into());
        }
    }
    let get = |k: &str| {
        q.get(k)
            .map(String::as_str)
            .ok_or("Incomplete pairing link")
    };
    if get("v")? != "1" || q.len() != 6 {
        return Err("Unsupported pairing version".into());
    }
    let host = get("host")?;
    if !host
        .parse::<std::net::Ipv4Addr>()
        .is_ok_and(|a| a.is_private() || a.is_loopback())
    {
        return Err("Pairing requires a local network address".into());
    }
    let id = get("id")?;
    let pin = get("pin")?;
    let token = get("token")?;
    if !valid_id(id)
        || !valid_pin(pin)
        || token.len() != 43
        || !token
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return Err("Invalid pairing identity".into());
    }
    let source = FtpSource {
        scheme: "https".into(),
        id: format!("phone:{id}"),
        name: "MusicCraft Android".into(),
        host: host.into(),
        port: get("port")?.into(),
        root: "/v1/audio/".into(),
        rediscover: Some(true),
        username: "musiccraft".into(),
        password: token.into(),
        tls_pin: Some(pin.into()),
        ..Default::default()
    };
    source.url()?;
    Ok(source)
}
#[derive(Deserialize)]
struct Catalog {
    protocol: u32,
    device_id: String,
    tracks: Vec<Item>,
}
#[derive(Deserialize)]
struct Item {
    id: String,
    filename: String,
    title: String,
    artist: String,
    album: String,
    duration: f64,
    number: u32,
    disc: u32,
    bytes: u64,
}
fn decode(bytes: &[u8], root: &str, id: &str) -> Result<Vec<Track>, String> {
    let c: Catalog = serde_json::from_slice(bytes).map_err(|_| "Invalid Android catalogue")?;
    if c.protocol != 1
        || format!("phone:{}", c.device_id) != id
        || c.tracks.len() > library::MAX_TRACKS
    {
        return Err("Android catalogue identity or limit mismatch".into());
    }
    let root = Url::parse(root).map_err(|_| "Invalid catalogue address")?;
    if root.path() != "/v1/audio/" {
        return Err("Invalid catalogue root".into());
    }
    let mut seen = HashSet::new();
    let mut tracks = Vec::with_capacity(c.tracks.len());
    for item in c.tracks {
        let ext = item
            .filename
            .strip_prefix(&format!("{}.", item.id))
            .ok_or("Invalid catalogue filename")?;
        if item.id.is_empty()
            || !item.id.bytes().all(|c| c.is_ascii_digit())
            || !matches!(
                ext,
                "flac"
                    | "wav"
                    | "aif"
                    | "aiff"
                    | "m4a"
                    | "alac"
                    | "mp3"
                    | "aac"
                    | "ogg"
                    | "oga"
                    | "mp4"
            )
            || !seen.insert(item.id)
            || item.bytes == 0
            || !item.duration.is_finite()
            || item.duration < 0.0
            || [&item.title, &item.artist, &item.album]
                .iter()
                .any(|s| s.len() > 4096)
        {
            return Err("Invalid or duplicate Android track".into());
        }
        let mut track = Track::placeholder(
            root.join(&item.filename)
                .map_err(|_| "Invalid track address")?
                .into(),
        );
        track.title = item.title;
        track.artist = item.artist;
        track.album = item.album;
        track.duration = item.duration;
        track.number = item.number;
        track.disc = item.disc;
        track.metadata_loaded = true;
        tracks.push(track);
    }
    Ok(tracks)
}
fn request(
    root: &str,
    path: &str,
    auth: &AuthStore,
    cancel: &AtomicBool,
    timeout: Duration,
) -> Result<(u32, Vec<u8>), String> {
    auth.peer(root)
        .ok_or("Pair this Android device before connecting")?;
    let mut url = Url::parse(root).map_err(|_| "Invalid Android address")?;
    url.set_path(path);
    let mut easy = curl::easy::Easy::new();
    let setup = (|| -> Result<(), curl::Error> {
        easy.url(url.as_str())?;
        easy.connect_timeout(Duration::from_secs(3).min(timeout))?;
        easy.timeout(timeout)?;
        easy.progress(true)?;
        easy.signal(false)?;
        auth.configure(&mut easy, root)?;
        Ok(())
    })();
    setup.map_err(|_| "Unable to configure Android connection")?;
    let mut bytes = Vec::new();
    let result = {
        let mut transfer = easy.transfer();
        transfer
            .write_function(|data| {
                if cancel.load(Ordering::Relaxed) || bytes.len() + data.len() > 16 * 1024 * 1024 {
                    return Ok(0);
                }
                bytes.extend_from_slice(data);
                Ok(data.len())
            })
            .map_err(|_| "Unable to read Android catalogue")?;
        transfer
            .progress_function(|_, _, _, _| !cancel.load(Ordering::Relaxed))
            .map_err(|_| "Unable to cancel Android connection")?;
        transfer.perform()
    };
    result.map_err(|_| "MusicCraft Android unavailable: check sharing, Wi-Fi and pairing")?;
    Ok((easy.response_code().unwrap_or(0), bytes))
}
pub fn online(
    root: &str,
    auth: &AuthStore,
    cancel: &AtomicBool,
    timeout: Duration,
) -> Result<bool, String> {
    let (code, bytes) = request(root, "/v1/status", auth, cancel, timeout)?;
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| "Invalid Android status")?;
    let (_, id) = auth.peer(root).ok_or("Pairing unavailable")?;
    if code != 200
        || value["protocol"] != 1
        || value["device_id"].as_str().map(|s| format!("phone:{s}")) != Some(id)
    {
        return Err("Android identity mismatch".into());
    }
    value["sharing"]
        .as_bool()
        .ok_or("Invalid sharing state".into())
}
pub fn catalog(
    root: &str,
    auth: &AuthStore,
    cancel: &AtomicBool,
    timeout: Duration,
) -> Result<Vec<Track>, String> {
    let (_, id) = auth
        .peer(root)
        .ok_or("Pair this Android device before connecting")?;
    let (code, bytes) = request(root, "/v1/catalog", auth, cancel, timeout)?;
    match code {
        200 => decode(&bytes, root, &id),
        503 => Err("Sharing suspended: pause audio on the phone and retry".into()),
        401 => Err("Pairing revoked: copy a new pairing link from the phone".into()),
        _ => Err("Android catalogue request refused".into()),
    }
}
/// Bonjour supplies only an address. The saved TLS pin and credential still authenticate it.
pub fn rediscover(source: &FtpSource, cancel: &AtomicBool) -> Option<FtpSource> {
    let id = source.id.strip_prefix("phone:")?;
    if !valid_id(id) {
        return None;
    }
    let auth = AuthStore::default();
    auth.insert(source).ok()?;
    #[cfg(not(target_os = "macos"))]
    if online(&source.url().ok()?, &auth, cancel, Duration::from_secs(2)).is_ok() {
        return Some(source.clone());
    }
    // Migrate legacy Bonjour names to a numeric endpoint only after the same
    // saved TLS pin, device identity and credential authenticate it.
    for ip in address::local_ipv4(&source.host, cancel) {
        let mut candidate = source.clone();
        candidate.host = ip.to_string();
        let check = AuthStore::default();
        if check.insert(&candidate).is_ok()
            && online(
                &candidate.url().ok()?,
                &check,
                cancel,
                Duration::from_secs(2),
            )
            .is_ok()
        {
            return Some(candidate);
        }
    }
    let prefix = format!("MusicCraft-{}", &id[..8]);
    for peer in nearby(cancel)
        .into_iter()
        .filter(|peer| peer.name.starts_with(&prefix))
    {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let mut updated = source.clone();
        updated.host = peer.host;
        updated.port = peer.port.to_string();
        let auth = AuthStore::default();
        if auth.insert(&updated).is_ok()
            && online(&updated.url().ok()?, &auth, cancel, Duration::from_secs(3)).is_ok()
        {
            return Some(updated);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    const ID: &str = "01234567-89ab-cdef-0123-456789abcdef";
    fn link() -> String {
        format!(
            "musiccraft://pair?v=1&host=192.168.1.2&port=8737&id={ID}&pin={}=&token={}",
            "A".repeat(43),
            "B".repeat(43)
        )
    }
    #[test]
    fn pairing_rejects_ambiguous_or_external_identity() {
        let source = pairing(&link()).unwrap();
        assert!(
            source
                .url()
                .unwrap()
                .starts_with("https://192.168.1.2:8737")
        );
        assert!(
            serde_json::to_string(&source)
                .unwrap()
                .find(&"B".repeat(43))
                .is_none()
        );
        for bad in [
            link() + "&host=192.168.1.3",
            link().replace("192.168.1.2", "8.8.8.8"),
            link().replace("v=1", "v=2"),
            link().replace("pin=", "pin=%0A"),
        ] {
            assert!(pairing(&bad).is_err())
        }
    }
    #[test]
    fn catalogue_is_atomic_and_scoped() {
        let item = r#"{"id":"42","filename":"42.flac","title":"Synthetic","artist":"QA","album":"Test","duration":2.5,"number":1,"disc":1,"bytes":10}"#;
        let decode_items = |items: &str| {
            decode(
                format!(r#"{{"protocol":1,"device_id":"{ID}","tracks":[{items}]}}"#).as_bytes(),
                "https://192.168.1.2:8737/v1/audio/",
                &format!("phone:{ID}"),
            )
        };
        assert_eq!(decode_items(item).unwrap()[0].title, "Synthetic");
        assert!(decode_items(&format!("{item},{item}")).is_err());
        assert!(decode_items(&item.replace("42.flac", "../secret.flac")).is_err());
        assert!(decode_items(&item.replace("\"bytes\":10", "\"bytes\":0")).is_err());
    }
}

#[cfg(test)]
mod integration {
    use super::*;
    #[test]
    #[ignore = "requires the emulator-only HTTPS server and a private pairing-link file"]
    fn live_android_catalog_sync_and_progressive_pcm() {
        use crate::streaming::{Cancellation, Pull, Stream};
        use std::sync::{Arc, atomic::AtomicU64};
        let link =
            std::fs::read_to_string(std::env::var_os("MUSICCRAFT_QA_PAIRING").unwrap()).unwrap();
        let source = pairing(&link).unwrap();
        assert_eq!(source.host, "127.0.0.1", "synthetic USB fixture only");
        let root = source.url().unwrap();
        let auth = AuthStore::default();
        auth.insert(&source).unwrap();
        let cancel = AtomicBool::new(false);
        if let Some(pinfile) = std::env::var_os("MUSICCRAFT_QA_PAIRING_CODE") {
            let code = std::fs::read_to_string(pinfile).unwrap();
            let peer = Nearby {
                name: "Synthetic phone".into(),
                host: source.host.clone(),
                port: source.port.parse().unwrap(),
            };
            let wrong = if code == "0000" { "0001" } else { "0000" };
            assert!(
                pair(&peer, wrong, &cancel, || {}).is_err(),
                "wrong PIN must fail"
            );
            let paired = pair(&peer, &code, &cancel, || {}).unwrap();
            assert_eq!(paired.tls_pin, source.tls_pin);
            assert_eq!(paired.password, source.password);
            assert_eq!(paired.id, source.id);
        }
        let tracks = catalog(&root, &auth, &cancel, Duration::from_secs(10)).unwrap();
        assert_eq!(tracks.len(), 3);
        assert!(tracks.iter().all(|t| t.artist == "MusicCraft QA"));
        let unknown = format!("{root}999999.flac");
        let other = "ftp://fixture.invalid/elsewhere.flac".to_string();
        let known = HashSet::from([unknown.clone(), other.clone(), tracks[0].source.clone()]);
        let target = crate::sync::Target::Ftp {
            id: source.id.clone(),
            root: root.clone(),
        };
        let report = crate::sync::scan(
            target.clone(),
            &auth,
            &known,
            &mut Default::default(),
            true,
            &cancel,
        );
        assert_eq!(report.stats.errors, 0);
        assert_eq!(report.absent, HashSet::from([unknown]));
        assert_eq!(report.tracks.len(), 3);
        let mut wrong = source.clone();
        wrong.tls_pin = Some(format!("{}=", "A".repeat(43)));
        let bad = AuthStore::default();
        bad.insert(&wrong).unwrap();
        let denied =
            crate::sync::scan(target, &bad, &known, &mut Default::default(), true, &cancel);
        assert!(denied.stats.root_unreachable && denied.absent.is_empty());
        for mut track in tracks {
            let fixture = std::path::Path::new("android/app/src/androidTest/assets")
                .join(format!("{}.flac", track.title));
            let expected = crate::integer::DecodedPcm::read(&fixture).unwrap();
            let temp = tempfile::tempdir().unwrap();
            let mut stream = Stream::prepare(
                &mut track,
                temp.path(),
                &auth,
                Cancellation::new(Arc::new(AtomicU64::new(1)), 1, None),
                true,
                None,
            )
            .unwrap();
            let mut actual = Vec::new();
            let deadline = std::time::Instant::now() + Duration::from_secs(15);
            loop {
                match stream.pull() {
                    Pull::Frame(f) => actual.extend_from_slice(&f[..expected.channels]),
                    Pull::End => break,
                    Pull::Pending if std::time::Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(1))
                    }
                    _ => panic!("HTTPS decoder interrupted"),
                }
            }
            assert_eq!(
                actual, expected.samples,
                "HTTPS must preserve every decoded sample"
            );
        }
    }
}
