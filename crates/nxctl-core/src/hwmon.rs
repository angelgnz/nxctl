//! Linux `/sys/class/hwmon` temperature sensor support.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::Result;

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
}
