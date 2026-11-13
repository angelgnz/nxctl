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
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            poll_interval_secs: DEFAULT_POLL_INTERVAL_SECS,
            hysteresis_celsius: DEFAULT_HYSTERESIS_CELSIUS,
            min_duty_delta: DEFAULT_MIN_DUTY_DELTA,
            temp_sensor: None,
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

    /// Builds the starter NZXT configuration containing `default`, `silent`, and `performance` profiles.
    pub fn starter_nzxt() -> Self {
        let mut curves = BTreeMap::new();
        curves.insert(
            "quiet".to_string(),
            FanCurve {
                points: vec![
                    CurvePoint {
                        temp_c: 25.0,
                        duty: 30,
                    },
                    CurvePoint {
                        temp_c: 35.0,
                        duty: 45,
                    },
                    CurvePoint {
                        temp_c: 45.0,
                        duty: 70,
                    },
                    CurvePoint {
                        temp_c: 55.0,
                        duty: 100,
                    },
                ],
            },
        );
        curves.insert(
            "aggressive".to_string(),
            FanCurve {
                points: vec![
                    CurvePoint {
                        temp_c: 25.0,
                        duty: 50,
                    },
                    CurvePoint {
                        temp_c: 32.0,
                        duty: 70,
                    },
                    CurvePoint {
                        temp_c: 40.0,
                        duty: 90,
                    },
                    CurvePoint {
                        temp_c: 45.0,
                        duty: 100,
                    },
                ],
            },
        );

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
    }

    #[test]
    fn starter_nzxt_is_valid_and_has_expected_profiles() {
        let cfg = Config::starter_nzxt();
        cfg.validate().expect("starter_nzxt should validate");

        assert_eq!(cfg.default_profile, "default");
        assert!(cfg.curves.contains_key("quiet"));
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
}
