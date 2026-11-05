//! Cached counts for the selected source, shared by desktop and mini; no per-frame library scan.
use super::*;
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(super) struct Counts {
    pub read: usize,
    pub total: usize,
    pub failed: usize,
}
impl Counts {
    fn pending(self) -> usize {
        self.total
            .saturating_sub(self.read.saturating_add(self.failed))
    }
    fn is_visible(self) -> bool {
        self.pending() > 0
    }
    fn processed(self) -> usize {
        self.read.saturating_add(self.failed).min(self.total)
    }
    fn label(self) -> String {
        if self.pending() == 0 {
            return crate::i18n::format(
                "Tag indexing complete: {read} read · {failed} errors",
                &[
                    ("read", self.read.to_string()),
                    ("failed", self.failed.to_string()),
                ],
            );
        }
        let mut label = crate::i18n::format(
            "Tags: {read}/{total} · {remaining} remaining",
            &[
                ("read", self.read.to_string()),
                ("total", self.total.to_string()),
                ("remaining", self.pending().to_string()),
            ],
        );
        if self.failed > 0 {
            label.push_str(" · ");
            label.push_str(&crate::i18n::format(
                "Tag read errors: {n}",
                &[("n", self.failed.to_string())],
            ));
        }
        label
    }
}
#[derive(Default)]
pub(super) struct Cache {
    key: Option<(u64, u64, usize, library::Scope)>,
    counts: Counts,
    #[cfg(test)]
    rebuilds: usize,
}
impl Cache {
    fn get(
        &mut self,
        tracks: &[Track],
        scope: library::Scope,
        revision: u64,
        index: &crate::remote_tags::Library,
    ) -> Counts {
        let key = (revision, index.revision(), tracks.len(), scope);
        if self.key.as_ref() != Some(&key) {
            self.counts = Counts::default();
            for track in tracks.iter().filter(|t| key.3.contains(&t.source)) {
                self.counts.total += 1;
                let read = track.metadata_loaded || track.rate > 0;
                self.counts.read += usize::from(read);
                self.counts.failed += usize::from(!read && index.has_failed(&track.source));
            }
            self.key = Some(key);
            #[cfg(test)]
            {
                self.rebuilds += 1;
            }
        }
        self.counts
    }
}
impl Aurea {
    pub(super) fn tag_progress_indicator(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        small: bool,
    ) {
        if self.connection_indicator(ui, rect, small) {
            return;
        }
        let scope = self.selected_scope();
        let counts = self.tag_progress.get(
            &self.saved.tracks,
            scope,
            self.library_revision,
            &self.remote_tags,
        );
        if !counts.is_visible() {
            return;
        }
        let remaining = counts.pending();
        let label = counts.label();
        let paused = !self.saved.auto_sync || self.remote_tags.is_paused();
        let state = if remaining == 0 {
            tr!("Tag indexing complete")
        } else if paused {
            tr!("Tag indexing paused")
        } else if self.remote_tags.waiting_for_source() {
            tr!("Waiting for tag sources")
        } else {
            tr!("Tag indexing in progress")
        };
        ui.put(
            egui::Rect::from_min_size(rect.min, vec2(rect.width(), 14.0)),
            egui::Label::new(
                RichText::new(&label)
                    .size(if small { 9.5 } else { 11.0 })
                    .color(palette::muted()),
            )
            .truncate()
            .selectable(false),
        )
        .on_hover_text(format!("{}\n{}\n{}", self.source_title(), label, state));
        let rail = egui::Rect::from_min_size(
            pos2(rect.left(), rect.bottom() - 2.0),
            vec2(rect.width(), 2.0),
        );
        ui.painter().rect_filled(rail, 1.0, palette::line());
        let fill = egui::Rect::from_min_size(
            rail.min,
            vec2(
                rail.width() * counts.processed() as f32 / counts.total as f32,
                2.0,
            ),
        );
        ui.painter().rect_filled(
            fill,
            1.0,
            if paused && remaining > 0 {
                palette::muted()
            } else {
                palette::accent()
            },
        );
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn eight_file_errors_finish_the_pass_without_claiming_their_tags_were_read() {
        let counts = Counts {
            read: 7237,
            total: 7245,
            failed: 8,
        };
        assert_eq!(counts.pending(), 0);
        assert!(!counts.is_visible());
        assert!(!Counts::default().is_visible());
        assert!(
            !Counts {
                read: 8,
                total: 8,
                failed: 0
            }
            .is_visible()
        );
        assert_eq!(counts.processed(), 7245);
        let label = counts.label();
        assert!(label.contains("7237"));
        assert!(label.contains("8"));
        assert!(!label.contains("remaining"));
        assert!(!label.contains("da leggere"));
        let with_new_track = Counts {
            total: 7246,
            ..counts
        };
        assert_eq!(with_new_track.pending(), 1);
        assert!(with_new_track.is_visible());
        assert_eq!(with_new_track.processed(), 7245);
        let offline = Counts {
            read: 7237,
            total: 7245,
            failed: 0,
        };
        assert_eq!(offline.pending(), 8);
        assert!(offline.is_visible());
        assert_eq!(offline.processed(), 7237);
    }
    use super::*;
    #[test]
    fn counts_follow_source_metadata_and_new_tracks_without_rescanning_animation_frames() {
        let mut tracks = vec![
            Track::placeholder("ftp://phone/Music/Queen/01.flac".into()),
            Track::placeholder("/nas/02.flac".into()),
        ];
        tracks[1].rate = 44100; // profiles from before metadata_loaded existed
        let mut cache = Cache::default();
        let index = crate::remote_tags::Library::default();
        assert_eq!(
            cache.get(&tracks, library::Scope::All, 0, &index),
            Counts {
                read: 1,
                total: 2,
                failed: 0
            }
        );
        for _ in 0..300 {
            cache.get(&tracks, library::Scope::All, 0, &index);
        }
        assert_eq!(cache.rebuilds, 1);
        let phone = library::Scope::Remote("ftp://phone/".into());
        assert_eq!(
            cache.get(&tracks, phone.clone(), 0, &index),
            Counts {
                read: 0,
                total: 1,
                failed: 0
            }
        );
        tracks[0].metadata_loaded = true;
        assert_eq!(
            cache.get(&tracks, phone.clone(), 1, &index),
            Counts {
                read: 1,
                total: 1,
                failed: 0
            }
        );
        tracks.push(Track::placeholder("ftp://phone/Music/Queen/02.flac".into()));
        assert_eq!(
            cache.get(&tracks, phone, 2, &index),
            Counts {
                read: 1,
                total: 2,
                failed: 0
            }
        );
        assert_eq!(
            cache.get(&tracks, library::Scope::Missing, 2, &index),
            Counts {
                read: 0,
                total: 0,
                failed: 0
            }
        );
    }
}
