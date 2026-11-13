use super::*;
use crate::playlist_rules::{Comparison, Rule, Rules, Tag};
use crate::playlists::Book;

struct RuleDraft {
    id: Option<u64>,
    name: String,
    rules: Rules,
}

#[derive(Default, PartialEq, Eq, Clone, Copy)]
pub(super) enum Page {
    #[default]
    Library,
    Playlists,
    Queue,
}
#[derive(Default)]
pub(super) struct State {
    pub page: Page,
    pub selected: Option<u64>,
    name: String,
    pub(super) row: Option<usize>,
    queue_row: Option<usize>,
    queue_follow: super::follow::State,
    list_follow: super::follow::State,
    pub(super) picker: bool,
    filter: String,
    picked: HashSet<String>,
    deleting: bool,
    track_index: HashMap<String, usize>,
    indexed: Option<(u64, usize, u64)>,
    picker_key: Option<(u64, usize, String)>,
    picker_indices: Arc<Vec<usize>>,
    resolved: HashMap<u64, Arc<Vec<String>>>,
    draft: Option<RuleDraft>,
}
impl Aurea {
    fn refresh_playlist_index(&mut self) {
        let key = (
            self.library_revision,
            self.saved.tracks.len(),
            self.stats_revision,
        );
        if self.playlist_ui.indexed != Some(key) {
            self.playlist_ui.track_index = self
                .saved
                .tracks
                .iter()
                .enumerate()
                .map(|(i, t)| (t.source.clone(), i))
                .collect();
            self.playlist_ui.indexed = Some(key);
            self.playlist_ui.resolved.clear();
        }
    }

    pub(super) fn playlist_tracks(&mut self, id: u64) -> Arc<Vec<String>> {
        self.refresh_playlist_index();
        if let Some(list) = self.playlist_book.get(id).filter(|p| p.rules.is_none()) {
            return list.tracks.clone();
        }
        if self.stats_error.is_some()
            && self
                .playlist_book
                .get(id)
                .and_then(|p| p.rules.as_ref())
                .is_some_and(Rules::uses_stats)
        {
            return Arc::default();
        }
        self.playlist_ui.resolved.retain(|&cached, _| cached == id);
        self.playlist_ui
            .resolved
            .entry(id)
            .or_insert_with(|| {
                self.playlist_book.resolve_with_scale(
                    id,
                    &self.saved.tracks,
                    &self.track_stats,
                    self.saved.rating_tag_scale,
                )
            })
            .clone()
    }
    pub(super) fn open_playlists(&mut self, page: Page) {
        self.playlist_ui.page = page;
        self.saved.mini = false;
    }
    pub(super) fn persist_playlists(&mut self) {
        if self.playlist_dirty && self.playlist_error.is_none() {
            match self.playlist_book.save(&self.playlist_path) {
                Ok(()) => self.playlist_dirty = false,
                Err(_) => self.message = tr!("Could not save playlist.").into(),
            }
        }
    }
    fn edit_playlists(&mut self, edit: impl FnOnce(&mut Book) -> Result<(), String>) -> bool {
        if self.playlist_error.is_some() {
            return false;
        }
        let mut next = self.playlist_book.clone();
        if edit(&mut next)
            .and_then(|()| next.save(&self.playlist_path))
            .is_err()
        {
            self.message = tr!("Could not save playlist.").into();
            return false;
        }
        self.playlist_book = next;
        self.playlist_ui.resolved.clear();
        self.playlist_dirty = false;
        true
    }
    pub(super) fn select_playlist(&mut self, id: u64) {
        self.playlist_ui.draft = None;
        self.playlist_ui.selected = Some(id);
        self.playlist_ui.name = self
            .playlist_book
            .get(id)
            .map(|p| p.name.clone())
            .unwrap_or_default();
        self.playlist_ui.row = None;
        self.playlist_ui.deleting = false;
        self.playlist_ui.picker = false;
        self.playlist_ui.picked.clear();
    }
    pub(super) fn create_playlist(&mut self, name: &str, tracks: Vec<String>) {
        let mut created = None;
        if self.edit_playlists(|book| {
            created = Some(book.create(name, tracks)?);
            Ok(())
        }) {
            self.select_playlist(created.unwrap());
        }
    }
    fn start_playlist(&mut self, id: u64, row: usize) {
        let Some(list) = self.playlist_book.get(id).cloned() else {
            return;
        };
        let tracks = self.playlist_tracks(id);
        let Some(start) = tracks.get(row).cloned() else {
            return;
        };
        if !self.saved.tracks.iter().any(|t| t.source == start) {
            return;
        }
        if !self.track_connection_ready(&start) || self.awaiting_credentials(&start) {
            self.message = tr!("Checking…").into();
            return;
        }
        // A snapshot: later playlist edits never mutate this queue or interrupt its audio.
        self.saved.queue_label = Some(list.name);
        self.playback_selection = Some(tracks.as_ref().clone());
        self.restored_selection = true;
        self.history = History::default();
        self.shuffle_cycle.clear();
        self.play(start);
    }
    fn queue_changed(&mut self, ids: Vec<String>) {
        self.playback_selection = Some(ids);
        self.saved
            .queue_label
            .get_or_insert_with(|| tr!("Playback queue").into());
        self.restored_selection = true;
        self.history.discard_forward();
        self.shuffle_cycle.clear();
        if let Some(id) = self.current.as_ref() {
            self.shuffle_cycle.record(id);
        }
        self.prepare_next();
    }
    pub(super) fn add_results_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button(tr!("Add to playlist…"), |ui| {
            self.refresh_browser();
            let count = self.browser.indices.len();
            ui.label(format!(
                "{} · {}",
                tr!("Filtered selection"),
                crate::i18n::tracks(count)
            ));
            ui.separator();
            ui.add_enabled_ui(count > 0 && self.playlist_error.is_none(), |ui| {
                if ui.button(tr!("New playlist")).clicked() {
                    let tracks = self
                        .browser
                        .indices
                        .iter()
                        .map(|&i| self.saved.tracks[i].source.clone())
                        .collect();
                    self.create_playlist(tr!("New playlist"), tracks);
                    self.playlist_ui.page = Page::Playlists;
                    ui.close();
                }
                let lists: Vec<_> = self
                    .playlist_book
                    .lists
                    .iter()
                    .filter(|p| p.rules.is_none())
                    .map(|p| (p.id, p.name.clone()))
                    .collect();
                for (id, name) in lists {
                    if ui.button(name).clicked() {
                        let tracks: Vec<_> = self
                            .browser
                            .indices
                            .iter()
                            .map(|&i| self.saved.tracks[i].source.clone())
                            .collect();
                        self.edit_playlists(|b| b.append(id, tracks));
                        ui.close();
                    }
                }
            });
        });
    }
    fn import_named_playlist(&mut self, path: &std::path::Path) {
        if self.scanning {
            self.message = tr!("Wait or cancel the current scan").into();
            return;
        }
        let Ok(ids) = library::playlist(path) else {
            self.message = tr!("Playlists unavailable.").into();
            return;
        };
        let known: HashSet<_> = self
            .saved
            .tracks
            .iter()
            .map(|t| t.source.as_str())
            .collect();
        let extra = ids.iter().filter(|s| !known.contains(s.as_str())).count();
        if self.saved.tracks.len() + extra > library::MAX_TRACKS {
            self.message = tr!("Could not save playlist.").into();
            return;
        }
        let name = path.file_stem().unwrap_or_default().to_string_lossy();
        let before = self.playlist_book.lists.len();
        self.create_playlist(&self.playlist_book.unique_name(&name), ids.clone());
        if self.playlist_book.lists.len() == before {
            return;
        }
        if extra > 0 {
            self.import(vec![path.to_path_buf()]);
        }
    }
    pub(super) fn playlists_page(&mut self, ui: &mut egui::Ui) {
        if let Some(error) = &self.playlist_error {
            ui.colored_label(palette::accent(), error);
            return;
        }
        self.refresh_playlist_index();
        let compact = ui.available_width() < 680.0;
        let lists: Vec<_> = self
            .playlist_book
            .lists
            .iter()
            .map(|p| (p.id, p.name.clone(), p.rules.is_some()))
            .collect();
        let summaries: Vec<_> = lists
            .into_iter()
            .map(|(id, name, dynamic)| {
                let count = if !dynamic || self.playlist_ui.selected == Some(id) {
                    Some(self.playlist_tracks(id).len())
                } else {
                    None
                };
                (id, name, count, dynamic)
            })
            .collect();
        ui.horizontal_wrapped(|ui| {
            if compact {
                let name = self
                    .playlist_ui
                    .selected
                    .and_then(|id| self.playlist_book.get(id))
                    .map(|p| {
                        summaries
                            .iter()
                            .find(|x| x.0 == p.id)
                            .map(|x| format!("{} · {}", x.1, x.2.unwrap_or(0)))
                            .unwrap_or_default()
                    })
                    .unwrap_or_else(|| tr!("No playlists yet").into());
                egui::ComboBox::from_id_salt("playlist-compact-selector")
                    .selected_text(name)
                    .show_ui(ui, |ui| {
                        let lists: Vec<_> = self
                            .playlist_book
                            .lists
                            .iter()
                            .map(|p| (p.id, p.name.clone()))
                            .collect();
                        for (id, name) in lists {
                            if ui
                                .selectable_label(self.playlist_ui.selected == Some(id), name)
                                .clicked()
                            {
                                self.select_playlist(id);
                            }
                        }
                    });
            }
            if ui.button(tr!("New playlist")).clicked() {
                self.create_playlist(tr!("New playlist"), vec![]);
            }
            if ui.button(tr!("New dynamic playlist")).clicked() {
                self.playlist_ui.draft = Some(RuleDraft {
                    id: None,
                    name: tr!("Dynamic playlist").into(),
                    rules: Rules::default(),
                });
            }
            if ui.button(tr!("Import playlist…")).clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("M3U", &["m3u", "m3u8"])
                    .pick_file()
            {
                self.import_named_playlist(&path);
            }
        });
        if self.playlist_ui.draft.is_none()
            && self
                .playlist_ui
                .selected
                .and_then(|id| self.playlist_book.get(id))
                .is_none()
            && let Some(id) = self.playlist_book.lists.first().map(|p| p.id)
        {
            self.select_playlist(id);
        }
        ui.add_space(8.0);
        let width = ui.available_width();
        if width >= 680.0 {
            egui::Panel::left("named-playlists-list")
                .resizable(true)
                .default_size(190.0)
                .size_range(150.0..=260.0)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("playlist-names")
                        .show(ui, |ui| {
                            for (id, name, count, dynamic) in &summaries {
                                if ui
                                    .selectable_label(
                                        self.playlist_ui.selected == Some(*id),
                                        format!(
                                            "{name}{}{}",
                                            count.map(|n| format!(" · {n}")).unwrap_or_default(),
                                            if *dynamic { " ⟳" } else { "" }
                                        ),
                                    )
                                    .clicked()
                                {
                                    self.select_playlist(*id);
                                }
                            }
                        });
                });
        }
        // Keep every editor state away from the sidebar separator, including empty/error/draft views.
        egui::Frame::new()
            .inner_margin(egui::Margin {
                left: if width >= 680.0 { 16 } else { 8 },
                right: 8,
                top: 8,
                bottom: 0,
            })
            .show(ui, |ui| {
                if self.playlist_ui.draft.is_some() {
                    self.playlist_rule_editor(ui);
                } else {
                    self.playlist_editor(ui);
                }
            });
    }
    fn playlist_editor(&mut self, ui: &mut egui::Ui) {
        self.refresh_playlist_index();
        let Some(list) = self
            .playlist_ui
            .selected
            .and_then(|id| self.playlist_book.get(id))
            .cloned()
        else {
            ui.add_space(24.0);
            ui.heading(tr!("No playlists yet"));
            return;
        };
        let tracks = self.playlist_tracks(list.id);
        if self.playlist_ui.picker {
            self.playlist_picker(ui, list.id);
            return;
        }
        let compact = ui.available_width() < 680.0 || ui.available_height() < 330.0;
        ui.horizontal_wrapped(|ui| {
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.playlist_ui.name)
                    .char_limit(120)
                    .desired_width(200.0)
                    .hint_text(tr!("Playlist name")),
            );
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, tr!("Playlist name"))
            });
            if ui
                .add_enabled(
                    !self.playlist_ui.name.trim().is_empty(),
                    egui::Button::new(tr!("Rename")),
                )
                .clicked()
            {
                let name = self.playlist_ui.name.clone();
                self.edit_playlists(|b| {
                    if b.rename(list.id, &name) {
                        Ok(())
                    } else {
                        Err(String::new())
                    }
                });
            }
            if ui.button(tr!("Duplicate")).clicked() {
                let mut created = None;
                if self.edit_playlists(|b| {
                    created = Some(b.duplicate(list.id)?);
                    Ok(())
                }) {
                    self.select_playlist(created.unwrap());
                }
                return;
            }
            if ui.button(tr!("Delete")).clicked() {
                self.playlist_ui.deleting = true;
            }
        });
        if self.playlist_ui.deleting {
            ui.horizontal_wrapped(|ui| {
                ui.label(tr!("Delete playlist?"));
                if ui.button(tr!("Delete")).clicked() {
                    self.edit_playlists(|b| {
                        b.remove(list.id);
                        Ok(())
                    });
                    self.playlist_ui.selected = None;
                    self.playlist_ui.deleting = false;
                }
                if ui.button(tr!("Cancel")).clicked() {
                    self.playlist_ui.deleting = false;
                }
            });
            return;
        }
        ui.add_space(5.0);
        ui.horizontal_wrapped(|ui| {
            let first = tracks
                .iter()
                .position(|s| self.playlist_ui.track_index.contains_key(s));
            if ui
                .add_enabled(first.is_some(), egui::Button::new(tr!("Play")))
                .clicked()
            {
                self.start_playlist(list.id, first.unwrap());
            }
            if list.rules.is_some() && ui.button(tr!("Edit rules…")).clicked() {
                self.playlist_ui.draft = Some(RuleDraft {
                    id: Some(list.id),
                    name: list.name.clone(),
                    rules: list.rules.clone().unwrap(),
                });
            }
            if list.rules.is_none() && ui.button(tr!("Add tracks…")).clicked() {
                self.playlist_ui.picker = true;
                self.playlist_ui.picked.clear();
            }
            if ui
                .add_enabled(
                    !(self.stats_error.is_some()
                        && list.rules.as_ref().is_some_and(Rules::uses_stats)),
                    egui::Button::new(tr!("Export playlist…")),
                )
                .clicked()
            {
                let name: String = list
                    .name
                    .chars()
                    .filter(|c| !c.is_control() && !['/', '\\', ':'].contains(c))
                    .collect();
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("M3U8", &["m3u8"])
                    .set_file_name(format!("{name}.m3u8"))
                    .save_file()
                    && self
                        .playlist_book
                        .m3u_with_scale(
                            list.id,
                            &self.saved.tracks,
                            &self.track_stats,
                            self.saved.rating_tag_scale,
                        )
                        .and_then(|s| crate::playlists::atomic_write(&path, s.as_bytes()))
                        .is_err()
                {
                    self.message = tr!("Could not save playlist.").into();
                }
            }
            if compact && list.rules.is_none() {
                self.playlist_edit_actions(ui, list.id, list.tracks.len());
            } else {
                caption(ui, &crate::i18n::tracks(tracks.len()));
            }
        });
        if self.playlist_ui.picker {
            self.playlist_picker(ui, list.id);
            return;
        }
        if !compact {
            ui.add(
                egui::Label::new(
                    RichText::new(tr!("Playlist changes do not change the playback queue."))
                        .small()
                        .color(palette::muted()),
                )
                .wrap(),
            );
        }
        if list.rules.as_ref().is_some_and(Rules::uses_stats) && self.stats_error.is_some() {
            ui.colored_label(palette::accent(), tr!("Listening statistics unavailable."));
        }
        if list.rules.is_some() {
            caption(ui, tr!("Updates automatically with the library."));
        } else if !compact {
            ui.horizontal(|ui| self.playlist_edit_actions(ui, list.id, list.tracks.len()));
        }
        let tracks = self.playlist_tracks(list.id);
        if tracks.is_empty() && list.rules.is_some() {
            ui.label(tr!("No matching tracks"));
        }
        if let Some(row) = self.playlist_rows(ui, &tracks, false) {
            self.start_playlist(list.id, row);
        }
    }
    fn playlist_edit_actions(&mut self, ui: &mut egui::Ui, id: u64, count: usize) {
        let row = self.playlist_ui.row;
        let mut movement = None;

        if ui
            .add_enabled_ui(row.is_some_and(|r| r > 0), |ui| {
                self.icons.button(ui, "chevron-up", tr!("Move up"), false)
            })
            .inner
            .clicked()
        {
            movement = row.map(|r| (r, r - 1));
        }
        if ui
            .add_enabled_ui(row.is_some_and(|r| r + 1 < count), |ui| {
                self.icons
                    .button(ui, "chevron-down", tr!("Move down"), false)
            })
            .inner
            .clicked()
        {
            movement = row.map(|r| (r, r + 1));
        }
        let remove = ui
            .add_enabled_ui(row.is_some_and(|r| r < count), |ui| {
                self.icons
                    .button(ui, "x", tr!("Remove from playlist"), false)
            })
            .inner
            .clicked();
        if let Some((from, to)) = movement {
            self.edit_playlists(|b| {
                b.move_track(id, from, to);
                Ok(())
            });
            self.playlist_ui.row = Some(to);
        }
        if remove && let Some(row) = row {
            self.edit_playlists(|b| {
                b.remove_track(id, row);
                Ok(())
            });
            self.playlist_ui.row = None;
        }
    }
    #[cfg(debug_assertions)]
    pub(super) fn preview_dynamic_playlist(&mut self, edit: bool) {
        self.create_playlist("Jazz · 2020–2029", vec![]);
        let id = self.playlist_ui.selected.unwrap();
        let rules = Rules {
            all: true,
            conditions: vec![
                Rule {
                    tag: Tag::Genre,
                    comparison: Comparison::Equals,
                    value: "Jazz".into(),
                },
                Rule {
                    tag: Tag::Year,
                    comparison: Comparison::AtLeast,
                    value: "2020".into(),
                },
                Rule {
                    tag: Tag::Year,
                    comparison: Comparison::AtMost,
                    value: "2029".into(),
                },
            ],
        };
        self.edit_playlists(|b| b.set_rules(id, Some(rules.clone())));
        self.playlist_ui.page = Page::Playlists;
        if edit {
            self.playlist_ui.draft = Some(RuleDraft {
                id: Some(id),
                name: "Jazz · 2020–2029".into(),
                rules,
            });
        }
    }
    #[cfg(debug_assertions)]
    pub(super) fn preview_stats_playlist(&mut self, edit: bool) {
        let ids: Vec<_> = self
            .saved
            .tracks
            .iter()
            .take(3)
            .map(|t| t.source.clone())
            .collect();
        for (n, id) in ids.iter().enumerate() {
            self.rate_track(id, if n == 1 { 4 } else { 5 });
            for _ in 0..[1, 2, 10][n] {
                self.track_stats.played(id);
            }
        }
        self.stats_dirty = true;
        self.stats_revision += 1;
        self.persist_track_stats();
        self.create_playlist("Preferiti da riscoprire", vec![]);
        let id = self.playlist_ui.selected.unwrap();
        let rules = Rules {
            all: true,
            conditions: vec![
                Rule {
                    tag: Tag::Rating,
                    comparison: Comparison::AtLeast,
                    value: "4".into(),
                },
                Rule {
                    tag: Tag::PlayCount,
                    comparison: Comparison::AtMost,
                    value: "2".into(),
                },
            ],
        };
        self.edit_playlists(|b| b.set_rules(id, Some(rules.clone())));
        self.playlist_ui.page = Page::Playlists;
        if edit {
            self.playlist_ui.draft = Some(RuleDraft {
                id: Some(id),
                name: "Preferiti da riscoprire".into(),
                rules,
            });
        }
    }
    fn playlist_rule_editor(&mut self, ui: &mut egui::Ui) {
        let Some(mut draft) = self.playlist_ui.draft.take() else {
            return;
        };
        ui.label(RichText::new(tr!("Dynamic playlist")).strong());
        ui.horizontal_wrapped(|ui| {
            let r = ui.add(
                egui::TextEdit::singleline(&mut draft.name)
                    .char_limit(120)
                    .desired_width(200.0)
                    .hint_text(tr!("Playlist name")),
            );
            r.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, tr!("Playlist name"))
            });
            egui::ComboBox::from_id_salt("rules-mode")
                .selected_text(if draft.rules.all {
                    tr!("All rules")
                } else {
                    tr!("Any rule")
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut draft.rules.all, true, tr!("All rules"));
                    ui.selectable_value(&mut draft.rules.all, false, tr!("Any rule"));
                });
        });
        let mut save = false;
        let mut cancel = false;
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    draft.rules.conditions.len() < 24,
                    egui::Button::new(tr!("Add rule")),
                )
                .clicked()
            {
                draft.rules.conditions.push(Rule::default());
            }
            save = ui
                .add_enabled(
                    draft.rules.valid() && !draft.name.trim().is_empty(),
                    egui::Button::new(tr!("Save rules")),
                )
                .clicked();
            cancel = ui.button(tr!("Cancel")).clicked();
        });
        caption(ui, tr!("Updates automatically with the library."));
        let mut remove = None;
        egui::ScrollArea::vertical()
            .id_salt("dynamic-rules-editor")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (index, rule) in draft.rules.conditions.iter_mut().enumerate() {
                    ui.push_id(index, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            let old = rule.tag;
                            egui::ComboBox::from_id_salt("rule-tag")
                                .width(100.0)
                                .selected_text(tag_label(rule.tag))
                                .show_ui(ui, |ui| {
                                    for tag in [
                                        Tag::Genre,
                                        Tag::Artist,
                                        Tag::Album,
                                        Tag::Title,
                                        Tag::Year,
                                        Tag::Rating,
                                        Tag::PlayCount,
                                    ] {
                                        ui.selectable_value(&mut rule.tag, tag, tag_label(tag));
                                    }
                                });
                            if old != rule.tag {
                                rule.comparison =
                                    if matches!(rule.tag, Tag::Year | Tag::Rating | Tag::PlayCount)
                                    {
                                        Comparison::Equals
                                    } else {
                                        Comparison::Contains
                                    };
                            }
                            let operators: &[Comparison] =
                                if matches!(rule.tag, Tag::Year | Tag::Rating | Tag::PlayCount) {
                                    &[Comparison::Equals, Comparison::AtLeast, Comparison::AtMost]
                                } else {
                                    &[
                                        Comparison::Contains,
                                        Comparison::Equals,
                                        Comparison::NotContains,
                                        Comparison::NotEquals,
                                    ]
                                };
                            egui::ComboBox::from_id_salt("rule-comparison")
                                .width(110.0)
                                .selected_text(comparison_label(rule.comparison))
                                .show_ui(ui, |ui| {
                                    for &comparison in operators {
                                        ui.selectable_value(
                                            &mut rule.comparison,
                                            comparison,
                                            comparison_label(comparison),
                                        );
                                    }
                                });
                            let r = ui.add(
                                egui::TextEdit::singleline(&mut rule.value)
                                    .char_limit(256)
                                    .desired_width(140.0)
                                    .hint_text(match rule.tag {
                                        Tag::Year => tr!("Year"),
                                        Tag::Rating => tr!("Rating (0–5)"),
                                        Tag::PlayCount => tr!("Play count"),
                                        _ => tr!("Tag value"),
                                    }),
                            );
                            r.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::TextEdit,
                                    true,
                                    tr!("Tag value"),
                                )
                            });
                            if self
                                .icons
                                .button(ui, "x", tr!("Remove rule"), false)
                                .clicked()
                            {
                                remove = Some(index);
                            }
                        });
                    });
                    ui.add_space(5.0);
                }
            });
        if let Some(index) = remove {
            draft.rules.conditions.remove(index);
        }
        if cancel {
            return;
        }
        if save {
            let mut id = draft.id;
            if self.edit_playlists(|book| {
                let target = match id {
                    Some(id) => {
                        if !book.rename(id, &draft.name) {
                            return Err(String::new());
                        }
                        id
                    }
                    None => book.create(&draft.name, vec![])?,
                };
                book.set_rules(target, Some(draft.rules.clone()))?;
                id = Some(target);
                Ok(())
            }) {
                self.select_playlist(id.unwrap());
                return;
            }
        }
        self.playlist_ui.draft = Some(draft);
    }
    fn playlist_picker(&mut self, ui: &mut egui::Ui, id: u64) {
        ui.label(tr!("Choose tracks from your library."));
        ui.add(
            egui::TextEdit::singleline(&mut self.playlist_ui.filter)
                .char_limit(256)
                .hint_text(tr!("Search title, artist, album or folder…"))
                .desired_width(ui.available_width()),
        );
        let query = self.playlist_ui.filter.to_lowercase();
        let key = (
            self.library_revision,
            self.saved.tracks.len(),
            query.clone(),
        );
        if self.playlist_ui.picker_key.as_ref() != Some(&key) {
            self.playlist_ui.picker_indices = Arc::new(
                self.saved
                    .tracks
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| {
                        query.is_empty()
                            || format!("{} {} {} {}", t.title, t.artist, t.album, t.source)
                                .to_lowercase()
                                .contains(&query)
                    })
                    .map(|(i, _)| i)
                    .collect(),
            );
            self.playlist_ui.picker_key = Some(key);
        }
        let indices = self.playlist_ui.picker_indices.clone();
        ui.horizontal_wrapped(|ui| {
            if ui.button(tr!("All")).clicked() {
                self.playlist_ui
                    .picked
                    .extend(indices.iter().map(|&i| self.saved.tracks[i].source.clone()));
            }
            if ui
                .add_enabled(
                    !self.playlist_ui.picked.is_empty(),
                    egui::Button::new(tr!("Add")),
                )
                .clicked()
            {
                let tracks = self
                    .saved
                    .tracks
                    .iter()
                    .filter(|t| self.playlist_ui.picked.contains(&t.source))
                    .map(|t| t.source.clone())
                    .collect::<Vec<_>>();
                if self.edit_playlists(|b| b.append(id, tracks)) {
                    self.playlist_ui.picker = false;
                }
            }
            if ui.button(tr!("Cancel")).clicked() {
                self.playlist_ui.picker = false;
            }
            caption(ui, &crate::i18n::tracks(self.playlist_ui.picked.len()));
        });
        egui::ScrollArea::vertical()
            .id_salt("playlist-pick-tracks")
            .auto_shrink([false, false])
            .show_rows(ui, 30.0, indices.len(), |ui, range| {
                for i in range {
                    let t = &self.saved.tracks[indices[i]];
                    let mut checked = self.playlist_ui.picked.contains(&t.source);
                    ui.push_id(&t.source, |ui| {
                        if ui
                            .checkbox(&mut checked, format!("{} — {}", t.title, t.artist))
                            .changed()
                        {
                            if checked {
                                self.playlist_ui.picked.insert(t.source.clone());
                            } else {
                                self.playlist_ui.picked.remove(&t.source);
                            }
                        }
                    });
                }
            });
    }
    /// Returns only an explicit double-click. Selecting a row never starts audio.
    fn playlist_rows(&mut self, ui: &mut egui::Ui, ids: &[String], queue: bool) -> Option<usize> {
        self.refresh_playlist_index();
        let tracks = &self.saved.tracks;
        let index = &self.playlist_ui.track_index;
        let selected = if queue {
            &mut self.playlist_ui.queue_row
        } else {
            &mut self.playlist_ui.row
        };
        let mut play = None;
        let mut rating_change = None;
        let stats = &self.track_stats;
        let stats_ready = self.stats_error.is_none();
        let follow = if queue {
            &mut self.playlist_ui.queue_follow
        } else {
            &mut self.playlist_ui.list_follow
        };
        let reveal = follow.row(
            self.current.as_deref(),
            self.library_revision.wrapping_add(self.stats_revision),
            ids.len(),
            ui.ctx().cumulative_frame_nr(),
            |source| ids.iter().position(|id| id == source),
        );
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt(if queue {
                "playback-queue-rows"
            } else {
                "named-playlist-rows"
            })
            .auto_shrink([false, false]);
        if let Some(row) = reveal {
            scroll = scroll
                .vertical_scroll_offset(super::follow::offset(
                    row,
                    34.0 + ui.spacing().item_spacing.y,
                    ui.available_height(),
                ))
                .animated(false);
        }
        scroll.show_rows(ui, 34.0, ids.len(), |ui, range| {
            ui.style_mut().interaction.selectable_labels = false;
            for i in range {
                let t = index.get(ids[i].as_str()).map(|&n| &tracks[n]);
                let title = t
                    .map(|t| format!("{} — {}", t.title, t.artist))
                    .unwrap_or_else(|| {
                        format!(
                            "{} · {}",
                            tr!("Unavailable"),
                            Track::placeholder(ids[i].clone()).title
                        )
                    });
                let current = self.current.as_ref() == Some(&ids[i]);
                let label = RichText::new(format!("{:02}  {title}", i + 1)).color(if current {
                    palette::accent()
                } else {
                    palette::text()
                });
                let r = ui
                    .push_id((queue, i), |ui| {
                        ui.horizontal(|ui| {
                            let title_width = (ui.available_width()
                                - super::track_stats::STAR_WIDTH
                                - ui.spacing().item_spacing.x)
                                .max(40.0);
                            let response = ui.add_sized(
                                [title_width, 32.0],
                                egui::Button::selectable(*selected == Some(i), label)
                                    .truncate()
                                    .right_text(()),
                            );
                            let (rect, _) = ui.allocate_exact_size(
                                vec2(super::track_stats::STAR_WIDTH, 32.0),
                                egui::Sense::hover(),
                            );
                            if let Some(rating) = super::track_stats::rating_stars(
                                ui,
                                rect,
                                (queue, &ids[i]),
                                t.map(|t| stats.effective(t, self.saved.rating_tag_scale))
                                    .unwrap_or_else(|| stats.get(&ids[i]))
                                    .rating,
                                stats_ready,
                            ) {
                                rating_change = Some((ids[i].clone(), rating));
                            }
                            response
                        })
                        .inner
                    })
                    .inner;
                r.context_menu(|ui| {
                    if let Some(t) = t {
                        super::track_stats::tag_rating_info(ui, t, self.saved.rating_tag_scale);
                    }
                    ui.add_enabled_ui(stats_ready, |ui| {
                        if let Some(rating) = super::track_stats::rating_menu(
                            ui,
                            t.map(|t| stats.effective(t, self.saved.rating_tag_scale))
                                .unwrap_or_else(|| stats.get(&ids[i])),
                        ) {
                            rating_change = Some((ids[i].clone(), rating));
                        }
                    });
                });
                if r.clicked() {
                    *selected = Some(i);
                }
                if t.is_some()
                    && (r.double_clicked()
                        || r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                {
                    play = Some(i);
                }
            }
        });
        if let Some((source, rating)) = rating_change {
            self.rate_track(&source, rating);
        }
        play
    }
    pub(super) fn queue_page(&mut self, ui: &mut egui::Ui) {
        ui.heading(tr!("Playback queue"));
        if let Some(name) = &self.saved.queue_label {
            ui.label(name);
        }
        let selection = self.playback_selection.take();
        let had_selection = selection.is_some();
        let mut ids = selection.unwrap_or_default();
        let mut changed = false;
        let row = self.playlist_ui.queue_row;
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    !ids.is_empty(),
                    egui::Button::new(tr!("Save queue as playlist")),
                )
                .clicked()
            {
                self.create_playlist(tr!("New playlist"), ids.clone());
            }
            if ui
                .add_enabled(!ids.is_empty(), egui::Button::new(tr!("Clear queue")))
                .clicked()
            {
                ids.clear();
                changed = true;
                self.playlist_ui.queue_row = None;
            }
            if ui
                .add_enabled(
                    row.is_some_and(|r| r < ids.len()),
                    egui::Button::new(tr!("Remove from queue")),
                )
                .clicked()
            {
                ids.remove(row.unwrap());
                changed = true;
                self.playlist_ui.queue_row = None;
            }
            caption(ui, &crate::i18n::tracks(ids.len()));
        });
        let play = self
            .playlist_rows(ui, &ids, true)
            .map(|row| ids[row].clone());
        // Move the snapshot back instead of copying all paths on every animation frame.
        if changed {
            self.queue_changed(ids);
        } else {
            self.playback_selection = had_selection.then_some(ids);
        }
        if let Some(id) = play {
            self.history.discard_forward();
            self.play(id);
        }
    }
}

fn tag_label(tag: Tag) -> &'static str {
    match tag {
        Tag::Genre => tr!("Genre"),
        Tag::Artist => tr!("Artist"),
        Tag::Album => tr!("Album"),
        Tag::Title => tr!("Title"),
        Tag::Year => tr!("Year"),
        Tag::Rating => tr!("Rating"),
        Tag::PlayCount => tr!("Play count"),
    }
}
fn comparison_label(comparison: Comparison) -> &'static str {
    match comparison {
        Comparison::Contains => tr!("Contains"),
        Comparison::Equals => tr!("Equals"),
        Comparison::NotContains => tr!("Does not contain"),
        Comparison::NotEquals => tr!("Does not equal"),
        Comparison::AtLeast => tr!("At least"),
        Comparison::AtMost => tr!("At most"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn app(ctx: &egui::Context, dir: &std::path::Path) -> Aurea {
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut a = Aurea::new(&cc, dir.into(), vec![], None, None);
        a.saved.auto_sync = false;
        a.saved.mini = false;
        a.saved.folders = vec!["/first".into(), "/second".into()];
        a.saved.active_source = Some("folder:/first".into());
        a.saved.tracks = ["/first/one.wav", "/second/two.wav", "/first/three.wav"]
            .into_iter()
            .map(|s| {
                let mut t = Track::placeholder(s.into());
                t.artist = "Fixture".into();
                t
            })
            .collect();
        a
    }
    #[test]
    fn playlist_edits_source_browsing_and_queue_are_independent() {
        let d = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut a = app(&ctx, d.path());
        let (tx, rx) = mpsc::channel();
        a.audio.tx = tx;
        let ids: Vec<_> = a.saved.tracks.iter().map(|t| t.source.clone()).collect();
        a.create_playlist("First", ids.clone());
        let id = a.playlist_ui.selected.unwrap();
        a.start_playlist(id, 0);
        assert!(matches!(rx.try_recv(),Ok(Command::Load{track,..}) if track.source==ids[0]));
        a.loading = false;
        a.current = Some(ids[0].clone());
        assert_eq!(
            a.choose_next(),
            Some(ids[1].clone()),
            "queue can cross library source boundaries"
        );
        a.select_source(Some("folder:/second".into()));
        assert_eq!(a.current.as_ref(), Some(&ids[0]));
        assert!(rx.try_recv().is_err(), "browsing must not stop playback");
        a.edit_playlists(|b| {
            b.rename(id, "Renamed");
            b.move_track(id, 0, 2);
            b.remove_track(id, 1);
            Ok(())
        });
        assert_eq!(a.playback_selection.as_ref(), Some(&ids));
        assert_eq!(
            a.saved.queue_label.as_deref(),
            Some("First"),
            "queue keeps its origin snapshot"
        );
        a.edit_playlists(|b| {
            b.remove(id);
            Ok(())
        });
        assert_eq!(a.playback_selection.as_ref(), Some(&ids));
        a.queue_changed(vec![]);
        assert!(a.choose_next().is_none());
        assert_eq!(a.saved.tracks.len(), 3);
        assert!(a.playlist_book.lists.is_empty());
    }
    struct Memory(HashMap<String, String>);
    impl eframe::Storage for Memory {
        fn get_string(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
        fn set_string(&mut self, key: &str, value: String) {
            self.0.insert(key.into(), value);
        }
        fn remove_string(&mut self, key: &str) {
            self.0.remove(key);
        }
        fn flush(&mut self) {}
    }
    #[test]
    fn restart_restores_named_playlists_and_separate_queue_without_autoplay() {
        let d = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut a = app(&ctx, d.path());
        let ids: Vec<_> = a.saved.tracks.iter().map(|t| t.source.clone()).collect();
        a.create_playlist("Original", ids.clone());
        let id = a.playlist_ui.selected.unwrap();
        a.saved.queue_label = Some("Original".into());
        a.playback_selection = Some(ids.clone());
        a.edit_playlists(|b| {
            b.remove_track(id, 1);
            Ok(())
        });
        let mut memory = Memory(HashMap::new());
        eframe::App::save(&mut a, &mut memory);
        let mut cc = eframe::CreationContext::_new_kittest(ctx);
        cc.storage = Some(&memory);
        let b = Aurea::new(&cc, d.path().into(), vec![], None, None);
        assert_eq!(b.playlist_book.get(id).unwrap().tracks.len(), 2);
        assert_eq!(b.playback_selection, Some(ids));
        assert_eq!(b.saved.queue_label.as_deref(), Some("Original"));
        assert!(b.current.is_none() && b.requested.is_none() && b.paused);
    }
    #[test]
    fn damaged_file_stays_read_only_and_legacy_profile_has_no_named_queue() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("playlists.json");
        std::fs::write(&path, b"{broken").unwrap();
        let ctx = egui::Context::default();
        let mut a = app(&ctx, d.path());
        a.create_playlist("Must not overwrite", vec![]);
        a.persist_playlists();
        assert_eq!(std::fs::read(&path).unwrap(), b"{broken");
        assert!(a.saved.queue_label.is_none());
        let legacy: Saved = serde_json::from_str("{}").unwrap();
        assert!(legacy.queue_label.is_none());
    }
    #[test]
    fn import_export_and_unavailable_tracks_keep_the_named_order() {
        let d = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut a = app(&ctx, d.path());
        let p = d.path().join("Road.m3u8");
        std::fs::write(
            &p,
            "#EXTM3U\n/second/two.wav\n/first/one.wav\n/second/two.wav\n",
        )
        .unwrap();
        a.import_named_playlist(&p);
        let id = a.playlist_ui.selected.unwrap();
        let expected = vec!["/second/two.wav".to_string(), "/first/one.wav".to_string()];
        assert_eq!(*a.playlist_book.get(id).unwrap().tracks, expected);
        a.saved.tracks.retain(|t| t.source != expected[0]);
        assert_eq!(
            a.playlist_book.get(id).unwrap().tracks.len(),
            2,
            "sync never silently deletes playlist references"
        );
        let p = d.path().join("export.m3u8");
        crate::playlists::atomic_write(
            &p,
            a.playlist_book.m3u(id, &a.saved.tracks).unwrap().as_bytes(),
        )
        .unwrap();
        assert_eq!(library::playlist(&p).unwrap(), expected);
    }
    #[test]
    fn playlist_tabs_and_create_action_fit_small_and_large_windows() {
        for size in [vec2(640.0, 440.0), vec2(1120.0, 760.0)] {
            let d = tempfile::tempdir().unwrap();
            let ctx = egui::Context::default();
            let mut a = app(&ctx, d.path());
            a.playlist_ui.page = Page::Playlists;
            let mut tick = 0;
            let mut frame = |a: &mut Aurea, events| {
                tick += 1;
                let mut o = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(pos2(0.0, 0.0), size)),
                        time: Some(tick as f64 * 0.04),
                        events,
                        ..Default::default()
                    },
                    |ui| a.desktop_ui(ui),
                );
                o.textures_delta.clear();
                o.platform_output.accesskit_update.unwrap().nodes
            };
            frame(&mut a, vec![]);
            let nodes = frame(&mut a, vec![]);
            let mut at = None;
            for label in [
                tr!("Library"),
                tr!("Playlists"),
                tr!("Playback queue"),
                tr!("New playlist"),
            ] {
                let b = nodes
                    .iter()
                    .find(|(_, n)| n.label() == Some(label))
                    .unwrap_or_else(|| panic!("missing {label}"))
                    .1
                    .bounds()
                    .unwrap();
                assert!(
                    b.x0 >= 0.0 && b.y0 >= 0.0 && b.x1 <= size.x as f64 && b.y1 <= size.y as f64,
                    "{label}: {b:?}"
                );
                if label == tr!("New playlist") {
                    at = Some(pos2(
                        ((b.x0 + b.x1) / 2.0) as f32,
                        ((b.y0 + b.y1) / 2.0) as f32,
                    ));
                }
            }
            let p = at.unwrap();
            frame(&mut a, vec![egui::Event::PointerMoved(p)]);
            for pressed in [true, false] {
                frame(
                    &mut a,
                    vec![egui::Event::PointerButton {
                        pos: p,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    }],
                );
            }
            assert_eq!(a.playlist_book.lists.len(), 1);
            assert!(a.playback_selection.is_none());
            assert_eq!(Book::load(&a.playlist_path).unwrap(), a.playlist_book);
        }
    }
}

#[cfg(test)]
mod transition_tests {
    use super::*;
    #[test]
    fn natural_transition_uses_the_queue_snapshot_after_original_playlist_is_reordered() {
        let d = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut a = Aurea::new(&cc, d.path().into(), vec![], None, None);
        a.saved.auto_sync = false;
        a.saved.tracks = (1..=3)
            .map(|n| Track::placeholder(format!("/synthetic/track-{n}.wav")))
            .collect();
        let ids: Vec<_> = a.saved.tracks.iter().map(|t| t.source.clone()).collect();
        let (tx, commands) = mpsc::channel();
        a.audio.tx = tx;
        let (events, rx) = mpsc::channel();
        a.audio.rx = rx;
        a.create_playlist("Three tracks", ids.clone());
        let id = a.playlist_ui.selected.unwrap();
        a.start_playlist(id, 0);
        for (index, expected) in ids.iter().enumerate() {
            let track = match commands.recv_timeout(Duration::from_secs(2)).unwrap() {
                Command::Load { track, .. } => track,
                _ => panic!("expected load"),
            };
            assert_eq!(&track.source, expected);
            events
                .send(Event::Started {
                    ticket: a.ticket,
                    track,
                    output: "fixture".into(),
                    cover: None,
                    exclusive: false,
                })
                .unwrap();
            a.poll(&ctx);
            if index == 0 {
                a.edit_playlists(|b| {
                    b.move_track(id, 1, 2);
                    Ok(())
                });
            }
            events.send(Event::Ended).unwrap();
            a.poll(&ctx);
        }
        assert!(a.paused);
        assert!(a.requested.is_none());
        assert_eq!(
            *a.playlist_book.get(id).unwrap().tracks,
            vec![ids[0].clone(), ids[2].clone(), ids[1].clone()]
        );
    }
}

#[cfg(test)]
mod gesture_tests {
    use super::*;
    pub(super) fn frame(
        a: &mut Aurea,
        ctx: &egui::Context,
        tick: &mut u32,
        events: Vec<egui::Event>,
    ) -> egui::PlatformOutput {
        *tick += 1;
        let mut o = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    pos2(0.0, 0.0),
                    vec2(1120.0, 760.0),
                )),
                time: Some(*tick as f64 * 0.04),
                events,
                ..Default::default()
            },
            |ui| a.desktop_ui(ui),
        );
        o.textures_delta.clear();
        o.platform_output
    }
    pub(super) fn click(a: &mut Aurea, ctx: &egui::Context, tick: &mut u32, label: &str) {
        frame(a, ctx, tick, vec![]);
        let o = frame(a, ctx, tick, vec![]);
        let nodes = o.accesskit_update.unwrap().nodes;
        // The confirmation's Delete comes after the toolbar's Delete.
        let matching = nodes
            .iter()
            .filter(|(_, n)| n.label() == Some(label))
            .filter_map(|(_, n)| n.bounds());
        let b = if label == tr!("Delete") && a.playlist_ui.deleting {
            matching.max_by(|a, b| a.y0.total_cmp(&b.y0))
        } else {
            matching.min_by(|a, b| a.y0.total_cmp(&b.y0))
        }
        .unwrap_or_else(|| panic!("missing {label}"));
        let p = pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32);
        frame(a, ctx, tick, vec![egui::Event::PointerMoved(p)]);
        for pressed in [true, false] {
            frame(
                a,
                ctx,
                tick,
                vec![egui::Event::PointerButton {
                    pos: p,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
        }
    }
    #[test]
    fn pointer_actions_add_reorder_remove_rename_duplicate_delete_and_play() {
        let d = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut a = Aurea::new(&cc, d.path().into(), vec![], None, None);
        a.saved.mini = false;
        a.saved.auto_sync = false;
        a.saved.folders = vec!["/first".into()];
        a.saved.active_source = Some("folder:/first".into());
        a.saved.tracks = ["/first/one.wav", "/second/two.wav", "/first/three.wav"]
            .into_iter()
            .map(|s| {
                let mut t = Track::placeholder(s.into());
                t.artist = "Fixture".into();
                t
            })
            .collect();
        let (tx, commands) = mpsc::channel();
        a.audio.tx = tx;
        a.create_playlist("Test", vec![]);
        let id = a.playlist_ui.selected.unwrap();
        a.playlist_ui.page = Page::Playlists;
        let mut tick = 0;
        click(&mut a, &ctx, &mut tick, tr!("Add tracks…"));
        click(&mut a, &ctx, &mut tick, tr!("All"));
        click(&mut a, &ctx, &mut tick, tr!("Add"));
        assert_eq!(a.playlist_book.get(id).unwrap().tracks.len(), 3);
        click(&mut a, &ctx, &mut tick, "01  one — Fixture");
        click(&mut a, &ctx, &mut tick, tr!("Move down"));
        assert_eq!(a.playlist_book.get(id).unwrap().tracks[1], "/first/one.wav");
        click(&mut a, &ctx, &mut tick, tr!("Remove from playlist"));
        assert_eq!(a.playlist_book.get(id).unwrap().tracks.len(), 2);
        a.playlist_ui.name = "Changed".into();
        click(&mut a, &ctx, &mut tick, tr!("Rename"));
        assert_eq!(a.playlist_book.get(id).unwrap().name, "Changed");
        click(&mut a, &ctx, &mut tick, tr!("Duplicate"));
        assert_eq!(a.playlist_book.lists.len(), 2);
        click(&mut a, &ctx, &mut tick, tr!("Delete"));
        assert!(a.playlist_ui.deleting);
        click(&mut a, &ctx, &mut tick, tr!("Delete"));
        assert_eq!(
            a.playlist_book.lists.len(),
            1,
            "deleting={}, selected={:?}, message={}",
            a.playlist_ui.deleting,
            a.playlist_ui.selected,
            a.message
        );
        assert!(a.requested.is_none());
        assert!(commands.try_recv().is_err());
        assert_eq!(a.saved.tracks.len(), 3);
        assert_eq!(Book::load(&a.playlist_path).unwrap(), a.playlist_book);
        click(&mut a, &ctx, &mut tick, tr!("Play"));
        assert!(
            matches!(commands.try_recv(),Ok(Command::Load{track,..}) if track.source=="/second/two.wav")
        );
        assert_eq!(a.playback_selection.as_ref().unwrap().len(), 2);
    }
}

#[cfg(test)]
mod dynamic_tests {
    use super::*;
    #[test]
    fn library_additions_removals_and_tag_edits_refresh_results_without_mutating_queue() {
        let d = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx);
        let mut a = Aurea::new(&cc, d.path().into(), vec![], None, None);
        a.saved.auto_sync = false;
        let mut track = Track::placeholder("/a.wav".into());
        track.genre = "Rock".into();
        a.saved.tracks = vec![track.clone()];
        a.create_playlist("Dynamic", vec![]);
        let id = a.playlist_ui.selected.unwrap();
        a.edit_playlists(|b| {
            b.set_rules(
                id,
                Some(Rules {
                    all: true,
                    conditions: vec![Rule {
                        tag: Tag::Genre,
                        comparison: Comparison::Equals,
                        value: "rock".into(),
                    }],
                }),
            )
        });
        let initial = a.playlist_tracks(id);
        assert_eq!(initial.as_slice(), ["/a.wav"]);
        assert!(
            Arc::ptr_eq(&initial, &a.playlist_tracks(id)),
            "unchanged frames reuse the result"
        );
        let (tx, _rx) = mpsc::channel();
        a.audio.tx = tx;
        a.start_playlist(id, 0);
        let queue = a.playback_selection.clone();
        track.source = "/b.wav".into();
        a.saved.tracks.push(track);
        a.library_revision += 1;
        assert_eq!(a.playlist_tracks(id).len(), 2);
        assert_eq!(a.playback_selection, queue);
        a.saved.tracks[0].genre = "Jazz".into();
        a.library_revision += 1;
        assert_eq!(a.playlist_tracks(id).as_slice(), ["/b.wav"]);
        a.saved.tracks.clear();
        a.library_revision += 1;
        assert!(a.playlist_tracks(id).is_empty());
        assert_eq!(a.playback_selection, queue);
        let reopened = Book::load(&a.playlist_path).unwrap();
        assert!(reopened.get(id).unwrap().rules.is_some());
        assert!(reopened.get(id).unwrap().tracks.is_empty());
    }
}

#[cfg(test)]
mod dynamic_gesture_tests {
    use super::*;
    #[test]
    fn create_edit_cancel_duplicate_and_export_dynamic_rules_keep_manual_lists() {
        use super::gesture_tests::{click, frame};
        let d = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut a = Aurea::new(&cc, d.path().into(), vec![], None, None);
        a.saved.mini = false;
        a.saved.auto_sync = false;
        let mut track = Track::placeholder("/a.wav".into());
        track.artist = "Queen".into();
        a.saved.tracks = vec![track];
        a.create_playlist("Manual", vec!["/a.wav".into()]);
        let manual = a.playlist_ui.selected.unwrap();
        a.playlist_ui.page = Page::Playlists;
        let mut tick = 0;
        click(&mut a, &ctx, &mut tick, tr!("New dynamic playlist"));
        assert!(a.playlist_ui.draft.is_some());
        assert_eq!(
            a.playlist_book.lists.len(),
            1,
            "blank rules do not create a saved list"
        );
        let draft = a.playlist_ui.draft.as_mut().unwrap();
        draft.name = "Queen mix".into();
        draft.rules.conditions = vec![Rule {
            tag: Tag::Artist,
            comparison: Comparison::Equals,
            value: "queen".into(),
        }];
        click(&mut a, &ctx, &mut tick, tr!("Save rules"));
        let id = a.playlist_ui.selected.unwrap();
        assert_ne!(manual, id);
        assert!(a.playlist_ui.draft.is_none());
        assert_eq!(a.playlist_tracks(id).len(), 1);
        click(&mut a, &ctx, &mut tick, tr!("Edit rules…"));
        a.playlist_ui.draft.as_mut().unwrap().rules.conditions[0].value = "Other".into();
        click(&mut a, &ctx, &mut tick, tr!("Cancel"));
        assert_eq!(a.playlist_tracks(id).len(), 1);
        click(&mut a, &ctx, &mut tick, tr!("Duplicate"));
        let duplicate = a.playlist_ui.selected.unwrap();
        assert_eq!(
            a.playlist_book.get(id).unwrap().rules,
            a.playlist_book.get(duplicate).unwrap().rules
        );
        assert_eq!(a.playlist_tracks(duplicate).len(), 1);
        let path = d.path().join("mix.m3u8");
        crate::playlists::atomic_write(
            &path,
            a.playlist_book.m3u(id, &a.saved.tracks).unwrap().as_bytes(),
        )
        .unwrap();
        assert_eq!(library::playlist(&path).unwrap(), vec!["/a.wav"]);
        assert_eq!(
            a.playlist_book.get(manual).unwrap().tracks.as_slice(),
            ["/a.wav"]
        );
        assert!(a.playback_selection.is_none());
        frame(&mut a, &ctx, &mut tick, vec![]);
        assert_eq!(Book::load(&a.playlist_path).unwrap(), a.playlist_book);
    }
}
