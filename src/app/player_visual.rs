//! Lightweight, original vector artwork sharing the player's existing paint cadence.
use super::*;

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Appearance {
    Cd,
    Cassette,
    ReelToReel,
    VuMeters,
    VuMetersSideBySide,
    #[default]
    #[serde(other)]
    Vinyl,
}
impl Appearance {
    pub const ALL: [Self; 6] = [
        Self::Vinyl,
        Self::Cd,
        Self::Cassette,
        Self::ReelToReel,
        Self::VuMeters,
        Self::VuMetersSideBySide,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Vinyl => tr!("Vinyl"),
            Self::Cd => tr!("CD"),
            Self::Cassette => tr!("Cassette"),
            Self::ReelToReel => tr!("Revox"),
            Self::VuMeters => tr!("VU meters"),
            Self::VuMetersSideBySide => tr!("Side-by-side VU"),
        }
    }

    pub fn paint(self, painter: &egui::Painter, rect: egui::Rect, state: VisualState<'_>) {
        let painter = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
        match self {
            Self::Vinyl => turntable(&painter, rect, &state),
            Self::Cd => compact_disc(&painter, rect, &state),
            Self::Cassette => cassette(&painter, rect, &state),
            Self::ReelToReel => tape_deck(&painter, rect, &state),
            Self::VuMeters => stereo_meters(&painter, rect, &state, false),
            Self::VuMetersSideBySide => stereo_meters(&painter, rect, &state, true),
        }
    }
}

pub(super) struct VisualState<'a> {
    pub phase: f32,
    pub progress: f32,
    pub reels: [f32; 2],
    pub art: Option<&'a egui::TextureHandle>,
    pub album: &'a str,
    pub vu: [f32; 2],
}

fn turntable(p: &egui::Painter, rect: egui::Rect, state: &VisualState<'_>) {
    super::vinyl(p, rect, state.phase, state.art);
}

fn circular_art(p: &egui::Painter, center: egui::Pos2, radius: f32, state: &VisualState<'_>) {
    p.circle_filled(center, radius, palette::accent());
    if let Some(art) = state.art {
        let mut mesh = egui::Mesh::with_texture(art.id());
        mesh.vertices.push(egui::epaint::Vertex {
            pos: center,
            uv: pos2(0.5, 0.5),
            color: Color32::WHITE,
        });
        for i in 0..=64 {
            let angle = i as f32 * std::f32::consts::TAU / 64.0;
            mesh.vertices.push(egui::epaint::Vertex {
                pos: center
                    + vec2((angle + state.phase).cos(), (angle + state.phase).sin()) * radius,
                uv: pos2(0.5 + angle.cos() * 0.5, 0.5 + angle.sin() * 0.5),
                color: Color32::WHITE,
            });
            if i > 0 {
                mesh.indices.extend([0, i, i + 1]);
            }
        }
        p.add(egui::Shape::mesh(mesh));
    } else {
        p.text(
            center,
            egui::Align2::CENTER_CENTER,
            "MC",
            egui::FontId::proportional(radius * 0.46),
            palette::bg(),
        );
    }
}

fn compact_disc(p: &egui::Painter, rect: egui::Rect, state: &VisualState<'_>) {
    let center = rect.center();
    let radius = rect.width() * 0.46;
    let mut mesh = egui::Mesh::default();
    for i in 0..=96 {
        let theta = i as f32 * std::f32::consts::TAU / 96.0;
        let shimmer = theta - state.phase;
        let light = 190.0 + 34.0 * (shimmer * 2.0).cos();
        let color = Color32::from_rgb(
            (light + 18.0 * (shimmer * 5.0).sin()) as u8,
            (light + 18.0 * (shimmer * 5.0 + 2.1).sin()) as u8,
            (light + 18.0 * (shimmer * 5.0 + 4.2).sin()) as u8,
        );
        for distance in [radius * 0.12, radius] {
            mesh.vertices.push(egui::epaint::Vertex {
                pos: center + vec2(theta.cos(), theta.sin()) * distance,
                uv: egui::epaint::WHITE_UV,
                color,
            });
        }
        if i > 0 {
            let n = i * 2;
            mesh.indices.extend([n - 2, n - 1, n, n - 1, n + 1, n]);
        }
    }
    p.add(egui::Shape::mesh(mesh));
    p.circle_stroke(
        center,
        radius,
        egui::Stroke::new(1.0, Color32::from_gray(230)),
    );
    circular_art(p, center, radius * 0.86, state);
    p.circle_filled(center, radius * 0.12, Color32::from_gray(110));
    p.circle_stroke(
        center,
        radius * 0.16,
        egui::Stroke::new(1.0, Color32::from_gray(235)),
    );
    p.circle_filled(center, radius * 0.055, palette::bg());
}

fn tape_radius(progress: f32, hub: f32, outer: f32) -> f32 {
    (hub * hub + progress.clamp(0.0, 1.0) * (outer * outer - hub * hub)).sqrt()
}

#[derive(Default)]
pub(super) struct ReelMotion {
    pub angles: [f32; 2],
    pub progress: f32,
}
impl ReelMotion {
    pub fn advance(&mut self, elapsed: f32, speed: f32) {
        for (angle, fill) in self
            .angles
            .iter_mut()
            .zip([1.0 - self.progress, self.progress])
        {
            // Equal linear tape speed on both reels; integrate angles instead of
            // scaling the turntable's wrapped angle, which caused periodic jumps.
            let radius = tape_radius(fill, 0.27, 0.86);
            *angle = (*angle - elapsed * speed / radius).rem_euclid(std::f32::consts::TAU);
        }
    }
}

// Small meshes provide fixed studio lighting without raster assets or CPU filters.
fn gradient(p: &egui::Painter, r: egui::Rect, top: Color32, middle: Color32, bottom: Color32) {
    let mut mesh = egui::Mesh::default();
    for (y, color) in [(r.top(), top), (r.center().y, middle), (r.bottom(), bottom)] {
        for x in [r.left(), r.right()] {
            mesh.colored_vertex(pos2(x, y), color);
        }
    }
    mesh.indices.extend([0, 1, 2, 1, 3, 2, 2, 3, 4, 3, 5, 4]);
    p.add(egui::Shape::mesh(mesh));
}
fn metal(p: &egui::Painter, c: egui::Pos2, r: f32, base: f32) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(c, Color32::from_gray((base + 14.0).clamp(0.0, 255.0) as u8));
    for i in 0..=64 {
        let a = i as f32 * std::f32::consts::TAU / 64.0;
        let light =
            (base + 38.0 * (a + 2.2).cos() + 17.0 * (a * 3.0).cos()).clamp(0.0, 255.0) as u8;
        mesh.colored_vertex(c + vec2(a.cos(), a.sin()) * r, Color32::from_gray(light));
        if i > 0 {
            mesh.indices.extend([0, i, i + 1]);
        }
    }
    p.add(egui::Shape::mesh(mesh));
}
fn shadow(p: &egui::Painter, r: egui::Rect) {
    for i in (1..=5).rev() {
        p.rect_filled(
            r.translate(vec2(1.0, 2.0)).expand(i as f32),
            6.0,
            Color32::from_black_alpha(7 + (6 - i) * 3),
        );
    }
}
fn screw(p: &egui::Painter, c: egui::Pos2, r: f32) {
    p.circle_filled(c + vec2(0.4, 0.7), r + 0.5, Color32::from_black_alpha(180));
    metal(p, c, r, 150.0);
    p.line_segment(
        [c - vec2(r * 0.58, r * 0.23), c + vec2(r * 0.58, r * 0.23)],
        egui::Stroke::new((r * 0.28).max(0.5), Color32::from_gray(37)),
    );
}
fn reel(p: &egui::Painter, c: egui::Pos2, radius: f32, fill: f32, phase: f32) {
    p.circle_filled(
        c + vec2(2.0, 3.0),
        radius + 1.0,
        Color32::from_black_alpha(150),
    );
    metal(p, c, radius, 164.0);
    let pack = tape_radius(fill, radius * 0.27, radius * 0.86) / radius;
    // Three curved apertures expose the changing tape pack through the metal flange.
    for slot in 0..3 {
        let center = phase + (slot as f32 + 0.5) * std::f32::consts::TAU / 3.0;
        let mut mesh = egui::Mesh::default();
        for i in 0..=24 {
            let u = i as f32 / 24.0 * 2.0 - 1.0;
            let a = center + u * 0.59;
            let inner = 0.31 + 0.06 * u.powi(6);
            let outer = 0.865 - 0.04 * u.powi(6);
            let tape = pack.clamp(inner, outer);
            let axis = vec2(a.cos(), a.sin());
            for (r, color) in [
                (inner, Color32::from_rgb(91, 57, 34)),
                (tape, Color32::from_rgb(115, 76, 47)),
                (tape, Color32::from_rgb(18, 22, 24)),
                (outer, Color32::from_rgb(29, 33, 35)),
            ] {
                mesh.colored_vertex(c + axis * radius * r, color);
            }
            if i > 0 {
                let n = i * 4;
                for k in [0, 2] {
                    mesh.indices
                        .extend([n - 4 + k, n - 3 + k, n + k, n - 3 + k, n + 1 + k, n + k]);
                }
            }
        }
        p.add(egui::Shape::mesh(mesh));
        for fraction in [0.38, 0.49, 0.60, 0.71, 0.82] {
            let r = radius * (0.27 + (pack - 0.27) * fraction);
            if r < radius * 0.38 {
                continue;
            }
            let points = (0..=18)
                .map(|i| {
                    let a = center - 0.52 + i as f32 / 18.0 * 1.04;
                    c + vec2(a.cos(), a.sin()) * r
                })
                .collect();
            p.add(egui::Shape::line(
                points,
                egui::Stroke::new(0.65, Color32::from_black_alpha(75)),
            ));
        }
    }
    for (r, width, color) in [
        (1.0, 1.0, Color32::from_gray(62)),
        (0.975, 1.1, Color32::from_gray(225)),
        (0.922, 0.6, Color32::from_gray(92)),
        (0.90, 0.5, Color32::from_gray(208)),
    ] {
        p.circle_stroke(c, radius * r, egui::Stroke::new(width, color));
    }
    p.circle_filled(c, radius * 0.245, Color32::from_gray(44));
    metal(p, c, radius * 0.218, 177.0);
    p.circle_stroke(
        c,
        radius * 0.184,
        egui::Stroke::new(1.4, Color32::from_gray(86)),
    );
    for i in 0..3 {
        let a = phase + i as f32 * std::f32::consts::TAU / 3.0;
        screw(
            p,
            c + vec2(a.cos(), a.sin()) * radius * 0.16,
            radius * 0.024,
        );
    }
    p.circle_filled(c, radius * 0.095, Color32::from_gray(25));
    metal(p, c, radius * 0.056, 94.0);
}

fn album_jacket(p: &egui::Painter, rect: egui::Rect, art: Option<&egui::TextureHandle>) {
    p.rect_filled(
        rect.translate(vec2(2.0, 3.0)),
        2.0,
        Color32::from_black_alpha(125),
    );
    p.rect_filled(rect.expand(1.0), 2.0, Color32::from_gray(190));
    if let Some(art) = art {
        p.image(
            art.id(),
            rect,
            egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    } else {
        p.rect_filled(rect, 1.0, palette::surface());
        p.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "MC",
            egui::FontId::proportional(rect.width() * 0.2),
            palette::accent(),
        );
    }
}

fn panel(p: &egui::Painter, rect: egui::Rect, radius: f32, fill: Color32) {
    p.rect_filled(rect, radius, fill);
    p.rect_stroke(
        rect,
        radius,
        egui::Stroke::new(1.0, Color32::from_gray(100)),
        egui::StrokeKind::Inside,
    );
}

fn cassette(p: &egui::Painter, rect: egui::Rect, state: &VisualState<'_>) {
    let width = rect.width() * 0.97;
    let shell = egui::Rect::from_center_size(rect.center(), vec2(width, width * 0.64));
    let point = |x, y| shell.min + vec2(x * shell.width(), y * shell.height());
    let region = |x1, y1, x2, y2| egui::Rect::from_min_max(point(x1, y1), point(x2, y2));
    shadow(p, shell);
    panel(p, shell, 6.0, Color32::from_gray(66));
    gradient(
        p,
        shell.shrink(2.0),
        Color32::from_rgb(58, 63, 65),
        Color32::from_rgb(24, 28, 30),
        Color32::from_rgb(43, 48, 50),
    );
    p.rect_stroke(
        shell.shrink(4.0),
        4.0,
        egui::Stroke::new(0.7, Color32::from_gray(14)),
        egui::StrokeKind::Inside,
    );
    let label = region(0.055, 0.075, 0.945, 0.755);
    p.rect_filled(label.expand(1.0), 3.0, Color32::from_gray(13));
    if let Some(art) = state.art {
        let ratio = label.aspect_ratio() / art.aspect_ratio();
        let uv = if ratio > 1.0 {
            let h = 1.0 / ratio;
            egui::Rect::from_min_max(pos2(0.0, (1.0 - h) * 0.5), pos2(1.0, (1.0 + h) * 0.5))
        } else {
            egui::Rect::from_min_max(
                pos2((1.0 - ratio) * 0.5, 0.0),
                pos2((1.0 + ratio) * 0.5, 1.0),
            )
        };
        p.image(art.id(), label, uv, Color32::WHITE);
    } else {
        p.rect_filled(label, 2.0, palette::accent().gamma_multiply(0.8));
    }
    let band = region(0.075, 0.10, 0.925, 0.27);
    p.rect_filled(band, 1.0, Color32::from_black_alpha(145));
    p.with_clip_rect(band.shrink(3.0)).text(
        band.left_center() + vec2(5.0, 0.0),
        egui::Align2::LEFT_CENTER,
        if state.album.is_empty() {
            "MusicCraft"
        } else {
            state.album
        },
        egui::FontId::proportional((width * 0.047).clamp(8.0, 14.0)),
        Color32::from_gray(244),
    );
    let window = region(0.115, 0.34, 0.885, 0.675);
    p.rect_filled(window.expand(2.0), 5.0, Color32::from_gray(18));
    gradient(
        p,
        window,
        Color32::from_rgb(10, 15, 17),
        Color32::from_rgb(33, 39, 39),
        Color32::from_rgb(10, 15, 17),
    );
    let radius = shell.height() * 0.147;
    for (i, (x, fill)) in [(0.285, 1.0 - state.progress), (0.715, state.progress)]
        .into_iter()
        .enumerate()
    {
        let center = point(x, 0.508);
        let pack = tape_radius(fill, radius * 0.43, radius * 0.97);
        p.circle_filled(center, pack, Color32::from_rgb(77, 47, 31));
        p.circle_stroke(
            center,
            pack,
            egui::Stroke::new(0.7, Color32::from_rgb(135, 91, 59)),
        );
        metal(p, center, radius * 0.52, 210.0);
        p.circle_filled(center, radius * 0.36, Color32::from_gray(184));
        for tooth in 0..6 {
            let a = state.reels[i] + tooth as f32 * std::f32::consts::TAU / 6.0;
            let axis = vec2(a.cos(), a.sin());
            p.line_segment(
                [center + axis * radius * 0.24, center + axis * radius * 0.42],
                egui::Stroke::new(radius * 0.10, Color32::from_gray(33)),
            );
        }
        p.circle_filled(center, radius * 0.17, Color32::from_rgb(13, 17, 18));
    }
    p.line_segment(
        [point(0.4, 0.58), point(0.6, 0.58)],
        egui::Stroke::new(1.0, Color32::from_rgb(139, 89, 54)),
    );
    p.add(egui::Shape::convex_polygon(
        vec![
            window.min + vec2(2.0, 1.0),
            window.right_top() - vec2(2.0, -1.0),
            window.right_center(),
            window.left_center() + vec2(0.0, -window.height() * 0.15),
        ],
        Color32::from_white_alpha(12),
        egui::Stroke::NONE,
    ));
    p.rect_stroke(
        window,
        4.0,
        egui::Stroke::new(0.8, Color32::from_gray(105)),
        egui::StrokeKind::Inside,
    );
    p.add(egui::Shape::convex_polygon(
        vec![
            point(0.25, 0.785),
            point(0.75, 0.785),
            point(0.84, 0.965),
            point(0.16, 0.965),
        ],
        Color32::from_rgb(23, 27, 28),
        egui::Stroke::new(0.7, Color32::from_gray(71)),
    ));
    for x in [0.31, 0.69] {
        p.circle_filled(
            point(x, 0.89),
            shell.height() * 0.027,
            Color32::from_gray(8),
        );
        p.circle_stroke(
            point(x, 0.89),
            shell.height() * 0.028,
            egui::Stroke::new(0.7, Color32::from_gray(72)),
        );
    }
    for (x, y) in [(0.035, 0.055), (0.965, 0.055), (0.035, 0.94), (0.965, 0.94)] {
        screw(p, point(x, y), shell.height() * 0.018);
    }
    p.text(
        point(0.5, 0.91),
        egui::Align2::CENTER_CENTER,
        "MC · 60",
        egui::FontId::monospace((width * 0.029).max(5.0)),
        Color32::from_gray(138),
    );
}

#[derive(Default)]
pub(super) struct NeedleMotion {
    pub levels: [f32; 2],
    velocity: [f32; 2],
}
impl NeedleMotion {
    pub fn advance(&mut self, elapsed: f32, peaks: [f32; 2]) {
        for (i, peak) in peaks.into_iter().enumerate() {
            // Critically damped movement: roughly 95% of a step in 300 ms.
            let target = (if peak.is_finite() { peak / 0.1259 } else { 0.0 })
                .clamp(0.0, std::f32::consts::SQRT_2);
            let omega = 16.0;
            let y = self.levels[i] - target;
            let c = self.velocity[i] + omega * y;
            let e = (-omega * elapsed).exp();
            self.levels[i] = target + (y + c * elapsed) * e;
            self.velocity[i] = (self.velocity[i] - omega * c * elapsed) * e;
        }
    }
}

fn vu_meter(p: &egui::Painter, rect: egui::Rect, level: f32) {
    panel(p, rect, 3.0, Color32::from_gray(12));
    let face = rect.shrink(2.0);
    gradient(
        p,
        face,
        Color32::from_rgb(221, 203, 155),
        Color32::from_rgb(243, 226, 184),
        Color32::from_rgb(212, 190, 142),
    );
    // The real movement pivots below the visible dial, giving the scale its
    // wide, shallow arc instead of a small semicircle in the middle.
    let radius = face.width() * 0.55;
    let pivot = pos2(face.center().x, face.top() + radius + face.height() * 0.20);
    let dial = p.with_clip_rect(face.intersect(p.clip_rect()));
    let angle = |t: f32| -2.40 + t * 1.66;
    let point = |t: f32, r: f32| {
        let a = angle(t);
        pivot + vec2(a.cos(), a.sin()) * radius * r
    };
    for (start, end, color) in [
        (0.03, 0.707, Color32::from_rgb(55, 44, 32)),
        (0.707, 1.0, Color32::from_rgb(167, 42, 28)),
    ] {
        dial.add(egui::Shape::line(
            (0..=24)
                .map(|i| point(start + (end - start) * i as f32 / 24.0, 1.0))
                .collect(),
            egui::Stroke::new((rect.height() * 0.012).max(0.7), color),
        ));
    }
    for db in [-20_i32, -10, -7, -5, -3, -2, -1, 0, 1, 2, 3] {
        let t = 10_f32.powf(db as f32 / 20.0) / std::f32::consts::SQRT_2;
        let major = [-20, -10, -5, -3, 0, 3].contains(&db);
        let color = if db > 0 {
            Color32::from_rgb(158, 43, 29)
        } else {
            Color32::from_rgb(48, 39, 29)
        };
        dial.line_segment(
            [point(t, if major { 0.86 } else { 0.92 }), point(t, 1.01)],
            egui::Stroke::new(if major { 1.0 } else { 0.6 }, color),
        );
        if major && rect.width() > 100.0 {
            dial.text(
                point(t, 1.08),
                egui::Align2::CENTER_CENTER,
                db.to_string(),
                egui::FontId::proportional((face.height() * 0.085).clamp(6.0, 11.0)),
                color,
            );
        }
    }
    dial.text(
        pos2(face.center().x, face.top() + face.height() * 0.65),
        egui::Align2::CENTER_CENTER,
        "VU",
        egui::FontId::proportional((face.height() * 0.16).max(5.0)),
        Color32::from_rgb(63, 46, 27),
    );
    let tip = point((level / std::f32::consts::SQRT_2).clamp(0.0, 1.0), 0.98);
    dial.line_segment(
        [pivot + vec2(1.0, 1.0), tip + vec2(1.0, 1.0)],
        egui::Stroke::new(1.5, Color32::from_black_alpha(40)),
    );
    dial.line_segment(
        [pivot, tip],
        egui::Stroke::new(
            (rect.height() * 0.011).max(0.8),
            Color32::from_rgb(113, 27, 20),
        ),
    );
    dial.circle_filled(
        pivot,
        (face.height() * 0.025).max(1.0),
        Color32::from_gray(43),
    );
    // A shallow glass reflection, inner shadow and polished bezel add depth.
    p.rect_filled(
        egui::Rect::from_min_max(face.min, face.right_top() + vec2(0.0, face.height() * 0.08)),
        1.0,
        Color32::from_black_alpha(30),
    );
    p.add(egui::Shape::convex_polygon(
        vec![
            face.left_top() + vec2(1.0, 1.0),
            face.right_top() - vec2(1.0, -1.0),
            face.right_top() + vec2(-face.width() * 0.15, face.height() * 0.31),
            face.left_top() + vec2(1.0, face.height() * 0.47),
        ],
        Color32::from_white_alpha(19),
        egui::Stroke::NONE,
    ));
    p.rect_stroke(
        rect.shrink(0.6),
        2.0,
        egui::Stroke::new(0.7, Color32::from_gray(101)),
        egui::StrokeKind::Inside,
    );
}

fn stereo_shell(rect: egui::Rect, side_by_side: bool) -> egui::Rect {
    let available = rect.shrink(4.0);
    if !side_by_side {
        return available;
    }
    // Keep the same wide, shallow dial proportions as the stacked meters,
    // even when the surrounding player artwork area is square or resized.
    let width = available.width().min(available.height() * 2.5);
    egui::Rect::from_min_size(
        pos2(available.center().x - width * 0.5, available.top()),
        vec2(width, width / 2.5),
    )
}

fn stereo_meters(p: &egui::Painter, rect: egui::Rect, state: &VisualState<'_>, side_by_side: bool) {
    let shell = stereo_shell(rect, side_by_side);
    shadow(p, shell);
    panel(p, shell, 5.0, Color32::from_gray(20));
    gradient(
        p,
        shell.shrink(2.0),
        Color32::from_rgb(68, 74, 78),
        Color32::from_rgb(30, 34, 36),
        Color32::from_rgb(45, 50, 53),
    );
    let point = |x, y| shell.min + shell.size() * vec2(x, y);
    for (i, y) in [0.085, 0.50].into_iter().enumerate() {
        let (meter, label) = if side_by_side {
            let x = 0.05 + i as f32 * 0.48;
            (
                egui::Rect::from_min_max(point(x, 0.20), point(x + 0.42, 0.59)),
                point(x, 0.73),
            )
        } else {
            (
                egui::Rect::from_min_max(point(0.07, y), point(0.93, y + 0.32)),
                point(0.08, y + 0.345),
            )
        };
        vu_meter(p, meter, state.vu[i]);
        p.text(
            label,
            egui::Align2::LEFT_CENTER,
            if i == 0 { "L" } else { "R" },
            egui::FontId::monospace((shell.width() * 0.028).max(6.0)),
            Color32::from_gray(167),
        );
    }
    for (x, y) in [
        (0.035, 0.035),
        (0.965, 0.035),
        (0.035, 0.965),
        (0.965, 0.965),
    ] {
        screw(p, point(x, y), shell.width() * 0.008);
    }
    p.text(
        point(0.5, 0.94),
        egui::Align2::CENTER_CENTER,
        "MUSICCRAFT",
        egui::FontId::monospace((shell.width() * 0.032).max(6.0)),
        Color32::from_gray(169),
    );
}

fn tape_deck(p: &egui::Painter, rect: egui::Rect, state: &VisualState<'_>) {
    let shell = rect.shrink(3.0);
    let point = |x, y| shell.min + vec2(x * shell.width(), y * shell.height());
    let region = |x1, y1, x2, y2| egui::Rect::from_min_max(point(x1, y1), point(x2, y2));
    shadow(p, shell);
    panel(p, shell, 5.0, Color32::from_rgb(61, 42, 32));
    for side in [0.012, 0.976] {
        for i in 0..5 {
            let x = side + i as f32 * 0.002;
            p.line_segment(
                [point(x, 0.02), point(x + 0.001, 0.97)],
                egui::Stroke::new(0.6, Color32::from_rgb(103, 69 + i * 2, 43)),
            );
        }
    }
    panel(
        p,
        region(0.035, 0.015, 0.965, 0.98),
        3.0,
        Color32::from_rgb(37, 42, 45),
    );
    gradient(
        p,
        region(0.04, 0.02, 0.96, 0.66),
        Color32::from_rgb(63, 68, 71),
        Color32::from_rgb(30, 35, 38),
        Color32::from_rgb(43, 47, 48),
    );
    // Thin case edges, inset top plate and metal lower fascia.
    p.line_segment(
        [point(0.04, 0.02), point(0.96, 0.02)],
        egui::Stroke::new(1.0, Color32::from_gray(116)),
    );
    let radius = shell.width() * 0.213;
    let centers = [point(0.265, 0.265), point(0.735, 0.265)];
    let fills = [1.0 - state.progress, state.progress];
    let rollers = [point(0.14, 0.53), point(0.86, 0.53)];
    let mut tangents = [pos2(0.0, 0.0); 2];
    for i in 0..2 {
        let r = tape_radius(fills[i], radius * 0.27, radius * 0.86);
        let d = rollers[i] - centers[i];
        let l2 = d.length_sq().max(r * r + 0.01);
        let perpendicular = vec2(-d.y, d.x);
        let sign = if i == 0 { -1.0 } else { 1.0 };
        tangents[i] =
            centers[i] + d * (r * r / l2) + perpendicular * (sign * r * (l2 - r * r).sqrt() / l2);
    }
    p.add(egui::Shape::line(
        vec![
            tangents[0],
            rollers[0],
            point(0.36, 0.59),
            point(0.64, 0.59),
            rollers[1],
            tangents[1],
        ],
        egui::Stroke::new(1.7, Color32::from_rgb(135, 88, 53)),
    ));
    for i in 0..2 {
        reel(p, centers[i], radius, fills[i], state.reels[i]);
    }
    for center in rollers {
        p.circle_filled(center, radius * 0.105, Color32::from_gray(16));
        p.circle_filled(center, radius * 0.077, Color32::from_gray(177));
        p.circle_filled(center, radius * 0.025, Color32::from_gray(55));
    }
    panel(
        p,
        region(0.36, 0.535, 0.64, 0.635),
        3.0,
        Color32::from_rgb(104, 110, 112),
    );
    gradient(
        p,
        region(0.365, 0.54, 0.635, 0.63),
        Color32::from_gray(185),
        Color32::from_gray(111),
        Color32::from_gray(137),
    );
    p.text(
        point(0.5, 0.593),
        egui::Align2::CENTER_CENTER,
        "MUSICCRAFT",
        egui::FontId::monospace((shell.width() * 0.025).max(5.0)),
        Color32::from_gray(47),
    );
    p.line_segment(
        [point(0.37, 0.55), point(0.63, 0.55)],
        egui::Stroke::new(1.0, Color32::from_gray(184)),
    );
    panel(
        p,
        region(0.055, 0.67, 0.945, 0.965),
        2.0,
        Color32::from_rgb(162, 167, 167),
    );
    gradient(
        p,
        region(0.06, 0.675, 0.94, 0.958),
        Color32::from_gray(204),
        Color32::from_gray(145),
        Color32::from_gray(179),
    );
    for i in 0..16 {
        let y = 0.68 + i as f32 * 0.017;
        p.line_segment(
            [point(0.06, y), point(0.94, y)],
            egui::Stroke::new(0.45, Color32::from_white_alpha(18)),
        );
    }
    for y in [0.678, 0.685, 0.94, 0.949] {
        p.line_segment(
            [point(0.06, y), point(0.94, y)],
            egui::Stroke::new(0.5, Color32::from_gray(185)),
        );
    }
    for (x, level) in [(0.08, state.vu[0]), (0.345, state.vu[1])] {
        vu_meter(p, region(x, 0.72, x + 0.23, 0.875), level);
    }
    // Album sleeve stays recognizable beside the transport controls.
    album_jacket(p, region(0.655, 0.69, 0.915, 0.95), state.art);
    for x in [0.105, 0.195, 0.285, 0.375, 0.465, 0.555] {
        panel(
            p,
            region(x, 0.905, x + 0.055, 0.935),
            1.0,
            Color32::from_rgb(47, 51, 53),
        );
    }
    for (x, y) in [(0.06, 0.04), (0.94, 0.04), (0.06, 0.65), (0.94, 0.65)] {
        let center = point(x, y);
        p.circle_filled(center, shell.width() * 0.006, Color32::from_gray(126));
        p.line_segment(
            [center - vec2(1.0, 0.0), center + vec2(1.0, 0.0)],
            egui::Stroke::new(0.6, Color32::from_gray(34)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analog_needles_settle_in_300ms_and_return_to_rest_without_instability() {
        let mut motion = NeedleMotion::default();
        motion.advance(0.3, [0.1259, 0.06295]);
        assert!((0.94..0.96).contains(&motion.levels[0]));
        assert!((0.47..0.48).contains(&motion.levels[1]));
        for _ in 0..100 {
            motion.advance(0.04, [0.0, 0.0]);
        }
        assert!(motion.levels.iter().all(|v| v.abs() < 0.0001));
        motion.advance(0.25, [f32::NAN, f32::INFINITY]);
        assert!(motion.levels.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn reels_keep_equal_tape_speed_and_continuous_angles_across_wraps_and_seeks() {
        for progress in [0.0, 0.1, 0.5, 0.9, 1.0] {
            let mut motion = ReelMotion {
                angles: [0.001, 0.001],
                progress,
            };
            motion.advance(0.02, 0.85);
            let distance: Vec<_> = motion
                .angles
                .iter()
                .zip([1.0 - progress, progress])
                .map(|(angle, fill)| {
                    (0.001 - angle).rem_euclid(std::f32::consts::TAU)
                        * tape_radius(fill, 0.27, 0.86)
                })
                .collect();
            assert!((distance[0] - 0.017).abs() < 0.00001);
            assert!((distance[1] - distance[0]).abs() < 0.00001);
            let angles = motion.angles;
            motion.progress = 1.0 - progress;
            motion.advance(0.0, 0.85);
            assert_eq!(motion.angles, angles);
        }
    }

    #[test]
    fn old_profiles_default_to_vinyl_and_all_choices_round_trip() {
        let old: Saved = serde_json::from_str(r#"{"mini":true,"mini_view":1}"#).unwrap();
        assert_eq!(old.player_appearance, Appearance::Vinyl);
        let future: Saved =
            serde_json::from_str(r#"{"player_appearance":"FutureAppearance"}"#).unwrap();
        assert_eq!(future.player_appearance, Appearance::Vinyl);
        for appearance in Appearance::ALL {
            let saved = Saved {
                player_appearance: appearance,
                ..Default::default()
            };
            let restored: Saved =
                serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
            assert_eq!(restored.player_appearance, appearance);
        }
    }

    #[test]
    fn side_by_side_meters_keep_their_proportions_inside_the_artwork_area() {
        for size in [vec2(180.0, 180.0), vec2(480.0, 360.0), vec2(520.0, 120.0)] {
            let area = egui::Rect::from_min_size(pos2(40.0, 60.0), size);
            let shell = stereo_shell(area, true);
            assert!(area.shrink(4.0).contains_rect(shell));
            assert!((shell.width() / shell.height() - 2.5).abs() < 0.001);
            assert_eq!(shell.center().x, area.center().x);
            assert_eq!(shell.top(), area.top() + 4.0);
            assert_eq!(stereo_shell(area, false), area.shrink(4.0));
        }
    }
}
