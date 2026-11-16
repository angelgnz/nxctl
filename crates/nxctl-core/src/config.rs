use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};

const DEFAULT_POLL_INTERVAL_SECS: u64 = 2;
const DEFAULT_HYSTERESIS_CELSIUS: f32 = 2.0;
const DEFAULT_MIN_DUTY_DELTA: u8 = 2;

fn default_poll_interval_secs() -> u64 {
    DEFAULT_POLL_INTERVAL_SECS
}

fn default_hysteresis_celsius() -> f32 {
    DEFAULT_HYSTERESIS_CELSIUS
}

fn default_min_duty_delta() -> u8 {
    DEFAULT_MIN_DUTY_DELTA
}

fn default_reapply_on_resume() -> bool {
    true
}

/// Top-level nxctl configuration stored as TOML.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub default_profile: String,
    #[serde(default)]
    pub daemon: DaemonConfig,
    #[serde(default)]
    pub curves: BTreeMap<String, FanCurve>,
    #[serde(default)]
    pub profiles: BTreeMap<String, Profile>,
}

/// Daemon polling and hysteresis settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DaemonConfig {
    #[serde(default = "default_poll_interval_secs")]
    pub poll_interval_secs: u64,
    #[serde(default = "default_hysteresis_celsius")]
    pub hysteresis_celsius: f32,
    #[serde(default = "default_min_duty_delta")]
    pub min_duty_delta: u8,
    #[serde(default)]
    pub temp_sensor: Option<TempSensorRef>,
    #[serde(default = "default_reapply_on_resume")]
    pub reapply_on_resume: bool,
    #[serde(default)]
    pub init_on_resume: bool,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            poll_interval_secs: DEFAULT_POLL_INTERVAL_SECS,
            hysteresis_celsius: DEFAULT_HYSTERESIS_CELSIUS,
            min_duty_delta: DEFAULT_MIN_DUTY_DELTA,
            temp_sensor: None,
            reapply_on_resume: true,
            init_on_resume: false,
        }
    }
}

/// Reference to a temperature sensor: a liquidctl status reading or a Linux hwmon sensor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TempSensorRef {
    Liquidctl {
        match_device: String,
        key: String,
    },
    Hwmon {
        hwmon: String,
        #[serde(default)]
        label: Option<String>,
    },
    Nvidia {
        nvidia: bool,
        #[serde(default)]
        gpu: Option<u32>,
    },
}

/// A single `(temperature_celsius, duty_percent)` point on a fan curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CurvePoint {
    pub temp_c: f32,
    pub duty: u8,
}

/// Piecewise-linear fan curve defined by temperature/duty points.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FanCurve {
    pub points: Vec<CurvePoint>,
}

/// Named lighting and cooling profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub lighting: Vec<LightingRule>,
    #[serde(default)]
    pub cooling: Vec<CoolingRule>,
}

/// Portable bundle containing a named profile and any fan curves it references.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileBundle {
    pub name: String,
    pub profile: Profile,
    #[serde(default)]
    pub curves: BTreeMap<String, FanCurve>,
}

/// Lighting configuration rule targeting a liquidctl device channel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LightingRule {
    pub match_device: String,
    pub channel: String,
    pub mode: String,
    #[serde(default)]
    pub colors: Vec<String>,
}

/// Cooling configuration rule targeting a pump or fan channel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoolingRule {
    pub match_device: String,
    pub channel: String,
    pub target: CoolingTarget,
}

/// Target speed strategy for a cooling channel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CoolingTarget {
    Fixed {
        duty: u8,
    },
    Curve {
        curve: String,
        #[serde(default)]
        sensor: Option<TempSensorRef>,
    },
}

impl Config {
    /// Returns the default configuration file path (`~/.config/nxctl/config.toml`).
    pub fn default_path() -> Result<PathBuf> {
        let base = dirs::config_dir().ok_or_else(|| {
            CoreError::Validation("could not determine user configuration directory".to_string())
        })?;
        Ok(base.join("nxctl").join("config.toml"))
    }

    /// Loads and validates configuration from `path`, or [`Config::default_path`] when `None`.
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let target_path = match path {
            Some(p) => p.to_path_buf(),
            None => Self::default_path()?,
        };
        let contents = fs::read_to_string(&target_path)?;
        let config: Self = toml::from_str(&contents)?;
        config.validate()?;
        Ok(config)
    }

    /// Serializes and saves configuration to `path` (or [`Config::default_path`]), creating parent directories.
    pub fn save(&self, path: Option<&Path>) -> Result<PathBuf> {
        let target_path = match path {
            Some(p) => p.to_path_buf(),
            None => Self::default_path()?,
        };
        if let Some(parent) = target_path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        let serialized = toml::to_string_pretty(self)?;
        fs::write(&target_path, serialized)?;
        Ok(target_path)
    }

    /// Exports a named profile and any fan curves it references to a TOML [`ProfileBundle`] file.
    pub fn export_profile(&self, name: &str, path: &Path) -> Result<()> {
        let profile = self
            .profiles
            .get(name)
            .ok_or_else(|| CoreError::ProfileNotFound(name.to_string()))?;

        let mut curves = BTreeMap::new();
        for rule in &profile.cooling {
            if let CoolingTarget::Curve { curve, .. } = &rule.target
                && let Some(fan_curve) = self.curves.get(curve)
            {
                curves.insert(curve.clone(), fan_curve.clone());
            }
        }

        let bundle = ProfileBundle {
            name: name.to_string(),
            profile: profile.clone(),
            curves,
        };
        let serialized = toml::to_string_pretty(&bundle)?;
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, serialized)?;
        Ok(())
    }

    /// Imports a [`ProfileBundle`] from `path`, inserting missing curves and the profile, then validates.
    pub fn import_profile(&mut self, path: &Path, overwrite: bool) -> Result<String> {
        let contents = fs::read_to_string(path)?;
        let bundle: ProfileBundle = toml::from_str(&contents)?;
        if bundle.name.trim().is_empty() {
            return Err(CoreError::Validation(
                "imported profile name must not be empty".to_string(),
            ));
        }
        if !overwrite && self.profiles.contains_key(&bundle.name) {
            return Err(CoreError::Validation(format!(
                "profile '{}' already exists",
                bundle.name
            )));
        }

        let mut candidate = self.clone();
        for (curve_name, curve) in bundle.curves {
            candidate.curves.entry(curve_name).or_insert(curve);
        }
        candidate
            .profiles
            .insert(bundle.name.clone(), bundle.profile);
        candidate.validate()?;
        *self = candidate;
        Ok(bundle.name)
    }

    /// Validates configuration invariants: existing `default_profile`, valid curves, and valid cooling targets.
    pub fn validate(&self) -> Result<()> {
        if self.default_profile.trim().is_empty() {
            return Err(CoreError::Validation(
                "default_profile must not be empty".to_string(),
            ));
        }
        if !self.profiles.contains_key(&self.default_profile) {
            return Err(CoreError::Validation(format!(
                "default_profile '{}' does not exist in profiles",
                self.default_profile
            )));
        }

        if self.daemon.poll_interval_secs == 0 {
            return Err(CoreError::Validation(
                "daemon.poll_interval_secs must be greater than 0".to_string(),
            ));
        }
        if !self.daemon.hysteresis_celsius.is_finite() || self.daemon.hysteresis_celsius < 0.0 {
            return Err(CoreError::Validation(
                "daemon.hysteresis_celsius must be finite and >= 0".to_string(),
            ));
        }
        if self.daemon.min_duty_delta > 100 {
            return Err(CoreError::Validation(
                "daemon.min_duty_delta must be <= 100".to_string(),
            ));
        }
        if let Some(sensor) = &self.daemon.temp_sensor {
            validate_sensor_ref(sensor, "daemon.temp_sensor")?;
        }

        for (curve_name, curve) in &self.curves {
            if curve.points.is_empty() {
                return Err(CoreError::Validation(format!(
                    "curve '{curve_name}' must contain at least one point"
                )));
            }
            for (idx, point) in curve.points.iter().enumerate() {
                if !point.temp_c.is_finite() {
                    return Err(CoreError::Validation(format!(
                        "curve '{curve_name}' point {idx} has non-finite temperature"
                    )));
                }
                if point.duty > 100 {
                    return Err(CoreError::Validation(format!(
                        "curve '{curve_name}' point {idx} has duty {} > 100",
                        point.duty
                    )));
                }
            }
        }

        for (profile_name, profile) in &self.profiles {
            for (idx, rule) in profile.lighting.iter().enumerate() {
                if rule.match_device.trim().is_empty()
                    || rule.channel.trim().is_empty()
                    || rule.mode.trim().is_empty()
                {
                    return Err(CoreError::Validation(format!(
                        "profile '{profile_name}' lighting rule {idx} has empty match_device, channel, or mode"
                    )));
                }
            }

            for (idx, rule) in profile.cooling.iter().enumerate() {
                if rule.match_device.trim().is_empty() || rule.channel.trim().is_empty() {
                    return Err(CoreError::Validation(format!(
                        "profile '{profile_name}' cooling rule {idx} has empty match_device or channel"
                    )));
                }
                match &rule.target {
                    CoolingTarget::Fixed { duty } => {
                        if *duty > 100 {
                            return Err(CoreError::Validation(format!(
                                "profile '{profile_name}' cooling rule {idx} has duty {duty} > 100"
                            )));
                        }
                    }
                    CoolingTarget::Curve { curve, sensor } => {
                        if !self.curves.contains_key(curve) {
                            return Err(CoreError::Validation(format!(
                                "profile '{profile_name}' cooling rule {idx} references unknown curve '{curve}'"
                            )));
                        }
                        if let Some(sensor_ref) = sensor {
                            validate_sensor_ref(
                                sensor_ref,
                                &format!("profile '{profile_name}' cooling rule {idx} sensor"),
                            )?;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Builds the starter NZXT configuration containing `default`, `silent`, `dynamic`, and `performance` profiles.
    pub fn starter_nzxt() -> Self {
        let curves = default_curves();
        let purple_sync_lighting = vec![
            LightingRule {
                match_device: "smart".to_string(),
                channel: "sync".to_string(),
                mode: "fixed".to_string(),
                colors: vec!["7400b8".to_string()],
            },
            LightingRule {
                match_device: "kraken".to_string(),
                channel: "sync".to_string(),
                mode: "fixed".to_string(),
                colors: vec!["7400b8".to_string()],
            },
        ];

        let mut profiles = BTreeMap::new();
        profiles.insert(
            "default".to_string(),
            Profile {
                description: Some(
                    "Default NZXT setup: purple sync, kraken pump 100%, smart fans 60%".to_string(),
                ),
                lighting: purple_sync_lighting.clone(),
                cooling: vec![
                    CoolingRule {
                        match_device: "kraken".to_string(),
                        channel: "pump".to_string(),
                        target: CoolingTarget::Fixed { duty: 100 },
                    },
                    CoolingRule {
                        match_device: "smart".to_string(),
                        channel: "fan1".to_string(),
                        target: CoolingTarget::Fixed { duty: 60 },
                    },
                    CoolingRule {
                        match_device: "smart".to_string(),
                        channel: "fan2".to_string(),
                        target: CoolingTarget::Fixed { duty: 60 },
                    },
                    CoolingRule {
                        match_device: "smart".to_string(),
                        channel: "fan3".to_string(),
                        target: CoolingTarget::Fixed { duty: 60 },
                    },
                ],
            },
        );

        profiles.insert(
            "silent".to_string(),
            Profile {
                description: Some("Silent cooling profile driven by quiet fan curve".to_string()),
                lighting: purple_sync_lighting.clone(),
                cooling: curve_cooling_rules("quiet"),
            },
        );

        profiles.insert(
            "dynamic".to_string(),
            Profile {
                description: Some(
                    "Dynamic cooling profile: silent up to 35°C liquid, aggressive ramp above"
                        .to_string(),
                ),
                lighting: purple_sync_lighting.clone(),
                cooling: curve_cooling_rules("dynamic"),
            },
        );

        profiles.insert(
            "performance".to_string(),
            Profile {
                description: Some(
                    "Performance cooling profile driven by aggressive fan curve".to_string(),
                ),
                lighting: purple_sync_lighting,
                cooling: curve_cooling_rules("aggressive"),
            },
        );

        Self {
            default_profile: "default".to_string(),
            daemon: DaemonConfig {
                temp_sensor: Some(TempSensorRef::Liquidctl {
                    match_device: "kraken".to_string(),
                    key: "Liquid temperature".to_string(),
                }),
                ..DaemonConfig::default()
            },
            curves,
            profiles,
        }
    }

    /// Builds a configuration automatically from detected liquidctl, hwmon, and NVIDIA hardware.
    pub fn from_detected(
        devices: &[crate::liquidctl::DeviceStatus],
        hwmon_sensors: &[crate::hwmon::HwmonSensorInfo],
        hwmon_fans: &[crate::hwmon::HwmonFanInfo],
        nvidia_gpus: &[crate::nvidia::NvidiaGpuInfo],
    ) -> Self {
        let mut channels: Vec<(String, String)> = Vec::new();
        for dev in devices {
            let match_device = dev.description.trim();
            if match_device.is_empty() {
                continue;
            }
            for reading in &dev.status {
                let key = reading.key.trim();
                let lower = key.to_ascii_lowercase();
                let prefix = lower
                    .strip_suffix(" speed")
                    .or_else(|| lower.strip_suffix(" duty"));
                let Some(prefix) = prefix else {
                    continue;
                };
                let channel = prefix.replace(' ', "");
                if channel.is_empty() {
                    continue;
                }
                let pair = (match_device.to_string(), channel);
                if !channels.contains(&pair) {
                    channels.push(pair);
                }
            }
        }

        let has_nzxt_liquidctl = devices.iter().any(|d| {
            let desc = d.description.to_ascii_lowercase();
            desc.contains("nzxt") || desc.contains("kraken") || desc.contains("smart")
        });

        for fan in hwmon_fans {
            let Some(pwm_file) = &fan.pwm_file else {
                continue;
            };
            if fan.name.trim().is_empty() {
                continue;
            }
            let name_lower = fan.name.to_ascii_lowercase();
            if has_nzxt_liquidctl
                && (name_lower.starts_with("nzxt") || name_lower.contains("kraken"))
            {
                continue;
            }
            let pair = (format!("hwmon:{}", fan.name), pwm_file.clone());
            if !channels.contains(&pair) {
                channels.push(pair);
            }
        }

        if channels.is_empty() && devices.is_empty() {
            return Self::starter_nzxt();
        }

        let lighting: Vec<LightingRule> = devices
            .iter()
            .filter(|d| !d.description.trim().is_empty())
            .map(|d| LightingRule {
                match_device: d.description.clone(),
                channel: "sync".to_string(),
                mode: "fixed".to_string(),
                colors: vec!["7400b8".to_string()],
            })
            .collect();

        let temp_sensor = detect_primary_temp_sensor(devices, hwmon_sensors, nvidia_gpus);

        let default_cooling: Vec<CoolingRule> = channels
            .iter()
            .map(|(match_device, channel)| {
                let duty = if channel.to_ascii_lowercase().contains("pump") {
                    100
                } else {
                    60
                };
                CoolingRule {
                    match_device: match_device.clone(),
                    channel: channel.clone(),
                    target: CoolingTarget::Fixed { duty },
                }
            })
            .collect();

        let curve_rules = |curve_name: &str| -> Vec<CoolingRule> {
            channels
                .iter()
                .map(|(match_device, channel)| CoolingRule {
                    match_device: match_device.clone(),
                    channel: channel.clone(),
                    target: CoolingTarget::Curve {
                        curve: curve_name.to_string(),
                        sensor: None,
                    },
                })
                .collect()
        };

        let mut profiles = BTreeMap::new();
        profiles.insert(
            "default".to_string(),
            Profile {
                description: Some("Auto-detected hardware setup (fixed speeds)".to_string()),
                lighting: lighting.clone(),
                cooling: default_cooling,
            },
        );
        profiles.insert(
            "silent".to_string(),
            Profile {
                description: Some("Silent cooling profile driven by quiet fan curve".to_string()),
                lighting: lighting.clone(),
                cooling: curve_rules("quiet"),
            },
        );
        profiles.insert(
            "dynamic".to_string(),
            Profile {
                description: Some(
                    "Dynamic cooling profile: quiet below thermal knee, aggressive ramp above"
                        .to_string(),
                ),
                lighting: lighting.clone(),
                cooling: curve_rules("dynamic"),
            },
        );
        profiles.insert(
            "performance".to_string(),
            Profile {
                description: Some(
                    "Performance cooling profile driven by aggressive fan curve".to_string(),
                ),
                lighting,
                cooling: curve_rules("aggressive"),
            },
        );

        let curves = match &temp_sensor {
            Some(TempSensorRef::Hwmon { .. } | TempSensorRef::Nvidia { .. }) => cpu_gpu_curves(),
            _ => default_curves(),
        };

        Self {
            default_profile: "default".to_string(),
            daemon: DaemonConfig {
                temp_sensor,
                ..DaemonConfig::default()
            },
            curves,
            profiles,
        }
    }
}

fn build_curves(
    quiet: [(f32, u8); 4],
    dynamic: [(f32, u8); 4],
    aggressive: [(f32, u8); 4],
) -> BTreeMap<String, FanCurve> {
    let to_curve = |pts: [(f32, u8); 4]| FanCurve {
        points: pts
            .into_iter()
            .map(|(temp_c, duty)| CurvePoint { temp_c, duty })
            .collect(),
    };
    BTreeMap::from([
        ("quiet".to_string(), to_curve(quiet)),
        ("dynamic".to_string(), to_curve(dynamic)),
        ("aggressive".to_string(), to_curve(aggressive)),
    ])
}

fn default_curves() -> BTreeMap<String, FanCurve> {
    build_curves(
        [(25.0, 30), (35.0, 45), (45.0, 70), (55.0, 100)],
        [(25.0, 30), (35.0, 35), (40.0, 75), (44.0, 100)],
        [(25.0, 50), (32.0, 70), (40.0, 90), (45.0, 100)],
    )
}

fn cpu_gpu_curves() -> BTreeMap<String, FanCurve> {
    build_curves(
        [(40.0, 30), (55.0, 45), (70.0, 70), (85.0, 100)],
        [(40.0, 30), (62.0, 40), (74.0, 80), (82.0, 100)],
        [(40.0, 50), (55.0, 70), (68.0, 90), (78.0, 100)],
    )
}

fn detect_primary_temp_sensor(
    devices: &[crate::liquidctl::DeviceStatus],
    hwmon_sensors: &[crate::hwmon::HwmonSensorInfo],
    nvidia_gpus: &[crate::nvidia::NvidiaGpuInfo],
) -> Option<TempSensorRef> {
    for dev in devices {
        if dev.description.trim().is_empty() {
            continue;
        }
        if let Some(reading) = dev.status.iter().find(|r| {
            let k = r.key.to_ascii_lowercase();
            (k.contains("liquid") || k.contains("coolant") || k.contains("water"))
                && r.value.as_f64().is_some()
        }) {
            return Some(TempSensorRef::Liquidctl {
                match_device: dev.description.clone(),
                key: reading.key.clone(),
            });
        }
    }
    for dev in devices {
        if dev.description.trim().is_empty() {
            continue;
        }
        if let Some(reading) = dev.status.iter().find(|r| {
            r.key.to_ascii_lowercase().contains("temperature") && r.value.as_f64().is_some()
        }) {
            return Some(TempSensorRef::Liquidctl {
                match_device: dev.description.clone(),
                key: reading.key.clone(),
            });
        }
    }
    if let Some(s) = hwmon_sensors
        .iter()
        .find(|s| matches!(s.name.as_str(), "k10temp" | "coretemp" | "zenpower"))
        .or_else(|| hwmon_sensors.first())
    {
        let hwmon = if s.name.trim().is_empty() {
            s.device_dir.clone()
        } else {
            s.name.clone()
        };
        let label = s.label.clone().filter(|l| !l.trim().is_empty());
        return Some(TempSensorRef::Hwmon { hwmon, label });
    }
    if let Some(gpu) = nvidia_gpus.first() {
        return Some(TempSensorRef::Nvidia {
            nvidia: true,
            gpu: Some(gpu.index),
        });
    }
    None
}

fn curve_cooling_rules(curve_name: &str) -> Vec<CoolingRule> {
    [
        ("kraken", "pump"),
        ("smart", "fan1"),
        ("smart", "fan2"),
        ("smart", "fan3"),
    ]
    .into_iter()
    .map(|(device, channel)| CoolingRule {
        match_device: device.to_string(),
        channel: channel.to_string(),
        target: CoolingTarget::Curve {
            curve: curve_name.to_string(),
            sensor: None,
        },
    })
    .collect()
}

fn validate_sensor_ref(sensor: &TempSensorRef, context: &str) -> Result<()> {
    match sensor {
        TempSensorRef::Liquidctl { match_device, key } => {
            if match_device.trim().is_empty() || key.trim().is_empty() {
                return Err(CoreError::Validation(format!(
                    "{context} must have non-empty match_device and key"
                )));
            }
        }
        TempSensorRef::Hwmon { hwmon, label } => {
            if hwmon.trim().is_empty() {
                return Err(CoreError::Validation(format!(
                    "{context} must have a non-empty hwmon"
                )));
            }
            if label.as_deref().is_some_and(|l| l.trim().is_empty()) {
                return Err(CoreError::Validation(format!(
                    "{context} hwmon label must not be empty"
                )));
            }
        }
        TempSensorRef::Nvidia { nvidia, .. } => {
            if !nvidia {
                return Err(CoreError::Validation(format!(
                    "{context} nvidia must be true"
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file_path(name: &str) -> PathBuf {
        let unique = format!(
            "nxctl_core_test_{}_{}_{}.toml",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        std::env::temp_dir().join("nxctl-core-tests").join(unique)
    }

    #[test]
    fn daemon_config_defaults_match_spec() {
        let daemon = DaemonConfig::default();
        assert_eq!(daemon.poll_interval_secs, 2);
        assert_eq!(daemon.hysteresis_celsius, 2.0);
        assert_eq!(daemon.min_duty_delta, 2);
        assert_eq!(daemon.temp_sensor, None);
        assert!(daemon.reapply_on_resume);
        assert!(!daemon.init_on_resume);
    }

    #[test]
    fn starter_nzxt_is_valid_and_has_expected_profiles() {
        let cfg = Config::starter_nzxt();
        cfg.validate().expect("starter_nzxt should validate");

        assert_eq!(cfg.default_profile, "default");
        assert!(cfg.curves.contains_key("quiet"));
        assert!(cfg.curves.contains_key("dynamic"));
        assert!(cfg.curves.contains_key("aggressive"));

        let default_profile = cfg.profiles.get("default").expect("default profile");
        assert_eq!(default_profile.lighting.len(), 2);
        assert_eq!(default_profile.lighting[0].match_device, "smart");
        assert_eq!(default_profile.lighting[0].channel, "sync");
        assert_eq!(default_profile.lighting[0].mode, "fixed");
        assert_eq!(default_profile.lighting[0].colors, vec!["7400b8"]);
        assert_eq!(default_profile.lighting[1].match_device, "kraken");
        assert_eq!(default_profile.cooling.len(), 4);
        assert_eq!(
            default_profile.cooling[0].target,
            CoolingTarget::Fixed { duty: 100 }
        );
        assert_eq!(
            default_profile.cooling[1].target,
            CoolingTarget::Fixed { duty: 60 }
        );
        assert_eq!(
            default_profile.cooling[2].target,
            CoolingTarget::Fixed { duty: 60 }
        );
        assert_eq!(
            default_profile.cooling[3].target,
            CoolingTarget::Fixed { duty: 60 }
        );

        let silent = cfg.profiles.get("silent").expect("silent profile");
        assert!(silent.cooling.iter().all(|r| matches!(
            &r.target,
            CoolingTarget::Curve { curve, .. } if curve == "quiet"
        )));

        let dynamic = cfg.profiles.get("dynamic").expect("dynamic profile");
        assert!(dynamic.cooling.iter().all(|r| matches!(
            &r.target,
            CoolingTarget::Curve { curve, .. } if curve == "dynamic"
        )));

        let performance = cfg
            .profiles
            .get("performance")
            .expect("performance profile");
        assert!(performance.cooling.iter().all(|r| matches!(
            &r.target,
            CoolingTarget::Curve { curve, .. } if curve == "aggressive"
        )));
    }

    #[test]
    fn toml_roundtrip_save_and_load() {
        let cfg = Config::starter_nzxt();
        let path = temp_file_path("roundtrip");

        let saved_path = cfg.save(Some(&path)).expect("save config");
        assert_eq!(saved_path, path);

        let loaded = Config::load(Some(&path)).expect("load config");
        assert_eq!(loaded, cfg);

        let _ = fs::remove_file(&path);
        if let Some(parent) = path.parent() {
            let _ = fs::remove_dir(parent);
        }
    }

    #[test]
    fn partial_daemon_section_uses_defaults() {
        let raw = r#"
default_profile = "default"

[daemon]
poll_interval_secs = 5

[profiles.default]
lighting = []
cooling = []
"#;
        let cfg: Config = toml::from_str(raw).expect("parse partial daemon");
        cfg.validate().expect("validate");
        assert_eq!(cfg.daemon.poll_interval_secs, 5);
        assert_eq!(cfg.daemon.hysteresis_celsius, 2.0);
        assert_eq!(cfg.daemon.min_duty_delta, 2);
        assert!(cfg.daemon.reapply_on_resume);
        assert!(!cfg.daemon.init_on_resume);
    }

    #[test]
    fn validate_rejects_missing_default_profile() {
        let mut cfg = Config::starter_nzxt();
        cfg.default_profile = "nonexistent".to_string();
        assert!(matches!(cfg.validate(), Err(CoreError::Validation(_))));
    }

    #[test]
    fn validate_rejects_empty_curve_and_nan_and_duty_over_100() {
        let mut cfg = Config::starter_nzxt();
        cfg.curves.get_mut("quiet").unwrap().points.clear();
        assert!(matches!(cfg.validate(), Err(CoreError::Validation(_))));

        let mut cfg = Config::starter_nzxt();
        cfg.curves.get_mut("quiet").unwrap().points[0].temp_c = f32::NAN;
        assert!(matches!(cfg.validate(), Err(CoreError::Validation(_))));

        let mut cfg = Config::starter_nzxt();
        cfg.curves.get_mut("quiet").unwrap().points[0].duty = 101;
        assert!(matches!(cfg.validate(), Err(CoreError::Validation(_))));
    }

    #[test]
    fn validate_rejects_invalid_cooling_targets() {
        let mut cfg = Config::starter_nzxt();
        cfg.profiles.get_mut("default").unwrap().cooling[0].target =
            CoolingTarget::Fixed { duty: 105 };
        assert!(matches!(cfg.validate(), Err(CoreError::Validation(_))));

        let mut cfg = Config::starter_nzxt();
        cfg.profiles.get_mut("silent").unwrap().cooling[0].target = CoolingTarget::Curve {
            curve: "missing_curve".to_string(),
            sensor: None,
        };
        assert!(matches!(cfg.validate(), Err(CoreError::Validation(_))));
    }

    #[test]
    fn default_path_ends_with_nxctl_config_toml() {
        let path = Config::default_path().expect("default path");
        assert!(path.ends_with(Path::new("nxctl").join("config.toml")));
    }

    #[test]
    fn temp_sensor_ref_toml_liquidctl_and_hwmon() {
        let liq: TempSensorRef =
            toml::from_str("match_device = \"kraken\"\nkey = \"Liquid temperature\"").unwrap();
        assert_eq!(
            liq,
            TempSensorRef::Liquidctl {
                match_device: "kraken".into(),
                key: "Liquid temperature".into()
            }
        );
        let hw: TempSensorRef = toml::from_str("hwmon = \"k10temp\"\nlabel = \"Tctl\"").unwrap();
        assert_eq!(
            hw,
            TempSensorRef::Hwmon {
                hwmon: "k10temp".into(),
                label: Some("Tctl".into())
            }
        );
        let hw2: TempSensorRef = toml::from_str("hwmon = \"amdgpu\"").unwrap();
        assert_eq!(
            hw2,
            TempSensorRef::Hwmon {
                hwmon: "amdgpu".into(),
                label: None
            }
        );
        assert!(validate_sensor_ref(&hw, "t").is_ok());
        let bad = TempSensorRef::Hwmon {
            hwmon: " ".into(),
            label: None,
        };
        assert!(validate_sensor_ref(&bad, "t").is_err());
    }

    #[test]
    fn temp_sensor_ref_nvidia_roundtrip_and_validation() {
        let with: TempSensorRef = toml::from_str("nvidia = true\ngpu = 1").unwrap();
        assert_eq!(
            with,
            TempSensorRef::Nvidia {
                nvidia: true,
                gpu: Some(1)
            }
        );
        let without: TempSensorRef = toml::from_str("nvidia = true").unwrap();
        assert_eq!(
            without,
            TempSensorRef::Nvidia {
                nvidia: true,
                gpu: None
            }
        );
        for r in [&with, &without] {
            let back: TempSensorRef = toml::from_str(&toml::to_string(r).unwrap()).unwrap();
            assert_eq!(&back, r);
            assert!(validate_sensor_ref(r, "t").is_ok());
        }
        let bad = TempSensorRef::Nvidia {
            nvidia: false,
            gpu: None,
        };
        assert!(validate_sensor_ref(&bad, "t").is_err());
    }

    #[test]
    fn from_detected_supports_corsair_hwmon_and_fallback() {
        use crate::hwmon::{HwmonFanInfo, HwmonSensorInfo};
        use crate::liquidctl::{DeviceStatus, StatusReading};

        // 1. Corsair/Aquacomputer liquidctl device + coretemp sensor
        let corsair = DeviceStatus {
            address: None,
            bus: Some("hid".into()),
            description: "Corsair H100i Platinum".into(),
            status: vec![
                StatusReading {
                    key: "Coolant temp".into(),
                    value: serde_json::json!(32.5),
                    unit: Some("°C".into()),
                },
                StatusReading {
                    key: "Pump speed".into(),
                    value: serde_json::json!(2300),
                    unit: Some("rpm".into()),
                },
                StatusReading {
                    key: "Fan 1 duty".into(),
                    value: serde_json::json!(45),
                    unit: Some("%".into()),
                },
            ],
        };
        let coretemp = HwmonSensorInfo {
            device_dir: "hwmon1".into(),
            name: "coretemp".into(),
            input_file: "temp1_input".into(),
            label: Some("Package id 0".into()),
            temp_c: 48.0,
        };
        let cfg = Config::from_detected(&[corsair], &[coretemp], &[], &[]);
        cfg.validate().expect("corsair config should validate");
        assert_eq!(
            cfg.daemon.temp_sensor,
            Some(TempSensorRef::Liquidctl {
                match_device: "Corsair H100i Platinum".into(),
                key: "Coolant temp".into(),
            })
        );
        let default_cooling = &cfg.profiles["default"].cooling;
        assert_eq!(default_cooling.len(), 2);
        assert_eq!(default_cooling[0].channel, "pump");
        assert_eq!(
            default_cooling[0].target,
            CoolingTarget::Fixed { duty: 100 }
        );
        assert_eq!(default_cooling[1].channel, "fan1");
        assert_eq!(default_cooling[1].target, CoolingTarget::Fixed { duty: 60 });
        assert!(cfg.profiles.contains_key("dynamic"));
        assert_eq!(cfg.curves["dynamic"].points[1].temp_c, 35.0);

        // 2. Motherboard hwmon PWM fans + k10temp + amdgpu
        let hwmon_fans = vec![HwmonFanInfo {
            device_dir: "hwmon2".into(),
            name: "nct6775".into(),
            channel: "pwm1".into(),
            pwm_file: Some("pwm1".into()),
            fan_file: Some("fan1_input".into()),
            label: Some("Chassis".into()),
            rpm: Some(1100),
            duty: Some(50),
            pwm_enable: Some(2),
        }];
        let sensors = vec![
            HwmonSensorInfo {
                device_dir: "hwmon0".into(),
                name: "k10temp".into(),
                input_file: "temp1_input".into(),
                label: Some("Tctl".into()),
                temp_c: 55.0,
            },
            HwmonSensorInfo {
                device_dir: "hwmon3".into(),
                name: "amdgpu".into(),
                input_file: "temp1_input".into(),
                label: Some("edge".into()),
                temp_c: 62.0,
            },
        ];
        let cfg_mobo = Config::from_detected(&[], &sensors, &hwmon_fans, &[]);
        cfg_mobo.validate().expect("mobo config should validate");
        assert_eq!(
            cfg_mobo.daemon.temp_sensor,
            Some(TempSensorRef::Hwmon {
                hwmon: "k10temp".into(),
                label: Some("Tctl".into()),
            })
        );
        assert_eq!(
            cfg_mobo.profiles["default"].cooling[0].match_device,
            "hwmon:nct6775"
        );
        assert_eq!(cfg_mobo.profiles["default"].cooling[0].channel, "pwm1");
        assert!(cfg_mobo.profiles.contains_key("dynamic"));
        assert_eq!(cfg_mobo.curves["dynamic"].points[1].temp_c, 62.0);

        // 3. Empty inputs fall back to starter_nzxt
        let fallback = Config::from_detected(&[], &[], &[], &[]);
        assert_eq!(fallback, Config::starter_nzxt());
    }

    #[test]
    fn export_and_import_profile_roundtrip_and_overwrite_check() {
        let mut src = Config::starter_nzxt();
        let custom_curve = FanCurve {
            points: vec![
                CurvePoint {
                    temp_c: 30.0,
                    duty: 25,
                },
                CurvePoint {
                    temp_c: 65.0,
                    duty: 90,
                },
            ],
        };
        src.curves
            .insert("custom_curve".into(), custom_curve.clone());
        let custom_profile = Profile {
            description: Some("Custom exported profile".into()),
            lighting: vec![LightingRule {
                match_device: "kraken".into(),
                channel: "sync".into(),
                mode: "fixed".into(),
                colors: vec!["00e5ff".into()],
            }],
            cooling: vec![CoolingRule {
                match_device: "kraken".into(),
                channel: "fan".into(),
                target: CoolingTarget::Curve {
                    curve: "custom_curve".into(),
                    sensor: None,
                },
            }],
        };
        src.profiles.insert("custom".into(), custom_profile.clone());

        let dir = std::env::temp_dir().join(format!("nxctl-profile-test-{}", std::process::id()));
        let bundle_path = dir.join("nested").join("custom.toml");

        // Exporting nonexistent profile errors.
        assert!(matches!(
            src.export_profile("missing", &bundle_path),
            Err(CoreError::ProfileNotFound(_))
        ));

        src.export_profile("custom", &bundle_path)
            .expect("export profile");

        let mut dst = Config::starter_nzxt();
        assert!(!dst.profiles.contains_key("custom"));
        assert!(!dst.curves.contains_key("custom_curve"));

        let imported = dst
            .import_profile(&bundle_path, false)
            .expect("import profile");
        assert_eq!(imported, "custom");
        assert_eq!(dst.profiles.get("custom"), Some(&custom_profile));
        assert_eq!(dst.curves.get("custom_curve"), Some(&custom_curve));
        dst.validate().expect("imported config validates");

        // Re-importing with overwrite = false must fail.
        assert!(matches!(
            dst.import_profile(&bundle_path, false),
            Err(CoreError::Validation(_))
        ));

        // Re-importing with overwrite = true succeeds.
        let reimported = dst
            .import_profile(&bundle_path, true)
            .expect("overwrite import");
        assert_eq!(reimported, "custom");

        let _ = fs::remove_dir_all(&dir);
    }
}
