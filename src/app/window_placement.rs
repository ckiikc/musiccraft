//! Native auxiliary-window placement in desktop points, independent of UI zoom.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct Placement {
    position: [f32; 2],
    size: [f32; 2],
}

impl Placement {
    fn valid(self) -> bool {
        self.position
            .iter()
            .all(|n| n.is_finite() && n.abs() < 1.0e7)
            && self
                .size
                .iter()
                .all(|n| n.is_finite() && (64.0..=16384.0).contains(n))
    }
    fn rect(self) -> egui::Rect {
        egui::Rect::from_min_size(self.position.into(), self.size.into())
    }
}

#[derive(Default)]
pub(super) struct Tracker {
    preferred: Option<Placement>,
    initial: Option<Placement>,
    observed: Option<Placement>,
    position_sent: bool,
}

fn restore(
    preferred: Option<Placement>,
    size: egui::Vec2,
    parent: egui::Rect,
    areas: &[egui::Rect],
) -> Placement {
    if let Some(p) = preferred.filter(|p| p.valid()) {
        let title = egui::Rect::from_min_size(p.rect().min, vec2(p.size[0], 32.0));
        if areas.iter().any(|area| {
            let visible = title.intersect(*area).size();
            visible.x >= 48.0 && visible.y >= 16.0
        }) {
            return p;
        }
    }
    let area = areas
        .iter()
        .copied()
        .min_by(|a, b| {
            a.distance_sq_to_pos(parent.center())
                .total_cmp(&b.distance_sq_to_pos(parent.center()))
        })
        .unwrap_or_else(|| egui::Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 900.0)));
    let usable = area.shrink(16.0);
    let preferred_size = preferred
        .filter(|p| p.valid())
        .map_or(size, |p| p.size.into());
    let size = preferred_size.min((usable.size() - vec2(0.0, 32.0)).max(vec2(64.0, 64.0)));
    let maximum = (usable.max - size - vec2(0.0, 32.0)).max(usable.min);
    let position = (parent.center() - size / 2.0).clamp(usable.min, maximum);
    Placement {
        position: position.into(),
        size: size.into(),
    }
}

fn desktop_areas(ctx: &egui::Context) -> Vec<egui::Rect> {
    #[cfg(all(target_os = "macos", not(test)))]
    if let Some(mtm) = objc2::MainThreadMarker::new() {
        let screens = objc2_app_kit::NSScreen::screens(mtm);
        if let Some(primary) = screens.firstObject() {
            let primary = primary.frame();
            let top = primary.origin.y + primary.size.height;
            return screens
                .iter()
                .map(|screen| {
                    let v = screen.visibleFrame();
                    egui::Rect::from_min_size(
                        pos2(v.origin.x as f32, (top - v.origin.y - v.size.height) as f32),
                        vec2(v.size.width as f32, v.size.height as f32),
                    )
                })
                .collect();
        }
    }
    vec![egui::Rect::from_min_size(
        pos2(0.0, 0.0),
        ctx.input(|i| i.viewport().monitor_size)
            .unwrap_or(vec2(1280.0, 900.0))
            * ctx.zoom_factor(),
    )]
}

impl Tracker {
    pub fn load(&mut self, placement: Option<Placement>) {
        self.preferred = placement.filter(|p| p.valid());
    }
    pub fn saved(&self) -> Option<Placement> {
        self.preferred.or(self.observed).or(self.initial)
    }
    pub fn open(&mut self, ctx: &egui::Context, size: egui::Vec2) {
        self.preferred = self.saved();
        let zoom = ctx.zoom_factor();
        let areas = desktop_areas(ctx);
        let parent = ctx
            .input(|i| i.viewport().outer_rect.or(i.viewport().inner_rect))
            .map(|r| egui::Rect::from_min_max(r.min * zoom, r.max * zoom))
            .unwrap_or_else(|| areas[0]);
        self.initial = Some(restore(self.preferred, size * zoom, parent, &areas));
        self.observed = None;
        self.position_sent = false;
    }
    fn observe_placement(&mut self, placement: Placement) {
        if !placement.valid() {
            return;
        }
        if self.observed.is_some_and(|previous| {
            (previous.rect().min - placement.rect().min)
                .abs()
                .max_elem()
                > 1.0
                || (previous.rect().size() - placement.rect().size())
                    .abs()
                    .max_elem()
                    > 1.0
        }) {
            self.preferred = Some(placement);
        }
        self.observed = Some(placement);
    }
    pub fn observe(&mut self, ctx: &egui::Context, viewport: egui::ViewportId) {
        if ctx.viewport_id() != viewport {
            return;
        }
        if let Some((outer, inner)) =
            ctx.input(|i| i.viewport().outer_rect.zip(i.viewport().inner_rect))
        {
            let zoom = ctx.zoom_factor();
            self.position_sent = true;
            self.observe_placement(Placement {
                position: (outer.min * zoom).into(),
                size: (inner.size() * zoom).into(),
            });
        }
    }
    pub fn builder(&mut self, ctx: &egui::Context, title: &str) -> egui::ViewportBuilder {
        let mut builder = egui::ViewportBuilder::default()
            .with_title(title)
            .with_transparent(false)
            .with_decorations(true)
            .with_titlebar_shown(true)
            .with_titlebar_buttons_shown(true)
            .with_fullsize_content_view(false)
            .with_clamp_size_to_monitor_size(true);
        if !self.position_sent
            && let Some(p) = self.initial
        {
            let zoom = ctx.zoom_factor();
            // Keep the requested position constant for this opening. Feeding
            // observed drag coordinates back to the builder causes native jumps.
            builder = builder
                .with_position(egui::Pos2::from(p.position) / zoom)
                .with_inner_size(egui::Vec2::from(p.size) / zoom);
        }
        builder
    }
    pub fn ready(&self) -> bool {
        self.initial.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn screens() -> [egui::Rect; 3] {
        [
            egui::Rect::from_min_size(pos2(-1440.0, 0.0), vec2(1440.0, 900.0)),
            egui::Rect::from_min_size(pos2(0.0, 0.0), vec2(1920.0, 1080.0)),
            egui::Rect::from_min_size(pos2(1920.0, -200.0), vec2(3440.0, 1440.0)),
        ]
    }
    #[test]
    fn initial_position_survives_parent_layout_passes_until_child_exists() {
        let ctx = egui::Context::default();
        let mut tracker = Tracker::default();
        tracker.open(&ctx, vec2(800.0, 760.0));
        let first = tracker.builder(&ctx, "Artwork");
        let second = tracker.builder(&ctx, "Artwork");
        assert_eq!(first.position, second.position);
        assert!(second.position.is_some());
        tracker.position_sent = true; // Native child has received its first frame.
        assert!(tracker.builder(&ctx, "Artwork").position.is_none());
    }
    #[test]
    fn cover_reopens_at_the_saved_position_on_each_monitor_after_serialization() {
        for position in [[-1200.0, 90.0], [200.0, 150.0], [2400.0, -100.0]] {
            let p = Placement {
                position,
                size: [800.0, 760.0],
            };
            let reopened: Placement =
                serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
            assert_eq!(
                restore(Some(reopened), vec2(800.0, 760.0), screens()[1], &screens()),
                p
            );
        }
    }
    #[test]
    fn disconnected_monitor_fallback_stays_visible_without_losing_the_preference() {
        let preferred = Placement {
            position: [2400.0, -100.0],
            size: [800.0, 760.0],
        };
        let fallback = restore(
            Some(preferred),
            vec2(800.0, 760.0),
            screens()[1],
            &screens()[1..2],
        );
        assert!(screens()[1].contains_rect(fallback.rect()));
        let mut tracker = Tracker::default();
        tracker.load(Some(preferred));
        tracker.observe_placement(fallback);
        tracker.observe_placement(fallback);
        assert_eq!(tracker.saved(), Some(preferred));
        let moved = Placement {
            position: [fallback.position[0] + 30.0, fallback.position[1]],
            ..fallback
        };
        tracker.observe_placement(moved);
        assert_eq!(tracker.saved(), Some(moved));
    }
    #[test]
    fn user_positions_near_the_edge_are_not_snapped_back_and_invalid_data_is_ignored() {
        let p = Placement {
            position: [-100.0, 100.0],
            size: [800.0, 760.0],
        };
        assert_eq!(
            restore(Some(p), vec2(800.0, 760.0), screens()[1], &screens()[1..2]),
            p
        );
        let mut tracker = Tracker::default();
        tracker.load(Some(Placement {
            position: [f32::NAN, 0.0],
            ..p
        }));
        assert!(tracker.saved().is_none());
    }
}
