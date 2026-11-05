use std::collections::VecDeque;

/// Only successfully started tracks enter history. Failed loads never move the cursor.
#[derive(Default)]
pub struct History {
    entries: VecDeque<String>,
    cursor: usize,
    pending: Option<usize>,
}

impl History {
    pub fn record(&mut self, id: String) {
        if let Some(index) = self.pending.take()
            && self.entries.get(index) == Some(&id)
        {
            self.cursor = index;
            return;
        }
        if self.entries.get(self.cursor) == Some(&id) {
            return;
        }
        // A gapless transition can follow the forward path without a manual
        // next() request. Keep the rest of that path just as the button does.
        if self.entries.get(self.cursor + 1) == Some(&id) {
            self.cursor += 1;
            return;
        }
        self.entries.truncate(self.cursor.saturating_add(1));
        self.entries.push_back(id);
        if self.entries.len() > 128 {
            self.entries.pop_front();
        }
        self.cursor = self.entries.len().saturating_sub(1);
    }

    pub fn previous(&mut self) -> Option<String> {
        self.request(self.cursor.checked_sub(1)?)
    }

    pub fn next(&mut self) -> Option<String> {
        self.request(self.cursor + 1)
    }

    pub fn peek_next(&self) -> Option<String> {
        self.entries.get(self.cursor + 1).cloned()
    }

    pub fn discard_forward(&mut self) {
        self.pending = None;
        self.entries.truncate(self.cursor.saturating_add(1));
    }

    fn request(&mut self, index: usize) -> Option<String> {
        self.pending = None;
        let id = self.entries.get(index)?.clone();
        self.pending = Some(index);
        Some(id)
    }

    pub fn remove_missing(
        &mut self,
        missing: &std::collections::HashSet<String>,
        current: Option<&str>,
    ) {
        let mut index = 0;
        let mut before_cursor = 0;
        self.entries.retain(|id| {
            let keep =
                !missing.contains(id) || (index == self.cursor && Some(id.as_str()) == current);
            if !keep && index < self.cursor {
                before_cursor += 1;
            }
            index += 1;
            keep
        });
        self.cursor = self
            .cursor
            .saturating_sub(before_cursor)
            .min(self.entries.len().saturating_sub(1));
        self.pending = None;
    }

    pub fn rebase(&mut self, old: &str, new: &str) {
        for entry in &mut self.entries {
            if let Some(rebased) = crate::network::rebase(entry, old, new) {
                *entry = rebased;
            }
        }
    }
    pub fn cancel(&mut self) {
        self.pending = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gapless_forward_and_mode_change_keep_back_navigation() {
        let mut h = History::default();
        for id in ["A", "B", "C", "D"] {
            h.record(id.into());
        }
        h.previous();
        h.record("C".into());
        h.previous();
        h.record("B".into());
        assert_eq!(h.peek_next().as_deref(), Some("C"));
        h.record("C".into());
        assert_eq!(h.peek_next().as_deref(), Some("D"));
        h.discard_forward();
        assert_eq!(h.peek_next(), None);
        assert_eq!(h.previous().as_deref(), Some("B"));
    }
    #[test]
    fn shuffle_round_trip_and_failed_load() {
        let mut h = History::default();
        for id in ["A", "C", "B"] {
            h.record(id.into());
        }
        assert_eq!(h.previous().as_deref(), Some("C"));
        h.cancel();
        assert_eq!(h.previous().as_deref(), Some("C"));
        h.record("C".into());
        assert_eq!(h.next().as_deref(), Some("B"));
        h.record("B".into());
        assert_eq!(h.next(), None);
        h.previous();
        h.record("C".into());
        h.record("D".into());
        assert_eq!(h.next(), None);
        assert_eq!(h.previous().as_deref(), Some("C"));
    }
}
