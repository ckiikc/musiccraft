//! Bounded, read-only enumeration and on-demand loading of album artwork.
use super::*;
use std::time::{Duration, Instant};

const MAX_IMAGES: usize = 128;
const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Clone)]
pub struct Item {
    pub source: String,
    pub name: String,
    pub front: bool,
}

fn image_file(name: &str) -> bool {
    !name.contains(['/', '\\'])
        && Path::new(name)
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|s| ["jpg", "jpeg", "png"].contains(&s.to_ascii_lowercase().as_str()))
}

fn child(root: &str, name: &str, directory: bool) -> Option<String> {
    if library::is_remote(root) {
        crate::network::child_url(root, name, directory)
    } else {
        Some(Path::new(root).join(name).to_string_lossy().into_owned())
    }
}

fn entries(
    root: &str,
    session: &mut crate::network::ListingSession,
    auth: &AuthStore,
    cancel: &AtomicBool,
) -> Vec<Entry> {
    if cancel.load(Ordering::Relaxed) {
        return vec![];
    }
    if library::is_remote(root) {
        session.list(root, auth, cancel).unwrap_or_default()
    } else {
        std::fs::read_dir(root)
            .into_iter()
            .flatten()
            .take(4096)
            .filter_map(std::result::Result::ok)
            .filter_map(|entry| {
                let kind = entry.file_type().ok()?;
                if !kind.is_file() && !kind.is_dir() {
                    return None;
                }
                Some(Entry {
                    name: entry.file_name().into_string().ok()?,
                    directory: kind.is_dir(),
                    modified: None,
                })
            })
            .collect()
    }
}

fn natural_key(name: &str) -> String {
    let lower = name.to_lowercase();
    let mut chars = lower.chars().peekable();
    let mut result = String::new();
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() {
            let mut digits = String::from(c);
            while chars.peek().is_some_and(char::is_ascii_digit) {
                digits.push(chars.next().unwrap_or('0'));
            }
            let digits = digits.trim_start_matches('0');
            result.push_str(&format!("{:04}{digits}", digits.len()));
        } else {
            result.push(c);
        }
    }
    result
}

pub fn list(track: &Track, auth: &AuthStore, cancel: &AtomicBool) -> Vec<Item> {
    let root = if library::is_remote(&track.source) {
        let Some(url) = url::Url::parse(&track.source)
            .ok()
            .and_then(|u| u.join(".").ok())
        else {
            return vec![];
        };
        if !matches!(url.scheme(), "ftp" | "ftps") {
            return vec![];
        }
        url.to_string()
    } else {
        let Some(parent) = Path::new(&track.source).parent() else {
            return vec![];
        };
        parent.to_string_lossy().into_owned()
    };
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut session = crate::network::ListingSession::with_timeout(Duration::from_secs(3));
    let top = entries(&root, &mut session, auth, cancel);
    let mut folders = vec![(root.clone(), String::new())];
    folders.extend(
        top.iter()
            .filter(|e| e.directory && artwork_directory(&e.name) && !e.name.contains(['/', '\\']))
            .take(8)
            .filter_map(|e| child(&root, &e.name, true).map(|path| (path, e.name.clone()))),
    );
    let mut found = Vec::new();
    for (index, (folder, prefix)) in folders.into_iter().enumerate() {
        if cancel.load(Ordering::Relaxed) || Instant::now() >= deadline {
            break;
        }
        let rows = if index == 0 {
            top.clone()
        } else {
            entries(&folder, &mut session, auth, cancel)
        };
        for entry in rows
            .into_iter()
            .filter(|e| !e.directory && image_file(&e.name))
        {
            let Some(source) = child(&folder, &entry.name, false) else {
                continue;
            };
            found.push(Item {
                source,
                front: cover_image_rank(&entry.name, Some(track)).is_some(),
                name: if prefix.is_empty() {
                    entry.name
                } else {
                    format!("{prefix}/{}", entry.name)
                },
            });
            if found.len() >= MAX_IMAGES {
                break;
            }
        }
        if found.len() >= MAX_IMAGES {
            break;
        }
    }
    found.sort_by_cached_key(|item| {
        let lower = item.name.to_lowercase();
        let rank = if item.front {
            0
        } else if ["back", "rear", "retro"]
            .iter()
            .any(|word| lower.contains(word))
        {
            1
        } else {
            2
        };
        (rank, natural_key(&item.name))
    });
    found
}

pub fn load(
    item: &Item,
    cache_root: &Path,
    auth: &AuthStore,
    generation: &AtomicU64,
    ticket: u64,
) -> Option<library::Artwork> {
    if generation.load(Ordering::Relaxed) != ticket {
        return None;
    }
    let path = if library::is_remote(&item.source) {
        cache::materialize_limited(
            &item.source,
            cache_root,
            generation,
            ticket,
            auth,
            MAX_IMAGE_BYTES,
            |_| {},
        )
        .ok()?
    } else {
        PathBuf::from(&item.source)
    };
    if !std::fs::metadata(&path).is_ok_and(|m| m.len() <= MAX_IMAGE_BYTES) {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    if generation.load(Ordering::Relaxed) != ticket {
        return None;
    }
    library::decode_art_sized(&bytes, 2048)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(path: &Path, size: u32) {
        image::RgbaImage::from_pixel(size, size, image::Rgba([40, 90, 150, 255]))
            .save(path)
            .unwrap();
    }

    #[test]
    fn gallery_lists_front_back_and_booklet_in_order_and_loads_large_images_on_demand() {
        let dir = tempfile::tempdir().unwrap();
        let scans = dir.path().join("Scans");
        std::fs::create_dir(&scans).unwrap();
        picture(&dir.path().join("front.png"), 1600);
        picture(&dir.path().join("back.png"), 32);
        picture(&scans.join("page10.png"), 32);
        picture(&scans.join("page2.png"), 32);
        std::fs::create_dir(scans.join("nested")).unwrap();
        picture(&scans.join("nested/ignored.png"), 32);
        std::fs::create_dir(dir.path().join("unrelated")).unwrap();
        picture(&dir.path().join("unrelated/ignored.png"), 32);
        std::fs::write(dir.path().join("bad.png"), b"invalid image").unwrap();
        let track =
            Track::placeholder(dir.path().join("track.flac").to_string_lossy().into_owned());
        let auth = AuthStore::default();
        let cancel = AtomicBool::new(false);
        let pages = list(&track, &auth, &cancel);
        assert_eq!(
            pages.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            [
                "front.png",
                "back.png",
                "bad.png",
                "Scans/page2.png",
                "Scans/page10.png"
            ]
        );
        let generation = AtomicU64::new(1);
        assert_eq!(
            load(&pages[0], dir.path(), &auth, &generation, 1)
                .unwrap()
                .size,
            [1600, 1600]
        );
        assert!(load(&pages[0], dir.path(), &auth, &generation, 2).is_none());
        assert!(load(&pages[2], dir.path(), &auth, &generation, 1).is_none());
        cancel.store(true, Ordering::Relaxed);
        assert!(list(&track, &auth, &cancel).is_empty());
    }

    #[test]
    #[ignore = "requires scripts/test_ftp.py fixture server"]
    fn live_ftp_gallery_reads_front_and_booklet_even_when_front_is_in_the_album_root() {
        let base = std::env::var("AUREA_TEST_FTP_URL").unwrap();
        let mut source = crate::network::FtpSource::smartphone();
        source.set_url(&base).unwrap();
        source.username = "fixture".into();
        source.password = "fixture-only".into();
        let auth = AuthStore::default();
        auth.insert(&source).unwrap();
        let root = crate::network::child_url(&base, "Gallery [Case]", true).unwrap();
        let track =
            Track::placeholder(crate::network::child_url(&root, "track.flac", false).unwrap());
        let pages = list(&track, &auth, &AtomicBool::new(false));
        assert_eq!(
            pages.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            [
                "front.png",
                "back.png",
                "Scans/page2.png",
                "Scans/page10.png"
            ]
        );
        let dir = tempfile::tempdir().unwrap();
        let generation = AtomicU64::new(1);
        for page in &pages {
            assert_eq!(
                load(page, dir.path(), &auth, &generation, 1).unwrap().size,
                [64, 64]
            );
        }
    }
}
