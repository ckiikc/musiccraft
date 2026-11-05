use rand::seq::IteratorRandom;
use std::collections::HashSet;

/// Persistent listening cycle. Preparing a candidate does not consume it: only a
/// successfully started track enters `played`, including explicit selections.
#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Cycle {
    played: HashSet<String>,
    last_started: Option<String>,
}

impl Cycle {
    pub fn record(&mut self, id: &str) {
        self.played.insert(id.into());
        self.last_started = Some(id.into());
    }

    pub fn clear(&mut self) {
        self.played.clear();
        self.last_started = None;
    }

    pub fn choose<'a>(
        &mut self,
        sources: impl Iterator<Item = &'a str>,
        current: Option<&str>,
    ) -> Option<String> {
        let candidates: HashSet<_> = sources.collect();
        let current = current.or(self.last_started.as_deref());
        self.played.retain(|id| candidates.contains(id.as_str()));
        if candidates.is_empty() {
            return None;
        }
        if candidates.iter().all(|id| self.played.contains(*id)) {
            self.played.clear();
        }
        // At the cycle boundary, avoid immediately repeating the last track
        // when another unplayed choice exists. It remains available later.
        let alternative = candidates
            .iter()
            .any(|id| Some(*id) != current && !self.played.contains(*id));
        candidates
            .into_iter()
            .filter(|id| !self.played.contains(*id) && (!alternative || Some(*id) != current))
            .choose(&mut rand::rng())
            .map(str::to_owned)
    }

    pub fn remap_sources(&mut self, aliases: &std::collections::HashMap<String, String>) {
        self.played = std::mem::take(&mut self.played)
            .into_iter()
            .map(|id| aliases.get(&id).cloned().unwrap_or(id))
            .collect();
        if let Some(last) = &mut self.last_started
            && let Some(kept) = aliases.get(last)
        {
            *last = kept.clone();
        }
    }

    pub fn rebase(&mut self, old: &str, new: &str) {
        if let Some(id) = &mut self.last_started
            && let Some(updated) = crate::network::rebase(id, old, new)
        {
            *id = updated;
        }
        self.played = std::mem::take(&mut self.played)
            .into_iter()
            .map(|id| crate::network::rebase(&id, old, new).unwrap_or(id))
            .collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restart_at_each_track_and_after_preparation_keeps_complete_cycles() {
        let tracks: Vec<_> = (0..131).map(|i| format!("track/{i}")).collect();
        let mut cycle = Cycle::default();
        let mut previous = None;
        for _ in 0..3 {
            let mut heard = HashSet::new();
            for _ in &tracks {
                cycle = serde_json::from_str(&serde_json::to_string(&cycle).unwrap()).unwrap();
                let next = cycle
                    .choose(tracks.iter().map(String::as_str), None)
                    .unwrap();
                assert_ne!(previous.as_ref(), Some(&next));
                assert!(heard.insert(next.clone()));
                cycle.record(&next);
                // Preparation may open the next cycle, but never consumes it.
                cycle.choose(tracks.iter().map(String::as_str), Some(&next));
                previous = Some(next);
            }
            assert_eq!(heard.len(), tracks.len());
        }
    }

    #[test]
    fn complete_cycles_have_no_repeats_and_no_boundary_double() {
        let tracks: Vec<_> = (0..257).map(|i| format!("track/{i}")).collect();
        let mut cycle = Cycle::default();
        let mut current = None;
        for _ in 0..3 {
            let mut heard = HashSet::new();
            for _ in &tracks {
                let next = cycle
                    .choose(tracks.iter().map(String::as_str), current.as_deref())
                    .unwrap();
                assert_ne!(Some(&next), current.as_ref());
                assert!(
                    heard.insert(next.clone()),
                    "repeated before exhausting the cycle"
                );
                cycle.record(&next);
                current = Some(next);
            }
            assert_eq!(heard.len(), tracks.len());
        }
    }

    #[test]
    fn abandoned_preparation_is_not_consumed_and_new_tracks_join_the_cycle() {
        let mut cycle = Cycle::default();
        cycle.record("A");
        for _ in 0..64 {
            assert_eq!(
                cycle.choose(["A", "B"].into_iter(), Some("A")).as_deref(),
                Some("B")
            );
        }
        // B has only been prepared. C arrives through source synchronization.
        cycle.record("C");
        assert_eq!(
            cycle
                .choose(["A", "B", "C"].into_iter(), Some("C"))
                .as_deref(),
            Some("B")
        );
        cycle.record("B");
        assert_eq!(
            cycle
                .choose(["A", "B", "C", "D"].into_iter(), Some("B"))
                .as_deref(),
            Some("D")
        );
        // A removed track cannot be selected; existing progress is retained.
        assert_eq!(
            cycle
                .choose(["A", "C", "D"].into_iter(), Some("B"))
                .as_deref(),
            Some("D")
        );
    }

    #[test]
    fn empty_single_duplicate_and_rebased_sources() {
        let mut cycle = Cycle::default();
        assert_eq!(cycle.choose(std::iter::empty(), None), None);
        for _ in 0..3 {
            assert_eq!(
                cycle
                    .choose(["only", "only"].into_iter(), Some("only"))
                    .as_deref(),
                Some("only")
            );
            cycle.record("only");
        }
        cycle.clear();
        cycle.record("ftp://192.168.1.2:8736/Music/A.flac");
        cycle.rebase(
            "ftp://192.168.1.2:8736/Music/",
            "ftp://192.168.1.3:8736/Music/",
        );
        assert_eq!(
            cycle
                .choose(
                    [
                        "ftp://192.168.1.3:8736/Music/A.flac",
                        "ftp://192.168.1.3:8736/Music/B.flac",
                    ]
                    .into_iter(),
                    None
                )
                .as_deref(),
            Some("ftp://192.168.1.3:8736/Music/B.flac")
        );
        cycle.clear();
        assert!(cycle.played.is_empty());
    }
}
