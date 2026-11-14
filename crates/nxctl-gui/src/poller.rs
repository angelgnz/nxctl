//! Background polling thread feeding the GUI with hardware and daemon telemetry.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use nxctl_core::liquidctl::{CliLiquidctlBackend, DeviceStatus, LiquidctlBackend};
use nxctl_core::{
    DaemonStatus, HwmonSensorInfo, IpcRequest, IpcResponse, NvidiaGpuInfo, list_hwmon_sensors,
    list_nvidia_gpus, send_request,
};

/// Number of samples kept per temperature history.
pub const HISTORY_LEN: usize = 60;
/// Interval between automatic polls.
pub const POLL_INTERVAL: Duration = Duration::from_secs(2);

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

/// Finds the liquid temperature reading among device statuses.
pub fn liquid_temp(devices: &[DeviceStatus]) -> Option<f32> {
    devices
        .iter()
        .flat_map(|d| d.status.iter())
        .find(|r| r.key.eq_ignore_ascii_case("Liquid temperature"))
        .and_then(|r| r.value.as_f64())
        .map(|v| v as f32)
}

/// Picks the CPU package temperature from hwmon sensors (`k10temp` / `coretemp`).
pub fn cpu_temp(sensors: &[HwmonSensorInfo]) -> Option<f32> {
    let cpu: Vec<&HwmonSensorInfo> = sensors
        .iter()
        .filter(|s| s.name == "k10temp" || s.name == "coretemp")
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
            loop {
                let mut snap = Snapshot::default();
                match backend.status(None) {
                    Ok(d) => snap.devices = d,
                    Err(e) => snap.backend_error = Some(e.to_string()),
                }
                snap.hwmon = list_hwmon_sensors().unwrap_or_default();
                snap.nvidia = list_nvidia_gpus().unwrap_or_default();
                snap.daemon = match send_request(&socket, &IpcRequest::Status) {
                    Ok(IpcResponse::Status { daemon }) => Some(daemon),
                    _ => None,
                };
                snap.channels = extract_channels(&snap.devices);
                snap.liquid_c = liquid_temp(&snap.devices);
                snap.cpu_c = cpu_temp(&snap.hwmon);
                snap.gpu_c = snap.nvidia.first().map(|g| g.temp_c);
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
}
