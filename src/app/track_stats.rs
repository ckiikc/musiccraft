use super::*;
impl Aurea {
    pub(super) fn persist_track_stats(&mut self) {
        if self.stats_dirty && self.stats_error.is_none() {
            match self.track_stats.save(&self.stats_path) {
                Ok(()) => self.stats_dirty = false,
                Err(_) => self.message = tr!("Could not save listening statistics.").into(),
            }
        }
    }
    pub(super) fn rate_track(&mut self, source: &str, rating: u8) {
        if self.stats_error.is_some() {
            self.message = tr!("Listening statistics unavailable.").into();
            return;
        }
        let when = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        if self.track_stats.rate(source, rating, when) {
            self.message = tr!("Rating kept in MusicCraft; audio tag update pending").into();
            self.stats_dirty = true;
            self.stats_revision = self.stats_revision.wrapping_add(1);
            self.persist_track_stats();
            #[cfg(not(test))]
            crate::jobs::rating_edit(crate::jobs::RatingEdit {
                source: source.to_string(),
                rating,
                revision: self.track_stats.get(source).edit_revision,
                auth: self.auth.clone(),
                tx: self.tx.clone(),
                ctx: self.ctx.clone(),
                scale: self.saved.rating_tag_scale,
                backups: self.stats_path.parent().unwrap().join("rating-backups"),
            });
        }
    }
    pub(super) fn remap_track_stats(&mut self, mut map: impl FnMut(&str) -> Option<String>) {
        let changed = self.track_stats.remap(&mut map);
        self.listening_counter.remap(map);
        if changed {
            self.stats_dirty = true;
            self.stats_revision = self.stats_revision.wrapping_add(1);
            self.persist_track_stats();
        }
    }
}
pub(super) const STAR_WIDTH: f32 = 110.0;
/// Five independent buttons. The title's click target never overlaps this area.
pub(super) fn rating_stars(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    id: impl std::hash::Hash + std::fmt::Debug,
    rating: u8,
    enabled: bool,
) -> Option<u8> {
    let mut changed = None;
    let preview = ui
        .input(|i| i.pointer.hover_pos())
        .filter(|p| enabled && rect.contains(*p))
        .map(|p| (((p.x - rect.left()) / (STAR_WIDTH / 5.0)) as u8 + 1).clamp(1, 5))
        .unwrap_or(rating);
    // An overlay child has its own ID and cursor, and never expands the parent
    // layout. The caller has already reserved the star area.
    let mut stars_ui = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(("track-stars", id))
            .max_rect(rect),
    );
    stars_ui.set_clip_rect(rect.intersect(ui.clip_rect()));
    stars_ui.spacing_mut().button_padding.x = 2.0;
    if !enabled {
        stars_ui.disable();
    }
    for n in 1..=5 {
        let cell = egui::Rect::from_min_size(
            rect.min + vec2(f32::from(n - 1) * STAR_WIDTH / 5.0, 0.0),
            vec2(STAR_WIDTH / 5.0, rect.height()),
        );
        let text = RichText::new(if n <= preview { "★" } else { "☆" })
            .size(17.0)
            .color(if n <= preview {
                palette::accent()
            } else {
                palette::muted()
            });
        let response = stars_ui.place(
            cell,
            egui::Button::new(text).selected(n <= rating).frame(false),
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                enabled,
                format!("{}: {n}/5", tr!("Rating")),
            )
        });
        if response.clicked() {
            changed = Some(if rating == n { 0 } else { n });
        }
        response.on_hover_text(if rating == n {
            tr!("Unrated").into()
        } else {
            format!("{}: {n}/5", tr!("Rating"))
        });
    }
    changed
}

pub(super) fn tag_rating_info(ui: &mut egui::Ui, track: &Track, scale: musiccraft_tags::Scale) {
    match &track.rating_tags {
        None => {
            ui.label(tr!("Audio tag ratings have not been read yet."));
        }
        Some(tags) => {
            if !tags.values.is_empty() {
                ui.label(format!("VOTE / RATING: {}", tags.values.join(", ")));
            }
            if tags.rating(scale).is_err() {
                ui.label(tr!(
                    "Audio tag rating is unknown: check the scale or conflicting values."
                ));
            }
        }
    }
}
pub(super) fn rating_menu(ui: &mut egui::Ui, entry: crate::track_stats::Entry) -> Option<u8> {
    ui.label(format!("{}: {}", tr!("Play count"), entry.plays));
    if entry.tag_pending {
        ui.label(tr!("Audio tag update pending"));
    }
    let mut selected = None;
    if entry.tag_pending && ui.button(tr!("Retry audio tag update")).clicked() {
        selected = Some(entry.rating);
    }
    ui.menu_button(
        format!(
            "{}: {}",
            tr!("Rating"),
            if entry.rating == 0 {
                tr!("Unrated").into()
            } else {
                format!("{}/5", entry.rating)
            }
        ),
        |ui| {
            for rating in 0..=5 {
                let label = if rating == 0 {
                    tr!("Unrated").into()
                } else {
                    format!("{} ({rating}/5)", "★".repeat(rating as usize))
                };
                if ui.selectable_label(entry.rating == rating, label).clicked() {
                    selected = Some(rating);
                    ui.close();
                }
            }
        },
    );
    selected
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepted_audio_events_count_once_exclude_seek_and_refresh_dynamic_results() {
        use crate::playlist_rules::{Comparison, Rule, Rules, Tag};
        let d = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut a = Aurea::new(&cc, d.path().into(), vec![], None, None);
        a.saved.auto_sync = false;
        let mut track = Track::placeholder("/a.wav".into());
        track.duration = 240.0;
        a.saved.tracks = vec![track.clone()];
        let (tx, _rx) = mpsc::channel();
        a.audio.tx = tx;
        let (events, rx) = mpsc::channel();
        a.audio.rx = rx;
        let id = a.playlist_book.create("Unheard", vec![]).unwrap();
        a.playlist_book
            .set_rules(
                id,
                Some(Rules {
                    all: true,
                    conditions: vec![Rule {
                        tag: Tag::PlayCount,
                        comparison: Comparison::Equals,
                        value: "0".into(),
                    }],
                }),
            )
            .unwrap();
        assert_eq!(a.playlist_tracks(id).len(), 1);
        a.rate_track(&track.source, 4);
        assert_eq!(a.track_stats.get(&track.source).rating, 4);
        let ticket = a.ticket;
        let started = |ticket| Event::Started {
            ticket,
            track: track.clone(),
            output: "synthetic".into(),
            cover: None,
            exclusive: false,
        };
        events.send(started(ticket + 99)).unwrap();
        events.send(Event::Position(50.0, false)).unwrap();
        a.poll(&ctx);
        assert_eq!(
            a.track_stats.get(&track.source).plays,
            0,
            "stale starts never count"
        );
        events.send(started(ticket)).unwrap();
        a.poll(&ctx);
        a.listening_counter.allow_test_elapsed(10);
        events.send(Event::Position(10.0, false)).unwrap();
        a.poll(&ctx);
        events.send(Event::Seeked(ticket)).unwrap();
        events.send(Event::Position(200.0, false)).unwrap();
        a.poll(&ctx);
        a.listening_counter.allow_test_elapsed(15);
        events.send(Event::Position(215.0, false)).unwrap();
        a.poll(&ctx);
        assert_eq!(a.track_stats.get(&track.source).plays, 0);
        a.listening_counter.allow_test_elapsed(5);
        events.send(Event::Position(220.0, false)).unwrap();
        events.send(Event::Position(225.0, false)).unwrap();
        a.poll(&ctx);
        assert_eq!(a.track_stats.get(&track.source).plays, 1);
        assert!(a.playlist_tracks(id).is_empty());
        let saved = crate::track_stats::Stats::load(&a.stats_path).unwrap();
        assert_eq!(saved.get(&track.source).rating, 4);
        assert_eq!(saved.get(&track.source).plays, 1);
        assert!(a.playback_selection.is_none());
    }
}

#[cfg(test)]
mod inline_tests {
    use super::*;
    use crate::app::playlists::Page;
    fn frame(
        a: &mut Aurea,
        ctx: &egui::Context,
        tick: &mut u32,
        size: egui::Vec2,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        *tick += 1;
        let mut o = ctx.run_ui(
            egui::RawInput {
                time: Some(*tick as f64 * 0.04),
                screen_rect: Some(egui::Rect::from_min_size(pos2(0.0, 0.0), size)),
                events,
                ..Default::default()
            },
            |ui| a.desktop_ui(ui),
        );
        o.textures_delta.clear();
        o
    }
    fn star_click(a: &mut Aurea, ctx: &egui::Context, tick: &mut u32, size: egui::Vec2, n: u8) {
        frame(a, ctx, tick, size, vec![]);
        let o = frame(a, ctx, tick, size, vec![]);
        let label = format!("{}: {n}/5", tr!("Rating"));
        let nodes = o.platform_output.accesskit_update.unwrap().nodes;
        let star_bounds: Vec<_> = nodes
            .iter()
            .filter(|(_, node)| node.label() == Some(label.as_str()))
            .filter_map(|(_, node)| node.bounds())
            .collect();
        if let Some(first) = star_bounds.first() {
            assert!(
                star_bounds.iter().all(|b| (b.x0 - first.x0).abs() < 0.5),
                "rating columns must not drift between rows"
            );
        }
        let b = nodes
            .iter()
            .filter(|(_, node)| node.label() == Some(label.as_str()))
            .filter_map(|(_, node)| node.bounds())
            .min_by(|a, b| a.y0.total_cmp(&b.y0))
            .expect("visible star control");
        assert!(
            b.x0 >= 0.0 && b.x1 <= f64::from(size.x),
            "stars must fit the window"
        );
        let pos = pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32);
        frame(a, ctx, tick, size, vec![egui::Event::PointerMoved(pos)]);
        for pressed in [true, false] {
            frame(
                a,
                ctx,
                tick,
                size,
                vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
        }
    }
    #[test]
    fn stars_set_clear_and_persist_without_selecting_playing_or_changing_the_queue() {
        for size in [vec2(640.0, 640.0), vec2(1120.0, 760.0)] {
            for page in [Page::Library, Page::Playlists, Page::Queue] {
                let d = tempfile::tempdir().unwrap();
                let ctx = egui::Context::default();
                let cc = eframe::CreationContext::_new_kittest(ctx.clone());
                let mut a = Aurea::new(&cc, d.path().into(), vec![], None, None);
                a.saved.auto_sync = false;
                a.saved.mini = false;
                a.saved.facets_visible = false;
                a.online_covers.store(false, Ordering::Relaxed);
                a.saved.tracks = (1..=2)
                    .map(|n| {
                        let mut t = Track::placeholder(format!("/synthetic/{n}.wav"));
                        t.title = format!("Track {n}");
                        t.artist = "Fixture".into();
                        t
                    })
                    .collect();
                let ids: Vec<_> = a.saved.tracks.iter().map(|t| t.source.clone()).collect();
                a.create_playlist("Manual", ids.clone());
                a.playback_selection = Some(ids.clone());
                a.saved.queue_label = Some("Original".into());
                a.playlist_ui.page = page;
                let (tx, commands) = mpsc::channel();
                a.audio.tx = tx;
                let mut tick = 0;
                star_click(&mut a, &ctx, &mut tick, size, 4);
                assert_eq!(a.track_stats.get(&ids[0]).rating, 4);
                assert_eq!(
                    crate::track_stats::Stats::load(&a.stats_path)
                        .unwrap()
                        .get(&ids[0])
                        .rating,
                    4
                );
                assert!(a.browser.selected_track.is_none());
                assert!(a.playlist_ui.row.is_none());
                assert!(a.current.is_none() && a.requested.is_none());
                assert!(commands.try_recv().is_err());
                assert_eq!(a.playback_selection.as_ref(), Some(&ids));
                star_click(&mut a, &ctx, &mut tick, size, 4);
                assert_eq!(a.track_stats.get(&ids[0]).rating, 0);
                assert!(
                    commands.try_recv().is_err(),
                    "even repeated star clicks must never play a track"
                );
                assert_eq!(a.playback_selection.as_ref(), Some(&ids));
            }
        }
    }
    #[test]
    fn active_rows_far_outside_the_viewport_are_rendered_in_browser_and_queue() {
        let d = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut a = Aurea::new(&cc, d.path().into(), vec![], None, None);
        a.saved.auto_sync = false;
        a.saved.mini = false;
        a.saved.facets_visible = false;
        a.saved.tracks = (0..500)
            .map(|n| {
                let mut t = Track::placeholder(format!("/{n}.flac"));
                t.title = format!("Track{n:03}");
                t.artist = "QA".into();
                t.metadata_loaded = true;
                t.rating_tags = Some(Default::default());
                t
            })
            .collect();
        let ids: Vec<_> = a.saved.tracks.iter().map(|t| t.source.clone()).collect();
        a.playback_selection = Some(ids);
        a.saved.queue_label = Some("Stable queue".into());
        let (tx, rx) = mpsc::channel();
        a.audio.tx = tx;
        let mut tick = 0;
        for facets in [false, true] {
            a.saved.facets_visible = facets;
            for page in [Page::Library, Page::Queue] {
                a.playlist_ui.page = page;
                for current in [400, 420] {
                    a.current = Some(format!("/{current}.flac"));
                    frame(&mut a, &ctx, &mut tick, vec2(1120.0, 760.0), vec![]);
                    let out = frame(&mut a, &ctx, &mut tick, vec2(1120.0, 760.0), vec![]);
                    let labels: Vec<_> = out
                        .platform_output
                        .accesskit_update
                        .unwrap()
                        .nodes
                        .into_iter()
                        .filter_map(|(_, n)| n.label().map(str::to_owned))
                        .collect();
                    let neighbor = format!("Track{:03}", current + 1);
                    assert!(
                        labels.iter().any(|s| s.contains(&neighbor)),
                        "playing row and neighboring rows must be in the virtualized viewport: {labels:?}"
                    );
                }
            }
        }
        assert_eq!(a.playback_selection.as_ref().unwrap().len(), 500);
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn imported_tag_rating_and_edits_refresh_rules_without_queue_changes() {
        use crate::playlist_rules::{Comparison, Rule, Rules, Tag};
        let d = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut a = Aurea::new(&cc, d.path().into(), vec![], None, None);
        a.saved.auto_sync = false;
        a.saved.rating_tag_scale = musiccraft_tags::Scale::Five;
        let mut t = Track::placeholder("/imported.flac".into());
        t.rating_tags = Some(musiccraft_tags::Summary {
            values: vec!["4".into()],
            issue: None,
        });
        a.saved.tracks = vec![t];
        a.create_playlist("Imported votes", vec![]);
        let id = a.playlist_ui.selected.unwrap();
        a.playlist_book
            .set_rules(
                id,
                Some(Rules {
                    all: true,
                    conditions: vec![Rule {
                        tag: Tag::Rating,
                        comparison: Comparison::AtLeast,
                        value: "4".into(),
                    }],
                }),
            )
            .unwrap();
        let queue = vec!["/imported.flac".into(), "/another.flac".into()];
        a.playback_selection = Some(queue.clone());
        a.saved.queue_label = Some("Existing queue".into());
        assert_eq!(
            a.playlist_tracks(id).as_ref(),
            &vec!["/imported.flac".to_string()]
        );
        let (tx, rx) = mpsc::channel();
        a.audio.tx = tx;
        a.rate_track("/imported.flac", 2);
        assert!(a.playlist_tracks(id).is_empty());
        a.rate_track("/imported.flac", 5);
        assert_eq!(a.playlist_tracks(id).len(), 1);
        assert_eq!(a.playback_selection, Some(queue));
        assert!(rx.try_recv().is_err());
        // A completion from an earlier identical vote cannot clear a newer pending edit.
        let revision = a.track_stats.get("/imported.flac").edit_revision;
        a.tx.send(Job::RatingWritten {
            revision: revision - 1,
            source: "/imported.flac".into(),
            rating: 5,
            result: Ok(Default::default()),
        })
        .unwrap();
        a.poll(&ctx);
        assert!(a.track_stats.get("/imported.flac").tag_pending);
    }
    #[test]
    fn inline_change_updates_a_dynamic_playlist_without_affecting_its_queue_snapshot() {
        use crate::playlist_rules::{Comparison, Rule, Rules, Tag};
        let d = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut a = Aurea::new(&cc, d.path().into(), vec![], None, None);
        a.saved.auto_sync = false;
        a.saved.mini = false;
        a.saved.tracks = ["/a.wav", "/b.wav"]
            .into_iter()
            .map(|s| {
                let mut t = Track::placeholder(s.into());
                t.rating_tags = Some(Default::default());
                t
            })
            .collect();
        let ids: Vec<_> = a.saved.tracks.iter().map(|t| t.source.clone()).collect();
        a.create_playlist("Low rated", vec![]);
        let id = a.playlist_ui.selected.unwrap();
        a.playlist_book
            .set_rules(
                id,
                Some(Rules {
                    all: true,
                    conditions: vec![Rule {
                        tag: Tag::Rating,
                        comparison: Comparison::AtMost,
                        value: "3".into(),
                    }],
                }),
            )
            .unwrap();
        a.playback_selection = Some(ids.clone());
        a.saved.queue_label = Some("Snapshot".into());
        a.playlist_ui.page = Page::Playlists;
        let (tx, commands) = mpsc::channel();
        a.audio.tx = tx;
        let mut tick = 0;
        star_click(&mut a, &ctx, &mut tick, vec2(1120.0, 760.0), 4);
        frame(&mut a, &ctx, &mut tick, vec2(1120.0, 760.0), vec![]);
        assert_eq!(a.playlist_tracks(id).as_slice(), ["/b.wav"]);
        assert_eq!(a.track_stats.get("/a.wav").rating, 4);
        assert_eq!(a.track_stats.get("/b.wav").rating, 0);
        assert_eq!(a.playback_selection, Some(ids));
        assert!(commands.try_recv().is_err());
    }
}
