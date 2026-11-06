use super::*;

#[derive(Default)]
pub(super) struct State {
    area: Option<egui::Rect>,
    candidate_area: Option<(egui::Rect, Instant)>,
    observed: Option<egui::Rect>,
    checked: Option<Instant>,
    monitor: Option<egui::Vec2>,
    pending: Option<(bool, egui::Vec2, Instant)>,
    fitted: Option<(bool, egui::Vec2)>,
}

fn close(a: egui::Vec2, b: egui::Vec2) -> bool {
    (a - b).abs().max_elem() < 2.0
}

fn same_area(a: egui::Rect, b: egui::Rect) -> bool {
    close(a.min.to_vec2(), b.min.to_vec2()) && close(a.max.to_vec2(), b.max.to_vec2())
}
fn adjusted_position(
    current: egui::Pos2,
    size: egui::Vec2,
    usable: egui::Rect,
    chrome: egui::Vec2,
) -> Option<egui::Pos2> {
    let maximum = usable.max - size - chrome;
    let adjusted = pos2(
        current.x.clamp(usable.min.x, maximum.x.max(usable.min.x)),
        current.y.clamp(usable.min.y, maximum.y.max(usable.min.y)),
    );
    (!close(current.to_vec2(), adjusted.to_vec2())).then_some(adjusted)
}
impl State {
    fn observe_area(&mut self, measured: egui::Rect, now: Instant, immediate: bool) -> bool {
        if immediate || self.area.is_none() {
            let changed = self.area.is_some_and(|old| !same_area(old, measured));
            self.area = Some(measured);
            self.candidate_area = None;
            return changed;
        }
        if same_area(self.area.unwrap(), measured) {
            self.candidate_area = None;
            return false;
        }
        if let Some((candidate, since)) = self.candidate_area
            && same_area(candidate, measured)
        {
            if now.duration_since(since) >= Duration::from_millis(350) {
                self.area = Some(measured);
                self.candidate_area = None;
                return true;
            }
        } else {
            self.candidate_area = Some((measured, now));
        }
        false
    }
    pub(super) fn remember(&mut self, mini: bool, size: egui::Vec2) -> bool {
        if let Some((mode, target, issued)) = self.pending
            && mode == mini
        {
            if close(size, target) || issued.elapsed() > Duration::from_secs(2) {
                self.pending = None;
            } else {
                return false;
            }
        }
        !self
            .fitted
            .is_some_and(|(mode, fitted)| mode == mini && close(size, fitted))
    }
}

fn fit(preferred: egui::Vec2, area: egui::Rect, chrome: egui::Vec2) -> (egui::Vec2, egui::Rect) {
    let margin = 24.0_f32.min((area.width().min(area.height()) / 4.0).max(0.0));
    let usable = area.shrink(margin);
    let maximum = (usable.size() - chrome).max(vec2(1.0, 1.0));
    (preferred.min(maximum).max(vec2(1.0, 1.0)), usable)
}

fn work_area(ctx: &egui::Context) -> egui::Rect {
    #[cfg(all(target_os = "macos", not(test)))]
    if let Some(area) = native_work_area(ctx) {
        return area;
    }
    let size = ctx
        .input(|i| i.viewport().monitor_size)
        .unwrap_or(vec2(1280.0, 900.0));
    egui::Rect::from_min_size(pos2(0.0, 0.0), size)
}

#[cfg(all(target_os = "macos", not(test)))]
fn native_work_area(ctx: &egui::Context) -> Option<egui::Rect> {
    let mtm = objc2::MainThreadMarker::new()?;
    let screens = objc2_app_kit::NSScreen::screens(mtm);
    let primary = screens.firstObject()?.frame();
    let top = primary.origin.y + primary.size.height;
    let scale = ctx.zoom_factor();
    let convert = |x: f64, y: f64, w: f64, h: f64| {
        egui::Rect::from_min_size(
            pos2(x as f32 / scale, (top - y - h) as f32 / scale),
            vec2(w as f32 / scale, h as f32 / scale),
        )
    };
    let window = ctx.input(|i| i.viewport().outer_rect.or(i.viewport().inner_rect));
    if window.is_none() {
        let v = screens.firstObject()?.visibleFrame();
        return Some(convert(v.origin.x, v.origin.y, v.size.width, v.size.height));
    }
    screens
        .iter()
        .map(|screen| {
            let f = screen.frame();
            let frame = convert(f.origin.x, f.origin.y, f.size.width, f.size.height);
            let v = screen.visibleFrame();
            let visible = convert(v.origin.x, v.origin.y, v.size.width, v.size.height);
            let overlap = window.map_or(0.0, |w| {
                w.intersect(frame).size().max(egui::Vec2::ZERO).x
                    * w.intersect(frame).size().max(egui::Vec2::ZERO).y
            });
            let distance = window.map_or(0.0, |w| w.center().distance_sq(frame.center()));
            (visible, overlap, distance)
        })
        .max_by(|a, b| a.1.total_cmp(&b.1).then_with(|| b.2.total_cmp(&a.2)))
        .map(|(v, _, _)| v)
}

impl Aurea {
    pub(super) fn resize_window(&mut self, ctx: &egui::Context) {
        self.resize_window_with_position(ctx, true);
    }

    fn resize_window_with_position(&mut self, ctx: &egui::Context, position_window: bool) {
        let mini = self.saved.mini;
        let area = self.window_geometry.area.unwrap_or_else(|| work_area(ctx));
        let preferred = if mini {
            self.mini_size()
        } else {
            self.full_size()
        };
        let chrome = if mini {
            egui::Vec2::ZERO
        } else {
            vec2(0.0, 32.0)
        };
        let (size, usable) = fit(preferred, area, chrome);
        let minimum = if mini {
            vec2(
                520.0,
                if self.saved.mini_view == 0 {
                    262.0
                } else {
                    520.0
                },
            )
        } else {
            vec2(640.0, 440.0)
        };
        let observed_position = ctx.input(|i| {
            i.viewport()
                .outer_rect
                .or(i.viewport().inner_rect)
                .map(|r| r.min)
        });
        let position = observed_position.unwrap_or_else(|| usable.center() - size / 2.0);
        ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(minimum.min(size)));
        if !ctx
            .input(|i| i.viewport().inner_rect.map(|r| r.size()))
            .is_some_and(|current| close(current, size))
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
        }
        if position_window
            && let Some(position) = adjusted_position(position, size, usable, chrome)
                .or_else(|| observed_position.is_none().then_some(position))
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(position));
        }
        self.window_geometry.pending = Some((mini, size, Instant::now()));
        self.window_geometry.fitted = (!close(preferred, size)).then_some((mini, size));
    }

    pub(super) fn sync_window_geometry(&mut self, ctx: &egui::Context) {
        let observed = ctx.input(|i| i.viewport().inner_rect);
        let monitor = ctx.input(|i| i.viewport().monitor_size);
        let mode_changed = self.viewport_mini != Some(self.saved.mini);
        let moved = observed != self.window_geometry.observed;
        #[cfg(debug_assertions)]
        if moved || mode_changed {
            super::window_drag::trace(
                serde_json::json!({"stage":"geometry", "mini":self.saved.mini,"mode_changed":mode_changed,"inner":observed.map(|r|[r.min.x,r.min.y,r.width(),r.height()]),"outer":ctx.input(|i|i.viewport().outer_rect).map(|r|[r.min.x,r.min.y,r.width(),r.height()])}),
            );
        }
        let check = mode_changed
            || monitor != self.window_geometry.monitor
            || moved
            || self
                .window_geometry
                .checked
                .is_none_or(|t| t.elapsed() > Duration::from_secs(1));
        let area = if check {
            work_area(ctx)
        } else {
            self.window_geometry.area.unwrap()
        };
        let screen_changed = check
            && self.window_geometry.observe_area(
                area,
                Instant::now(),
                mode_changed || monitor != self.window_geometry.monitor,
            );
        if check {
            self.window_geometry.checked = Some(Instant::now());
        }
        if let Some(size) = observed.map(|r| r.size()) {
            if mode_changed && !screen_changed {
                match self.viewport_mini {
                    Some(true) => self.remember_mini_size(size),
                    Some(false) => self.remember_full_size(size),
                    None if self.saved.mini && self.saved.mini_size.is_none() => {
                        self.remember_mini_size(size)
                    }
                    None if !self.saved.mini && self.saved.full_size.is_none() => {
                        self.remember_full_size(size)
                    }
                    None => {}
                }
            } else if moved && !screen_changed {
                if self.saved.mini {
                    self.remember_mini_size(size);
                } else {
                    self.remember_full_size(size);
                }
            }
        }
        self.window_geometry.monitor = monitor;
        self.window_geometry.observed = observed;
        if mode_changed {
            super::window_drag::reset(ctx);
            ctx.send_viewport_cmd(egui::ViewportCommand::Transparent(self.saved.mini));
            ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(!self.saved.mini));
            self.resize_window(ctx);
            self.viewport_mini = Some(self.saved.mini);
        } else if screen_changed {
            // A monitor or Dock change can follow a native drag. Fit the size
            // when needed, but retain the position chosen by the user and do
            // not reapply macOS window styles while the window is moving.
            self.resize_window_with_position(ctx, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(app: &mut Aurea, size: egui::Vec2, monitor: egui::Vec2) -> egui::FullOutput {
        let ctx = app.ctx.clone();
        let rect = egui::Rect::from_min_size(pos2(40.0, 40.0), size);
        let mut input = egui::RawInput {
            screen_rect: Some(rect),
            ..Default::default()
        };
        let info = input.viewports.get_mut(&egui::ViewportId::ROOT).unwrap();
        info.inner_rect = Some(rect);
        info.outer_rect = Some(rect);
        info.monitor_size = Some(monitor);
        ctx.begin_pass(input);
        app.sync_window_geometry(&ctx);
        let mut output = ctx.end_pass();
        output.textures_delta.clear();
        output
    }
    #[track_caller]
    fn requested(output: &egui::FullOutput) -> egui::Vec2 {
        output
            .viewport_output
            .values()
            .flat_map(|v| &v.commands)
            .find_map(|c| {
                if let egui::ViewportCommand::InnerSize(size) = c {
                    Some(*size)
                } else {
                    None
                }
            })
            .unwrap()
    }

    #[test]
    fn changing_screens_fits_size_without_snapping_the_dragged_position_or_resetting_styles() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx);
        let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
        let size = vec2(812.0, 640.0);
        frame(&mut app, size, vec2(2000.0, 1200.0));
        let output = frame(&mut app, size, vec2(800.0, 600.0));
        assert_eq!(requested(&output), vec2(752.0, 552.0));
        assert!(
            output
                .viewport_output
                .values()
                .flat_map(|v| &v.commands)
                .all(|command| !matches!(
                    command,
                    egui::ViewportCommand::OuterPosition(_)
                        | egui::ViewportCommand::Transparent(_)
                        | egui::ViewportCommand::Decorations(_)
                ))
        );
    }

    #[test]
    fn full_and_mini_keep_independent_sizes_and_shrink_without_losing_preferences() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx);
        let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
        let large = vec2(2000.0, 1200.0);
        app.saved.mini = false;
        let initial = frame(&mut app, vec2(1380.0, 840.0), large);
        assert!(
            initial
                .viewport_output
                .values()
                .flat_map(|v| &v.commands)
                .all(|c| !matches!(
                    c,
                    egui::ViewportCommand::InnerSize(_) | egui::ViewportCommand::OuterPosition(_)
                ))
        );
        assert_eq!(app.full_size(), vec2(1380.0, 840.0));
        frame(&mut app, vec2(1380.0, 840.0), large);
        app.saved.mini = true;
        frame(&mut app, vec2(1380.0, 840.0), large);
        frame(&mut app, vec2(520.0, 520.0), large);
        frame(&mut app, vec2(812.0, 640.0), large);
        app.saved.mini = false;
        assert_eq!(
            requested(&frame(&mut app, vec2(812.0, 640.0), large)),
            vec2(1380.0, 840.0)
        );
        frame(&mut app, vec2(1380.0, 840.0), large);
        app.saved.mini = true;
        assert_eq!(
            requested(&frame(&mut app, vec2(1380.0, 840.0), large)),
            vec2(812.0, 640.0)
        );
        frame(&mut app, vec2(812.0, 640.0), large);
        let small = vec2(800.0, 600.0);
        let fitted = requested(&frame(&mut app, vec2(812.0, 640.0), small));
        assert_eq!(fitted, vec2(752.0, 552.0));
        frame(&mut app, fitted, small);
        app.remember_mini_size(fitted); // also used by save and tab changes
        assert_eq!(app.mini_size(), vec2(812.0, 640.0));
        assert_eq!(
            requested(&frame(&mut app, fitted, large)),
            vec2(812.0, 640.0)
        );
        let restored: Saved =
            serde_json::from_value(serde_json::to_value(&app.saved).unwrap()).unwrap();
        assert_eq!(restored.full_size, Some([1380.0, 840.0]));
        assert_eq!(restored.mini_size, Some([812.0, 640.0]));
    }

    #[test]
    fn tiny_or_transient_work_area_changes_do_not_reposition_the_window() {
        let start = Instant::now();
        let area = egui::Rect::from_min_size(pos2(0.0, 25.0), vec2(1440.0, 850.0));
        let mut state = State::default();
        state.observe_area(area, start, true);
        for (i, delta) in [0.5, -0.5, 1.0, 0.0].into_iter().enumerate() {
            assert!(!state.observe_area(
                area.translate(vec2(0.0, delta)),
                start + Duration::from_millis(i as u64 * 20),
                false
            ));
        }
        let dock = egui::Rect::from_min_max(area.min, area.max - vec2(0.0, 60.0));
        assert!(!state.observe_area(dock, start + Duration::from_millis(100), false));
        assert!(!state.observe_area(area, start + Duration::from_millis(200), false));
        assert_eq!(state.area, Some(area));
        assert!(!state.observe_area(dock, start + Duration::from_millis(300), false));
        assert!(state.observe_area(dock, start + Duration::from_millis(700), false));
        assert_eq!(state.area, Some(dock));
        let other_screen = area.translate(vec2(-1440.0, 0.0));
        assert!(state.observe_area(other_screen, start + Duration::from_millis(710), true));
    }
    #[test]
    fn animation_frames_do_not_reset_a_pending_work_area_measurement() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx);
        let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
        let size = vec2(812.0, 640.0);
        let monitor = vec2(2000.0, 1200.0);
        frame(&mut app, size, monitor);
        let candidate = app.window_geometry.area.unwrap().shrink(5.0);
        app.window_geometry.candidate_area = Some((candidate, Instant::now()));
        frame(&mut app, size, monitor);
        assert_eq!(app.window_geometry.candidate_area.unwrap().0, candidate);
    }
    #[test]
    fn resizing_keeps_an_in_bounds_position_and_only_corrects_real_overflow() {
        let usable = egui::Rect::from_min_max(pos2(24.0, 49.0), pos2(1416.0, 876.0));
        let size = vec2(812.0, 640.0);
        assert_eq!(
            adjusted_position(pos2(100.0, 100.0), size, usable, egui::Vec2::ZERO),
            None
        );
        assert_eq!(
            adjusted_position(pos2(23.5, 100.0), size, usable, egui::Vec2::ZERO),
            None
        );
        assert_eq!(
            adjusted_position(pos2(800.0, 600.0), size, usable, egui::Vec2::ZERO),
            Some(pos2(604.0, 236.0))
        );
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx);
        let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
        let output = frame(&mut app, vec2(812.0, 640.0), vec2(2000.0, 1200.0));
        assert!(
            output
                .viewport_output
                .values()
                .flat_map(|v| &v.commands)
                .all(|cmd| !matches!(
                    cmd,
                    egui::ViewportCommand::OuterPosition(_) | egui::ViewportCommand::InnerSize(_)
                ))
        );
    }
    #[test]
    fn fit_respects_visible_area_chrome_and_negative_monitor_origins() {
        let area = egui::Rect::from_min_size(pos2(-1600.0, -80.0), vec2(1600.0, 950.0));
        let (size, usable) = fit(vec2(2000.0, 1200.0), area, vec2(0.0, 32.0));
        assert_eq!(size, vec2(1552.0, 870.0));
        assert_eq!(usable.min, pos2(-1576.0, -56.0));
        assert_eq!(usable.max, pos2(-24.0, 846.0));
        let (tiny, _) = fit(
            vec2(520.0, 520.0),
            egui::Rect::from_min_size(pos2(0.0, 0.0), vec2(480.0, 420.0)),
            egui::Vec2::ZERO,
        );
        assert_eq!(tiny, vec2(432.0, 372.0));
    }
}
