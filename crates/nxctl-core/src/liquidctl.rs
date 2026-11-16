use std::io::ErrorKind;
use std::process::Command;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};

/// Device status object deserialized from `liquidctl status --json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceStatus {
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub bus: Option<String>,
    pub description: String,
    #[serde(default)]
    pub status: Vec<StatusReading>,
}

/// Single sensor/channel reading inside a [`DeviceStatus`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatusReading {
    pub key: String,
    pub value: serde_json::Value,
    #[serde(default)]
    pub unit: Option<String>,
}

/// Abstraction over liquidctl execution for querying status and applying lighting/fan changes.
pub trait LiquidctlBackend: Send + Sync {
    /// Queries device status, optionally filtered by a `liquidctl --match` substring.
    fn status(&self, match_device: Option<&str>) -> Result<Vec<DeviceStatus>>;

    /// Sets lighting color/mode for a channel on devices matching `match_device`.
    fn set_color(
        &self,
        match_device: &str,
        channel: &str,
        mode: &str,
        colors: &[String],
        dry_run: bool,
    ) -> Result<()>;

    /// Sets fixed fan/pump duty percentage (`0..=100`) on devices matching `match_device`.
    fn set_speed(&self, match_device: &str, channel: &str, duty: u8, dry_run: bool) -> Result<()>;

    /// Initializes devices (all devices when `match_device` is `None`).
    fn initialize(&self, match_device: Option<&str>, dry_run: bool) -> Result<()>;
}

/// Backend that invokes the `liquidctl` CLI binary.
#[derive(Debug, Clone)]
pub struct CliLiquidctlBackend {
    binary: String,
}

impl Default for CliLiquidctlBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl CliLiquidctlBackend {
    /// Creates a backend invoking `liquidctl` from `PATH`.
    pub fn new() -> Self {
        Self {
            binary: "liquidctl".to_string(),
        }
    }

    /// Creates a backend invoking a custom executable path or binary name.
    pub fn with_binary(binary: impl Into<String>) -> Self {
        Self {
            binary: binary.into(),
        }
    }

    fn execute(&self, args: Vec<String>) -> Result<Vec<u8>> {
        let output = Command::new(&self.binary)
            .args(&args)
            .output()
            .map_err(|err| {
                if err.kind() == ErrorKind::NotFound {
                    CoreError::LiquidctlNotFound
                } else {
                    CoreError::Io(err)
                }
            })?;

        if !output.status.success() {
            return Err(CoreError::LiquidctlFailed {
                args,
                code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }

        Ok(output.stdout)
    }
}

impl LiquidctlBackend for CliLiquidctlBackend {
    fn status(&self, match_device: Option<&str>) -> Result<Vec<DeviceStatus>> {
        let mut args = Vec::new();
        if let Some(pattern) = match_device {
            args.push("--match".to_string());
            args.push(pattern.to_string());
        }
        args.push("status".to_string());
        args.push("--json".to_string());

        let stdout = self.execute(args)?;
        let statuses: Vec<DeviceStatus> = serde_json::from_slice(&stdout)?;
        Ok(statuses)
    }

    fn set_color(
        &self,
        match_device: &str,
        channel: &str,
        mode: &str,
        colors: &[String],
        dry_run: bool,
    ) -> Result<()> {
        if let Some(openrgb_dev) = crate::openrgb::parse_openrgb_target(match_device) {
            return crate::openrgb::set_openrgb_color(openrgb_dev, channel, mode, colors, dry_run);
        }

        let mut args = vec![
            "--match".to_string(),
            match_device.to_string(),
            "set".to_string(),
            channel.to_string(),
            "color".to_string(),
            mode.to_string(),
        ];
        args.extend(colors.iter().cloned());

        if dry_run {
            tracing::info!(binary = %self.binary, ?args, "dry-run: liquidctl set_color");
            return Ok(());
        }

        self.execute(args)?;
        Ok(())
    }

    fn set_speed(&self, match_device: &str, channel: &str, duty: u8, dry_run: bool) -> Result<()> {
        if duty > 100 {
            return Err(CoreError::Validation(format!(
                "duty cycle {duty} exceeds 100%"
            )));
        }

        if let Some(hwmon_target) = match_device
            .strip_prefix("hwmon:")
            .or_else(|| match_device.strip_prefix("HWMON:"))
        {
            return crate::hwmon::set_hwmon_pwm(hwmon_target, channel, duty, dry_run);
        }
        if let Some(gpu) = crate::nvidia::parse_nvidia_target(match_device) {
            return crate::nvidia::set_nvidia_fan_speed(gpu, channel, duty, dry_run);
        }

        let args = vec![
            "--match".to_string(),
            match_device.to_string(),
            "set".to_string(),
            channel.to_string(),
            "speed".to_string(),
            duty.to_string(),
        ];

        if dry_run {
            tracing::info!(binary = %self.binary, ?args, "dry-run: liquidctl set_speed");
            return Ok(());
        }

        self.execute(args)?;
        Ok(())
    }

    fn initialize(&self, match_device: Option<&str>, dry_run: bool) -> Result<()> {
        let mut args = Vec::new();
        if let Some(pattern) = match_device {
            args.push("--match".to_string());
            args.push(pattern.to_string());
        }
        args.push("initialize".to_string());

        if dry_run {
            tracing::info!(binary = %self.binary, ?args, "dry-run: liquidctl initialize");
            return Ok(());
        }

        self.execute(args)?;
        Ok(())
    }
}

/// Recorded invocation on [`MockLiquidctlBackend`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MockCall {
    Status {
        match_device: Option<String>,
    },
    SetColor {
        match_device: String,
        channel: String,
        mode: String,
        colors: Vec<String>,
        dry_run: bool,
    },
    SetSpeed {
        match_device: String,
        channel: String,
        duty: u8,
        dry_run: bool,
    },
    Initialize {
        match_device: Option<String>,
        dry_run: bool,
    },
}

/// In-memory mock implementation of [`LiquidctlBackend`] for unit and integration tests.
#[derive(Debug, Default)]
pub struct MockLiquidctlBackend {
    statuses: Mutex<Vec<DeviceStatus>>,
    calls: Mutex<Vec<MockCall>>,
}

impl MockLiquidctlBackend {
    /// Creates an empty mock backend with no devices configured.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a mock backend pre-populated with `statuses`.
    pub fn with_statuses(statuses: Vec<DeviceStatus>) -> Self {
        Self {
            statuses: Mutex::new(statuses),
            calls: Mutex::new(Vec::new()),
        }
    }

    /// Replaces the configured [`DeviceStatus`] list returned by `status()`.
    pub fn set_statuses(&self, statuses: Vec<DeviceStatus>) {
        *self.statuses.lock().expect("mock statuses mutex poisoned") = statuses;
    }

    /// Returns a snapshot of all calls recorded so far.
    pub fn calls(&self) -> Vec<MockCall> {
        self.calls
            .lock()
            .expect("mock calls mutex poisoned")
            .clone()
    }

    /// Clears all recorded calls.
    pub fn clear_calls(&self) {
        self.calls
            .lock()
            .expect("mock calls mutex poisoned")
            .clear();
    }
}

impl LiquidctlBackend for MockLiquidctlBackend {
    fn status(&self, match_device: Option<&str>) -> Result<Vec<DeviceStatus>> {
        self.calls
            .lock()
            .expect("mock calls mutex poisoned")
            .push(MockCall::Status {
                match_device: match_device.map(str::to_string),
            });

        let statuses = self.statuses.lock().expect("mock statuses mutex poisoned");
        let filtered = match match_device {
            Some(pattern) => {
                let needle = pattern.to_lowercase();
                statuses
                    .iter()
                    .filter(|d| d.description.to_lowercase().contains(&needle))
                    .cloned()
                    .collect()
            }
            None => statuses.clone(),
        };
        Ok(filtered)
    }

    fn set_color(
        &self,
        match_device: &str,
        channel: &str,
        mode: &str,
        colors: &[String],
        dry_run: bool,
    ) -> Result<()> {
        self.calls
            .lock()
            .expect("mock calls mutex poisoned")
            .push(MockCall::SetColor {
                match_device: match_device.to_string(),
                channel: channel.to_string(),
                mode: mode.to_string(),
                colors: colors.to_vec(),
                dry_run,
            });
        Ok(())
    }

    fn set_speed(&self, match_device: &str, channel: &str, duty: u8, dry_run: bool) -> Result<()> {
        if duty > 100 {
            return Err(CoreError::Validation(format!(
                "duty cycle {duty} exceeds 100%"
            )));
        }
        self.calls
            .lock()
            .expect("mock calls mutex poisoned")
            .push(MockCall::SetSpeed {
                match_device: match_device.to_string(),
                channel: channel.to_string(),
                duty,
                dry_run,
            });
        Ok(())
    }

    fn initialize(&self, match_device: Option<&str>, dry_run: bool) -> Result<()> {
        self.calls
            .lock()
            .expect("mock calls mutex poisoned")
            .push(MockCall::Initialize {
                match_device: match_device.map(str::to_string),
                dry_run,
            });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn device_status_deserializes_liquidctl_json() {
        let raw = r#"[
            {
                "bus": "hid",
                "address": "/dev/hidraw3",
                "description": "NZXT Kraken X63",
                "status": [
                    {"key": "Liquid temperature", "value": 34.2, "unit": "°C"},
                    {"key": "Pump speed", "value": 2400, "unit": "rpm"}
                ]
            }
        ]"#;

        let parsed: Vec<DeviceStatus> = serde_json::from_str(raw).expect("parse liquidctl json");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].description, "NZXT Kraken X63");
        assert_eq!(parsed[0].bus.as_deref(), Some("hid"));
        assert_eq!(parsed[0].status.len(), 2);
        assert_eq!(parsed[0].status[0].key, "Liquid temperature");
        assert_eq!(parsed[0].status[0].value, json!(34.2));
        assert_eq!(parsed[0].status[0].unit.as_deref(), Some("°C"));
    }

    #[test]
    fn cli_backend_returns_not_found_and_supports_dry_run() {
        let cli = CliLiquidctlBackend::with_binary("definitely-missing-liquidctl-bin-xyz");
        assert!(matches!(
            cli.status(None),
            Err(CoreError::LiquidctlNotFound)
        ));
        assert!(matches!(
            cli.set_speed("kraken", "pump", 80, false),
            Err(CoreError::LiquidctlNotFound)
        ));

        // dry_run does not spawn the missing binary.
        cli.set_color("smart", "sync", "fixed", &["7400b8".into()], true)
            .expect("dry_run set_color should succeed");
        cli.set_speed("kraken", "pump", 100, true)
            .expect("dry_run set_speed should succeed");
    }

    #[test]
    fn cli_backend_returns_failed_on_nonzero_exit() {
        let cli = CliLiquidctlBackend::with_binary("false");
        let err = cli.status(Some("kraken")).expect_err("false should fail");
        assert!(matches!(
            err,
            CoreError::LiquidctlFailed {
                ref args,
                code: Some(1),
                ..
            } if args == &vec!["--match", "kraken", "status", "--json"]
        ));
    }

    #[test]
    fn mock_backend_filters_statuses_and_records_calls() {
        let mock = MockLiquidctlBackend::with_statuses(vec![
            DeviceStatus {
                address: None,
                bus: Some("hid".into()),
                description: "NZXT Kraken X63".into(),
                status: vec![StatusReading {
                    key: "Liquid temperature".into(),
                    value: json!(33.5),
                    unit: Some("°C".into()),
                }],
            },
            DeviceStatus {
                address: None,
                bus: Some("hid".into()),
                description: "NZXT Smart Device V2".into(),
                status: vec![],
            },
        ]);

        let kraken = mock.status(Some("kraken")).expect("status");
        assert_eq!(kraken.len(), 1);
        assert_eq!(kraken[0].description, "NZXT Kraken X63");

        mock.set_color("smart", "sync", "fixed", &["7400b8".into()], false)
            .expect("set_color");
        mock.set_speed("kraken", "pump", 100, true)
            .expect("set_speed");
        mock.initialize(Some("kraken"), false).expect("initialize");

        let calls = mock.calls();
        assert_eq!(
            calls,
            vec![
                MockCall::Status {
                    match_device: Some("kraken".into())
                },
                MockCall::SetColor {
                    match_device: "smart".into(),
                    channel: "sync".into(),
                    mode: "fixed".into(),
                    colors: vec!["7400b8".into()],
                    dry_run: false,
                },
                MockCall::SetSpeed {
                    match_device: "kraken".into(),
                    channel: "pump".into(),
                    duty: 100,
                    dry_run: true,
                },
                MockCall::Initialize {
                    match_device: Some("kraken".into()),
                    dry_run: false,
                },
            ]
        );
    }

    #[test]
    fn cli_backend_initialize_supports_dry_run() {
        let backend = CliLiquidctlBackend::with_binary("nonexistent-liquidctl-binary");
        backend
            .initialize(None, true)
            .expect("dry-run initialize should not spawn binary");
    }

    #[test]
    fn cli_backend_dispatches_hwmon_nvidia_and_openrgb() {
        let backend = CliLiquidctlBackend::with_binary("nonexistent-liquidctl-binary");

        // hwmon target dispatches to hwmon validation instead of spawning liquidctl
        assert!(matches!(
            backend.set_speed("hwmon:nonexistent-chip-xyz", "pwm1", 50, true),
            Err(CoreError::Validation(_))
        ));

        // nvidia target dispatches to nvidia-settings dry-run
        backend
            .set_speed("nvidia:0", "fan0", 70, true)
            .expect("dry-run nvidia set_speed should succeed");

        // openrgb target dispatches to openrgb dry-run
        backend
            .set_color("openrgb:0", "sync", "static", &["ff0000".into()], true)
            .expect("dry-run openrgb set_color should succeed");
    }
}
