pub mod config;
pub mod curve;
pub mod engine;
pub mod error;
pub mod hwmon;
pub mod liquidctl;

pub use config::{
    Config, CoolingRule, CoolingTarget, CurvePoint, DaemonConfig, FanCurve, LightingRule, Profile,
    TempSensorRef,
};
pub use curve::{HysteresisController, evaluate_curve};
pub use engine::{ApplyReport, SpeedChange, apply_profile, step_daemon_tick};
pub use error::{CoreError, Result};
pub use hwmon::{HwmonSensorInfo, list_hwmon_sensors, read_hwmon_temp};
pub use liquidctl::{
    CliLiquidctlBackend, DeviceStatus, LiquidctlBackend, MockCall, MockLiquidctlBackend,
    StatusReading,
};
