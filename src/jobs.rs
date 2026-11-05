use crate::{
    audio, cache,
    library::{self, MAX_TRACKS, Track},
    network::{AuthStore, Entry},
};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::Sender,
    },
};

pub mod gallery;

pub enum Job {
    ConnectionChecked {
        token: u64,
        ok: bool,
    },
    Credential {
        id: String,
        username: String,
        password: Option<String>,
    },
    NetworkSnapshot(Vec<crate::discovery::Lan>),
    Rediscovered {
        request: u64,
        outcome: crate::discovery::Outcome,
    },
    SyncDone(crate::sync::Report),
    Tracks(Vec<Track>),
    Tags(Track, u64),
    TagAlias {
        source: String,
        target: String,
        ticket: u64,
    },
    TagsDone(u64, crate::remote_tags::Completion),
    Done(usize),
    Problem(String),
    Directory(String, Result<Vec<Entry>, String>),
    Ready(Track, Option<crate::streaming::Pcm>, u64, Option<u64>),
    Prepared(Track, Option<library::Artwork>, u64),
}

pub fn import(paths: Vec<PathBuf>, tx: Sender<Job>, cancel: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let mut batch = Vec::new();
        let mut count = 0;
        let mut errors = 0;
        let mut seen = HashSet::new();
        for root in paths {
            if !root.is_dir() && !root.is_file() {
                errors += 1;
                continue;
            }

            let paths: Vec<_> = if root.is_dir() {
                walkdir::WalkDir::new(root)
                    .follow_links(false)
                    .sort_by_file_name()
                    .max_depth(32)
                    .into_iter()
                    .take_while(|_| !cancel.load(Ordering::Relaxed))
                    .filter_map(|e| match e {
                        Ok(e) => Some(e),
                        Err(_) => {
                            errors += 1;
                            None
                        }
                    })
                    .filter(|e| e.file_type().is_file() && library::audio_path(e.path()))
                    .take(MAX_TRACKS)
                    .map(|e| e.into_path())
                    .collect()
            } else {
                vec![root]
            };
            for path in paths {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                let playlist = path.extension().is_some_and(|e| {
                    e.eq_ignore_ascii_case("m3u") || e.eq_ignore_ascii_case("m3u8")
                });
                let sources = if playlist {
                    match library::playlist(&path) {
                        Ok(s) => s,
                        Err(e) => {
                            let _ = tx.send(Job::Problem(e));
                            continue;
                        }
                    }
                } else {
                    vec![path.to_string_lossy().into_owned()]
                };
                for source in sources {
                    if cancel.load(Ordering::Relaxed) || count >= MAX_TRACKS {
                        break;
                    }
                    if !seen.insert(source.clone()) {
                        continue;
                    }
                    batch.push(Track::read(source));
                    count += 1;
                    if batch.len() >= 32 {
                        let _ = tx.send(Job::Tracks(std::mem::take(&mut batch)));
                    }
                }
            }
        }
        if !batch.is_empty() {
            let _ = tx.send(Job::Tracks(batch));
        }
        let _ = tx.send(Job::Done(errors));
    });
}

#[cfg(test)]
pub fn ftp_scan(root: String, auth: AuthStore, tx: Sender<Job>, cancel: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let mut session = crate::network::ListingSession::new();
        let mut stack = vec![(root, 0)];
        let mut visited = HashSet::new();
        let mut count = 0;
        let mut errors = 0;
        let mut batch = Vec::new();
        while let Some((directory, depth)) = stack.pop() {
            if cancel.load(Ordering::Relaxed) || count >= MAX_TRACKS {
                break;
            }
            if depth > 24 || !visited.insert(directory.clone()) {
                continue;
            }
            match session.list(&directory, &auth, &cancel) {
                Ok(mut entries) => {
                    entries.sort_by(|a, b| a.name.cmp(&b.name));
                    let mut folders = Vec::new();
                    for entry in entries {
                        if let Some(url) =
                            crate::network::child_url(&directory, &entry.name, entry.directory)
                        {
                            if entry.directory {
                                folders.push((url, depth + 1));
                            } else if library::audio_path(Path::new(&entry.name)) {
                                batch.push(Track::placeholder(url));
                                count += 1;
                                if batch.len() >= 32 {
                                    let _ = tx.send(Job::Tracks(std::mem::take(&mut batch)));
                                }
                                if count >= MAX_TRACKS {
                                    break;
                                }
                            }
                        }
                    }
                    stack.extend(folders.into_iter().rev());
                }
                Err(e) => {
                    errors += 1;
                    if visited.len() == 1 {
                        let _ = tx.send(Job::Problem(e));
                    }
                }
            }
        }
        if !batch.is_empty() {
            let _ = tx.send(Job::Tracks(batch));
        }
        let _ = tx.send(Job::Done(errors));
    });
}

#[cfg(test)]
pub fn artwork(
    path: &Path,
    source: &str,
    root: &Path,
    auth: &AuthStore,
    generation: &AtomicU64,
    ticket: u64,
) -> Option<library::Artwork> {
    artwork_for_track(
        path,
        &Track::placeholder(source.into()),
        root,
        auth,
        generation,
        ticket,
    )
}

pub fn artwork_for_track(
    path: &Path,
    track: &Track,
    root: &Path,
    auth: &AuthStore,
    generation: &AtomicU64,
    ticket: u64,
) -> Option<library::Artwork> {
    let source = &track.source;
    if let Some(art) = audio::cover(path).and_then(|b| library::decode_art(&b)) {
        return Some(art);
    }
    let names = folder_images(source, auth, generation, ticket, Some(track));
    for name in names {
        if generation.load(Ordering::Relaxed) != ticket {
            return None;
        }
        let candidate = if library::is_remote(source) {
            let mut u = url::Url::parse(source).ok()?.join(".").ok()?.to_string();
            let mut parts = name.split('/').peekable();
            while let Some(part) = parts.next() {
                u = crate::network::child_url(&u, part, parts.peek().is_some())?;
            }
            cache::materialize_limited(&u, root, generation, ticket, auth, 8 * 1024 * 1024, |_| {})
                .ok()
        } else {
            Path::new(source).parent().map(|p| p.join(&name))
        };
        if let Some(path) = candidate
            && std::fs::metadata(&path).is_ok_and(|m| m.len() <= 8 * 1024 * 1024)
            && let Ok(bytes) = std::fs::read(path)
            && let Some(art) = library::decode_art(&bytes)
        {
            return Some(art);
        }
    }
    crate::album_art::stored(track, root)
}

// Accept explicit front-cover roles or an exact album identity, never an arbitrary scan.
fn cover_image_rank(name: &str, identity: Option<&Track>) -> Option<usize> {
    let p = Path::new(name);
    let ext = p.extension()?.to_str()?.to_ascii_lowercase();
    if !["jpg", "jpeg", "png"].contains(&ext.as_str()) {
        return None;
    }
    let stem = p.file_stem()?.to_str()?.to_lowercase();
    if let Some(rank) = [
        "cover",
        "folder",
        "front",
        "front cover",
        "album",
        "albumart",
        "albumartsmall",
        "fronte",
        "copertina",
        "f",
    ]
    .iter()
    .position(|s| *s == stem)
    {
        return Some(rank);
    }
    if stem.starts_with("cover.musiccraft-") {
        return Some(0);
    }
    if let Some(track) = identity.filter(|t| !t.album.trim().is_empty()) {
        let name = crate::listening::canonical(&stem);
        let album = crate::listening::canonical(&crate::listening::lookup_title(&track.album));
        let artist = if track.album_artist.is_empty() {
            &track.artist
        } else {
            &track.album_artist
        };
        if !album.is_empty()
            && (name == album || name == format!("{}{album}", crate::listening::canonical(artist)))
        {
            return Some(8);
        }
        if album.len() >= 3
            && name.strip_prefix(&album).is_some_and(|suffix| {
                ["finalcover", "cover", "frontcover", "front"]
                    .iter()
                    .any(|role| {
                        suffix.strip_prefix(role).is_some_and(|size| {
                            size.len() <= 4 && size.bytes().all(|b| b.is_ascii_digit())
                        })
                    })
            })
        {
            return Some(8);
        }
    }
    let words: Vec<_> = stem
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .collect();
    if words.iter().any(|w| {
        [
            "back", "rear", "retro", "inside", "inlay", "booklet", "disc", "cd", "proof",
        ]
        .contains(w)
    }) {
        return None;
    }
    if words
        .iter()
        .any(|w| ["front", "fronte", "avanti"].contains(w))
        || (words.len() == 2
            && words[0].bytes().all(|b| b.is_ascii_digit())
            && ["cover", "folder", "front"].contains(&words[1]))
        || ["coverjpg", "coverjpeg", "coverpng"]
            .contains(&crate::listening::canonical(&stem).as_str())
    {
        return Some(7);
    }
    ["thumbnail", "thumb"].contains(&stem.as_str()).then_some(9)
}

fn artwork_directory(name: &str) -> bool {
    [
        "art", "artwork", "artworks", "scan", "scans", "covers", "cover", "booklet",
    ]
    .contains(&name.to_ascii_lowercase().as_str())
}

fn folder_images(
    source: &str,
    auth: &AuthStore,
    generation: &AtomicU64,
    ticket: u64,
    identity: Option<&Track>,
) -> Vec<String> {
    let rank = |name: &str| cover_image_rank(name, identity);
    let mut names: Vec<_> = if library::is_remote(source) {
        let Some(root) = url::Url::parse(source).ok().and_then(|u| u.join(".").ok()) else {
            return vec![];
        };
        if matches!(root.scheme(), "http" | "https") {
            return [
                "cover.jpg",
                "folder.jpg",
                "cover.png",
                "Folder.jpg",
                "front.jpg",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect();
        }
        let mut session = crate::network::ListingSession::new();
        let entries = session
            .list(
                root.as_str(),
                auth,
                &AtomicBool::new(generation.load(Ordering::Relaxed) != ticket),
            )
            .unwrap_or_default();
        let mut found: Vec<_> = entries
            .iter()
            .filter(|e| !e.directory)
            .filter_map(|e| rank(&e.name).map(|r| (r, e.name.clone())))
            .collect();
        if found.is_empty() {
            for directory in entries
                .iter()
                .filter(|e| e.directory && artwork_directory(&e.name))
                .take(8)
            {
                if generation.load(Ordering::Relaxed) != ticket {
                    break;
                }
                let Some(child) = crate::network::child_url(root.as_str(), &directory.name, true)
                else {
                    continue;
                };
                for e in session
                    .list(
                        &child,
                        auth,
                        &AtomicBool::new(generation.load(Ordering::Relaxed) != ticket),
                    )
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|e| !e.directory)
                {
                    if let Some(r) = rank(&e.name) {
                        found.push((r + 16, format!("{}/{}", directory.name, e.name)));
                    }
                }
            }
        }
        found
    } else {
        let Some(parent) = Path::new(source).parent() else {
            return vec![];
        };
        let entries: Vec<_> = std::fs::read_dir(parent)
            .into_iter()
            .flatten()
            .filter_map(std::result::Result::ok)
            .collect();
        let mut found: Vec<_> = entries
            .iter()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .filter_map(|e| e.file_name().into_string().ok())
            .filter_map(|n| rank(&n).map(|r| (r, n)))
            .collect();
        if found.is_empty() {
            for directory in entries
                .iter()
                .filter(|e| {
                    e.file_type().is_ok_and(|t| t.is_dir())
                        && artwork_directory(&e.file_name().to_string_lossy())
                })
                .take(8)
            {
                if generation.load(Ordering::Relaxed) != ticket {
                    break;
                }
                for e in std::fs::read_dir(directory.path())
                    .into_iter()
                    .flatten()
                    .filter_map(std::result::Result::ok)
                    .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                {
                    if let Ok(name) = e.file_name().into_string()
                        && let Some(r) = rank(&name)
                    {
                        found.push((
                            r + 16,
                            format!("{}/{}", directory.file_name().to_string_lossy(), name),
                        ));
                    }
                }
            }
        }
        found
    };
    names.sort();
    names.truncate(12);
    names.into_iter().map(|(_, n)| n).collect()
}

#[derive(Default)]
pub struct PrepareOptions {
    pub artwork_only: bool,
    pub next_plan: Option<(Arc<AtomicU64>, u64)>,
    pub exclusive: Option<bool>,
    pub online_art: Option<Arc<AtomicBool>>,
    pub save_art: Option<Arc<AtomicBool>>,
}

#[cfg(test)]
mod progressive_regression {
    use super::*;
    #[test]
    #[ignore = "explicit throttled synthetic FTP fixture"]
    fn regression_long_ftp_preparation_has_bounded_startup() {
        let root = std::env::var("AUREA_TEST_FTP_URL").unwrap();
        let mut source = crate::network::FtpSource::smartphone();
        source.set_url(&root).unwrap();
        source.username = "fixture".into();
        source.password = "fixture-only".into();
        let auth = AuthStore::default();
        auth.insert(&source).unwrap();
        let generation = Arc::new(AtomicU64::new(1));
        let cache = tempfile::tempdir().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let start = std::time::Instant::now();
        prefetch(
            Track::placeholder(crate::network::child_url(&root, "Long.wav", false).unwrap()),
            cache.path().into(),
            auth,
            generation.clone(),
            1,
            tx,
            PrepareOptions {
                next_plan: Some((Arc::new(AtomicU64::new(1)), 1)),
                exclusive: Some(true),
                ..Default::default()
            },
        );
        let result = rx.recv_timeout(std::time::Duration::from_secs(3));
        let ready = matches!(result, Ok(Job::Ready(_, Some(_), 1, Some(1))));
        // Cancel before asserting so the old whole-file downloader is also cleaned up.
        generation.store(2, Ordering::Relaxed);
        println!(
            "throttled FTP preparation: ready={ready}, {} ms",
            start.elapsed().as_millis()
        );
        std::thread::sleep(std::time::Duration::from_millis(150));
        assert!(
            ready,
            "Whole-file loading blocked preparation of a one-hour track"
        );
    }
}

pub fn prefetch(
    mut track: Track,
    root: PathBuf,
    auth: AuthStore,
    generation: Arc<AtomicU64>,
    ticket: u64,
    tx: Sender<Job>,
    options: PrepareOptions,
) {
    std::thread::spawn(move || {
        let cancelled = || {
            generation.load(Ordering::Relaxed) != ticket
                || options
                    .next_plan
                    .as_ref()
                    .is_some_and(|(epoch, value)| epoch.load(Ordering::Relaxed) != *value)
        };
        let mut embedded = None;
        let path = if library::is_remote(&track.source) {
            PathBuf::new()
        } else {
            PathBuf::from(&track.source)
        };
        if !library::is_remote(&track.source) {
            track.update_metadata(&path);
        }
        if !options.artwork_only {
            match crate::streaming::Stream::prepare(
                &mut track,
                &root,
                &auth,
                crate::streaming::Cancellation::new(
                    generation.clone(),
                    ticket,
                    options.next_plan.clone(),
                ),
                options.exclusive == Some(true),
                None,
            ) {
                Ok(mut prepared) => {
                    embedded = prepared.cover.take();
                    if cancelled() {
                        return;
                    }
                    let _ = tx.send(Job::Ready(
                        track.clone(),
                        Some(prepared.into()),
                        ticket,
                        options.next_plan.as_ref().map(|(_, n)| *n),
                    ));
                }
                Err(e) => {
                    if !cancelled() {
                        let _ = tx.send(Job::Problem(format!("Precaricamento audio: {e}")));
                    }
                    return;
                }
            }
        }
        if !cancelled() {
            let cover = embedded
                .or_else(|| artwork_for_track(&path, &track, &root, &auth, &generation, ticket))
                .or_else(|| {
                    let flag = options.online_art.as_ref()?;
                    crate::album_art::online(&track, &root, &|| {
                        cancelled() || !flag.load(Ordering::Relaxed)
                    })
                });
            if !cancelled() {
                let save = cover.as_ref().filter(|a| a.origin.is_some()).cloned();
                let source = track.source.clone();
                let _ = tx.send(Job::Prepared(track, cover, ticket));
                if let (Some(art), Some(flag)) = (save, options.save_art.as_ref()) {
                    let _ = save_album_cover(&source, &art, &auth, &generation, ticket, &|| {
                        cancelled() || !flag.load(Ordering::Relaxed)
                    });
                }
            }
        }
    });
}

fn save_album_cover(
    source: &str,
    art: &library::Artwork,
    auth: &AuthStore,
    generation: &AtomicU64,
    ticket: u64,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<String>, String> {
    use image::ImageEncoder;
    use std::io::Write;
    if cancelled() || !folder_images(source, auth, generation, ticket, None).is_empty() {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(
            &art.rgba,
            art.size[0] as u32,
            art.size[1] as u32,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|_| "Copertina non codificabile")?;
    if cancelled() {
        return Ok(None);
    }
    if library::is_remote(source) {
        return crate::network::save_album_cover(source, &bytes, auth, cancelled).map(Some);
    }
    let parent = Path::new(source)
        .parent()
        .ok_or("Cartella album non disponibile")?;
    let temporary = parent.join(format!(".musiccraft-{:032x}.part", rand::random::<u128>()));
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|_| "Cartella album non scrivibile")?;
    let result = (|| {
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| "Salvataggio copertina interrotto")?;
        if cancelled() {
            return Ok(None);
        }
        match std::fs::hard_link(&temporary, parent.join("cover.png")) {
            Ok(()) => Ok(Some("cover.png".into())),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(None),
            Err(_) => Err(
                "Salvataggio senza sovrascrittura non disponibile; copertina nella cache".into(),
            ),
        }
    })();
    drop(file);
    let _ = std::fs::remove_file(temporary);
    result
}

#[cfg(test)]
mod cover_tests {
    use super::*;
    #[test]
    fn artwork_subfolders_are_bounded_and_prefer_explicit_front() {
        let root = tempfile::tempdir().unwrap();
        let scans = root.path().join("Scans");
        std::fs::create_dir(&scans).unwrap();
        std::fs::write(scans.join("Front.jpg"), picture([20, 120, 80, 255])).unwrap();
        std::fs::write(scans.join("Back.jpg"), picture([255, 0, 0, 255])).unwrap();
        let track = Track::placeholder(root.path().join("01.flac").to_string_lossy().into_owned());
        let auth = AuthStore::default();
        let generation = AtomicU64::new(1);
        let art = artwork_for_track(
            Path::new(&track.source),
            &track,
            root.path(),
            &auth,
            &generation,
            1,
        )
        .unwrap();
        assert_eq!(&art.rgba[..4], &[20, 120, 80, 255]);
        std::fs::remove_file(scans.join("Front.jpg")).unwrap();
        let deeper = scans.join("Artwork");
        std::fs::create_dir(&deeper).unwrap();
        std::fs::write(deeper.join("Front.jpg"), picture([0, 0, 255, 255])).unwrap();
        assert!(
            artwork_for_track(
                Path::new(&track.source),
                &track,
                root.path(),
                &auth,
                &generation,
                1
            )
            .is_none()
        );
    }
    #[test]
    fn descriptive_front_and_album_filenames_work_without_selecting_other_scans() {
        let dir = tempfile::tempdir().unwrap();
        let mut track =
            Track::placeholder(dir.path().join("01.flac").to_string_lossy().into_owned());
        track.artist = "Fink".into();
        track.album = "Perfect Darkness".into();
        let bytes = picture([20, 120, 80, 255]);
        std::fs::write(dir.path().join("Fink - Perfect Darkness.jpeg"), &bytes).unwrap();
        std::fs::write(
            dir.path().join("Back Booklet Scan.jpg"),
            picture([255, 0, 0, 255]),
        )
        .unwrap();
        let art = artwork_for_track(
            Path::new(&track.source),
            &track,
            dir.path(),
            &AuthStore::default(),
            &AtomicU64::new(1),
            1,
        )
        .unwrap();
        assert_eq!(&art.rgba[..4], &[20, 120, 80, 255]);
        for name in [
            "Selling_England_By_The_Pound-Front.jpg",
            "High Tide I - Slipcase - Front.jpg",
            "Cover JPG.jpg",
            "thumbnail.jpg",
        ] {
            assert!(cover_image_rank(name, Some(&track)).is_some(), "{name}");
        }
        for name in [
            "Fink - Other Album.jpeg",
            "Back Booklet Scan.jpg",
            "Front-Inside.png",
            "Disc.png",
            "02.png",
            "front.exe",
        ] {
            assert!(cover_image_rank(name, Some(&track)).is_none(), "{name}");
        }
        track.artist = "AC/DC".into();
        track.album = "Back in Black".into();
        assert!(cover_image_rank("Back in Black.jpg", Some(&track)).is_some());
        track.artist = "Ana Tijoux".into();
        track.album = "1977".into();
        assert!(cover_image_rank("1977FinalCover1400.jpg", Some(&track)).is_some());
        assert!(cover_image_rank("1997FinalCover1400.jpg", Some(&track)).is_none());
        for name in [
            "Fronte.jpg",
            "Copertina-Nerkiology-Avanti.jpg",
            "00 cover.jpg",
            "The Who Front Greatest Hits.jpg",
            "F.jpg",
            "thumb.jpg",
        ] {
            assert!(cover_image_rank(name, Some(&track)).is_some(), "{name}");
        }
        for name in [
            "Copertina-Nerkiology-Retro.jpg",
            "album-proof.jpg",
            "The Who Back Greatest Hits.jpg",
        ] {
            assert!(cover_image_rank(name, Some(&track)).is_none(), "{name}");
        }
    }
    fn picture(color: [u8; 4]) -> Vec<u8> {
        use image::ImageEncoder;
        let mut bytes = Vec::new();
        image::codecs::png::PngEncoder::new(&mut bytes)
            .write_image(
                &color.repeat(16 * 16),
                16,
                16,
                image::ExtendedColorType::Rgba8,
            )
            .unwrap();
        bytes
    }
    #[test]
    fn embedded_front_wins_then_case_insensitive_folder_and_existing_files_are_preserved() {
        use lofty::{
            config::WriteOptions,
            picture::{MimeType, Picture, PictureType},
            tag::{Tag, TagExt, TagType},
        };
        let dir = tempfile::tempdir().unwrap();
        let track = dir.path().join("01.wav");
        let mut writer = hound::WavWriter::create(
            &track,
            hound::WavSpec {
                channels: 2,
                sample_rate: 44100,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for _ in 0..100 {
            writer.write_sample(0_i16).unwrap();
        }
        writer.finalize().unwrap();
        let blue = picture([10, 20, 200, 255]);
        let red = picture([200, 20, 10, 255]);
        std::fs::write(dir.path().join("Front.JPEG"), &blue).unwrap();
        let auth = AuthStore::default();
        let gen_id = AtomicU64::new(1);
        assert_eq!(
            artwork(
                &track,
                track.to_str().unwrap(),
                dir.path(),
                &auth,
                &gen_id,
                1
            )
            .unwrap()
            .rgba[..4],
            [10, 20, 200, 255]
        );
        let mut tag = Tag::new(TagType::Id3v2);
        tag.push_picture(
            Picture::unchecked(blue.clone())
                .mime_type(MimeType::Png)
                .pic_type(PictureType::CoverBack)
                .build(),
        );
        tag.push_picture(
            Picture::unchecked(red.clone())
                .mime_type(MimeType::Png)
                .pic_type(PictureType::CoverFront)
                .build(),
        );
        tag.save_to_path(&track, WriteOptions::default()).unwrap();
        assert_eq!(
            artwork(
                &track,
                track.to_str().unwrap(),
                dir.path(),
                &auth,
                &gen_id,
                1
            )
            .unwrap()
            .rgba[..4],
            [200, 20, 10, 255]
        );
        let art = library::decode_art(&red).unwrap();
        assert!(
            save_album_cover(track.to_str().unwrap(), &art, &auth, &gen_id, 1, &|| false)
                .unwrap()
                .is_none()
        );
        assert_eq!(std::fs::read(dir.path().join("Front.JPEG")).unwrap(), blue);
        std::fs::remove_file(dir.path().join("Front.JPEG")).unwrap();
        assert_eq!(
            save_album_cover(track.to_str().unwrap(), &art, &auth, &gen_id, 1, &|| false)
                .unwrap()
                .as_deref(),
            Some("cover.png")
        );
        let before = std::fs::read(dir.path().join("cover.png")).unwrap();
        assert!(
            save_album_cover(
                track.to_str().unwrap(),
                &library::decode_art(&blue).unwrap(),
                &auth,
                &gen_id,
                1,
                &|| false
            )
            .unwrap()
            .is_none()
        );
        assert_eq!(before, std::fs::read(dir.path().join("cover.png")).unwrap());
        assert!(
            !std::fs::read_dir(dir.path())
                .unwrap()
                .flatten()
                .any(|e| e.path().extension().is_some_and(|s| s == "part"))
        );
    }
    #[test]
    #[ignore = "Requires isolated writable FTP fixture"]
    fn live_ftp_nested_cover_handles_encoded_names_and_preserves_existing_art() {
        let root = std::env::var("AUREA_TEST_FTP_URL").unwrap();
        let mut config = crate::network::FtpSource::smartphone();
        config.set_url(&root).unwrap();
        config.username = "fixture".into();
        config.password = "fixture-only".into();
        let auth = AuthStore::default();
        auth.insert(&config).unwrap();
        let source = crate::network::child_url(&root, "Artwork Only", true).unwrap() + "01.flac";
        let track = Track::placeholder(source.clone());
        let cache = tempfile::tempdir().unwrap();
        let generation = AtomicU64::new(1);
        let art = artwork_for_track(
            Path::new("/synthetic/no-audio"),
            &track,
            cache.path(),
            &auth,
            &generation,
            1,
        )
        .unwrap();
        assert!(art.size.iter().all(|side| *side > 0 && *side <= 800));
        assert_eq!(&art.rgba[..4], &[50, 75, 95, 255]);
        assert!(
            save_album_cover(&source, &art, &auth, &generation, 1, &|| false)
                .unwrap()
                .is_none()
        );
    }
    #[test]
    #[ignore = "Requires isolated writable FTP fixture"]
    fn live_ftp_cover_save_is_unique_and_does_not_overwrite_existing_art() {
        let root = std::env::var("AUREA_TEST_FTP_URL").unwrap();
        let disk = PathBuf::from(std::env::var("AUREA_TEST_FTP_ROOT").unwrap());
        let source = crate::network::child_url(&root, "Upload Album", true).unwrap() + "01.flac";
        let mut config = crate::network::FtpSource::smartphone();
        config.set_url(&root).unwrap();
        config.username = "fixture-write".into();
        config.password = "fixture-only".into();
        let auth = AuthStore::default();
        auth.insert(&config).unwrap();
        let gen_id = AtomicU64::new(1);
        let art = library::decode_art(&picture([20, 120, 80, 255])).unwrap();
        let name = save_album_cover(&source, &art, &auth, &gen_id, 1, &|| false)
            .unwrap()
            .unwrap();
        assert!(name.starts_with("cover.MusicCraft-"));
        assert!(disk.join("Upload Album").join(&name).is_file());
        let before = std::fs::read(disk.join("Upload Album").join(&name)).unwrap();
        assert!(
            save_album_cover(&source, &art, &auth, &gen_id, 1, &|| false)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            before,
            std::fs::read(disk.join("Upload Album").join(name)).unwrap()
        );
        assert_eq!(
            std::fs::read_dir(disk.join("Upload Album"))
                .unwrap()
                .count(),
            1
        );
        config.username = "fixture".into();
        auth.insert(&config).unwrap();
        let denied = crate::network::child_url(&root, "Read Only Album", true).unwrap() + "01.flac";
        assert!(save_album_cover(&denied, &art, &auth, &gen_id, 1, &|| false).is_err());
        assert_eq!(
            std::fs::read_dir(disk.join("Read Only Album"))
                .unwrap()
                .count(),
            0
        );
    }
}
