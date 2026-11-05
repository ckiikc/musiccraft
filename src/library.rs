use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};
use url::Url;

pub const MAX_TRACKS: usize = 100_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scope {
    All,
    Remote(String),
    Local(PathBuf),
    Missing,
}
impl Scope {
    pub fn contains(&self, source: &str) -> bool {
        match self {
            Self::All => true,
            Self::Remote(root) => source.starts_with(root),
            Self::Local(root) => !is_remote(source) && Path::new(source).starts_with(root),
            Self::Missing => false,
        }
    }
}

/// FTP percent encoding is a spelling of a path, not a second file. Decode once
/// per segment and re-encode bytes, preserving encoded slashes, literal percent,
/// path case, host, port and protocol. Local/NAS and HTTP identities stay intact.
pub fn source_key(source: &str) -> String {
    let Ok(mut url) = Url::parse(source) else {
        return source.to_owned();
    };
    if !matches!(url.scheme(), "ftp" | "ftps") {
        return source.to_owned();
    }
    const SEGMENT: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'.')
        .remove(b'_')
        .remove(b'~');
    let segments: Vec<_> = url
        .path_segments()
        .into_iter()
        .flatten()
        .map(|segment| {
            let bytes: Vec<_> = percent_encoding::percent_decode_str(segment).collect();
            percent_encoding::percent_encode(&bytes, SEGMENT).to_string()
        })
        .collect();
    url.set_path(&format!("/{}", segments.join("/")));
    url.into()
}
/// Retain stable source IDs and the richest existing metadata. No source files are modified.
/// Returned aliases also reconcile persistent listening selections and shuffle history.
pub fn deduplicate_tracks(tracks: &mut Vec<Track>) -> HashMap<String, String> {
    let mut positions = HashMap::new();
    let mut aliases = HashMap::new();
    let mut unique: Vec<Track> = Vec::with_capacity(tracks.len());
    let quality = |t: &Track| {
        (
            t.metadata_loaded || t.rate > 0,
            usize::from(!t.artist.is_empty())
                + usize::from(!t.album.is_empty())
                + usize::from(!t.album_artist.is_empty())
                + usize::from(!t.genre.is_empty())
                + usize::from(t.year.is_some()),
            t.rate > 0,
            t.metadata_loaded,
        )
    };
    for track in std::mem::take(tracks) {
        let key = source_key(&track.source);
        if let Some(&index) = positions.get(&key) {
            let kept: &mut Track = &mut unique[index];
            if kept.source != track.source {
                aliases.insert(track.source.clone(), kept.source.clone());
            }
            if quality(&track) > quality(kept) {
                let source = kept.source.clone();
                *kept = track;
                kept.source = source;
            }
        } else {
            positions.insert(key, unique.len());
            unique.push(track);
        }
    }
    *tracks = unique;
    aliases
}

/// Merge only an unread, server-verified phantom into an existing literal entry.
/// Enriched records are never removed; this is not whitespace-based global deduplication.
pub fn merge_verified_alias(tracks: &mut Vec<Track>, source: &str, target: &str) -> bool {
    if source == target {
        return false;
    }
    let Some(pos) = tracks
        .iter()
        .position(|t| t.source == source && !t.metadata_loaded && t.rate == 0)
    else {
        return false;
    };
    if !tracks.iter().any(|t| t.source == target) {
        let mut literal = tracks[pos].clone();
        literal.source = target.to_owned();
        tracks.push(literal);
    }
    tracks.remove(pos);
    true
}
/// Source refresh is additive: preserve order and enriched metadata of known tracks.
pub fn merge_tracks(stored: &mut Vec<Track>, incoming: Vec<Track>) -> usize {
    let mut known: HashSet<String> = stored.iter().map(|t| source_key(&t.source)).collect();
    let before = stored.len();
    for track in incoming {
        if stored.len() >= MAX_TRACKS {
            break;
        }
        if known.insert(source_key(&track.source)) {
            stored.push(track);
        }
    }
    stored.len() - before
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Track {
    pub source: String,
    #[serde(default)]
    pub metadata_loaded: bool,
    pub title: String,
    pub artist: String,
    pub album: String,
    #[serde(default)]
    pub album_artist: String,
    #[serde(default)]
    pub genre: String,
    #[serde(default)]
    pub year: Option<u32>,
    #[serde(default)]
    pub disc: u32,
    pub codec: String,
    pub rate: u32,
    pub bits: u8,
    pub channels: u8,
    pub duration: f64,
    pub number: u32,
}

impl Track {
    pub fn placeholder(source: String) -> Self {
        let path = Url::parse(&source)
            .ok()
            .filter(|u| matches!(u.scheme(), "ftp" | "ftps" | "http" | "https"))
            .map(|u| {
                percent_encoding::percent_decode_str(u.path())
                    .decode_utf8_lossy()
                    .into_owned()
            })
            .unwrap_or_else(|| source.clone());
        let p = Path::new(&path);
        Self {
            title: p
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            codec: p
                .extension()
                .unwrap_or_default()
                .to_string_lossy()
                .to_uppercase(),
            source,
            metadata_loaded: false,
            artist: String::new(),
            album: String::new(),
            album_artist: String::new(),
            genre: String::new(),
            year: None,
            disc: 0,
            rate: 0,
            bits: 0,
            channels: 0,
            duration: 0.0,
            number: 0,
        }
    }

    pub fn read(source: String) -> Self {
        let mut track = Self::placeholder(source);
        if !is_remote(&track.source) {
            track.update_metadata(Path::new(&track.source.clone()));
        }
        track
    }

    pub fn update_metadata(&mut self, path: &Path) {
        use lofty::file::{AudioFile, TaggedFileExt};
        let parsed = (|| {
            lofty::probe::Probe::open(path)
                .ok()?
                .guess_file_type()
                .ok()?
                .options(lofty::config::ParseOptions::new().read_cover_art(false))
                .read()
                .ok()
        })();
        if let Some(file) = parsed {
            let props = file.properties();
            self.rate = props.sample_rate().unwrap_or(0);
            self.bits = props.bit_depth().unwrap_or(0);
            self.channels = props.channels().unwrap_or(0);
            self.duration = props.duration().as_secs_f64();
            if let Some(tag) = file.primary_tag().or_else(|| file.first_tag()) {
                self.apply_tag(tag);
            }
            self.metadata_loaded = true;
        }
    }

    /// Merge descriptive metadata only; the playing decoder owns the audio properties.
    pub(crate) fn adopt_tags(&mut self, incoming: Self) {
        self.title = incoming.title;
        self.artist = incoming.artist;
        self.album = incoming.album;
        self.album_artist = incoming.album_artist;
        self.genre = incoming.genre;
        self.year = incoming.year;
        self.number = incoming.number;
        self.disc = incoming.disc;
        self.metadata_loaded = true;
    }

    pub(crate) fn apply_tag(&mut self, tag: &lofty::tag::Tag) {
        use lofty::tag::Accessor;
        let trim = |s: std::borrow::Cow<'_, str>| s.chars().take(512).collect::<String>();
        if let Some(title) = tag.title() {
            self.title = trim(title);
        }
        if let Some(artist) = tag.artist() {
            self.artist = trim(artist);
        }
        if let Some(album) = tag.album() {
            self.album = trim(album);
        }
        self.album_artist = tag
            .get_string(lofty::tag::ItemKey::AlbumArtist)
            .unwrap_or_default()
            .chars()
            .take(512)
            .collect();
        self.genre = tag.genre().map(trim).unwrap_or_default();
        self.year = tag
            .get_string(lofty::tag::ItemKey::RecordingDate)
            .and_then(|v| v.get(..4))
            .and_then(|v| v.parse().ok());
        self.disc = tag.disk().unwrap_or(0);
        self.number = tag.track().unwrap_or(0);
        self.metadata_loaded = true;
    }

    /// Upgrade only untouched legacy URL placeholders; genuine tag text is never decoded.
    pub(crate) fn repair_placeholder(&mut self) -> bool {
        if !is_remote(&self.source)
            || self.metadata_loaded
            || self.rate > 0
            || !self.artist.is_empty()
            || !self.album.is_empty()
        {
            return false;
        }
        let Ok(url) = Url::parse(&self.source) else {
            return false;
        };
        let old = Path::new(url.path())
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy();
        if self.title != old {
            return false;
        }
        let repaired = Self::placeholder(self.source.clone()).title;
        let changed = self.title != repaired;
        self.title = repaired;
        changed
    }

    pub fn quality(&self) -> String {
        let mut text = self.codec.clone();
        if self.rate > 0 {
            text += &format!(" · {:.1} kHz", self.rate as f64 / 1000.0);
        }
        if self.bits > 0 {
            text += &format!(" · {} bit", self.bits);
        }
        if self.channels > 0 {
            text += &format!(" · {} ch", self.channels);
        }
        text
    }
}

pub fn is_remote(source: &str) -> bool {
    Url::parse(source).is_ok_and(|u| matches!(u.scheme(), "http" | "https" | "ftp" | "ftps"))
}

/// Never persist credentials from an imported playlist or expose them in process arguments.
pub fn validate_url(input: &str) -> Result<String, String> {
    let u = Url::parse(input).map_err(|_| "Indirizzo di rete non valido".to_string())?;
    if !matches!(u.scheme(), "http" | "https" | "ftp" | "ftps") || u.host_str().is_none() {
        return Err("Usa HTTP(S)/FTP(S), oppure monta la condivisione SMB nel Finder".into());
    }
    if !u.username().is_empty() || u.password().is_some() || u.query().is_some() {
        return Err("Per librerie autenticate usa una cartella NAS montata: le credenziali e i token URL non vengono salvati".into());
    }
    Ok(u.into())
}

pub fn audio_path(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        [
            "flac", "wav", "aif", "aiff", "m4a", "alac", "mp3", "aac", "ogg", "oga", "mp4",
        ]
        .contains(&e.to_ascii_lowercase().as_str())
    })
}

pub fn playlist(path: &Path) -> Result<Vec<String>, String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|_| "Impossibile leggere la playlist".to_string())?
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Lettura playlist interrotta".to_string())?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("Playlist oltre 16 MiB".into());
    }
    let text = String::from_utf8(bytes.clone())
        .unwrap_or_else(|_| bytes.iter().map(|&b| char::from(b)).collect());
    let base = path.parent().unwrap_or(Path::new("."));
    let mut found = HashSet::new();
    let mut out = Vec::new();
    for line in text.trim_start_matches('\u{feff}').lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let source = if line.contains("://") {
            if line.starts_with("file://") {
                Url::parse(line)
                    .ok()
                    .and_then(|u| u.to_file_path().ok())
                    .map(|p| p.to_string_lossy().into_owned())
            } else {
                validate_url(line).ok()
            }
        } else {
            let p = PathBuf::from(line);
            Some(
                if p.is_absolute() { p } else { base.join(p) }
                    .to_string_lossy()
                    .into_owned(),
            )
        };
        if let Some(source) = source
            && found.insert(source.clone())
        {
            out.push(source);
            if out.len() >= MAX_TRACKS {
                break;
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod identity_tests {
    use super::*;
    #[test]
    fn ftp_identity_decodes_once_without_merging_distinct_files_or_sources() {
        let a = "ftp://phone:8736/Music/Album%20%28FLAC%20%2B%20CUE%29/01%20%5BLive%5D.flac";
        let b = "ftp://phone:8736/Music/Album%20(FLAC%20+%20CUE)/01%20[Live].flac";
        assert_eq!(source_key(a), source_key(b));
        assert_eq!(source_key(&source_key(b)), source_key(b));
        for (a, b) in [
            ("ftp://phone/A%2FB.flac", "ftp://phone/A/B.flac"),
            ("ftp://phone/A%2520B.flac", "ftp://phone/A%20B.flac"),
            ("ftp://phone/A.flac", "ftp://phone/a.flac"),
            ("ftp://phone/A.flac", "ftp://other/A.flac"),
            ("ftp://phone:8736/A.flac", "ftp://phone:8737/A.flac"),
            ("ftp://phone/A.flac", "ftps://phone/A.flac"),
            ("/nas/A.flac", "ftp://phone/A.flac"),
            ("https://example/A%2BB.flac", "https://example/A+B.flac"),
        ] {
            assert_ne!(source_key(a), source_key(b), "{a} / {b}");
        }
    }
    #[test]
    fn existing_duplicates_merge_metadata_and_future_sync_is_idempotent() {
        let legacy = "ftp://phone/Music/Album%20%28FLAC%20%2B%20CUE%29/01.flac";
        let current = "ftp://phone/Music/Album%20(FLAC%20+%20CUE)/01.flac";
        let mut enriched = Track::placeholder(current.into());
        enriched.title = "Actual title".into();
        enriched.artist = "Actual artist".into();
        enriched.album = "Actual album".into();
        enriched.metadata_loaded = true;
        enriched.rate = 44100;
        let mut tracks = vec![
            Track::placeholder(legacy.into()),
            enriched.clone(),
            Track::placeholder("/nas/01.flac".into()),
        ];
        let aliases = deduplicate_tracks(&mut tracks);
        assert_eq!(tracks.len(), 2);
        assert_eq!(aliases[current], legacy);
        assert_eq!(tracks[0].source, legacy);
        assert_eq!(tracks[0].title, "Actual title");
        assert!(tracks[0].metadata_loaded);
        assert_eq!(tracks[0].rate, 44100);
        assert_eq!(
            merge_tracks(&mut tracks, vec![enriched.clone(), enriched]),
            0
        );
        assert_eq!(tracks.len(), 2);
        assert!(deduplicate_tracks(&mut tracks).is_empty());
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn verified_alias_repair_preserves_loaded_tags_and_only_removes_unread_phantoms() {
        use super::*;
        let old = "ftp://phone/Music/01.flac";
        let new = "ftp://phone/Music/%20%2001.flac";
        let mut tagged = Track::placeholder(new.into());
        tagged.metadata_loaded = true;
        tagged.artist = "Original Artist".into();
        tagged.title = "Original Title".into();
        let mut tracks = vec![Track::placeholder(old.into()), tagged];
        assert!(merge_verified_alias(&mut tracks, old, new));
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].artist, "Original Artist");
        assert_eq!(tracks[0].title, "Original Title");
        assert!(!merge_verified_alias(&mut tracks, new, old));
        assert_eq!(tracks.len(), 1);
        let mut tracks = vec![Track::placeholder(old.into())];
        assert!(merge_verified_alias(&mut tracks, old, new));
        assert_eq!(tracks[0].source, new);
    }
    use super::*;
    #[test]
    fn remote_placeholders_decode_once_and_migrate_without_touching_tags_or_urls() {
        let source = "ftp://phone/Music/Album/02%20-%20Citt%C3%A0%20%2520.flac";
        let mut track = Track::placeholder(source.into());
        assert_eq!(track.title, "02 - Città %20");
        assert_eq!(track.source, source);
        track.title = "02%20-%20Citt%C3%A0%20%2520".into();
        assert!(track.repair_placeholder());
        assert_eq!(track.title, "02 - Città %20");
        let mut tagged = Track::placeholder(source.into());
        tagged.title = "Genuine %20 title".into();
        tagged.metadata_loaded = true;
        assert!(!tagged.repair_placeholder());
        assert_eq!(tagged.title, "Genuine %20 title");
        assert_eq!(
            Track::placeholder("/nas/02%20literal.flac".into()).title,
            "02%20literal"
        );
    }

    #[test]
    fn facet_metadata_is_read_from_real_tags_without_modifying_the_file() {
        use lofty::{
            config::WriteOptions,
            tag::{Accessor, ItemKey, Tag, TagExt, TagType},
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tagged.wav");
        let mut writer = hound::WavWriter::create(
            &path,
            hound::WavSpec {
                channels: 2,
                sample_rate: 44100,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for _ in 0..4410 {
            writer.write_sample(0_i16).unwrap();
            writer.write_sample(0_i16).unwrap();
        }
        writer.finalize().unwrap();
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_title("Session One".into());
        tag.set_artist("Test Ensemble".into());
        tag.set_album("Sessions".into());
        tag.set_genre("Jazz".into());
        tag.set_disk(2);
        tag.set_track(3);
        tag.insert_text(ItemKey::AlbumArtist, "Various Test Artists".into());
        tag.insert_text(ItemKey::RecordingDate, "2024-04-10".into());
        tag.save_to_path(&path, WriteOptions::default()).unwrap();
        let before = std::fs::read(&path).unwrap();
        let t = Track::read(path.to_string_lossy().into_owned());
        assert_eq!(
            (
                t.artist.as_str(),
                t.album_artist.as_str(),
                t.genre.as_str(),
                t.year,
                t.disc,
                t.number
            ),
            (
                "Test Ensemble",
                "Various Test Artists",
                "Jazz",
                Some(2024),
                2,
                3
            )
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    #[test]
    fn playlist_preserves_order_resolves_unicode_and_rejects_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("album.m3u8");
        std::fs::write(&path, "\u{feff}#EXTM3U\n01 Città.flac\n02 Finale.wav\n01 Città.flac\nftp://fixture:fixture-only@nas.invalid/track.flac\nhttps://nas/music/03.flac\n").unwrap();
        let tracks = playlist(&path).unwrap();
        assert_eq!(tracks.len(), 3);
        assert_eq!(
            tracks[0],
            dir.path().join("01 Città.flac").to_string_lossy()
        );
        assert!(tracks[2].starts_with("https://"));
        assert!(validate_url("https://nas/t?token=secret").is_err());
        assert!(validate_url("file:///etc/passwd").is_err());
    }
}

#[derive(Clone)]
pub struct Artwork {
    pub size: [usize; 2],
    pub rgba: std::sync::Arc<[u8]>,
    pub origin: Option<(String, String)>,
}

pub fn decode_art(bytes: &[u8]) -> Option<Artwork> {
    decode_art_sized(bytes, 800)
}

pub fn decode_art_sized(bytes: &[u8], max_side: u32) -> Option<Artwork> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let decoded = reader.decode().ok()?;
    let image = decoded
        .thumbnail(
            max_side.min(decoded.width()),
            max_side.min(decoded.height()),
        )
        .to_rgba8();
    Some(Artwork {
        size: [image.width() as usize, image.height() as usize],
        rgba: image.into_raw().into(),
        origin: None,
    })
}

/// Keep an album together; absent tags fall back to its containing folder.
/// Separate folders keep distinct editions (and unknown albums) independent.
pub fn random_album_next<'a>(
    tracks: impl Iterator<Item = &'a Track>,
    current: Option<&str>,
) -> Option<String> {
    use rand::seq::IteratorRandom;
    let tracks: Vec<_> = tracks.collect();
    let parent = |t: &Track| t.source.rsplit_once('/').map_or("", |(p, _)| p).to_string();
    let mut complete = std::collections::HashMap::new();
    for track in &tracks {
        let tagged = complete.entry(parent(track)).or_insert(true);
        *tagged &= !track.album.is_empty();
    }
    let mut albums = std::collections::BTreeMap::new();
    for track in tracks {
        let folder = parent(track);
        let album = if complete[&folder] {
            track.album.clone()
        } else {
            String::new()
        };
        albums
            .entry((folder, album))
            .or_insert_with(Vec::new)
            .push(track);
    }
    for album in albums.values_mut() {
        let numbered = album.iter().all(|t| t.number > 0);
        album.sort_by(|a, b| {
            (if numbered {
                a.number.cmp(&b.number)
            } else {
                std::cmp::Ordering::Equal
            })
            .then_with(|| a.source.cmp(&b.source))
        });
    }
    let active = albums.iter().find_map(|(key, album)| {
        album
            .iter()
            .position(|t| Some(t.source.as_str()) == current)
            .map(|i| (key, album, i))
    });
    if let Some((_, album, i)) = active
        && let Some(next) = album.get(i + 1)
    {
        return Some(next.source.clone());
    }
    let selected = albums
        .iter()
        .filter(|(k, _)| active.is_none_or(|(current, _, _)| *k != current))
        .choose(&mut rand::rng());
    selected
        .or_else(|| albums.iter().next())
        .and_then(|(_, album)| album.first())
        .map(|t| t.source.clone())
}

#[cfg(test)]
mod album_order_tests {
    use super::*;
    #[test]
    fn random_albums_preserve_track_order_and_do_not_mix_folders() {
        let mut a2 = Track::placeholder("/phone/A/02.flac".into());
        a2.album = "A".into();
        a2.number = 2;
        let mut a1 = a2.clone();
        a1.source = "/phone/A/01.flac".into();
        a1.number = 1;
        let b = Track::placeholder("/phone/B/01.flac".into());
        let tracks = [a2, b, a1];
        assert_eq!(
            random_album_next(tracks.iter(), Some("/phone/A/01.flac")),
            Some("/phone/A/02.flac".into())
        );
        for _ in 0..30 {
            assert_eq!(
                random_album_next(tracks.iter(), Some("/phone/A/02.flac")),
                Some("/phone/B/01.flac".into())
            );
            assert_eq!(
                random_album_next(tracks.iter(), Some("/phone/B/01.flac")),
                Some("/phone/A/01.flac".into())
            );
            assert_ne!(
                random_album_next(tracks.iter(), None),
                Some("/phone/A/02.flac".into())
            );
        }
        let mut partial = tracks.clone();
        partial[0].album.clear();
        partial[0].number = 0;
        assert_eq!(
            random_album_next(partial.iter(), Some("/phone/A/01.flac")),
            Some("/phone/A/02.flac".into())
        );
        assert_eq!(random_album_next([].iter(), None), None);
    }
}
