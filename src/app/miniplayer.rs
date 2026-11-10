use super::*;

/// Indices only: metadata and URLs remain owned by the library.
#[derive(Default)]
pub(super) struct MiniIndex {
    key: Option<(u64, usize, library::Scope)>,
    query: String,
    scoped: Vec<usize>,
    pub(super) rows: Vec<usize>,
    #[cfg(test)]
    rebuilds: usize,
}
impl MiniIndex {
    fn refresh(&mut self, tracks: &[Track], scope: library::Scope, revision: u64, query: &str) {
        let changed = self
            .key
            .as_ref()
            .is_none_or(|(r, n, s)| *r != revision || *n != tracks.len() || s != &scope);
        if !changed && self.query == query {
            return;
        }
        if changed {
            self.scoped.clear();
            self.scoped.extend(
                tracks
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| scope.contains(&t.source))
                    .map(|(i, _)| i),
            );
            self.key = Some((revision, tracks.len(), scope));
        }
        self.query.clear();
        self.query.push_str(query);
        let query = query.to_lowercase();
        self.rows.clear();
        self.rows.extend(self.scoped.iter().copied().filter(|&i| {
            let t = &tracks[i];
            query.is_empty()
                || format!("{} {} {}", t.title, t.artist, t.album)
                    .to_lowercase()
                    .contains(&query)
        }));
        #[cfg(test)]
        {
            self.rebuilds += 1;
        }
    }
}
#[derive(Default)]
pub(super) struct VinylIndex {
    key: Option<(u64, usize, VinylAlbum)>,
    rows: Vec<usize>,
}

/// A server URL directory is not necessarily an album directory. In particular,
/// Android exposes the entire MediaStore catalog under one /v1/audio/ endpoint.
/// Keep tags within their physical folder/remote endpoint so editions and copies
/// from other sources do not get combined.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct VinylAlbum {
    folder: String,
    album: String,
    artist: String,
    untagged_remote: Option<String>,
}
impl VinylAlbum {
    fn of(track: &Track) -> Self {
        let album = track.album.trim().to_lowercase();
        let artist = if album.is_empty() {
            String::new()
        } else if track.album_artist.trim().is_empty() {
            track.artist.trim().to_lowercase()
        } else {
            track.album_artist.trim().to_lowercase()
        };
        Self {
            folder: track
                .source
                .rsplit_once('/')
                .map_or("", |(parent, _)| parent)
                .into(),
            // Without album tags an HTTP endpoint says nothing about membership.
            untagged_remote: (album.is_empty()
                && (track.source.starts_with("https://") || track.source.starts_with("http://")))
            .then(|| track.source.clone()),
            album,
            artist,
        }
    }
}
impl VinylIndex {
    fn refresh(&mut self, tracks: &[Track], revision: u64, current: &Track) {
        let album = VinylAlbum::of(current);
        if self
            .key
            .as_ref()
            .is_some_and(|(r, n, a)| *r == revision && *n == tracks.len() && a == &album)
        {
            return;
        }
        self.rows.clear();
        self.rows.extend(
            tracks
                .iter()
                .enumerate()
                .filter(|(_, t)| VinylAlbum::of(t) == album)
                .map(|(i, _)| i),
        );
        let numbered = self.rows.iter().all(|&i| tracks[i].number > 0);
        self.rows.sort_by(|&a, &b| {
            let (a, b) = (&tracks[a], &tracks[b]);
            (if numbered {
                (a.disc, a.number).cmp(&(b.disc, b.number))
            } else {
                std::cmp::Ordering::Equal
            })
            .then_with(|| a.source.cmp(&b.source))
        });
        self.key = Some((revision, tracks.len(), album));
    }
}

impl Aurea {
    pub(super) fn refresh_mini_index(&mut self) {
        let scope = self.selected_scope();
        self.mini_index.refresh(
            &self.saved.tracks,
            scope,
            self.library_revision,
            &self.query,
        );
    }
    pub(super) fn mini_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let area = ui.max_rect();
        let width = area.width();
        let origin = area.min;
        let (background, surface, text, muted, accent) = match self.saved.theme % 3 {
            1 => (
                Color32::from_rgb(19, 19, 21),
                Color32::from_rgb(36, 36, 38),
                Color32::from_rgb(245, 242, 237),
                Color32::from_rgb(163, 161, 153),
                Color32::from_rgb(255, 148, 64),
            ),
            2 => (
                Color32::from_rgb(230, 237, 242),
                Color32::from_rgb(251, 253, 255),
                Color32::from_rgb(20, 31, 41),
                Color32::from_rgb(79, 97, 110),
                Color32::from_rgb(5, 122, 158),
            ),
            _ => (
                palette::bg(),
                palette::panel(),
                palette::text(),
                palette::muted(),
                palette::accent(),
            ),
        };
        let visual = ui.visuals_mut();
        visual.override_text_color = Some(text);
        visual.widgets.inactive.bg_fill = surface;
        visual.widgets.inactive.weak_bg_fill = surface;
        visual.widgets.inactive.bg_stroke = egui::Stroke::NONE;
        visual.widgets.inactive.fg_stroke.color = text;
        visual.widgets.noninteractive.fg_stroke.color = muted;
        visual.widgets.hovered.bg_fill = surface;
        visual.widgets.active.bg_fill = surface;
        visual.selection.bg_fill = accent.gamma_multiply(0.16);
        visual.selection.stroke.color = text;
        visual.text_edit_bg_color = Some(surface);
        visual.extreme_bg_color = background;
        ui.spacing_mut().scroll.foreground_color = true;
        ui.painter().rect_filled(area, 14.0, background);
        ui.painter().rect_stroke(
            area.shrink(0.5),
            14.0,
            egui::Stroke::new(1.0, surface),
            egui::StrokeKind::Inside,
        );
        let rect = |x, y, w, h| egui::Rect::from_min_size(origin + vec2(x, y), vec2(w, h));
        drag_titlebar(ui, rect(12.0, 4.0, width - 164.0, 32.0));
        ui.painter().circle_filled(
            origin + vec2(20.0, 22.0),
            3.0,
            if self.paused { muted } else { accent },
        );
        clipped(
            ui,
            rect(31.0, 15.0, 126.0, 16.0),
            "M U S I C C R A F T",
            10.0,
            muted,
        );
        if self.current.is_some() {
            clipped(
                ui,
                rect(165.0, 16.0, (width - 315.0).max(40.0), 14.0),
                if self.exclusive_active() {
                    tr!("EXCLUSIVE")
                } else {
                    tr!("SHARED")
                },
                8.5,
                muted,
            );
            ui.interact(
                rect(162.0, 10.0, (width - 312.0).max(40.0), 24.0),
                egui::Id::new("mini-output"),
                egui::Sense::hover(),
            )
            .on_hover_text(crate::i18n::signal_message(&self.output));
        }
        if button(
            ui,
            &self.icons,
            rect(width - 142.0, 8.0, 30.0, 28.0),
            tr!("Open full library"),
            false,
            surface,
            muted,
            "arrow-up-right",
        ) {
            self.saved.mini = false;
        }
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(rect(width - 108.0, 8.0, 30.0, 28.0)),
            |ui| {
                let response = ui.add(
                    egui::Button::image(self.icons.image("settings", 16.0, muted))
                        .frame(false)
                        .min_size(vec2(30.0, 28.0)),
                );
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        true,
                        tr!("Mini player options"),
                    )
                });
                egui::Popup::menu(&response).show(|ui| {
                    ui.set_min_width(210.0);
                    if ui.button(tr!("All settings…")).clicked() {
                        self.open_settings(0);
                        ui.close();
                    }
                    ui.checkbox(
                        &mut self.saved.appearance_by_year,
                        tr!("Appearance by year"),
                    );
                    if ui.button(tr!("Server and multiroom…")).clicked() {
                        self.open_settings(3);
                        ui.close();
                    }
                    if ui.button(tr!("User guide")).clicked() {
                        self.help_dialog = true;
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(tr!("Connect smartphone / FTP…")).clicked() {
                        ui.close();
                        self.open_network();
                    }
                    if ui.button(tr!("Sources and synchronization…")).clicked() {
                        ui.close();
                        self.saved.mini = false;
                        self.sources_dialog = true;
                    }
                    if ui.button(tr!("Audio output…")).clicked() {
                        ui.close();
                        self.saved.mini = false;
                        self.audio_dialog = true;
                    }
                    ui.separator();
                    self.online_cover_control(ui);
                    self.cover_save_control(ui);
                    ui.separator();
                    for (theme, name) in [(0, tr!("Gold")), (1, tr!("Graphite")), (2, tr!("Light"))]
                    {
                        if ui
                            .selectable_value(&mut self.saved.theme, theme, name)
                            .clicked()
                        {
                            ui.close();
                        }
                    }
                });
            },
        );
        if button(
            ui,
            &self.icons,
            rect(width - 74.0, 8.0, 30.0, 28.0),
            tr!("Always on top"),
            self.saved.pinned,
            surface,
            if self.saved.pinned { accent } else { muted },
            "pin",
        ) {
            self.saved.pinned = !self.saved.pinned;
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(if self.saved.pinned {
                egui::WindowLevel::AlwaysOnTop
            } else {
                egui::WindowLevel::Normal
            }));
        }
        if button(
            ui,
            &self.icons,
            rect(width - 40.0, 8.0, 30.0, 28.0),
            tr!("Quit MusicCraft"),
            false,
            surface,
            muted,
            "x",
        ) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.refresh_mini_index();
        let track = self.displayed_track().cloned();
        let area = player_content_area(area);
        let width = area.width();
        let origin = area.min;
        let rect = |x, y, w, h| egui::Rect::from_min_size(origin + vec2(x, y), vec2(w, h));
        let cover = rect(20.0, 49.0, 70.0, 70.0);
        ui.painter().rect_filled(cover, 7.0, surface);
        if let Some(art) = &self.art {
            ui.put(
                cover,
                egui::Image::new(art)
                    .fit_to_exact_size(cover.size())
                    .corner_radius(7.0),
            );
        } else {
            ui.put(cover.shrink(21.0), self.icons.image("disc-3", 28.0, muted));
        }
        let cover_response = ui.interact(cover, ui.id().with("open-artwork"), egui::Sense::click());
        cover_response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tr!("Album artwork"))
        });
        if cover_viewer::requested(&cover_response) {
            self.open_cover_viewer();
        }
        cover_response.on_hover_text(tr!("Double-click to enlarge artwork"));
        let title = track
            .as_ref()
            .map_or(tr!("Library ready"), |t| t.title.as_str());
        clipped(
            ui,
            rect(106.0, 49.0, width - 126.0, 25.0),
            title,
            18.0,
            text,
        );
        let artist = track
            .as_ref()
            .map_or(tr!("Select a track and start listening"), |t| {
                if t.artist.is_empty() {
                    tr!("Artist not specified")
                } else {
                    &t.artist
                }
            });
        clipped(
            ui,
            rect(106.0, 78.0, width - 126.0, 19.0),
            artist,
            12.0,
            if track.is_some() { text } else { muted },
        );
        let album = track.as_ref().map_or_else(
            || {
                format!(
                    "{} · {}",
                    self.source_title(),
                    crate::i18n::tracks(self.mini_index.scoped.len())
                )
            },
            |t| t.album.clone(),
        );
        clipped(
            ui,
            rect(106.0, 99.0, width - 316.0, 18.0),
            &album,
            10.5,
            muted,
        );
        // Two readable peak rows, with a dedicated gap after the album caption.
        for (i, level) in self.meters.iter().enumerate() {
            ui.painter().text(
                origin + vec2(width - 194.0, 101.0 + i as f32 * 11.5),
                egui::Align2::LEFT_CENTER,
                ["L", "R"][i],
                egui::FontId::proportional(8.5),
                muted,
            );
            let fraction = ((20.0 * level.max(0.000001).log10() + 60.0) / 60.0).clamp(0.0, 1.0);
            for segment in 0..16 {
                ui.painter().rect_filled(
                    rect(
                        width - 180.0 + segment as f32 * 10.0,
                        97.5 + i as f32 * 11.5,
                        8.0,
                        7.0,
                    ),
                    1.0,
                    if segment as f32 / 16.0 < fraction {
                        accent
                    } else {
                        surface
                    },
                );
            }
        }
        let duration = track.as_ref().map_or(0.0, |t| t.duration);
        let progress = (self.position / duration.max(0.001)).clamp(0.0, 1.0) as f32;
        let seek_rect = rect(20.0, 128.0, width - 40.0, 18.0);
        let seek = ui.interact(
            seek_rect,
            egui::Id::new("mini-seek"),
            egui::Sense::click_and_drag(),
        );
        ui.painter()
            .rect_filled(rect(20.0, 135.0, width - 40.0, 3.0), 1.5, surface);
        if track.is_some() {
            ui.painter().rect_filled(
                rect(20.0, 135.0, (width - 40.0) * progress, 3.0),
                1.5,
                accent,
            );
        }
        if seek.hovered() || seek.dragged() {
            ui.painter().circle_filled(
                origin + vec2(20.0 + (width - 40.0) * progress, 136.5),
                4.0,
                accent,
            );
        }
        if (seek.clicked() || seek.dragged())
            && !self.loading
            && duration > 0.0
            && let Some(p) = seek.interact_pointer_pos()
        {
            self.position =
                f64::from(((p.x - origin.x - 20.0) / (width - 40.0)).clamp(0.0, 1.0)) * duration;
            let _ = self.audio.tx.send(Command::Seek(self.position));
        }
        let status = if self.loading {
            crate::i18n::format(
                "Loading · {size} MiB",
                &[(
                    "size",
                    format!("{:.1}", self.loaded_bytes as f64 / 1048576.0),
                )],
            )
        } else {
            time(self.position)
        };
        clipped(ui, rect(20.0, 145.0, 65.0, 14.0), &status, 9.5, muted);
        self.tag_progress_indicator(ui, rect(90.0, 145.0, width - 180.0, 20.0), true);
        ui.painter().text(
            origin + vec2(width - 20.0, 145.0),
            egui::Align2::RIGHT_TOP,
            time(duration),
            egui::FontId::monospace(9.5),
            muted,
        );
        // One transport line. Secondary controls stay neutral until enabled.
        let center = width / 2.0;
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(rect(center - 101.0, 183.0, 30.0, 30.0)),
            |ui| self.order_selector(ui, accent),
        );
        if button_if(
            ui,
            &self.icons,
            rect(center - 65.0, 183.0, 32.0, 32.0),
            tr!("Previous"),
            false,
            surface,
            text,
            "skip-back",
            self.navigation_ready(),
        ) {
            if let Some(id) = self.history.previous() {
                self.play(id);
            } else {
                let _ = self.audio.tx.send(Command::Seek(0.0));
            }
        }
        if button_if(
            ui,
            &self.icons,
            rect(center - 21.0, 178.0, 42.0, 42.0),
            if self.paused {
                tr!("Play")
            } else {
                tr!("Pause")
            },
            true,
            surface,
            accent,
            if self.paused { "play" } else { "pause" },
            self.can_toggle(),
        ) {
            self.toggle();
        }
        if button_if(
            ui,
            &self.icons,
            rect(center + 33.0, 183.0, 32.0, 32.0),
            tr!("Next"),
            false,
            surface,
            text,
            "skip-forward",
            self.navigation_ready(),
        ) {
            self.next();
        }
        if button(
            ui,
            &self.icons,
            rect(center + 71.0, 183.0, 30.0, 30.0),
            tr!("Repeat track"),
            self.saved.repeat,
            surface,
            if self.saved.repeat { accent } else { muted },
            "repeat-1",
        ) {
            self.saved.repeat = !self.saved.repeat;
            self.prepare_next();
        }
        if button(
            ui,
            &self.icons,
            rect(14.0, 185.0, 26.0, 28.0),
            tr!("Mute"),
            self.mute,
            surface,
            muted,
            if self.mute { "volume-x" } else { "volume-2" },
        ) && !self.exclusive_active()
        {
            self.mute = !self.mute;
            self.gain();
        }
        if self.exclusive_active() {
            clipped(
                ui,
                rect(44.0, 193.0, 100.0, 14.0),
                tr!("Hardware volume"),
                9.0,
                muted,
            );
        } else {
            let slider_width = (width / 2.0 - 145.0).clamp(100.0, 170.0);
            let response = ui
                .scope_builder(
                    egui::UiBuilder::new().max_rect(rect(40.0, 184.0, slider_width, 30.0)),
                    |ui| {
                        super::widgets::volume_slider(
                            ui,
                            "mini-volume",
                            &mut self.volume_db,
                            vec2(slider_width, 30.0),
                        )
                    },
                )
                .inner;
            if response.changed() {
                self.gain();
            }
        }
        if button(
            ui,
            &self.icons,
            rect(width - 43.0, 183.0, 30.0, 30.0),
            tr!("Stop"),
            false,
            surface,
            muted,
            "square",
        ) {
            self.stop();
        }
        if let Some(t) = &track {
            let quality_width = (width / 2.0 - 155.0).clamp(90.0, 180.0);
            let quality = if t.rate == 0 {
                t.codec.clone()
            } else if t.bits == 0 || width < 680.0 {
                format!("{} · {:.1} kHz", t.codec, t.rate as f32 / 1000.0)
            } else {
                format!(
                    "{} · {} bit · {:.1} kHz",
                    t.codec,
                    t.bits,
                    t.rate as f32 / 1000.0
                )
            };
            clipped(
                ui,
                rect(
                    width - 53.0 - quality_width,
                    if width < 680.0 && t.bits > 0 {
                        186.0
                    } else {
                        193.0
                    },
                    quality_width,
                    14.0,
                ),
                &quality,
                9.0,
                muted,
            );
            if width < 680.0 && t.bits > 0 {
                clipped(
                    ui,
                    rect(width - 53.0 - quality_width, 201.0, quality_width, 12.0),
                    &format!("{} bit", t.bits),
                    8.5,
                    muted,
                );
            }
        }
        ui.painter().line_segment(
            [
                origin + vec2(20.0, 227.0),
                origin + vec2(width - 20.0, 227.0),
            ],
            egui::Stroke::new(1.0, surface),
        );
        let tab_width = (width - 84.0) / 3.0;
        let active_view = if self.saved.mini_view == 0 {
            self.saved.mini_last_view
        } else {
            self.saved.mini_view
        };
        for (index, icon, label) in [
            (1, "list-music", tr!("Library")),
            (
                2,
                "disc-3",
                self.player_appearance_for(track.as_ref().and_then(|t| t.year))
                    .label(),
            ),
            (3, "book-open", tr!("Artist and lyrics")),
        ] {
            // Stored v0.6 values 1=vinyl / 2=playlist remain compatible.
            let view = match index {
                1 => 2,
                2 => 1,
                other => other,
            };
            let tab = rect(
                20.0 + (index - 1) as f32 * (tab_width + 4.0),
                232.0,
                tab_width,
                28.0,
            );
            let active = active_view == view || (view == 3 && active_view == 4);
            let response = ui.put(
                tab,
                egui::Button::image_and_text(
                    self.icons
                        .image(icon, 14.0, if active { accent } else { muted }),
                    RichText::new(label)
                        .size(11.5)
                        .color(if active { text } else { muted }),
                )
                .fill(if active {
                    surface
                } else {
                    Color32::TRANSPARENT
                })
                .frame_when_inactive(active)
                .corner_radius(5.0),
            );
            if response.clicked() {
                let selected = if active && self.saved.mini_view != 0 {
                    0
                } else if active {
                    active_view
                } else {
                    view
                };
                self.set_mini_view(selected);
                if selected != 0 && selected != active_view {
                    self.last_lyric_line = None;
                }
            }
        }
        if button(
            ui,
            &self.icons,
            rect(width - 48.0, 232.0, 28.0, 28.0),
            tr!("Collapse / expand panel"),
            false,
            surface,
            muted,
            if self.saved.mini_view == 0 {
                "chevron-down"
            } else {
                "chevron-up"
            },
        ) {
            self.set_mini_view(if self.saved.mini_view == 0 {
                self.saved.mini_last_view.clamp(1, 4)
            } else {
                0
            });
        }
        if self.saved.mini_view == 0 {
            return;
        }
        let content = rect(20.0, 275.0, width - 40.0, (area.height() - 291.0).max(1.0));
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("mini-content")
                .layout(egui::Layout::top_down(egui::Align::Min))
                .max_rect(content),
        );
        match self.saved.mini_view {
            1 => self.mini_vinyl(
                &mut child,
                content,
                track.as_ref(),
                surface,
                text,
                muted,
                accent,
            ),
            3 | 4 => self.listening_split_content(&mut child, text, muted, accent),
            _ => self.mini_library(&mut child, surface, text, muted, accent),
        }
        if !self.message.is_empty() {
            clipped(
                ui,
                rect(20.0, area.height() - 13.0, width - 40.0, 12.0),
                &self.playback_message(),
                9.0,
                Color32::from_rgb(243, 148, 128),
            );
        }
    }
    fn mini_library(
        &mut self,
        ui: &mut egui::Ui,
        surface: Color32,
        text: Color32,
        muted: Color32,
        accent: Color32,
    ) {
        let mut source = self.saved.active_source.clone();
        let choices = self.source_choices();
        let total = self.mini_index.scoped.len();
        ui.spacing_mut().interact_size.y = 24.0;
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            egui::ComboBox::from_id_salt("mini-source")
                .width(148.0)
                .truncate()
                .selected_text(RichText::new(self.source_title()).size(11.0))
                .show_ui(ui, |ui| {
                    for (id, name) in choices {
                        ui.selectable_value(&mut source, id, name);
                    }
                });
            ui.add_sized(
                vec2((ui.available_width() - 44.0).max(100.0), 24.0),
                egui::TextEdit::singleline(&mut self.query)
                    .hint_text(tr!("Search this source"))
                    .font(egui::FontId::proportional(11.0)),
            );
            ui.label(
                RichText::new(total.to_string())
                    .monospace()
                    .size(9.0)
                    .color(muted),
            )
            .on_hover_text(tr!("Tracks in the selected source"));
        });
        if source != self.saved.active_source {
            self.select_source(source);
        }
        ui.add_space(4.0);
        self.refresh_mini_index();
        let rows = &self.mini_index.rows;
        if rows.is_empty() {
            ui.label(
                RichText::new(tr!("No tracks found"))
                    .size(12.0)
                    .color(muted),
            );
            return;
        }
        ui.spacing_mut().item_spacing.y = 0.0;
        let mut selected = None;
        egui::ScrollArea::vertical()
            .id_salt("mini-playlist")
            .auto_shrink([false, false])
            .show_rows(ui, 38.0, rows.len(), |ui, range| {
                for i in range {
                    let t = &self.saved.tracks[rows[i]];
                    let active = self.current.as_ref() == Some(&t.source);
                    let (row, response) = ui.allocate_exact_size(
                        vec2(ui.available_width(), 38.0),
                        egui::Sense::click(),
                    );
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &t.title)
                    });
                    if active || response.hovered() {
                        ui.painter().rect_filled(
                            row,
                            4.0,
                            if active {
                                accent.gamma_multiply(0.12)
                            } else {
                                surface
                            },
                        );
                    }
                    if active {
                        ui.painter().rect_filled(
                            egui::Rect::from_min_size(row.min + vec2(0.0, 11.0), vec2(2.0, 21.0)),
                            1.0,
                            accent,
                        );
                    }
                    let number = if active {
                        "›".to_string()
                    } else {
                        format!(
                            "{:02}",
                            if t.number > 0 {
                                t.number as usize
                            } else {
                                i + 1
                            }
                        )
                    };
                    ui.painter().text(
                        row.min + vec2(10.0, 13.0),
                        egui::Align2::LEFT_TOP,
                        number,
                        egui::FontId::monospace(10.0),
                        if active { accent } else { muted },
                    );
                    let title_rect = egui::Rect::from_min_size(
                        row.min + vec2(35.0, 3.0),
                        vec2(row.width() - 95.0, 19.0),
                    );
                    clipped(
                        ui,
                        title_rect,
                        &t.title,
                        12.0,
                        if active { accent } else { text },
                    );
                    let subtitle = if t.album.is_empty() {
                        t.artist.clone()
                    } else {
                        format!("{} · {}", t.artist, t.album)
                    };
                    clipped(
                        ui,
                        title_rect.translate(vec2(0.0, 18.0)),
                        &subtitle,
                        9.5,
                        muted,
                    );
                    ui.painter().text(
                        row.right_top() + vec2(-8.0, 14.0),
                        egui::Align2::RIGHT_TOP,
                        if t.duration > 0.0 {
                            time(t.duration)
                        } else {
                            "--:--".into()
                        },
                        egui::FontId::monospace(9.5),
                        muted,
                    );
                    if response.double_clicked()
                        || (response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                    {
                        selected = Some(t.source.clone());
                    }
                }
            });
        if let Some(id) = selected {
            self.play_results(id);
        }
    }
    pub(super) fn remember_mini_size(&mut self, size: egui::Vec2) {
        if !self.window_geometry.remember(true, size) {
            return;
        }
        if !size.x.is_finite() || !size.y.is_finite() {
            return;
        }
        let old = self.saved.mini_size.unwrap_or([520.0, 520.0]);
        self.saved.mini_size = Some([
            size.x.max(520.0),
            if self.saved.mini_view == 0 {
                old[1]
            } else {
                size.y.max(520.0)
            },
        ]);
    }
    pub(super) fn remember_full_size(&mut self, size: egui::Vec2) {
        if !self.window_geometry.remember(false, size) {
            return;
        }
        if size.x.is_finite() && size.y.is_finite() && size.x > 0.0 && size.y > 0.0 {
            self.saved.full_size = Some([size.x.clamp(640.0, 8192.0), size.y.clamp(440.0, 8192.0)]);
        }
    }
    pub(super) fn full_size(&self) -> egui::Vec2 {
        let size = self.saved.full_size.unwrap_or([1120.0, 760.0]);
        let valid = |v: f32, min: f32, fallback: f32| {
            if v.is_finite() {
                v.clamp(min, 8192.0)
            } else {
                fallback
            }
        };
        vec2(valid(size[0], 640.0, 1120.0), valid(size[1], 440.0, 760.0))
    }
    pub(super) fn mini_size(&self) -> egui::Vec2 {
        let size = self.saved.mini_size.unwrap_or([520.0, 520.0]);
        let valid = |v: f32| {
            if v.is_finite() {
                v.clamp(520.0, 8192.0)
            } else {
                520.0
            }
        };
        vec2(
            valid(size[0]),
            if self.saved.mini_view == 0 {
                262.0
            } else {
                valid(size[1])
            },
        )
    }
    fn set_mini_view(&mut self, view: u8) {
        let old = self.saved.mini_view;
        if old != 0 {
            self.saved.mini_last_view = old;
        }
        if view != 0 {
            self.saved.mini_last_view = view;
        }
        self.remember_mini_size(self.ctx.content_rect().size());
        self.saved.mini_view = view;
        // Changing an expanded tab is a content change, not a window resize.
        if (old == 0) == (view == 0) {
            return;
        }
        self.resize_window(&self.ctx.clone());
    }
    #[allow(clippy::too_many_arguments)]
    fn mini_vinyl(
        &mut self,
        ui: &mut egui::Ui,
        content: egui::Rect,
        current: Option<&Track>,
        surface: Color32,
        text: Color32,
        muted: Color32,
        accent: Color32,
    ) {
        let layout = VinylLayout::new(content);
        let track_progress = current
            .filter(|track| {
                self.position.is_finite() && track.duration.is_finite() && track.duration > 0.0
            })
            .map_or(0.0, |track| {
                (self.position / track.duration).clamp(0.0, 1.0) as f32
            });
        self.reel_motion.progress = track_progress;
        let appearance = self.player_appearance_for(current.and_then(|track| track.year));
        appearance.paint(
            ui.painter(),
            layout.disc,
            player_visual::VisualState {
                phase: self.angle,
                progress: track_progress,
                reels: self.reel_motion.angles,
                art: self.art.as_ref(),
                album: current.map_or("", |track| track.album.as_str()),
                vu: self.needle_motion.levels,
            },
        );
        let visual_response = ui.interact(
            layout.disc,
            ui.id().with("player-visual"),
            egui::Sense::click(),
        );
        if cover_viewer::requested(&visual_response) {
            self.open_cover_viewer();
        }
        visual_response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Image, true, appearance.label())
        });
        let Some(current) = current else {
            return;
        };
        self.vinyl_index
            .refresh(&self.saved.tracks, self.library_revision, current);
        let tracks = &self.vinyl_index.rows;
        clipped(
            ui,
            egui::Rect::from_min_size(content.min, vec2(content.width() - 68.0, 22.0)),
            if current.album.is_empty() {
                tr!("Album playing")
            } else {
                &current.album
            },
            14.0,
            text,
        );
        ui.painter().text(
            content.right_top(),
            egui::Align2::RIGHT_TOP,
            crate::i18n::tracks(tracks.len()),
            egui::FontId::proportional(9.0),
            muted,
        );
        let half = tracks.len().div_ceil(2);
        let mut selected = None;
        for (side, rect, rows) in [
            (0, layout.left, &tracks[..half]),
            (1, layout.right, &tracks[half..]),
        ] {
            let mut column = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt(("vinyl-column", side))
                    .max_rect(rect)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            column.set_clip_rect(rect.intersect(ui.clip_rect()));
            // Titles belong to the row button: text selection must not consume
            // the click before it reaches the playback action.
            column.style_mut().interaction.selectable_labels = false;
            column.spacing_mut().item_spacing.y = 0.0;
            egui::ScrollArea::vertical()
                .id_salt((
                    "vinyl-tracks",
                    side,
                    self.vinyl_index.key.as_ref().map(|(_, _, album)| album),
                ))
                .auto_shrink([false, false])
                .show_rows(&mut column, 28.0, rows.len(), |ui, range| {
                    for i in range {
                        let t = &self.saved.tracks[rows[i]];
                        let active = t.source == current.source;
                        let (row, response) = ui.allocate_exact_size(
                            vec2(ui.available_width(), 28.0),
                            egui::Sense::click(),
                        );
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &t.title)
                        });
                        if active || response.hovered() || response.has_focus() {
                            ui.painter().rect_filled(row, 4.0, surface);
                        }
                        ui.painter().text(
                            row.min + vec2(5.0, 6.0),
                            egui::Align2::LEFT_TOP,
                            format!(
                                "{:02}",
                                if t.number > 0 {
                                    t.number as usize
                                } else {
                                    side * half + i + 1
                                }
                            ),
                            egui::FontId::monospace(10.0),
                            if active { accent } else { muted },
                        );
                        clipped(
                            ui,
                            egui::Rect::from_min_size(
                                row.min + vec2(29.0, 4.0),
                                vec2((row.width() - 32.0).max(1.0), 22.0),
                            ),
                            &t.title,
                            12.5,
                            if active { accent } else { muted },
                        );
                        if response.clicked()
                            || (response.has_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                        {
                            selected = Some(t.source.clone());
                        }
                        response
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text(&t.title);
                    }
                });
        }
        if let Some(id) = selected {
            // A direct track choice must not replace the listening selection or
            // turn shuffle tracks into an implicit single-album shuffle.
            self.play(id);
        }
    }
}
fn player_content_area(area: egui::Rect) -> egui::Rect {
    let maximum = (area.height() * 1.85).clamp(900.0, 1280.0);
    let width = area.width().min(maximum);
    egui::Rect::from_min_size(
        pos2(area.center().x - width * 0.5, area.top()),
        vec2(width, area.height()),
    )
}

struct VinylLayout {
    disc: egui::Rect,
    left: egui::Rect,
    right: egui::Rect,
}
impl VinylLayout {
    fn new(content: egui::Rect) -> Self {
        let body = egui::Rect::from_min_max(content.min + vec2(0.0, 30.0), content.max);
        let diameter = body.height().min(body.width() * 0.44).max(1.0);
        let disc = egui::Rect::from_min_size(
            pos2(body.center().x - diameter * 0.5, body.top()),
            vec2(diameter, diameter),
        );
        Self {
            disc,
            left: egui::Rect::from_min_max(body.min, pos2(disc.left() - 12.0, body.bottom())),
            right: egui::Rect::from_min_max(pos2(disc.right() + 12.0, body.top()), body.max),
        }
    }
}
fn drag_titlebar(ui: &mut egui::Ui, rect: egui::Rect) {
    let header = ui.interact(
        rect,
        egui::Id::new("mini-drag"),
        egui::Sense::click_and_drag(),
    );
    let started = header.id.with("native-drag-started");
    let armed = header.id.with("titlebar-pressed");
    if ui.input(|i| i.pointer.primary_pressed()) {
        window_drag::reset(ui.ctx());
        if header.is_pointer_button_down_on() {
            ui.ctx().data_mut(|data| data.insert_temp(armed, true));
            window_drag::arm(ui.ctx());
        }
    }
    if !ui.input(|i| i.pointer.primary_down())
        || ui.input(|i| {
            i.events.iter().any(|event| {
                matches!(
                    event,
                    egui::Event::PointerGone | egui::Event::WindowFocused(false)
                )
            })
        })
    {
        window_drag::reset(ui.ctx());
        return;
    }
    if !ui
        .ctx()
        .data(|data| data.get_temp::<bool>(armed).unwrap_or(false))
    {
        return;
    }
    // A drag-only sense starts on mouse-down, including a click that merely
    // focuses the window. Wait for real movement before entering macOS's
    // native move loop; holding a stationary click must not start it either.
    let threshold = ui.ctx().options(|o| o.input_options.max_click_dist);
    let moved = ui.input(|i| {
        i.pointer
            .press_origin()
            .zip(i.pointer.interact_pos())
            .is_some_and(|(from, to)| from.distance(to) > threshold)
    });
    if header.dragged_by(egui::PointerButton::Primary)
        && moved
        && !ui
            .ctx()
            .data(|d| d.get_temp::<bool>(started).unwrap_or(false))
    {
        ui.ctx().data_mut(|d| d.insert_temp(started, true));
        window_drag::start(ui.ctx());
    }
}

#[allow(clippy::too_many_arguments)]
fn button_if(
    ui: &mut egui::Ui,
    icons: &Icons,
    rect: egui::Rect,
    label: &str,
    active: bool,
    surface: Color32,
    color: Color32,
    icon: &str,
    enabled: bool,
) -> bool {
    ui.scope(|ui| {
        if !enabled {
            ui.disable();
        }
        button(ui, icons, rect, label, active, surface, color, icon)
    })
    .inner
}
#[allow(clippy::too_many_arguments)]
fn button(
    ui: &mut egui::Ui,
    icons: &Icons,
    rect: egui::Rect,
    label: &str,
    active: bool,
    surface: Color32,
    color: Color32,
    icon: &str,
) -> bool {
    let primary = matches!(icon, "play" | "pause");
    let response = ui
        .scope(|ui| {
            ui.spacing_mut().button_padding = vec2(4.0, 4.0);
            ui.put(
                rect,
                egui::Button::image(icons.image(
                    icon,
                    if primary { 20.0 } else { 16.0 },
                    if primary { palette::bg() } else { color },
                ))
                .fill(if primary {
                    color
                } else if active {
                    surface
                } else {
                    Color32::TRANSPARENT
                })
                .frame_when_inactive(primary || active)
                .corner_radius(if primary { 21.0 } else { 5.0 }),
            )
        })
        .inner;
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    response.on_hover_text(label).clicked()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn flat_phone_catalog_visual_lists_only_current_album() {
        for appearance in player_visual::Appearance::ALL {
            let dir = tempfile::tempdir().unwrap();
            let ctx = egui::Context::default();
            let cc = eframe::CreationContext::_new_kittest(ctx.clone());
            let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
            app.preview("mini-vinyl-flat-catalog");
            app.saved.player_appearance = appearance;
            let current = app.current.clone();
            let selection = app.playback_selection.clone();
            frame_sized(&mut app, &ctx, vec2(1018.0, 842.0), vec![]);
            let output = frame_sized(&mut app, &ctx, vec2(1018.0, 842.0), vec![]);
            assert_eq!(app.vinyl_index.rows, (0..8).collect::<Vec<_>>());
            let nodes = output.platform_output.accesskit_update.unwrap().nodes;
            assert!(
                nodes
                    .iter()
                    .any(|(_, n)| n.label() == Some("Opening Light"))
            );
            assert!(
                !nodes
                    .iter()
                    .any(|(_, n)| { n.label().is_some_and(|s| s.starts_with("Unrelated")) })
            );
            assert_eq!(app.current, current);
            assert_eq!(app.playback_selection, selection);
            assert_eq!(app.saved.tracks.len(), 72);
        }
    }

    #[test]
    fn album_rows_distinguish_artists_editions_sources_and_unknown_tags() {
        fn track(source: &str, album: &str, artist: &str, disc: u32, number: u32) -> Track {
            let mut t = Track::placeholder(source.into());
            t.album = album.into();
            t.artist = artist.into();
            t.disc = disc;
            t.number = number;
            t
        }
        let root = "https://phone.invalid:8737/v1/audio/";
        let mut tracks = vec![
            track(&format!("{root}90.flac"), "Collection", "Artist A", 2, 1),
            track(&format!("{root}20.flac"), "Collection", "Artist A", 1, 2),
            track(&format!("{root}80.flac"), "Collection", "Artist A", 1, 1),
            track(&format!("{root}1.flac"), "Collection", "Artist B", 1, 1),
            track(&format!("{root}2.flac"), "Other album", "Artist A", 1, 1),
            track(
                "https://other.invalid/v1/audio/1.flac",
                "Collection",
                "Artist A",
                1,
                1,
            ),
            track("/music/Collection/1.flac", "Collection", "Artist A", 1, 1),
            track(
                &format!("{root}3.flac"),
                "Collection (Deluxe)",
                "Artist A",
                1,
                1,
            ),
            track(&format!("{root}4.flac"), "", "Artist A", 1, 1),
            track(&format!("{root}5.flac"), "", "Artist A", 1, 2),
        ];
        let mut index = VinylIndex::default();
        index.refresh(&tracks, 1, &tracks[0]);
        assert_eq!(index.rows, [2, 1, 0]);
        // Album changes in the same endpoint must invalidate the cached group.
        index.refresh(&tracks, 1, &tracks[3]);
        assert_eq!(index.rows, [3]);
        index.refresh(&tracks, 1, &tracks[8]);
        assert_eq!(index.rows, [8], "an untagged HTTP URL is not an album");
        // Compilations use album artist even when the performers differ.
        for t in &mut tracks[..4] {
            t.album_artist = "Various Artists".into();
        }
        index.refresh(&tracks, 2, &tracks[0]);
        assert_eq!(index.rows.len(), 4);
        // Refresh when tags change, even without adding/removing tracks.
        tracks[3].album = "Other album".into();
        index.refresh(&tracks, 3, &tracks[0]);
        assert_eq!(index.rows, [2, 1, 0]);
        // Physical folders still keep separate editions and unknown albums.
        let local = [
            track("/music/a/1.flac", "", "A", 1, 1),
            track("/music/a/2.flac", "", "B", 1, 2),
            track("/music/a/3.flac", "Known", "A", 1, 3),
            track("/music/b/1.flac", "", "A", 1, 1),
        ];
        index.refresh(&local, 4, &local[0]);
        assert_eq!(index.rows, [0, 1]);
    }

    #[test]
    fn mini_indices_refresh_for_query_scope_metadata_and_new_tracks_only() {
        let mut tracks = vec![
            Track::placeholder("/nas/01.flac".into()),
            Track::placeholder("ftp://phone/Music/02.flac".into()),
        ];
        tracks[0].title = "Prima".into();
        let mut index = MiniIndex::default();
        for _ in 0..120 {
            index.refresh(&tracks, library::Scope::Local("/nas".into()), 0, "");
        }
        assert_eq!(index.rebuilds, 1);
        assert_eq!(index.rows, [0]);
        index.refresh(&tracks, library::Scope::Local("/nas".into()), 0, "seconda");
        assert!(index.rows.is_empty());
        tracks[0].title = "Seconda".into();
        index.refresh(&tracks, library::Scope::Local("/nas".into()), 1, "seconda");
        assert_eq!(index.rows, [0]);
        tracks.push(Track::placeholder("/nas/new.flac".into()));
        index.refresh(&tracks, library::Scope::Local("/nas".into()), 1, "");
        assert_eq!(index.rows, [0, 2]);
        index.refresh(
            &tracks,
            library::Scope::Remote("ftp://phone/Music/".into()),
            1,
            "",
        );
        assert_eq!(index.rows, [1]);
        let mut vinyl = VinylIndex::default();
        vinyl.refresh(&tracks, 1, &tracks[0]);
        assert_eq!(vinyl.rows.len(), 2);
        tracks[2].number = 1;
        tracks[0].number = 2;
        vinyl.refresh(&tracks, 2, &tracks[0]);
        assert_eq!(vinyl.rows, [2, 0]);
        vinyl.refresh(&tracks, 2, &tracks[1]);
        assert_eq!(vinyl.rows, [1]);
    }
    fn frame(app: &mut Aurea, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
        frame_sized(app, ctx, vec2(520.0, 520.0), events)
    }
    fn frame_sized(
        app: &mut Aurea,
        ctx: &egui::Context,
        size: egui::Vec2,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(pos2(0.0, 0.0), size)),
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| app.mini_ui(ui));
        output.textures_delta.clear();
        output
    }
    fn click_sized(
        app: &mut Aurea,
        ctx: &egui::Context,
        size: egui::Vec2,
        pos: egui::Pos2,
    ) -> egui::FullOutput {
        let mut output = frame_sized(app, ctx, size, vec![egui::Event::PointerMoved(pos)]);
        for pressed in [true, false] {
            output = frame_sized(
                app,
                ctx,
                size,
                vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
        }
        output
    }
    fn click(app: &mut Aurea, ctx: &egui::Context, pos: egui::Pos2) {
        frame(app, ctx, vec![egui::Event::PointerMoved(pos)]);
        for pressed in [true, false] {
            frame(
                app,
                ctx,
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
    fn interrupted_titlebar_gestures_require_a_fresh_press() {
        for interruption in [
            Some(egui::Event::WindowFocused(false)),
            Some(egui::Event::PointerGone),
            None,
        ] {
            let ctx = egui::Context::default();
            let frame = |events| {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            pos2(0.0, 0.0),
                            vec2(520.0, 520.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        drag_titlebar(
                            ui,
                            egui::Rect::from_min_max(pos2(12.0, 4.0), pos2(356.0, 36.0)),
                        )
                    },
                );
                output.textures_delta.clear();
                output
                    .viewport_output
                    .values()
                    .flat_map(|viewport| &viewport.commands)
                    .filter(|command| matches!(command, egui::ViewportCommand::StartDrag))
                    .count()
            };
            let origin = pos2(80.0, 20.0);
            let press = |pressed| egui::Event::PointerButton {
                pos: origin,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            frame(vec![egui::Event::PointerMoved(origin)]);
            assert_eq!(frame(vec![press(true)]), 0);
            if let Some(event) = interruption {
                assert_eq!(frame(vec![event]), 0);
            } else {
                // Same cancellation used when switching full/mini geometry.
                window_drag::reset(&ctx);
            }
            assert_eq!(
                frame(vec![
                    egui::Event::WindowFocused(true),
                    egui::Event::PointerMoved(origin + vec2(60.0, 0.0))
                ]),
                0,
                "a stale button-down must not rearm the native drag"
            );
            frame(vec![press(false)]);
            frame(vec![egui::Event::PointerMoved(origin)]);
            assert_eq!(frame(vec![press(true)]), 0);
            assert_eq!(
                frame(vec![egui::Event::PointerMoved(origin + vec2(20.0, 0.0))]),
                1
            );
        }
    }

    #[test]
    fn titlebar_click_does_not_start_a_native_window_drag() {
        let ctx = egui::Context::default();
        let frame = |events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        pos2(0.0, 0.0),
                        vec2(520.0, 520.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    drag_titlebar(
                        ui,
                        egui::Rect::from_min_max(pos2(12.0, 4.0), pos2(356.0, 36.0)),
                    )
                },
            );
            output.textures_delta.clear();
            output
                .viewport_output
                .values()
                .flat_map(|v| &v.commands)
                .filter(|c| matches!(c, egui::ViewportCommand::StartDrag))
                .count()
        };
        let origin = pos2(80.0, 20.0);
        frame(vec![egui::Event::PointerMoved(origin)]);
        for button in [egui::PointerButton::Primary, egui::PointerButton::Secondary] {
            let press = |pressed| egui::Event::PointerButton {
                pos: origin,
                button,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            assert_eq!(frame(vec![press(true)]), 0);
            assert_eq!(
                frame(vec![egui::Event::PointerMoved(origin + vec2(1.0, 0.0))]),
                0
            );
            assert_eq!(frame(vec![press(false)]), 0);
        }
        frame(vec![egui::Event::PointerMoved(origin)]);
        assert_eq!(
            frame(vec![egui::Event::PointerButton {
                pos: origin,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }]),
            0
        );
        // Holding the mouse still beyond egui's long-press timeout is not a drag.
        for _ in 0..60 {
            assert_eq!(frame(vec![]), 0);
        }
        assert_eq!(
            frame(vec![egui::Event::PointerMoved(origin + vec2(20.0, 0.0))]),
            1
        );
        assert_eq!(
            frame(vec![egui::Event::PointerMoved(origin + vec2(30.0, 0.0))]),
            0
        );
    }
    #[test]
    fn collapse_reopens_the_selected_tab_after_repeated_toggles_and_profile_reload() {
        for size in [vec2(520.0, 520.0), vec2(812.0, 600.0)] {
            for view in [1, 2, 3, 4] {
                let dir = tempfile::tempdir().unwrap();
                let ctx = egui::Context::default();
                let cc = eframe::CreationContext::_new_kittest(ctx.clone());
                let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
                app.saved.mini_view = view;
                for _ in 0..4 {
                    click_sized(&mut app, &ctx, size, pos2(size.x - 34.0, 246.0));
                    assert_eq!(app.saved.mini_view, 0);
                    assert_eq!(app.saved.mini_last_view, view);
                    app.saved =
                        serde_json::from_str(&serde_json::to_string(&app.saved).unwrap()).unwrap();
                    click_sized(&mut app, &ctx, size, pos2(size.x - 34.0, 246.0));
                    assert_eq!(app.saved.mini_view, view);
                }
            }
        }
    }

    #[test]
    fn order_icon_cycles_in_both_dark_and_light_mini_without_changing_repeat() {
        for theme in [0, 2] {
            let dir = tempfile::tempdir().unwrap();
            let ctx = egui::Context::default();
            let cc = eframe::CreationContext::_new_kittest(ctx.clone());
            let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
            app.saved.theme = theme;
            app.saved.repeat = true;
            for expected in [(true, false), (false, true), (false, false)] {
                click(&mut app, &ctx, pos2(174.0, 198.0));
                assert_eq!((app.saved.shuffle, app.saved.shuffle_albums), expected);
                assert!(app.saved.repeat);
                assert!(app.current.is_none());
                assert!(app.requested.is_none());
                assert!(app.planned.is_none());
            }
        }
    }
    #[test]
    fn mini_views_are_accessible_without_starting_audio() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
        for (x, view) in [(55.0, 2), (205.0, 1), (354.0, 3), (354.0, 0)] {
            click(&mut app, &ctx, pos2(x, 246.0));
            assert_eq!(app.saved.mini_view, view);
            assert!(app.current.is_none());
            assert!(app.requested.is_none());
        }
        click(&mut app, &ctx, pos2(393.0, 22.0));
        assert!(!app.saved.mini);
    }
    #[test]
    fn library_shows_five_complete_rows_and_karaoke_click_seeks_without_unpausing() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
        app.preview("mini");
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        let update = output.platform_output.accesskit_update.unwrap();
        let fifth = update
            .nodes
            .iter()
            .find(|(_, n)| n.label() == Some("Interlude No. 2"))
            .unwrap()
            .1
            .bounds()
            .unwrap();
        assert!(fifth.y1 <= 504.0, "Fifth row is clipped: {fifth:?}");
        app.preview("mini-lyrics");
        app.paused = true;
        app.lyric_follow = false;
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        let update = output.platform_output.accesskit_update.unwrap();
        let bounds = update
            .nodes
            .iter()
            .find(|(_, n)| n.label() == Some("La luce attraversa la stanza"))
            .unwrap()
            .1
            .bounds()
            .unwrap();
        click(
            &mut app,
            &ctx,
            pos2(
                ((bounds.x0 + bounds.x1) / 2.0) as f32,
                ((bounds.y0 + bounds.y1) / 2.0) as f32,
            ),
        );
        assert_eq!(app.position, 12.0);
        assert!(app.paused);
        assert!(app.requested.is_none());
    }
    #[test]
    fn combined_info_keeps_both_columns_and_format_visible_in_legacy_and_wide_views() {
        for (size, view) in [(vec2(520.0, 520.0), 3), (vec2(812.0, 640.0), 4)] {
            let dir = tempfile::tempdir().unwrap();
            let ctx = egui::Context::default();
            let cc = eframe::CreationContext::_new_kittest(ctx.clone());
            let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
            app.preview("mini-artist");
            app.saved.mini_view = view;
            app.lyric_follow = false;
            frame_sized(&mut app, &ctx, size, vec![]);
            let out = frame_sized(&mut app, &ctx, size, vec![]);
            let nodes = out.platform_output.accesskit_update.unwrap().nodes;
            let bio = nodes
                .iter()
                .find(|(_, n)| {
                    n.value() == Some("Contenuto sintetico per il collaudo dell’interfaccia.")
                })
                .expect("Biography paragraph must be visible in the combined tab")
                .1
                .bounds()
                .unwrap();
            let lyric = nodes
                .iter()
                .find(|(_, n)| n.label() == Some("La luce attraversa la stanza"))
                .unwrap()
                .1
                .bounds()
                .unwrap();
            assert!(
                bio.x1 <= f64::from(size.x / 2.0 - 10.0) + 1.0
                    && bio.y1 <= f64::from(size.y - 16.0)
            );
            assert!(
                lyric.x0 >= f64::from(size.x / 2.0 + 10.0) - 1.0
                    && lyric.y1 <= f64::from(size.y - 16.0)
            );
            assert!(nodes.iter().any(|(_, n)| {
                n.value()
                    .is_some_and(|s| s.contains("FLAC") && s.contains("kHz"))
            }));
            app.lyric_follow = true;
            frame_sized(
                &mut app,
                &ctx,
                size,
                vec![
                    egui::Event::PointerMoved(pos2(30.0, 400.0)),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: vec2(0.0, -40.0),
                        phase: egui::TouchPhase::Move,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
            assert!(
                app.lyric_follow,
                "Scrolling the biography must not disable lyric follow"
            );
        }
    }
    #[test]
    fn resized_mini_keeps_size_across_tabs_collapse_and_serialization() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
        app.preview("mini-vinyl");
        let size = vec2(812.0, 640.0);
        for (x, view) in [(625.0, 3), (380.0, 1), (625.0, 3)] {
            let output = click_sized(&mut app, &ctx, size, pos2(x, 246.0));
            assert_eq!(app.saved.mini_view, view);
            assert_eq!(app.mini_size(), size);
            assert!(output.viewport_output.values().all(|v| {
                v.commands
                    .iter()
                    .all(|c| !matches!(c, egui::ViewportCommand::InnerSize(_)))
            }));
        }
        click_sized(&mut app, &ctx, size, pos2(658.0, 246.0));
        assert_eq!(app.mini_size(), vec2(812.0, 262.0));
        click_sized(&mut app, &ctx, vec2(812.0, 262.0), pos2(100.0, 246.0));
        assert_eq!(app.mini_size(), size);
        let restored: Saved =
            serde_json::from_value(serde_json::to_value(&app.saved).unwrap()).unwrap();
        assert_eq!(restored.mini_size, Some([812.0, 640.0]));
        let legacy: Saved =
            serde_json::from_value(serde_json::json!({"mini":true,"mini_view":1})).unwrap();
        assert!(legacy.mini_size.is_none());
    }
    #[test]
    fn repeated_order_cycles_keep_current_audio_selection_and_latest_candidate_consistent() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
        let (tx, commands) = mpsc::channel();
        app.audio.tx = tx;
        app.saved.tracks = [
            "/synthetic/A/1.wav",
            "/synthetic/A/2.wav",
            "/synthetic/B/1.wav",
            "/synthetic/B/2.wav",
        ]
        .into_iter()
        .map(|s| Track::placeholder(s.into()))
        .collect();
        app.current = Some("/synthetic/A/1.wav".into());
        app.ticket = 7;
        app.audio.generation.store(7, Ordering::Relaxed);
        app.saved.repeat = false;
        app.playback_selection = Some(app.saved.tracks.iter().map(|t| t.source.clone()).collect());
        let selected = app.playback_selection.clone();
        for cycle in 1..=12 {
            let before = app.audio.plan.load(Ordering::Relaxed);
            click(&mut app, &ctx, pos2(174.0, 198.0));
            let output = frame(&mut app, &ctx, vec![]);
            assert_eq!(app.order_mode(), cycle % 3);
            assert!(app.audio.plan.load(Ordering::Relaxed) > before);
            assert_eq!(
                app.audio.generation.load(Ordering::Relaxed),
                7,
                "mode changes cannot restart current audio"
            );
            assert_eq!(app.current.as_deref(), Some("/synthetic/A/1.wav"));
            assert_eq!(app.playback_selection, selected);
            if cycle % 3 == 1 {
                assert!(
                    app.planned
                        .as_ref()
                        .is_some_and(|id| id != "/synthetic/A/1.wav"
                            && app.saved.tracks.iter().any(|t| &t.source == id))
                );
            } else {
                assert_eq!(app.planned.as_deref(), Some("/synthetic/A/2.wav"));
            }
            let label = crate::i18n::format(
                "Playback order: {mode}",
                &[("mode", app.order_label().to_string())],
            );
            assert!(
                output
                    .platform_output
                    .accesskit_update
                    .unwrap()
                    .nodes
                    .iter()
                    .any(|(_, n)| n.label() == Some(label.as_str())),
                "displayed mode matches active behavior"
            );
            assert!(
                commands.try_recv().is_err(),
                "mode change must not request a new current track"
            );
        }
    }
    #[test]
    fn returning_to_track_shuffle_replaces_old_plans_for_next_and_end_of_track() {
        for previous_mode in [0, 2] {
            for filtered in [false, true] {
                for automatic in [false, true] {
                    let dir = tempfile::tempdir().unwrap();
                    let ctx = egui::Context::default();
                    let cc = eframe::CreationContext::_new_kittest(ctx.clone());
                    let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
                    let (commands_tx, commands) = mpsc::channel();
                    let (events_tx, events) = mpsc::channel();
                    app.audio.tx = commands_tx;
                    app.audio.rx = events;
                    app.saved.auto_sync = false;
                    app.online_covers.store(false, Ordering::Relaxed);
                    app.saved.mini = true;
                    app.saved.folders = vec!["/synthetic/phone".into(), "/synthetic/NAS".into()];
                    app.saved.active_source = Some("folder:/synthetic/phone".into());
                    app.saved.tracks = [
                        "/synthetic/phone/A/1.wav",
                        "/synthetic/phone/A/2.wav",
                        "/synthetic/phone/B/1.wav",
                        "/synthetic/phone/B/2.wav",
                        "/synthetic/NAS/C/1.wav",
                    ]
                    .into_iter()
                    .map(|s| Track::placeholder(s.into()))
                    .collect();
                    if filtered {
                        app.playback_selection = Some(
                            app.saved.tracks[..3]
                                .iter()
                                .map(|t| t.source.clone())
                                .collect(),
                        );
                    }
                    app.current = Some(app.saved.tracks[0].source.clone());
                    app.ticket = 7;
                    app.audio.generation.store(7, Ordering::Relaxed);
                    app.saved.repeat = false;
                    app.saved.shuffle = false;
                    app.saved.shuffle_albums = previous_mode == 2;
                    for i in [3, 0, 1] {
                        app.history.record(app.saved.tracks[i].source.clone());
                    }
                    app.history.previous();
                    app.history.record(app.current.clone().unwrap());
                    app.prepare_next();
                    assert_eq!(app.planned, Some(app.saved.tracks[1].source.clone()));
                    let old_plan = app.audio.plan.load(Ordering::Relaxed);
                    let selected = app.playback_selection.clone();
                    for _ in 0..if previous_mode == 2 { 2 } else { 1 } {
                        click(&mut app, &ctx, pos2(174.0, 198.0));
                    }
                    assert_eq!(app.order_mode(), 1);
                    assert!(app.saved.shuffle && !app.saved.shuffle_albums);
                    assert_eq!(app.history.peek_next(), None);
                    assert!(app.audio.plan.load(Ordering::Relaxed) > old_plan);
                    assert_eq!(app.audio.generation.load(Ordering::Relaxed), 7);
                    assert_eq!(app.playback_selection, selected);
                    let expected = app.planned.clone().unwrap();
                    assert_ne!(Some(&expected), app.current.as_ref());
                    assert!(app.listening_tracks().iter().any(|t| t.source == expected));
                    assert_eq!(app.listening_tracks().len(), if filtered { 3 } else { 4 });
                    assert!(commands.try_recv().is_err());
                    if automatic {
                        events_tx.send(crate::audio::Event::Ended).unwrap();
                        app.poll(&ctx);
                    } else {
                        app.next();
                    }
                    assert_eq!(app.requested.as_ref(), Some(&expected));
                    assert!(matches!(
                        commands.try_recv(),
                        Ok(Command::Load { track, .. }) if track.source == expected
                    ));
                    assert_eq!(app.order_mode(), 1);
                }
            }
        }
    }
    #[test]
    fn mode_changes_replace_the_old_forward_path_and_match_the_gapless_choice() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
        let (tx, commands) = mpsc::channel();
        app.audio.tx = tx;
        app.saved.auto_sync = false;
        app.online_covers.store(false, Ordering::Relaxed);
        app.saved.tracks = [
            "/synthetic/A/1.wav",
            "/synthetic/A/2.wav",
            "/synthetic/B/1.wav",
            "/synthetic/B/2.wav",
        ]
        .into_iter()
        .map(|s| Track::placeholder(s.into()))
        .collect();
        app.current = Some("/synthetic/B/1.wav".into());
        for source in [
            "/synthetic/A/1.wav",
            "/synthetic/B/1.wav",
            "/synthetic/A/2.wav",
        ] {
            app.history.record(source.into());
        }
        app.history.previous();
        app.history.record("/synthetic/B/1.wav".into());
        app.saved.shuffle = true;
        // Cycle shuffle tracks -> shuffle albums -> sequence, with a forward
        // history entry that conflicts with the selected sequence.
        click(&mut app, &ctx, pos2(174.0, 198.0));
        click(&mut app, &ctx, pos2(174.0, 198.0));
        assert_eq!(app.order_mode(), 0);
        assert_eq!(app.planned.as_deref(), Some("/synthetic/B/2.wav"));
        let next = app.planned.clone();
        app.next();
        assert_eq!(
            app.requested, next,
            "Next must match the prepared transition after a mode change"
        );
        assert!(
            matches!(commands.try_recv(),Ok(Command::Load{track,..}) if track.source=="/synthetic/B/2.wav")
        );
    }
    #[test]
    fn vinyl_track_click_preserves_all_order_modes_and_the_existing_selection() {
        for appearance in player_visual::Appearance::ALL {
            for mode in 0..3 {
                for filtered in [false, true] {
                    for (size, title) in [
                        (vec2(520.0, 520.0), "Beyond the Shore"),
                        (vec2(812.0, 640.0), "Evening Lines"),
                    ] {
                        let dir = tempfile::tempdir().unwrap();
                        let ctx = egui::Context::default();
                        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
                        let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
                        app.preview("mini-vinyl");
                        app.saved.player_appearance = appearance;
                        app.saved.tracks.truncate(8);
                        for (i, t) in app.saved.tracks.iter_mut().enumerate() {
                            t.source = format!("/synthetic-not-present/Music/album/{i}.flac");
                        }
                        app.saved.tracks.push(Track::placeholder(
                            "/synthetic-not-present/Music/other-album/track.flac".into(),
                        ));
                        app.saved.shuffle = mode == 1;
                        app.saved.shuffle_albums = mode == 2;
                        if filtered {
                            app.playback_selection = Some(
                                [8, 5, 1]
                                    .into_iter()
                                    .map(|i| app.saved.tracks[i].source.clone())
                                    .collect(),
                            );
                        }
                        let selection = app.playback_selection.clone();
                        app.saved.folders = vec!["/synthetic-not-present/Music".into()];
                        app.saved.active_source =
                            Some("folder:/synthetic-not-present/Music".into());
                        app.current = Some(app.saved.tracks[0].source.clone());
                        app.query = "filter excludes clicked track".into();
                        frame_sized(&mut app, &ctx, size, vec![]);
                        let output = frame_sized(&mut app, &ctx, size, vec![]);
                        let nodes = output.platform_output.accesskit_update.unwrap().nodes;
                        let node = nodes
                            .iter()
                            .find(|(_, n)| {
                                n.label() == Some(title)
                                    && n.role() == egui::accesskit::Role::Button
                            })
                            .unwrap();
                        let r = node.1.bounds().unwrap();
                        let target = app
                            .saved
                            .tracks
                            .iter()
                            .find(|t| t.title == title)
                            .unwrap()
                            .source
                            .clone();
                        click_sized(
                            &mut app,
                            &ctx,
                            size,
                            pos2(((r.x0 + r.x1) / 2.0) as f32, ((r.y0 + r.y1) / 2.0) as f32),
                        );
                        assert_eq!(
                            app.requested.as_deref(),
                            Some(target.as_str()),
                            "bounds={r:?} selection={:?} message={}",
                            app.playback_selection,
                            app.message
                        );
                        assert!(app.loading);
                        assert_eq!(app.playback_selection, selection);
                        assert_eq!(
                            (app.saved.shuffle, app.saved.shuffle_albums),
                            (mode == 1, mode == 2)
                        );
                        assert_eq!(
                            app.listening_tracks().len(),
                            if filtered { 3 } else { 9 },
                            "another album stays in the listening selection"
                        );
                        assert_eq!(app.saved.mini_view, 1);
                    }
                }
            }
        }
    }
    #[test]
    fn vinyl_columns_use_added_width_and_disc_stays_between_them() {
        let mut previous_width = 0.0;
        for size in [vec2(520.0, 520.0), vec2(812.0, 640.0), vec2(1040.0, 800.0)] {
            let dir = tempfile::tempdir().unwrap();
            let ctx = egui::Context::default();
            let cc = eframe::CreationContext::_new_kittest(ctx.clone());
            let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
            app.preview("mini-vinyl");
            frame_sized(&mut app, &ctx, size, vec![]);
            let output = frame_sized(&mut app, &ctx, size, vec![]);
            let nodes = output.platform_output.accesskit_update.unwrap().nodes;
            let bounds = nodes
                .iter()
                .filter(|(_, n)| {
                    n.label() == Some("Beyond the Shore") || n.value() == Some("Beyond the Shore")
                })
                .filter_map(|(_, n)| n.bounds())
                .max_by(|a, b| (a.x1 - a.x0).total_cmp(&(b.x1 - b.x0)))
                .unwrap();
            assert!(bounds.x1 - bounds.x0 > previous_width);
            previous_width = bounds.x1 - bounds.x0;
            let layout = VinylLayout::new(egui::Rect::from_min_size(
                pos2(20.0, 275.0),
                vec2(size.x - 40.0, size.y - 291.0),
            ));
            assert_eq!(layout.disc.top(), 305.0, "artwork must stay near its tab");
            assert!(
                bounds.x1 <= f64::from(layout.disc.left() - 12.0) + 1.0,
                "row={bounds:?}, size={size:?}, column={:?}",
                layout.left
            );
            assert!(
                layout.left.right() < layout.disc.left()
                    && layout.right.left() > layout.disc.right()
            );
            assert!(layout.disc.bottom() <= size.y - 16.0);
        }
    }

    #[test]
    fn wide_miniplayer_keeps_body_centered_and_window_controls_at_the_edge() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
        app.preview("mini-vinyl");
        for size in [
            vec2(2560.0, 749.0),
            vec2(520.0, 520.0),
            vec2(1600.0, 520.0),
            vec2(812.0, 640.0),
            vec2(2560.0, 749.0),
        ] {
            frame_sized(&mut app, &ctx, size, vec![]);
            let output = frame_sized(&mut app, &ctx, size, vec![]);
            let nodes = output.platform_output.accesskit_update.unwrap().nodes;
            let bounds = |label| {
                nodes
                    .iter()
                    .find(|(_, n)| n.label() == Some(label))
                    .and_then(|(_, n)| n.bounds())
                    .unwrap()
            };
            let artwork = bounds(tr!("Album artwork"));
            let quit = bounds(tr!("Quit MusicCraft"));
            let pause = bounds(tr!("Pause"));
            assert!(artwork.x0 >= f64::from((size.x - 1280.0) * 0.5));
            assert!(quit.x1 > f64::from(size.x - 20.0));
            assert!(((pause.x0 + pause.x1) * 0.5 - f64::from(size.x * 0.5)).abs() < 1.0);
            assert!(artwork.x0 < pause.x0 && pause.x1 < quit.x0);
        }
    }
}
