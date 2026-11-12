//! Read-only discovery: no media downloads, no removal of indexed tracks.
use crate::{
    library::{self, Track},
    network::{self, AuthStore},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const INTERVAL: Duration = Duration::from_secs(120);
const AUDIT: u64 = 30 * 60;
const MAX_DIRS: usize = 16_384;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Ftp { id: String, root: String },
    Folder(PathBuf),
}
impl Target {
    pub fn key(&self) -> String {
        match self {
            Self::Ftp { id, root } => format!("ftp:{id}:{root}"),
            Self::Folder(p) => format!("folder:{}", p.display()),
        }
    }
    fn root(&self) -> String {
        match self {
            Self::Ftp { root, .. } => root.clone(),
            Self::Folder(p) => p.to_string_lossy().into_owned(),
        }
    }
}
struct Schedule {
    next: Instant,
    failures: u32,
}
#[derive(Default)]
pub struct Scheduler {
    entries: BTreeMap<String, Schedule>,
}
impl Scheduler {
    pub fn reset(&mut self) {
        self.entries.clear();
    }
    pub fn due(&mut self, targets: &[Target], now: Instant) -> Option<Target> {
        self.entries
            .retain(|key, _| targets.iter().any(|t| t.key() == *key));
        for target in targets {
            self.entries.entry(target.key()).or_insert(Schedule {
                next: now + Duration::from_secs(5),
                failures: 0,
            });
        }
        targets
            .iter()
            .filter(|t| self.entries[&t.key()].next <= now)
            .min_by_key(|t| self.entries[&t.key()].next)
            .cloned()
    }
    pub fn finished(&mut self, target: &Target, now: Instant, success: bool) {
        let slot = self.entries.entry(target.key()).or_insert(Schedule {
            next: now,
            failures: 0,
        });
        slot.failures = if success {
            0
        } else {
            (slot.failures + 1).min(6)
        };
        let delay = if success {
            INTERVAL
        } else {
            Duration::from_secs((15 * (1 << slot.failures)).min(600))
        };
        slot.next = now + delay;
    }
}
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Index {
    root: String,
    last_full: u64,
    indexed_tracks: usize,
    directories: BTreeMap<String, Directory>,
}
#[derive(Serialize, Deserialize)]
struct Directory {
    stamp: Option<String>,
    leaf: bool,
}
#[derive(Default, Debug)]
pub struct Stats {
    pub listed: usize,
    pub skipped: usize,
    pub errors: usize,
    pub root_unreachable: bool,
    pub suspended: bool,
}
pub struct Report {
    pub target: Target,
    pub tracks: Vec<Track>,
    pub absent: HashSet<String>,
    pub stats: Stats,
    pub cancelled: bool,
}
fn seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Every container is visited. Only known leaves with a fresh parent-provided stamp
/// can be skipped; never infer an entire subtree is unchanged from its root mtime.
pub fn scan(
    target: Target,
    auth: &AuthStore,
    known: &HashSet<String>,
    index: &mut Index,
    force: bool,
    cancel: &AtomicBool,
) -> Report {
    let root = target.root();
    if index.root != root {
        *index = Index {
            root: root.clone(),
            ..Default::default()
        };
    }
    let scope = match &target {
        Target::Ftp { root, .. } => library::Scope::Remote(root.clone()),
        Target::Folder(p) => library::Scope::Local(p.clone()),
    };
    let indexed = known.iter().filter(|path| scope.contains(path)).count();
    let previous = known;
    let known: HashSet<_> = previous.iter().map(|id| library::source_key(id)).collect();
    // A crash may persist this cache before the UI has saved its newly discovered tracks.
    // Never skip leaves when the library is behind the cache.
    let full = force
        || indexed < index.indexed_tracks
        || index.last_full == 0
        || seconds().saturating_sub(index.last_full) >= AUDIT;
    let mut report = Report {
        target: target.clone(),
        tracks: Vec::new(),
        absent: HashSet::new(),
        stats: Stats::default(),
        cancelled: false,
    };
    if root.starts_with("https://") {
        match crate::phone::catalog(&root, auth, cancel, Duration::from_secs(25)) {
            Ok(tracks) if !cancel.load(Ordering::Relaxed) => {
                let present: HashSet<_> = tracks.iter().map(|t| t.source.clone()).collect();
                report.absent = previous
                    .iter()
                    .filter(|p| scope.contains(p) && !present.contains(*p))
                    .cloned()
                    .collect();
                report.tracks = tracks;
                report.stats.listed = 1;
                index.last_full = seconds();
                index.indexed_tracks = present.len();
            }
            Ok(_) => report.cancelled = true,
            Err(_) => {
                report.stats.errors = 1;
                report.stats.suspended =
                    crate::phone::online(&root, auth, cancel, Duration::from_secs(2)) == Ok(false);
                report.stats.root_unreachable = !report.stats.suspended;
                report.cancelled = cancel.load(Ordering::Relaxed);
            }
        }
        return report;
    }
    let mut visited = HashSet::new();
    let mut complete_dirs = HashSet::new();
    let mut listed_children = HashSet::new();
    let mut stack = vec![(root, None, 0)];
    let mut session = network::ListingSession::new();
    let mut consecutive_errors = 0;
    while let Some((directory, stamp, depth)) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            report.cancelled = true;
            break;
        }
        if visited.len() >= MAX_DIRS || known.len() + report.tracks.len() >= library::MAX_TRACKS {
            report.stats.errors += 1;
            break;
        }
        if depth > 32 || !visited.insert(directory.clone()) {
            continue;
        }
        if !full
            && stamp.is_some()
            && index
                .directories
                .get(&directory)
                .is_some_and(|old| old.leaf && old.stamp == stamp)
        {
            report.stats.skipped += 1;
            continue;
        }
        report.stats.listed += 1;
        let result =
            match &target {
                Target::Ftp { id, .. } => session.list_with_coverage(&directory, auth, cancel).map(
                    |(entries, complete)| {
                        let count = entries.len();
                        let children: Vec<_> = entries
                            .into_iter()
                            .filter_map(|entry| {
                                network::child_url(&directory, &entry.name, entry.directory)
                                    .map(|url| (url, entry.directory, entry.modified))
                            })
                            .collect();
                        if !id.is_empty() && complete && children.len() == count {
                            complete_dirs.insert(library::source_key(&directory));
                            listed_children.extend(
                                children
                                    .iter()
                                    .map(|(path, _, _)| library::source_key(path)),
                            );
                        }
                        children
                    },
                ),
                Target::Folder(_) => local_entries(Path::new(&directory)),
            };
        let mut entries = match result {
            Ok(entries) => {
                consecutive_errors = 0;
                entries
            }
            Err(_) => {
                if cancel.load(Ordering::Relaxed) {
                    report.cancelled = true;
                    break;
                }
                report.stats.errors += 1;
                report.stats.root_unreachable |= depth == 0;
                consecutive_errors += 1;
                // One failure at the root is sufficient to establish it is offline.
                if depth == 0 || consecutive_errors >= 3 {
                    break;
                }
                continue;
            }
        };
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        let leaf = !entries.iter().any(|e| e.1);
        let mut folders = Vec::new();
        for (path, directory, stamp) in entries {
            if directory {
                folders.push((path, stamp, depth + 1));
            } else if library::audio_path(Path::new(&path))
                && !known.contains(&library::source_key(&path))
            {
                if known.len() + report.tracks.len() >= library::MAX_TRACKS {
                    break;
                }
                report.tracks.push(if matches!(target, Target::Folder(_)) {
                    Track::read(path)
                } else {
                    Track::placeholder(path)
                });
            }
        }
        index
            .directories
            .insert(directory, Directory { stamp, leaf });
        stack.extend(folders.into_iter().rev());
    }
    if !report.cancelled && !cancel.load(Ordering::Relaxed) && !complete_dirs.is_empty() {
        let root = library::source_key(&target.root());
        report.absent.extend(
            previous
                .iter()
                .filter(|source| {
                    let key = library::source_key(source);
                    if !key.starts_with(&root) {
                        return false;
                    }
                    let mut child = key.as_str();
                    while child.len() > root.len() {
                        let Some((parent, _)) = child.trim_end_matches('/').rsplit_once('/') else {
                            break;
                        };
                        let parent = &child[..parent.len() + 1];
                        if complete_dirs.contains(parent) && !listed_children.contains(child) {
                            return true;
                        }
                        child = parent;
                    }
                    false
                })
                .cloned(),
        );
    }
    if !report.cancelled && report.stats.errors == 0 {
        index.directories.retain(|key, _| visited.contains(key));
        index.indexed_tracks = indexed.saturating_sub(report.absent.len()) + report.tracks.len();
        if full {
            index.last_full = seconds();
        }
    }
    report
}
fn local_entries(root: &Path) -> Result<Vec<(String, bool, Option<String>)>, String> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(root).map_err(|_| "Cartella non raggiungibile")? {
        let entry = entry.map_err(|_| "Cartella non leggibile")?;
        let kind = entry
            .file_type()
            .map_err(|_| "Tipo di file non disponibile")?;
        if kind.is_symlink() || (!kind.is_dir() && !kind.is_file()) {
            continue;
        }
        let stamp = kind
            .is_dir()
            .then(|| {
                entry
                    .metadata()
                    .ok()?
                    .modified()
                    .ok()?
                    .duration_since(UNIX_EPOCH)
                    .ok()
                    .map(|t| format!("{}:{}", t.as_secs(), t.subsec_nanos()))
            })
            .flatten();
        entries.push((
            entry.path().to_string_lossy().into_owned(),
            kind.is_dir(),
            stamp,
        ));
    }
    Ok(entries)
}
fn index_path(cache: &Path, target: &Target) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    target.key().hash(&mut hash);
    cache.join(format!("{:016x}.json", hash.finish()))
}
pub fn rebase_index(cache: &Path, old: &network::FtpSource, new: &network::FtpSource) {
    let (Ok(old_root), Ok(new_root)) = (old.url(), new.url()) else {
        return;
    };
    let old_path = index_path(
        cache,
        &Target::Ftp {
            id: old.id.clone(),
            root: old_root.clone(),
        },
    );
    let Ok(bytes) = std::fs::read(old_path) else {
        return;
    };
    if bytes.len() > 8 * 1024 * 1024 {
        return;
    }
    let Ok(mut index) = serde_json::from_slice::<Index>(&bytes) else {
        return;
    };
    if index.root != old_root || index.directories.len() > MAX_DIRS {
        return;
    }
    index.root = new_root.clone();
    index.directories = index
        .directories
        .into_iter()
        .filter_map(|(key, value)| {
            network::rebase(&key, &old_root, &new_root).map(|key| (key, value))
        })
        .collect();
    let path = index_path(
        cache,
        &Target::Ftp {
            id: new.id.clone(),
            root: new_root,
        },
    );
    if let Ok(bytes) = serde_json::to_vec(&index) {
        let temporary = path.with_extension("tmp");
        if std::fs::write(&temporary, bytes).is_ok() {
            let _ = std::fs::rename(temporary, path);
        }
    }
}
pub fn start(
    target: Target,
    auth: AuthStore,
    known: HashSet<String>,
    cache: PathBuf,
    force: bool,
    cancel: Arc<AtomicBool>,
    tx: std::sync::mpsc::Sender<crate::jobs::Job>,
) {
    std::thread::spawn(move || {
        let path = index_path(&cache, &target);
        let mut index = std::fs::read(&path)
            .ok()
            .filter(|b| b.len() <= 8 * 1024 * 1024)
            .and_then(|b| serde_json::from_slice::<Index>(&b).ok())
            .filter(|i| i.directories.len() <= MAX_DIRS)
            .unwrap_or_default();
        let report = scan(target, &auth, &known, &mut index, force, &cancel);
        if !report.cancelled
            && report.stats.errors == 0
            && std::fs::create_dir_all(&cache).is_ok()
            && let Ok(bytes) = serde_json::to_vec(&index)
        {
            let temporary = path.with_extension("tmp");
            if std::fs::write(&temporary, bytes).is_ok() {
                let _ = std::fs::rename(temporary, path);
            }
        }
        let _ = tx.send(crate::jobs::Job::SyncDone(report));
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn discovery_rebases_incremental_index_without_resetting_its_audit_or_touching_original() {
        let directory = tempfile::tempdir().unwrap();
        let mut old = network::FtpSource::smartphone();
        old.id = "fixture".into();
        old.host = "old.local".into();
        let mut new = old.clone();
        new.host = "new.local".into();
        let old_root = old.url().unwrap();
        let new_root = new.url().unwrap();
        let index = Index {
            root: old_root.clone(),
            last_full: 1234,
            indexed_tracks: 27,
            directories: BTreeMap::from([(
                format!("{old_root}Album/"),
                Directory {
                    stamp: Some("synthetic-time".into()),
                    leaf: true,
                },
            )]),
        };
        let old_path = index_path(
            directory.path(),
            &Target::Ftp {
                id: old.id.clone(),
                root: old_root,
            },
        );
        std::fs::write(&old_path, serde_json::to_vec(&index).unwrap()).unwrap();
        rebase_index(directory.path(), &old, &new);
        let path = index_path(
            directory.path(),
            &Target::Ftp {
                id: new.id.clone(),
                root: new_root.clone(),
            },
        );
        let rebased: Index = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(rebased.root, new_root);
        assert_eq!(rebased.last_full, 1234);
        assert_eq!(rebased.indexed_tracks, 27);
        assert!(
            rebased
                .directories
                .contains_key(&format!("{new_root}Album/"))
        );
        assert!(old_path.exists());
    }
    #[test]
    fn schedule_staggers_sources_backs_off_and_recovers_without_busy_retries() {
        let a = Target::Folder("/a".into());
        let b = Target::Folder("/b".into());
        let targets = [a.clone(), b.clone()];
        let mut scheduler = Scheduler::default();
        let now = Instant::now();
        assert!(scheduler.due(&targets, now).is_none());
        assert_eq!(
            scheduler.due(&targets, now + Duration::from_secs(5)),
            Some(a.clone())
        );
        scheduler.finished(&a, now + Duration::from_secs(5), false);
        assert_eq!(
            scheduler.due(&targets, now + Duration::from_secs(5)),
            Some(b.clone())
        );
        scheduler.finished(&b, now + Duration::from_secs(5), true);
        assert!(
            scheduler
                .due(&targets, now + Duration::from_secs(34))
                .is_none()
        );
        assert_eq!(
            scheduler.due(&targets, now + Duration::from_secs(35)),
            Some(a.clone())
        );
        scheduler.finished(&a, now + Duration::from_secs(35), false);
        assert_eq!(
            scheduler.entries[&a.key()].next,
            now + Duration::from_secs(95)
        );
        scheduler.finished(&a, now + Duration::from_secs(95), true);
        assert_eq!(
            scheduler.entries[&a.key()].next,
            now + Duration::from_secs(215)
        );
        scheduler.due(&[], now);
        assert!(scheduler.entries.is_empty());
    }
    #[test]
    fn local_incremental_scan_finds_deep_additions_skips_leaves_and_audits_stale_timestamps() {
        let dir = tempfile::tempdir().unwrap();
        let artist = dir.path().join("Artist");
        let a = artist.join("A");
        let b = artist.join("B");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(a.join("01.flac"), b"synthetic").unwrap();
        std::fs::write(b.join("01.flac"), b"synthetic").unwrap();
        let target = Target::Folder(dir.path().into());
        let mut index = Index::default();
        let mut known = HashSet::new();
        let cancel = AtomicBool::new(false);
        let first = scan(
            target.clone(),
            &AuthStore::default(),
            &known,
            &mut index,
            false,
            &cancel,
        );
        assert_eq!(first.tracks.len(), 2);
        assert_eq!(first.stats.listed, 4);
        let recovered = scan(
            target.clone(),
            &AuthStore::default(),
            &known,
            &mut index,
            false,
            &cancel,
        );
        assert_eq!(
            recovered.tracks.len(),
            2,
            "a cache saved before the library cannot hide songs after restart"
        );
        known.extend(first.tracks.into_iter().map(|t| t.source));
        // Check the persisted index also preserves incremental behavior across restarts.
        index = serde_json::from_slice(&serde_json::to_vec(&index).unwrap()).unwrap();
        let unchanged = scan(
            target.clone(),
            &AuthStore::default(),
            &known,
            &mut index,
            false,
            &cancel,
        );
        assert!(unchanged.tracks.is_empty());
        assert_eq!(unchanged.stats.listed, 2);
        assert_eq!(unchanged.stats.skipped, 2);
        let stamp = std::fs::metadata(&a).unwrap().modified().unwrap();
        std::fs::write(a.join("02.flac"), b"new").unwrap();
        // Force a distinct timestamp without sleeping or relying on filesystem precision.
        set_test_directory_modified(&a, stamp + Duration::from_secs(2));
        let added = scan(
            target.clone(),
            &AuthStore::default(),
            &known,
            &mut index,
            false,
            &cancel,
        );
        assert_eq!(added.tracks.len(), 1);
        assert!(added.tracks[0].source.ends_with("02.flac"));
        assert_eq!(added.stats.skipped, 1);
        known.extend(added.tracks.into_iter().map(|t| t.source));
        // A formerly leaf directory can gain an entire subtree.
        let deeper = a.join("Disc 2");
        std::fs::create_dir(&deeper).unwrap();
        std::fs::write(deeper.join("01.flac"), b"new").unwrap();
        set_test_directory_modified(&a, stamp + Duration::from_secs(3));
        let nested = scan(
            target.clone(),
            &AuthStore::default(),
            &known,
            &mut index,
            false,
            &cancel,
        );
        assert_eq!(nested.tracks.len(), 1);
        assert!(nested.tracks[0].source.contains("Disc 2"));
        known.extend(nested.tracks.into_iter().map(|t| t.source));
        let old = std::fs::metadata(&b).unwrap().modified().unwrap();
        std::fs::write(b.join("02.flac"), b"new").unwrap();
        set_test_directory_modified(&b, old);
        assert!(
            scan(
                target.clone(),
                &AuthStore::default(),
                &known,
                &mut index,
                false,
                &cancel
            )
            .tracks
            .is_empty()
        );
        index.last_full = seconds() - AUDIT;
        assert_eq!(
            scan(
                target.clone(),
                &AuthStore::default(),
                &known,
                &mut index,
                false,
                &cancel
            )
            .tracks
            .len(),
            1
        );
        cancel.store(true, Ordering::Relaxed);
        let stopped = scan(
            target,
            &AuthStore::default(),
            &known,
            &mut index,
            false,
            &cancel,
        );
        assert!(stopped.cancelled);
        assert_eq!(stopped.stats.listed, 0);
    }
}

#[cfg(test)]
mod integration {
    use super::*;
    #[test]
    #[ignore = "requires scripts/test_ftp.py fixture server"]
    fn live_ftp_incremental_reuses_login_skips_unchanged_albums_and_finds_new_tracks() {
        let root = PathBuf::from(std::env::var("AUREA_TEST_FTP_ROOT").unwrap());
        let root_url = std::env::var("AUREA_TEST_FTP_URL").unwrap();
        let artist = root.join("Auto Sync Artist");
        for i in 0..12 {
            let album = artist.join(format!("Album {i:02}"));
            std::fs::create_dir_all(&album).unwrap();
            std::fs::write(album.join("01.flac"), b"synthetic").unwrap();
        }
        let mut source = network::FtpSource::smartphone();
        source.id = "synthetic-sync".into();
        source.set_url(&root_url).unwrap();
        source.username = "fixture".into();
        source.password = "fixture-only".into();
        let auth = AuthStore::default();
        auth.insert(&source).unwrap();
        let target = Target::Ftp {
            id: source.id.clone(),
            root: root_url.clone(),
        };
        let mut known = HashSet::new();
        let mut index = Index::default();
        let cancel = AtomicBool::new(false);
        let metrics = || std::fs::read_to_string(root.join(".sync-metrics")).unwrap_or_default();
        let before = metrics();
        let first = scan(target.clone(), &auth, &known, &mut index, false, &cancel);
        assert_eq!(first.stats.errors, 0);
        assert_eq!(first.stats.listed, 15);
        assert!(first.tracks.len() >= 16);
        let after = metrics();
        assert_eq!(
            after.matches("login").count() - before.matches("login").count(),
            1,
            "one login for all fifteen directory listings"
        );
        assert_eq!(
            after.matches("download").count(),
            before.matches("download").count(),
            "discovery never downloads audio or artwork"
        );
        known.extend(first.tracks.into_iter().map(|t| t.source));
        let second = scan(target.clone(), &auth, &known, &mut index, false, &cancel);
        assert_eq!(second.stats.errors, 0);
        assert!(second.tracks.is_empty());
        assert_eq!(second.stats.listed, 2);
        assert_eq!(second.stats.skipped, 13);
        let album = artist.join("Album 05");
        let stamp = std::fs::metadata(&album).unwrap().modified().unwrap();
        std::fs::write(album.join("02 New song.flac"), b"new synthetic").unwrap();
        set_test_directory_modified(&album, stamp + Duration::from_secs(2));
        let new = scan(target.clone(), &auth, &known, &mut index, false, &cancel);
        assert_eq!(new.tracks.len(), 1);
        assert!(new.tracks[0].source.ends_with("02%20New%20song.flac"));
        assert_eq!(new.stats.listed, 3);
        assert_eq!(new.stats.skipped, 12);
        known.extend(new.tracks.into_iter().map(|t| t.source));
        let sub = album.join("New Disc");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("03.flac"), b"new synthetic").unwrap();
        set_test_directory_modified(&album, stamp + Duration::from_secs(3));
        let nested = scan(target.clone(), &auth, &known, &mut index, false, &cancel);
        assert_eq!(nested.tracks.len(), 1);
        assert!(nested.tracks[0].source.contains("New%20Disc"));
        known.extend(nested.tracks.into_iter().map(|t| t.source));
        assert!(
            scan(target.clone(), &auth, &known, &mut index, false, &cancel)
                .tracks
                .is_empty()
        );
        // MLSD is optional: LIST-only servers still discover new songs, with full listings.
        let legacy_url = std::env::var("AUREA_TEST_FTP_LEGACY_URL").unwrap();
        source.set_url(&legacy_url).unwrap();
        auth.insert(&source).unwrap();
        let legacy_target = Target::Ftp {
            id: source.id.clone(),
            root: legacy_url,
        };
        let mut legacy_index = Index::default();
        let legacy_first = scan(
            legacy_target.clone(),
            &auth,
            &HashSet::new(),
            &mut legacy_index,
            false,
            &cancel,
        );
        assert_eq!(legacy_first.stats.errors, 0);
        assert_eq!(legacy_first.tracks.len(), known.len());
        let legacy_known = legacy_first.tracks.into_iter().map(|t| t.source).collect();
        let legacy_second = scan(
            legacy_target,
            &auth,
            &legacy_known,
            &mut legacy_index,
            false,
            &cancel,
        );
        assert_eq!(legacy_second.stats.errors, 0);
        assert!(legacy_second.tracks.is_empty());
        assert_eq!(legacy_second.stats.skipped, 0);
        assert!(legacy_second.absent.is_empty());
        std::fs::remove_dir_all(artist.join("Album 00")).unwrap();
        std::fs::remove_file(album.join("02 New song.flac")).unwrap();
        let deleted = scan(target.clone(), &auth, &known, &mut index, false, &cancel);
        assert_eq!(deleted.stats.errors, 0);
        assert!(deleted.stats.skipped > 0);
        assert_eq!(deleted.absent.len(), 2);
        assert!(
            deleted
                .absent
                .iter()
                .any(|id| id.ends_with("Album%2000/01.flac"))
        );
        assert!(
            deleted
                .absent
                .iter()
                .any(|id| id.ends_with("Album%2005/02%20New%20song.flac"))
        );
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let offline = scan(
            Target::Ftp {
                id: "offline".into(),
                root: format!("ftp://127.0.0.1:{port}/"),
            },
            &auth,
            &known,
            &mut Index::default(),
            false,
            &cancel,
        );
        assert_eq!(offline.stats.errors, 1);
        assert_eq!(offline.stats.listed, 1);
        assert!(offline.tracks.is_empty());
        assert!(offline.absent.is_empty());
        cancel.store(true, Ordering::Relaxed);
        let cancelled = scan(target, &auth, &known, &mut index, false, &cancel);
        assert!(cancelled.cancelled);
        assert!(cancelled.absent.is_empty());
        assert_eq!(cancelled.stats.listed, 0);
        println!(
            "FTP sync: 15 directory listings -> 2 unchanged / 3 with a new song; one login per sweep; zero media downloads; LIST fallback and offline/cancel passed"
        );
    }
}

#[cfg(test)]
pub(crate) fn set_test_directory_modified(path: &Path, time: SystemTime) {
    #[cfg(windows)]
    let file = {
        use std::os::windows::fs::OpenOptionsExt;
        // Directory handles need backup semantics and FILE_WRITE_ATTRIBUTES.
        std::fs::OpenOptions::new()
            .access_mode(0x100)
            .custom_flags(0x0200_0000)
            .open(path)
            .unwrap()
    };
    #[cfg(not(windows))]
    let file = std::fs::File::open(path).unwrap();
    file.set_modified(time).unwrap();
}
