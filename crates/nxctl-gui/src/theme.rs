//! "Obsidian Telemetry" design system: colors, visuals and custom painted widgets.

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Frame, Margin, Pos2, Rect, Sense, Shape, Stroke,
    StrokeKind, Vec2,
};

pub const BG_CANVAS: Color32 = Color32::from_rgb(0x09, 0x0A, 0x0F);
pub const BG_SIDEBAR: Color32 = Color32::from_rgb(0x0C, 0x0E, 0x16);
pub const BG_CARD: Color32 = Color32::from_rgb(0x12, 0x14, 0x1C);
pub const BG_CONTROL: Color32 = Color32::from_rgb(0x18, 0x1B, 0x26);
pub const BORDER_MUTED: Color32 = Color32::from_rgb(0x23, 0x29, 0x36);
pub const BORDER_ACTIVE: Color32 = Color32::from_rgb(0x2F, 0x37, 0x4A);
pub const CYAN: Color32 = Color32::from_rgb(0x00, 0xF0, 0xFF);
pub const MAGENTA: Color32 = Color32::from_rgb(0xD9, 0x46, 0xEF);
pub const EMERALD: Color32 = Color32::from_rgb(0x10, 0xB9, 0x81);
pub const AMBER: Color32 = Color32::from_rgb(0xF5, 0x9E, 0x0B);
pub const CRIMSON: Color32 = Color32::from_rgb(0xEF, 0x44, 0x44);
pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(0xF1, 0xF5, 0xF9);
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(0x94, 0xA3, 0xB8);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x64, 0x74, 0x8B);

const ROUNDING: CornerRadius = CornerRadius::same(4);

/// Installs dark visuals matching the Obsidian Telemetry palette.
pub fn apply_obsidian_theme(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.override_text_color = Some(TEXT_PRIMARY);
    v.panel_fill = BG_CANVAS;
    v.window_fill = BG_CARD;
    v.window_stroke = Stroke::new(1.0, BORDER_MUTED);
    v.window_corner_radius = ROUNDING;
    v.menu_corner_radius = ROUNDING;
    v.extreme_bg_color = BG_CANVAS;
    v.faint_bg_color = BG_CARD;
    v.hyperlink_color = CYAN;
    v.selection.bg_fill = CYAN.gamma_multiply(0.25);
    v.selection.stroke = Stroke::new(1.0, CYAN);

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = BG_CARD;
    w.noninteractive.weak_bg_fill = BG_CARD;
    w.noninteractive.bg_stroke = Stroke::new(1.0, BORDER_MUTED);
    w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_SECONDARY);
    w.inactive.bg_fill = BG_CONTROL;
    w.inactive.weak_bg_fill = BG_CONTROL;
    w.inactive.bg_stroke = Stroke::new(1.0, BORDER_MUTED);
    w.inactive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    w.hovered.bg_fill = BG_CONTROL;
    w.hovered.weak_bg_fill = BG_CONTROL;
    w.hovered.bg_stroke = Stroke::new(1.0, BORDER_ACTIVE);
    w.hovered.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    w.active.bg_fill = BORDER_ACTIVE;
    w.active.weak_bg_fill = BORDER_ACTIVE;
    w.active.bg_stroke = Stroke::new(1.0, CYAN);
    w.active.fg_stroke = Stroke::new(1.0, CYAN);
    w.open.bg_fill = BG_CONTROL;
    w.open.weak_bg_fill = BG_CONTROL;
    w.open.bg_stroke = Stroke::new(1.0, BORDER_ACTIVE);
    for s in [
        &mut w.noninteractive,
        &mut w.inactive,
        &mut w.hovered,
        &mut w.active,
        &mut w.open,
    ] {
        s.corner_radius = ROUNDING;
    }
    ctx.set_visuals(v);
}

/// Card container frame.
pub fn card_frame() -> Frame {
    Frame::new()
        .fill(BG_CARD)
        .stroke(Stroke::new(1.0, BORDER_MUTED))
        .corner_radius(ROUNDING)
        .inner_margin(Margin::same(12))
}

/// Fraction (`0.0..=1.0`) of the gauge to fill for `value_c` out of `max_c`.
pub fn gauge_ratio(value_c: Option<f32>, max_c: f32) -> f32 {
    match value_c {
        Some(v) if v.is_finite() && max_c > 0.0 => (v / max_c).clamp(0.0, 1.0),
        _ => 0.0,
    }
}

/// Fraction (`0.0..=1.0`) of a PWM bar to fill for `duty` percent.
pub fn duty_ratio(duty: f32) -> f32 {
    if duty.is_finite() {
        (duty / 100.0).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Points of an arc starting at `start` radians sweeping `sweep` radians (clockwise on screen).
pub fn arc_points(center: Pos2, radius: f32, start: f32, sweep: f32, segments: usize) -> Vec<Pos2> {
    let n = segments.max(1);
    (0..=n)
        .map(|i| {
            let a = start + sweep * (i as f32 / n as f32);
            center + Vec2::new(a.cos(), a.sin()) * radius
        })
        .collect()
}

const GAUGE_START: f32 = 135.0_f32 * std::f32::consts::PI / 180.0;
const GAUGE_SWEEP: f32 = 270.0_f32 * std::f32::consts::PI / 180.0;

/// Draws a 270° radial gauge with a centered temperature and sublabel.
pub fn draw_radial_gauge(
    ui: &mut egui::Ui,
    value_c: Option<f32>,
    max_c: f32,
    sublabel: &str,
    accent: Color32,
) {
    let size = Vec2::splat(132.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter();
    let center = rect.center();
    let radius = rect.width() / 2.0 - 8.0;
    let track = arc_points(center, radius, GAUGE_START, GAUGE_SWEEP, 64);
    painter.add(Shape::line(track, Stroke::new(8.0, BG_CONTROL)));
    let ratio = gauge_ratio(value_c, max_c);
    if ratio > 0.0 {
        let segs = ((64.0 * ratio) as usize).max(2);
        let arc = arc_points(center, radius, GAUGE_START, GAUGE_SWEEP * ratio, segs);
        painter.add(Shape::line(arc, Stroke::new(8.0, accent)));
    }
    let text = match value_c {
        Some(v) => format!("{v:.0}°C"),
        None => "--".to_string(),
    };
    painter.text(
        center - Vec2::new(0.0, 6.0),
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(26.0),
        TEXT_PRIMARY,
    );
    painter.text(
        center + Vec2::new(0.0, 18.0),
        Align2::CENTER_CENTER,
        sublabel,
        FontId::proportional(11.0),
        TEXT_SECONDARY,
    );
}

/// Draws a crisp 3-blade fan rotor inside a subtle ring.
pub fn draw_fan_icon(ui: &mut egui::Ui, size: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let painter = ui.painter();
    let center = rect.center();
    let outer_r = size * 0.44;
    let hub_r = size * 0.11;
    let tip_r = size * 0.37;

    painter.circle_stroke(
        center,
        outer_r,
        Stroke::new(1.1, color.gamma_multiply(0.45)),
    );

    let pt = |r: f32, angle: f32| center + Vec2::new(angle.cos(), angle.sin()) * r;
    for i in 0..3 {
        let base = -std::f32::consts::FRAC_PI_2 + i as f32 * (std::f32::consts::TAU / 3.0);
        let blade = vec![
            pt(hub_r * 0.9, base - 0.15),
            pt(tip_r * 0.96, base + 0.04),
            pt(tip_r, base + 0.34),
            pt(tip_r * 0.86, base + 0.58),
            pt(hub_r * 0.9, base + 0.32),
        ];
        painter.add(Shape::convex_polygon(blade, color, Stroke::NONE));
    }

    painter.circle_filled(center, hub_r, color);
}

/// Draws a crisp liquid-pump impeller with concentric ring and center droplet accent.
pub fn draw_pump_icon(ui: &mut egui::Ui, size: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let painter = ui.painter();
    let center = rect.center();
    let outer_r = size * 0.44;
    let inner_r = size * 0.27;

    painter.circle_stroke(
        center,
        outer_r,
        Stroke::new(1.1, color.gamma_multiply(0.50)),
    );
    painter.circle_stroke(
        center,
        inner_r,
        Stroke::new(1.1, color.gamma_multiply(0.85)),
    );

    // Four subtle impeller vanes between inner and outer rings.
    for i in 0..4 {
        let a = std::f32::consts::FRAC_PI_4 + i as f32 * std::f32::consts::FRAC_PI_2;
        let dir = Vec2::new(a.cos(), a.sin());
        painter.line_segment(
            [center + dir * inner_r, center + dir * (outer_r * 0.95)],
            Stroke::new(1.1, color.gamma_multiply(0.75)),
        );
    }

    // Center droplet / flow accent.
    let drop_r = size * 0.10;
    let drop_center = center + Vec2::new(0.0, size * 0.05);
    painter.circle_filled(drop_center, drop_r, color);
    let tip = vec![
        center + Vec2::new(0.0, -size * 0.16),
        drop_center + Vec2::new(drop_r * 0.95, -drop_r * 0.2),
        drop_center + Vec2::new(-drop_r * 0.95, -drop_r * 0.2),
    ];
    painter.add(Shape::convex_polygon(tip, color, Stroke::NONE));
}

/// Draws a crisp CPU die icon with pins on all four sides.
pub fn draw_cpu_icon(ui: &mut egui::Ui, size: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let painter = ui.painter();
    let c = rect.center();
    let body = Rect::from_center_size(c, Vec2::splat(size * 0.56));
    let stroke = Stroke::new(1.2, color);
    painter.rect_stroke(body, CornerRadius::same(2), stroke, StrokeKind::Inside);
    painter.rect_filled(
        Rect::from_center_size(c, Vec2::splat(size * 0.24)),
        CornerRadius::same(1),
        color.gamma_multiply(0.85),
    );
    let pin = size * 0.14;
    for offset in [-size * 0.14, size * 0.14] {
        painter.line_segment(
            [
                Pos2::new(c.x + offset, body.top()),
                Pos2::new(c.x + offset, body.top() - pin),
            ],
            stroke,
        );
        painter.line_segment(
            [
                Pos2::new(c.x + offset, body.bottom()),
                Pos2::new(c.x + offset, body.bottom() + pin),
            ],
            stroke,
        );
        painter.line_segment(
            [
                Pos2::new(body.left(), c.y + offset),
                Pos2::new(body.left() - pin, c.y + offset),
            ],
            stroke,
        );
        painter.line_segment(
            [
                Pos2::new(body.right(), c.y + offset),
                Pos2::new(body.right() + pin, c.y + offset),
            ],
            stroke,
        );
    }
}

/// Draws a crisp dual-fan GPU card icon.
pub fn draw_gpu_icon(ui: &mut egui::Ui, size: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let painter = ui.painter();
    let c = rect.center();
    let shroud = Rect::from_center_size(c, Vec2::new(size * 0.84, size * 0.52));
    let stroke = Stroke::new(1.2, color);
    painter.rect_stroke(shroud, CornerRadius::same(2), stroke, StrokeKind::Inside);
    let fan_r = size * 0.14;
    for dx in [-size * 0.19, size * 0.19] {
        let fc = Pos2::new(c.x + dx, c.y);
        painter.circle_stroke(fc, fan_r, stroke);
        painter.circle_filled(fc, size * 0.04, color);
    }
    painter.line_segment(
        [
            Pos2::new(shroud.left() - size * 0.06, shroud.top() - size * 0.06),
            Pos2::new(shroud.left() - size * 0.06, shroud.bottom() + size * 0.08),
        ],
        stroke,
    );
}

/// Draws a crisp shield icon for Daemon Watchdog.
pub fn draw_shield_icon(ui: &mut egui::Ui, size: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let painter = ui.painter();
    let c = rect.center();
    let w = size * 0.36;
    let h = size * 0.40;
    let pts = vec![
        Pos2::new(c.x, c.y - h),
        Pos2::new(c.x + w, c.y - h * 0.55),
        Pos2::new(c.x + w * 0.82, c.y + h * 0.35),
        Pos2::new(c.x, c.y + h),
        Pos2::new(c.x - w * 0.82, c.y + h * 0.35),
        Pos2::new(c.x - w, c.y - h * 0.55),
    ];
    painter.add(Shape::closed_line(pts, Stroke::new(1.2, color)));
    painter.circle_filled(c, size * 0.09, color);
}

/// Draws a crisp thermometer sensor icon.
pub fn draw_sensor_icon(ui: &mut egui::Ui, size: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let painter = ui.painter();
    let c = rect.center();
    painter.line_segment(
        [
            Pos2::new(c.x, c.y - size * 0.34),
            Pos2::new(c.x, c.y + size * 0.14),
        ],
        Stroke::new(2.0, color),
    );
    painter.circle_filled(Pos2::new(c.x, c.y + size * 0.24), size * 0.17, color);
}

/// Draws a titled PWM bar with fan/pump icon and RPM / duty readout.
pub fn draw_pwm_bar(
    ui: &mut egui::Ui,
    title: &str,
    rpm: Option<f64>,
    duty: f32,
    accent: Color32,
    is_pump: bool,
) {
    ui.horizontal(|ui| {
        if is_pump {
            draw_pump_icon(ui, 14.0, accent);
        } else {
            draw_fan_icon(ui, 14.0, accent);
        }
        ui.label(egui::RichText::new(title).color(TEXT_SECONDARY).size(12.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let rpm_txt = rpm.map_or("-- RPM".to_string(), |r| format!("{r:.0} RPM"));
            ui.label(
                egui::RichText::new(format!("{rpm_txt} · {duty:.0}%"))
                    .color(TEXT_PRIMARY)
                    .monospace()
                    .size(12.0),
            );
        });
    });
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 10.0), Sense::hover());
    let painter = ui.painter();
    let r = CornerRadius::same(2);
    painter.rect_filled(rect, r, BG_CANVAS);
    let fill_w = (rect.width() * duty_ratio(duty)).max(0.0);
    if fill_w > 0.0 {
        let fill = Rect::from_min_size(rect.min, Vec2::new(fill_w, rect.height()));
        painter.rect_filled(fill, r, accent);
    }
    painter.rect_stroke(rect, r, Stroke::new(1.0, BORDER_MUTED), StrokeKind::Inside);
    ui.add_space(6.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gauge_ratio_math() {
        assert_eq!(gauge_ratio(None, 100.0), 0.0);
        assert_eq!(gauge_ratio(Some(50.0), 100.0), 0.5);
        assert_eq!(gauge_ratio(Some(150.0), 100.0), 1.0);
        assert_eq!(gauge_ratio(Some(-5.0), 100.0), 0.0);
        assert_eq!(gauge_ratio(Some(50.0), 0.0), 0.0);
        assert_eq!(gauge_ratio(Some(f32::NAN), 100.0), 0.0);
        assert_eq!(duty_ratio(60.0), 0.6);
        assert_eq!(duty_ratio(250.0), 1.0);
    }

    #[test]
    fn arc_has_expected_points() {
        let pts = arc_points(Pos2::ZERO, 10.0, 0.0, std::f32::consts::PI, 8);
        assert_eq!(pts.len(), 9);
        assert!((pts[0].x - 10.0).abs() < 1e-4);
        assert!((pts[8].x + 10.0).abs() < 1e-4);
    }

    #[test]
    fn color_constants_match_design() {
        assert_eq!(BG_CANVAS, Color32::from_rgb(9, 10, 15));
        assert_eq!(BG_SIDEBAR, Color32::from_rgb(12, 14, 22));
        assert_eq!(BG_CARD, Color32::from_rgb(18, 20, 28));
        assert_eq!(BG_CONTROL, Color32::from_rgb(24, 27, 38));
        assert_eq!(BORDER_MUTED, Color32::from_rgb(35, 41, 54));
        assert_eq!(BORDER_ACTIVE, Color32::from_rgb(47, 55, 74));
        assert_eq!(CYAN, Color32::from_rgb(0, 240, 255));
        assert_eq!(MAGENTA, Color32::from_rgb(217, 70, 239));
        assert_eq!(EMERALD, Color32::from_rgb(16, 185, 129));
        assert_eq!(AMBER, Color32::from_rgb(245, 158, 11));
        assert_eq!(CRIMSON, Color32::from_rgb(239, 68, 68));
        assert_eq!(TEXT_PRIMARY, Color32::from_rgb(241, 245, 249));
        assert_eq!(TEXT_SECONDARY, Color32::from_rgb(148, 163, 184));
        assert_eq!(TEXT_MUTED, Color32::from_rgb(100, 116, 139));
    }
}
