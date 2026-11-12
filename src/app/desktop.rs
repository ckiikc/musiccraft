use super::*;

impl Aurea {
    pub(super) fn pick_music(&mut self, folder: bool) {
        if folder {
            let dialog = rfd::FileDialog::new();
            #[cfg(target_os = "macos")]
            let dialog = dialog.set_directory("/Volumes");
            if let Some(path) = dialog.pick_folder() {
                self.import(vec![path]);
            }
        } else if let Some(paths) = rfd::FileDialog::new()
            .add_filter(
                tr!("Audio and playlists"),
                &[
                    "flac", "wav", "aiff", "aif", "m4a", "mp3", "aac", "ogg", "m3u", "m3u8",
                    "alac", "oga", "mp4", "dsf", "dff", "dsd", "ape", "wma", "asf", "wv", "opus",
                ],
            )
            .pick_files()
        {
            self.import(paths);
        }
    }
    fn show_mini(&mut self) {
        self.saved.mini = true;
    }
    fn desktop_menu(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.label(
                RichText::new("MUSICCRAFT")
                    .strong()
                    .color(palette::accent()),
            );
            ui.separator();
            ui.menu_button(tr!("File"), |ui| {
                if ui.button(tr!("Add files or playlist…")).clicked() {
                    ui.close();
                    self.pick_music(false);
                }
                if ui.button(tr!("Add local / NAS folder…")).clicked() {
                    ui.close();
                    self.pick_music(true);
                }
                if ui.button(tr!("Connect smartphone / FTP…")).clicked() {
                    ui.close();
                    self.open_network();
                }
                ui.separator();
                if ui.button(tr!("Quit MusicCraft")).clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button(tr!("Library"), |ui| {
                self.auto_sync_control(ui);
                ui.separator();
                if ui.button(tr!("Sources and synchronization…")).clicked() {
                    self.sources_dialog = true;
                    ui.close();
                }
                if ui
                    .add_enabled(
                        self.scanning,
                        egui::Button::new(tr!("Cancel synchronization")),
                    )
                    .clicked()
                {
                    self.cancel.store(true, Ordering::Relaxed);
                    ui.close();
                }
                if ui
                    .checkbox(&mut self.saved.facets_visible, tr!("Faceted browser"))
                    .changed()
                {
                    ui.close();
                }
                if ui.button(tr!("Clear search and filters")).clicked() {
                    self.query.clear();
                    self.browser.clear();
                    ui.close();
                }
            });
            ui.menu_button(tr!("Playback"), |ui| {
                if ui
                    .add_enabled(
                        self.can_toggle(),
                        egui::Button::new(if self.paused {
                            tr!("Play")
                        } else {
                            tr!("Pause")
                        }),
                    )
                    .clicked()
                {
                    self.toggle();
                    ui.close();
                }
                if ui
                    .add_enabled(
                        self.navigation_ready(),
                        egui::Button::new(tr!("Previous track")),
                    )
                    .clicked()
                {
                    if let Some(id) = self.history.previous() {
                        self.play(id);
                    }
                    ui.close();
                }
                if ui
                    .add_enabled(
                        self.navigation_ready(),
                        egui::Button::new(tr!("Next track")),
                    )
                    .clicked()
                {
                    self.next();
                    ui.close();
                }
                if ui.button(tr!("Stop")).clicked() {
                    self.stop();
                    ui.close();
                }
                ui.separator();
                ui.horizontal(|ui| {
                    self.order_selector(ui, palette::accent());
                    ui.label(self.order_label());
                });
                if ui
                    .checkbox(&mut self.saved.repeat, tr!("Repeat track"))
                    .changed()
                {
                    self.prepare_next();
                }
                if ui.button(tr!("Track details…")).clicked() {
                    self.track_dialog = true;
                    ui.close();
                }
                if ui.button(tr!("Artist and lyrics…")).clicked() {
                    self.info_dialog = true;
                    self.last_lyric_line = None;
                    ui.close();
                }
            });
            ui.menu_button(tr!("Settings"), |ui| {
                if ui.button(tr!("All settings…")).clicked() {
                    self.open_settings(0);
                    ui.close();
                }
                if ui.button(tr!("Server and multiroom…")).clicked() {
                    self.open_settings(3);
                    ui.close();
                }
                ui.separator();
                self.online_cover_control(ui);
                self.cover_save_control(ui);
                if ui.button(tr!("Audio output…")).clicked() {
                    self.audio_dialog = true;
                    ui.close();
                }
                if ui
                    .checkbox(&mut self.saved.pinned, tr!("Always on top"))
                    .changed()
                {
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                            if self.saved.pinned {
                                egui::WindowLevel::AlwaysOnTop
                            } else {
                                egui::WindowLevel::Normal
                            },
                        ));
                }
                if ui.button(tr!("Mini player")).clicked() {
                    self.show_mini();
                    ui.close();
                }
            });
            ui.menu_button(tr!("Help"), |ui| {
                #[cfg(not(target_os = "macos"))]
                if ui.button(tr!("About MusicCraft")).clicked() {
                    crate::about::show(ui.ctx());
                    ui.close();
                }
                if ui.button(tr!("User guide")).clicked() {
                    self.help_dialog = true;
                    ui.close();
                }
            });
        });
    }
    pub(super) fn desktop_transport(&mut self, ui: &mut egui::Ui) {
        let area = ui.max_rect();
        let width = area.width();
        let wide = width >= 940.0;
        let r = |x: f32, y: f32, w: f32, h: f32| {
            egui::Rect::from_min_size(area.min + vec2(x, y), vec2(w, h))
        };
        let track = self.displayed_track().cloned();
        let cover_size = if wide { 54.0 } else { 40.0 };
        let cover = r(0.0, if wide { 17.0 } else { 0.0 }, cover_size, cover_size);
        if let Some(art) = &self.art {
            ui.put(
                cover,
                egui::Image::new(art)
                    .fit_to_exact_size(cover.size())
                    .corner_radius(7.0),
            );
        } else {
            ui.painter().rect_filled(cover, 7.0, palette::surface());
            ui.put(
                cover.shrink(10.0),
                self.icons
                    .image("disc-3", cover_size - 20.0, palette::muted()),
            );
        }
        let cover_response = ui.interact(cover, ui.id().with("open-artwork"), egui::Sense::click());
        cover_response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tr!("Album artwork"))
        });
        if cover_viewer::requested(&cover_response) {
            self.open_cover_viewer();
        }
        cover_response.on_hover_text(tr!("Double-click to enlarge artwork"));
        let metadata_width = if wide {
            (width * 0.26).min(300.0) - cover_size - 12.0
        } else {
            width - 238.0 - cover_size - 12.0
        };
        let y = if wide { 20.0 } else { 0.0 };
        clipped(
            ui,
            r(cover_size + 12.0, y, metadata_width, 22.0),
            track
                .as_ref()
                .map_or(tr!("Choose your next listen"), |t| t.title.as_str()),
            13.0,
            palette::text(),
        );
        let detail = track
            .as_ref()
            .map_or(tr!("Your library awaits").into(), |t| {
                if t.artist.is_empty() {
                    t.quality()
                } else {
                    t.artist.clone()
                }
            });
        clipped(
            ui,
            r(cover_size + 12.0, y + 23.0, metadata_width, 18.0),
            if self.loading {
                tr!("Loading…")
            } else {
                &detail
            },
            11.0,
            palette::muted(),
        );
        if wide && let Some(t) = &track {
            clipped(
                ui,
                r(cover_size + 12.0, y + 43.0, metadata_width, 18.0),
                &t.quality(),
                10.0,
                palette::accent(),
            );
        }
        let (x, w, cy, sy) = if wide {
            let x = (width * 0.29).max(270.0);
            (x, (width - x - 230.0).max(320.0), 2.0, 60.0)
        } else {
            (0.0, width, 48.0, 100.0)
        };
        let center = x + w / 2.0;
        // Stable groups: order and stop, previous/play/next, repeat.
        let bx = center - 22.0;
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(r(bx - 126.0, cy + 7.0, 30.0, 30.0)),
            |ui| {
                ui.spacing_mut().button_padding = vec2(7.0, 5.0);
                self.order_selector(ui, palette::accent());
            },
        );
        if self
            .icons
            .at(
                ui,
                r(bx - 88.0, cy + 7.0, 30.0, 30.0),
                "square",
                tr!("Stop"),
                false,
                false,
                true,
            )
            .clicked()
        {
            self.stop();
        }
        if self
            .icons
            .at(
                ui,
                r(bx - 44.0, cy + 5.0, 34.0, 34.0),
                "skip-back",
                tr!("Previous"),
                false,
                false,
                self.navigation_ready(),
            )
            .clicked()
        {
            if let Some(id) = self.history.previous() {
                self.play(id);
            } else {
                let _ = self.audio.tx.send(Command::Seek(0.0));
            }
        }
        if self
            .icons
            .at(
                ui,
                r(bx, cy, 44.0, 44.0),
                if self.paused { "play" } else { "pause" },
                if self.paused {
                    tr!("Play")
                } else {
                    tr!("Pause")
                },
                false,
                true,
                self.can_toggle(),
            )
            .clicked()
        {
            self.toggle();
        }
        if self
            .icons
            .at(
                ui,
                r(bx + 52.0, cy + 5.0, 34.0, 34.0),
                "skip-forward",
                tr!("Next"),
                false,
                false,
                self.navigation_ready(),
            )
            .clicked()
        {
            self.next();
        }
        if self
            .icons
            .at(
                ui,
                r(bx + 96.0, cy + 7.0, 30.0, 30.0),
                "repeat-1",
                tr!("Repeat track"),
                self.saved.repeat,
                false,
                true,
            )
            .clicked()
        {
            self.saved.repeat = !self.saved.repeat;
            self.prepare_next();
        }
        let duration = track.as_ref().map_or(0.0, |t| t.duration);
        ui.painter().text(
            r(x, sy, w, 22.0).left_center(),
            egui::Align2::LEFT_CENTER,
            time(self.position),
            egui::FontId::monospace(11.0),
            palette::muted(),
        );
        ui.painter().text(
            r(x, sy, w, 22.0).right_center(),
            egui::Align2::RIGHT_CENTER,
            if duration > 0.0 {
                time(duration)
            } else {
                "—:—".into()
            },
            egui::FontId::monospace(11.0),
            palette::muted(),
        );
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(r(x + 42.0, sy, w - 84.0, 22.0)),
            |ui| {
                ui.spacing_mut().slider_width = w - 84.0;
                ui.spacing_mut().interact_size.y = 22.0;
                ui.spacing_mut().slider_rail_height = 3.0;
                let response = ui.add_enabled(
                    duration > 0.0 && !self.loading,
                    egui::Slider::new(&mut self.position, 0.0..=duration.max(0.01))
                        .show_value(false)
                        .handle_shape(egui::style::HandleShape::Circle),
                );
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Slider,
                        duration > 0.0,
                        tr!("Playback position"),
                    )
                });
                if response.drag_stopped() || response.clicked() {
                    let _ = self.audio.tx.send(Command::Seek(self.position));
                }
            },
        );
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(r(
                width - 208.0,
                if wide { 10.0 } else { 0.0 },
                208.0,
                72.0,
            )),
            |ui| self.desktop_volume(ui),
        );
    }
    fn desktop_volume(&mut self, ui: &mut egui::Ui) {
        let exclusive = self.exclusive_active();
        ui.horizontal(|ui| {
            ui.add_enabled_ui(!exclusive, |ui| {
                if self
                    .icons
                    .button(
                        ui,
                        if self.mute { "volume-x" } else { "volume-2" },
                        tr!("Mute"),
                        self.mute,
                    )
                    .clicked()
                {
                    self.mute = !self.mute;
                    self.gain();
                }
                let response = super::widgets::volume_slider(
                    ui,
                    "desktop-volume",
                    &mut self.volume_db,
                    vec2(96.0, 24.0),
                );
                if response.changed() {
                    self.gain();
                }
            });
            ui.label(
                RichText::new(if exclusive {
                    "HW".into()
                } else {
                    format!("{:.1} dB", self.volume_db)
                })
                .monospace()
                .size(11.0)
                .color(palette::muted()),
            );
        });
        let device = if self.saved.device.is_empty() {
            tr!("System output")
        } else {
            &self.saved.device
        };
        let text = if exclusive {
            format!("{} · {device}", tr!("Exclusive"))
        } else {
            device.to_owned()
        };
        let response = ui.add_sized(
            [ui.available_width(), 24.0],
            egui::Button::new(RichText::new(text).size(11.5).color(palette::muted()))
                .frame(false)
                .wrap_mode(egui::TextWrapMode::Truncate),
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tr!("Audio"))
        });
        if response
            .on_hover_text(if exclusive {
                tr!("Hardware volume · Open audio output")
            } else {
                tr!("Configure audio output and exclusive mode")
            })
            .clicked()
        {
            self.audio_dialog = true;
        }
    }

    pub(super) fn desktop_ui(&mut self, root: &mut egui::Ui) {
        root.painter()
            .rect_filled(root.max_rect(), 0.0, palette::bg());
        egui::Panel::top("main-menu")
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(palette::panel())
                    .inner_margin(egui::Margin::symmetric(12, 7)),
            )
            .show(root, |ui| self.desktop_menu(ui));
        egui::Panel::bottom("main-status")
            .resizable(false)
            .exact_size(28.0)
            .frame(
                egui::Frame::new()
                    .fill(palette::bg())
                    .inner_margin(egui::Margin::symmetric(14, 4)),
            )
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    let width = (ui.available_width() * 0.55).clamp(210.0, 430.0);
                    let (rect, _) = ui.allocate_exact_size(vec2(width, 20.0), egui::Sense::hover());
                    self.tag_progress_indicator(ui, rect, false);
                    if self.scanning {
                        ui.add(egui::Spinner::new().size(14.0));
                    }
                    let text = if self.message.is_empty() {
                        format!(
                            "{}  ·  {}",
                            crate::i18n::tracks(self.scoped_tracks().count()),
                            if self.loading {
                                tr!("Loading…")
                            } else if self.paused {
                                tr!("Ready")
                            } else {
                                tr!("Playing")
                            }
                        )
                    } else {
                        self.playback_message()
                    };
                    ui.add(
                        egui::Label::new(RichText::new(&text).size(11.0).color(palette::muted()))
                            .truncate(),
                    )
                    .on_hover_text(text);
                });
            });
        egui::Panel::bottom("main-transport")
            .resizable(false)
            .exact_size(if root.max_rect().width() < 976.0 {
                150.0
            } else {
                116.0
            })
            .frame(
                egui::Frame::new()
                    .fill(palette::panel())
                    .inner_margin(egui::Margin::symmetric(18, 12)),
            )
            .show(root, |ui| self.desktop_transport(ui));
        if root.max_rect().width() >= 960.0 {
            egui::Panel::left("main-sources")
                .default_size(220.0)
                .size_range(210.0..=280.0)
                .resizable(true)
                .frame(egui::Frame::new().fill(palette::bg()).inner_margin(14.0))
                .show(root, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("sources-scroll")
                        .show(ui, |ui| self.sidebar(ui));
                });
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(palette::bg()).inner_margin(
                if root.max_rect().height() < 620.0 {
                    12.0
                } else {
                    18.0
                },
            ))
            .show(root, |ui| {
                let short = ui.max_rect().height() < 360.0;
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width((ui.available_width() - 128.0).max(160.0));
                        let title = if short {
                            format!(
                                "{} · {}",
                                self.source_title(),
                                crate::i18n::tracks(self.scoped_tracks().count())
                            )
                        } else {
                            self.source_title()
                        };
                        ui.add(
                            egui::Label::new(
                                RichText::new(title)
                                    .size(if short { 17.0 } else { 22.0 })
                                    .strong()
                                    .color(palette::text()),
                            )
                            .truncate(),
                        );
                        if !short {
                            ui.label(
                                RichText::new(crate::i18n::tracks(self.scoped_tracks().count()))
                                    .size(11.0)
                                    .color(palette::muted()),
                            );
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self
                            .icons
                            .button(ui, "arrow-down-left", tr!("Mini player"), false)
                            .clicked()
                        {
                            self.show_mini();
                        }
                        if self
                            .icons
                            .button(ui, "headphones", tr!("Audio"), false)
                            .clicked()
                        {
                            self.audio_dialog = true;
                        }
                        if self
                            .icons
                            .button(ui, "sliders-horizontal", tr!("Sources"), false)
                            .clicked()
                        {
                            self.sources_dialog = true;
                        }
                    });
                });
                ui.add_space(if short { 2.0 } else { 10.0 });
                self.library(ui);
            });
    }
    pub(super) fn auxiliary_windows(&mut self, ctx: &egui::Context) {
        if self.info_dialog {
            self.info_dialog = self.detached_window(
                ctx,
                "artist-lyrics-window",
                tr!("Artist and lyrics"),
                vec2(700.0, 560.0),
                |app, ui| {
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut app.info_lyrics, false, tr!("Artist"));
                        if ui
                            .selectable_value(&mut app.info_lyrics, true, tr!("Lyrics"))
                            .clicked()
                        {
                            app.last_lyric_line = None;
                        }
                    });
                    ui.separator();
                    app.listening_content(
                        ui,
                        app.info_lyrics,
                        palette::text(),
                        palette::muted(),
                        palette::accent(),
                    );
                },
            );
        } else {
            self.auxiliary_geometry.close("artist-lyrics-window");
        }
        if self.audio_dialog {
            self.audio_dialog = self.detached_window(
                ctx,
                "audio-settings-v06",
                tr!("Audio output and quality"),
                vec2(600.0, 700.0),
                |app, ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("audio-content")
                        .show(ui, |ui| app.signal(ui));
                },
            );
        } else {
            self.auxiliary_geometry.close("audio-settings-v06");
        }
        if self.sources_dialog {
            let open = self.detached_window(
                ctx,
                "source-settings-v06",
                tr!("Sources and synchronization"),
                vec2(500.0, 680.0),
                |app, ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("sources-content")
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(tr!(
                                    "Find new tracks without changing existing ones."
                                ))
                                .color(palette::muted()),
                            );
                            ui.add_space(10.0);
                            app.sidebar(ui);
                        });
                },
            );
            self.sources_dialog = open && !self.ftp_dialog;
        } else {
            self.auxiliary_geometry.close("source-settings-v06");
        }
        if self.track_dialog {
            self.track_dialog = self.detached_window(
                ctx,
                "track-details-v06",
                tr!("Playing"),
                vec2(650.0, 680.0),
                |app, ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("track-content")
                        .show(ui, |ui| {
                            app.now_playing(ui, true);
                            if let Some((provider, url)) = &app.art_origin {
                                ui.hyperlink_to(
                                    crate::i18n::format(
                                        "Cover · {provider}",
                                        &[("provider", provider.clone())],
                                    ),
                                    url,
                                );
                            }
                        });
                },
            );
        } else {
            self.auxiliary_geometry.close("track-details-v06");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(
        app: &mut Aurea,
        ctx: &egui::Context,
        size: egui::Vec2,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(pos2(0.0, 0.0), size)),
                events,
                ..Default::default()
            },
            |ui| {
                app.desktop_ui(ui);
                app.auxiliary_windows(ctx);
                app.ftp_window(ctx);
            },
        );
        output.textures_delta.clear();
        output
    }
    #[test]
    fn responsive_layout_keeps_transport_and_settings_visible_and_opens_audio_window() {
        for size in [vec2(640.0, 440.0), vec2(760.0, 520.0), vec2(1120.0, 760.0)] {
            let dir = tempfile::tempdir().unwrap();
            let ctx = egui::Context::default();
            let cc = eframe::CreationContext::_new_kittest(ctx.clone());
            let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
            app.saved.mini = false;
            app.preview("library");
            let mut out = frame(&mut app, &ctx, size, vec![]);
            out.textures_delta.clear();
            let update = out
                .platform_output
                .accesskit_update
                .expect("accessibility controls available");
            let mut audio = None;
            for name in [
                "File",
                "Sorgenti",
                "Audio",
                "Riproduci",
                "Successivo",
                "Volume",
            ] {
                let node = update
                    .nodes
                    .iter()
                    .find(|(_, n)| n.label() == Some(name))
                    .unwrap_or_else(|| panic!("control missing: {name}"));
                let bounds = node.1.bounds().expect("control rectangle");
                assert!(
                    bounds.x0 >= 0.0
                        && bounds.y0 >= 0.0
                        && bounds.x1 <= f64::from(size.x)
                        && bounds.y1 <= f64::from(size.y),
                    "{name} outside {size:?}: {bounds:?}"
                );
                if name == "Audio" {
                    audio = Some(pos2(
                        ((bounds.x0 + bounds.x1) / 2.0) as f32,
                        ((bounds.y0 + bounds.y1) / 2.0) as f32,
                    ));
                }
            }
            let pos = audio.unwrap();
            frame(&mut app, &ctx, size, vec![egui::Event::PointerMoved(pos)]);
            for pressed in [true, false] {
                frame(
                    &mut app,
                    &ctx,
                    size,
                    vec![egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    }],
                );
            }
            assert!(app.audio_dialog, "Audio opens at {size:?}");
            assert!(app.current.is_none());
        }
    }
    fn click_label(app: &mut Aurea, ctx: &egui::Context, size: egui::Vec2, label: &str) {
        let out = frame(app, ctx, size, vec![]);
        let update = out.platform_output.accesskit_update.unwrap();
        let bounds = update
            .nodes
            .iter()
            .find(|(_, n)| n.label() == Some(label))
            .unwrap_or_else(|| panic!("Missing {label}"))
            .1
            .bounds()
            .unwrap();
        let pos = pos2(
            ((bounds.x0 + bounds.x1) / 2.0) as f32,
            ((bounds.y0 + bounds.y1) / 2.0) as f32,
        );
        frame(app, ctx, size, vec![egui::Event::PointerMoved(pos)]);
        for pressed in [true, false] {
            frame(
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
    }
    #[test]
    fn order_icon_cycles_all_modes_without_starting_playback() {
        for size in [vec2(640.0, 440.0), vec2(1120.0, 760.0)] {
            let dir = tempfile::tempdir().unwrap();
            let ctx = egui::Context::default();
            let cc = eframe::CreationContext::_new_kittest(ctx.clone());
            let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
            app.preview("library");
            let ticket = app.ticket;
            for (label, expected) in [
                ("Sequenza", (true, false)),
                ("Casuale brani", (false, true)),
                ("Casuale album", (false, false)),
            ] {
                click_label(
                    &mut app,
                    &ctx,
                    size,
                    &crate::i18n::format("Playback order: {mode}", &[("mode", label.to_string())]),
                );
                assert_eq!((app.saved.shuffle, app.saved.shuffle_albums), expected);
                assert!(app.current.is_none());
                assert!(app.requested.is_none());
                assert!(app.planned.is_none());
                assert_eq!(app.ticket, ticket);
                let restored: Saved =
                    serde_json::from_value(serde_json::to_value(&app.saved).unwrap()).unwrap();
                assert_eq!((restored.shuffle, restored.shuffle_albums), expected);
            }
        }
    }
    #[test]
    fn populated_library_has_visible_rows_and_unclipped_controls_at_all_sizes() {
        for size in [
            vec2(640.0, 440.0),
            vec2(760.0, 520.0),
            vec2(960.0, 600.0),
            vec2(1120.0, 760.0),
            vec2(1440.0, 900.0),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let ctx = egui::Context::default();
            let cc = eframe::CreationContext::_new_kittest(ctx.clone());
            let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
            app.preview("library");
            let out = frame(&mut app, &ctx, size, vec![]);
            let update = out.platform_output.accesskit_update.unwrap();
            for label in [
                "Azzera ricerca e filtri",
                "Ripeti brano",
                "Stop",
                "Successivo",
                "Audio",
                "Opening Light, Aurea Ensemble",
            ] {
                let (_, node) = update
                    .nodes
                    .iter()
                    .find(|(_, n)| n.label() == Some(label))
                    .unwrap_or_else(|| panic!("Missing {label} at {size:?}"));
                let b = node.bounds().unwrap();
                assert!(
                    b.x0 >= 0.0
                        && b.x1 <= f64::from(size.x)
                        && b.y0 >= 0.0
                        && b.y1 <= f64::from(size.y),
                    "{label} out of bounds {b:?} at {size:?}"
                );
                if label == "Opening Light, Aurea Ensemble" {
                    let dock = if size.x < 976.0 { 150.0 } else { 116.0 };
                    assert!(
                        b.y1 <= f64::from(size.y - dock - 28.0),
                        "first row behind transport: {b:?}"
                    );
                }
            }
        }
    }
    #[test]
    fn facet_clicks_reset_and_audio_modes_work_without_starting_audio() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
        app.preview("library");
        let size = vec2(1120.0, 760.0);
        click_label(&mut app, &ctx, size, "Aurea Ensemble");
        assert!(app.browser.active());
        assert_eq!(app.browser.indices.len(), 24);
        assert!(app.current.is_none());
        app.capture_selection();
        assert_eq!(app.listening_tracks().len(), 24);
        app.saved.shuffle = true;
        for _ in 0..20 {
            let id = app.choose_next().unwrap();
            assert!(
                app.saved
                    .tracks
                    .iter()
                    .any(|t| t.source == id && t.artist == "Aurea Ensemble")
            );
        }
        click_label(&mut app, &ctx, size, "Azzera ricerca e filtri");
        assert!(!app.browser.active());
        assert_eq!(app.browser.indices.len(), 72);
        click_label(&mut app, &ctx, size, "Audio");
        assert!(app.audio_dialog);
        click_label(&mut app, &ctx, size, "Esclusiva");
        assert!(app.saved.exclusive);
        assert!(app.current.is_none());
        assert!(app.planned.is_none());
        click_label(&mut app, &ctx, size, "Condivisa");
        assert!(!app.saved.exclusive);
    }
    #[test]
    fn network_save_footer_stays_visible_and_closes_at_minimum_size() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
        app.preview("network");
        let size = vec2(640.0, 440.0);
        for _ in 0..2 {
            frame(&mut app, &ctx, size, vec![]);
        }
        let out = frame(&mut app, &ctx, size, vec![]);
        let update = out.platform_output.accesskit_update.unwrap();
        let (_, node) = update
            .nodes
            .iter()
            .find(|(_, n)| n.label() == Some("Verifica e salva"))
            .unwrap();
        let b = node.bounds().unwrap();
        assert!(
            b.x0 >= 0.0 && b.x1 <= 640.0 && b.y1 <= 440.0,
            "save outside viewport: {b:?}"
        );
        click_label(&mut app, &ctx, size, "Chiudi");
        assert!(!app.ftp_dialog);
        assert!(app.current.is_none());
    }
}
