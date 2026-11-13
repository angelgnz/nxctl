use std::collections::HashMap;

use crate::config::{Config, CoolingTarget, TempSensorRef};
use crate::curve::{HysteresisController, evaluate_curve};
use crate::error::{CoreError, Result};
use crate::liquidctl::{DeviceStatus, LiquidctlBackend};

/// Summary of applying a profile once.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ApplyReport {
    pub lighting_applied: usize,
    pub cooling_applied: usize,
    pub warnings: Vec<String>,
}

/// Speed update emitted by [`step_daemon_tick`] when hysteresis allows a change.
#[derive(Debug, Clone, PartialEq)]
pub struct SpeedChange {
    pub match_device: String,
    pub channel: String,
    pub temp_c: f32,
    pub duty: u8,
}

/// Applies all lighting and cooling rules from `profile_name` using `backend`.
pub fn apply_profile(
    backend: &dyn LiquidctlBackend,
    config: &Config,
    profile_name: &str,
    dry_run: bool,
) -> Result<ApplyReport> {
    let profile = config
        .profiles
        .get(profile_name)
        .ok_or_else(|| CoreError::ProfileNotFound(profile_name.to_string()))?;

    let mut report = ApplyReport::default();

    for rule in &profile.lighting {
        backend.set_color(
            &rule.match_device,
            &rule.channel,
            &rule.mode,
            &rule.colors,
            dry_run,
        )?;
        report.lighting_applied += 1;
    }

    let mut status_cache: HashMap<String, Vec<DeviceStatus>> = HashMap::new();

    for rule in &profile.cooling {
        match &rule.target {
            CoolingTarget::Fixed { duty } => {
                backend.set_speed(&rule.match_device, &rule.channel, *duty, dry_run)?;
                report.cooling_applied += 1;
            }
            CoolingTarget::Curve { curve, sensor } => {
                let fan_curve = config.curves.get(curve).ok_or_else(|| {
                    CoreError::Validation(format!(
                        "curve '{curve}' referenced by '{}:{}' does not exist",
                        rule.match_device, rule.channel
                    ))
                })?;

                let Some(sensor_ref) = sensor.as_ref().or(config.daemon.temp_sensor.as_ref())
                else {
                    report.warnings.push(format!(
                        "no temperature sensor configured for curve '{curve}' on '{}:{}'",
                        rule.match_device, rule.channel
                    ));
                    continue;
                };

                match read_sensor_temp(&mut status_cache, backend, sensor_ref)? {
                    Ok(temp_c) => {
                        let duty = evaluate_curve(fan_curve, temp_c);
                        backend.set_speed(&rule.match_device, &rule.channel, duty, dry_run)?;
                        report.cooling_applied += 1;
                    }
                    Err(reason) => {
                        report.warnings.push(format!(
                            "skipping '{}:{}' (curve '{curve}'): {reason}",
                            rule.match_device, rule.channel
                        ));
                    }
                }
            }
        }
    }

    Ok(report)
}

/// Evaluates curve-based cooling rules for one daemon tick, applying changes gated by `controller`.
pub fn step_daemon_tick(
    backend: &dyn LiquidctlBackend,
    config: &Config,
    profile_name: &str,
    controller: &mut HysteresisController,
    dry_run: bool,
) -> Result<Vec<SpeedChange>> {
    let profile = config
        .profiles
        .get(profile_name)
        .ok_or_else(|| CoreError::ProfileNotFound(profile_name.to_string()))?;

    let mut status_cache: HashMap<String, Vec<DeviceStatus>> = HashMap::new();
    let mut changes = Vec::new();

    for rule in &profile.cooling {
        let CoolingTarget::Curve { curve, sensor } = &rule.target else {
            continue;
        };

        let fan_curve = config.curves.get(curve).ok_or_else(|| {
            CoreError::Validation(format!(
                "curve '{curve}' referenced by '{}:{}' does not exist",
                rule.match_device, rule.channel
            ))
        })?;

        let sensor_ref = sensor
            .as_ref()
            .or(config.daemon.temp_sensor.as_ref())
            .ok_or_else(|| {
                CoreError::Validation(format!(
                    "no temperature sensor configured for curve '{curve}' on '{}:{}'",
                    rule.match_device, rule.channel
                ))
            })?;

        let temp_c = read_sensor_temp(&mut status_cache, backend, sensor_ref)?
            .map_err(CoreError::Validation)?;

        let duty = evaluate_curve(fan_curve, temp_c);
        let channel_key = format!("{}:{}", rule.match_device, rule.channel);

        if controller.should_apply(&channel_key, temp_c, duty) {
            backend.set_speed(&rule.match_device, &rule.channel, duty, dry_run)?;
            changes.push(SpeedChange {
                match_device: rule.match_device.clone(),
                channel: rule.channel.clone(),
                temp_c,
                duty,
            });
        }
    }

    Ok(changes)
}

fn read_sensor_temp(
    cache: &mut HashMap<String, Vec<DeviceStatus>>,
    backend: &dyn LiquidctlBackend,
    sensor_ref: &TempSensorRef,
) -> Result<std::result::Result<f32, String>> {
    let (match_device, key) = match sensor_ref {
        TempSensorRef::Hwmon { hwmon, label } => {
            return Ok(crate::hwmon::read_hwmon_temp(hwmon, label.as_deref()));
        }
        TempSensorRef::Liquidctl { match_device, key } => (match_device, key),
    };
    if !cache.contains_key(match_device) {
        let statuses = backend.status(Some(match_device))?;
        cache.insert(match_device.clone(), statuses);
    }

    let devices = &cache[match_device];
    if devices.is_empty() {
        return Ok(Err(format!("no devices matched sensor '{}'", match_device)));
    }

    for device in devices {
        if let Some(reading) = device.status.iter().find(|r| r.key == *key) {
            return match reading.value.as_f64() {
                Some(val) if val.is_finite() => Ok(Ok(val as f32)),
                _ => Ok(Err(format!(
                    "sensor reading '{}' on '{}' is not a finite number",
                    key, device.description
                ))),
            };
        }
    }

    Ok(Err(format!(
        "sensor key '{}' not found on device(s) matching '{}'",
        key, match_device
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::liquidctl::{MockCall, MockLiquidctlBackend, StatusReading};
    use serde_json::json;

    fn kraken_with_temp(temp_c: f64) -> DeviceStatus {
        DeviceStatus {
            address: Some("/dev/hidraw0".into()),
            bus: Some("hid".into()),
            description: "NZXT Kraken X63".into(),
            status: vec![StatusReading {
                key: "Liquid temperature".into(),
                value: json!(temp_c),
                unit: Some("°C".into()),
            }],
        }
    }

    #[test]
    fn apply_profile_default_applies_lighting_and_fixed_cooling() {
        let cfg = Config::starter_nzxt();
        let mock = MockLiquidctlBackend::new();

        let report = apply_profile(&mock, &cfg, "default", false).expect("apply default");
        assert_eq!(
            report,
            ApplyReport {
                lighting_applied: 2,
                cooling_applied: 4,
                warnings: vec![],
            }
        );

        let calls = mock.calls();
        assert_eq!(calls.len(), 6);
        assert_eq!(
            calls[0],
            MockCall::SetColor {
                match_device: "smart".into(),
                channel: "sync".into(),
                mode: "fixed".into(),
                colors: vec!["7400b8".into()],
                dry_run: false,
            }
        );
        assert_eq!(
            calls[2],
            MockCall::SetSpeed {
                match_device: "kraken".into(),
                channel: "pump".into(),
                duty: 100,
                dry_run: false,
            }
        );
        assert_eq!(
            calls[5],
            MockCall::SetSpeed {
                match_device: "smart".into(),
                channel: "fan3".into(),
                duty: 60,
                dry_run: false,
            }
        );
    }

    #[test]
    fn apply_profile_curve_caches_status_and_warns_when_sensor_missing() {
        let cfg = Config::starter_nzxt();
        let mock = MockLiquidctlBackend::with_statuses(vec![kraken_with_temp(35.0)]);

        let report = apply_profile(&mock, &cfg, "silent", true).expect("apply silent");
        assert_eq!(report.lighting_applied, 2);
        assert_eq!(report.cooling_applied, 4);
        assert!(report.warnings.is_empty());

        let status_calls = mock
            .calls()
            .into_iter()
            .filter(|c| matches!(c, MockCall::Status { .. }))
            .count();
        assert_eq!(status_calls, 1);

        // When no devices match sensor, warnings are populated instead of failing.
        mock.set_statuses(vec![]);
        let warn_report = apply_profile(&mock, &cfg, "silent", true).expect("apply with warning");
        assert_eq!(warn_report.lighting_applied, 2);
        assert_eq!(warn_report.cooling_applied, 0);
        assert_eq!(warn_report.warnings.len(), 4);
    }

    #[test]
    fn apply_profile_returns_profile_not_found() {
        let cfg = Config::starter_nzxt();
        let mock = MockLiquidctlBackend::new();
        assert!(matches!(
            apply_profile(&mock, &cfg, "missing", false),
            Err(CoreError::ProfileNotFound(name)) if name == "missing"
        ));
    }

    #[test]
    fn step_daemon_tick_respects_hysteresis_and_emits_speed_changes() {
        let cfg = Config::starter_nzxt();
        let mock = MockLiquidctlBackend::with_statuses(vec![kraken_with_temp(35.0)]);
        let mut controller =
            HysteresisController::new(cfg.daemon.hysteresis_celsius, cfg.daemon.min_duty_delta);

        // Tick 1 at 35C: quiet curve maps 35C -> 45% on all 4 channels.
        let changes1 =
            step_daemon_tick(&mock, &cfg, "silent", &mut controller, false).expect("tick 1");
        assert_eq!(changes1.len(), 4);
        assert_eq!(changes1[0].duty, 45);
        assert_eq!(changes1[0].temp_c, 35.0);

        // Tick 2 at 36C: within 2.0C hysteresis -> no changes applied.
        mock.set_statuses(vec![kraken_with_temp(36.0)]);
        let changes2 =
            step_daemon_tick(&mock, &cfg, "silent", &mut controller, false).expect("tick 2");
        assert!(changes2.is_empty());

        // Tick 3 at 45C: exceeds hysteresis and duty delta -> updates to 70%.
        mock.set_statuses(vec![kraken_with_temp(45.0)]);
        let changes3 =
            step_daemon_tick(&mock, &cfg, "silent", &mut controller, false).expect("tick 3");
        assert_eq!(changes3.len(), 4);
        assert!(changes3.iter().all(|c| c.duty == 70 && c.temp_c == 45.0));
    }

    #[test]
    fn step_daemon_tick_skips_fixed_rules() {
        let cfg = Config::starter_nzxt();
        let mock = MockLiquidctlBackend::new();
        let mut controller = HysteresisController::new(2.0, 2);

        let changes =
            step_daemon_tick(&mock, &cfg, "default", &mut controller, false).expect("tick default");
        assert!(changes.is_empty());
        assert!(mock.calls().is_empty());
    }

    #[test]
    fn step_daemon_tick_uses_per_rule_sensor_override_and_errors_when_missing() {
        let mut cfg = Config::starter_nzxt();
        cfg.daemon.temp_sensor = None;
        cfg.profiles.get_mut("silent").unwrap().cooling[0].target = CoolingTarget::Curve {
            curve: "quiet".to_string(),
            sensor: Some(crate::config::TempSensorRef::Liquidctl {
                match_device: "kraken".to_string(),
                key: "Liquid temperature".to_string(),
            }),
        };

        let mock = MockLiquidctlBackend::with_statuses(vec![kraken_with_temp(45.0)]);
        let mut controller = HysteresisController::new(2.0, 2);

        // Rule 0 has explicit sensor override, rule 1 has no sensor and daemon.temp_sensor is None.
        let err = step_daemon_tick(&mock, &cfg, "silent", &mut controller, false)
            .expect_err("missing sensor should fail");
        assert!(matches!(err, CoreError::Validation(_)));

        // If we keep only rule 0 (which has the explicit sensor), tick succeeds.
        cfg.profiles.get_mut("silent").unwrap().cooling.truncate(1);
        let mut ctrl2 = HysteresisController::new(2.0, 2);
        let changes =
            step_daemon_tick(&mock, &cfg, "silent", &mut ctrl2, false).expect("tick override");
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].duty, 70);
    }

    #[test]
    fn read_sensor_temp_hwmon_missing_device_returns_inner_err() {
        let mock = MockLiquidctlBackend::with_statuses(vec![]);
        let mut cache = HashMap::new();
        let r = read_sensor_temp(
            &mut cache,
            &mock,
            &TempSensorRef::Hwmon {
                hwmon: "definitely-not-a-real-hwmon".into(),
                label: None,
            },
        )
        .expect("outer ok");
        assert!(r.is_err());
        assert!(mock.calls().is_empty());
    }
}
