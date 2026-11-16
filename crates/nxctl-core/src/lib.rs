pub mod config;
pub mod curve;
pub mod engine;
pub mod error;
pub mod hwmon;
pub mod ipc;
pub mod liquidctl;
pub mod nvidia;
pub mod openrgb;
pub mod resume;

pub use config::{
    Config, CoolingRule, CoolingTarget, CurvePoint, DaemonConfig, FanCurve, LightingRule, Profile,
    ProfileBundle, TempSensorRef,
};
pub use curve::{HysteresisController, evaluate_curve};
pub use engine::{ApplyReport, SpeedChange, apply_profile, step_daemon_tick};
pub use error::{CoreError, Result};
pub use hwmon::{
    HwmonFanInfo, HwmonSensorInfo, list_hwmon_fans, list_hwmon_sensors, read_hwmon_temp,
    set_hwmon_pwm,
};
pub use ipc::{DaemonStatus, IpcRequest, IpcResponse, default_socket_path, send_request};
pub use liquidctl::{
    CliLiquidctlBackend, DeviceLightingInfo, DeviceStatus, LiquidctlBackend, MockCall,
    MockLiquidctlBackend, StatusReading, list_liquidctl_lighting_info, parse_lighting_info_json,
};
pub use nvidia::{NvidiaGpuInfo, list_nvidia_gpus, read_nvidia_temp};
pub use resume::{DEFAULT_RESUME_THRESHOLD_SECS, ResumeDetector, detect_resume_gap};
