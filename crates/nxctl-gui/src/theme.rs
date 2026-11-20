//! Graphite Instrument design system: tokens, fonts, visuals, and custom widgets.

use std::collections::VecDeque;

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, FontTweak,
    Frame, Margin, Pos2, Rect, Sense, Shadow, Shape, Stroke, StrokeKind, TextStyle, Vec2,
    emath::easing, epaint::text::VariationCoords,
};

// ----------------------------------------------------------------- Surfaces
pub const BG_SIDEBAR: Color32 = Color32::from_rgb(0x0F, 0x0F, 0x10);
pub const BG_CANVAS: Color32 = Color32::from_rgb(0x14, 0x14, 0x15);
pub const BG_CARD: Color32 = Color32::from_rgb(0x19, 0x19, 0x1B);
pub const BG_CONTROL: Color32 = Color32::from_rgb(0x23, 0x23, 0x25);
pub const BG_CONTROL_HOVER: Color32 = Color32::from_rgb(0x2B, 0x2B, 0x2E);
pub const BG_PLOT: Color32 = Color32::from_rgb(0x14, 0x14, 0x15);

// --------------------------------------------------------- Hairlines & glow-free depth
pub const BORDER_MUTED: Color32 = Color32::from_rgba_premultiplied(19, 19, 19, 20);
pub const BORDER_ACTIVE: Color32 = Color32::from_rgba_premultiplied(35, 35, 35, 38);
pub const HIGHLIGHT_TOP: Color32 = Color32::from_rgba_premultiplied(28, 28, 28, 28);
pub const PLOT_GRID: Color32 = Color32::from_rgba_premultiplied(22, 22, 22, 24);

// --------------------------------------------------------------- Text roles
pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(0xED, 0xED, 0xED);
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(0xA1, 0xA1, 0xA6);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x85, 0x85, 0x8B);

// ------------------------------------------------ Chrome accent & telemetry series
pub const ACCENT: Color32 = Color32::from_rgb(0x4E, 0xA8, 0xDE);
pub const SERIES_CPU: Color32 = ACCENT;
pub const SERIES_GPU: Color32 = Color32::from_rgb(0xD9, 0x8A, 0x4E);
pub const SERIES_AIO: Color32 = Color32::from_rgb(0x4C, 0xC3, 0x8A);
pub const SERIES_EXTRA: Color32 = Color32::from_rgb(0x9B, 0x8E, 0xC4);
pub const PWM_FAN_FILL: Color32 = Color32::from_rgb(0x73, 0x7E, 0x8F);

// ------------------------------------------------------------------ Semantic
pub const SEMANTIC_OK: Color32 = Color32::from_rgb(0x4C, 0xC3, 0x8A);
pub const SEMANTIC_WARN: Color32 = Color32::from_rgb(0xE0, 0x9F, 0x3E);
pub const SEMANTIC_ERR: Color32 = Color32::from_rgb(0xE5, 0x48, 0x4D);

// ------------------------------------------------------------- Nested radii
pub const RADIUS_CARD: CornerRadius = CornerRadius::same(12);
pub const RADIUS_CONTROL: CornerRadius = CornerRadius::same(8);
pub const RADIUS_CHIP: CornerRadius = CornerRadius::same(6);
pub const RADIUS_TRACK: CornerRadius = CornerRadius::same(3);

const ICON_STROKE: f32 = 1.5;
const FONT_MEDIUM_FAMILY: &str = "medium";

const PROPORTIONAL_CANDIDATES: &[&str] = &[
    "/usr/share/fonts/TTF/Inter-Regular.ttf",
    "/usr/share/fonts/inter/Inter-Regular.ttf",
    "/usr/share/fonts/TTF/IBMPlexSans-Regular.ttf",
    "/usr/share/fonts/Adwaita/AdwaitaSans-Regular.ttf",
    "/usr/share/fonts/TTF/FiraSans-Regular.ttf",
];

const STATIC_MEDIUM_PAIRS: &[(&str, &str)] = &[
    (
        "/usr/share/fonts/TTF/Inter-Regular.ttf",
        "/usr/share/fonts/TTF/Inter-Medium.ttf",
    ),
    (
        "/usr/share/fonts/inter/Inter-Regular.ttf",
        "/usr/share/fonts/inter/Inter-Medium.ttf",
    ),
    (
        "/usr/share/fonts/TTF/IBMPlexSans-Regular.ttf",
        "/usr/share/fonts/TTF/IBMPlexSans-Medium.ttf",
    ),
    (
        "/usr/share/fonts/TTF/FiraSans-Regular.ttf",
        "/usr/share/fonts/TTF/FiraSans-Medium.ttf",
    ),
];

const MONO_CANDIDATES: &[&str] = &[
    "/usr/share/fonts/TTF/JetBrainsMono-Regular.ttf",
    "/usr/share/fonts/TTF/IBMPlexMono-Regular.ttf",
    "/usr/share/fonts/Adwaita/AdwaitaMono-Regular.ttf",
    "/usr/share/fonts/TTF/Hack-Regular.ttf",
];

const FALLBACK_GLYPH_PATHS: &[&str] = &[
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
];

/// Returns the medium-weight proportional font id.
pub fn font_medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(FONT_MEDIUM_FAMILY.into()))
}

/// Formats a temperature uniformly across the entire GUI (`44.2°C` or `--.-°C`).
pub fn format_temp(temp: Option<f32>) -> String {
    match temp {
        Some(v) if v.is_finite() => format!("{v:.1}°C"),
        _ => "--.-°C".to_string(),
    }
}

/// Formats an integer RPM readout with comma thousands grouping (`1,212 RPM` or `-- RPM`).
pub fn format_rpm(rpm: Option<f64>) -> String {
    match rpm {
        Some(r) if r.is_finite() && r >= 0.0 => {
            let rounded = r.round() as u64;
            if rounded >= 1000 {
                format!("{},{:03} RPM", rounded / 1000, rounded % 1000)
            } else {
                format!("{rounded} RPM")
            }
        }
        _ => "-- RPM".to_string(),
    }
}

/// Computes the observed `(min, max)` temperature range from a rolling history.
pub fn temp_range(history: &VecDeque<f64>) -> Option<(f64, f64)> {
    let mut min_v = f64::INFINITY;
    let mut max_v = f64::NEG_INFINITY;
    let mut any = false;
    for &v in history {
        if v.is_finite() {
            min_v = min_v.min(v);
            max_v = max_v.max(v);
            any = true;
        }
    }
    any.then_some((min_v, max_v))
}

/// Returns a toned semantic text color only for elevated temperatures; nominal temps stay neutral.
pub fn temp_semantic_color(temp: Option<f32>) -> Color32 {
    match temp {
        Some(v) if v.is_finite() && v >= 80.0 => SEMANTIC_ERR,
        Some(v) if v.is_finite() && v >= 60.0 => SEMANTIC_WARN,
        Some(v) if v.is_finite() => TEXT_PRIMARY,
        _ => TEXT_MUTED,
    }
}

fn install_fonts(ctx: &egui::Context) {
    let mut defs = FontDefinitions::default();
    let mut prop_list: Vec<String> = Vec::new();
    let mut med_list: Vec<String> = Vec::new();

    if let Some((path, bytes)) = PROPORTIONAL_CANDIDATES
        .iter()
        .find_map(|p| std::fs::read(p).ok().map(|b| (*p, b)))
    {
        let primary_data = FontData::from_owned(bytes.clone());
        let is_variable = !primary_data.variation_axes().is_empty();
        defs.font_data
            .insert("ui_sans".into(), std::sync::Arc::new(primary_data));
        prop_list.push("ui_sans".into());

        let static_medium = STATIC_MEDIUM_PAIRS
            .iter()
            .find(|(reg, _)| *reg == path)
            .and_then(|(_, med)| std::fs::read(med).ok());

        if let Some(med_bytes) = static_medium {
            defs.font_data.insert(
                "ui_sans_medium".into(),
                std::sync::Arc::new(FontData::from_owned(med_bytes)),
            );
            med_list.push("ui_sans_medium".into());
        } else if is_variable {
            let med_data = FontData::from_owned(bytes).tweak(FontTweak {
                coords: VariationCoords::new([(b"wght", 500.0)]),
                ..Default::default()
            });
            defs.font_data
                .insert("ui_sans_medium".into(), std::sync::Arc::new(med_data));
            med_list.push("ui_sans_medium".into());
        }
    }

    if let Some(bytes) = FALLBACK_GLYPH_PATHS
        .iter()
        .find_map(|p| std::fs::read(p).ok())
    {
        defs.font_data.insert(
            "ui_glyph_fallback".into(),
            std::sync::Arc::new(FontData::from_owned(bytes)),
        );
        prop_list.push("ui_glyph_fallback".into());
    }

    if !prop_list.is_empty() {
        let mut combined_prop = prop_list;
        if let Some(existing) = defs.families.get(&FontFamily::Proportional) {
            combined_prop.extend(existing.iter().cloned());
        }
        let mut combined_med = med_list;
        combined_med.extend(combined_prop.iter().cloned());
        defs.families
            .insert(FontFamily::Proportional, combined_prop);
        defs.families
            .insert(FontFamily::Name(FONT_MEDIUM_FAMILY.into()), combined_med);
    } else if let Some(existing) = defs.families.get(&FontFamily::Proportional).cloned() {
        defs.families
            .insert(FontFamily::Name(FONT_MEDIUM_FAMILY.into()), existing);
    }

    if let Some(mono_bytes) = MONO_CANDIDATES.iter().find_map(|p| std::fs::read(p).ok()) {
        defs.font_data.insert(
            "ui_mono".into(),
            std::sync::Arc::new(FontData::from_owned(mono_bytes)),
        );
        let mut combined_mono = vec!["ui_mono".to_string()];
        if defs.font_data.contains_key("ui_glyph_fallback") {
            combined_mono.push("ui_glyph_fallback".to_string());
        }
        if let Some(existing) = defs.families.get(&FontFamily::Monospace) {
            combined_mono.extend(existing.iter().cloned());
        }
        defs.families.insert(FontFamily::Monospace, combined_mono);
    }

    ctx.set_fonts(defs);
}

/// Installs the Graphite Instrument fonts, type scale, spacing, and dark visuals.
pub fn apply_theme(ctx: &egui::Context) {
    install_fonts(ctx);
    ctx.set_theme(egui::Theme::Dark);

    ctx.all_styles_mut(|s| {
        s.text_styles
            .insert(TextStyle::Small, FontId::proportional(12.0));
        s.text_styles
            .insert(TextStyle::Body, FontId::proportional(14.0));
        s.text_styles
            .insert(TextStyle::Button, FontId::proportional(13.0));
        s.text_styles
            .insert(TextStyle::Monospace, FontId::monospace(12.5));
        s.text_styles.insert(TextStyle::Heading, font_medium(16.0));

        s.spacing.item_spacing = Vec2::new(8.0, 8.0);
        s.spacing.button_padding = Vec2::new(12.0, 6.0);
        s.spacing.interact_size = Vec2::new(40.0, 32.0);
        s.spacing.slider_width = 160.0;
        s.spacing.combo_width = 148.0;
    });

    let mut v = egui::Visuals::dark();
    v.override_text_color = Some(TEXT_PRIMARY);
    v.panel_fill = BG_CANVAS;
    v.window_fill = BG_CARD;
    v.window_stroke = Stroke::new(1.0, BORDER_MUTED);
    v.window_corner_radius = RADIUS_CARD;
    v.window_shadow = Shadow::NONE;
    v.popup_shadow = Shadow {
        offset: [0, 4],
        blur: 16,
        spread: 0,
        color: Color32::from_black_alpha(90),
    };
    v.menu_corner_radius = RADIUS_CONTROL;
    v.extreme_bg_color = BG_PLOT;
    v.faint_bg_color = BG_CARD;
    v.hyperlink_color = ACCENT;
    v.selection.bg_fill = ACCENT.gamma_multiply(0.22);
    v.selection.stroke = Stroke::new(1.0, ACCENT);

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = BG_CARD;
    w.noninteractive.weak_bg_fill = BG_CARD;
    w.noninteractive.bg_stroke = Stroke::new(1.0, BORDER_MUTED);
    w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_SECONDARY);

    w.inactive.bg_fill = BG_CONTROL;
    w.inactive.weak_bg_fill = BG_CONTROL;
    w.inactive.bg_stroke = Stroke::new(1.0, BORDER_MUTED);
    w.inactive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);

    w.hovered.bg_fill = BG_CONTROL_HOVER;
    w.hovered.weak_bg_fill = BG_CONTROL_HOVER;
    w.hovered.bg_stroke = Stroke::new(1.0, BORDER_ACTIVE);
    w.hovered.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);

    w.active.bg_fill = BG_CONTROL_HOVER;
    w.active.weak_bg_fill = BG_CONTROL_HOVER;
    w.active.bg_stroke = Stroke::new(1.0, ACCENT);
    w.active.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);

    w.open.bg_fill = BG_CONTROL_HOVER;
    w.open.weak_bg_fill = BG_CONTROL_HOVER;
    w.open.bg_stroke = Stroke::new(1.0, BORDER_ACTIVE);
    w.open.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);

    for s in [
        &mut w.noninteractive,
        &mut w.inactive,
        &mut w.hovered,
        &mut w.active,
        &mut w.open,
    ] {
        s.corner_radius = RADIUS_CONTROL;
    }
    ctx.set_visuals(v);
}

/// Primary card container frame (20px padding, 12px radius, hairline border).
pub fn card_frame() -> Frame {
    Frame::new()
        .fill(BG_CARD)
        .stroke(Stroke::new(1.0, BORDER_MUTED))
        .corner_radius(RADIUS_CARD)
        .inner_margin(Margin::same(20))
}

/// Nested list/channel row frame (12x8 padding, 8px radius).
pub fn row_frame(highlighted: bool) -> Frame {
    Frame::new()
        .fill(if highlighted { BG_CONTROL } else { BG_CANVAS })
        .stroke(Stroke::new(
            1.0,
            if highlighted {
                BORDER_ACTIVE
            } else {
                BORDER_MUTED
            },
        ))
        .corner_radius(RADIUS_CONTROL)
        .inner_margin(Margin::symmetric(12, 8))
}

/// Off-white primary action button with dark graphite text.
pub fn primary_button(label: &str) -> egui::Button<'_> {
    egui::Button::new(
        egui::RichText::new(label)
            .font(font_medium(13.0))
            .color(BG_SIDEBAR),
    )
    .fill(TEXT_PRIMARY)
    .stroke(Stroke::NONE)
    .corner_radius(RADIUS_CONTROL)
    .min_size(Vec2::new(0.0, 32.0))
}

/// Raised neutral secondary button using Visuals hover/active transitions.
pub fn secondary_button(label: &str) -> egui::Button<'_> {
    egui::Button::new(egui::RichText::new(label).size(13.0).color(TEXT_PRIMARY))
        .corner_radius(RADIUS_CONTROL)
        .min_size(Vec2::new(0.0, 32.0))
}

/// Subtle hairline-only ghost button.
pub fn ghost_button(label: &str, text_color: Color32) -> egui::Button<'_> {
    egui::Button::new(egui::RichText::new(label).size(13.0).color(text_color))
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::new(1.0, BORDER_MUTED))
        .corner_radius(RADIUS_CONTROL)
        .min_size(Vec2::new(0.0, 32.0))
}

/// Quiet neutral chip without neon saturation.
pub fn quiet_chip(ui: &mut egui::Ui, text: &str, color: Color32) {
    Frame::new()
        .fill(BG_CONTROL)
        .stroke(Stroke::new(1.0, BORDER_MUTED))
        .corner_radius(RADIUS_CHIP)
        .inner_margin(Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).color(color).size(11.5));
        });
}

/// Live 6px status dot paired with a quiet label (always dot-then-label regardless of parent layout).
pub fn status_dot_label(ui: &mut egui::Ui, dot_color: Color32, label: &str, text_color: Color32) {
    if ui.layout().main_dir() == egui::Direction::RightToLeft {
        ui.label(egui::RichText::new(label).color(text_color).size(12.0));
        let (dot_rect, _) = ui.allocate_exact_size(Vec2::new(8.0, 14.0), Sense::hover());
        ui.painter()
            .circle_filled(dot_rect.center(), 3.0, dot_color);
    } else {
        let (dot_rect, _) = ui.allocate_exact_size(Vec2::new(8.0, 14.0), Sense::hover());
        ui.painter()
            .circle_filled(dot_rect.center(), 3.0, dot_color);
        ui.label(egui::RichText::new(label).color(text_color).size(12.0));
    }
}

/// Crisp 1px horizontal hairline divider.
pub fn hairline_divider(ui: &mut egui::Ui) {
    let width = ui.available_width().max(1.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 1.0), Sense::hover());
    ui.painter().line_segment(
        [
            Pos2::new(rect.left(), rect.center().y),
            Pos2::new(rect.right(), rect.center().y),
        ],
        Stroke::new(1.0, BORDER_MUTED),
    );
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

/// Draws a calibrated 270° instrument dial with outer ticks, animated arc, and tabular readout.
pub fn draw_radial_gauge(
    ui: &mut egui::Ui,
    id: egui::Id,
    value_c: Option<f32>,
    max_c: f32,
    sublabel: &str,
    range_text: Option<&str>,
    accent: Color32,
) {
    let size = Vec2::splat(120.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter();
    let center = rect.center();
    let radius = 46.0;
    let stroke_w = 4.5;

    // Subtle outer calibration ticks at 0%, 25%, 50%, 75%, 100%.
    for step in 0..=4 {
        let angle = GAUGE_START + GAUGE_SWEEP * (step as f32 / 4.0);
        let dir = Vec2::new(angle.cos(), angle.sin());
        painter.line_segment(
            [center + dir * (radius + 5.0), center + dir * (radius + 8.5)],
            Stroke::new(1.0, BORDER_ACTIVE),
        );
    }

    let track = arc_points(center, radius, GAUGE_START, GAUGE_SWEEP, 64);
    painter.add(Shape::line(track, Stroke::new(stroke_w, BG_CONTROL)));

    let target_ratio = gauge_ratio(value_c, max_c);
    let ratio = ui
        .ctx()
        .animate_value_with_time(id, target_ratio, 0.22)
        .clamp(0.0, 1.0);
    if ratio > 0.005 {
        let segs = ((64.0 * ratio) as usize).max(2);
        let end_angle = GAUGE_START + GAUGE_SWEEP * ratio;
        let arc = arc_points(center, radius, GAUGE_START, GAUGE_SWEEP * ratio, segs);
        painter.add(Shape::line(arc, Stroke::new(stroke_w, accent)));
        let tip = center + Vec2::new(end_angle.cos(), end_angle.sin()) * radius;
        painter.circle_filled(tip, 3.2, accent);
    }

    painter.text(
        center - Vec2::new(0.0, 6.0),
        Align2::CENTER_CENTER,
        format_temp(value_c),
        FontId::monospace(19.0),
        TEXT_PRIMARY,
    );
    painter.text(
        center + Vec2::new(0.0, 14.0),
        Align2::CENTER_CENTER,
        sublabel,
        FontId::proportional(11.5),
        TEXT_MUTED,
    );
    if let Some(range) = range_text {
        painter.text(
            center + Vec2::new(0.0, 40.0),
            Align2::CENTER_CENTER,
            range,
            FontId::monospace(10.5),
            TEXT_MUTED,
        );
    }
}

// ------------------------------------------------------------ Unified 16px 1.5-stroke icons

fn icon_stroke(size: f32, color: Color32) -> Stroke {
    let w = (size / 16.0 * ICON_STROKE).clamp(1.2, 2.0);
    Stroke::new(w, color)
}

/// Flat geometric brand emblem (single ring + 3 inner fan vents + 12-o'clock accent notch).
pub fn draw_brand_mark(ui: &mut egui::Ui, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let painter = ui.painter();
    let c = rect.center();
    let r = size * 0.42;
    painter.circle_stroke(c, r, Stroke::new(1.5, TEXT_PRIMARY));
    painter.circle_filled(c, size * 0.09, TEXT_PRIMARY);
    for i in 0..3 {
        let a = -std::f32::consts::FRAC_PI_2 + i as f32 * (std::f32::consts::TAU / 3.0);
        let dir = Vec2::new(a.cos(), a.sin());
        painter.line_segment(
            [c + dir * (size * 0.14), c + dir * (r * 0.82)],
            Stroke::new(1.4, TEXT_SECONDARY),
        );
    }
    // Single quiet accent index mark at 12 o'clock.
    painter.line_segment(
        [Pos2::new(c.x, c.y - r - 1.5), Pos2::new(c.x, c.y - r + 2.5)],
        Stroke::new(1.8, ACCENT),
    );
}

/// 4-quadrant rounded grid icon for Dashboard navigation.
pub fn draw_nav_dashboard_icon(ui: &mut egui::Ui, size: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let painter = ui.painter();
    let c = rect.center();
    let stroke = icon_stroke(size, color);
    let cell = size * 0.30;
    let offset = size * 0.21;
    for dy in [-offset, offset] {
        for dx in [-offset, offset] {
            let box_rect = Rect::from_center_size(c + Vec2::new(dx, dy), Vec2::splat(cell));
            painter.rect_stroke(box_rect, CornerRadius::same(2), stroke, StrokeKind::Inside);
        }
    }
}

/// Clean 3-blade rotor outline icon for Fans navigation and fan devices.
pub fn draw_fan_icon(ui: &mut egui::Ui, size: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let painter = ui.painter();
    let c = rect.center();
    let stroke = icon_stroke(size, color);
    let outer_r = size * 0.42;
    let hub_r = size * 0.11;
    painter.circle_stroke(c, outer_r, stroke);
    painter.circle_filled(c, hub_r, color);
    for i in 0..3 {
        let a = -std::f32::consts::FRAC_PI_2 + i as f32 * (std::f32::consts::TAU / 3.0);
        let dir = Vec2::new(a.cos(), a.sin());
        let perp = Vec2::new(-a.sin(), a.cos());
        let tip = c + dir * (outer_r * 0.82) + perp * (size * 0.08);
        let ctrl = c + dir * (outer_r * 0.48) - perp * (size * 0.07);
        painter.add(Shape::line(vec![c + dir * hub_r, ctrl, tip], stroke));
    }
}

/// Concentric light-ring icon for RGB navigation.
pub fn draw_nav_rgb_icon(ui: &mut egui::Ui, size: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let painter = ui.painter();
    let c = rect.center();
    let stroke = icon_stroke(size, color);
    painter.circle_stroke(c, size * 0.40, stroke);
    painter.circle_stroke(c, size * 0.17, stroke);
}

/// Horizontal slider lines icon for Profiles navigation.
pub fn draw_nav_profiles_icon(ui: &mut egui::Ui, size: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let painter = ui.painter();
    let c = rect.center();
    let stroke = icon_stroke(size, color);
    let half_w = size * 0.36;
    let rows = [(-0.24, -0.10), (0.0, 0.14), (0.24, -0.06)];
    for (dy_f, knob_f) in rows {
        let y = c.y + size * dy_f;
        let knob_x = c.x + size * knob_f;
        let gap = size * 0.10;
        painter.line_segment(
            [Pos2::new(c.x - half_w, y), Pos2::new(knob_x - gap, y)],
            stroke,
        );
        painter.line_segment(
            [Pos2::new(knob_x + gap, y), Pos2::new(c.x + half_w, y)],
            stroke,
        );
        painter.circle_stroke(Pos2::new(knob_x, y), size * 0.08, stroke);
    }
}

/// Monochrome liquid-pump impeller icon.
pub fn draw_pump_icon(ui: &mut egui::Ui, size: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let painter = ui.painter();
    let c = rect.center();
    let stroke = icon_stroke(size, color);
    let outer_r = size * 0.42;
    let inner_r = size * 0.22;
    painter.circle_stroke(c, outer_r, stroke);
    painter.circle_stroke(c, inner_r, stroke);
    for i in 0..4 {
        let a = std::f32::consts::FRAC_PI_4 + i as f32 * std::f32::consts::FRAC_PI_2;
        let dir = Vec2::new(a.cos(), a.sin());
        painter.line_segment([c + dir * inner_r, c + dir * (outer_r * 0.92)], stroke);
    }
}

/// Monochrome palette swatch-ring line icon for Pywal Sync.
pub fn draw_palette_icon(ui: &mut egui::Ui, size: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let painter = ui.painter();
    let c = rect.center();
    let r = size * 0.40;
    let stroke = icon_stroke(size, color);
    painter.circle_stroke(c, r, stroke);
    let dot_r = size * 0.07;
    painter.circle_filled(c + Vec2::new(-r * 0.40, -r * 0.22), dot_r, color);
    painter.circle_filled(c + Vec2::new(r * 0.38, -r * 0.22), dot_r, color);
    painter.circle_filled(c + Vec2::new(0.0, r * 0.42), dot_r, color);
}

/// LED emitter line icon for RGB strips and channels.
pub fn draw_led_icon(ui: &mut egui::Ui, size: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let painter = ui.painter();
    let c = rect.center();
    let stroke = icon_stroke(size, color);
    painter.circle_stroke(c, size * 0.18, stroke);
    let inner = size * 0.28;
    let outer = size * 0.42;
    for i in 0..6 {
        let a = i as f32 * (std::f32::consts::TAU / 6.0);
        let dir = Vec2::new(a.cos(), a.sin());
        painter.line_segment([c + dir * inner, c + dir * outer], stroke);
    }
}

/// Concentric ring icon for `ring` RGB channels.
pub fn draw_ring_icon(ui: &mut egui::Ui, size: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let painter = ui.painter();
    let c = rect.center();
    let stroke = icon_stroke(size, color);
    painter.circle_stroke(c, size * 0.36, stroke);
    painter.circle_stroke(c, size * 0.16, stroke);
}

/// Linked ring icon for `sync` RGB channels.
pub fn draw_sync_icon(ui: &mut egui::Ui, size: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let painter = ui.painter();
    let c = rect.center();
    let r = size * 0.34;
    let stroke = icon_stroke(size, color);
    painter.circle_stroke(c, r, stroke);
    let node_r = size * 0.08;
    painter.circle_filled(c + Vec2::new(0.0, -r), node_r, color);
    painter.circle_filled(c + Vec2::new(0.0, r), node_r, color);
}

// ---------------------------------------------------- Quiet interactive controls

/// Quiet pushpin toggle button with 140ms hover transition.
pub fn pin_button(ui: &mut egui::Ui, pinned: bool, tooltip: &str) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::click());
    let resp = resp
        .on_hover_text(tooltip)
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let hover_t = ui.ctx().animate_bool_with_time_and_easing(
        resp.id.with("hover"),
        resp.hovered(),
        0.14,
        easing::quadratic_out,
    );
    let painter = ui.painter();

    if hover_t > 0.01 {
        painter.rect_filled(rect, RADIUS_CHIP, BG_CONTROL.gamma_multiply(hover_t));
        painter.rect_stroke(
            rect,
            RADIUS_CHIP,
            Stroke::new(1.0, BORDER_ACTIVE.gamma_multiply(hover_t)),
            StrokeKind::Inside,
        );
    }

    let c = rect.center();
    let stroke_col = if pinned {
        if resp.hovered() {
            TEXT_PRIMARY
        } else {
            TEXT_SECONDARY
        }
    } else if resp.hovered() {
        TEXT_SECONDARY
    } else {
        TEXT_MUTED.gamma_multiply(0.65)
    };
    let fill_col = if pinned {
        stroke_col.gamma_multiply(0.25)
    } else {
        Color32::TRANSPARENT
    };

    painter.line_segment(
        [
            Pos2::new(c.x - 4.0, c.y - 4.8),
            Pos2::new(c.x + 4.0, c.y - 4.8),
        ],
        Stroke::new(ICON_STROKE, stroke_col),
    );
    let body = vec![
        Pos2::new(c.x - 2.6, c.y - 4.8),
        Pos2::new(c.x + 2.6, c.y - 4.8),
        Pos2::new(c.x + 4.4, c.y + 1.2),
        Pos2::new(c.x - 4.4, c.y + 1.2),
    ];
    painter.add(Shape::convex_polygon(
        body,
        fill_col,
        Stroke::new(ICON_STROKE, stroke_col),
    ));
    painter.line_segment(
        [Pos2::new(c.x, c.y + 1.2), Pos2::new(c.x, c.y + 6.0)],
        Stroke::new(ICON_STROKE, stroke_col),
    );

    resp.clicked()
}

/// Quiet 2x3 grip dot handle for dragging dashboard cards; brightens smoothly on hover/drag.
pub fn drag_handle(ui: &mut egui::Ui, id: egui::Id, tooltip: &str) -> egui::Response {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(12.0, 18.0), Sense::hover());
    let resp = ui
        .interact(rect, id, Sense::drag())
        .on_hover_text(tooltip)
        .on_hover_cursor(egui::CursorIcon::Grab);
    if resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }

    let hover_t = ui.ctx().animate_bool_with_time_and_easing(
        id.with("hover"),
        resp.hovered() || resp.dragged(),
        0.14,
        easing::quadratic_out,
    );
    let c = rect.center();
    let col = if resp.dragged() {
        ACCENT
    } else {
        TEXT_MUTED.gamma_multiply(0.40 + 0.60 * hover_t)
    };
    let painter = ui.painter();
    for dy in [-4.0, 0.0, 4.0] {
        for dx in [-2.2, 2.2] {
            painter.circle_filled(Pos2::new(c.x + dx, c.y + dy), 1.2, col);
        }
    }

    resp
}

/// Quiet bottom-edge resize handle; invisible idle, fades in on hover or drag.
pub fn card_resize_handle(
    ui: &mut egui::Ui,
    id: egui::Id,
    current_h: f32,
    min_h: f32,
    max_h: f32,
) -> f32 {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 8.0), Sense::hover());
    let resp = ui
        .interact(rect, id, Sense::drag())
        .on_hover_cursor(egui::CursorIcon::ResizeVertical);
    if resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }

    let active_t = ui.ctx().animate_bool_with_time_and_easing(
        id.with("hover"),
        resp.hovered() || resp.dragged(),
        0.14,
        easing::quadratic_out,
    );
    if active_t > 0.01 {
        let col = if resp.dragged() {
            ACCENT
        } else {
            BORDER_ACTIVE.gamma_multiply(active_t)
        };
        let pill = Rect::from_center_size(rect.center(), Vec2::new(32.0, 2.0));
        ui.painter().rect_filled(pill, CornerRadius::same(1), col);
    }

    (current_h + resp.drag_delta().y).clamp(min_h, max_h)
}

/// Quiet 16px vertical column splitter channel; invisible idle, fades in on hover or drag.
pub fn column_splitter(ui: &mut egui::Ui, id: egui::Id, height: f32) -> egui::Response {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(16.0, height.max(120.0)), Sense::hover());
    let resp = ui
        .interact(rect, id, Sense::click_and_drag())
        .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
    if resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }

    let active_t = ui.ctx().animate_bool_with_time_and_easing(
        id.with("hover"),
        resp.hovered() || resp.dragged(),
        0.14,
        easing::quadratic_out,
    );
    if active_t > 0.01 {
        let col = if resp.dragged() {
            ACCENT
        } else {
            BORDER_ACTIVE.gamma_multiply(active_t)
        };
        let bar = Rect::from_center_size(
            rect.center(),
            Vec2::new(2.0, (rect.height() - 24.0).max(24.0)),
        );
        ui.painter().rect_filled(bar, CornerRadius::same(1), col);
    }

    resp
}

/// Draws a calibrated PWM channel bar with 16px muted icon, tabular RPM/duty, and 25/50/75% ticks.
pub fn draw_pwm_bar(
    ui: &mut egui::Ui,
    id: egui::Id,
    title: &str,
    rpm: Option<f64>,
    duty: f32,
    is_pump: bool,
) {
    ui.horizontal(|ui| {
        if is_pump {
            draw_pump_icon(ui, 16.0, TEXT_MUTED);
        } else {
            draw_fan_icon(ui, 16.0, TEXT_MUTED);
        }
        ui.label(egui::RichText::new(title).color(TEXT_PRIMARY).size(13.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.allocate_ui_with_layout(
                Vec2::new(36.0, 16.0),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    ui.label(
                        egui::RichText::new(format!("{duty:.0}%"))
                            .color(TEXT_PRIMARY)
                            .monospace()
                            .size(12.5),
                    );
                },
            );
            ui.label(egui::RichText::new("·").color(TEXT_MUTED).size(12.5));
            ui.label(
                egui::RichText::new(format_rpm(rpm))
                    .color(TEXT_SECONDARY)
                    .monospace()
                    .size(12.5),
            );
        });
    });
    ui.add_space(6.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 6.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, RADIUS_TRACK, BG_CONTROL);

    let anim_duty = ui
        .ctx()
        .animate_value_with_time(id, duty_ratio(duty), 0.22)
        .clamp(0.0, 1.0);
    let fill_w = (rect.width() * anim_duty).max(0.0);
    if fill_w > 0.0 {
        let fill = Rect::from_min_size(rect.min, Vec2::new(fill_w, rect.height()));
        let fill_color = if is_pump { ACCENT } else { PWM_FAN_FILL };
        painter.rect_filled(fill, RADIUS_TRACK, fill_color);
    }

    // Calibrated 25%, 50%, 75% scale ticks across the track.
    for frac in [0.25_f32, 0.50, 0.75] {
        let x = rect.left() + rect.width() * frac;
        painter.line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            Stroke::new(1.0, BG_CARD),
        );
    }
    ui.add_space(12.0);
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
        assert_eq!(BG_SIDEBAR, Color32::from_rgb(0x0F, 0x0F, 0x10));
        assert_eq!(BG_CANVAS, Color32::from_rgb(0x14, 0x14, 0x15));
        assert_eq!(BG_CARD, Color32::from_rgb(0x19, 0x19, 0x1B));
        assert_eq!(BG_CONTROL, Color32::from_rgb(0x23, 0x23, 0x25));
        assert_eq!(TEXT_PRIMARY, Color32::from_rgb(0xED, 0xED, 0xED));
        assert_eq!(TEXT_SECONDARY, Color32::from_rgb(0xA1, 0xA1, 0xA6));
        assert_eq!(TEXT_MUTED, Color32::from_rgb(0x85, 0x85, 0x8B));
        assert_eq!(ACCENT, Color32::from_rgb(0x4E, 0xA8, 0xDE));
        assert_eq!(SERIES_GPU, Color32::from_rgb(0xD9, 0x8A, 0x4E));
        assert_eq!(SERIES_AIO, Color32::from_rgb(0x4C, 0xC3, 0x8A));
        assert_eq!(SEMANTIC_OK, Color32::from_rgb(0x4C, 0xC3, 0x8A));
        assert_eq!(SEMANTIC_WARN, Color32::from_rgb(0xE0, 0x9F, 0x3E));
        assert_eq!(SEMANTIC_ERR, Color32::from_rgb(0xE5, 0x48, 0x4D));
    }

    #[test]
    fn formatting_and_range_helpers_work() {
        assert_eq!(format_temp(None), "--.-°C");
        assert_eq!(format_temp(Some(f32::NAN)), "--.-°C");
        assert_eq!(format_temp(Some(44.24)), "44.2°C");
        assert_eq!(format_rpm(None), "-- RPM");
        assert_eq!(format_rpm(Some(850.0)), "850 RPM");
        assert_eq!(format_rpm(Some(1263.4)), "1,263 RPM");

        let mut hist = VecDeque::from([42.0, 48.5, 39.2]);
        assert_eq!(temp_range(&hist), Some((39.2, 48.5)));
        hist.clear();
        assert_eq!(temp_range(&hist), None);

        assert_eq!(temp_semantic_color(None), TEXT_MUTED);
        assert_eq!(temp_semantic_color(Some(45.0)), TEXT_PRIMARY);
        assert_eq!(temp_semantic_color(Some(60.0)), SEMANTIC_WARN);
        assert_eq!(temp_semantic_color(Some(80.0)), SEMANTIC_ERR);
    }
}
