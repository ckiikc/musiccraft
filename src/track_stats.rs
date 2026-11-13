//! Private, persistent ratings and qualified plays. Never writes music files.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io::Read, path::Path};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Entry {
    pub rating: u8,
    pub plays: u64,
    pub rated_at: u64,
    pub rating_set: bool,
    pub tag_pending: bool,
    pub edit_revision: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stats {
    version: u32,
    entries: BTreeMap<String, Entry>,
}
impl Default for Stats {
    fn default() -> Self {
        Self {
            version: 1,
            entries: BTreeMap::new(),
        }
    }
}
impl Stats {
    pub fn get(&self, source: &str) -> Entry {
        self.entries.get(source).copied().unwrap_or_default()
    }
    pub fn rating_for(
        &self,
        track: &crate::library::Track,
        scale: musiccraft_tags::Scale,
    ) -> Option<u8> {
        let entry = self.get(&track.source);
        if entry.rating_set || entry.rated_at > 0 || entry.rating > 0 {
            return Some(entry.rating);
        }
        let tags = track.rating_tags.as_ref()?;
        tags.rating(scale).ok().map(|n| n.unwrap_or(0))
    }
    pub fn effective(&self, track: &crate::library::Track, scale: musiccraft_tags::Scale) -> Entry {
        let mut entry = self.get(&track.source);
        entry.rating = self.rating_for(track, scale).unwrap_or(0);
        entry
    }
    pub fn pending(&mut self, source: &str, pending: bool) {
        if let Some(entry) = self.entries.get_mut(source) {
            entry.tag_pending = pending;
        }
    }
    fn valid(&self) -> bool {
        self.version == 1
            && self.entries.len() <= 100_000
            && self
                .entries
                .iter()
                .all(|(s, e)| crate::playlists::valid_source(s) && e.rating <= 5)
    }
    pub fn load(path: &Path) -> Result<Self, String> {
        let f = match std::fs::File::open(path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(_) => return Err("Listening statistics unavailable.".into()),
        };
        let mut b = vec![];
        f.take(16 * 1024 * 1024 + 1)
            .read_to_end(&mut b)
            .map_err(|_| "Listening statistics unavailable.")?;
        if b.len() > 16 * 1024 * 1024 {
            return Err("Listening statistics unavailable.".into());
        }
        let s: Self =
            serde_json::from_slice(&b).map_err(|_| "Listening statistics unavailable.")?;
        if !s.valid() {
            return Err("Listening statistics unavailable.".into());
        }
        Ok(s)
    }
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if !self.valid() {
            return Err("Could not save listening statistics.".into());
        }
        let b = serde_json::to_vec(self).map_err(|_| "Could not save listening statistics.")?;
        if b.len() > 16 * 1024 * 1024 {
            return Err("Could not save listening statistics.".into());
        }
        crate::playlists::atomic_write(path, &b)
            .map_err(|_| "Could not save listening statistics.".into())
    }
    pub fn rate(&mut self, source: &str, rating: u8, when: u64) -> bool {
        if rating > 5
            || !crate::playlists::valid_source(source)
            || (!self.entries.contains_key(source) && self.entries.len() >= 100_000)
        {
            return false;
        }
        let e = self.entries.entry(source.into()).or_default();
        e.rating = rating;
        e.rating_set = true;
        e.tag_pending = true;
        e.edit_revision = e.edit_revision.wrapping_add(1);
        e.rated_at = when;
        true
    }
    pub fn played(&mut self, source: &str) -> bool {
        if !crate::playlists::valid_source(source)
            || (!self.entries.contains_key(source) && self.entries.len() >= 100_000)
        {
            return false;
        }
        let e = self.entries.entry(source.into()).or_default();
        e.plays = e.plays.saturating_add(1);
        true
    }
    pub fn remap(&mut self, mut map: impl FnMut(&str) -> Option<String>) -> bool {
        let mut changed = false;
        let mut next: BTreeMap<String, Entry> = BTreeMap::new();
        for (source, entry) in &self.entries {
            let target = map(source)
                .filter(|s| crate::playlists::valid_source(s))
                .unwrap_or_else(|| source.clone());
            changed |= target != *source;
            next.entry(target)
                .and_modify(|old| {
                    old.plays = old.plays.max(entry.plays);
                    if entry.rated_at > old.rated_at {
                        old.rating = entry.rating;
                        old.rated_at = entry.rated_at;
                        old.rating_set = entry.rating_set;
                        old.tag_pending = entry.tag_pending;
                    }
                })
                .or_insert(*entry);
        }
        if changed {
            self.entries = next;
        }
        changed
    }
}
/// Position is measured by the existing audio engine. Successful seeks reset the
/// baseline; pauses, invalid/backward positions and duplicate samples add no time.
#[derive(Default)]
pub struct Counter {
    source: Option<String>,
    last: Option<f64>,
    paused: bool,
    seconds: f64,
    threshold: f64,
    counted: bool,
    clock: Option<std::time::Instant>,
    budget: f64,
}
impl Counter {
    pub fn start(&mut self, source: String, duration: f64) {
        *self = Self {
            source: Some(source),
            clock: Some(std::time::Instant::now()),
            last: Some(0.0),
            threshold: if duration.is_finite() && duration > 0.0 {
                (duration / 2.0).clamp(1.0, 30.0)
            } else {
                30.0
            },
            ..Self::default()
        };
    }
    pub fn seeked(&mut self) {
        self.budget = 0.0;
        self.clock = Some(std::time::Instant::now());
        self.last = None;
    }
    pub fn stop(&mut self) {
        *self = Self::default();
    }
    pub fn remap(&mut self, mut map: impl FnMut(&str) -> Option<String>) {
        if let Some(source) = &mut self.source
            && let Some(new) = map(source)
        {
            *source = new;
        }
    }
    pub fn position(&mut self, pos: f64, paused: bool) -> Option<String> {
        let now = std::time::Instant::now();
        let elapsed = self.clock.replace(now).map_or(0.0, |last| {
            now.saturating_duration_since(last).as_secs_f64()
        });
        self.observe(pos, paused, elapsed)
    }
    #[cfg(test)]
    pub(crate) fn allow_test_elapsed(&mut self, seconds: u64) {
        self.clock = Some(std::time::Instant::now() - std::time::Duration::from_secs(seconds));
    }
    fn observe(&mut self, pos: f64, paused: bool, elapsed: f64) -> Option<String> {
        if !pos.is_finite() || pos < 0.0 {
            return None;
        }
        if !paused && !self.paused {
            self.budget += elapsed.max(0.0);
        } else {
            self.budget = 0.0;
        }
        if let Some(last) = self.last
            && !paused
            && !self.paused
            && pos >= last
        {
            // Buffered position notifications share one wall-time budget; stale
            // positions after a transition cannot manufacture listening time.
            let heard = (pos - last).min(self.budget);
            self.seconds += heard;
            self.budget -= heard;
        }
        self.last = Some(pos);
        self.paused = paused;
        if !self.counted
            && self.seconds >= self.threshold
            && let Some(source) = &self.source
        {
            self.counted = true;
            return Some(source.clone());
        }
        None
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ambiguous_unread_and_absent_rating_tags_are_distinct() {
        let stats = Stats::default();
        let mut t = crate::library::Track::placeholder("/test.flac".into());
        assert_eq!(stats.rating_for(&t, musiccraft_tags::Scale::Five), None);
        t.rating_tags = Some(Default::default());
        assert_eq!(stats.rating_for(&t, musiccraft_tags::Scale::Five), Some(0));
        t.rating_tags.as_mut().unwrap().values = vec!["4".into(), "2".into()];
        assert_eq!(stats.rating_for(&t, musiccraft_tags::Scale::Five), None);
    }
    #[test]
    fn imported_ratings_never_override_explicit_votes_even_zero_after_reopen() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("stats.json");
        let mut t = crate::library::Track::placeholder("/test.flac".into());
        t.rating_tags = Some(musiccraft_tags::Summary {
            values: vec!["4".into()],
            issue: None,
        });
        let mut stats = Stats::default();
        assert_eq!(stats.effective(&t, musiccraft_tags::Scale::Five).rating, 4);
        stats.played(&t.source);
        assert_eq!(stats.effective(&t, musiccraft_tags::Scale::Five).rating, 4);
        assert!(stats.rate(&t.source, 0, 1));
        stats.save(&path).unwrap();
        let stats = Stats::load(&path).unwrap();
        assert_eq!(stats.effective(&t, musiccraft_tags::Scale::Five).rating, 0);
        let legacy: Stats = serde_json::from_str(
            r#"{"version":1,"entries":{"/test.flac":{"rating":2,"plays":0,"rated_at":0}}}"#,
        )
        .unwrap();
        assert_eq!(legacy.effective(&t, musiccraft_tags::Scale::Five).rating, 2);
    }
    #[test]
    fn qualified_plays_exclude_pause_seek_failed_start_and_duplicate_samples() {
        let mut c = Counter::default();
        assert!(c.observe(500.0, false, 500.0).is_none());
        c.start("/a.flac".into(), 240.0);
        assert!(c.observe(10.0, false, 10.0).is_none());
        assert!(c.observe(10.0, true, 60.0).is_none());
        assert!(c.observe(10.0, true, 60.0).is_none());
        assert!(c.observe(10.0, false, 10.0).is_none());
        c.seeked();
        assert!(c.observe(200.0, false, 0.0).is_none());
        assert!(c.observe(215.0, false, 15.0).is_none());
        assert_eq!(c.observe(220.0, false, 5.0).as_deref(), Some("/a.flac"));
        assert!(c.observe(230.0, false, 10.0).is_none());
        c.start("/a.flac".into(), 4.0);
        assert!(c.observe(1.0, false, 1.0).is_none());
        assert_eq!(c.observe(2.0, false, 1.0).as_deref(), Some("/a.flac"));
        c.stop();
        assert!(c.observe(100.0, false, 100.0).is_none());
    }
    #[test]
    fn stale_positions_and_a_burst_of_notifications_cannot_exceed_real_elapsed_time() {
        let mut c = Counter::default();
        c.start("/short.wav".into(), 4.0);
        assert!(c.observe(50.0, false, 0.1).is_none());
        assert!(c.observe(100.0, false, 0.0).is_none());
        assert!(c.observe(150.0, false, 0.0).is_none());
        assert!(c.seconds <= 0.1);
        c.start("/short.wav".into(), 4.0);
        assert!(c.observe(0.5, false, 1.0).is_none());
        assert!(c.observe(1.0, false, 0.0).is_none());
        assert!(c.observe(2.0, false, 0.0).is_none());
        assert_eq!(c.seconds, 1.0);
        assert_eq!(c.observe(3.0, false, 1.0).as_deref(), Some("/short.wav"));
    }
    #[test]
    fn ratings_plays_persist_and_rebase_without_deleting_corrupt_files() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("listening-stats.json");
        let mut s = Stats::default();
        assert!(s.rate("https://old.local/a.flac", 5, 10));
        assert!(s.played("https://old.local/a.flac"));
        assert!(s.remap(|p| crate::network::rebase(p, "https://old.local/", "https://new.local/")));
        s.save(&path).unwrap();
        assert_eq!(Stats::load(&path).unwrap(), s);
        assert_eq!(s.get("https://new.local/a.flac").plays, 1);
        assert_eq!(s.get("https://new.local/a.flac").rating, 5);
        assert!(!s.rate("/a.flac", 6, 20));
        assert!(!s.rate("https://fixture:fixture-only@nas.invalid/a", 3, 20));
        std::fs::write(&path, b"broken").unwrap();
        assert!(Stats::load(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"broken");
    }
}
