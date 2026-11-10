use super::*;

pub(super) struct Icons(HashMap<&'static str, egui::TextureHandle>);
impl Icons {
    pub fn new(ctx: &egui::Context) -> Self {
        let mut textures = HashMap::new();
        macro_rules! load { ($($name:literal),* $(,)?) => {$(
            let image=image::load_from_memory(include_bytes!(concat!("../../assets/icons/",$name,".png"))).expect("bundled Lucide icon").to_rgba8();
            textures.insert($name,ctx.load_texture($name,egui::ColorImage::from_rgba_unmultiplied([image.width() as usize,image.height() as usize],image.as_raw()),egui::TextureOptions::LINEAR));
        )*}; }
        load!(
            "book-open",
            "mic-vocal",
            "chevron-down",
            "chevron-up",
            "list-ordered",
            "shuffle",
            "play",
            "pause",
            "skip-back",
            "skip-forward",
            "square",
            "repeat-1",
            "disc-3",
            "volume-2",
            "volume-x",
            "headphones",
            "library",
            "search",
            "x",
            "pin",
            "palette",
            "list-music",
            "sliders-horizontal",
            "arrow-up-right",
            "arrow-down-left",
            "panel-left",
            "settings",
            "refresh-cw"
        );
        Self(textures)
    }
    pub fn image(&self, name: &str, size: f32, color: Color32) -> egui::Image<'_> {
        egui::Image::new(self.0.get(name).expect("known icon"))
            .fit_to_exact_size(vec2(size, size))
            .tint(color)
    }
    pub fn button(
        &self,
        ui: &mut egui::Ui,
        name: &str,
        label: &str,
        active: bool,
    ) -> egui::Response {
        let r = ui
            .scope(|ui| {
                ui.spacing_mut().button_padding = vec2(5.0, 5.0);
                ui.add(
                    egui::Button::image(self.image(
                        name,
                        16.0,
                        if active {
                            palette::accent()
                        } else {
                            palette::text()
                        },
                    ))
                    .min_size(vec2(30.0, 30.0))
                    .frame_when_inactive(active)
                    .fill(if active {
                        palette::selected()
                    } else {
                        Color32::TRANSPARENT
                    })
                    .corner_radius(6.0),
                )
            })
            .inner;
        r.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
        });
        r.on_hover_text(label)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn at(
        &self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        name: &str,
        label: &str,
        active: bool,
        primary: bool,
        enabled: bool,
    ) -> egui::Response {
        let color = if primary {
            palette::bg()
        } else if active {
            palette::accent()
        } else {
            palette::text()
        };
        let response = ui
            .scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                if !enabled {
                    ui.disable();
                }
                ui.spacing_mut().button_padding = vec2(4.0, 4.0);
                ui.put(
                    rect,
                    egui::Button::image(self.image(name, if primary { 21.0 } else { 17.0 }, color))
                        .fill(if primary {
                            palette::accent()
                        } else if active {
                            palette::selected()
                        } else {
                            Color32::TRANSPARENT
                        })
                        .frame_when_inactive(primary || active)
                        .corner_radius(if primary { 24.0 } else { 7.0 }),
                )
            })
            .inner;
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
        response.on_hover_text(label)
    }
}
pub(super) fn section(ui: &mut egui::Ui, title: &str, description: &str) {
    ui.label(
        RichText::new(title)
            .size(14.0)
            .strong()
            .color(palette::text()),
    );
    if !description.is_empty() {
        ui.label(
            RichText::new(description)
                .size(12.0)
                .color(palette::muted()),
        );
    }
    ui.add_space(6.0);
}
pub(super) fn caption(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .size(11.5)
            .strong()
            .color(palette::muted()),
    );
}
pub(super) fn clipped(ui: &mut egui::Ui, rect: egui::Rect, text: &str, size: f32, color: Color32) {
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    child.add(egui::Label::new(RichText::new(text).size(size).color(color)).truncate());
}

#[derive(Clone, Copy)]
struct VolumeDrag {
    pointer: egui::Pos2,
}

/// Pressing preserves the current level; hidden-pointer dragging adjusts it relatively.
/// Keep fractional dB values: rounding belongs only in the displayed number.
pub(super) fn volume_slider(
    ui: &mut egui::Ui,
    id: &str,
    value: &mut f32,
    size: egui::Vec2,
) -> egui::Response {
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let mut response = ui.interact(rect, ui.make_persistent_id(id), egui::Sense::drag());
    let rail = egui::Rect::from_min_max(
        pos2(rect.left() + 5.0, rect.center().y - 1.5),
        pos2(rect.right() - 5.0, rect.center().y + 1.5),
    );
    let before = *value;
    let drag_id = response.id.with("volume-drag");
    let (focused, pointer, primary_down, pressed, interrupted) = ui.input(|i| {
        (
            i.focused,
            i.pointer.latest_pos(),
            i.pointer.primary_down(),
            i.pointer.button_pressed(egui::PointerButton::Primary),
            i.events.iter().any(|event| {
                matches!(
                    event,
                    egui::Event::PointerGone | egui::Event::WindowFocused(false)
                )
            }),
        )
    });
    let mut drag = ui.data_mut(|data| data.get_temp::<VolumeDrag>(drag_id));
    if !ui.is_enabled() || !primary_down || pointer.is_none() || interrupted {
        // A release outside the app may never arrive. Never let egui's retained
        // button-down state resume this gesture when focus/the pointer returns.
        drag = None;
    } else if response.is_pointer_button_down_on() && pressed {
        // macOS can deliver the activation press before the new window focus.
        // Keep this explicit press for the following focused frame. A stale
        // button-down alone still cannot start or resume a gesture.
        drag = pointer.map(|pointer| VolumeDrag { pointer });
    } else if !focused || !response.is_pointer_button_down_on() {
        drag = None;
    }
    let held = drag.is_some();
    if ui.is_enabled() {
        let fine = ui.input(|i| i.modifiers.shift);
        if response.clicked() || response.drag_started() {
            response.request_focus();
        }
        if response.has_focus() {
            ui.memory_mut(|memory| {
                memory.set_focus_lock_filter(
                    response.id,
                    egui::EventFilter {
                        horizontal_arrows: true,
                        ..Default::default()
                    },
                )
            });
        }
        if let (Some(active), Some(pointer)) = (&mut drag, pointer) {
            // Consume actual positions once, including across repeated layout
            // passes; a press starts here without inheriting the hover delta.
            *value +=
                (pointer.x - active.pointer.x) * 60.0 / rail.width() * if fine { 0.1 } else { 1.0 };
            active.pointer = pointer;
        }
        if response.hovered() {
            let delta = ui.input_mut(|i| {
                // egui maps Shift + vertical wheel to horizontal scrolling.
                if fine && i.smooth_scroll_delta.y == 0.0 {
                    std::mem::take(&mut i.smooth_scroll_delta.x)
                } else {
                    std::mem::take(&mut i.smooth_scroll_delta.y)
                }
            });
            *value += delta * if fine { 0.002 } else { 0.01 };
        }
        let keys = if response.has_focus() {
            ui.input_mut(|i| {
                i32::from(i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight))
                    - i32::from(i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft))
                    + i32::from(i.consume_key(egui::Modifiers::SHIFT, egui::Key::ArrowRight))
                    - i32::from(i.consume_key(egui::Modifiers::SHIFT, egui::Key::ArrowLeft))
            })
        } else {
            0
        };
        let actions = ui.input(|i| {
            i.num_accesskit_action_requests(response.id, egui::accesskit::Action::Increment) as i32
                - i.num_accesskit_action_requests(response.id, egui::accesskit::Action::Decrement)
                    as i32
        });
        *value += (keys + actions) as f32 * if fine { 0.1 } else { 0.5 };
        ui.input(|i| {
            for request in
                i.accesskit_action_requests(response.id, egui::accesskit::Action::SetValue)
            {
                if let Some(egui::accesskit::ActionData::NumericValue(db)) = request.data {
                    *value = db as f32;
                }
            }
        });
        *value = super::valid_volume(*value);
    }
    ui.data_mut(|data| {
        if let Some(drag) = drag {
            data.insert_temp(drag_id, drag);
        } else {
            data.remove::<VolumeDrag>(drag_id);
        }
    });
    if *value != before {
        response.mark_changed();
    }
    let fraction = (*value + 60.0) / 60.0;
    let color = if response.hovered() || response.dragged() || response.has_focus() {
        palette::accent()
    } else {
        palette::muted()
    };
    ui.painter().rect_filled(rail, 1.5, palette::line());
    let x = rail.left() + rail.width() * fraction;
    ui.painter().rect_filled(
        egui::Rect::from_min_max(rail.min, pos2(x, rail.bottom())),
        1.5,
        color,
    );
    ui.painter().circle_filled(
        pos2(x, rail.center().y),
        if response.dragged() { 5.0 } else { 4.0 },
        color,
    );
    response.widget_info(|| {
        egui::WidgetInfo::slider(ui.is_enabled(), f64::from(*value), tr!("Volume"))
    });
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_min_numeric_value(-60.0);
        node.set_max_numeric_value(0.0);
        node.set_numeric_value_step(0.5);
        if ui.is_enabled() {
            node.add_action(egui::accesskit::Action::SetValue);
            if *value < 0.0 {
                node.add_action(egui::accesskit::Action::Increment);
            }
            if *value > -60.0 {
                node.add_action(egui::accesskit::Action::Decrement);
            }
        }
    });
    response.on_hover_and_drag_cursor(if held {
        egui::CursorIcon::None
    } else {
        egui::CursorIcon::ResizeHorizontal
    }).on_hover_text(format!("{:.1} dB\n{}",*value,tr!("Drag to adjust volume; hold Shift for fine control. Scroll or use arrow keys for small changes.")))
}

#[cfg(test)]
mod volume_tests {
    use super::*;
    fn frame(
        ctx: &egui::Context,
        value: &mut f32,
        events: Vec<egui::Event>,
        shift: bool,
        enabled: bool,
    ) -> egui::Id {
        frame_with_cursor(ctx, value, events, shift, enabled).0
    }
    fn frame_with_cursor(
        ctx: &egui::Context,
        value: &mut f32,
        events: Vec<egui::Event>,
        shift: bool,
        enabled: bool,
    ) -> (egui::Id, egui::CursorIcon) {
        frame_with_focus(ctx, value, events, shift, enabled, true)
    }
    fn frame_with_focus(
        ctx: &egui::Context,
        value: &mut f32,
        mut events: Vec<egui::Event>,
        shift: bool,
        enabled: bool,
        focused: bool,
    ) -> (egui::Id, egui::CursorIcon) {
        let mut target = egui::Id::new("pending-volume");
        events.insert(
            0,
            egui::Event::ModifiersChanged(egui::Modifiers {
                shift,
                ..Default::default()
            }),
        );
        let mut output = ctx.run_ui(
            egui::RawInput {
                focused,
                screen_rect: Some(egui::Rect::from_min_size(
                    pos2(0.0, 0.0),
                    vec2(300.0, 150.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                ui.add_enabled_ui(enabled, |ui| {
                    target = volume_slider(ui, "test-volume", value, vec2(100.0, 30.0)).id;
                });
            },
        );
        output.textures_delta.clear();
        (target, output.platform_output.cursor_icon)
    }
    fn pointer(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        }
    }
    #[test]
    fn volume_hover_shows_adjustment_cursor_before_any_press() {
        for focused in [false, true] {
            let ctx = egui::Context::default();
            let mut db = -30.0;
            frame_with_focus(&ctx, &mut db, vec![], false, true, focused);
            let (_, cursor) = frame_with_focus(
                &ctx,
                &mut db,
                vec![egui::Event::PointerMoved(pos2(50.0, 20.0))],
                false,
                true,
                focused,
            );
            assert_eq!(cursor, egui::CursorIcon::ResizeHorizontal);
            assert_eq!(db, -30.0);
        }
    }

    #[test]
    fn first_volume_press_can_gain_window_focus_without_losing_the_gesture() {
        for fine in [false, true] {
            let ctx = egui::Context::default();
            let mut db = -30.0;
            // A newly opened app is visible while another app is still active.
            frame_with_focus(&ctx, &mut db, vec![], fine, true, false);
            let start = pos2(50.0, 20.0);
            frame_with_focus(
                &ctx,
                &mut db,
                vec![egui::Event::PointerMoved(start), pointer(start, true)],
                fine,
                true,
                false,
            );
            assert_eq!(db, -30.0, "the activation press must not jump the volume");
            frame_with_focus(
                &ctx,
                &mut db,
                vec![egui::Event::WindowFocused(true)],
                fine,
                true,
                true,
            );
            frame_with_focus(
                &ctx,
                &mut db,
                vec![egui::Event::PointerMoved(start + vec2(5.0, 0.0))],
                fine,
                true,
                true,
            );
            let expected = -30.0 + 5.0 * 60.0 / 90.0 * if fine { 0.1 } else { 1.0 };
            assert!(
                (db - expected).abs() < 0.001,
                "first gesture stayed blocked: {db}"
            );
        }
    }

    #[test]
    fn an_activation_press_without_focus_cannot_resume_later() {
        for cancel in [
            None,
            Some(egui::Event::PointerGone),
            Some(egui::Event::WindowFocused(false)),
        ] {
            let ctx = egui::Context::default();
            let mut db = -30.0;
            let start = pos2(50.0, 20.0);
            frame_with_focus(&ctx, &mut db, vec![], false, true, false);
            frame_with_focus(
                &ctx,
                &mut db,
                vec![egui::Event::PointerMoved(start), pointer(start, true)],
                false,
                true,
                false,
            );
            // Only bridge the activation frame; do not keep an unfocused gesture.
            frame_with_focus(
                &ctx,
                &mut db,
                cancel.into_iter().collect(),
                false,
                true,
                false,
            );
            frame_with_focus(
                &ctx,
                &mut db,
                vec![
                    egui::Event::WindowFocused(true),
                    egui::Event::PointerMoved(pos2(95.0, 20.0)),
                ],
                false,
                true,
                true,
            );
            assert_eq!(db, -30.0);
        }
    }

    #[test]
    fn leaving_window_cancels_drag_until_a_fresh_press_even_if_release_is_missing() {
        let ctx = egui::Context::default();
        let mut db = -30.0;
        frame(&ctx, &mut db, vec![], false, true);
        let start = pos2(50.0, 20.0);
        frame(
            &ctx,
            &mut db,
            vec![egui::Event::PointerMoved(start), pointer(start, true)],
            false,
            true,
        );
        frame(
            &ctx,
            &mut db,
            vec![egui::Event::PointerMoved(start + vec2(5.0, 0.0))],
            false,
            true,
        );
        let before = db;
        let (_, cursor) =
            frame_with_cursor(&ctx, &mut db, vec![egui::Event::PointerGone], false, true);
        assert_ne!(
            cursor,
            egui::CursorIcon::None,
            "restore cursor when leaving the window"
        );
        for x in [250.0, 90.0, 50.0] {
            frame(
                &ctx,
                &mut db,
                vec![egui::Event::PointerMoved(pos2(x, 20.0))],
                false,
                true,
            );
            assert_eq!(
                db, before,
                "stale button-down must never resume a cancelled drag"
            );
        }
        frame(&ctx, &mut db, vec![pointer(start, false)], false, true);
        frame(&ctx, &mut db, vec![pointer(start, true)], false, true);
        frame(
            &ctx,
            &mut db,
            vec![egui::Event::PointerMoved(start + vec2(5.0, 0.0))],
            false,
            true,
        );
        assert!(db > before, "a new press must enable adjustment again");
    }
    #[test]
    fn refocusing_does_not_resume_a_drag_released_outside_the_app() {
        let ctx = egui::Context::default();
        let mut db = -30.0;
        frame(&ctx, &mut db, vec![], false, true);
        let start = pos2(50.0, 20.0);
        frame(
            &ctx,
            &mut db,
            vec![egui::Event::PointerMoved(start), pointer(start, true)],
            false,
            true,
        );
        frame_with_focus(
            &ctx,
            &mut db,
            vec![egui::Event::WindowFocused(false)],
            false,
            true,
            false,
        );
        frame_with_focus(
            &ctx,
            &mut db,
            vec![
                egui::Event::WindowFocused(true),
                egui::Event::PointerMoved(pos2(250.0, 20.0)),
            ],
            false,
            true,
            true,
        );
        assert_eq!(
            db, -30.0,
            "refocusing without a fresh press must not change volume"
        );
    }
    #[test]
    fn release_and_rapid_focus_changes_never_apply_late_motion() {
        for interrupted in [false, true] {
            let ctx = egui::Context::default();
            let mut db = -30.0;
            frame(&ctx, &mut db, vec![], false, true);
            let start = pos2(50.0, 20.0);
            frame(
                &ctx,
                &mut db,
                vec![egui::Event::PointerMoved(start), pointer(start, true)],
                false,
                true,
            );
            let mut events = vec![egui::Event::PointerMoved(pos2(250.0, 20.0))];
            if interrupted {
                events.extend([
                    egui::Event::WindowFocused(false),
                    egui::Event::WindowFocused(true),
                ]);
            } else {
                events.push(pointer(pos2(250.0, 20.0), false));
            }
            let (_, cursor) = frame_with_cursor(&ctx, &mut db, events, false, true);
            assert_ne!(cursor, egui::CursorIcon::None);
            assert_eq!(db, -30.0);
            for x in [90.0, 10.0, 250.0] {
                frame(
                    &ctx,
                    &mut db,
                    vec![egui::Event::PointerMoved(pos2(x, 20.0))],
                    false,
                    true,
                );
                assert_eq!(db, -30.0);
            }
        }
    }
    #[test]
    fn repeated_layout_passes_apply_each_mouse_movement_only_once() {
        let ctx = egui::Context::default();
        let mut db = -30.0;
        frame(&ctx, &mut db, vec![], false, true);
        let start = pos2(50.0, 20.0);
        frame(
            &ctx,
            &mut db,
            vec![egui::Event::PointerMoved(start), pointer(start, true)],
            false,
            true,
        );
        let mut passes = 0;
        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    pos2(0.0, 0.0),
                    vec2(300.0, 150.0),
                )),
                events: vec![egui::Event::PointerMoved(start + vec2(5.0, 0.0))],
                ..Default::default()
            },
            |ui| {
                ui.add_enabled_ui(true, |ui| {
                    volume_slider(ui, "test-volume", &mut db, vec2(100.0, 30.0));
                });
                passes += 1;
                if passes == 1 {
                    ctx.request_discard("volume regression: second layout pass");
                }
            },
        );
        assert!(passes >= 2);
        assert!(
            (db - (-30.0 + 5.0 * 60.0 / 90.0)).abs() < 0.001,
            "motion applied more than once: {db}"
        );
    }
    #[test]
    fn press_and_release_anywhere_preserve_exact_level_and_restore_cursor() {
        for x in [7.0, 50.0, 94.0] {
            let ctx = egui::Context::default();
            let mut db = -38.34375;
            frame(&ctx, &mut db, vec![], false, true);
            let p = pos2(x, 20.0);
            let (_, cursor) = frame_with_cursor(
                &ctx,
                &mut db,
                vec![egui::Event::PointerMoved(p), pointer(p, true)],
                false,
                true,
            );
            assert_eq!(db, -38.34375, "pressing must preserve the exact level");
            assert_eq!(cursor, egui::CursorIcon::None, "hide immediately on press");
            let (_, cursor) =
                frame_with_cursor(&ctx, &mut db, vec![pointer(p, false)], false, true);
            assert_eq!(db, -38.34375, "a click must never set an absolute level");
            assert_ne!(cursor, egui::CursorIcon::None, "restore on release");
        }
    }
    #[test]
    fn relative_drag_starts_at_current_level_and_shift_is_ten_times_finer() {
        for shift in [false, true] {
            let ctx = egui::Context::default();
            let mut db = -30.0;
            frame(&ctx, &mut db, vec![], false, true);
            let start = pos2(80.0, 20.0);
            frame(
                &ctx,
                &mut db,
                vec![egui::Event::PointerMoved(start), pointer(start, true)],
                shift,
                true,
            );
            for dx in [1.0, 10.0, -10.0, 0.0] {
                let (_, cursor) = frame_with_cursor(
                    &ctx,
                    &mut db,
                    vec![egui::Event::PointerMoved(start + vec2(dx, 0.0))],
                    shift,
                    true,
                );
                let expected = -30.0 + dx * 60.0 / 90.0 * if shift { 0.1 } else { 1.0 };
                assert!(
                    (db - expected).abs() < 0.001,
                    "shift={shift}, dx={dx}: {db}"
                );
                assert_eq!(cursor, egui::CursorIcon::None);
            }
            frame(&ctx, &mut db, vec![pointer(start, false)], shift, true);
            assert!(
                (db + 30.0).abs() < 0.001,
                "returning to the starting point restores the level"
            );
        }
    }
    #[test]
    fn disabling_during_drag_restores_cursor_and_stops_adjustment() {
        let ctx = egui::Context::default();
        let mut db = -30.0;
        frame(&ctx, &mut db, vec![], false, true);
        let p = pos2(80.0, 20.0);
        frame(
            &ctx,
            &mut db,
            vec![egui::Event::PointerMoved(p), pointer(p, true)],
            false,
            true,
        );
        let (_, cursor) = frame_with_cursor(
            &ctx,
            &mut db,
            vec![egui::Event::PointerMoved(pos2(90.0, 20.0))],
            false,
            false,
        );
        assert_eq!(db, -30.0);
        assert_ne!(cursor, egui::CursorIcon::None);
    }
    #[test]
    fn volume_is_bounded_and_disabled_controls_do_not_change_the_gain() {
        let ctx = egui::Context::default();
        let mut db = -1.0;
        frame(&ctx, &mut db, vec![], false, true);
        let start = pos2(50.0, 20.0);
        frame(
            &ctx,
            &mut db,
            vec![egui::Event::PointerMoved(start), pointer(start, true)],
            false,
            true,
        );
        frame(
            &ctx,
            &mut db,
            vec![egui::Event::PointerMoved(start + vec2(600.0, 0.0))],
            false,
            true,
        );
        assert_eq!(db, 0.0);
        frame(
            &ctx,
            &mut db,
            vec![egui::Event::PointerMoved(start - vec2(600.0, 0.0))],
            false,
            true,
        );
        assert_eq!(db, -60.0);
        let ctx = egui::Context::default();
        db = -23.5;
        frame(&ctx, &mut db, vec![], false, false);
        frame(
            &ctx,
            &mut db,
            vec![egui::Event::PointerMoved(start), pointer(start, true)],
            false,
            false,
        );
        frame(
            &ctx,
            &mut db,
            vec![
                egui::Event::PointerMoved(start + vec2(100.0, 0.0)),
                pointer(start + vec2(100.0, 0.0), false),
            ],
            false,
            false,
        );
        assert_eq!(db, -23.5);
    }
    #[test]
    fn focused_arrow_keys_adjust_by_half_a_db_or_a_tenth_with_shift() {
        let ctx = egui::Context::default();
        let mut db = -30.0;
        frame(&ctx, &mut db, vec![], false, true);
        let pos = pos2(50.0, 20.0);
        frame(
            &ctx,
            &mut db,
            vec![egui::Event::PointerMoved(pos), pointer(pos, true)],
            false,
            true,
        );
        frame(&ctx, &mut db, vec![pointer(pos, false)], false, true);
        db = -30.0;
        for (shift, expected) in [(false, -29.5), (true, -29.4)] {
            let modifiers = egui::Modifiers {
                shift,
                ..Default::default()
            };
            let target = frame(
                &ctx,
                &mut db,
                vec![egui::Event::Key {
                    key: egui::Key::ArrowRight,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers,
                }],
                shift,
                true,
            );
            assert!((db - expected).abs() < 0.001, "shift={shift}: {db}");
            assert_eq!(ctx.memory(|m| m.focused()), Some(target));
        }
    }
    #[test]
    fn wheel_adjustment_is_small_and_shift_makes_it_finer() {
        let mut changes = Vec::new();
        for shift in [false, true] {
            let ctx = egui::Context::default();
            let mut db = -30.0;
            frame(&ctx, &mut db, vec![], shift, true);
            frame(
                &ctx,
                &mut db,
                vec![
                    egui::Event::PointerMoved(pos2(50.0, 20.0)),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: vec2(0.0, 50.0),
                        phase: egui::TouchPhase::Move,
                        modifiers: egui::Modifiers {
                            shift,
                            ..Default::default()
                        },
                    },
                ],
                shift,
                true,
            );
            for _ in 0..120 {
                frame(&ctx, &mut db, vec![], shift, true);
            }
            changes.push(db + 30.0);
        }
        assert!(changes[0] > 0.0 && changes[0] < 1.0, "{changes:?}");
        assert!((changes[1] - changes[0] * 0.2).abs() < 0.001, "{changes:?}");
    }
}
