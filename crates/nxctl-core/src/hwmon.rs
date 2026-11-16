//! Linux `/sys/class/hwmon` temperature sensor and PWM fan support.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};

const HWMON_BASE: &str = "/sys/class/hwmon";

/// A discovered hwmon temperature sensor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HwmonSensorInfo {
    pub device_dir: String,
    pub name: String,
    pub input_file: String,
    pub label: Option<String>,
    pub temp_c: f32,
}

/// A discovered hwmon fan/PWM channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HwmonFanInfo {
    pub device_dir: String,
    pub name: String,
    pub channel: String,
    pub pwm_file: Option<String>,
    pub fan_file: Option<String>,
    pub label: Option<String>,
    pub rpm: Option<u32>,
    pub duty: Option<u8>,
    pub pwm_enable: Option<u8>,
}

fn read_trimmed(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn read_millideg(path: &Path) -> Option<f32> {
    let v: i64 = read_trimmed(path)?.parse().ok()?;
    Some(v as f32 / 1000.0)
}

fn sorted_dirs(base: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(base)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    dirs.sort();
    Ok(dirs)
}

/// Sorted `(file_name, path)` of `temp*_input` files in a hwmon directory.
fn temp_inputs(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut out: Vec<(String, PathBuf)> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            (name.starts_with("temp") && name.ends_with("_input")).then(|| (name, e.path()))
        })
        .collect();
    out.sort();
    out
}

fn label_for(dir: &Path, input_name: &str) -> Option<String> {
    let stem = input_name.strip_suffix("_input")?;
    read_trimmed(&dir.join(format!("{stem}_label")))
}

fn dir_name(dir: &Path) -> String {
    dir.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Lists all readable hwmon temperature sensors under `base`.
pub fn list_hwmon_sensors_in(base: &Path) -> Result<Vec<HwmonSensorInfo>> {
    let mut sensors = Vec::new();
    for dir in sorted_dirs(base)? {
        let device_dir = dir_name(&dir);
        let name = read_trimmed(&dir.join("name")).unwrap_or_default();
        for (file, path) in temp_inputs(&dir) {
            let Some(temp_c) = read_millideg(&path) else {
                continue;
            };
            sensors.push(HwmonSensorInfo {
                device_dir: device_dir.clone(),
                name: name.clone(),
                label: label_for(&dir, &file),
                input_file: file,
                temp_c,
            });
        }
    }
    Ok(sensors)
}

/// Lists hwmon temperature sensors from `/sys/class/hwmon`.
pub fn list_hwmon_sensors() -> Result<Vec<HwmonSensorInfo>> {
    list_hwmon_sensors_in(Path::new(HWMON_BASE))
}

/// Reads a hwmon temperature (°C) from `base`, matching `hwmon` by directory or chip name.
pub fn read_hwmon_temp_in(
    base: &Path,
    hwmon: &str,
    label: Option<&str>,
) -> std::result::Result<f32, String> {
    let wanted = hwmon.trim().to_lowercase();
    let dirs = sorted_dirs(base).map_err(|e| format!("cannot read {}: {e}", base.display()))?;
    let mut matched_any = false;

    for dir in dirs {
        let name = read_trimmed(&dir.join("name")).unwrap_or_default();
        if dir_name(&dir).to_lowercase() != wanted && name.to_lowercase() != wanted {
            continue;
        }
        matched_any = true;

        for (file, path) in temp_inputs(&dir) {
            if let Some(target) = label {
                let target = target.trim().to_lowercase();
                let stem = file.strip_suffix("_input").unwrap_or(&file).to_lowercase();
                let label_match = label_for(&dir, &file)
                    .map(|l| l.to_lowercase() == target)
                    .unwrap_or(false);
                if !(label_match || file.to_lowercase() == target || stem == target) {
                    continue;
                }
            }
            if let Some(v) = read_millideg(&path) {
                return Ok(v);
            }
        }
    }

    Err(if !matched_any {
        format!("no hwmon device matched '{hwmon}'")
    } else if let Some(l) = label {
        format!("no readable temperature matching label '{l}' on hwmon '{hwmon}'")
    } else {
        format!("no readable temp*_input on hwmon '{hwmon}'")
    })
}

/// Reads a hwmon temperature (°C) from `/sys/class/hwmon`.
pub fn read_hwmon_temp(hwmon: &str, label: Option<&str>) -> std::result::Result<f32, String> {
    read_hwmon_temp_in(Path::new(HWMON_BASE), hwmon, label)
}

/// Converts a duty percentage (`0..=100`) to a Linux hwmon PWM value (`0..=255`).
pub fn pwm_from_duty(duty: u8) -> u8 {
    ((u16::from(duty.min(100)) * 255 + 50) / 100) as u8
}

/// Converts a Linux hwmon PWM value (`0..=255`) to a duty percentage (`0..=100`).
pub fn duty_from_pwm(pwm: u8) -> u8 {
    ((u16::from(pwm) * 100 + 127) / 255) as u8
}

fn parse_channel_index(file_name: &str) -> Option<u32> {
    if let Some(rest) = file_name.strip_prefix("pwm")
        && !rest.is_empty()
        && rest.bytes().all(|b| b.is_ascii_digit())
    {
        return rest.parse().ok();
    }
    if let Some(rest) = file_name
        .strip_prefix("fan")
        .and_then(|s| s.strip_suffix("_input"))
        && !rest.is_empty()
        && rest.bytes().all(|b| b.is_ascii_digit())
    {
        return rest.parse().ok();
    }
    None
}

fn fan_indices(dir: &Path) -> Vec<u32> {
    let mut indices = BTreeSet::new();
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(idx) = parse_channel_index(&name) {
            indices.insert(idx);
        }
    }
    indices.into_iter().collect()
}

fn read_u32(path: &Path) -> Option<u32> {
    read_trimmed(path)?.parse().ok()
}

fn read_u8(path: &Path) -> Option<u8> {
    read_trimmed(path)?.parse().ok()
}

fn fan_label(dir: &Path, idx: u32) -> Option<String> {
    read_trimmed(&dir.join(format!("fan{idx}_label")))
        .or_else(|| read_trimmed(&dir.join(format!("pwm{idx}_label"))))
}

/// Lists all discovered hwmon fan/PWM channels under `base`.
pub fn list_hwmon_fans_in(base: &Path) -> Result<Vec<HwmonFanInfo>> {
    let mut fans = Vec::new();
    for dir in sorted_dirs(base)? {
        let device_dir = dir_name(&dir);
        let name = read_trimmed(&dir.join("name")).unwrap_or_default();
        for idx in fan_indices(&dir) {
            let pwm_name = format!("pwm{idx}");
            let fan_name = format!("fan{idx}_input");
            let pwm_path = dir.join(&pwm_name);
            let fan_path = dir.join(&fan_name);
            let has_pwm = pwm_path.exists();
            let has_fan = fan_path.exists();
            if !has_pwm && !has_fan {
                continue;
            }

            let duty = if has_pwm {
                read_u32(&pwm_path).map(|v| duty_from_pwm(v.min(255) as u8))
            } else {
                None
            };
            let pwm_enable = if has_pwm {
                read_u8(&dir.join(format!("pwm{idx}_enable")))
            } else {
                None
            };
            let rpm = if has_fan { read_u32(&fan_path) } else { None };
            let label = fan_label(&dir, idx);
            let channel = if has_pwm {
                pwm_name.clone()
            } else {
                format!("fan{idx}")
            };

            fans.push(HwmonFanInfo {
                device_dir: device_dir.clone(),
                name: name.clone(),
                channel,
                pwm_file: has_pwm.then_some(pwm_name),
                fan_file: has_fan.then_some(fan_name),
                label,
                rpm,
                duty,
                pwm_enable,
            });
        }
    }
    Ok(fans)
}

/// Lists hwmon fan/PWM channels from `/sys/class/hwmon`.
pub fn list_hwmon_fans() -> Result<Vec<HwmonFanInfo>> {
    list_hwmon_fans_in(Path::new(HWMON_BASE))
}

fn resolve_pwm_channel(dir: &Path, channel: &str) -> Option<String> {
    let wanted = channel.trim().to_ascii_lowercase();
    if wanted.is_empty() {
        return None;
    }
    if let Some(rest) = wanted.strip_prefix("pwm")
        && !rest.is_empty()
        && rest.bytes().all(|b| b.is_ascii_digit())
    {
        return Some(format!("pwm{rest}"));
    }
    if let Some(rest) = wanted.strip_prefix("fan")
        && !rest.is_empty()
        && rest.bytes().all(|b| b.is_ascii_digit())
    {
        return Some(format!("pwm{rest}"));
    }
    if wanted.bytes().all(|b| b.is_ascii_digit()) {
        return Some(format!("pwm{wanted}"));
    }
    for idx in fan_indices(dir) {
        if fan_label(dir, idx).is_some_and(|l| l.eq_ignore_ascii_case(&wanted)) {
            return Some(format!("pwm{idx}"));
        }
    }
    None
}

/// Sets a hwmon PWM duty percentage (`0..=100`) inside `base`.
pub fn set_hwmon_pwm_in(
    base: &Path,
    hwmon: &str,
    channel: &str,
    duty: u8,
    dry_run: bool,
) -> Result<()> {
    if duty > 100 {
        return Err(CoreError::Validation(format!(
            "duty cycle {duty} exceeds 100%"
        )));
    }
    let wanted = hwmon.trim();
    if wanted.is_empty() {
        return Err(CoreError::Validation(
            "hwmon target must not be empty".to_string(),
        ));
    }

    let dirs = sorted_dirs(base)?;
    let mut matched_dir: Option<PathBuf> = None;
    for dir in dirs {
        let name = read_trimmed(&dir.join("name")).unwrap_or_default();
        if dir_name(&dir).eq_ignore_ascii_case(wanted) || name.eq_ignore_ascii_case(wanted) {
            matched_dir = Some(dir);
            break;
        }
    }

    let dir = matched_dir.ok_or_else(|| {
        CoreError::Validation(format!("no hwmon device matched '{wanted}'"))
    })?;
    let pwm_name = resolve_pwm_channel(&dir, channel).ok_or_else(|| {
        CoreError::Validation(format!(
            "hwmon channel '{channel}' not found on '{}'",
            wanted
        ))
    })?;
    let pwm_path = dir.join(&pwm_name);
    if !pwm_path.exists() {
        return Err(CoreError::Validation(format!(
            "hwmon PWM file '{}' does not exist on '{}'",
            pwm_name, wanted
        )));
    }

    let pwm_val = pwm_from_duty(duty);
    if dry_run {
        tracing::info!(
            hwmon = wanted,
            channel = %pwm_name,
            duty,
            pwm = pwm_val,
            "dry-run: hwmon set_pwm"
        );
        return Ok(());
    }

    let enable_path = dir.join(format!("{pwm_name}_enable"));
    if enable_path.exists() && read_trimmed(&enable_path).as_deref() != Some("1") {
        let _ = fs::write(&enable_path, "1\n");
    }

    fs::write(&pwm_path, format!("{pwm_val}\n"))?;
    Ok(())
}

/// Sets a hwmon PWM duty percentage (`0..=100`) under `/sys/class/hwmon`.
pub fn set_hwmon_pwm(hwmon: &str, channel: &str, duty: u8, dry_run: bool) -> Result<()> {
    set_hwmon_pwm_in(Path::new(HWMON_BASE), hwmon, channel, duty, dry_run)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_sys(tag: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "nxctl_hwmon_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let h0 = base.join("hwmon0");
        fs::create_dir_all(&h0).unwrap();
        fs::write(h0.join("name"), "k10temp\n").unwrap();
        fs::write(h0.join("temp1_input"), "45500\n").unwrap();
        fs::write(h0.join("temp1_label"), "Tctl\n").unwrap();
        fs::write(h0.join("temp2_input"), "50000\n").unwrap();
        fs::write(h0.join("temp2_label"), "Tccd1\n").unwrap();
        let h1 = base.join("hwmon4");
        fs::create_dir_all(&h1).unwrap();
        fs::write(h1.join("name"), "amdgpu\n").unwrap();
        fs::write(h1.join("temp1_input"), "61000\n").unwrap();
        fs::write(h1.join("temp1_label"), "edge\n").unwrap();
        base
    }

    #[test]
    fn lists_sensors() {
        let base = fake_sys("list");
        let s = list_hwmon_sensors_in(&base).unwrap();
        assert_eq!(s.len(), 3);
        assert_eq!(s[0].device_dir, "hwmon0");
        assert_eq!(s[0].name, "k10temp");
        assert_eq!(s[0].input_file, "temp1_input");
        assert_eq!(s[0].label.as_deref(), Some("Tctl"));
        assert_eq!(s[0].temp_c, 45.5);
        assert_eq!(s[2].name, "amdgpu");
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn reads_by_name_dir_and_label() {
        let base = fake_sys("read");
        assert_eq!(read_hwmon_temp_in(&base, "K10Temp", None), Ok(45.5));
        assert_eq!(read_hwmon_temp_in(&base, "hwmon4", None), Ok(61.0));
        assert_eq!(
            read_hwmon_temp_in(&base, "k10temp", Some("tccd1")),
            Ok(50.0)
        );
        assert_eq!(
            read_hwmon_temp_in(&base, "k10temp", Some("TEMP2_INPUT")),
            Ok(50.0)
        );
        assert_eq!(read_hwmon_temp_in(&base, "amdgpu", Some("temp1")), Ok(61.0));
        assert!(read_hwmon_temp_in(&base, "amdgpu", Some("junction")).is_err());
        assert!(read_hwmon_temp_in(&base, "coretemp", None).is_err());
        let _ = fs::remove_dir_all(base);
    }

    fn fake_pwm_sys(tag: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "nxctl_hwmon_pwm_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let h2 = base.join("hwmon2");
        fs::create_dir_all(&h2).unwrap();
        fs::write(h2.join("name"), "nct6775\n").unwrap();
        fs::write(h2.join("pwm1"), "128\n").unwrap();
        fs::write(h2.join("pwm1_enable"), "2\n").unwrap();
        fs::write(h2.join("fan1_input"), "1150\n").unwrap();
        fs::write(h2.join("fan1_label"), "Chassis 1\n").unwrap();
        fs::write(h2.join("fan2_input"), "800\n").unwrap();
        base
    }

    #[test]
    fn lists_and_controls_hwmon_pwm_channels() {
        let base = fake_pwm_sys("ctrl");
        let fans = list_hwmon_fans_in(&base).unwrap();
        assert_eq!(fans.len(), 2);
        assert_eq!(fans[0].name, "nct6775");
        assert_eq!(fans[0].channel, "pwm1");
        assert_eq!(fans[0].pwm_file.as_deref(), Some("pwm1"));
        assert_eq!(fans[0].fan_file.as_deref(), Some("fan1_input"));
        assert_eq!(fans[0].label.as_deref(), Some("Chassis 1"));
        assert_eq!(fans[0].rpm, Some(1150));
        assert_eq!(fans[0].duty, Some(50));
        assert_eq!(fans[0].pwm_enable, Some(2));
        assert_eq!(fans[1].channel, "fan2");
        assert_eq!(fans[1].pwm_file, None);
        assert_eq!(fans[1].rpm, Some(800));

        // dry_run does not modify pwm1 or pwm1_enable
        set_hwmon_pwm_in(&base, "nct6775", "pwm1", 100, true).unwrap();
        assert_eq!(fs::read_to_string(base.join("hwmon2/pwm1")).unwrap(), "128\n");
        assert_eq!(
            fs::read_to_string(base.join("hwmon2/pwm1_enable")).unwrap(),
            "2\n"
        );

        // Apply by pwm1, fan1, and label
        set_hwmon_pwm_in(&base, "nct6775", "pwm1", 100, false).unwrap();
        assert_eq!(fs::read_to_string(base.join("hwmon2/pwm1")).unwrap(), "255\n");
        assert_eq!(
            fs::read_to_string(base.join("hwmon2/pwm1_enable")).unwrap(),
            "1\n"
        );

        set_hwmon_pwm_in(&base, "hwmon2", "fan1", 0, false).unwrap();
        assert_eq!(fs::read_to_string(base.join("hwmon2/pwm1")).unwrap(), "0\n");

        set_hwmon_pwm_in(&base, "nct6775", "chassis 1", 60, false).unwrap();
        assert_eq!(
            fs::read_to_string(base.join("hwmon2/pwm1")).unwrap(),
            format!("{}\n", pwm_from_duty(60))
        );

        // Validation errors
        assert!(matches!(
            set_hwmon_pwm_in(&base, "nct6775", "pwm1", 101, false),
            Err(CoreError::Validation(_))
        ));
        assert!(matches!(
            set_hwmon_pwm_in(&base, "missing", "pwm1", 50, false),
            Err(CoreError::Validation(_))
        ));
        assert!(matches!(
            set_hwmon_pwm_in(&base, "nct6775", "pwm2", 50, false),
            Err(CoreError::Validation(_))
        ));

        let _ = fs::remove_dir_all(base);
    }
}
