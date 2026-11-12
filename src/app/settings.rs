use super::*;
pub(super) struct RoomUpdate {
    pub origin: (String, String),
    pub result: Result<crate::multiroom::Snapshot, String>,
}
impl Aurea {
    fn room_client(&self) -> Option<crate::multiroom::Client> {
        if self.saved.multiroom.integrated {
            self.embedded_connection.as_ref().map(|c| c.client.clone())
        } else {
            Some(crate::multiroom::Client {
                config: self.saved.multiroom.clone(),
                password: self.room_password.clone(),
            })
        }
    }
    fn stop_integrated(&mut self) {
        self.embedded_server.stop();
        self.embedded_connection = None;
        self.embedded_starting = false;
        self.media_server = None;
        self.room_state = Default::default();
    }
    pub(super) fn start_integrated(&mut self) {
        self.embedded_autostart_attempted = true;
        self.embedded_message.clear();
        let configured = self
            .saved
            .multiroom
            .bind
            .parse::<std::net::SocketAddrV4>()
            .ok();
        let interfaces = crate::embedded_server::interfaces();
        let host = configured
            .filter(|a| interfaces.contains(a.ip()))
            .map(|a| *a.ip())
            .or_else(|| interfaces.first().copied());
        let Some(host) = host else {
            self.embedded_message = "Connect the Mac to a private Wi-Fi or Ethernet network".into();
            return;
        };
        self.media_server = None;
        let scope = self.selected_scope();
        let tracks = self
            .saved
            .tracks
            .iter()
            .filter(|t| scope.contains(&t.source))
            .cloned()
            .collect();
        let bind = std::net::SocketAddr::from((host, 0));
        match crate::media_server::Server::start(
            bind,
            tracks,
            self.cache.clone(),
            self.auth.clone(),
        ) {
            Ok(server) => {
                self.saved.multiroom.bind = format!("{host}:9042");
                let playlist = server.playlist().to_string();
                self.media_server = Some(server);
                self.shared_source = self.saved.active_source.clone();
                self.shared_revision = self.library_revision;
                self.embedded_starting = true;
                self.embedded_server.start(host, playlist);
            }
            Err(message) => self.embedded_message = message,
        }
    }
    pub(super) fn refresh_shared_library(&mut self, select_current: bool) {
        if self.media_server.is_none() {
            return;
        }
        if !select_current && (self.scanning || self.shared_revision == self.library_revision) {
            return;
        }
        if select_current {
            self.shared_source = self.saved.active_source.clone();
        }
        let scope = self.scope_for_source(&self.shared_source);
        let tracks = self
            .saved
            .tracks
            .iter()
            .filter(|t| scope.contains(&t.source))
            .cloned()
            .collect();
        let server = self.media_server.as_mut().unwrap();
        if server.update(tracks, &self.root_aliases) && self.saved.multiroom.integrated {
            self.embedded_server.update_playlist(server.playlist());
        }
        self.shared_revision = self.library_revision;
    }
    fn integrated_controls(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new(tr!("MusicCraft includes the server and manages its settings. Start it, then select MusicCraft as the server in piCorePlayer.")).color(palette::muted()));
        ui.label(RichText::new(tr!("New tracks are shared automatically. Refresh changes the shared source without stopping playback.")).small().color(palette::muted()));
        if let Some(server) = &self.media_server {
            let name = self
                .source_choices()
                .into_iter()
                .find(|(id, _)| id == &self.shared_source)
                .map(|(_, name)| name)
                .unwrap_or_else(|| tr!("Source unavailable").into());
            ui.label(format!("{} · {}", name, crate::i18n::tracks(server.count)));
        }
        ui.add_space(8.0);
        if self.embedded_starting {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(tr!("Starting integrated server…"));
            });
            if ui.button(tr!("Stop music server")).clicked() {
                self.stop_integrated();
            }
        } else if let Some(connection) = &self.embedded_connection {
            ui.label(crate::i18n::format(
                "piCorePlayer server address: {address}",
                &[("address", connection.host.to_string())],
            ));
            ui.label(
                RichText::new(tr!("Server ready · automatic player discovery enabled"))
                    .color(palette::accent()),
            );
            ui.horizontal_wrapped(|ui| {
                if ui.button(tr!("Stop music server")).clicked() {
                    self.stop_integrated();
                }
                if ui.button(tr!("Refresh shared source")).clicked() {
                    self.refresh_shared_library(true);
                }
            });
        } else {
            let interfaces = crate::embedded_server::interfaces();
            if !interfaces.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    ui.label(tr!("Host / IP"));
                    for address in &interfaces {
                        let selected = self
                            .saved
                            .multiroom
                            .bind
                            .starts_with(&format!("{address}:"));
                        if ui.selectable_label(selected, address.to_string()).clicked() {
                            self.saved.multiroom.bind = format!("{address}:9042");
                        }
                    }
                });
            }
            if ui.button(tr!("Start integrated multiroom")).clicked() {
                self.start_integrated();
            }
        }
        ui.checkbox(
            &mut self.saved.multiroom.auto_start,
            tr!("Start multiroom when MusicCraft opens"),
        );
        if !self.embedded_message.is_empty() {
            ui.label(
                RichText::new(crate::i18n::engine_message(&self.embedded_message))
                    .color(palette::accent()),
            );
        }
        ui.label(
            RichText::new(tr!(
                "Closing MusicCraft stops its server. The Mac must stay awake and connected."
            ))
            .small()
            .color(palette::muted()),
        );
        ui.separator();
    }
    pub(super) fn open_settings(&mut self, page: u8) {
        if !self.settings_dialog {
            self.settings_geometry.open(&self.ctx, vec2(880.0, 720.0));
            self.settings_focused_on_open =
                self.ctx.input(|i| i.viewport().focused.unwrap_or(false));
        }
        self.settings_dialog = true;
        self.settings_page = page;
    }
    fn room_task(&mut self, command: Option<(String, Vec<String>)>) {
        if self.room_rx.is_some() {
            return;
        }
        let Some(mut client) = self.room_client() else {
            return;
        };
        client.config.selected = self.saved.multiroom.selected.clone();
        let (tx, rx) = mpsc::channel();
        let ctx = self.ctx.clone();
        self.room_rx = Some(rx);
        self.room_refresh_at = Instant::now() + Duration::from_secs(3);
        let origin = (
            client.config.lms_url.clone(),
            client.config.username.clone(),
        );
        std::thread::spawn(move || {
            let result = (|| {
                if let Some((player, command)) = command {
                    client.request(&player, &command)?;
                }
                client.snapshot()
            })();
            let _ = tx.send(RoomUpdate { origin, result });
            ctx.request_repaint();
        });
    }
    pub(super) fn settings_window(&mut self, ctx: &egui::Context) {
        self.refresh_shared_library(false);
        if !self.settings_dialog
            || self.settings_page != 3
            || !ctx.input(|i| i.pointer.primary_down())
        {
            self.room_volume_dragging = false;
        }
        while let Some(event) = self.embedded_server.poll() {
            self.embedded_starting = false;
            match event {
                crate::embedded_server::Event::Ready(connection) => {
                    self.embedded_connection = Some(connection);
                    self.embedded_message.clear();
                    self.room_state = Default::default();
                    self.room_rx = None;
                    self.room_task(None);
                }
                crate::embedded_server::Event::Stopped => {
                    self.embedded_connection = None;
                    self.media_server = None;
                    self.room_state = Default::default();
                }
                crate::embedded_server::Event::LibraryUpdated => {
                    self.embedded_message.clear();
                }
                crate::embedded_server::Event::LibraryUpdateFailed(message) => {
                    self.embedded_message = message;
                }
                crate::embedded_server::Event::Failed(message) => {
                    self.embedded_message = message.into();
                    self.embedded_connection = None;
                    self.media_server = None;
                    self.room_state = Default::default();
                }
            }
        }
        if self.saved.multiroom.integrated
            && self.saved.multiroom.auto_start
            && !self.embedded_autostart_attempted
            && !crate::embedded_server::interfaces().is_empty()
        {
            self.start_integrated();
        }
        if let Some(update) = self.room_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.room_rx = None;
            self.room_refresh_at = Instant::now() + Duration::from_secs(3);
            if self.room_client().is_some_and(|c| {
                update.origin.0 == c.config.lms_url && update.origin.1 == c.config.username
            }) {
                match update.result {
                    Ok(state) => {
                        if !self.room_volume_dragging
                            && let Some(status) = state
                                .status
                                .as_ref()
                                .filter(|s| s.player == self.saved.multiroom.selected)
                            && let Some(volume) = status.volume
                        {
                            self.room_volume = volume;
                        }
                        self.room_state = state;
                        self.room_message = tr!("LMS connection verified").into();
                    }
                    Err(error) => {
                        self.room_message = error;
                        self.room_state = Default::default();
                    }
                }
            }
        }
        if self.settings_dialog
            && self.settings_page == 3
            && self.room_rx.is_none()
            && !self.room_state.version.is_empty()
            && !self.room_volume_dragging
            && Instant::now() >= self.room_refresh_at
        {
            self.room_task(None);
        }
        if !self.settings_dialog {
            return;
        }
        if !self.settings_geometry.ready() {
            self.settings_geometry.open(ctx, vec2(880.0, 720.0));
        }
        let viewport = egui::ViewportId::from_hash_of("musiccraft-settings");
        let builder = self
            .settings_geometry
            .builder(ctx, tr!("MusicCraft settings"))
            .with_min_inner_size(vec2(660.0, 460.0))
            .with_window_level(if self.saved.pinned {
                egui::WindowLevel::AlwaysOnTop
            } else {
                egui::WindowLevel::Normal
            })
            .with_active(self.settings_focused_on_open);
        let mut close = false;
        ctx.show_viewport_immediate(viewport, builder, |ui, _class| {
            let ctx = ui.ctx().clone();
            ui.painter().rect_filled(ui.max_rect(), 0.0, palette::bg());
            self.settings_geometry.observe(&ctx, viewport);
            close = ctx.input(|i| i.viewport().close_requested());
            close |= ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
            self.settings_contents(ui);
            if close && ctx.viewport_id() == viewport {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
        if close {
            self.settings_dialog = false;
        }
    }

    fn settings_contents(&mut self, ui: &mut egui::Ui) {
        let area = ui.max_rect();
        let navigation_width = (area.width() * 0.26).clamp(148.0, 184.0);
        let navigation = egui::Rect::from_min_max(
            area.min,
            pos2(area.left() + navigation_width, area.bottom()),
        );
        ui.painter().rect_filled(navigation, 0.0, palette::panel());
        ui.painter().line_segment(
            [navigation.right_top(), navigation.right_bottom()],
            egui::Stroke::new(1.0, palette::muted().gamma_multiply(0.18)),
        );
        let mut sidebar = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("settings-navigation")
                .max_rect(navigation.shrink2(vec2(12.0, 20.0)))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        sidebar.label(
            RichText::new("MUSICCRAFT")
                .size(11.0)
                .strong()
                .color(palette::muted()),
        );
        sidebar.add_space(24.0);
        sidebar.spacing_mut().item_spacing.y = 8.0;
        for (page, icon, label) in [
            (0, "palette", tr!("Interface")),
            (1, "sliders-horizontal", tr!("Audio")),
            (2, "library", tr!("Library")),
            (3, "headphones", tr!("Multiroom")),
        ] {
            let selected = self.settings_page == page;
            let color = if selected {
                palette::accent()
            } else {
                palette::muted()
            };
            if sidebar
                .add_sized(
                    vec2(sidebar.available_width(), 42.0),
                    egui::Button::new((
                        self.icons.image(icon, 17.0, color),
                        RichText::new(label).size(13.5),
                        egui::Atom::grow(),
                    ))
                    .selected(selected)
                    .frame(true)
                    .frame_when_inactive(selected),
                )
                .clicked()
            {
                self.settings_page = page;
            }
        }
        sidebar.add_space((sidebar.available_height() - 24.0).max(0.0));
        sidebar.label(
            RichText::new(env!("CARGO_PKG_VERSION"))
                .monospace()
                .size(11.0)
                .color(palette::muted()),
        );
        let content = egui::Rect::from_min_max(
            pos2(navigation.right() + 24.0, area.top() + 22.0),
            area.max - vec2(24.0, 18.0),
        );
        let mut body = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("settings-page")
                .max_rect(content)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        body.spacing_mut().item_spacing = vec2(10.0, 12.0);
        body.label(
            RichText::new(match self.settings_page {
                1 => tr!("Audio"),
                2 => tr!("Library"),
                3 => tr!("Multiroom"),
                _ => tr!("Interface"),
            })
            .size(24.0)
            .strong(),
        );
        body.add_space(2.0);
        body.separator();
        body.add_space(6.0);
        egui::ScrollArea::vertical()
            .id_salt(("settings-content", self.settings_page))
            .auto_shrink([false, false])
            .max_height(body.available_height())
            .show(&mut body, |ui| match self.settings_page {
                1 => {
                    settings_card(ui, |ui| self.signal(ui));
                }
                2 => {
                    settings_card(ui, |ui| {
                        section(
                            ui,
                            tr!("Library and content"),
                            tr!("Manage synchronization, covers and sources."),
                        );
                        self.auto_sync_control(ui);
                        ui.add_space(8.0);
                        if ui
                            .add_sized(
                                vec2(ui.available_width(), 34.0),
                                egui::Button::image_and_text(
                                    self.icons.image("library", 16.0, palette::muted()),
                                    tr!("Sources and synchronization…"),
                                ),
                            )
                            .clicked()
                        {
                            self.sources_dialog = true;
                        }
                    });
                    settings_card(ui, |ui| {
                        section(ui, tr!("Cover"), "");
                        self.online_cover_control(ui);
                        self.cover_save_control(ui);
                    });
                }
                3 => {
                    settings_card(ui, |ui| self.multiroom_settings(ui));
                }
                _ => self.interface_settings(ui),
            });
    }

    fn interface_settings(&mut self, ui: &mut egui::Ui) {
        settings_card(ui, |ui| {
            section(ui, tr!("Application theme"), "");
            ui.columns(3, |columns| {
                for (theme, label) in [(0, tr!("Gold")), (1, tr!("Graphite")), (2, tr!("Light"))] {
                    let column = &mut columns[theme as usize];
                    if column
                        .add_sized(
                            vec2(column.available_width(), 36.0),
                            egui::Button::new(label).selected(self.saved.theme == theme),
                        )
                        .clicked()
                    {
                        self.saved.theme = theme;
                    }
                }
            });
        });
        settings_card(ui, |ui| {
            section(ui, tr!("Player appearance"), "");
            ui.horizontal(|ui| {
                ui.add_sized(
                    vec2(112.0, 32.0),
                    egui::Label::new(
                        RichText::new(tr!("Manual appearance"))
                            .size(12.0)
                            .color(palette::muted()),
                    )
                    .wrap(),
                );
                egui::ComboBox::from_id_salt("player-appearance")
                    .width(ui.available_width())
                    .selected_text(self.saved.player_appearance.label())
                    .show_ui(ui, |ui| {
                        for appearance in player_visual::Appearance::ALL {
                            ui.selectable_value(
                                &mut self.saved.player_appearance,
                                appearance,
                                appearance.label(),
                            );
                        }
                    });
            });
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);
            ui.checkbox(
                &mut self.saved.appearance_by_year,
                tr!("Appearance by year"),
            );
            ui.label(
                RichText::new(tr!("Tracks without a year use the manual appearance."))
                    .size(12.0)
                    .color(palette::muted()),
            );
            if self.saved.appearance_by_year {
                ui.add_space(4.0);
                egui::Frame::new()
                    .fill(palette::bg())
                    .corner_radius(6.0)
                    .inner_margin(12.0)
                    .show(ui, |ui| {
                        let choice_width = (ui.available_width() - 144.0).max(100.0);
                        egui::Grid::new("appearance-year-periods")
                            .spacing(vec2(12.0, 10.0))
                            .min_col_width(100.0)
                            .show(ui, |ui| {
                                ui.add_sized(
                                    vec2(100.0, 24.0),
                                    egui::Label::new(
                                        RichText::new(tr!("Up to year"))
                                            .size(11.5)
                                            .color(palette::muted()),
                                    )
                                    .wrap(),
                                );
                                ui.label(
                                    RichText::new(tr!("Player appearance"))
                                        .size(11.5)
                                        .color(palette::muted()),
                                );
                                ui.end_row();
                                for i in 0..4 {
                                    if i < 3 {
                                        let lower = if i == 0 {
                                            1
                                        } else {
                                            self.saved.appearance_periods.until[i - 1] + 1
                                        };
                                        let upper = if i == 2 {
                                            9998
                                        } else {
                                            self.saved.appearance_periods.until[i + 1] - 1
                                        };
                                        ui.add_sized(
                                            vec2(100.0, 32.0),
                                            egui::DragValue::new(
                                                &mut self.saved.appearance_periods.until[i],
                                            )
                                            .range(lower..=upper),
                                        );
                                    } else {
                                        ui.add_sized(
                                            vec2(100.0, 32.0),
                                            egui::Label::new(
                                                RichText::new(format!(
                                                    "{}\n{}",
                                                    tr!("After year"),
                                                    self.saved.appearance_periods.until[2]
                                                ))
                                                .size(12.0)
                                                .color(palette::muted()),
                                            )
                                            .halign(egui::Align::Min)
                                            .wrap(),
                                        );
                                    }
                                    egui::ComboBox::from_id_salt(("appearance-period", i))
                                        .width(choice_width)
                                        .selected_text(
                                            self.saved.appearance_periods.media[i].label(),
                                        )
                                        .show_ui(ui, |ui| {
                                            for appearance in player_visual::Appearance::MEDIA {
                                                ui.selectable_value(
                                                    &mut self.saved.appearance_periods.media[i],
                                                    appearance,
                                                    appearance.label(),
                                                );
                                            }
                                        });
                                    ui.end_row();
                                }
                            });
                    });
            }
        });
        settings_card(ui, |ui| {
            section(ui, tr!("Interface size"), "");
            if ui
                .add(
                    egui::Slider::new(&mut self.saved.ui_scale, 1.0..=1.3)
                        .step_by(0.05)
                        .custom_formatter(|value, _| format!("{:.0}%", value * 100.0)),
                )
                .changed()
            {
                self.ctx.set_zoom_factor(self.saved.ui_scale);
            }
            ui.label(
                RichText::new(if cfg!(target_os = "macos") {
                    tr!("Use ⌘+ and ⌘− to adjust text size.").to_owned()
                } else {
                    tr!("Use ⌘+ and ⌘− to adjust text size.").replace('⌘', "Ctrl")
                })
                .size(12.0)
                .color(palette::muted()),
            );
        });
        settings_card(ui, |ui| {
            section(ui, tr!("Window and animation"), "");
            ui.checkbox(
                &mut self.saved.reduce_motion,
                tr!("Reduce player animation"),
            );
            ui.checkbox(&mut self.saved.pinned, tr!("Always on top"));
        });
        settings_card(ui, |ui| self.language_settings(ui));
    }
    fn multiroom_settings(&mut self, ui: &mut egui::Ui) {
        section(
            ui,
            tr!("Music server and rooms"),
            tr!(
                "The integrated server connects piCorePlayer and Squeezelite without a separate installation."
            ),
        );
        ui.label(RichText::new(tr!("Remote playback uses each room’s settings. This Mac’s DSP and exclusive mode apply only to local listening.")).small().color(palette::muted()));
        ui.add_space(12.0);
        let previous = self.saved.multiroom.integrated;
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(
                &mut self.saved.multiroom.integrated,
                true,
                tr!("Integrated server"),
            );
            ui.selectable_value(
                &mut self.saved.multiroom.integrated,
                false,
                tr!("External LMS"),
            );
        });
        if previous != self.saved.multiroom.integrated {
            self.stop_integrated();
            self.room_rx = None;
            self.room_state = Default::default();
            self.room_message.clear();
        }
        if self.saved.multiroom.integrated {
            self.integrated_controls(ui);
        } else {
            ui.add_space(12.0);
            section(
                ui,
                tr!("Source sharing"),
                tr!(
                    "New tracks are shared automatically. Refresh changes the shared source without stopping playback."
                ),
            );
            if self.media_server.is_none() {
                ui.horizontal_wrapped(|ui| {
                    ui.label(tr!("Local address and port"));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.saved.multiroom.bind)
                            .desired_width(190.0),
                    );
                });
                if !self.networks.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(tr!("Host / IP"));
                        for lan in &self.networks {
                            if ui.small_button(lan.address.to_string()).clicked() {
                                let port = self
                                    .saved
                                    .multiroom
                                    .bind
                                    .parse::<std::net::SocketAddr>()
                                    .map_or(9042, |a| a.port());
                                self.saved.multiroom.bind = format!("{}:{port}", lan.address);
                            }
                        }
                    });
                }
                ui.label(RichText::new(tr!("Use the Mac’s private Wi-Fi or Ethernet IP for Raspberry access. 127.0.0.1 works only on this Mac.")).small().color(palette::muted()));
                if ui.button(tr!("Start music server")).clicked() {
                    let scope = self.selected_scope();
                    let tracks = self
                        .saved
                        .tracks
                        .iter()
                        .filter(|t| scope.contains(&t.source))
                        .cloned()
                        .collect();
                    match self
                        .saved
                        .multiroom
                        .bind
                        .parse()
                        .map_err(|_| tr!("Invalid local address").into())
                        .and_then(|bind| {
                            crate::media_server::Server::start(
                                bind,
                                tracks,
                                self.cache.clone(),
                                self.auth.clone(),
                            )
                        }) {
                        Ok(server) => {
                            self.media_server = Some(server);
                            self.shared_source = self.saved.active_source.clone();
                            self.shared_revision = self.library_revision;
                        }
                        Err(error) => self.room_message = error,
                    }
                }
            } else {
                let server = self.media_server.as_ref().unwrap();
                ui.label(crate::i18n::format(
                    "Server active · {count} · {address}",
                    &[
                        ("count", crate::i18n::tracks(server.count)),
                        ("address", server.address.to_string()),
                    ],
                ));
                if ui.button(tr!("Copy library URL")).clicked() {
                    ui.ctx().copy_text(server.playlist_url());
                }
                ui.label(RichText::new(tr!("The link has a temporary key: share it only with your players. Closing MusicCraft stops the server.")).small().color(palette::muted()));
                if ui.button(tr!("Refresh shared source")).clicked() {
                    self.refresh_shared_library(true);
                }
                if ui.button(tr!("Stop music server")).clicked() {
                    self.media_server = None;
                }
            }
            ui.add_space(16.0);
            ui.separator();
            section(
                ui,
                "Lyrion Music Server",
                tr!("LMS can run on a Mac, NAS or Raspberry."),
            );
            let endpoint_before = (
                self.saved.multiroom.lms_url.clone(),
                self.saved.multiroom.username.clone(),
            );
            ui.label(tr!("LMS address"));
            ui.add(
                egui::TextEdit::singleline(&mut self.saved.multiroom.lms_url)
                    .desired_width(f32::INFINITY),
            );
            ui.horizontal_wrapped(|ui| {
                ui.label(tr!("User"));
                ui.add(
                    egui::TextEdit::singleline(&mut self.saved.multiroom.username)
                        .desired_width(120.0),
                );
                ui.label(tr!("Password"));
                ui.add(
                    egui::TextEdit::singleline(&mut self.room_password)
                        .password(true)
                        .desired_width(120.0),
                );
            });
            if endpoint_before.0 != self.saved.multiroom.lms_url
                || endpoint_before.1 != self.saved.multiroom.username
            {
                self.room_state = Default::default();
                self.saved.multiroom.selected.clear();
                self.room_message.clear();
            }
            ui.label(
                RichText::new(tr!("LMS password is kept only for this session."))
                    .small()
                    .color(palette::muted()),
            );
            if ui
                .add_enabled(
                    self.room_rx.is_none(),
                    egui::Button::new(tr!("Connect and refresh rooms")),
                )
                .clicked()
            {
                self.room_task(None);
            }
            if self.room_rx.is_some() {
                ui.spinner();
            }
            if !self.room_message.is_empty() {
                ui.label(
                    RichText::new(crate::i18n::engine_message(&self.room_message))
                        .color(palette::accent()),
                );
            }
            if !self.room_state.version.is_empty() {
                ui.label(format!("LMS {}", self.room_state.version));
            }
        }
        let selected_before = self.saved.multiroom.selected.clone();
        let players = self.room_state.players.clone();
        for player in &players {
            ui.horizontal_wrapped(|ui| {
                ui.add_enabled_ui(player.connected, |ui| {
                    ui.selectable_value(
                        &mut self.saved.multiroom.selected,
                        player.id.clone(),
                        &player.name,
                    );
                });
                ui.label(RichText::new(&player.model).small().color(palette::muted()));
                if !player.connected {
                    ui.label(tr!("Offline"));
                }
                if player.connected
                    && !self.saved.multiroom.selected.is_empty()
                    && self.saved.multiroom.selected != player.id
                    && ui.small_button(tr!("Group")).clicked()
                {
                    self.room_task(Some((
                        self.saved.multiroom.selected.clone(),
                        vec!["sync".into(), player.id.clone()],
                    )));
                }
            });
        }
        if selected_before != self.saved.multiroom.selected && self.room_rx.is_none() {
            self.room_task(None);
        }
        let selected = self.saved.multiroom.selected.clone();
        let connected = players.iter().any(|p| p.connected && p.id == selected);
        ui.add_enabled_ui(connected && self.room_rx.is_none(), |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button(tr!("Play source in room")).clicked() {
                    if let Some(server) = &self.media_server {
                        self.room_task(Some((
                            selected.clone(),
                            vec!["playlist".into(), "play".into(), server.playlist_url()],
                        )));
                    } else {
                        self.room_message = tr!("Start the music server first").into();
                    }
                }
                for (icon, label, command) in [
                    ("play", tr!("Play"), "play"),
                    ("pause", tr!("Pause"), "pause"),
                    ("square", tr!("Stop"), "stop"),
                ] {
                    if self.icons.button(ui, icon, label, false).clicked() {
                        self.room_task(Some((selected.clone(), vec![command.into()])));
                    }
                }
                if ui.button(tr!("Ungroup room")).clicked() {
                    self.room_task(Some((selected.clone(), vec!["sync".into(), "-".into()])));
                }
            });
        });
        if let Some(status) = self
            .room_state
            .status
            .clone()
            .filter(|s| s.player == selected)
        {
            ui.horizontal_wrapped(|ui| {
                ui.label(match status.mode.as_str() {
                    "play" => tr!("Playing"),
                    "pause" => tr!("Playback paused"),
                    _ => tr!("Ready"),
                });
                ui.add(
                    egui::Label::new(RichText::new(&status.title).color(palette::muted()))
                        .truncate(),
                );
            });
            ui.add_enabled_ui(
                connected
                    && status.volume_control
                    && status.volume.is_some()
                    && self.room_rx.is_none(),
                |ui| {
                    ui.horizontal_wrapped(|ui| {
                        if self
                            .icons
                            .button(
                                ui,
                                if status.muted { "volume-x" } else { "volume-2" },
                                tr!("Mute"),
                                status.muted,
                            )
                            .clicked()
                        {
                            self.room_task(Some((
                                selected.clone(),
                                vec![
                                    "mixer".into(),
                                    "muting".into(),
                                    if status.muted { "0" } else { "1" }.into(),
                                ],
                            )));
                        }
                        let response = ui.add(
                            egui::Slider::new(&mut self.room_volume, 0.0..=100.0)
                                .text(tr!("Volume"))
                                .suffix("%"),
                        );
                        self.room_volume_dragging = response.dragged();
                        if response.drag_stopped() || (response.changed() && !response.dragged()) {
                            self.room_task(Some((
                                selected.clone(),
                                vec![
                                    "mixer".into(),
                                    "volume".into(),
                                    format!("{:.0}", self.room_volume),
                                ],
                            )));
                        }
                    });
                },
            );
        }
        for group in &self.room_state.groups {
            ui.label(group);
        }
        #[cfg(debug_assertions)]
        if self.ctx.data(|d| {
            d.get_temp::<bool>(egui::Id::new("preview-room-controls"))
                .unwrap_or_default()
        }) {
            ui.scroll_to_cursor(Some(egui::Align::Max));
        }
        ui.hyperlink_to(
            tr!("piCorePlayer / LMS guide"),
            "https://docs.picoreplayer.org/how-to/install_lms/",
        );
    }
    fn language_settings(&mut self, ui: &mut egui::Ui) {
        section(ui, tr!("Language"), "");
        let previous = self.saved.language.clone();
        let title = crate::i18n::LANGUAGES
            .iter()
            .find(|(code, _)| *code == self.saved.language)
            .map(|(_, name)| *name)
            .unwrap_or(tr!("System language"));
        let popup = egui::ComboBox::from_id_salt("interface-language")
            .width(ui.available_width())
            .selected_text(title)
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut self.saved.language,
                    String::new(),
                    tr!("System language"),
                );
                for (code, name) in crate::i18n::LANGUAGES {
                    ui.selectable_value(&mut self.saved.language, code.to_string(), name);
                }
            });
        let open = popup.inner.is_some();
        if open != self.language_fonts_open {
            self.language_fonts_open = open;
            listening_ui::configure_font_stack(&self.ctx, open);
        }
        if previous != self.saved.language {
            let language = if self.saved.language.is_empty() {
                crate::i18n::system_language()
            } else {
                self.saved.language.clone()
            };
            crate::i18n::set(&language);
            self.library_revision = self.library_revision.wrapping_add(1);
            self.refresh_browser();
            listening_ui::configure_font_stack(&self.ctx, open);
        }
    }
}

fn settings_card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let inner = egui::Frame::new()
        .fill(palette::panel())
        .corner_radius(8.0)
        .stroke(egui::Stroke::new(
            1.0,
            palette::muted().gamma_multiply(0.16),
        ))
        .inner_margin(16.0)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.spacing_mut().item_spacing = vec2(10.0, 10.0);
            ui.spacing_mut().button_padding = vec2(8.0, 6.0);
            ui.spacing_mut().interact_size.y = 32.0;
            add(ui)
        })
        .inner;
    ui.add_space(14.0);
    inner
}

#[cfg(test)]
mod design_tests {
    use super::*;

    fn render(
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
            |ui| app.settings_contents(ui),
        );
        output.textures_delta.clear();
        output
    }

    fn button_events(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            },
        ]
    }

    #[test]
    fn settings_navigation_stays_inside_small_and_large_windows() {
        for size in [
            vec2(660.0, 460.0),
            vec2(680.0, 620.0),
            vec2(880.0, 720.0),
            vec2(1100.0, 780.0),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let ctx = egui::Context::default();
            let cc = eframe::CreationContext::_new_kittest(ctx.clone());
            let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
            for (page, label) in [
                (0, tr!("Interface")),
                (1, tr!("Audio")),
                (2, tr!("Library")),
                (3, tr!("Multiroom")),
            ] {
                render(&mut app, &ctx, size, vec![]);
                let output = render(&mut app, &ctx, size, vec![]);
                let nodes = output.platform_output.accesskit_update.unwrap().nodes;
                let bounds = nodes
                    .iter()
                    .find(|(_, n)| {
                        n.label() == Some(label) && n.role() == egui::accesskit::Role::Button
                    })
                    .and_then(|(_, n)| n.bounds())
                    .unwrap();
                assert!(bounds.x0 >= 0.0 && bounds.x1 <= 184.0 && bounds.y1 < f64::from(size.y));
                let pos = pos2(
                    ((bounds.x0 + bounds.x1) * 0.5) as f32,
                    ((bounds.y0 + bounds.y1) * 0.5) as f32,
                );
                render(&mut app, &ctx, size, button_events(pos, true));
                render(&mut app, &ctx, size, button_events(pos, false));
                assert_eq!(app.settings_page, page);
            }
        }
    }

    #[test]
    fn year_checkbox_changes_graphics_without_sending_audio_commands() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
        let (tx, commands) = mpsc::channel();
        app.audio.tx = tx;
        app.saved.player_appearance = player_visual::Appearance::Cd;
        let mut track = Track::placeholder("/synthetic/1985.flac".into());
        track.year = Some(1985);
        app.current = Some(track.source.clone());
        app.saved.tracks.push(track);
        let size = vec2(880.0, 720.0);
        for enabled in [true, false] {
            render(&mut app, &ctx, size, vec![]);
            let output = render(&mut app, &ctx, size, vec![]);
            let nodes = output.platform_output.accesskit_update.unwrap().nodes;
            let bounds = nodes
                .iter()
                .find(|(_, n)| {
                    n.label() == Some(tr!("Appearance by year"))
                        && n.role() == egui::accesskit::Role::CheckBox
                })
                .and_then(|(_, n)| n.bounds())
                .unwrap();
            let pos = pos2(
                ((bounds.x0 + bounds.x1) * 0.5) as f32,
                ((bounds.y0 + bounds.y1) * 0.5) as f32,
            );
            render(&mut app, &ctx, size, button_events(pos, true));
            render(&mut app, &ctx, size, button_events(pos, false));
            assert_eq!(app.saved.appearance_by_year, enabled);
            assert_eq!(
                app.effective_player_appearance(),
                if enabled {
                    player_visual::Appearance::Cassette
                } else {
                    player_visual::Appearance::Cd
                }
            );
            assert!(commands.try_recv().is_err());
        }
    }
}
