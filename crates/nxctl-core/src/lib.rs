pub mod config;
pub mod curve;
pub mod engine;
pub mod error;
pub mod hwmon;
pub mod i18n;
pub mod ipc;
pub mod liquidctl;
pub mod nvidia;
pub mod openrgb;
pub mod pywal;
pub mod resume;

pub use config::{
    Config, CoolingRule, CoolingTarget, CurvePoint, DaemonConfig, FanCurve, LightingRule, Profile,
    ProfileBundle, PywalConfig, PywalTarget, TempSensorRef,
};
pub use curve::{HysteresisController, evaluate_curve};
pub use engine::{ApplyReport, SpeedChange, apply_profile, step_daemon_tick};
pub use error::{CoreError, Result};
pub use hwmon::{
    HwmonFanInfo, HwmonSensorInfo, hwmon_device_model, hwmon_device_model_in, list_hwmon_fans,
    list_hwmon_sensors, parse_cpu_model_from_cpuinfo, read_cpu_model_name, read_hwmon_temp,
    set_hwmon_pwm,
};
pub use i18n::Language;
pub use ipc::{DaemonStatus, IpcRequest, IpcResponse, default_socket_path, send_request};
pub use liquidctl::{
    CliLiquidctlBackend, DeviceLightingInfo, DeviceStatus, LiquidctlBackend, MockCall,
    MockLiquidctlBackend, StatusReading, list_liquidctl_lighting_info, parse_lighting_info_json,
};
pub use nvidia::{NvidiaGpuInfo, list_nvidia_gpus, read_nvidia_temp};
pub use pywal::{
    PywalPalette, active_palette_entries, active_palette_entries_vivid, apply_pywal_palette,
    default_colors_json_path, is_extreme_bw, load_pywal_palette, normalize_saturate,
    parse_pywal_colors_json, rgb_to_hex, saturate_rgb, select_primary_pywal_rgb,
};
pub use resume::{DEFAULT_RESUME_THRESHOLD_SECS, ResumeDetector, detect_resume_gap};
