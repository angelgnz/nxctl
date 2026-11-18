use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::PywalConfig;
use crate::error::{CoreError, Result};
use crate::liquidctl::LiquidctlBackend;

/// Parsed pywal color palette from `~/.cache/wal/colors.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PywalPalette {
    #[serde(default)]
    pub wallpaper: Option<String>,
    #[serde(default)]
    pub colors: BTreeMap<String, String>,
}

impl PywalPalette {
    /// Returns the normalized 6-character lowercase hex color (without `#`) for `key`.
    pub fn color_hex(&self, key: &str) -> Option<&str> {
        self.colors.get(key).map(String::as_str)
    }

    /// Parses and returns the `[r, g, b]` tuple for `key`, if present and valid 6-char hex.
    pub fn color_rgb(&self, key: &str) -> Option<[u8; 3]> {
        let hex = self.color_hex(key)?;
        let normalized = hex.trim().trim_start_matches('#');
        if normalized.len() != 6 || !normalized.is_ascii() {
            return None;
        }
        let r = u8::from_str_radix(&normalized[0..2], 16).ok()?;
        let g = u8::from_str_radix(&normalized[2..4], 16).ok()?;
        let b = u8::from_str_radix(&normalized[4..6], 16).ok()?;
        Some([r, g, b])
    }
}

/// Returns the default pywal `colors.json` path (`~/.cache/wal/colors.json`).
pub fn default_colors_json_path() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("wal").join("colors.json"))
}

/// Parses a pywal `colors.json` payload, normalizing hex values to lowercase 6-char hex without `#`.
pub fn parse_pywal_colors_json(json: &str) -> Result<PywalPalette> {
    let mut palette: PywalPalette = serde_json::from_str(json)?;
    for (key, value) in &mut palette.colors {
        let normalized = value.trim().trim_start_matches('#').to_ascii_lowercase();
        if normalized.len() != 6 || !normalized.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(CoreError::Validation(format!(
                "invalid pywal hex color '{value}' for key '{key}'"
            )));
        }
        *value = normalized;
    }
    Ok(palette)
}

/// Loads and parses pywal's `colors.json` from `path`, or [`default_colors_json_path`] when `None`.
pub fn load_pywal_palette(path: Option<&Path>) -> Result<PywalPalette> {
    let resolved = match path {
        Some(p) => p.to_path_buf(),
        None => default_colors_json_path().ok_or_else(|| {
            CoreError::Validation(
                "could not determine user cache directory for pywal colors.json".to_string(),
            )
        })?,
    };
    let contents = fs::read_to_string(&resolved)?;
    parse_pywal_colors_json(&contents)
}

/// Returns `true` when `rgb` is too dark (near black) or low-saturation bright (near white) for RGB LEDs.
pub fn is_extreme_bw(rgb: [u8; 3]) -> bool {
    let [r, g, b] = rgb;
    let luma = 0.2126 * f32::from(r) + 0.7152 * f32::from(g) + 0.0722 * f32::from(b);
    let max_c = r.max(g).max(b);
    let min_c = r.min(g).min(b);
    let chroma = max_c - min_c;
    luma < 32.0 || max_c < 45 || (luma > 205.0 && chroma < 42)
}

/// Normalizes a saturation setting (`0.0..=1.0` or percentage `1.0..=100.0`) into `0.0..=1.0`.
pub fn normalize_saturate(val: f32) -> f32 {
    if !val.is_finite() || val <= 0.0 {
        0.0
    } else if val > 1.0 {
        (val / 100.0).clamp(0.0, 1.0)
    } else {
        val.clamp(0.0, 1.0)
    }
}

/// Formats an `[r, g, b]` tuple as a 6-character lowercase hex string without `#`.
pub fn rgb_to_hex(rgb: [u8; 3]) -> String {
    format!("{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])
}

/// Boosts HSV saturation and value of `rgb` by `amount` (`0.0..=1.0` or `0..=100`)
/// so muted pywal colors appear vivid on hardware RGB LEDs.
/// Near-neutral greys (`saturation < 0.05`) are returned unchanged.
pub fn saturate_rgb(rgb: [u8; 3], amount: f32) -> [u8; 3] {
    let amt = normalize_saturate(amount);
    if amt <= 0.0 {
        return rgb;
    }
    let rf = f32::from(rgb[0]) / 255.0;
    let gf = f32::from(rgb[1]) / 255.0;
    let bf = f32::from(rgb[2]) / 255.0;
    let max = rf.max(gf).max(bf);
    let min = rf.min(gf).min(bf);
    let delta = max - min;
    if max <= 1e-5 || delta <= 1e-5 {
        return rgb;
    }
    let s = delta / max;
    if s < 0.05 {
        return rgb;
    }
    let h = if (max - rf).abs() < 1e-5 {
        60.0 * (((gf - bf) / delta).rem_euclid(6.0))
    } else if (max - gf).abs() < 1e-5 {
        60.0 * (((bf - rf) / delta) + 2.0)
    } else {
        60.0 * (((rf - gf) / delta) + 4.0)
    };
    let new_s = (s + (1.0 - s) * amt).clamp(0.0, 1.0);
    let new_v = if max > 0.10 {
        (max + (1.0 - max) * (amt * 0.65)).clamp(0.0, 1.0)
    } else {
        max
    };
    let c = new_v * new_s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = new_v - c;
    let (r1, g1, b1) = match (h / 60.0).floor() as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [
        ((r1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((g1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((b1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
    ]
}

/// Returns `(key, hex, rgb)` entries from `color0..color15`, optionally filtering extreme
/// black/white colors and rotating left by `offset`.
pub fn active_palette_entries(
    palette: &PywalPalette,
    filter_extremes: bool,
    offset: usize,
) -> Vec<(String, String, [u8; 3])> {
    active_palette_entries_vivid(palette, filter_extremes, offset, 0.0)
}

/// Returns `(key, hex, rgb)` entries from `color0..color15`, optionally filtering extreme
/// black/white colors, boosting HSV vividness by `saturate`, and rotating left by `offset`.
pub fn active_palette_entries_vivid(
    palette: &PywalPalette,
    filter_extremes: bool,
    offset: usize,
    saturate: f32,
) -> Vec<(String, String, [u8; 3])> {
    let mut all: Vec<(String, String, [u8; 3])> = (0..16)
        .filter_map(|i| {
            let key = format!("color{i}");
            let hex = palette.color_hex(&key)?.to_string();
            let rgb = palette.color_rgb(&key)?;
            Some((key, hex, rgb))
        })
        .collect();

    if filter_extremes {
        let filtered: Vec<_> = all
            .iter()
            .filter(|(_, _, rgb)| !is_extreme_bw(*rgb))
            .cloned()
            .collect();
        if !filtered.is_empty() {
            all = filtered;
        }
    }

    let sat = normalize_saturate(saturate);
    if sat > 0.0 {
        for (_, hex, rgb) in &mut all {
            let vivid = saturate_rgb(*rgb, sat);
            *rgb = vivid;
            *hex = rgb_to_hex(vivid);
        }
    }

    if !all.is_empty() {
        let shift = offset % all.len();
        all.rotate_left(shift);
    }
    all
}

/// Resolves the primary RGB color chosen for automatic pywal synchronization.
pub fn select_primary_pywal_rgb(palette: &PywalPalette, pywal: &PywalConfig) -> Option<[u8; 3]> {
    let sat = normalize_saturate(pywal.saturate);
    if !pywal.filter_extremes && pywal.palette_offset == 0 {
        return palette
            .color_rgb("color4")
            .or_else(|| palette.color_rgb("color1"))
            .map(|rgb| saturate_rgb(rgb, sat));
    }
    active_palette_entries_vivid(palette, pywal.filter_extremes, pywal.palette_offset, sat)
        .into_iter()
        .next()
        .map(|(_, _, rgb)| rgb)
        .or_else(|| {
            palette
                .color_rgb("color4")
                .or_else(|| palette.color_rgb("color1"))
                .map(|rgb| saturate_rgb(rgb, sat))
        })
}

fn select_primary_hex(palette: &PywalPalette, pywal: &PywalConfig) -> Option<String> {
    select_primary_pywal_rgb(palette, pywal).map(rgb_to_hex)
}

/// Applies colors from a [`PywalPalette`] to liquidctl devices according to [`PywalConfig`].
///
/// Returns the number of `set_color` calls issued.
pub fn apply_pywal_palette(
    backend: &dyn LiquidctlBackend,
    pywal: &PywalConfig,
    palette: &PywalPalette,
    dry_run: bool,
) -> Result<usize> {
    let mut applied = 0;

    if !pywal.targets.is_empty() {
        let sat = normalize_saturate(pywal.saturate);
        for target in &pywal.targets {
            let mut rgb = palette.color_rgb(&target.color_key).ok_or_else(|| {
                CoreError::Validation(format!(
                    "pywal palette does not contain color key '{}'",
                    target.color_key
                ))
            })?;
            if sat > 0.0 {
                rgb = saturate_rgb(rgb, sat);
            }
            let hex = rgb_to_hex(rgb);
            backend.set_color(
                &target.match_device,
                &target.channel,
                &target.mode,
                &[hex],
                dry_run,
            )?;
            applied += 1;
        }
        return Ok(applied);
    }

    let hex = select_primary_hex(palette, pywal).ok_or_else(|| {
        CoreError::Validation(
            "pywal palette does not contain fallback color keys 'color4' or 'color1'".to_string(),
        )
    })?;

    let mode = if pywal.mode.trim().is_empty() {
        "fixed"
    } else {
        pywal.mode.trim()
    };

    let statuses = backend.status(None)?;
    let devices: Vec<&str> = statuses
        .iter()
        .map(|d| d.description.trim())
        .filter(|desc| !desc.is_empty())
        .collect();

    if !devices.is_empty() {
        for desc in devices {
            backend.set_color(desc, "sync", mode, std::slice::from_ref(&hex), dry_run)?;
            applied += 1;
        }
    } else {
        for fallback_match in ["smart", "kraken"] {
            backend.set_color(
                fallback_match,
                "sync",
                mode,
                std::slice::from_ref(&hex),
                dry_run,
            )?;
            applied += 1;
        }
    }

    Ok(applied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PywalTarget;
    use crate::liquidctl::{DeviceStatus, MockCall, MockLiquidctlBackend};

    const SAMPLE_WAL_JSON: &str = r##"{
        "wallpaper": "/home/user/Pictures/wall.png",
        "alpha": "100",
        "special": {
            "background": "#0f0a19",
            "foreground": "#e1d9f0",
            "cursor": "#e1d9f0"
        },
        "colors": {
            "color0": "#0F0A19",
            "color1": "#A348B5",
            "color4": "#5E81AC"
        }
    }"##;

    #[test]
    fn parse_pywal_colors_json_normalizes_hex_and_wallpaper() {
        let palette = parse_pywal_colors_json(SAMPLE_WAL_JSON).expect("valid pywal json");
        assert_eq!(
            palette.wallpaper.as_deref(),
            Some("/home/user/Pictures/wall.png")
        );
        assert_eq!(palette.color_hex("color0"), Some("0f0a19"));
        assert_eq!(palette.color_hex("color1"), Some("a348b5"));
        assert_eq!(palette.color_hex("color4"), Some("5e81ac"));
        assert_eq!(palette.color_hex("color9"), None);
    }

    #[test]
    fn parse_pywal_colors_json_rejects_invalid_hex() {
        let bad = r##"{ "colors": { "color0": "#ZZZZZZ" } }"##;
        let err = parse_pywal_colors_json(bad).expect_err("should reject non-hex");
        assert!(matches!(err, CoreError::Validation(_)));
    }

    #[test]
    fn color_rgb_decodes_hex_bytes() {
        let palette = parse_pywal_colors_json(SAMPLE_WAL_JSON).expect("valid pywal json");
        assert_eq!(palette.color_rgb("color0"), Some([0x0f, 0x0a, 0x19]));
        assert_eq!(palette.color_rgb("color4"), Some([0x5e, 0x81, 0xac]));
        assert_eq!(palette.color_rgb("missing"), None);
    }

    #[test]
    fn apply_pywal_palette_uses_explicit_targets() {
        let palette = parse_pywal_colors_json(SAMPLE_WAL_JSON).expect("valid pywal json");
        let cfg = PywalConfig {
            enabled: true,
            targets: vec![
                PywalTarget {
                    match_device: "kraken".to_string(),
                    channel: "ring".to_string(),
                    color_key: "color4".to_string(),
                    mode: "fixed".to_string(),
                },
                PywalTarget {
                    match_device: "smart".to_string(),
                    channel: "sync".to_string(),
                    color_key: "color1".to_string(),
                    mode: "breathing".to_string(),
                },
            ],
            ..PywalConfig::default()
        };
        let mock = MockLiquidctlBackend::new();

        let count = apply_pywal_palette(&mock, &cfg, &palette, true).expect("apply succeeds");
        assert_eq!(count, 2);
        assert_eq!(
            mock.calls(),
            vec![
                MockCall::SetColor {
                    match_device: "kraken".to_string(),
                    channel: "ring".to_string(),
                    mode: "fixed".to_string(),
                    colors: vec!["5e81ac".to_string()],
                    dry_run: true,
                },
                MockCall::SetColor {
                    match_device: "smart".to_string(),
                    channel: "sync".to_string(),
                    mode: "breathing".to_string(),
                    colors: vec!["a348b5".to_string()],
                    dry_run: true,
                },
            ]
        );
    }

    #[test]
    fn apply_pywal_palette_errors_on_missing_target_color_key() {
        let palette = parse_pywal_colors_json(SAMPLE_WAL_JSON).expect("valid pywal json");
        let cfg = PywalConfig {
            enabled: true,
            targets: vec![PywalTarget {
                match_device: "kraken".to_string(),
                channel: "sync".to_string(),
                color_key: "color15".to_string(),
                mode: "fixed".to_string(),
            }],
            ..PywalConfig::default()
        };
        let mock = MockLiquidctlBackend::new();

        let err = apply_pywal_palette(&mock, &cfg, &palette, false)
            .expect_err("missing color_key should fail");
        assert!(matches!(err, CoreError::Validation(_)));
        assert!(mock.calls().is_empty());
    }

    #[test]
    fn apply_pywal_palette_empty_targets_uses_detected_devices() {
        let palette = parse_pywal_colors_json(SAMPLE_WAL_JSON).expect("valid pywal json");
        let cfg = PywalConfig::default();
        let mock = MockLiquidctlBackend::with_statuses(vec![
            DeviceStatus {
                address: None,
                bus: None,
                description: "NZXT Kraken X63".to_string(),
                status: vec![],
            },
            DeviceStatus {
                address: None,
                bus: None,
                description: "NZXT Smart Device V2".to_string(),
                status: vec![],
            },
        ]);

        let count = apply_pywal_palette(&mock, &cfg, &palette, false).expect("apply succeeds");
        assert_eq!(count, 2);
        assert_eq!(
            mock.calls(),
            vec![
                MockCall::Status { match_device: None },
                MockCall::SetColor {
                    match_device: "NZXT Kraken X63".to_string(),
                    channel: "sync".to_string(),
                    mode: "fixed".to_string(),
                    colors: vec!["5e81ac".to_string()],
                    dry_run: false,
                },
                MockCall::SetColor {
                    match_device: "NZXT Smart Device V2".to_string(),
                    channel: "sync".to_string(),
                    mode: "fixed".to_string(),
                    colors: vec!["5e81ac".to_string()],
                    dry_run: false,
                },
            ]
        );
    }

    #[test]
    fn apply_pywal_palette_empty_targets_falls_back_when_no_devices_detected() {
        let json_only_color1 = r##"{ "colors": { "color1": "#a348b5" } }"##;
        let palette = parse_pywal_colors_json(json_only_color1).expect("valid pywal json");
        let cfg = PywalConfig::default();
        let mock = MockLiquidctlBackend::new();

        let count = apply_pywal_palette(&mock, &cfg, &palette, false).expect("apply succeeds");
        assert_eq!(count, 2);
        assert_eq!(
            mock.calls(),
            vec![
                MockCall::Status { match_device: None },
                MockCall::SetColor {
                    match_device: "smart".to_string(),
                    channel: "sync".to_string(),
                    mode: "fixed".to_string(),
                    colors: vec!["a348b5".to_string()],
                    dry_run: false,
                },
                MockCall::SetColor {
                    match_device: "kraken".to_string(),
                    channel: "sync".to_string(),
                    mode: "fixed".to_string(),
                    colors: vec!["a348b5".to_string()],
                    dry_run: false,
                },
            ]
        );
    }

    #[test]
    fn filters_extreme_bw_and_rotates_palette() {
        assert!(is_extreme_bw([15, 10, 25]));
        assert!(is_extreme_bw([240, 242, 245]));
        assert!(!is_extreme_bw([0, 240, 255]));
        assert!(!is_extreme_bw([220, 98, 248]));

        let json = r##"{
            "colors": {
                "color0": "#0f0a19",
                "color1": "#345199",
                "color2": "#dc62f8",
                "color7": "#f1f5f9"
            }
        }"##;
        let palette = parse_pywal_colors_json(json).expect("valid json");
        let filtered = active_palette_entries(&palette, true, 0);
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].0, "color1");
        assert_eq!(filtered[1].0, "color2");

        let rotated = active_palette_entries(&palette, true, 1);
        assert_eq!(rotated[0].0, "color2");
        assert_eq!(rotated[1].0, "color1");
    }

    #[test]
    fn saturate_rgb_boosts_vividness_and_preserves_neutrals() {
        assert!((normalize_saturate(75.0) - 0.75).abs() < 1e-6);
        assert!(normalize_saturate(-1.0).abs() < f32::EPSILON);

        // Zero amount leaves color untouched
        let muted = [0x56, 0x64, 0xa2];
        assert_eq!(saturate_rgb(muted, 0.0), muted);

        // Greyscale neutrals stay neutral
        let grey = [0x77, 0x85, 0x9e]; // non-greyscale muted slate vs true neutral
        let neutral = [120, 120, 120];
        assert_eq!(saturate_rgb(neutral, 0.75), neutral);
        assert_eq!(saturate_rgb([10, 10, 10], 1.0), [10, 10, 10]);

        // Muted blue-slate becomes much more vivid (higher chroma & dominant channel)
        let vivid = saturate_rgb(muted, 0.75);
        let muted_chroma = muted.into_iter().max().unwrap() - muted.into_iter().min().unwrap();
        let vivid_chroma = vivid.into_iter().max().unwrap() - vivid.into_iter().min().unwrap();
        assert!(vivid_chroma > muted_chroma);
        assert!(vivid[2] > muted[2]);

        let vivid_slate = saturate_rgb(grey, 0.8);
        assert!(vivid_slate[2] >= grey[2]);

        let palette = parse_pywal_colors_json(SAMPLE_WAL_JSON).expect("valid pywal json");
        let entries = active_palette_entries_vivid(&palette, true, 0, 0.75);
        assert_eq!(entries[0].0, "color1");
        assert_ne!(entries[0].1, "a348b5");
        assert_eq!(entries[0].1, rgb_to_hex(entries[0].2));
    }
}
