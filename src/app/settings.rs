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
            self.settings_geometry.open(&self.ctx, vec2(680.0, 620.0));
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
            self.settings_geometry.open(ctx, vec2(680.0, 620.0));
        }
        let viewport = egui::ViewportId::from_hash_of("musiccraft-settings");
        let builder = self
            .settings_geometry
            .builder(ctx, tr!("MusicCraft settings"))
            .with_min_inner_size(vec2(460.0, 340.0))
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
            egui::Frame::new()
                .fill(palette::bg())
                .inner_margin(16.0)
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for (page, label) in [
                            (0, tr!("Interface")),
                            (1, tr!("Audio")),
                            (2, tr!("Library")),
                            (3, tr!("Multiroom")),
                        ] {
                            ui.selectable_value(&mut self.settings_page, page, label);
                        }
                    });
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .id_salt("settings-content")
                        .max_height(ui.available_height().max(100.0))
                        .show(ui, |ui| match self.settings_page {
                            1 => self.signal(ui),
                            2 => {
                                section(
                                    ui,
                                    tr!("Library and content"),
                                    tr!("Manage synchronization, covers and sources."),
                                );
                                self.auto_sync_control(ui);
                                self.online_cover_control(ui);
                                self.cover_save_control(ui);
                                if ui.button(tr!("Sources and synchronization…")).clicked() {
                                    self.sources_dialog = true;
                                }
                            }
                            3 => self.multiroom_settings(ui),
                            _ => self.interface_settings(ui),
                        });
                });
            if close && ctx.viewport_id() == viewport {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
        if close {
            self.settings_dialog = false;
        }
    }

    fn interface_settings(&mut self, ui: &mut egui::Ui) {
        section(
            ui,
            tr!("Appearance and readability"),
            tr!("Preferences shared by the full and mini views."),
        );
        ui.horizontal_wrapped(|ui| {
            ui.label(tr!("Application theme"));
            for (theme, label) in [(0, tr!("Gold")), (1, tr!("Graphite")), (2, tr!("Light"))] {
                ui.selectable_value(&mut self.saved.theme, theme, label);
            }
        });
        ui.add_space(12.0);
        ui.label(tr!("Player appearance"));
        egui::ComboBox::from_id_salt("player-appearance")
            .selected_text(self.saved.player_appearance.label())
            .show_ui(ui, |ui| {
                for appearance in player_visual::Appearance::ALL {
                    ui.selectable_value(
                        &mut self.saved.player_appearance,
                        appearance,
                        appearance.label(),
                    );
                }
            })
            .response
            .on_hover_text(tr!("Player appearance"));
        ui.checkbox(
            &mut self.saved.reduce_motion,
            tr!("Reduce player animation"),
        );
        ui.checkbox(&mut self.saved.pinned, tr!("Always on top"));
        ui.add_space(12.0);
        ui.label(tr!("Interface size"));
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
        ui.label(RichText::new(tr!("Use ⌘+ and ⌘− to adjust text size.")).color(palette::muted()));
        ui.add_space(14.0);
        // Language selector is populated by the per-app catalog module.
        self.language_settings(ui);
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
            .width(240.0)
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
