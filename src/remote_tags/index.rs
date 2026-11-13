//! A single paced metadata lane, independent of directory synchronization and playback selection.
use super::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    fs,
    hash::{Hash, Hasher},
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const JOURNAL_LIMIT: u64 = 64 * 1024 * 1024;
const RECORD_LIMIT: usize = 32 * 1024;
#[derive(Clone, Default, Serialize, Deserialize)]
struct Retry {
    failures: u32,
    after: u64,
    #[serde(default)]
    file_error: bool,
}
#[derive(Default, Serialize, Deserialize)]
struct Retries {
    #[serde(default)]
    policy: u8,
    files: BTreeMap<u64, Retry>,
    roots: BTreeMap<String, Retry>,
}
struct Active {
    source: String,
    root: String,
    index: usize,
    ticket: u64,
}
#[derive(Default)]
pub struct Library {
    epoch: Arc<AtomicU64>,
    path: Option<PathBuf>,
    groups: BTreeMap<String, VecDeque<usize>>,
    roots: Vec<String>,
    count: usize,
    initialized: bool,
    active: Option<Active>,
    retries: Retries,
    next: Option<Instant>,
    checkpoint: Option<Instant>,
    lane: usize,
    paused: bool,
    priority: Option<String>,
    revision: u64,
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn key(source: &str) -> u64 {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hash);
    hash.finish()
}
fn defer(retry: &mut Retry) {
    retry.failures = retry.failures.saturating_add(1).min(10);
    retry.after = now().saturating_add((30_u64 * (1 << (retry.failures - 1))).min(15 * 60));
}
fn journal(path: &Path, track: &Track) {
    let Ok(mut bytes) = serde_json::to_vec(track) else {
        return;
    };
    if bytes.len() > RECORD_LIMIT {
        return;
    }
    bytes.push(b'\n');
    if fs::create_dir_all(path).is_err() {
        return;
    }
    let file = path.join("tags.jsonl");
    if fs::metadata(&file).is_ok_and(|m| m.len().saturating_add(bytes.len() as u64) > JOURNAL_LIMIT)
    {
        return;
    }
    let mut options = fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    if let Ok(mut file) = options.open(file) {
        let _ = file.write_all(&bytes);
    }
}
impl Library {
    pub fn new(path: PathBuf, tracks: &mut [Track]) -> Self {
        // Replay results that arrived after the last UI autosave. A partly written final record
        // is ignored; audio properties, source URLs and already-loaded tags remain authoritative.
        if let Ok(file) = fs::File::open(path.join("tags.jsonl")) {
            let mut bytes = Vec::new();
            if file.take(JOURNAL_LIMIT).read_to_end(&mut bytes).is_ok() {
                let positions: HashMap<_, _> = tracks
                    .iter()
                    .enumerate()
                    .map(|(i, t)| (t.source.clone(), i))
                    .collect();
                for row in bytes
                    .split(|&b| b == b'\n')
                    .filter(|row| !row.is_empty() && row.len() <= RECORD_LIMIT)
                {
                    if let Ok(track) = serde_json::from_slice::<Track>(row)
                        && track.metadata_loaded
                        && let Some(&i) = positions.get(&track.source)
                        && tracks[i].needs_tag_read()
                    {
                        if tracks[i].metadata_loaded || tracks[i].rate > 0 {
                            tracks[i].rating_tags = track.rating_tags;
                        } else {
                            tracks[i].adopt_tags(track);
                        }
                    }
                }
            }
        }
        let mut retries = fs::File::open(path.join("retries.json"))
            .ok()
            .and_then(|file| serde_json::from_reader::<_, Retries>(file.take(8 * 1024 * 1024)).ok())
            .filter(|r| r.files.len() <= crate::library::MAX_TRACKS)
            .unwrap_or_default();
        // Reconsider old failures once after the bounded metadata reader upgrade. Preserve
        // tag journals and failure counts; only reset wait deadlines, never loaded metadata.
        if retries.policy != 3 {
            retries.roots.clear();
            for retry in retries.files.values_mut() {
                retry.after = 0;
            }
            retries.policy = 3;
        }
        let mut library = Self::default();
        library.path = Some(path);
        library.retries = retries;
        library
    }
    pub fn accepts(&self, ticket: u64) -> bool {
        self.epoch.load(Ordering::Relaxed) == ticket
    }
    fn cancel(&mut self) {
        if self.active.as_ref().is_some_and(|a| self.accepts(a.ticket)) {
            self.epoch.fetch_add(1, Ordering::Relaxed);
        }
    }
    pub fn library_changed(&mut self) {
        self.initialized = false;
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn has_failed(&self, source: &str) -> bool {
        self.retries
            .files
            .get(&key(source))
            .is_some_and(|retry| retry.file_error)
    }
    pub fn is_paused(&self) -> bool {
        self.paused
    }
    pub fn waiting_for_source(&self) -> bool {
        !self.roots.is_empty()
            && self.roots.iter().all(|r| {
                self.retries
                    .roots
                    .get(r)
                    .is_some_and(|retry| retry.after > now())
            })
    }
    pub fn prioritize(&mut self, tracks: &[Track], query: &str) {
        let query = query.trim().to_lowercase();
        if self.priority.as_ref() == Some(&query) {
            return;
        }
        self.priority = Some(query.clone());
        if query.chars().count() < 2 {
            return;
        }
        for queue in self.groups.values_mut() {
            let (mut matching, rest): (VecDeque<_>, VecDeque<_>) =
                queue.drain(..).partition(|&i| {
                    tracks.get(i).is_some_and(|t| {
                        percent_encoding::percent_decode_str(&t.source)
                            .decode_utf8_lossy()
                            .to_lowercase()
                            .contains(&query)
                    })
                });
            matching.extend(rest);
            *queue = matching;
        }
    }
    pub fn finished(&mut self, ticket: u64, completion: Completion) {
        if !self.active.as_ref().is_some_and(|a| a.ticket == ticket) {
            return;
        }
        let Some(active) = self.active.take() else {
            return;
        };
        self.revision = self.revision.wrapping_add(1);
        let queue = self.groups.get_mut(&active.root);
        if self.epoch.load(Ordering::Relaxed) != ticket
            || crate::cache::audio_transfer_pending()
            || completion == Completion::Interrupted
        {
            if let Some(queue) = queue {
                queue.push_front(active.index);
            }
        } else if completion == Completion::Indexed {
            self.retries.files.remove(&key(&active.source));
            self.retries.roots.remove(&active.root);
        } else {
            if let Some(queue) = queue {
                queue.push_back(active.index);
            }
            let retry = self.retries.files.entry(key(&active.source)).or_default();
            // A completed attempt with a file error is not pending work. A later connection
            // failure must not erase that known outcome; success still clears it entirely.
            retry.file_error |= completion == Completion::FileError;
            defer(retry);
            if completion == Completion::SourceError {
                defer(self.retries.roots.entry(active.root).or_default());
            }
        }
        self.next = Some(Instant::now() + Duration::from_millis(250));
        self.save_retries(false);
    }
    fn save_retries(&mut self, force: bool) {
        if !force && self.checkpoint.is_some_and(|at| Instant::now() < at) {
            return;
        }
        self.checkpoint = Some(Instant::now() + Duration::from_secs(15));
        let Some(path) = &self.path else { return };
        if fs::create_dir_all(path).is_err() {
            return;
        }
        let Ok(bytes) = serde_json::to_vec(&self.retries) else {
            return;
        };
        let temp = path.join("retries.tmp");
        if fs::write(&temp, bytes).is_ok() {
            let _ = fs::rename(temp, path.join("retries.json"));
        }
    }
    pub fn tick(
        &mut self,
        tracks: &[Track],
        auth: &AuthStore,
        tx: &Sender<Job>,
        mut roots: Vec<String>,
        busy: bool,
    ) {
        if tracks
            .iter()
            .any(|t| !crate::library::is_remote(&t.source) && t.rating_tags.is_none())
        {
            roots.push("/".into());
        }
        roots.sort();
        roots.dedup();
        if roots != self.roots || tracks.len() != self.count || !self.initialized {
            if roots != self.roots {
                self.cancel();
            }
            self.roots = roots;
            self.count = tracks.len();
            self.initialized = true;
            self.priority = None;
            self.groups = self
                .roots
                .iter()
                .map(|r| (r.clone(), VecDeque::new()))
                .collect();
            self.retries
                .roots
                .retain(|root, _| self.groups.contains_key(root));
            for (i, track) in tracks
                .iter()
                .enumerate()
                .filter(|(_, t)| t.needs_tag_read())
            {
                if self
                    .active
                    .as_ref()
                    .is_some_and(|a| a.source == track.source)
                {
                    continue;
                }
                if let Some((_, queue)) = self
                    .groups
                    .iter_mut()
                    .filter(|(root, _)| track.source.starts_with(root.as_str()))
                    .max_by_key(|(root, _)| root.len())
                {
                    queue.push_back(i);
                }
            }
            self.next = None;
        }
        self.paused = busy || crate::cache::audio_transfer_pending();
        if self.paused {
            self.cancel();
            return;
        }
        if self.active.is_some() || self.next.is_some_and(|at| Instant::now() < at) {
            return;
        }
        self.next = Some(Instant::now() + Duration::from_secs(1));
        let time = now();
        // Round-robin sources: a disconnected phone never blocks a reachable NAS/FTP source.
        for _ in 0..self.roots.len() {
            self.lane = (self.lane + 1) % self.roots.len();
            let root = &self.roots[self.lane];
            if self.retries.roots.get(root).is_some_and(|r| r.after > time) {
                continue;
            }
            let Some(queue) = self.groups.get_mut(root) else {
                continue;
            };
            let mut selected = None;
            for _ in 0..queue.len().min(64) {
                let Some(index) = queue.pop_front() else {
                    break;
                };
                let Some(track) = tracks
                    .get(index)
                    .filter(|t| t.needs_tag_read() && t.source.starts_with(root))
                else {
                    continue;
                };
                if self
                    .retries
                    .files
                    .get(&key(&track.source))
                    .is_some_and(|r| r.after > time)
                {
                    queue.push_back(index);
                    continue;
                }
                selected = Some((index, track.clone()));
                break;
            }
            let Some((index, track)) = selected else {
                continue;
            };
            let ticket = self.epoch.load(Ordering::Relaxed);
            self.active = Some(Active {
                source: track.source.clone(),
                root: root.clone(),
                index,
                ticket,
            });
            let (auth, tx, epoch, path) = (
                auth.clone(),
                tx.clone(),
                self.epoch.clone(),
                self.path.clone(),
            );
            let source = track.source.clone();
            std::thread::spawn(move || {
                let result = super::read(track, &auth, epoch.clone(), ticket);
                if result
                    .as_ref()
                    .is_err_and(|e| e.kind() == io::ErrorKind::NotFound)
                    && epoch.load(Ordering::Relaxed) == ticket
                    && let Some(target) = verified_space_alias(&source, &auth)
                    && epoch.load(Ordering::Relaxed) == ticket
                {
                    let _ = tx.send(Job::TagAlias {
                        source: source.clone(),
                        target,
                        ticket,
                    });
                    let _ = tx.send(Job::TagsDone(ticket, Completion::Indexed));
                    return;
                }
                let completion = match &result {
                    Ok(_) => Completion::Indexed,
                    Err(e)
                        if matches!(
                            e.kind(),
                            io::ErrorKind::ConnectionAborted | io::ErrorKind::Interrupted
                        ) =>
                    {
                        Completion::Interrupted
                    }
                    Err(e)
                        if matches!(
                            e.kind(),
                            io::ErrorKind::NotConnected
                                | io::ErrorKind::PermissionDenied
                                | io::ErrorKind::TimedOut
                        ) =>
                    {
                        Completion::SourceError
                    }
                    Err(_) => Completion::FileError,
                };
                if epoch.load(Ordering::Relaxed) == ticket
                    && let Ok(track) = result
                {
                    if let Some(path) = path {
                        journal(&path, &track);
                    }
                    let _ = tx.send(Job::Tags(track, ticket));
                }
                let _ = tx.send(Job::TagsDone(ticket, completion));
            });
            break;
        }
    }
}
/// Recover only a server-verified missing alias with one unambiguous leading-space filename.
pub(super) fn verified_space_alias(source: &str, auth: &AuthStore) -> Option<String> {
    let (parent, name) = source.rsplit_once('/')?;
    let name = percent_encoding::percent_decode_str(name)
        .decode_utf8()
        .ok()?;
    let root = format!("{parent}/");
    let entries = crate::network::list(&root, auth).ok()?;
    if entries.iter().any(|e| e.name == name) {
        return None;
    }
    let mut matches = entries
        .iter()
        .filter(|e| !e.directory && e.name != name && e.name.trim_start() == name);
    let found = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    crate::network::child_url(&root, &found.name, false)
}
impl Drop for Library {
    fn drop(&mut self) {
        self.epoch.fetch_add(1, Ordering::Relaxed);
        self.save_retries(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn track(path: &str) -> Track {
        Track::placeholder(format!("ftp://127.0.0.1:9/{path}"))
    }
    fn temp() -> PathBuf {
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        std::env::temp_dir().join(format!(
            "musiccraft-tag-index-{}-{}-{}",
            std::process::id(),
            now(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ))
    }
    #[test]
    fn all_sources_queue_once_without_playback_and_skip_known_tags() {
        let mut tracks = vec![
            track("a/one.flac"),
            track("a/nested/two.flac"),
            track("b/three.mp3"),
            track("outside/four.flac"),
            track("a/known.flac"),
            track("b/decoded.flac"),
        ];
        tracks[4].metadata_loaded = true;
        tracks[4].rating_tags = Some(Default::default());
        tracks[5].rate = 44100;
        tracks[5].rating_tags = Some(Default::default());
        let roots = vec![
            "ftp://127.0.0.1:9/a/".into(),
            "ftp://127.0.0.1:9/a/nested/".into(),
            "ftp://127.0.0.1:9/b/".into(),
        ];
        let mut index = Library::default();
        let (tx, _) = mpsc::channel();
        index.tick(&tracks, &AuthStore::default(), &tx, roots.clone(), true);
        assert_eq!(
            index.groups.values().flatten().copied().collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        assert!(index.active.is_none());
        // Completed rows are skipped lazily, without rebuilding the entire library per result.
        tracks[0].metadata_loaded = true;
        tracks[0].rating_tags = Some(Default::default());
        index.tick(&tracks, &AuthStore::default(), &tx, roots, true);
        assert_eq!(index.groups.values().map(VecDeque::len).sum::<usize>(), 3);
    }
    #[test]
    fn searching_prioritizes_unread_paths_without_inventing_artist_tags() {
        let tracks = vec![
            track("Music/Alpha/01.flac"),
            track("Music/Queen%20Live/01.flac"),
            track("Music/Beta/01.flac"),
            track("Music/Queen/02.flac"),
        ];
        let root = "ftp://127.0.0.1:9/".to_owned();
        let mut index = Library::default();
        let (tx, _) = mpsc::channel();
        index.tick(
            &tracks,
            &AuthStore::default(),
            &tx,
            vec![root.clone()],
            true,
        );
        index.prioritize(&tracks, " QUEEN ");
        assert_eq!(index.groups[&root], VecDeque::from([1, 3, 0, 2]));
        assert!(
            tracks
                .iter()
                .all(|t| !t.metadata_loaded && t.artist.is_empty())
        );
        index.prioritize(&tracks, "beta");
        assert_eq!(index.groups[&root], VecDeque::from([2, 1, 3, 0]));
        // An unchanged query must not re-partition work on every animation frame.
        index.groups.get_mut(&root).unwrap().rotate_left(1);
        let expected = index.groups[&root].clone();
        index.prioritize(&tracks, "beta");
        assert_eq!(index.groups[&root], expected);
    }
    #[test]
    fn file_outcomes_survive_restart_but_connection_waits_remain_pending() {
        let path = temp();
        let tracks = vec![track("Music/bad.flac")];
        let source = tracks[0].source.clone();
        let root = "ftp://127.0.0.1:9/".to_owned();
        let mut index = Library::new(path.clone(), &mut []);
        let (tx, _) = mpsc::channel();
        index.tick(
            &tracks,
            &AuthStore::default(),
            &tx,
            vec![root.clone()],
            true,
        );
        // An unreachable source has not been examined, so it cannot complete the pass.
        index.active = Some(Active {
            source: source.clone(),
            root: root.clone(),
            index: 0,
            ticket: 0,
        });
        index.finished(0, Completion::SourceError);
        assert!(!index.has_failed(&source));
        // A real file-level failure is a completed attempt, separately counted from tags read.
        index.active = Some(Active {
            source: source.clone(),
            root: root.clone(),
            index: 0,
            ticket: 0,
        });
        index.finished(0, Completion::FileError);
        assert!(index.has_failed(&source));
        drop(index);
        let mut index = Library::new(path.clone(), &mut []);
        assert!(index.has_failed(&source));
        // Retrying remains possible. A temporary outage does not erase the previous outcome.
        index.active = Some(Active {
            source: source.clone(),
            root: root.clone(),
            index: 0,
            ticket: 0,
        });
        index.finished(0, Completion::SourceError);
        assert!(index.has_failed(&source));
        index.active = Some(Active {
            source: source.clone(),
            root,
            index: 0,
            ticket: 0,
        });
        index.finished(0, Completion::Indexed);
        assert!(!index.has_failed(&source));
        drop(index);
        std::fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn bad_file_does_not_suspend_the_entire_reachable_source() {
        let root = "ftp://127.0.0.1:9/".to_owned();
        let tracks = vec![track("Music/bad.flac")];
        let mut index = Library::default();
        let (tx, _) = mpsc::channel();
        index.tick(
            &tracks,
            &AuthStore::default(),
            &tx,
            vec![root.clone()],
            true,
        );
        index.active = Some(Active {
            source: tracks[0].source.clone(),
            root: root.clone(),
            index: 0,
            ticket: 0,
        });
        index.finished(0, Completion::FileError);
        assert_eq!(index.retries.files[&key(&tracks[0].source)].failures, 1);
        assert!(index.retries.roots.is_empty());
    }
    #[test]
    fn old_source_waits_are_reconsidered_once_without_losing_failed_files() {
        let path = temp();
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("retries.json"), r#"{"files":{"7":{"failures":2,"after":9999999999}},"roots":{"ftp://phone/":{"failures":6,"after":9999999999}}}"#).unwrap();
        let index = Library::new(path.clone(), &mut []);
        assert!(index.retries.roots.is_empty());
        assert_eq!(index.retries.files[&7].failures, 2);
        assert_eq!(index.retries.policy, 3);
        assert_eq!(index.retries.files[&7].after, 0);
        drop(index);
        fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn cancelled_work_resumes_without_penalty_and_failures_back_off_across_restarts() {
        let path = temp();
        let tracks = vec![track("a/one.flac")];
        let root = "ftp://127.0.0.1:9/a/".to_owned();
        let mut index = Library::new(path.clone(), &mut []);
        let (tx, _) = mpsc::channel();
        index.tick(
            &tracks,
            &AuthStore::default(),
            &tx,
            vec![root.clone()],
            true,
        );
        index.groups.get_mut(&root).unwrap().clear();
        index.active = Some(Active {
            source: tracks[0].source.clone(),
            root: root.clone(),
            index: 0,
            ticket: 0,
        });
        index.tick(
            &tracks,
            &AuthStore::default(),
            &tx,
            vec![root.clone()],
            true,
        );
        assert!(!index.accepts(0));
        index.finished(0, Completion::Interrupted);
        assert_eq!(index.groups[&root].front(), Some(&0));
        assert!(index.retries.files.is_empty());
        index.active = Some(Active {
            source: tracks[0].source.clone(),
            root: root.clone(),
            index: 0,
            ticket: 1,
        });
        index.finished(1, Completion::SourceError);
        assert!(index.retries.roots[&root].after > now());
        drop(index);
        let restored = Library::new(path.clone(), &mut []);
        assert_eq!(restored.retries.files[&key(&tracks[0].source)].failures, 1);
        assert!(restored.retries.roots[&root].after > now());
        drop(restored);
        fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn journal_restores_tags_and_preserves_decoder_properties_and_known_metadata() {
        let path = temp();
        let mut result = track("a/one.flac");
        result.title = "Actual title".into();
        result.artist = "Actual artist".into();
        result.album = "Actual album".into();
        result.number = 7;
        result.metadata_loaded = true;
        journal(&path, &result);
        fs::OpenOptions::new()
            .append(true)
            .open(path.join("tags.jsonl"))
            .unwrap()
            .write_all(b"\ninvalid\n{\"source\":")
            .unwrap();
        let mut tracks = vec![track("a/one.flac")];
        let index = Library::new(path.clone(), &mut tracks);
        assert_eq!(tracks[0].title, "Actual title");
        assert_eq!(tracks[0].number, 7);
        assert!(tracks[0].metadata_loaded);
        drop(index);
        tracks[0].title = "Decoder title".into();
        tracks[0].rate = 96000;
        tracks[0].metadata_loaded = false;
        let index = Library::new(path.clone(), &mut tracks);
        assert_eq!(tracks[0].title, "Decoder title");
        assert_eq!(tracks[0].rate, 96000);
        drop(index);
        fs::remove_dir_all(path).unwrap();
    }
    #[test]
    #[ignore = "Synthetic FTP server provided by scripts/test_ftp.py"]
    fn live_ftp_library_tags_without_playback_and_resume() {
        let root = std::env::var("AUREA_TEST_FTP_URL").unwrap();
        let mut source = crate::network::FtpSource::smartphone();
        source.set_url(&root).unwrap();
        source.username = "fixture".into();
        source.password = "fixture-only".into();
        let auth = AuthStore::default();
        auth.insert(&source).unwrap();
        let folder = crate::network::child_url(&root, "Album Città", true).unwrap();
        let initial = vec![
            Track::placeholder(
                crate::network::child_url(&folder, "02 Interlude.flac", false).unwrap(),
            ),
            Track::placeholder(
                crate::network::child_url(&root, ".range-probe.bin", false).unwrap(),
            ),
            Track::placeholder("ftp://127.0.0.1:1/offline.flac".into()),
        ];
        let path = temp();
        let mut tracks = initial.clone();
        let mut index = Library::new(path.clone(), &mut tracks);
        let (tx, rx) = mpsc::channel();
        let roots = vec![root.clone(), "ftp://127.0.0.1:1/".into()];
        fn pump(
            index: &mut Library,
            tracks: &mut [Track],
            auth: &AuthStore,
            tx: &Sender<Job>,
            rx: &mpsc::Receiver<Job>,
            roots: &[String],
        ) {
            let deadline = Instant::now() + Duration::from_secs(15);
            while tracks
                .iter()
                .any(|t| t.source.starts_with(&roots[0]) && !t.metadata_loaded)
                || index.active.is_some()
            {
                assert!(Instant::now() < deadline, "background index stalled");
                index.tick(tracks, auth, tx, roots.to_vec(), false);
                while let Ok(job) = rx.try_recv() {
                    match job {
                        Job::Tags(result, ticket) if index.accepts(ticket) => {
                            tracks
                                .iter_mut()
                                .find(|t| t.source == result.source)
                                .unwrap()
                                .adopt_tags(result);
                        }
                        Job::TagsDone(ticket, success) => index.finished(ticket, success),
                        _ => {}
                    }
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        // A directory scan interrupts an actual background job. Its result must be
        // discarded, the lane released, and the same track retried without backoff.
        index.tick(&tracks, &auth, &tx, roots.clone(), false);
        let ticket = index.active.as_ref().unwrap().ticket;
        index.tick(&tracks, &auth, &tx, roots.clone(), true);
        assert!(!index.accepts(ticket));
        let deadline = Instant::now() + Duration::from_secs(6);
        while index.active.is_some() {
            assert!(
                Instant::now() < deadline,
                "cancelled metadata reader stalled"
            );
            if let Ok(Job::TagsDone(ticket, completion)) = rx.try_recv() {
                index.finished(ticket, completion);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(index.retries.files.is_empty());
        assert!(index.retries.roots.is_empty());
        assert!(tracks.iter().all(|t| !t.metadata_loaded));
        pump(&mut index, &mut tracks, &auth, &tx, &rx, &roots);
        assert_eq!(tracks[0].title, "Interlude");
        assert_eq!(tracks[1].title, "Range Title");
        assert!(tracks.iter().all(|t| t.rate == 0));
        assert_eq!(index.retries.roots["ftp://127.0.0.1:1/"].failures, 1);
        drop(index);
        // Simulate a quit before the UI profile was saved: the disk journal restores everything.
        let mut tracks = initial;
        let mut index = Library::new(path.clone(), &mut tracks);
        assert!(tracks[..2].iter().all(|t| t.metadata_loaded));
        index.tick(&tracks, &auth, &tx, roots.clone(), false);
        assert!(index.active.is_none());
        assert!(index.groups[&root].is_empty());
        // Directory sync discovers one new track: only this item needs a metadata request.
        tracks.push(Track::placeholder(
            crate::network::child_url(&folder, "04 Encore.mp3", false).unwrap(),
        ));
        pump(&mut index, &mut tracks, &auth, &tx, &rx, &roots);
        assert_eq!(tracks[3].title, "Encore");
        assert_eq!(index.retries.roots["ftp://127.0.0.1:1/"].failures, 1);
        drop(index);
        fs::remove_dir_all(path).unwrap();
    }
}
