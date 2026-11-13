use super::*;
use crate::facets::{Field, Sort};

impl Aurea {
    pub(super) fn capture_start_selection(&mut self) {
        // The restored listening selection is independent of the library view.
        // Explicit result clicks or a new query/filter replace it as usual.
        if self.saved.queue_label.is_some() {
            return;
        }
        if !self.restored_selection || !self.query.is_empty() || self.browser.active() {
            self.capture_selection();
        }
    }
    pub(super) fn refresh_browser(&mut self) {
        self.browser.refresh(
            &self.saved.tracks,
            self.selected_scope(),
            self.saved.active_source.clone(),
            &self.query,
            self.saved.facet_fields,
            self.library_revision,
        );
    }
    pub(super) fn capture_selection(&mut self) {
        if self.saved.queue_label.take().is_some() {
            self.shuffle_cycle.clear();
        }
        self.restored_selection = false;
        let ids: Vec<String> = if self.saved.mini {
            self.refresh_mini_index();
            self.mini_index
                .rows
                .iter()
                .map(|&i| self.saved.tracks[i].source.clone())
                .collect()
        } else {
            self.refresh_browser();
            self.browser
                .indices
                .iter()
                .map(|&i| self.saved.tracks[i].source.clone())
                .collect()
        };
        let changed = {
            let previous: HashSet<_> = self
                .listening_tracks()
                .into_iter()
                .map(|t| t.source.as_str())
                .collect();
            let next: HashSet<_> = ids.iter().map(String::as_str).collect();
            previous != next
        };
        if changed {
            self.shuffle_cycle.clear();
            if let Some(current) = self.current.as_ref().filter(|id| ids.contains(id)) {
                self.shuffle_cycle.record(current);
            }
        }
        self.playback_selection = Some(ids);
        self.history.discard_forward();
    }
    pub(super) fn play_results(&mut self, id: String) {
        self.capture_selection();
        self.play(id);
    }
    pub(super) fn library(&mut self, ui: &mut egui::Ui) {
        let short = ui.available_height() < 280.0;
        ui.horizontal_wrapped(|ui| {
            ui.add(self.icons.image("search", 16.0, palette::muted()));
            let search_width = (ui.available_width() - 410.0).max(100.0);
            ui.add_sized(
                [search_width, 30.0],
                egui::TextEdit::singleline(&mut self.query)
                    .hint_text(tr!("Search title, artist, album or folder…"))
                    .margin(vec2(9.0, 7.0)),
            );
            let active = self.saved.facets_visible;
            if ui
                .selectable_label(active, tr!("Browse"))
                .on_hover_text(tr!("Show or hide linked filters"))
                .clicked()
            {
                self.saved.facets_visible = !active;
            }
            if self
                .icons
                .button(ui, "x", tr!("Clear search and filters"), false)
                .clicked()
            {
                self.browser.clear();
                self.query.clear();
                self.restored_selection = false;
            }
            self.add_results_menu(ui);
            egui::ComboBox::from_id_salt("track-sort")
                .width(100.0)
                .selected_text(self.browser.sort.label())
                .show_ui(ui, |ui| {
                    for sort in [
                        Sort::Album,
                        Sort::Original,
                        Sort::Title,
                        Sort::Artist,
                        Sort::Duration,
                    ] {
                        ui.selectable_value(&mut self.browser.sort, sort, sort.label());
                    }
                });
        });
        self.refresh_browser();
        if self.saved.facets_visible && !self.saved.tracks.is_empty() {
            ui.add_space(4.0);
            if ui.available_height() < 350.0 {
                self.compact_facets(ui);
            } else {
                let height = (ui.available_height() * 0.48).clamp(160.0, 236.0);
                self.facet_panels(ui, height);
            }
            self.refresh_browser();
            ui.add_space(6.0);
        }
        if !short {
            ui.horizontal(|ui| {
                caption(ui, &crate::i18n::tracks(self.browser.indices.len()));
                if self.browser.active() {
                    ui.label(
                        RichText::new(tr!("Filtered selection"))
                            .small()
                            .color(palette::accent()),
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(tr!("Double-click to play"))
                            .size(11.0)
                            .color(palette::muted()),
                    );
                });
            });
            ui.add_space(2.0);
        }
        self.track_table(ui);
    }
    fn compact_facets(&mut self, ui: &mut egui::Ui) {
        ui.columns(3, |columns| {
            for (col, ui) in columns.iter_mut().enumerate() {
                ui.push_id(("compact-facet", col), |ui| {
                    let field = self.saved.facet_fields[col];
                    let selection = if self.browser.selected[col].is_empty() {
                        tr!("All").into()
                    } else {
                        format!(
                            "{} · {}",
                            tr!("Filtered selection"),
                            self.browser.selected[col].len()
                        )
                    };
                    egui::ComboBox::from_id_salt("values")
                        .width(ui.available_width())
                        .truncate()
                        .selected_text(format!("{}: {selection}", field.label()))
                        .show_ui(ui, |ui| {
                            ui.set_max_width(270.0);
                            ui.menu_button(tr!("Choose field…"), |ui| {
                                for f in Field::ALL {
                                    if ui
                                        .selectable_value(
                                            &mut self.saved.facet_fields[col],
                                            f,
                                            f.label(),
                                        )
                                        .changed()
                                    {
                                        self.browser.clear();
                                        ui.close();
                                    }
                                }
                            });
                            ui.separator();
                            if ui
                                .add(
                                    egui::TextEdit::singleline(&mut self.browser.searches[col])
                                        .hint_text(tr!("Filter values…"))
                                        .desired_width(240.0),
                                )
                                .changed()
                            {
                                self.refresh_browser();
                            }
                            if ui
                                .selectable_label(self.browser.selected[col].is_empty(), tr!("All"))
                                .clicked()
                            {
                                for n in col..3 {
                                    self.browser.selected[n].clear();
                                }
                                self.refresh_browser();
                                ui.close();
                            }
                            let entries = self.browser.entries[col].clone();
                            for e in entries {
                                if ui
                                    .selectable_label(
                                        self.browser.selected[col].contains(&e.key),
                                        format!("{}  ·  {}", e.label, e.count),
                                    )
                                    .clicked()
                                {
                                    let modifiers = ui.input(|i| i.modifiers);
                                    self.browser.select(
                                        col,
                                        &e.key,
                                        modifiers.command || modifiers.ctrl,
                                        modifiers.shift,
                                    );
                                    self.refresh_browser();
                                    if !modifiers.command && !modifiers.ctrl {
                                        ui.close();
                                    }
                                }
                            }
                        });
                });
            }
        });
    }
    fn facet_panels(&mut self, ui: &mut egui::Ui, height: f32) {
        ui.columns(3, |columns| {
            for (col, ui) in columns.iter_mut().enumerate() {
                ui.push_id(("facet", col), |ui| {
                    egui::Frame::new()
                        .fill(palette::panel())
                        .stroke(egui::Stroke::new(1.0, palette::line()))
                        .corner_radius(8.0)
                        .inner_margin(8.0)
                        .show(ui, |ui| {
                            ui.set_height(height - 16.0);
                            let old = self.saved.facet_fields[col];
                            egui::ComboBox::from_id_salt("field")
                                .width(ui.available_width())
                                .selected_text(
                                    RichText::new(old.label()).strong().color(palette::text()),
                                )
                                .show_ui(ui, |ui| {
                                    for field in Field::ALL {
                                        ui.selectable_value(
                                            &mut self.saved.facet_fields[col],
                                            field,
                                            field.label(),
                                        );
                                    }
                                })
                                .response
                                .on_hover_text(tr!("Choose this column’s field"));
                            if old != self.saved.facet_fields[col] {
                                for next in col..3 {
                                    self.browser.selected[next].clear();
                                    self.browser.searches[next].clear();
                                }
                                self.refresh_browser();
                            }
                            if height >= 145.0
                                && ui
                                    .add(
                                        egui::TextEdit::singleline(&mut self.browser.searches[col])
                                            .hint_text(tr!("Filter values…"))
                                            .desired_width(ui.available_width())
                                            .margin(vec2(6.0, 4.0)),
                                    )
                                    .changed()
                            {
                                self.refresh_browser();
                            }
                            let total = self.browser.totals[col];
                            if ui
                                .add_sized(
                                    [ui.available_width(), 24.0],
                                    egui::Button::new(
                                        RichText::new(format!("{}  ·  {total}", tr!("All")))
                                            .small(),
                                    )
                                    .selected(self.browser.selected[col].is_empty())
                                    .frame_when_inactive(false),
                                )
                                .clicked()
                            {
                                for n in col..3 {
                                    self.browser.selected[n].clear();
                                }
                                self.refresh_browser();
                            }
                            let mut selection = None;
                            let entries = &self.browser.entries[col];
                            egui::ScrollArea::vertical()
                                .id_salt("values")
                                .auto_shrink([false, false])
                                .max_height(ui.available_height().max(20.0))
                                .show_rows(ui, 32.0, entries.len(), |ui, range| {
                                    // The row owns pointer input, including clicks over its text.
                                    ui.style_mut().interaction.selectable_labels = false;
                                    for i in range {
                                        let entry = &entries[i];
                                        let active =
                                            self.browser.selected[col].contains(&entry.key);
                                        let (rect, response) = ui.allocate_exact_size(
                                            vec2(ui.available_width(), 32.0),
                                            egui::Sense::click(),
                                        );
                                        response.widget_info(|| {
                                            egui::WidgetInfo::labeled(
                                                egui::WidgetType::SelectableLabel,
                                                true,
                                                &entry.label,
                                            )
                                        });
                                        if active || response.hovered() {
                                            ui.painter().rect_filled(
                                                rect,
                                                5.0,
                                                if active {
                                                    palette::selected()
                                                } else {
                                                    palette::surface()
                                                },
                                            );
                                        }
                                        clipped(
                                            ui,
                                            egui::Rect::from_min_size(
                                                rect.min + vec2(6.0, 6.0),
                                                vec2((rect.width() - 46.0).max(24.0), 22.0),
                                            ),
                                            &entry.label,
                                            12.0,
                                            if active {
                                                palette::accent()
                                            } else {
                                                palette::text()
                                            },
                                        );
                                        ui.painter().text(
                                            rect.right_center() - vec2(6.0, 0.0),
                                            egui::Align2::RIGHT_CENTER,
                                            entry.count.to_string(),
                                            egui::FontId::monospace(10.0),
                                            palette::muted(),
                                        );
                                        if response.clicked()
                                            || response.has_focus()
                                                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                                        {
                                            selection = Some((
                                                entry.key.clone(),
                                                ui.input(|i| {
                                                    i.modifiers.command || i.modifiers.ctrl
                                                }),
                                                ui.input(|i| i.modifiers.shift),
                                            ));
                                        }
                                        response.on_hover_text(if entry.detail.is_empty() {
                                            entry.label.clone()
                                        } else {
                                            format!("{}\n{}", entry.label, entry.detail)
                                        });
                                    }
                                });
                            if let Some((key, add, range)) = selection {
                                self.browser.select(col, &key, add, range);
                                self.refresh_browser();
                            }
                        });
                });
            }
        });
    }
    fn track_table(&mut self, ui: &mut egui::Ui) {
        let wide = ui.available_width() >= 700.0;
        let width = ui.available_width();
        let text_width = width - super::track_stats::STAR_WIDTH - 12.0;
        let rating_x = width - super::track_stats::STAR_WIDTH - 78.0;
        let title_w = if wide {
            (text_width - 150.0) * 0.44
        } else {
            text_width - 118.0
        };
        let artist_x = 44.0 + title_w;
        let album_x = artist_x + (text_width - 150.0) * 0.25;
        let (header, _) = ui.allocate_exact_size(vec2(width, 22.0), egui::Sense::hover());
        for (x, w, label) in [
            (10.0, 28.0, "#"),
            (44.0, title_w, tr!("TITLE")),
            (width - 66.0, 60.0, tr!("DURATION")),
            (
                rating_x + 4.0,
                super::track_stats::STAR_WIDTH,
                tr!("Rating"),
            ),
        ] {
            clipped(
                ui,
                egui::Rect::from_min_size(header.min + vec2(x, 2.0), vec2(w, 18.0)),
                label,
                10.0,
                palette::muted(),
            );
        }
        if wide {
            clipped(
                ui,
                egui::Rect::from_min_size(
                    header.min + vec2(artist_x, 2.0),
                    vec2(album_x - artist_x - 10.0, 18.0),
                ),
                tr!("ARTIST"),
                10.0,
                palette::muted(),
            );
            clipped(
                ui,
                egui::Rect::from_min_size(
                    header.min + vec2(album_x, 2.0),
                    vec2(text_width - album_x - 84.0, 18.0),
                ),
                tr!("ALBUM"),
                10.0,
                palette::muted(),
            );
        }
        ui.painter().hline(
            header.x_range(),
            header.bottom(),
            egui::Stroke::new(1.0, palette::line()),
        );
        if self.browser.indices.is_empty() {
            ui.add_space(16.0);
            section(
                ui,
                if self.saved.tracks.is_empty() {
                    tr!("Your library, here")
                } else {
                    tr!("No matching tracks")
                },
                if self.saved.tracks.is_empty() {
                    tr!("Add a local folder, NAS or smartphone from the File menu.")
                } else {
                    tr!("Try a different search, filter or source.")
                },
            );
            if (self.browser.active() || !self.query.is_empty())
                && ui.button(tr!("Clear search and filters")).clicked()
            {
                self.query.clear();
                self.browser.clear();
            }
            return;
        }
        let mut play = None;
        let mut rating_change = None;
        let stats = &self.track_stats;
        let stats_ready = self.stats_error.is_none();
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt("library-tracks")
            .auto_shrink([false, false]);
        if std::mem::take(&mut self.browser.reset_scroll) {
            scroll = scroll.vertical_scroll_offset(0.0);
        }
        let reveal = self.library_follow.row(
            self.current.as_deref(),
            self.browser.rebuilds as u64,
            self.browser.indices.len(),
            ui.ctx().cumulative_frame_nr(),
            |source| {
                self.browser
                    .indices
                    .iter()
                    .position(|&i| self.saved.tracks[i].source == source)
            },
        );
        if let Some(row) = reveal {
            scroll = scroll
                .vertical_scroll_offset(super::follow::offset(
                    row,
                    48.0 + ui.spacing().item_spacing.y,
                    ui.available_height(),
                ))
                .animated(false);
        }
        scroll.show_rows(ui, 48.0, self.browser.indices.len(), |ui, range| {
            // The table row owns selection, double-click and keyboard focus.
            // Its child labels must not intercept clicks to select title text.
            ui.style_mut().interaction.selectable_labels = false;
            for row in range {
                let t = &self.saved.tracks[self.browser.indices[row]];
                let current = self.current.as_ref() == Some(&t.source);
                let selected = self.browser.selected_track.as_ref() == Some(&t.source);
                let (rect, space) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 48.0), egui::Sense::hover());
                let row_hit = egui::Rect::from_min_max(
                    rect.min,
                    pos2(rect.left() + rating_x - 8.0, rect.bottom()),
                );
                let response =
                    ui.interact(row_hit, space.id.with("track-title"), egui::Sense::click());
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::SelectableLabel,
                        true,
                        format!("{}, {}", t.title, t.artist),
                    )
                });
                ui.painter().rect_filled(
                    rect,
                    5.0,
                    if current {
                        palette::selected()
                    } else if selected || response.hovered() {
                        palette::surface()
                    } else if row % 2 == 0 {
                        palette::panel()
                    } else {
                        palette::bg()
                    },
                );
                let star_rect = egui::Rect::from_min_size(
                    rect.min + vec2(rating_x, 10.0),
                    vec2(super::track_stats::STAR_WIDTH, 28.0),
                );
                if let Some(rating) = super::track_stats::rating_stars(
                    ui,
                    star_rect,
                    &t.source,
                    stats.effective(t, self.saved.rating_tag_scale).rating,
                    stats_ready,
                ) {
                    rating_change = Some((t.source.clone(), rating));
                }
                if selected && !current {
                    ui.painter().rect_stroke(
                        rect.shrink(0.5),
                        5.0,
                        egui::Stroke::new(1.0, palette::line()),
                        egui::StrokeKind::Inside,
                    );
                }
                if current {
                    ui.painter().rect_filled(
                        egui::Rect::from_min_size(rect.min + vec2(0.0, 10.0), vec2(2.0, 28.0)),
                        1.0,
                        palette::accent(),
                    );
                }
                if current {
                    let indicator =
                        egui::Rect::from_center_size(rect.min + vec2(25.0, 24.0), vec2(18.0, 20.0));
                    playing_indicator(
                        ui.painter(),
                        indicator,
                        self.meters,
                        self.paused,
                        self.saved.reduce_motion,
                    );
                    let label = if self.paused {
                        tr!("Pause")
                    } else {
                        tr!("Playing")
                    };
                    let marker =
                        ui.interact(indicator, response.id.with("playing"), egui::Sense::hover());
                    marker.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Image, true, label)
                    });
                    marker.on_hover_text(label);
                } else {
                    ui.painter().text(
                        rect.min + vec2(12.0, 24.0),
                        egui::Align2::LEFT_CENTER,
                        format!(
                            "{:02}",
                            if t.number > 0 {
                                t.number as usize
                            } else {
                                row + 1
                            }
                        ),
                        egui::FontId::monospace(10.0),
                        palette::muted(),
                    );
                }
                clipped(
                    ui,
                    egui::Rect::from_min_size(
                        rect.min + vec2(44.0, 6.0),
                        vec2(title_w - 10.0, 20.0),
                    ),
                    &t.title,
                    13.0,
                    if current {
                        palette::accent()
                    } else {
                        palette::text()
                    },
                );
                let detail = if wide {
                    t.quality()
                } else if t.artist.is_empty() {
                    format!("{} · {}", t.codec, tr!("Tags unavailable"))
                } else {
                    format!("{} · {}", t.artist, t.album)
                };
                clipped(
                    ui,
                    egui::Rect::from_min_size(
                        rect.min + vec2(44.0, 27.0),
                        vec2(title_w - 10.0, 17.0),
                    ),
                    &detail,
                    10.5,
                    palette::muted(),
                );
                if wide {
                    clipped(
                        ui,
                        egui::Rect::from_min_size(
                            rect.min + vec2(artist_x, 14.0),
                            vec2(album_x - artist_x - 12.0, 22.0),
                        ),
                        if t.artist.is_empty() {
                            "—"
                        } else {
                            &t.artist
                        },
                        12.0,
                        palette::muted(),
                    );
                    clipped(
                        ui,
                        egui::Rect::from_min_size(
                            rect.min + vec2(album_x, 14.0),
                            vec2(text_width - album_x - 84.0, 22.0),
                        ),
                        if t.album.is_empty() { "—" } else { &t.album },
                        12.0,
                        palette::muted(),
                    );
                }
                ui.painter().text(
                    rect.right_center() - vec2(12.0, 0.0),
                    egui::Align2::RIGHT_CENTER,
                    if t.duration > 0.0 {
                        time(t.duration)
                    } else {
                        "—:—".into()
                    },
                    egui::FontId::monospace(11.0),
                    palette::muted(),
                );
                if response.clicked() {
                    response.request_focus();
                    self.browser.selected_track = Some(t.source.clone());
                }
                if response.double_clicked()
                    || response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
                {
                    play = Some(t.source.clone());
                }
                response.context_menu(|ui| {
                    super::track_stats::tag_rating_info(ui, t, self.saved.rating_tag_scale);
                    ui.add_enabled_ui(stats_ready, |ui| {
                        if let Some(rating) = super::track_stats::rating_menu(
                            ui,
                            stats.effective(t, self.saved.rating_tag_scale),
                        ) {
                            rating_change = Some((t.source.clone(), rating));
                        }
                    });
                    ui.separator();
                    if ui.button(tr!("Play from here")).clicked() {
                        play = Some(t.source.clone());
                        ui.close();
                    }
                });
            }
        });
        if let Some((source, rating)) = rating_change {
            self.rate_track(&source, rating);
        }
        if let Some(id) = play {
            self.play_results(id);
        }
    }
}

/// The same -60..0 dB peak range as the mini player's stereo meters.
fn peak_fraction(level: f32) -> f32 {
    ((20.0 * level.max(0.000001).log10() + 60.0) / 60.0).clamp(0.0, 1.0)
}

fn playing_indicator(
    p: &egui::Painter,
    rect: egui::Rect,
    levels: [f32; 2],
    paused: bool,
    reduce_motion: bool,
) {
    if reduce_motion && !paused {
        let center = rect.center();
        p.add(egui::Shape::convex_polygon(
            vec![
                center + vec2(-4.0, -6.0),
                center + vec2(5.0, 0.0),
                center + vec2(-4.0, 6.0),
            ],
            palette::accent(),
            egui::Stroke::NONE,
        ));
        return;
    }
    for (channel, level) in levels.into_iter().enumerate() {
        let x = rect.center().x - 5.0 + channel as f32 * 7.0;
        let rail = egui::Rect::from_min_size(pos2(x, rect.bottom() - 18.0), vec2(3.0, 18.0));
        if paused {
            p.rect_filled(rail.shrink2(vec2(0.0, 3.0)), 1.0, palette::muted());
        } else {
            p.rect_filled(rail, 1.0, palette::line());
            let fill = egui::Rect::from_min_max(
                pos2(
                    rail.left(),
                    rail.bottom() - rail.height() * peak_fraction(level),
                ),
                rail.max,
            );
            p.rect_filled(fill, 1.0, palette::accent());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playback_indicator_preserves_stereo_dynamics_and_handles_invalid_peaks() {
        assert_eq!(peak_fraction(0.0), 0.0);
        assert_eq!(peak_fraction(f32::NAN), 0.0);
        assert_eq!(peak_fraction(1.0), 1.0);
        assert_eq!(peak_fraction(f32::INFINITY), 1.0);
        assert!(peak_fraction(0.1) < peak_fraction(0.8));
        assert!(peak_fraction(0.001) < peak_fraction(0.01));
    }

    #[test]
    fn stereo_indicator_swaps_channels_and_pause_ignores_the_peaks() {
        fn paint(levels: [f32; 2], paused: bool) -> Vec<(egui::Rect, Color32)> {
            let ctx = egui::Context::default();
            let mut output = ctx.run_ui(Default::default(), |ui| {
                playing_indicator(
                    ui.painter(),
                    egui::Rect::from_min_size(pos2(10.0, 10.0), vec2(18.0, 20.0)),
                    levels,
                    paused,
                    false,
                );
            });
            output.textures_delta.clear();
            output
                .shapes
                .into_iter()
                .filter_map(|s| match s.shape {
                    egui::Shape::Rect(r) if r.rect.width() == 3.0 => Some((r.rect, r.fill)),
                    _ => None,
                })
                .collect()
        }
        let a = paint([0.1, 0.8], false);
        let b = paint([0.8, 0.1], false);
        let heights = |rows: &[(egui::Rect, Color32)]| {
            rows.iter()
                .filter(|(_, color)| *color == palette::accent())
                .map(|(r, _)| r.height())
                .collect::<Vec<_>>()
        };
        let (a, b) = (heights(&a), heights(&b));
        assert_eq!(a.len(), 2);
        assert!(a[0] < a[1]);
        assert_eq!(a, [b[1], b[0]]);
        assert_eq!(paint([0.0, 1.0], true), paint([1.0, 0.0], true));
    }

    #[test]
    fn track_text_click_selects_the_row_and_double_click_or_enter_plays_it() {
        for size in [vec2(640.0, 300.0), vec2(1120.0, 400.0)] {
            for column in if size.x >= 700.0 { 0..3 } else { 0..1 } {
                let dir = tempfile::tempdir().unwrap();
                let ctx = egui::Context::default();
                let cc = eframe::CreationContext::_new_kittest(ctx.clone());
                let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
                let (tx, commands) = mpsc::channel();
                app.audio.tx = tx;
                app.saved.auto_sync = false;
                app.online_covers.store(false, Ordering::Relaxed);
                app.saved.tracks = (1..=2)
                    .map(|n| {
                        let mut t = Track::placeholder(format!("/synthetic/{n}.flac"));
                        t.title = format!("Track title {n}");
                        t.artist = "Test artist".into();
                        t.album = "Test album".into();
                        t
                    })
                    .collect();
                let target = app.saved.tracks[0].source.clone();
                let mut tick = 0;
                let frame = |app: &mut Aurea, tick: &mut u32, events| {
                    *tick += 1;
                    app.refresh_browser();
                    let mut output = ctx.run_ui(
                        egui::RawInput {
                            time: Some(*tick as f64 * 0.03),
                            screen_rect: Some(egui::Rect::from_min_size(pos2(0.0, 0.0), size)),
                            events,
                            ..Default::default()
                        },
                        |ui| app.track_table(ui),
                    );
                    output.textures_delta.clear();
                    output
                };
                frame(&mut app, &mut tick, vec![]);
                let output = frame(&mut app, &mut tick, vec![]);
                let nodes = output.platform_output.accesskit_update.unwrap().nodes;
                let text = ["Track title 1", "Test artist", "Test album"][column];
                let row = nodes
                    .iter()
                    .find(|(_, n)| n.label() == Some("Track title 1, Test artist"))
                    .unwrap()
                    .1
                    .bounds()
                    .unwrap();
                let bounds = nodes
                    .iter()
                    .find(|(_, n)| {
                        n.value() == Some(text)
                            && n.bounds().is_some_and(|b| b.y0 >= row.y0 && b.y1 <= row.y1)
                    })
                    .unwrap()
                    .1
                    .bounds()
                    .unwrap();
                // A point on the glyphs, not the empty padding of the row.
                let pos = pos2(
                    (bounds.x0 + 3.0) as f32,
                    ((bounds.y0 + bounds.y1) / 2.0) as f32,
                );
                frame(&mut app, &mut tick, vec![egui::Event::PointerMoved(pos)]);
                for pressed in [true, false] {
                    frame(
                        &mut app,
                        &mut tick,
                        vec![egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        }],
                    );
                }
                assert_eq!(
                    app.browser.selected_track.as_ref(),
                    Some(&target),
                    "column={column}"
                );
                assert!(
                    commands.try_recv().is_err(),
                    "one click selects without playing"
                );
                for pressed in [true, false] {
                    frame(
                        &mut app,
                        &mut tick,
                        vec![egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        }],
                    );
                }
                assert!(
                    matches!(commands.try_recv(), Ok(Command::Load{track,..}) if track.source==target)
                );
                assert_eq!(app.playback_selection.as_ref().unwrap().len(), 2);
                // Enter uses the same focused row action, retaining the results.
                app.loading = false;
                frame(
                    &mut app,
                    &mut tick,
                    vec![egui::Event::Key {
                        key: egui::Key::Enter,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                );
                assert!(
                    matches!(commands.try_recv(), Ok(Command::Load{track,..}) if track.source==target)
                );
            }
        }
    }

    #[test]
    fn facet_rows_select_when_clicked_on_their_text_or_padding() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
        app.saved.facet_fields = [Field::Artist, Field::Album, Field::Year];
        app.saved.tracks = [
            ("Alpha", "First", 2020),
            ("Beta", "Second", 2021),
            ("Gamma", "Third", 2022),
        ]
        .into_iter()
        .map(|(artist, album, year)| {
            let mut track = Track::placeholder(format!("/synthetic/{artist}/track.flac"));
            track.artist = artist.into();
            track.album = album.into();
            track.year = Some(year);
            track
        })
        .collect();

        let frame = |app: &mut Aurea, events| {
            app.refresh_browser();
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        pos2(0.0, 0.0),
                        vec2(960.0, 380.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| app.facet_panels(ui, 300.0),
            );
            output.textures_delta.clear();
            output
        };

        for (column, label) in [(0, "Alpha"), (1, "First"), (2, "2020")] {
            for on_text in [true, false] {
                app.browser.clear();
                frame(&mut app, vec![]);
                let output = frame(&mut app, vec![]);
                let nodes = output.platform_output.accesskit_update.unwrap().nodes;
                let text = nodes
                    .iter()
                    .find(|(_, node)| node.value() == Some(label))
                    .unwrap()
                    .1
                    .bounds()
                    .unwrap();
                let row = nodes
                    .iter()
                    .find(|(_, node)| node.label() == Some(label))
                    .unwrap()
                    .1
                    .bounds()
                    .unwrap();
                let pos = if on_text {
                    pos2((text.x0 + 3.0) as f32, ((text.y0 + text.y1) / 2.0) as f32)
                } else {
                    pos2((row.x1 - 3.0) as f32, ((row.y0 + row.y1) / 2.0) as f32)
                };
                frame(&mut app, vec![egui::Event::PointerMoved(pos)]);
                for pressed in [true, false] {
                    frame(
                        &mut app,
                        vec![egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        }],
                    );
                }
                assert_eq!(
                    app.browser.selected[column].len(),
                    1,
                    "column={column}, text={on_text}"
                );
                assert_eq!(app.browser.indices.len(), 1);
                assert_eq!(app.saved.tracks[app.browser.indices[0]].artist, "Alpha");
            }
        }
    }
}
