//! Reveal the active row once per track/view/filter change, including virtualized lists.
//! No audio command, selection change or per-frame scan of the full library.
#[derive(Default)]
pub(super) struct State {
    key: Option<(String, u64, usize)>,
    frame: Option<u64>,
}
impl State {
    pub fn row(
        &mut self,
        current: Option<&str>,
        revision: u64,
        len: usize,
        frame: u64,
        find: impl FnOnce(&str) -> Option<usize>,
    ) -> Option<usize> {
        let entered = self.frame.is_none_or(|old| frame > old + 1);
        self.frame = Some(frame);
        let Some(current) = current else {
            self.key = None;
            return None;
        };
        if !entered
            && self
                .key
                .as_ref()
                .is_some_and(|(s, r, n)| s == current && *r == revision && *n == len)
        {
            return None;
        }
        self.key = Some((current.to_string(), revision, len));
        find(current)
    }
}
pub(super) fn offset(row: usize, pitch: f32, viewport: f32) -> f32 {
    ((row as f32 + 0.5) * pitch - viewport.max(pitch) * 0.5).max(0.0)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn follows_track_reentry_and_revisions_without_repeated_scans_or_filter_changes() {
        let mut s = State::default();
        assert_eq!(s.row(Some("a"), 0, 500, 1, |_| Some(300)), Some(300));
        assert_eq!(
            s.row(Some("a"), 0, 500, 2, |_| panic!("no per-frame scan")),
            None
        );
        assert_eq!(s.row(Some("b"), 0, 500, 3, |_| Some(450)), Some(450));
        assert_eq!(s.row(Some("b"), 0, 500, 7, |_| Some(450)), Some(450));
        assert_eq!(s.row(Some("b"), 1, 2, 8, |_| None), None);
        assert_eq!(offset(300, 52.0, 400.0), 15426.0);
    }
}
