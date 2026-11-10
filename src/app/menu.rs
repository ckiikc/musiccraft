use super::*;
use crate::native_menu::{Action, State};

impl Aurea {
    fn menu_state(&self) -> State {
        State {
            language: crate::i18n::language(),
            can_toggle: self.can_toggle(),
            navigation: self.navigation_ready(),
            paused: self.paused,
            scanning: self.scanning,
            auto_sync: self.saved.auto_sync,
            facets: self.saved.facets_visible,
            order: self.order_mode(),
            repeat: self.saved.repeat,
            online_covers: self.saved.online_covers,
            save_covers: self.saved.save_covers,
            pinned: self.saved.pinned,
            mini: self.saved.mini,
        }
    }
    pub(super) fn native_menus(&mut self, ctx: &egui::Context) {
        for action in crate::native_menu::take() {
            self.menu_action(action, ctx)
        }
        crate::native_menu::sync(ctx, self.menu_state());
        #[cfg(debug_assertions)]
        crate::native_menu::preview(ctx);
    }
    fn menu_action(&mut self, action: Action, ctx: &egui::Context) {
        if !action.enabled(&self.menu_state()) {
            return;
        };
        match action {
            Action::Files => self.pick_music(false),
            Action::Folder => self.pick_music(true),
            Action::Network => self.open_network(),
            Action::PairPhone => self.open_phone_pair(),
            Action::AutoSync => self.set_auto_sync(!self.saved.auto_sync),
            Action::Sources => self.sources_dialog = true,
            Action::CancelSync => self.cancel.store(true, Ordering::Relaxed),
            Action::Facets => self.saved.facets_visible = !self.saved.facets_visible,
            Action::ClearFilters => {
                self.query.clear();
                self.browser.clear()
            }
            Action::PlayPause => self.toggle(),
            Action::Previous => {
                if let Some(id) = self.history.previous() {
                    self.play(id)
                }
            }
            Action::Next => self.next(),
            Action::Stop => self.stop(),
            Action::Sequence => self.set_order_mode(0),
            Action::ShuffleTracks => self.set_order_mode(1),
            Action::ShuffleAlbums => self.set_order_mode(2),
            Action::Repeat => {
                self.saved.repeat = !self.saved.repeat;
                self.prepare_next()
            }
            Action::Details => self.track_dialog = true,
            Action::Artist => {
                self.info_dialog = true;
                self.last_lyric_line = None
            }
            Action::Settings => self.open_settings(0),
            Action::Multiroom => self.open_settings(3),
            Action::OnlineCovers => {
                self.saved.online_covers = !self.saved.online_covers;
                self.online_covers
                    .store(self.saved.online_covers, Ordering::Relaxed)
            }
            Action::SaveCovers => {
                self.saved.save_covers = !self.saved.save_covers;
                self.save_covers
                    .store(self.saved.save_covers, Ordering::Relaxed)
            }
            Action::Audio => self.audio_dialog = true,
            Action::Pinned => {
                self.saved.pinned = !self.saved.pinned;
                ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(if self.saved.pinned {
                    egui::WindowLevel::AlwaysOnTop
                } else {
                    egui::WindowLevel::Normal
                }))
            }
            Action::Mini => self.saved.mini = true,
            Action::Guide => self.help_dialog = true,
        }
    }
}
