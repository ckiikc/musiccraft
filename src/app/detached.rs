//! Independent native tool windows, with shared desktop/multi-monitor placement.
use super::*;

#[derive(Default)]
pub(super) struct Windows {
    windows: HashMap<String, Window>,
}

#[derive(Default)]
struct Window {
    geometry: window_placement::Tracker,
    open: bool,
    focus: bool,
}
impl Windows {
    pub fn load(&mut self, saved: &HashMap<String, window_placement::Placement>) {
        for (id, placement) in saved {
            self.windows
                .entry(id.clone())
                .or_default()
                .geometry
                .load(Some(*placement));
        }
    }
    pub fn saved(&self) -> HashMap<String, window_placement::Placement> {
        self.windows
            .iter()
            .filter_map(|(id, window)| window.geometry.saved().map(|p| (id.clone(), p)))
            .collect()
    }
    pub fn close(&mut self, id: &str) {
        if let Some(window) = self.windows.get_mut(id) {
            window.open = false;
        }
    }
}
impl Aurea {
    pub(super) fn detached_window(
        &mut self,
        ctx: &egui::Context,
        id: &'static str,
        title: &str,
        size: egui::Vec2,
        mut contents: impl FnMut(&mut Self, &mut egui::Ui),
    ) -> bool {
        // Move this window's tracker out while rendering so content can use the normal app methods.
        let (key, mut window) = self
            .auxiliary_geometry
            .windows
            .remove_entry(id)
            .unwrap_or_else(|| (id.into(), Window::default()));
        if !window.open {
            window.geometry.open(ctx, size);
            window.focus = ctx.input(|i| i.raw.viewports.values().any(|v| v.focused == Some(true)));
            window.open = true;
        }
        let viewport = egui::ViewportId::from_hash_of(("musiccraft-auxiliary", id));
        let builder = window
            .geometry
            .builder(ctx, title)
            .with_min_inner_size(size.min(vec2(480.0, 320.0)))
            .with_window_level(if self.saved.pinned {
                egui::WindowLevel::AlwaysOnTop
            } else {
                egui::WindowLevel::Normal
            })
            .with_active(window.focus);
        let mut open = true;
        ctx.show_viewport_immediate(viewport, builder, |ui, _class| {
            let child = ui.ctx().clone();
            // egui supplies its own embedded wrapper for web/headless integrations.
            // Native eframe renders this callback in an independent OS window.
            window.geometry.observe(&child, viewport);
            open = !child.input(|i| i.viewport().close_requested());
            open &= !child.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
            ui.painter().rect_filled(ui.max_rect(), 0.0, palette::bg());
            let rect = ui.max_rect().shrink(12.0);
            ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                contents(self, ui)
            });
            if !open && child.viewport_id() == viewport {
                child.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
        window.open = open;
        self.auxiliary_geometry.windows.insert(key, window);
        open
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    #[test]
    fn tool_windows_have_separate_viewports_and_closing_one_keeps_the_player() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
        let requests = Rc::new(RefCell::new(Vec::new()));
        let close = Rc::new(Cell::new(None));
        let root = egui::Rect::from_min_size(pos2(100.0, 100.0), vec2(520.0, 520.0));
        egui::Context::set_immediate_viewport_renderer({
            let requests = requests.clone();
            let close = close.clone();
            move |ctx, mut viewport| {
                requests.borrow_mut().push((
                    viewport.ids.this,
                    viewport.ids.parent,
                    viewport.builder.clone(),
                ));
                let size = viewport.builder.inner_size.unwrap_or(vec2(760.0, 700.0));
                let mut input = egui::RawInput {
                    viewport_id: viewport.ids.this,
                    screen_rect: Some(egui::Rect::from_min_size(pos2(0.0, 0.0), size)),
                    ..Default::default()
                };
                input.viewports.insert(
                    egui::ViewportId::ROOT,
                    egui::ViewportInfo {
                        inner_rect: Some(root),
                        outer_rect: Some(root),
                        ..Default::default()
                    },
                );
                input.viewports.insert(
                    viewport.ids.this,
                    egui::ViewportInfo {
                        parent: Some(viewport.ids.parent),
                        inner_rect: Some(egui::Rect::from_min_size(pos2(900.0, 100.0), size)),
                        outer_rect: Some(egui::Rect::from_min_size(pos2(900.0, 100.0), size)),
                        events: if close.get() == Some(viewport.ids.this) {
                            vec![egui::ViewportEvent::Close]
                        } else {
                            vec![]
                        },
                        ..Default::default()
                    },
                );
                let mut output = ctx.run_ui(input, |ui| (viewport.viewport_ui_cb)(ui));
                output.textures_delta.clear();
                assert!(
                    output
                        .viewport_output
                        .get(&egui::ViewportId::ROOT)
                        .is_none_or(|v| !v.commands.contains(&egui::ViewportCommand::Close)),
                    "Closing a tool must not close the main window"
                );
            }
        });
        ctx.set_embed_viewports(false);
        app.audio_dialog = true;
        app.info_dialog = true;
        app.sources_dialog = true;
        app.track_dialog = true;
        app.help_dialog = true;
        app.ftp_dialog = true;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(root),
                ..Default::default()
            },
            |_| {
                app.auxiliary_windows(&ctx);
                app.help_window(&ctx);
                app.ftp_window(&ctx);
            },
        );
        output.textures_delta.clear();
        let help = egui::ViewportId::from_hash_of(("musiccraft-auxiliary", "musiccraft-help"));
        let ids: std::collections::HashSet<_> =
            requests.borrow().iter().map(|(id, _, _)| *id).collect();
        assert_eq!(
            ids.len(),
            6,
            "Every tool must request its own native window"
        );
        for (id, parent, builder) in requests.borrow().iter() {
            assert_ne!(*id, egui::ViewportId::ROOT);
            assert_eq!(*parent, egui::ViewportId::ROOT);
            assert_eq!(builder.transparent, Some(false));
            assert_eq!(builder.decorations, Some(true));
        }
        assert!(requests.borrow().iter().any(|(id, _, b)| {
            *id == help
                && b.inner_size
                    .is_some_and(|s| s.x > root.width() && s.y > root.height())
        }));
        close.set(Some(help));
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(root),
                ..Default::default()
            },
            |_| app.help_window(&ctx),
        );
        output.textures_delta.clear();
        assert!(!app.help_dialog);
        assert!(app.audio_dialog && app.info_dialog && app.track_dialog);
        assert_eq!(app.auxiliary_geometry.saved().len(), 6);
        ctx.set_embed_viewports(true);
    }
}
