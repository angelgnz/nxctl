//! Pure state types, sensor/channel helpers, RGB presets, and profile capture logic for `nxctl tui`.

use std::collections::{BTreeMap, HashMap, HashSet};

use nxctl_core::config::{Config, CoolingRule, CoolingTarget, LightingRule, Profile};
use nxctl_core::liquidctl::{DeviceLightingInfo, DeviceStatus};
use nxctl_core::{HwmonFanInfo, HwmonSensorInfo};

/// Default liquidctl lighting modes used when dynamic driver metadata is unavailable.
pub const RGB_MODES: [&str; 4] = ["fixed", "breathing", "fading", "spectrum-wave"];

/// Default liquidctl lighting channels used when dynamic driver metadata is unavailable.
pub const RGB_CHANNELS: [&str; 4] = ["sync", "led", "ring", "logo"];

/// Curated RGB color presets: `(display_name, hex_rgb, (r, g, b))`.
pub const RGB_PRESETS: [(&str, &str, (u8, u8, u8)); 8] = [
    ("Cyan", "00f0ff", (0x00, 0xf0, 0xff)),
    ("Purple", "7400b8", (0x74, 0x00, 0xb8)),
    ("Magenta", "d946ef", (0xd9, 0x46, 0xef)),
    ("Emerald", "10b981", (0x10, 0xb9, 0x81)),
    ("Amber", "f59e0b", (0xf5, 0x9e, 0x0b)),
    ("Crimson", "ef4444", (0xef, 0x44, 0x44)),
    ("White", "f1f5f9", (0xf1, 0xf5, 0xf9)),
    ("Blue", "3b82f6", (0x3b, 0x82, 0xf6)),
];

/// Top-level navigation views for `nxctl tui`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TuiView {
    #[default]
    Dashboard,
    Fans,
    Rgb,
    Profiles,
}

impl TuiView {
    /// All views in tab order.
    pub const ALL: [TuiView; 4] = [
        TuiView::Dashboard,
        TuiView::Fans,
        TuiView::Rgb,
        TuiView::Profiles,
    ];

    /// Advances to the next view, wrapping from `Profiles` to `Dashboard`.
    pub fn next(self) -> Self {
        match self {
            Self::Dashboard => Self::Fans,
            Self::Fans => Self::Rgb,
            Self::Rgb => Self::Profiles,
            Self::Profiles => Self::Dashboard,
        }
    }

    /// Moves to the previous view, wrapping from `Dashboard` to `Profiles`.
    pub fn prev(self) -> Self {
        match self {
            Self::Dashboard => Self::Profiles,
            Self::Fans => Self::Dashboard,
            Self::Rgb => Self::Fans,
            Self::Profiles => Self::Rgb,
        }
    }

    /// Maps a 1-based numeric shortcut (`1..=4`) to a [`TuiView`].
    pub fn from_number(n: u8) -> Option<Self> {
        match n {
            1 => Some(Self::Dashboard),
            2 => Some(Self::Fans),
            3 => Some(Self::Rgb),
            4 => Some(Self::Profiles),
            _ => None,
        }
    }
}

/// Focusable panels within the Dashboard view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DashboardFocus {
    #[default]
    Featured,
    OtherSensors,
}

/// Focusable sections within the RGB lighting view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RgbFocus {
    #[default]
    Pywal,
    Channels,
}

/// Fan or pump channel extracted from `liquidctl` or `hwmon` status readings.
#[derive(Debug, Clone, PartialEq)]
pub struct ChannelInfo {
    pub device: String,
    pub channel: String,
    pub label: String,
    pub rpm: Option<f64>,
    pub duty: Option<f32>,
}

/// Hardware category assigned to a secondary `hwmon` temperature sensor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensorCategory {
    Ram,
    Ssd,
    Hdd,
    IGpu,
    Wifi,
    Net,
    Sys,
}

/// Selected lighting mode and color state for a single device RGB channel.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChannelRgbState {
    pub mode: usize,
    pub color_idx: usize,
    pub custom_rgb: Option<(u8, u8, u8)>,
}

impl ChannelRgbState {
    /// Returns the active `(r, g, b)` color, preferring any custom override over `color_idx`.
    pub fn rgb(&self) -> (u8, u8, u8) {
        if let Some(rgb) = self.custom_rgb {
            return rgb;
        }
        RGB_PRESETS
            .get(self.color_idx)
            .map(|preset| preset.2)
            .unwrap_or(RGB_PRESETS[0].2)
    }

    /// Formats the active color as 6 lowercase hexadecimal digits without `#`.
    pub fn hex(&self) -> String {
        let (r, g, b) = self.rgb();
        format!("{r:02x}{g:02x}{b:02x}")
    }

    /// Sets an explicit `(r, g, b)` color override.
    pub fn set_rgb(&mut self, rgb: (u8, u8, u8)) {
        if let Some(idx) = RGB_PRESETS
            .iter()
            .position(|(_, _, preset_rgb)| *preset_rgb == rgb)
        {
            self.color_idx = idx;
        }
        self.custom_rgb = Some(rgb);
    }

    /// Clears any custom color override and cycles to the next color preset.
    pub fn next_preset(&mut self) {
        self.custom_rgb = None;
        self.color_idx = (self.color_idx + 1) % RGB_PRESETS.len();
    }

    /// Clears any custom color override and cycles to the previous color preset.
    pub fn prev_preset(&mut self) {
        self.custom_rgb = None;
        self.color_idx = if self.color_idx == 0 {
            RGB_PRESETS.len() - 1
        } else {
            (self.color_idx - 1) % RGB_PRESETS.len()
        };
    }
}

/// Returns a stable identifier for a `hwmon` sensor.
pub fn hwmon_sensor_id(s: &HwmonSensorInfo) -> String {
    format!("hwmon:{}:{}", s.device_dir, s.input_file)
}

/// Extracts `<name> speed` / `<name> duty` pairs from `liquidctl` device status readings.
pub fn extract_channels(devices: &[DeviceStatus]) -> Vec<ChannelInfo> {
    let mut out: Vec<ChannelInfo> = Vec::new();
    for dev in devices {
        for reading in &dev.status {
            let key = reading.key.trim();
            let lower = key.to_ascii_lowercase();
            let (label, is_speed) = if let Some(l) = lower.strip_suffix(" speed") {
                (key[..l.len()].to_string(), true)
            } else if let Some(l) = lower.strip_suffix(" duty") {
                (key[..l.len()].to_string(), false)
            } else {
                continue;
            };
            let Some(v) = reading.value.as_f64() else {
                continue;
            };
            let channel = label.to_ascii_lowercase().replace(' ', "");
            let idx = match out
                .iter()
                .position(|c| c.device == dev.description && c.channel == channel)
            {
                Some(i) => i,
                None => {
                    out.push(ChannelInfo {
                        device: dev.description.clone(),
                        channel,
                        label,
                        rpm: None,
                        duty: None,
                    });
                    out.len() - 1
                }
            };
            if is_speed {
                out[idx].rpm = Some(v);
            } else {
                out[idx].duty = Some(v as f32);
            }
        }
    }
    out
}

/// Extracts controllable `hwmon` PWM channels, skipping NZXT kernel duplicates when `liquidctl` devices exist.
pub fn extract_hwmon_channels(fans: &[HwmonFanInfo], devices: &[DeviceStatus]) -> Vec<ChannelInfo> {
    let has_liquidctl = !devices.is_empty();
    let mut out = Vec::new();
    for f in fans {
        let Some(pwm_file) = f.pwm_file.clone() else {
            continue;
        };
        if f.name.trim().is_empty() {
            continue;
        }
        let lower_name = f.name.to_ascii_lowercase();
        if has_liquidctl && (lower_name.starts_with("nzxt") || lower_name.contains("kraken")) {
            continue;
        }
        let label = f.label.clone().unwrap_or_else(|| pwm_file.clone());
        out.push(ChannelInfo {
            device: format!("hwmon:{}", f.name),
            channel: pwm_file,
            label,
            rpm: f.rpm.map(f64::from),
            duty: f.duty.map(f32::from),
        });
    }
    out
}

/// Groups channels by device name while preserving first-seen device and channel order.
pub fn group_channels_by_device(channels: &[ChannelInfo]) -> Vec<(String, Vec<&ChannelInfo>)> {
    let mut groups: Vec<(String, Vec<&ChannelInfo>)> = Vec::new();
    for ch in channels {
        if let Some((_, list)) = groups.iter_mut().find(|(dev, _)| dev == &ch.device) {
            list.push(ch);
        } else {
            groups.push((ch.device.clone(), vec![ch]));
        }
    }
    groups
}

/// Returns `true` if a cooling channel name represents an AIO or liquid pump.
pub fn is_pump_channel(channel: &str) -> bool {
    let lower = channel.to_ascii_lowercase();
    lower == "pump" || lower.contains("pump") || lower.contains("bomba")
}

/// Classifies a `hwmon` driver name into a hardware [`SensorCategory`].
pub fn classify_hwmon_sensor(name: &str) -> SensorCategory {
    let lower = name.to_ascii_lowercase();
    if lower.contains("spd5118") || lower.contains("jc42") || lower.contains("ddr") {
        SensorCategory::Ram
    } else if lower.contains("nvme") {
        SensorCategory::Ssd
    } else if lower.contains("drivetemp") || lower.contains("sata") || lower.contains("hdd") {
        SensorCategory::Hdd
    } else if lower.contains("amdgpu") || lower.contains("radeon") || lower.contains("nouveau") {
        SensorCategory::IGpu
    } else if lower.contains("wifi")
        || lower.contains("mt79")
        || lower.contains("iwlwifi")
        || lower.contains("ath")
    {
        SensorCategory::Wifi
    } else if lower.contains("r8169")
        || lower.contains("igc")
        || lower.contains("e1000")
        || lower.contains("eth")
    {
        SensorCategory::Net
    } else {
        SensorCategory::Sys
    }
}

/// Filters out primary CPU and AIO/liquid sensors already shown in featured hero cards.
pub fn secondary_hwmon_sensors(sensors: &[HwmonSensorInfo]) -> Vec<&HwmonSensorInfo> {
    sensors
        .iter()
        .filter(|s| {
            let lower = s.name.to_ascii_lowercase();
            let is_cpu = matches!(
                lower.as_str(),
                "k10temp" | "coretemp" | "zenpower" | "cpu_thermal"
            );
            let is_aio = lower.contains("kraken")
                || matches!(
                    lower.as_str(),
                    "x53" | "x63" | "x73" | "z53" | "z63" | "z73"
                )
                || lower.contains("corsair")
                || lower.contains("aquacomputer");
            !is_cpu && !is_aio
        })
        .collect()
}

/// Returns the default featured sensor IDs shown in the Dashboard hero row.
pub fn default_featured_sensors() -> Vec<String> {
    vec!["cpu".into(), "gpu".into(), "aio".into()]
}

/// Returns `true` if `id` is currently included in `featured`.
pub fn is_featured(featured: &[String], id: &str) -> bool {
    featured.iter().any(|s| s == id)
}

/// Appends `id` to `featured` if not already present.
pub fn promote_sensor(featured: &mut Vec<String>, id: &str) {
    if !is_featured(featured, id) {
        featured.push(id.to_string());
    }
}

/// Removes `id` from `featured` if present.
pub fn demote_sensor(featured: &mut Vec<String>, id: &str) {
    featured.retain(|s| s != id);
}

/// Returns `false` for built-in (`default`, `performance`, `silent`) or active default profiles.
pub fn is_deletable_profile(name: &str, default_profile: &str, profile_count: usize) -> bool {
    let lower = name.to_ascii_lowercase();
    profile_count > 1
        && name != default_profile
        && !matches!(lower.as_str(), "default" | "performance" | "silent")
}

/// Resolves the available RGB `(channels, modes)` for a device using driver info and configured profiles.
pub fn resolve_device_rgb(
    lighting_info: &HashMap<String, DeviceLightingInfo>,
    config: &Config,
    dev: &str,
) -> (Vec<String>, Vec<String>) {
    let mut channels: Vec<String> = lighting_info
        .get(dev)
        .filter(|info| !info.channels.is_empty())
        .map(|info| info.channels.clone())
        .unwrap_or_else(|| {
            let mut dev_channels: Vec<String> =
                RGB_CHANNELS.iter().map(|s| (*s).to_string()).collect();
            let dev_lower = dev.to_ascii_lowercase();
            for p in config.profiles.values() {
                for l in &p.lighting {
                    let rule_lower = l.match_device.to_ascii_lowercase();
                    let matches = l.match_device.is_empty()
                        || dev_lower.contains(&rule_lower)
                        || rule_lower.contains(&dev_lower);
                    if matches && !dev_channels.contains(&l.channel) {
                        dev_channels.push(l.channel.clone());
                    }
                }
            }
            dev_channels
        });

    if let Some(pos) = channels.iter().position(|c| c == "sync") {
        let sync_ch = channels.remove(pos);
        channels.insert(0, sync_ch);
    }

    let modes: Vec<String> = lighting_info
        .get(dev)
        .filter(|info| !info.modes.is_empty())
        .map(|info| info.modes.clone())
        .unwrap_or_else(|| RGB_MODES.iter().map(|s| (*s).to_string()).collect());

    (channels, modes)
}

/// Captures a [`Profile`] from the current TUI RGB, manual duty, and per-device curve state.
pub fn capture_profile_from_state(
    rgb_per_channel: &HashMap<(String, String), ChannelRgbState>,
    manual_duty: &HashMap<(String, String), f32>,
    device_curves: &HashMap<String, String>,
    channels: &[ChannelInfo],
    lighting_info: &HashMap<String, DeviceLightingInfo>,
) -> Profile {
    let mut cooling = Vec::new();
    let mut seen_cooling = HashSet::new();

    for ch in channels {
        let key = (ch.device.clone(), ch.channel.clone());
        seen_cooling.insert(key.clone());
        let target = if let Some(curve) = device_curves.get(&ch.device) {
            CoolingTarget::Curve {
                curve: curve.clone(),
                sensor: None,
            }
        } else if let Some(&duty) = manual_duty.get(&key) {
            CoolingTarget::Fixed {
                duty: duty.round().clamp(0.0, 100.0) as u8,
            }
        } else if let Some(duty) = ch.duty {
            CoolingTarget::Fixed {
                duty: duty.round().clamp(0.0, 100.0) as u8,
            }
        } else {
            continue;
        };
        cooling.push(CoolingRule {
            match_device: ch.device.clone(),
            channel: ch.channel.clone(),
            target,
        });
    }

    let mut extra_manual: Vec<_> = manual_duty
        .iter()
        .filter(|(k, _)| !seen_cooling.contains(*k))
        .collect();
    extra_manual.sort_by_key(|(a, _)| *a);
    for ((dev, ch), &duty) in extra_manual {
        let target = if let Some(curve) = device_curves.get(dev) {
            CoolingTarget::Curve {
                curve: curve.clone(),
                sensor: None,
            }
        } else {
            CoolingTarget::Fixed {
                duty: duty.round().clamp(0.0, 100.0) as u8,
            }
        };
        cooling.push(CoolingRule {
            match_device: dev.clone(),
            channel: ch.clone(),
            target,
        });
    }

    let mut rgb_keys: Vec<_> = rgb_per_channel.keys().collect();
    rgb_keys.sort();
    let mut lighting = Vec::with_capacity(rgb_keys.len());
    for key in rgb_keys {
        let (dev, ch) = key;
        let state = &rgb_per_channel[key];
        let modes: Vec<&str> = lighting_info
            .get(dev)
            .filter(|info| !info.modes.is_empty())
            .map(|info| info.modes.iter().map(String::as_str).collect())
            .unwrap_or_else(|| RGB_MODES.to_vec());
        let idx = state.mode.min(modes.len().saturating_sub(1));
        let mode = modes.get(idx).copied().unwrap_or("fixed").to_string();
        let colors = if mode == "spectrum-wave" || mode == "off" {
            Vec::new()
        } else {
            vec![state.hex()]
        };
        lighting.push(LightingRule {
            match_device: dev.clone(),
            channel: ch.clone(),
            mode,
            colors,
        });
    }

    Profile {
        description: None,
        lighting,
        cooling,
    }
}

/// Generates a non-colliding profile name from `base` (e.g. `mi_perfil`, `mi_perfil_2`, ...).
pub fn next_available_profile_name(existing: &BTreeMap<String, Profile>, base: &str) -> String {
    let trimmed = base.trim();
    let stem = if trimmed.is_empty() {
        "profile"
    } else {
        trimmed
    };
    if !existing.contains_key(stem) {
        return stem.to_string();
    }
    let mut idx = 2usize;
    loop {
        let candidate = format!("{stem}_{idx}");
        if !existing.contains_key(&candidate) {
            return candidate;
        }
        idx += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nxctl_core::StatusReading;
    use nxctl_core::config::{DaemonConfig, PywalConfig};
    use nxctl_core::i18n::Language;

    fn sensor(dir: &str, name: &str, input: &str, temp_c: f32) -> HwmonSensorInfo {
        HwmonSensorInfo {
            device_dir: dir.into(),
            name: name.into(),
            input_file: input.into(),
            label: None,
            temp_c,
        }
    }

    fn empty_config() -> Config {
        Config {
            default_profile: "default".into(),
            language: Language::Es,
            daemon: DaemonConfig::default(),
            pywal: PywalConfig::default(),
            curves: BTreeMap::new(),
            profiles: BTreeMap::new(),
        }
    }

    #[test]
    fn tui_view_navigation_and_shortcuts() {
        assert_eq!(TuiView::default(), TuiView::Dashboard);
        assert_eq!(
            TuiView::ALL,
            [
                TuiView::Dashboard,
                TuiView::Fans,
                TuiView::Rgb,
                TuiView::Profiles
            ]
        );

        assert_eq!(TuiView::Dashboard.next(), TuiView::Fans);
        assert_eq!(TuiView::Fans.next(), TuiView::Rgb);
        assert_eq!(TuiView::Rgb.next(), TuiView::Profiles);
        assert_eq!(TuiView::Profiles.next(), TuiView::Dashboard);

        assert_eq!(TuiView::Dashboard.prev(), TuiView::Profiles);
        assert_eq!(TuiView::Fans.prev(), TuiView::Dashboard);
        assert_eq!(TuiView::Rgb.prev(), TuiView::Fans);
        assert_eq!(TuiView::Profiles.prev(), TuiView::Rgb);

        assert_eq!(TuiView::from_number(0), None);
        assert_eq!(TuiView::from_number(1), Some(TuiView::Dashboard));
        assert_eq!(TuiView::from_number(2), Some(TuiView::Fans));
        assert_eq!(TuiView::from_number(3), Some(TuiView::Rgb));
        assert_eq!(TuiView::from_number(4), Some(TuiView::Profiles));
        assert_eq!(TuiView::from_number(5), None);

        assert_eq!(DashboardFocus::default(), DashboardFocus::Featured);
        assert_eq!(RgbFocus::default(), RgbFocus::Pywal);
    }

    #[test]
    fn extracts_liquidctl_and_hwmon_channels() {
        let rd = |k: &str, v: f64| StatusReading {
            key: k.into(),
            value: serde_json::json!(v),
            unit: None,
        };
        let kraken = DeviceStatus {
            address: None,
            bus: None,
            description: "NZXT Kraken X53".into(),
            status: vec![
                rd("Liquid temperature", 31.0),
                rd("Pump speed", 2000.0),
                rd("Pump duty", 60.0),
                rd("Fan 1 speed", 900.0),
                rd("Fan 1 duty", 42.0),
            ],
        };
        let ch = extract_channels(std::slice::from_ref(&kraken));
        assert_eq!(ch.len(), 2);
        assert_eq!(ch[0].device, "NZXT Kraken X53");
        assert_eq!(ch[0].channel, "pump");
        assert_eq!(ch[0].label, "Pump");
        assert_eq!(ch[0].rpm, Some(2000.0));
        assert_eq!(ch[0].duty, Some(60.0));
        assert_eq!(ch[1].channel, "fan1");
        assert_eq!(ch[1].rpm, Some(900.0));
        assert_eq!(ch[1].duty, Some(42.0));

        let fans = vec![
            HwmonFanInfo {
                device_dir: "hwmon1".into(),
                name: "nzxt_kraken3".into(),
                channel: "pwm1".into(),
                pwm_file: Some("pwm1".into()),
                fan_file: None,
                label: None,
                rpm: Some(2000),
                duty: Some(80),
                pwm_enable: Some(1),
            },
            HwmonFanInfo {
                device_dir: "hwmon2".into(),
                name: "nct6775".into(),
                channel: "pwm1".into(),
                pwm_file: Some("pwm1".into()),
                fan_file: Some("fan1_input".into()),
                label: Some("Chassis 1".into()),
                rpm: Some(950),
                duty: Some(45),
                pwm_enable: Some(1),
            },
            HwmonFanInfo {
                device_dir: "hwmon3".into(),
                name: " ".into(),
                channel: "pwm2".into(),
                pwm_file: Some("pwm2".into()),
                fan_file: None,
                label: None,
                rpm: None,
                duty: None,
                pwm_enable: None,
            },
            HwmonFanInfo {
                device_dir: "hwmon4".into(),
                name: "asus_ec".into(),
                channel: "fan1".into(),
                pwm_file: None,
                fan_file: Some("fan1_input".into()),
                label: None,
                rpm: Some(800),
                duty: None,
                pwm_enable: None,
            },
        ];

        // When liquidctl devices exist, NZXT kernel hwmon duplicate is skipped.
        let hwmon_with_liq = extract_hwmon_channels(&fans, &[kraken]);
        assert_eq!(hwmon_with_liq.len(), 1);
        assert_eq!(hwmon_with_liq[0].device, "hwmon:nct6775");
        assert_eq!(hwmon_with_liq[0].channel, "pwm1");
        assert_eq!(hwmon_with_liq[0].label, "Chassis 1");
        assert_eq!(hwmon_with_liq[0].rpm, Some(950.0));
        assert_eq!(hwmon_with_liq[0].duty, Some(45.0));

        // Without liquidctl devices, NZXT hwmon channel is retained.
        let hwmon_without_liq = extract_hwmon_channels(&fans, &[]);
        assert_eq!(hwmon_without_liq.len(), 2);
        assert_eq!(hwmon_without_liq[0].device, "hwmon:nzxt_kraken3");
        assert_eq!(hwmon_without_liq[0].label, "pwm1");
    }

    #[test]
    fn groups_channels_and_detects_pumps() {
        let ch = |dev: &str, channel: &str| ChannelInfo {
            device: dev.into(),
            channel: channel.into(),
            label: channel.into(),
            rpm: Some(1200.0),
            duty: Some(50.0),
        };
        let input = vec![
            ch("Kraken X53", "pump"),
            ch("Smart Device V2", "fan1"),
            ch("Kraken X53", "fan"),
            ch("Smart Device V2", "fan2"),
        ];
        let grouped = group_channels_by_device(&input);
        assert_eq!(grouped.len(), 2);
        assert_eq!(grouped[0].0, "Kraken X53");
        assert_eq!(
            grouped[0]
                .1
                .iter()
                .map(|c| c.channel.as_str())
                .collect::<Vec<_>>(),
            vec!["pump", "fan"]
        );
        assert_eq!(grouped[1].0, "Smart Device V2");
        assert_eq!(
            grouped[1]
                .1
                .iter()
                .map(|c| c.channel.as_str())
                .collect::<Vec<_>>(),
            vec!["fan1", "fan2"]
        );

        assert!(is_pump_channel("pump"));
        assert!(is_pump_channel("Pump"));
        assert!(is_pump_channel("aio_pump1"));
        assert!(is_pump_channel("bomba"));
        assert!(!is_pump_channel("fan1"));
        assert!(!is_pump_channel("sync"));
    }

    #[test]
    fn promotes_and_demotes_featured_sensors() {
        let mut featured = default_featured_sensors();
        assert_eq!(featured, vec!["cpu", "gpu", "aio"]);
        assert!(is_featured(&featured, "cpu"));
        assert!(is_featured(&featured, "gpu"));
        assert!(is_featured(&featured, "aio"));

        let nvme = sensor("hwmon4", "nvme", "temp1_input", 42.0);
        let nvme_id = hwmon_sensor_id(&nvme);
        assert_eq!(nvme_id, "hwmon:hwmon4:temp1_input");
        assert!(!is_featured(&featured, &nvme_id));

        promote_sensor(&mut featured, &nvme_id);
        assert!(is_featured(&featured, &nvme_id));
        promote_sensor(&mut featured, &nvme_id);
        assert_eq!(featured.iter().filter(|id| *id == &nvme_id).count(), 1);

        demote_sensor(&mut featured, "cpu");
        demote_sensor(&mut featured, "gpu");
        demote_sensor(&mut featured, "aio");
        assert!(!is_featured(&featured, "cpu"));
        assert_eq!(featured, vec![nvme_id.clone()]);

        demote_sensor(&mut featured, &nvme_id);
        assert!(featured.is_empty());
    }

    #[test]
    fn classifies_and_filters_secondary_hwmon_sensors() {
        assert_eq!(classify_hwmon_sensor("spd5118"), SensorCategory::Ram);
        assert_eq!(classify_hwmon_sensor("jc42"), SensorCategory::Ram);
        assert_eq!(classify_hwmon_sensor("ddr5"), SensorCategory::Ram);
        assert_eq!(classify_hwmon_sensor("nvme"), SensorCategory::Ssd);
        assert_eq!(classify_hwmon_sensor("drivetemp"), SensorCategory::Hdd);
        assert_eq!(classify_hwmon_sensor("amdgpu"), SensorCategory::IGpu);
        assert_eq!(classify_hwmon_sensor("mt7921_phy0"), SensorCategory::Wifi);
        assert_eq!(classify_hwmon_sensor("r8169_0_c00:00"), SensorCategory::Net);
        assert_eq!(classify_hwmon_sensor("prom21_xhci"), SensorCategory::Sys);

        let sensors = [
            sensor("hwmon0", "k10temp", "temp1_input", 45.0),
            sensor("hwmon1", "x53", "temp1_input", 29.5),
            sensor("hwmon2", "kraken3", "temp1_input", 30.0),
            sensor("hwmon3", "nvme", "temp1_input", 42.0),
            sensor("hwmon4", "spd5118", "temp1_input", 35.0),
        ];
        let secondary = secondary_hwmon_sensors(&sensors);
        assert_eq!(secondary.len(), 2);
        assert_eq!(secondary[0].name, "nvme");
        assert_eq!(secondary[1].name, "spd5118");
    }

    #[test]
    fn rgb_presets_and_channel_rgb_state_work() {
        assert_eq!(RGB_PRESETS.len(), 8);
        let expected_hexes = [
            "00f0ff", "7400b8", "d946ef", "10b981", "f59e0b", "ef4444", "f1f5f9", "3b82f6",
        ];
        for hex in expected_hexes {
            assert!(
                RGB_PRESETS
                    .iter()
                    .any(|(_, preset_hex, _)| *preset_hex == hex),
                "missing preset hex {hex}"
            );
        }

        let mut st = ChannelRgbState::default();
        assert_eq!(st.mode, 0);
        assert_eq!(st.color_idx, 0);
        assert_eq!(st.custom_rgb, None);
        assert_eq!(st.rgb(), (0x00, 0xf0, 0xff));
        assert_eq!(st.hex(), "00f0ff");

        st.prev_preset();
        assert_eq!(st.color_idx, RGB_PRESETS.len() - 1);
        assert_eq!(st.hex(), "3b82f6");

        st.next_preset();
        assert_eq!(st.color_idx, 0);
        assert_eq!(st.hex(), "00f0ff");

        st.set_rgb((0x12, 0xab, 0x05));
        assert_eq!(st.custom_rgb, Some((0x12, 0xab, 0x05)));
        assert_eq!(st.rgb(), (0x12, 0xab, 0x05));
        assert_eq!(st.hex(), "12ab05");

        st.next_preset();
        assert_eq!(st.custom_rgb, None);
        assert_eq!(st.color_idx, 1);
        assert_eq!(st.hex(), "7400b8");
    }

    #[test]
    fn resolves_device_rgb_from_info_and_config_fallback() {
        let mut config = empty_config();
        config.profiles.insert(
            "custom".into(),
            Profile {
                description: None,
                lighting: vec![
                    LightingRule {
                        match_device: "Smart Device".into(),
                        channel: "led1".into(),
                        mode: "fixed".into(),
                        colors: vec!["00f0ff".into()],
                    },
                    LightingRule {
                        match_device: "Other".into(),
                        channel: "strip".into(),
                        mode: "fixed".into(),
                        colors: vec!["ffffff".into()],
                    },
                ],
                cooling: vec![],
            },
        );

        let mut lighting_info = HashMap::new();
        lighting_info.insert(
            "Kraken X53".into(),
            DeviceLightingInfo {
                description: "Kraken X53".into(),
                channels: vec!["ring".into(), "logo".into(), "sync".into()],
                modes: vec!["fixed".into(), "tai-chi".into()],
            },
        );

        let (kraken_ch, kraken_modes) = resolve_device_rgb(&lighting_info, &config, "Kraken X53");
        assert_eq!(kraken_ch, vec!["sync", "ring", "logo"]);
        assert_eq!(kraken_modes, vec!["fixed", "tai-chi"]);

        let (smart_ch, smart_modes) =
            resolve_device_rgb(&lighting_info, &config, "NZXT Smart Device V2");
        assert_eq!(smart_ch, vec!["sync", "led", "ring", "logo", "led1"]);
        assert_eq!(
            smart_modes,
            RGB_MODES
                .iter()
                .map(|s| (*s).to_string())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn captures_profile_from_state() {
        let mut rgb_per_channel = HashMap::new();
        let mut ring_state = ChannelRgbState::default();
        ring_state.set_rgb((0x00, 0xe5, 0xff));
        rgb_per_channel.insert(("Kraken X53".into(), "ring".into()), ring_state);
        rgb_per_channel.insert(
            ("Kraken X53".into(), "logo".into()),
            ChannelRgbState {
                mode: 3, // spectrum-wave -> empty colors
                color_idx: 2,
                custom_rgb: None,
            },
        );

        let mut manual_duty = HashMap::new();
        manual_duty.insert(("Smart Device V2".into(), "fan1".into()), 64.6);
        manual_duty.insert(("hwmon:nct6775".into(), "pwm2".into()), 50.0);

        let mut device_curves = HashMap::new();
        device_curves.insert("Kraken X53".into(), "silent".into());

        let channels = vec![
            ChannelInfo {
                device: "Kraken X53".into(),
                channel: "pump".into(),
                label: "pump".into(),
                rpm: Some(2200.0),
                duty: Some(80.0),
            },
            ChannelInfo {
                device: "Smart Device V2".into(),
                channel: "fan1".into(),
                label: "fan1".into(),
                rpm: Some(1100.0),
                duty: Some(40.0),
            },
            ChannelInfo {
                device: "Smart Device V2".into(),
                channel: "fan2".into(),
                label: "fan2".into(),
                rpm: Some(900.0),
                duty: Some(35.0),
            },
        ];

        let lighting_info = HashMap::new();
        let profile = capture_profile_from_state(
            &rgb_per_channel,
            &manual_duty,
            &device_curves,
            &channels,
            &lighting_info,
        );

        assert_eq!(profile.cooling.len(), 4);
        assert_eq!(
            profile.cooling[0],
            CoolingRule {
                match_device: "Kraken X53".into(),
                channel: "pump".into(),
                target: CoolingTarget::Curve {
                    curve: "silent".into(),
                    sensor: None,
                },
            }
        );
        assert_eq!(
            profile.cooling[1],
            CoolingRule {
                match_device: "Smart Device V2".into(),
                channel: "fan1".into(),
                target: CoolingTarget::Fixed { duty: 65 },
            }
        );
        assert_eq!(
            profile.cooling[2],
            CoolingRule {
                match_device: "Smart Device V2".into(),
                channel: "fan2".into(),
                target: CoolingTarget::Fixed { duty: 35 },
            }
        );
        assert_eq!(
            profile.cooling[3],
            CoolingRule {
                match_device: "hwmon:nct6775".into(),
                channel: "pwm2".into(),
                target: CoolingTarget::Fixed { duty: 50 },
            }
        );

        assert_eq!(profile.lighting.len(), 2);
        assert_eq!(
            profile.lighting[0],
            LightingRule {
                match_device: "Kraken X53".into(),
                channel: "logo".into(),
                mode: "spectrum-wave".into(),
                colors: vec![],
            }
        );
        assert_eq!(
            profile.lighting[1],
            LightingRule {
                match_device: "Kraken X53".into(),
                channel: "ring".into(),
                mode: "fixed".into(),
                colors: vec!["00e5ff".into()],
            }
        );
    }

    #[test]
    fn checks_deletable_and_next_available_profile_names() {
        assert!(!is_deletable_profile("default", "default", 4));
        assert!(!is_deletable_profile("performance", "default", 4));
        assert!(!is_deletable_profile("silent", "default", 4));
        assert!(!is_deletable_profile("Silent", "default", 4));
        assert!(!is_deletable_profile("custom", "custom", 4));
        assert!(!is_deletable_profile("custom", "default", 1));
        assert!(is_deletable_profile("custom", "default", 4));

        let mut profiles = BTreeMap::new();
        let empty = Profile {
            description: None,
            lighting: vec![],
            cooling: vec![],
        };
        assert_eq!(
            next_available_profile_name(&profiles, "mi_perfil"),
            "mi_perfil"
        );
        profiles.insert("mi_perfil".into(), empty.clone());
        assert_eq!(
            next_available_profile_name(&profiles, "mi_perfil"),
            "mi_perfil_2"
        );
        profiles.insert("mi_perfil_2".into(), empty);
        assert_eq!(
            next_available_profile_name(&profiles, "mi_perfil"),
            "mi_perfil_3"
        );
        assert_eq!(next_available_profile_name(&profiles, "   "), "profile");
    }
}
