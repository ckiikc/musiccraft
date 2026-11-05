use super::*;
use crate::jobs::gallery::{self, Item};
use std::sync::atomic::AtomicU64;

// Assistive technologies activate the image button without a pointer double-click.
pub(super) fn requested(response: &egui::Response) -> bool {
    response.double_clicked()
        || response.ctx.input(|input| {
            input
                .accesskit_action_requests(response.id, egui::accesskit::Action::Click)
                .next()
                .is_some()
        })
}

enum Update {
    Pages(Vec<Item>),
    Image(Option<library::Artwork>),
}

#[derive(Default)]
struct State {
    open: bool,
    title: String,
    geometry: window_placement::Tracker,
    focused_on_open: bool,
    pages: Vec<Item>,
    index: usize,
    texture: Option<egui::TextureHandle>,
    fallback: Option<egui::TextureHandle>,
    pending: Option<mpsc::Receiver<Update>>,
    cancel: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
    loading: bool,
    unavailable: bool,
}

impl State {
    pub fn is_open(&self) -> bool {
        self.open
    }

    fn cancel_task(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.pending = None;
    }

    fn close(&mut self) {
        self.cancel_task();
        self.open = false;
        self.texture = None;
        self.fallback = None;
        self.pages.clear();
    }

    fn open(
        &mut self,
        track: Track,
        texture: Option<egui::TextureHandle>,
        auth: AuthStore,
        ctx: egui::Context,
    ) {
        self.cancel_task();
        self.open = true;
        self.title = track.album.clone();
        self.focused_on_open = ctx.input(|input| input.viewport().focused.unwrap_or(false));
        self.geometry.open(&ctx, vec2(800.0, 760.0));
        self.index = 0;
        self.pages = vec![Item {
            source: String::new(),
            name: String::new(),
            front: true,
        }];
        self.fallback = texture.clone();
        self.texture = texture;
        self.loading = true;
        self.unavailable = false;
        self.cancel = Arc::new(AtomicBool::new(false));
        let cancel = self.cancel.clone();
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        std::thread::spawn(move || {
            let items = gallery::list(&track, &auth, &cancel);
            let _ = tx.send(Update::Pages(items));
            ctx.request_repaint_of(viewport_id());
        });
    }

    fn request_image(&mut self, auth: &AuthStore, cache: &std::path::Path, ctx: &egui::Context) {
        self.cancel_task();
        self.unavailable = false;
        let Some(item) = self.pages.get(self.index).cloned() else {
            return;
        };
        if item.source.is_empty() {
            self.texture = self.fallback.clone();
            self.loading = false;
            self.unavailable = self.texture.is_none();
            return;
        }
        self.texture = None;
        self.loading = true;
        let (auth, cache, ctx) = (auth.clone(), cache.to_owned(), ctx.clone());
        let generation = self.generation.clone();
        let ticket = generation.load(Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        std::thread::spawn(move || {
            let image = gallery::load(&item, &cache, &auth, &generation, ticket);
            let _ = tx.send(Update::Image(image));
            ctx.request_repaint_of(viewport_id());
        });
    }

    fn poll(&mut self, auth: &AuthStore, cache: &std::path::Path, ctx: &egui::Context) {
        let Some(update) = self.pending.as_ref().and_then(|rx| rx.try_recv().ok()) else {
            return;
        };
        self.pending = None;
        match update {
            Update::Pages(mut items) => {
                if items.first().is_some_and(|item| item.front)
                    || self.fallback.is_none() && !items.is_empty()
                {
                    self.pages[0] = items.remove(0);
                }
                self.pages.extend(items);
                self.request_image(auth, cache, ctx);
            }
            Update::Image(image) => {
                self.loading = false;
                self.texture = image
                    .map(|image| {
                        ctx.load_texture(
                            "album-artwork-page",
                            egui::ColorImage::from_rgba_unmultiplied(image.size, &image.rgba),
                            Default::default(),
                        )
                    })
                    .or_else(|| (self.index == 0).then(|| self.fallback.clone()).flatten());
                self.unavailable = self.texture.is_none();
            }
        }
    }

    fn content(&mut self, ui: &mut egui::Ui, reduce_motion: bool, direction: &mut isize) {
        let ctx = ui.ctx().clone();
        ctx.enable_accesskit();
        ui.painter().rect_filled(ui.max_rect(), 0.0, palette::bg());
        ui.label(RichText::new(&self.title).strong());
        let size = vec2(
            ui.available_width().max(1.0),
            (ui.available_height() - 35.0).max(80.0),
        );
        let (area, response) = ui.allocate_exact_size(size, egui::Sense::hover());
        ui.painter().rect_filled(area, 4.0, palette::bg());
        if let Some(texture) = &self.texture {
            let original = texture.size_vec2();
            let scale = (area.width() / original.x).min(area.height() / original.y);
            let image_rect = egui::Rect::from_center_size(area.center(), original * scale);
            ui.painter().image(
                texture.id(),
                image_rect,
                egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        if self.unavailable {
            ui.painter().text(
                area.center(),
                egui::Align2::CENTER_CENTER,
                tr!("Artwork unavailable"),
                egui::FontId::proportional(14.0),
                palette::muted(),
            );
        }
        if self.pages.len() > 1 {
            let hovered = response.hovered()
                || ui.input(|i| i.pointer.hover_pos().is_some_and(|p| area.contains(p)));
            let alpha = if reduce_motion {
                f32::from(hovered)
            } else {
                ctx.animate_bool(ui.id().with("artwork-controls"), hovered)
            };
            ui.scope(|ui| {
                ui.set_opacity(alpha);
                for (side, caption, label, delta) in [
                    (false, "‹", tr!("Previous image"), -1),
                    (true, "›", tr!("Next image"), 1),
                ] {
                    let center = pos2(
                        if side {
                            area.right() - 27.0
                        } else {
                            area.left() + 27.0
                        },
                        area.center().y,
                    );
                    let button = ui.put(
                        egui::Rect::from_center_size(center, vec2(42.0, 54.0)),
                        egui::Button::new(RichText::new(caption).size(32.0))
                            .fill(palette::panel().gamma_multiply(0.94)),
                    );
                    button.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label)
                    });
                    if button.clicked() {
                        *direction = delta;
                    }
                    button.on_hover_text(label);
                }
            });
        }
        ui.horizontal(|ui| {
            ui.label(format!("{} / {}", self.index + 1, self.pages.len()));
            if let Some(item) = self.pages.get(self.index) {
                ui.add(
                    egui::Label::new(if item.name.is_empty() {
                        tr!("Cover")
                    } else {
                        &item.name
                    })
                    .truncate(),
                );
            }
            if self.loading {
                ui.spinner();
                ui.label(tr!("Loading…"));
            }
        });
    }

    fn update_ui(
        &mut self,
        ui: &mut egui::Ui,
        auth: &AuthStore,
        cache: &std::path::Path,
        reduce_motion: bool,
    ) {
        let ctx = ui.ctx().clone();
        self.geometry.observe(&ctx, viewport_id());
        self.poll(auth, cache, &ctx);
        let mut close = ctx.input(|input| input.viewport().close_requested());
        let mut direction = 0;
        ctx.input_mut(|input| {
            close |= input.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
            if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft) {
                direction = -1;
            }
            if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight) {
                direction = 1;
            }
        });
        egui::Frame::new()
            .fill(palette::bg())
            .inner_margin(12.0)
            .show(ui, |ui| self.content(ui, reduce_motion, &mut direction));
        if close {
            self.close();
            if ctx.viewport_id() == viewport_id() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ctx.request_repaint_of(egui::ViewportId::ROOT);
        } else if direction != 0 && self.pages.len() > 1 {
            self.index =
                (self.index as isize + direction).rem_euclid(self.pages.len() as isize) as usize;
            self.request_image(auth, cache, &ctx);
        }
    }
}

fn viewport_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("album-artwork-viewer")
}

#[derive(Default)]
pub(super) struct Viewer {
    state: Arc<std::sync::Mutex<State>>,
}
impl Viewer {
    pub fn load_placement(&mut self, placement: Option<window_placement::Placement>) {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .geometry
            .load(placement);
    }
    pub fn placement(&self) -> Option<window_placement::Placement> {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .geometry
            .saved()
    }
    pub fn is_open(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_open()
    }

    fn open(
        &mut self,
        track: Track,
        texture: Option<egui::TextureHandle>,
        auth: AuthStore,
        ctx: egui::Context,
    ) {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .open(track, texture, auth, ctx);
    }

    fn show(
        &mut self,
        ctx: &egui::Context,
        auth: &AuthStore,
        cache: &std::path::Path,
        reduce_motion: bool,
        pinned: bool,
    ) {
        let builder = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if !state.open {
                return;
            }
            state
                .geometry
                .builder(ctx, tr!("Album artwork"))
                .with_min_inner_size(vec2(300.0, 240.0))
                .with_window_level(if pinned {
                    egui::WindowLevel::AlwaysOnTop
                } else {
                    egui::WindowLevel::Normal
                })
                .with_active(state.focused_on_open)
        };
        let (state, auth, cache) = (self.state.clone(), auth.clone(), cache.to_owned());
        // Independent repainting: a static full-size cover doesn't repaint with
        // every frame of the rotating player in the main window.
        ctx.show_viewport_deferred(viewport_id(), builder, move |ui, _class| {
            let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
            if state.open {
                state.update_ui(ui, &auth, &cache, reduce_motion);
            }
        });
    }
}
impl Drop for Viewer {
    fn drop(&mut self) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).close();
    }
}

impl Aurea {
    pub(super) fn open_cover_viewer(&mut self) {
        let Some(track) = self
            .current
            .as_ref()
            .and_then(|id| self.saved.tracks.iter().find(|track| &track.source == id))
            .cloned()
        else {
            return;
        };
        self.cover_viewer
            .open(track, self.art.clone(), self.auth.clone(), self.ctx.clone());
    }

    pub(super) fn cover_window(&mut self, ctx: &egui::Context) {
        self.cover_viewer.show(
            ctx,
            &self.auth,
            &self.cache,
            self.saved.reduce_motion,
            self.saved.pinned,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn double_click_opens_album_artwork_and_arrows_navigate_without_changing_the_track() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
        app.preview("mini-vinyl");
        let source = dir.path().join("track.flac").to_string_lossy().into_owned();
        app.saved.tracks[0].source = source.clone();
        app.current = Some(source.clone());
        let frame = |app: &mut Aurea, time, events| {
            let mut result = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        pos2(0.0, 0.0),
                        vec2(900.0, 740.0),
                    )),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ui| {
                    app.mini_ui(ui);
                    app.cover_window(&ctx);
                },
            );
            result.textures_delta.clear();
        };
        let pos = pos2(45.0, 80.0);
        frame(&mut app, 0.0, vec![egui::Event::PointerMoved(pos)]);
        for (time, pressed) in [(0.1, true), (0.15, false), (0.2, true), (0.25, false)] {
            frame(
                &mut app,
                time,
                vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
        }
        assert!(app.cover_viewer.is_open());
        app.cover_viewer.state.lock().unwrap().cancel_task();
        app.cover_viewer.state.lock().unwrap().pages = vec![
            Item {
                source: String::new(),
                name: "front.png".into(),
                front: true,
            },
            Item {
                source: String::new(),
                name: "back.png".into(),
                front: false,
            },
        ];
        frame(
            &mut app,
            0.4,
            vec![egui::Event::Key {
                key: egui::Key::ArrowRight,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert_eq!(app.cover_viewer.state.lock().unwrap().index, 1);
        assert_eq!(app.current.as_deref(), Some(source.as_str()));
        frame(
            &mut app,
            0.5,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(!app.cover_viewer.is_open());
        assert!(app.cover_viewer.state.lock().unwrap().texture.is_none());
    }
}
