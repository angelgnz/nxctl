//! NVIDIA GPU temperature sensors via `nvidia-smi`.

use std::io::ErrorKind;
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::error::CoreError;

/// An NVIDIA GPU and its current temperature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NvidiaGpuInfo {
    pub index: u32,
    pub name: String,
    pub temp_c: f32,
}

/// Parses `nvidia-smi --format=csv,noheader,nounits` output (`index, name, temp`).
pub fn parse_nvidia_smi_csv(csv: &str) -> std::result::Result<Vec<NvidiaGpuInfo>, String> {
    let mut gpus = Vec::new();
    for line in csv.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let parts: Vec<&str> = line.split(',').map(str::trim).collect();
        if parts.len() < 3 {
            return Err(format!("invalid nvidia-smi line: '{line}'"));
        }
        let index = parts[0]
            .parse::<u32>()
            .map_err(|_| format!("invalid GPU index in line: '{line}'"))?;
        let temp_c = parts[parts.len() - 1]
            .parse::<f32>()
            .ok()
            .filter(|t| t.is_finite())
            .ok_or_else(|| format!("invalid temperature in line: '{line}'"))?;
        let name = parts[1..parts.len() - 1].join(", ");
        gpus.push(NvidiaGpuInfo {
            index,
            name,
            temp_c,
        });
    }
    Ok(gpus)
}

fn query_nvidia_smi() -> std::io::Result<String> {
    let output = Command::new("nvidia-smi")
        .args([
            "--query-gpu=index,name,temperature.gpu",
            "--format=csv,noheader,nounits",
        ])
        .output()?;
    if !output.status.success() {
        return Err(std::io::Error::other(format!(
            "nvidia-smi failed (exit code: {:?}): {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Lists NVIDIA GPUs; returns an empty list if `nvidia-smi` is not installed.
pub fn list_nvidia_gpus() -> crate::Result<Vec<NvidiaGpuInfo>> {
    match query_nvidia_smi() {
        Ok(csv) => parse_nvidia_smi_csv(&csv)
            .map_err(|e| CoreError::Io(std::io::Error::new(ErrorKind::InvalidData, e))),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(CoreError::Io(e)),
    }
}

/// Extracts the temperature for `gpu` (or the first GPU when `None`) from CSV output.
pub fn read_nvidia_temp_from_csv(csv: &str, gpu: Option<u32>) -> std::result::Result<f32, String> {
    let gpus = parse_nvidia_smi_csv(csv)?;
    match gpu {
        Some(idx) => gpus
            .iter()
            .find(|g| g.index == idx)
            .map(|g| g.temp_c)
            .ok_or_else(|| format!("NVIDIA GPU {idx} not found")),
        None => gpus
            .first()
            .map(|g| g.temp_c)
            .ok_or_else(|| "no NVIDIA GPUs found".to_string()),
    }
}

/// Reads an NVIDIA GPU temperature via `nvidia-smi`.
pub fn read_nvidia_temp(gpu: Option<u32>) -> std::result::Result<f32, String> {
    let csv = query_nvidia_smi().map_err(|e| {
        if e.kind() == ErrorKind::NotFound {
            "nvidia-smi was not found in PATH".to_string()
        } else {
            e.to_string()
        }
    })?;
    read_nvidia_temp_from_csv(&csv, gpu)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_gpu_float() {
        let r = parse_nvidia_smi_csv("0, NVIDIA GeForce RTX 4070, 48.89\n").unwrap();
        assert_eq!(
            r,
            vec![NvidiaGpuInfo {
                index: 0,
                name: "NVIDIA GeForce RTX 4070".into(),
                temp_c: 48.89
            }]
        );
    }

    #[test]
    fn parses_multi_gpu_int_and_blank_lines() {
        let r = parse_nvidia_smi_csv("0, A, 40\n\n  1 ,  B ,  55.5 \n").unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].temp_c, 40.0);
        assert_eq!(r[1].index, 1);
        assert_eq!(r[1].name, "B");
        assert_eq!(r[1].temp_c, 55.5);
    }

    #[test]
    fn rejects_invalid_lines() {
        assert!(parse_nvidia_smi_csv("garbage").is_err());
        assert!(parse_nvidia_smi_csv("x, A, 40").is_err());
        assert!(parse_nvidia_smi_csv("0, A, hot").is_err());
        assert!(parse_nvidia_smi_csv("0, A, NaN").is_err());
    }

    #[test]
    fn empty_csv_is_empty_list() {
        assert!(parse_nvidia_smi_csv("").unwrap().is_empty());
    }

    #[test]
    fn read_temp_from_csv_selects_gpu() {
        let csv = "0, A, 40\n1, B, 61.5\n";
        assert_eq!(read_nvidia_temp_from_csv(csv, None), Ok(40.0));
        assert_eq!(read_nvidia_temp_from_csv(csv, Some(1)), Ok(61.5));
        assert!(read_nvidia_temp_from_csv(csv, Some(2)).is_err());
        assert!(read_nvidia_temp_from_csv("", None).is_err());
        assert!(read_nvidia_temp_from_csv("bad", None).is_err());
    }
}
