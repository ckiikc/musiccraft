//! Missing covers only: exact album/artist matches, bounded downloads and a private disk cache.
#[cfg(test)]
mod audit;
use crate::library::{self, Artwork, Track};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    path::Path,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant, SystemTime},
};
use url::Url;

const MAX_IMAGE: usize = 8 * 1024 * 1024;
const LOOKUP_REVISION: u8 = 2;
#[derive(Clone, Serialize, Deserialize)]
struct Entry {
    created: u64,
    provider: String,
    source: String,
    found: bool,
    #[serde(default)]
    lookup_revision: u8,
    #[serde(default)]
    lookup_key: String,
}
#[derive(Clone)]
struct Candidate {
    image: String,
    source: String,
    provider: &'static str,
}
#[derive(Deserialize)]
struct LookupCorrections {
    version: u8,
    corrections: Vec<LookupCorrection>,
}
#[derive(Deserialize)]
struct LookupCorrection {
    original_artist: String,
    original_album: String,
    artist: String,
    album: String,
}
// These are lookup-only, verified identities; never write them back into Track or the audio tags.
fn lookup_track(track: &Track, cache: &Path) -> Track {
    let mut lookup = track.clone();
    let Some(parent) = cache.parent() else {
        return lookup;
    };
    let path = parent.join("cover-lookups.json");
    if !std::fs::metadata(&path).is_ok_and(|m| m.len() <= 512 * 1024) {
        return lookup;
    }
    let Some(file) = std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice::<LookupCorrections>(&b).ok())
    else {
        return lookup;
    };
    if file.version != 1 || file.corrections.len() > 512 {
        return lookup;
    }
    let artist = if track.album_artist.is_empty() {
        &track.artist
    } else {
        &track.album_artist
    };
    if let Some(c) = file.corrections.into_iter().find(|c| {
        key(&c.original_artist, &c.original_album) == key(artist, &track.album)
            && !c.artist.trim().is_empty()
            && !c.album.trim().is_empty()
            && c.artist.len() <= 512
            && c.album.len() <= 512
    }) {
        lookup.album_artist = c.artist;
        lookup.album = c.album;
    }
    lookup
}
#[derive(Debug)]
enum Error {
    Cancelled,
    Unavailable,
    RateLimited,
}
type Result<T> = std::result::Result<T, Error>;

fn key(artist: &str, album: &str) -> String {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    crate::listening::artist_key(artist).hash(&mut hash);
    album_key(album, artist).hash(&mut hash);
    format!("{:016x}", hash.finish())
}
fn album_key(album: &str, artist: &str) -> String {
    // Normalize the whole word only; Graystone and Greystone remain distinct.
    let title: String = album_query(album, artist)
        .replace('&', " and ")
        .split(|c: char| !c.is_alphanumeric())
        .map(|word| {
            crate::listening::canonical(if word.eq_ignore_ascii_case("gray") {
                "grey"
            } else {
                word
            })
        })
        .collect();
    let prefix = format!(
        "{}sings",
        crate::listening::canonical(&artist.replace('&', " and "))
    );
    title
        .strip_prefix(&prefix)
        .filter(|s| !s.is_empty())
        .unwrap_or(&title)
        .to_owned()
}

// Covers identify the album rather than its disc number or mastering date.
// Keep live/re-recorded/mix subtitles; those can denote a different recording.
fn album_title(album: &str) -> String {
    let mut title = crate::listening::lookup_title(album);
    loop {
        let before = title.clone();
        if let Some((head, country)) = title.rsplit_once('.')
            && ["japan", "germany", "usa", "uk"].contains(&country.to_ascii_lowercase().as_str())
            && let Some((base, code)) = head.rsplit_once('.')
            && catalogue_code(code)
            && !base.trim().is_empty()
        {
            title = base.trim().to_owned();
        }
        for (open, close) in [('(', ')'), ('[', ']')] {
            if title.ends_with(close)
                && let Some((head, suffix)) = title.rsplit_once(open)
                && !head.trim().is_empty()
                && cover_edition(suffix.trim_end_matches(close))
            {
                title = head.trim().to_owned();
            }
        }
        let words: Vec<_> = title.split_whitespace().collect();
        if words.len() >= 3
            && words
                .last()
                .is_some_and(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
            && ["cd", "disc", "disk"]
                .contains(&words[words.len() - 2].to_ascii_lowercase().as_str())
        {
            title = words[..words.len() - 2]
                .join(" ")
                .trim_end_matches([' ', '-'])
                .to_owned();
        }
        for count in [3, 2, 1] {
            let words: Vec<_> = title.split_whitespace().collect();
            if words.len() > count && disc_marker(&words[words.len() - count..].join(" ")) {
                title = words[..words.len() - count]
                    .join(" ")
                    .trim_end_matches([' ', '-', ',', '.'])
                    .to_owned();
                break;
            }
        }
        for suffix in [" soundtrack"] {
            if title.to_lowercase().ends_with(suffix) && title.len() > suffix.len() {
                title.truncate(title.len() - suffix.len());
            }
        }
        if title == before {
            break;
        }
    }
    title
}
fn album_query(album: &str, artist: &str) -> String {
    let title = album_title(album);
    for separator in [" - ", ": "] {
        if let Some((prefix, rest)) = title.split_once(separator)
            && crate::listening::artist_key(prefix) == crate::listening::artist_key(artist)
            && !rest.trim().is_empty()
        {
            return rest.trim().to_owned();
        }
    }
    title
}
fn catalogue_code(code: &str) -> bool {
    code.contains('-')
        && code.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        && code.bytes().filter(u8::is_ascii_digit).count() >= 2
        && code.bytes().filter(u8::is_ascii_alphabetic).count() >= 2
}
fn cover_edition(s: &str) -> bool {
    let s = s.trim().to_lowercase();
    if disc_marker(&s) {
        return true;
    }
    if [
        "original motion picture soundtrack",
        "original soundtrack",
        "soundtrack",
        "colonna sonora originale del film",
        "colonna sonora originale",
        "deluxe edition",
        "deluxe",
        "expanded",
        "original soundtrack recording",
        "expanded edition",
        "special edition",
        "anniversary edition",
    ]
    .contains(&s.as_str())
    {
        return true;
    }
    let words: Vec<_> = s.split_whitespace().collect();
    if words.len() >= 3
        && words.contains(&"first")
        && (words.contains(&"press") || words.contains(&"pressing"))
        && words.iter().all(|w| {
            [
                "first",
                "press",
                "pressing",
                "toshiba-emi",
                "toshiba",
                "emi",
                "japan",
                "japanese",
            ]
            .contains(w)
                || catalogue_code(w)
        })
    {
        return true;
    }
    if words.len() == 2
        && ["cd", "disc", "disk"].contains(&words[0])
        && words[1].chars().all(|c| c.is_ascii_digit())
    {
        return true;
    }
    words.len() >= 2
        && words.contains(&"anniversary")
        && words.iter().all(|w| {
            ["anniversary", "edition"].contains(w)
                || w.strip_suffix("th")
                    .or_else(|| w.strip_suffix("st"))
                    .or_else(|| w.strip_suffix("nd"))
                    .or_else(|| w.strip_suffix("rd"))
                    .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        })
}
fn disc_marker(s: &str) -> bool {
    let value = crate::listening::canonical(s);
    ["cd", "disc", "disk"].iter().any(|prefix| {
        value.strip_prefix(prefix).is_some_and(|number| {
            !number.is_empty() && number.len() <= 3 && number.bytes().all(|b| b.is_ascii_digit())
        })
    })
}
// Verified local copies are persistent user assets, separate from the evictable web cache.
pub fn stored(track: &Track, cache: &Path) -> Option<Artwork> {
    let artist = if track.album_artist.is_empty() {
        &track.artist
    } else {
        &track.album_artist
    };
    let path = cache
        .parent()?
        .join("covers")
        .join(format!("{}.png", key(artist, &track.album)));
    if !std::fs::metadata(&path).is_ok_and(|m| m.len() <= MAX_IMAGE as u64) {
        return None;
    }
    library::decode_art(&std::fs::read(path).ok()?)
}
fn matches(artist: &str, album: &str, t: &Track) -> bool {
    let wanted = if t.album_artist.trim().is_empty() {
        &t.artist
    } else {
        &t.album_artist
    };
    cover_artist_key(artist) == cover_artist_key(wanted)
        && !album_key(&t.album, wanted).is_empty()
        && (album_key(album, artist) == album_key(&t.album, wanted)
            || article_album_key(album, artist) == article_album_key(&t.album, wanted))
}
fn article_album_key(album: &str, artist: &str) -> String {
    let mut title = album_query(album, artist);
    for suffix in [" - single", " - ep"] {
        if title.to_lowercase().ends_with(suffix) && title.len() > suffix.len() {
            title.truncate(title.len() - suffix.len());
        }
    }
    let base = title
        .split_once(char::is_whitespace)
        .filter(|(prefix, rest)| prefix.eq_ignore_ascii_case("the") && !rest.trim().is_empty())
        .map_or(title.as_str(), |(_, rest)| rest.trim());
    album_key(base, artist)
}
fn cover_artist_key(artist: &str) -> String {
    let key = crate::listening::artist_key(&artist.replace('&', " and "));
    if [
        "variousartists",
        "various",
        "artistivari",
        "artistesdivers",
        "verschiedeneinterpreten",
        "artistasvarios",
        "variosartistas",
    ]
    .contains(&key.as_str())
    {
        "variousartists".into()
    } else {
        key
    }
}
fn allowed(url: &Url) -> bool {
    if url.scheme() != "https"
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return false;
    }
    let h = url.host_str().unwrap_or_default();
    [
        "musicbrainz.org",
        "coverartarchive.org",
        "archive.org",
        "itunes.apple.com",
        "api.deezer.com",
    ]
    .contains(&h)
        || h.ends_with(".archive.org")
        || h.ends_with(".mzstatic.com")
        || h.ends_with(".dzcdn.net")
}
#[derive(Default)]
struct Host {
    last: Option<Instant>,
    blocked: Option<Instant>,
}
fn get(url: &str, limit: usize, cancelled: &dyn Fn() -> bool) -> Result<Option<Vec<u8>>> {
    static HOSTS: OnceLock<Mutex<HashMap<String, Host>>> = OnceLock::new();
    let hosts = HOSTS.get_or_init(Default::default);
    let mut url = Url::parse(url).map_err(|_| Error::Unavailable)?;
    for _ in 0..5 {
        if !allowed(&url) {
            return Err(Error::Unavailable);
        }
        let host = url.host_str().unwrap().to_owned();
        let interval = Duration::from_millis(if host == "musicbrainz.org" {
            1100
        } else if host == "itunes.apple.com" {
            3000
        } else {
            350
        });
        loop {
            if cancelled() {
                return Err(Error::Cancelled);
            }
            let wait = {
                let mut all = hosts.lock().map_err(|_| Error::Unavailable)?;
                let h = all.entry(host.clone()).or_default();
                if h.blocked.is_some_and(|t| t > Instant::now()) {
                    return Err(Error::RateLimited);
                }
                let wait = h
                    .last
                    .map_or(Duration::ZERO, |at| interval.saturating_sub(at.elapsed()));
                if wait.is_zero() {
                    h.last = Some(Instant::now());
                }
                wait
            };
            if wait.is_zero() {
                break;
            }
            std::thread::sleep(wait.min(Duration::from_millis(50)));
        }
        let mut easy = curl::easy::Easy::new();
        easy.url(url.as_str()).map_err(|_| Error::Unavailable)?;
        easy.useragent(concat!(
            "MusicCraft/",
            env!("CARGO_PKG_VERSION"),
            " (https://github.com/ckiikc)"
        ))
        .map_err(|_| Error::Unavailable)?;
        easy.connect_timeout(Duration::from_secs(4))
            .map_err(|_| Error::Unavailable)?;
        easy.timeout(Duration::from_secs(15))
            .map_err(|_| Error::Unavailable)?;
        easy.progress(true).map_err(|_| Error::Unavailable)?;
        let mut bytes = Vec::new();
        let mut location = None;
        let mut retry = 300;
        {
            let mut transfer = easy.transfer();
            transfer
                .header_function(|h| {
                    if let Ok(h) = std::str::from_utf8(h)
                        && let Some((name, value)) = h.split_once(':')
                    {
                        if name.eq_ignore_ascii_case("Location") {
                            location = Some(value.trim().to_owned());
                        }
                        if name.eq_ignore_ascii_case("Retry-After") {
                            retry = value
                                .trim()
                                .parse::<u64>()
                                .unwrap_or_else(|_| {
                                    httpdate::parse_http_date(value.trim())
                                        .ok()
                                        .and_then(|t| t.duration_since(SystemTime::now()).ok())
                                        .map_or(300, |d| d.as_secs())
                                })
                                .clamp(1, 86400);
                        }
                    }
                    true
                })
                .map_err(|_| Error::Unavailable)?;
            transfer
                .write_function(|data| {
                    if cancelled() || bytes.len().saturating_add(data.len()) > limit {
                        return Ok(0);
                    }
                    bytes.extend_from_slice(data);
                    Ok(data.len())
                })
                .map_err(|_| Error::Unavailable)?;
            transfer
                .progress_function(|_, _, _, _| !cancelled())
                .map_err(|_| Error::Unavailable)?;
            transfer.perform().map_err(|_| {
                if cancelled() {
                    Error::Cancelled
                } else {
                    Error::Unavailable
                }
            })?;
        }
        if cancelled() {
            return Err(Error::Cancelled);
        }
        match easy.response_code().unwrap_or(0) {
            200 => return Ok(Some(bytes)),
            404 => return Ok(None),
            301 | 302 | 303 | 307 | 308 => {
                url = url
                    .join(&location.ok_or(Error::Unavailable)?)
                    .map_err(|_| Error::Unavailable)?;
            }
            429 | 503 => {
                if let Ok(mut all) = hosts.lock() {
                    all.entry(host).or_default().blocked =
                        Instant::now().checked_add(Duration::from_secs(retry));
                }
                return Err(Error::RateLimited);
            }
            _ => return Err(Error::Unavailable),
        }
    }
    Err(Error::Unavailable)
}
fn json(url: Url, cancelled: &dyn Fn() -> bool) -> Result<Option<Value>> {
    get(url.as_str(), 2 * 1024 * 1024, cancelled)?
        .map(|b| serde_json::from_slice(&b).map_err(|_| Error::Unavailable))
        .transpose()
}
fn query_url(base: &str, fields: &[(&str, &str)]) -> Url {
    let mut url = Url::parse(base).unwrap();
    url.query_pairs_mut().extend_pairs(fields.iter().copied());
    url
}
fn candidates(provider: usize, t: &Track, cancelled: &dyn Fn() -> bool) -> Result<Vec<Candidate>> {
    let artist = if t.album_artist.trim().is_empty() {
        &t.artist
    } else {
        &t.album_artist
    };
    let album = album_query(&t.album, artist);
    let url = match provider {
        0 => query_url(
            "https://musicbrainz.org/ws/2/release-group/",
            &[
                (
                    "query",
                    &format!(
                        "releasegroup:\"{}\" AND artist:\"{}\"",
                        album.replace(['"', '\\'], " "),
                        artist.replace(['"', '\\'], " ")
                    ),
                ),
                ("fmt", "json"),
                ("limit", "10"),
            ],
        ),
        1 => query_url(
            "https://itunes.apple.com/search",
            &[
                ("term", &format!("{artist} {album}")),
                ("entity", "album"),
                ("limit", "20"),
            ],
        ),
        _ => query_url(
            "https://api.deezer.com/search/album",
            &[
                (
                    "q",
                    &format!(
                        "artist:\"{}\" album:\"{}\"",
                        artist.replace('"', " "),
                        album.replace('"', " ")
                    ),
                ),
                ("limit", "20"),
            ],
        ),
    };
    let response = json(url, cancelled);
    #[cfg(test)]
    audit::record_response(provider, t, &response);
    let Some(data) = response? else {
        return Ok(vec![]);
    };
    let Some(rows) = data[match provider {
        0 => "release-groups",
        1 => "results",
        _ => "data",
    }]
    .as_array() else {
        return Err(Error::Unavailable);
    };
    let found = parse_candidates(provider, t, rows);
    if provider == 1 && found.is_empty() && cover_artist_key(artist) != "variousartists" {
        let artists = query_url(
            "https://itunes.apple.com/search",
            &[("term", artist), ("entity", "musicArtist"), ("limit", "10")],
        );
        let artist_response = json(artists, cancelled);
        #[cfg(test)]
        audit::record_response(5, t, &artist_response);
        if let Some(data) = artist_response?
            && let Some(rows) = data["results"].as_array()
        {
            let mut ids = Vec::new();
            for row in rows.iter().filter(|r| {
                r["artistName"]
                    .as_str()
                    .is_some_and(|a| cover_artist_key(a) == cover_artist_key(artist))
            }) {
                if let Some(id) = row["artistId"].as_u64()
                    && !ids.contains(&id)
                {
                    ids.push(id);
                }
            }
            for id in ids.into_iter().take(2) {
                for (index, country) in ["US", preferred_store()].into_iter().enumerate() {
                    if index == 1 && country == "US" {
                        break;
                    }
                    let albums = query_url(
                        "https://itunes.apple.com/lookup",
                        &[
                            ("id", &id.to_string()),
                            ("entity", "album"),
                            ("limit", "200"),
                            ("country", country),
                        ],
                    );
                    let album_response = json(albums, cancelled);
                    #[cfg(test)]
                    audit::record_response(if country == "US" { 3 } else { 4 }, t, &album_response);
                    if let Some(data) = album_response?
                        && let Some(rows) = data["results"].as_array()
                    {
                        let found = parse_candidates(1, t, rows);
                        if !found.is_empty() {
                            return Ok(found);
                        }
                    }
                    if preferred_store() == "US" {
                        break;
                    }
                }
            }
        }
    }
    Ok(found)
}
fn preferred_store() -> &'static str {
    match crate::i18n::language() {
        "it" => "IT",
        "de" => "DE",
        "fr" => "FR",
        "es" => "ES",
        "pt-br" => "BR",
        "ja" => "JP",
        "ru" => "RU",
        "uk" => "UA",
        "zh-hans" => "CN",
        "zh-hant" => "TW",
        "ar" => "AE",
        "bg" => "BG",
        "cs" => "CZ",
        "el" => "GR",
        "hu" => "HU",
        "kk" => "KZ",
        "lt" => "LT",
        "pl" => "PL",
        "sl" => "SI",
        "te" => "IN",
        "tr" => "TR",
        _ => "GB",
    }
}
fn parse_candidates(provider: usize, t: &Track, rows: &[Value]) -> Vec<Candidate> {
    let mut found = Vec::new();
    for r in rows.iter().take(201) {
        let credit = r["artist-credit"]
            .as_array()
            .map(|credits| {
                credits
                    .iter()
                    .filter_map(|credit| {
                        credit["artist"]["name"]
                            .as_str()
                            .or_else(|| credit["name"].as_str())
                    })
                    .collect::<Vec<_>>()
                    .join(" and ")
            })
            .unwrap_or_default();
        let (a, b) = match provider {
            0 => (Some(credit.as_str()), r["title"].as_str()),
            1 => (r["artistName"].as_str(), r["collectionName"].as_str()),
            _ => (r["artist"]["name"].as_str(), r["title"].as_str()),
        };
        if !a.zip(b).is_some_and(|(a, b)| matches(a, b, t)) {
            continue;
        }
        let c = match provider {
            0 => {
                let Some(id) = r["id"].as_str().filter(|id| {
                    id.len() == 36 && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
                }) else {
                    continue;
                };
                Candidate {
                    image: format!("https://coverartarchive.org/release-group/{id}/front-500"),
                    source: format!("https://musicbrainz.org/release-group/{id}"),
                    provider: "MusicBrainz / Cover Art Archive",
                }
            }
            1 => {
                let Some(image) = r["artworkUrl100"].as_str() else {
                    continue;
                };
                Candidate {
                    image: image.replace("/100x100bb.", "/800x800bb."),
                    source: r["collectionViewUrl"]
                        .as_str()
                        .unwrap_or("https://music.apple.com")
                        .into(),
                    provider: "Apple Music",
                }
            }
            _ => {
                let Some(image) = r["cover_xl"].as_str().or_else(|| r["cover_big"].as_str()) else {
                    continue;
                };
                Candidate {
                    image: image.into(),
                    source: r["link"]
                        .as_str()
                        .unwrap_or("https://www.deezer.com")
                        .into(),
                    provider: "Deezer",
                }
            }
        };
        found.push(c);
        if found.len() == 2 {
            break;
        }
    }
    found
}

pub fn online(t: &Track, cache: &Path, cancelled: &dyn Fn() -> bool) -> Option<Artwork> {
    if t.album.trim().is_empty()
        || (t.artist.trim().is_empty() && t.album_artist.trim().is_empty())
        || cancelled()
    {
        return None;
    }
    static SINGLE: Mutex<()> = Mutex::new(());
    let _guard = loop {
        if cancelled() {
            return None;
        }
        match SINGLE.try_lock() {
            Ok(g) => break g,
            Err(std::sync::TryLockError::WouldBlock) => {
                std::thread::sleep(Duration::from_millis(50))
            }
            Err(_) => return None,
        }
    };
    let lookup = lookup_track(t, cache);
    let lookup_artist = if lookup.album_artist.is_empty() {
        &lookup.artist
    } else {
        &lookup.album_artist
    };
    let key = self::key(lookup_artist, &lookup.album);
    let lookup_key = key.clone();
    let root = cache.join("album-art-v1");
    let manifest = root.join(format!("{key}.json"));
    let image = root.join(format!("{key}.png"));
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()?
        .as_secs();
    if std::fs::metadata(&manifest).is_ok_and(|m| m.len() <= 16 * 1024)
        && let Ok(bytes) = std::fs::read(&manifest)
        && let Ok(entry) = serde_json::from_slice::<Entry>(&bytes)
    {
        if !entry.found
            && entry.lookup_key == lookup_key
            && entry.lookup_revision == LOOKUP_REVISION
            && now.saturating_sub(entry.created) < 600
        {
            return None;
        }
        if entry.found
            && (entry.lookup_key.is_empty() || entry.lookup_key == lookup_key)
            && std::fs::metadata(&image).is_ok_and(|m| m.len() <= MAX_IMAGE as u64)
            && let Ok(bytes) = std::fs::read(&image)
            && bytes.len() <= MAX_IMAGE
            && let Some(mut art) = library::decode_art(&bytes)
        {
            art.origin = Some((entry.provider, entry.source));
            return Some(art);
        }
    }
    let mut complete = true;
    for provider in 0..3 {
        let result = candidates(provider, &lookup, cancelled);
        if matches!(result, Err(Error::Cancelled)) {
            return None;
        }
        if result.is_err() {
            complete = false;
        }
        for c in result.unwrap_or_default() {
            if cancelled() {
                return None;
            }
            let downloaded = get(&c.image, MAX_IMAGE, cancelled);
            if downloaded.is_err() {
                complete = false;
            }
            if let Ok(Some(bytes)) = downloaded
                && let Some(mut art) = library::decode_art(&bytes)
            {
                if cancelled() {
                    return None;
                }
                let entry = Entry {
                    created: now,
                    provider: c.provider.into(),
                    source: c.source.clone(),
                    found: true,
                    lookup_revision: LOOKUP_REVISION,
                    lookup_key: lookup_key.clone(),
                };
                if std::fs::create_dir_all(&root).is_ok() {
                    let temp = image.with_extension("png.part");
                    if image::save_buffer_with_format(
                        &temp,
                        &art.rgba,
                        art.size[0] as u32,
                        art.size[1] as u32,
                        image::ColorType::Rgba8,
                        image::ImageFormat::Png,
                    )
                    .is_ok()
                    {
                        let _ = std::fs::rename(temp, &image);
                    }
                    write_entry(&manifest, &entry);
                    trim(&root, &key);
                }
                art.origin = Some((c.provider.into(), c.source));
                return Some(art);
            }
        }
    }
    if complete && !cancelled() && std::fs::create_dir_all(&root).is_ok() {
        write_entry(
            &manifest,
            &Entry {
                created: now,
                provider: String::new(),
                source: String::new(),
                found: false,
                lookup_revision: LOOKUP_REVISION,
                lookup_key,
            },
        );
        trim(&root, &key);
    }
    None
}
fn write_entry(path: &Path, entry: &Entry) {
    if let Ok(bytes) = serde_json::to_vec(entry) {
        let temp = path.with_extension("json.part");
        if std::fs::write(&temp, bytes).is_ok() {
            let _ = std::fs::rename(temp, path);
        }
    }
}
fn trim(root: &Path, keep: &str) {
    let mut entries: Vec<_> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .filter_map(std::result::Result::ok)
        .filter_map(|e| {
            let p = e.path();
            let stem = p.file_stem()?.to_str()?.to_owned();
            if stem.len() != 16
                || !stem.chars().all(|c| c.is_ascii_hexdigit())
                || p.extension()? != "json"
            {
                return None;
            }
            let png = root.join(format!("{stem}.png"));
            let m = e.metadata().ok()?;
            Some((
                stem,
                m.modified().ok()?,
                m.len() + std::fs::metadata(png).map_or(0, |m| m.len()),
            ))
        })
        .collect();
    entries.sort_by_key(|(_, at, _)| *at);
    let mut bytes: u64 = entries.iter().map(|(_, _, n)| *n).sum();
    let mut count = entries.len();
    for (key, _, n) in entries {
        if count <= 256 && bytes <= 128 * 1024 * 1024 {
            break;
        }
        if key == keep {
            continue;
        }
        let _ = std::fs::remove_file(root.join(format!("{key}.png")));
        if std::fs::remove_file(root.join(format!("{key}.json"))).is_ok() {
            count -= 1;
            bytes = bytes.saturating_sub(n);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stored_art_is_scoped_to_the_original_identity_and_bounded() {
        let profile = tempfile::tempdir().unwrap();
        let cache = profile.path().join("cache");
        let mut t = Track::placeholder("/synthetic/01.flac".into());
        t.artist = "Composer".into();
        t.album = "Verified Recording".into();
        let root = profile.path().join("covers");
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join(format!("{}.png", key(&t.artist, &t.album)));
        image::save_buffer(&path, &[20, 80, 120, 255], 1, 1, image::ColorType::Rgba8).unwrap();
        assert_eq!(&stored(&t, &cache).unwrap().rgba[..4], &[20, 80, 120, 255]);
        t.album = "Another Recording".into();
        assert!(stored(&t, &cache).is_none());
        t.album = "Verified Recording".into();
        std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .unwrap()
            .set_len(MAX_IMAGE as u64 + 1)
            .unwrap();
        assert!(stored(&t, &cache).is_none());
    }
    #[test]
    fn corrections_are_lookup_only_and_compilations_share_a_verified_cache() {
        let profile = tempfile::tempdir().unwrap();
        let cache = profile.path().join("cache");
        let mut t = Track::placeholder("/synthetic/01.flac".into());
        t.artist = "Actor One".into();
        t.album = "Original Soundtrack".into();
        std::fs::write(profile.path().join("cover-lookups.json"),serde_json::to_vec(&serde_json::json!({"version":1,"corrections":[{"original_artist":t.artist,"original_album":t.album,"artist":"Various Artists","album":"Verified Soundtrack"}]})).unwrap()).unwrap();
        let lookup = lookup_track(&t, &cache);
        assert_eq!(lookup.album_artist, "Various Artists");
        assert_eq!(t.artist, "Actor One");
        assert_eq!(t.album, "Original Soundtrack");
        let k = key(&lookup.album_artist, &lookup.album);
        let root = cache.join("album-art-v1");
        std::fs::create_dir_all(&root).unwrap();
        image::save_buffer(
            root.join(format!("{k}.png")),
            &[20, 80, 120, 255],
            1,
            1,
            image::ColorType::Rgba8,
        )
        .unwrap();
        write_entry(
            &root.join(format!("{k}.json")),
            &Entry {
                created: 1,
                provider: "Fixture".into(),
                source: "https://example.invalid/fixture".into(),
                found: true,
                lookup_revision: LOOKUP_REVISION,
                lookup_key: k,
            },
        );
        let checks = std::sync::atomic::AtomicUsize::new(0);
        let art = online(&t, &cache, &|| {
            checks.fetch_add(1, std::sync::atomic::Ordering::Relaxed) >= 2
        })
        .unwrap();
        assert_eq!(&art.rgba[..4], &[20, 80, 120, 255]);
        assert_eq!(art.origin.unwrap().0, "Fixture");
        std::fs::write(profile.path().join("cover-lookups.json"), b"invalid JSON").unwrap();
        assert!(lookup_track(&t, &cache).album_artist.is_empty());
    }
    #[test]
    fn articles_and_various_artist_aliases_keep_exact_album_and_artist_boundaries() {
        let mut t = Track::placeholder("/synthetic/01.flac".into());
        t.artist = "Sting".into();
        t.album = "Dream of the Blue Turtles".into();
        assert!(matches("Sting", "The Dream of the Blue Turtles", &t));
        assert!(!matches(
            "Other Artist",
            "The Dream of the Blue Turtles",
            &t
        ));
        t.album = "Atre".into();
        assert!(!matches("Sting", "Theatre", &t));
        t.album_artist = "Various Artists".into();
        t.album = "Death Proof".into();
        assert!(matches(
            "Artisti Vari",
            "Death Proof (Original Motion Picture Soundtrack)",
            &t
        ));
        assert!(!matches("Other Band", "Death Proof", &t));
        assert!(!matches("Artisti Vari", "Death Proof Live", &t));
    }
    #[test]
    fn complete_musicbrainz_credits_and_the_last_artist_catalogue_album_are_checked() {
        let mut t = Track::placeholder("/synthetic/01.flac".into());
        t.artist = "Alpha & Beta".into();
        t.album = "Joint Album".into();
        let row = serde_json::json!({"title":"Joint Album","id":"12345678-1234-1234-1234-123456789abc","artist-credit":[{"artist":{"name":"Alpha"},"joinphrase":" & "},{"artist":{"name":"Beta"}}]});
        assert_eq!(parse_candidates(0, &t, std::slice::from_ref(&row)).len(), 1);
        t.artist = "Alpha".into();
        assert!(parse_candidates(0, &t, &[row]).is_empty());
        let mut catalogue = vec![
            serde_json::json!({"artistName":"Alpha","collectionName":"Other Album","artworkUrl100":"https://example.invalid/x"});
            200
        ];
        catalogue.push(serde_json::json!({"artistName":"Alpha","collectionName":"Joint Album","artworkUrl100":"https://example.invalid/x"}));
        assert_eq!(parse_candidates(1, &t, &catalogue).len(), 1);
    }
    #[test]
    fn cover_queries_remove_mastering_and_disc_labels_but_preserve_recording_identity() {
        for (tag, base) in [
            ("Revolver (2009 Stereo Remaster)", "Revolver"),
            ("The Beatles Disc 2 (2009 Stereo Remaster)", "The Beatles"),
            ("Profondo Rosso CD 1", "Profondo Rosso"),
            ("Greatest Hits (CD1)", "Greatest Hits"),
            ("Greatest Hits (CD2)", "Greatest Hits"),
            ("Joe's Garage Cd.1.", "Joe's Garage"),
            ("Suspiria Soundtrack", "Suspiria"),
            (
                "Suspiria (40th Anniversary) [Original Motion Picture Soundtrack]",
                "Suspiria",
            ),
        ] {
            assert_eq!(album_title(tag), base);
        }
        let mut t = Track::placeholder("/synthetic/01.flac".into());
        t.artist = "Goblin".into();
        t.album = "Suspiria Soundtrack".into();
        assert!(matches(
            "Goblin",
            "Suspiria (40th Anniversary) (Original Motion Picture Soundtrack)",
            &t
        ));
        assert!(!matches("Thom Yorke", "Suspiria", &t));
        for title in [
            "Suspiria Live",
            "Suspiria (Live)",
            "Suspiria (New Recording)",
            "Suspiria (Remix)",
        ] {
            assert!(!matches("Goblin", title, &t));
        }
        let legacy: Entry =
            serde_json::from_str(r#"{"created":1,"provider":"","source":"","found":false}"#)
                .unwrap();
        assert_ne!(legacy.lookup_revision, LOOKUP_REVISION);
    }
    #[test]
    fn actual_catalogue_codes_and_artist_prefixes_do_not_hide_an_exact_album() {
        let mut t = Track::placeholder("/synthetic/01.flac".into());
        t.artist = "Genesis".into();
        for (tag, base) in [
            ("Nursery Cryme.32VD-1026.Japan", "Nursery Cryme"),
            ("Trespass.VJD28-006.Japan", "Trespass"),
            (
                "Invisible Touch (Toshiba-EMI First Press 32VD-1023)",
                "Invisible Touch",
            ),
        ] {
            t.album = tag.into();
            assert_eq!(album_query(tag, "Genesis"), base);
            assert!(matches("Genesis", base, &t));
            assert!(!matches("Other Band", base, &t));
        }
        t.artist = "Oliver Onions".into();
        t.album = "Oliver Onions - Best Of Bud Spencer & Terence Hill".into();
        assert!(matches(
            "Oliver Onions",
            "Best of Bud Spencer and Terence Hill",
            &t
        ));
        assert!(!matches(
            "Oliver Onions",
            "Best of Bud Spencer and Terence Hill Live",
            &t
        ));
        assert_eq!(
            album_query("Artist - Other Album", "Different Artist"),
            "Artist - Other Album"
        );
        assert_eq!(album_title("Vol.1.Japan"), "Vol.1.Japan");
    }
    #[test]
    fn grey_and_gray_match_without_fuzzy_artist_or_title_matching() {
        let mut t = Track::placeholder("/synthetic/caravan.flac".into());
        t.artist = "Caravan".into();
        t.album = "In the Land of Gray and Pink".into();
        assert!(matches("Caravan", "In the Land of Grey and Pink", &t));
        assert_eq!(
            key(&t.artist, &t.album),
            key("Caravan", "In the Land of Grey and Pink")
        );
        assert!(!matches("Other Artist", "In the Land of Grey and Pink", &t));
        assert!(!matches("Caravan", "In the Land of Grey and Pink Live", &t));
        assert!(!matches("Caravan", "In the Land of Grey and Blue", &t));
        assert_ne!(
            album_key("Graystone", "Caravan"),
            album_key("Greystone", "Caravan")
        );
        assert_eq!(
            album_key("Gray-Pink", "Caravan"),
            album_key("GREY Pink", "Caravan")
        );
    }
    #[test]
    #[ignore = "Explicit public cover integration; requires Internet"]
    fn live_caravan_gray_tags_download_cover_and_reopen_from_cache() {
        let mut t = Track::placeholder("/synthetic/caravan.flac".into());
        t.artist = "Caravan".into();
        t.album = "In the Land of Gray and Pink".into();
        let temporary = tempfile::tempdir().unwrap();
        let root = std::env::var_os("MUSICCRAFT_COVER_TEST_CACHE")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| temporary.path().to_owned());
        let first =
            online(&t, &root, &|| false).expect("Caravan cover must be found with Gray tags");
        assert!(first.size[0] >= 400 && first.origin.is_some());
        t.album = "In the Land of Grey and Pink".into();
        let second = online(&t, &root, &|| false).unwrap();
        assert_eq!(first.rgba, second.rgba);
        println!(
            "CARAVAN provider={:?} size={:?} cache_reopened=true",
            first.origin, first.size
        );
    }
    #[test]
    fn album_identity_supports_sings_prefix_without_accepting_other_albums_or_artists() {
        let mut t = Track::placeholder("/synthetic/01.flac".into());
        t.artist = "Eric Woolfson".into();
        t.album = "The Alan Parsons Project That Never Was".into();
        assert!(matches(
            "Eric Woolfson",
            "Eric Woolfson Sings the Alan Parsons Project That Never Was",
            &t
        ));
        assert!(!matches(
            "The Alan Parsons Project",
            "The Best of The Alan Parsons Project",
            &t
        ));
        assert!(!matches("Other Artist", &t.album, &t));
        for u in [
            "http://musicbrainz.org/x",
            "https://localhost/x",
            "https://musicbrainz.org.evil.example/x",
            "file:///tmp/a",
            "https://user@musicbrainz.org/x",
        ] {
            assert!(!allowed(&Url::parse(u).unwrap()));
        }
        assert!(allowed(
            &Url::parse("https://is1-ssl.mzstatic.com/image/x.jpg").unwrap()
        ));
    }
    #[test]
    #[ignore = "Explicit read-only public cover integration; requires Internet"]
    fn live_reported_album_downloads_cover_and_reopens_from_cache() {
        let mut t = Track::placeholder("/synthetic/01.flac".into());
        t.artist = "Eric Woolfson".into();
        t.album = "The Alan Parsons Project That Never Was".into();
        let root = tempfile::tempdir().unwrap();
        let first = online(&t, root.path(), &|| false).expect("Reported cover must be found");
        assert!(first.size[0] >= 400 && first.origin.is_some());
        let second = online(&t, root.path(), &|| false).unwrap();
        assert_eq!(first.rgba, second.rgba);
        println!(
            "COVER provider={:?} size={:?} cache_reopened=true",
            first.origin, first.size
        );
    }
}
