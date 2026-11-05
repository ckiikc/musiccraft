//! One palette shared by the player, menus and auxiliary windows.
use super::*;
use std::cell::Cell;
thread_local! {static THEME:Cell<u8>=const{Cell::new(0)};}
pub(super) fn current() -> u8 {
    THEME.with(Cell::get)
}
fn colors() -> [Color32; 8] {
    let rgb = match current() {
        1 => [
            (19, 19, 21),
            (36, 36, 38),
            (245, 242, 237),
            (163, 161, 153),
            (255, 148, 64),
            (44, 44, 46),
            (58, 58, 61),
            (67, 46, 32),
        ],
        2 => [
            (230, 237, 242),
            (251, 253, 255),
            (20, 31, 41),
            (79, 97, 110),
            (5, 122, 158),
            (219, 229, 237),
            (181, 194, 204),
            (199, 225, 237),
        ],
        _ => [
            (17, 21, 25),
            (25, 30, 35),
            (230, 234, 238),
            (143, 153, 160),
            (219, 175, 116),
            (33, 39, 45),
            (48, 55, 62),
            (57, 49, 38),
        ],
    };
    rgb.map(|(r, g, b)| Color32::from_rgb(r, g, b))
}
pub(super) fn bg() -> Color32 {
    colors()[0]
}
pub(super) fn panel() -> Color32 {
    colors()[1]
}
pub(super) fn text() -> Color32 {
    colors()[2]
}
pub(super) fn muted() -> Color32 {
    colors()[3]
}
pub(super) fn accent() -> Color32 {
    colors()[4]
}
pub(super) fn surface() -> Color32 {
    colors()[5]
}
pub(super) fn line() -> Color32 {
    colors()[6]
}
pub(super) fn selected() -> Color32 {
    colors()[7]
}
pub(super) fn configure(ctx: &egui::Context, theme: u8) {
    THEME.with(|t| t.set(theme % 3));
    let native = if current() == 2 {
        egui::Theme::Light
    } else {
        egui::Theme::Dark
    };
    let mut style = (*ctx.style_of(native)).clone();
    style.visuals = if current() == 2 {
        egui::Visuals::light()
    } else {
        egui::Visuals::dark()
    };
    style.visuals.panel_fill = bg();
    style.visuals.window_fill = panel();
    style.visuals.selection.bg_fill = selected();
    style.visuals.selection.stroke.color = accent();
    style.visuals.widgets.active.bg_fill = selected();
    style.visuals.widgets.hovered.bg_fill = surface();
    style.visuals.widgets.inactive.bg_fill = surface();
    style.visuals.widgets.inactive.weak_bg_fill = surface();
    style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, line());
    style.visuals.widgets.inactive.fg_stroke.color = text();
    style.visuals.widgets.noninteractive.fg_stroke.color = text();
    style.visuals.widgets.noninteractive.bg_stroke.color = line();
    style.visuals.window_stroke = egui::Stroke::new(1.0, line());
    style.visuals.window_corner_radius = 10.into();
    style.visuals.text_edit_bg_color = Some(bg());
    style.visuals.slider_trailing_fill = true;
    style.spacing.item_spacing = vec2(8.0, 8.0);
    style.spacing.button_padding = vec2(10.0, 6.0);
    style.spacing.interact_size = vec2(32.0, 28.0);
    style.spacing.scroll = egui::style::ScrollStyle::solid();
    style.spacing.scroll.bar_width = 6.0;
    for (role, size) in [
        (egui::TextStyle::Body, 13.0),
        (egui::TextStyle::Button, 12.0),
        (egui::TextStyle::Small, 11.0),
    ] {
        style
            .text_styles
            .insert(role, egui::FontId::proportional(size));
    }
    ctx.set_theme(native);
    ctx.set_style_of(native, style);
}
