//! Background polling thread feeding the GUI with hardware and daemon telemetry.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use nxctl_core::liquidctl::{
    CliLiquidctlBackend, DeviceLightingInfo, DeviceStatus, LiquidctlBackend,
};
use nxctl_core::{
    DaemonStatus, HwmonFanInfo, HwmonSensorInfo, IpcRequest, IpcResponse, NvidiaGpuInfo,
    list_hwmon_fans, list_hwmon_sensors, list_liquidctl_lighting_info, list_nvidia_gpus,
    send_request,
};

/// Number of samples kept per temperature history.
pub const HISTORY_LEN: usize = 60;
/// Interval between automatic polls.
pub const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Returns a stable identifier for a `hwmon` sensor.
pub fn hwmon_sensor_id(s: &HwmonSensorInfo) -> String {
    format!("hwmon:{}:{}", s.device_dir, s.input_file)
}

/// Appends each `hwmon` sensor's temperature to its rolling history, capped at `cap`.
pub fn update_hwmon_history(
    history: &mut HashMap<String, VecDeque<f64>>,
    sensors: &[HwmonSensorInfo],
    cap: usize,
) {
    for s in sensors {
        let entry = history.entry(hwmon_sensor_id(s)).or_default();
        push_capped(entry, f64::from(s.temp_c), cap);
    }
}

/// Appends `value` to `history`, dropping the oldest samples beyond `cap`.
pub fn push_capped(history: &mut VecDeque<f64>, value: f64, cap: usize) {
    history.push_back(value);
    while history.len() > cap {
        history.pop_front();
    }
}

/// Fan/pump channel extracted from liquidctl status readings.
#[derive(Debug, Clone, PartialEq)]
pub struct ChannelInfo {
    pub device: String,
    pub channel: String,
    pub label: String,
    pub rpm: Option<f64>,
    pub duty: Option<f32>,
}

/// Extracts `<name> speed` / `<name> duty` pairs from device status readings.
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

/// Extracts controllable hwmon PWM channels, skipping NZXT kernel hwmon duplicates when liquidctl devices exist.
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

/// Finds the liquid/coolant temperature reading among device statuses.
pub fn liquid_temp(devices: &[DeviceStatus]) -> Option<f32> {
    devices
        .iter()
        .flat_map(|d| d.status.iter())
        .find(|r| {
            let lower = r.key.to_ascii_lowercase();
            (lower == "liquid temperature"
                || lower.contains("liquid")
                || lower.contains("coolant")
                || lower.contains("water"))
                && r.value.as_f64().is_some()
        })
        .and_then(|r| r.value.as_f64())
        .map(|v| v as f32)
}

/// Picks the CPU package temperature from hwmon sensors (`k10temp` / `coretemp` / `zenpower`).
pub fn cpu_temp(sensors: &[HwmonSensorInfo]) -> Option<f32> {
    let cpu: Vec<&HwmonSensorInfo> = sensors
        .iter()
        .filter(|s| s.name == "k10temp" || s.name == "coretemp" || s.name == "zenpower")
        .collect();
    cpu.iter()
        .find(|s| {
            s.label
                .as_deref()
                .is_some_and(|l| l == "Tctl" || l.starts_with("Package"))
        })
        .or_else(|| cpu.first())
        .map(|s| s.temp_c)
}

/// Picks the GPU temperature from NVIDIA `nvidia-smi` or Linux DRM hwmon (`amdgpu`, `radeon`, `xe`, `i915`).
pub fn gpu_temp(nvidia: &[NvidiaGpuInfo], hwmon: &[HwmonSensorInfo]) -> Option<f32> {
    nvidia.first().map(|g| g.temp_c).or_else(|| {
        hwmon
            .iter()
            .find(|s| matches!(s.name.as_str(), "amdgpu" | "radeon" | "xe" | "i915"))
            .map(|s| s.temp_c)
    })
}

/// One consistent poll result, including rolling histories.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub devices: Vec<DeviceStatus>,
    pub channels: Vec<ChannelInfo>,
    pub hwmon: Vec<HwmonSensorInfo>,
    pub nvidia: Vec<NvidiaGpuInfo>,
    pub daemon: Option<DaemonStatus>,
    pub backend_error: Option<String>,
    pub liquid_c: Option<f32>,
    pub cpu_c: Option<f32>,
    pub gpu_c: Option<f32>,
    pub liquid_history: VecDeque<f64>,
    pub cpu_history: VecDeque<f64>,
    pub gpu_history: VecDeque<f64>,
    pub hwmon_history: HashMap<String, VecDeque<f64>>,
    pub lighting_info: HashMap<String, DeviceLightingInfo>,
}

/// Handle to the polling thread.
pub struct Poller {
    trigger: Sender<()>,
    pub rx: Receiver<Arc<Snapshot>>,
}

impl Poller {
    /// Spawns the polling thread; `ctx` is used to wake the UI after each poll.
    pub fn spawn(socket: PathBuf, ctx: Option<eframe::egui::Context>) -> Self {
        let (trigger, trigger_rx) = mpsc::channel::<()>();
        let (tx, rx) = mpsc::channel::<Arc<Snapshot>>();
        thread::spawn(move || {
            let backend = CliLiquidctlBackend::new();
            let mut liquid = VecDeque::new();
            let mut cpu = VecDeque::new();
            let mut gpu = VecDeque::new();
            let mut hwmon_hist: HashMap<String, VecDeque<f64>> = HashMap::new();
            let mut cached_lighting: HashMap<String, DeviceLightingInfo> = HashMap::new();
            let mut prev_device_descs: Vec<String> = Vec::new();
            let mut lighting_loaded = false;
            loop {
                let mut snap = Snapshot::default();
                match backend.status(None) {
                    Ok(d) => snap.devices = d,
                    Err(e) => snap.backend_error = Some(e.to_string()),
                }
                let current_descs: Vec<String> =
                    snap.devices.iter().map(|d| d.description.clone()).collect();
                if !lighting_loaded || current_descs != prev_device_descs {
                    let list = list_liquidctl_lighting_info();
                    cached_lighting = list
                        .into_iter()
                        .map(|info| (info.description.clone(), info))
                        .collect();
                    prev_device_descs = current_descs;
                    lighting_loaded = true;
                }
                snap.lighting_info = cached_lighting.clone();

                snap.hwmon = list_hwmon_sensors().unwrap_or_default();
                let hwmon_fans = list_hwmon_fans().unwrap_or_default();
                snap.nvidia = list_nvidia_gpus().unwrap_or_default();
                snap.daemon = match send_request(&socket, &IpcRequest::Status) {
                    Ok(IpcResponse::Status { daemon }) => Some(daemon),
                    _ => None,
                };
                snap.channels = extract_channels(&snap.devices);
                snap.channels
                    .extend(extract_hwmon_channels(&hwmon_fans, &snap.devices));
                snap.liquid_c = liquid_temp(&snap.devices);
                snap.cpu_c = cpu_temp(&snap.hwmon);
                snap.gpu_c = gpu_temp(&snap.nvidia, &snap.hwmon);
                if let Some(v) = snap.liquid_c {
                    push_capped(&mut liquid, f64::from(v), HISTORY_LEN);
                }
                if let Some(v) = snap.cpu_c {
                    push_capped(&mut cpu, f64::from(v), HISTORY_LEN);
                }
                if let Some(v) = snap.gpu_c {
                    push_capped(&mut gpu, f64::from(v), HISTORY_LEN);
                }
                snap.liquid_history = liquid.clone();
                snap.cpu_history = cpu.clone();
                snap.gpu_history = gpu.clone();
                update_hwmon_history(&mut hwmon_hist, &snap.hwmon, HISTORY_LEN);
                snap.hwmon_history = hwmon_hist.clone();
                if tx.send(Arc::new(snap)).is_err() {
                    break;
                }
                if let Some(c) = &ctx {
                    c.request_repaint();
                }
                match trigger_rx.recv_timeout(POLL_INTERVAL) {
                    Ok(()) | Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
        });
        Self { trigger, rx }
    }

    /// Requests an immediate poll.
    pub fn trigger(&self) {
        let _ = self.trigger.send(());
    }

    /// Returns a cloned sender that triggers immediate background polls.
    pub fn trigger_sender(&self) -> Sender<()> {
        self.trigger.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nxctl_core::StatusReading;

    #[test]
    fn history_is_capped() {
        let mut h = VecDeque::new();
        for i in 0..100 {
            push_capped(&mut h, f64::from(i), HISTORY_LEN);
        }
        assert_eq!(h.len(), 60);
        assert_eq!(h.front().copied(), Some(40.0));
        assert_eq!(h.back().copied(), Some(99.0));
    }

    #[test]
    fn channels_pair_speed_and_duty() {
        let rd = |k: &str, v: f64| StatusReading {
            key: k.into(),
            value: serde_json::json!(v),
            unit: None,
        };
        let dev = DeviceStatus {
            address: None,
            bus: None,
            description: "NZXT Kraken".into(),
            status: vec![
                rd("Liquid temperature", 31.0),
                rd("Pump speed", 2000.0),
                rd("Pump duty", 60.0),
                rd("Fan 1 speed", 900.0),
            ],
        };
        let ch = extract_channels(std::slice::from_ref(&dev));
        assert_eq!(ch.len(), 2);
        assert_eq!(ch[0].channel, "pump");
        assert_eq!(ch[0].rpm, Some(2000.0));
        assert_eq!(ch[0].duty, Some(60.0));
        assert_eq!(ch[1].channel, "fan1");
        assert_eq!(liquid_temp(&[dev]), Some(31.0));
    }

    #[test]
    fn supports_corsair_liquid_amd_gpu_and_hwmon_channels() {
        let corsair = DeviceStatus {
            address: None,
            bus: None,
            description: "Corsair H100i".into(),
            status: vec![StatusReading {
                key: "Coolant temp".into(),
                value: serde_json::json!(29.5),
                unit: Some("°C".into()),
            }],
        };
        assert_eq!(liquid_temp(std::slice::from_ref(&corsair)), Some(29.5));

        let amd = HwmonSensorInfo {
            device_dir: "hwmon3".into(),
            name: "amdgpu".into(),
            input_file: "temp1_input".into(),
            label: Some("edge".into()),
            temp_c: 64.0,
        };
        assert_eq!(gpu_temp(&[], std::slice::from_ref(&amd)), Some(64.0));

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
        ];
        let ch = extract_hwmon_channels(&fans, &[corsair]);
        assert_eq!(ch.len(), 1);
        assert_eq!(ch[0].device, "hwmon:nct6775");
        assert_eq!(ch[0].channel, "pwm1");
        assert_eq!(ch[0].label, "Chassis 1");
        assert_eq!(ch[0].rpm, Some(950.0));
        assert_eq!(ch[0].duty, Some(45.0));
    }

    #[test]
    fn snapshot_default_has_empty_lighting_info() {
        let snap = Snapshot::default();
        assert!(snap.lighting_info.is_empty());
        assert!(snap.hwmon_history.is_empty());
    }

    #[test]
    fn hwmon_history_tracks_and_caps_per_sensor() {
        let mut s1 = HwmonSensorInfo {
            device_dir: "hwmon4".into(),
            name: "nvme".into(),
            input_file: "temp1_input".into(),
            label: Some("Composite".into()),
            temp_c: 40.0,
        };
        let s2 = HwmonSensorInfo {
            device_dir: "hwmon5".into(),
            name: "spd5118".into(),
            input_file: "temp1_input".into(),
            label: None,
            temp_c: 34.5,
        };
        assert_eq!(hwmon_sensor_id(&s1), "hwmon:hwmon4:temp1_input");
        assert_eq!(hwmon_sensor_id(&s2), "hwmon:hwmon5:temp1_input");

        let mut hist: HashMap<String, VecDeque<f64>> = HashMap::new();
        for i in 0..75 {
            s1.temp_c = 30.0 + i as f32;
            update_hwmon_history(&mut hist, &[s1.clone(), s2.clone()], HISTORY_LEN);
        }

        let h1 = &hist["hwmon:hwmon4:temp1_input"];
        assert_eq!(h1.len(), HISTORY_LEN);
        assert_eq!(h1.front().copied(), Some(45.0));
        assert_eq!(h1.back().copied(), Some(104.0));

        let h2 = &hist["hwmon:hwmon5:temp1_input"];
        assert_eq!(h2.len(), HISTORY_LEN);
        assert_eq!(h2.back().copied(), Some(34.5));
    }
}
