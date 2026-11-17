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

/// Returns `(key, hex, rgb)` entries from `color0..color15`, optionally filtering extreme
/// black/white colors and rotating left by `offset`.
pub fn active_palette_entries(
    palette: &PywalPalette,
    filter_extremes: bool,
    offset: usize,
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

    if !all.is_empty() {
        let shift = offset % all.len();
        all.rotate_left(shift);
    }
    all
}

fn select_primary_hex(palette: &PywalPalette, pywal: &PywalConfig) -> Option<String> {
    if !pywal.filter_extremes && pywal.palette_offset == 0 {
        return palette
            .color_hex("color4")
            .or_else(|| palette.color_hex("color1"))
            .map(str::to_string);
    }
    active_palette_entries(palette, pywal.filter_extremes, pywal.palette_offset)
        .into_iter()
        .next()
        .map(|(_, hex, _)| hex)
        .or_else(|| {
            palette
                .color_hex("color4")
                .or_else(|| palette.color_hex("color1"))
                .map(str::to_string)
        })
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
        for target in &pywal.targets {
            let hex = palette.color_hex(&target.color_key).ok_or_else(|| {
                CoreError::Validation(format!(
                    "pywal palette does not contain color key '{}'",
                    target.color_key
                ))
            })?;
            backend.set_color(
                &target.match_device,
                &target.channel,
                &target.mode,
                &[hex.to_string()],
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
}
