//! Interfaz de terminal (`nxctl tui`) con estilo "Obsidian Telemetry" basada en `ratatui` + `crossterm`.

use std::collections::{HashMap, VecDeque};
use std::io::{self, Stdout};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use crate::i18n;
use crate::tui_state::{
    ChannelInfo, ChannelRgbState, DashboardFocus, RgbFocus, SensorCategory, TuiView,
    capture_profile_from_state, classify_hwmon_sensor, default_featured_sensors, demote_sensor,
    extract_channels, extract_hwmon_channels, group_channels_by_device, hwmon_sensor_id,
    is_deletable_profile, is_featured, is_pump_channel, next_available_profile_name,
    promote_sensor, resolve_device_rgb, secondary_hwmon_sensors,
};
use anyhow::Result;
use nxctl_core::config::Config;
use nxctl_core::engine::apply_profile;
use nxctl_core::liquidctl::{
    CliLiquidctlBackend, DeviceLightingInfo, DeviceStatus, LiquidctlBackend,
};
use nxctl_core::{
    DaemonStatus, HwmonFanInfo, HwmonSensorInfo, IpcRequest, IpcResponse, Language, NvidiaGpuInfo,
    PywalPalette, active_palette_entries, apply_pywal_palette, evaluate_curve, list_hwmon_fans,
    list_hwmon_sensors, list_liquidctl_lighting_info, list_nvidia_gpus, load_pywal_palette,
    send_request,
};
use ratatui::Frame;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Axis, Block, Borders, Chart, Clear, Dataset, GraphType, LineGauge, List, ListItem, Paragraph,
    Sparkline, Wrap,
};

// Paleta "Obsidian Telemetry" (paridad exacta con `nxctl-gui/src/theme.rs`).
pub const BG_CANVAS: Color = Color::Rgb(0x09, 0x0A, 0x0F);
pub const BG_SIDEBAR: Color = Color::Rgb(0x0C, 0x0E, 0x16);
pub const BG_CARD: Color = Color::Rgb(0x12, 0x14, 0x1C);
pub const BG_CONTROL: Color = Color::Rgb(0x18, 0x1B, 0x26);
pub const BORDER_MUTED: Color = Color::Rgb(0x23, 0x29, 0x36);
pub const BORDER_ACTIVE: Color = Color::Rgb(0x2F, 0x37, 0x4A);
pub const CYAN: Color = Color::Rgb(0x00, 0xF0, 0xFF);
pub const MAGENTA: Color = Color::Rgb(0xD9, 0x46, 0xEF);
pub const EMERALD: Color = Color::Rgb(0x10, 0xB9, 0x81);
pub const AMBER: Color = Color::Rgb(0xF5, 0x9E, 0x0B);
pub const CRIMSON: Color = Color::Rgb(0xEF, 0x44, 0x44);
pub const TEXT_PRIMARY: Color = Color::Rgb(0xF1, 0xF5, 0xF9);
pub const TEXT_SECONDARY: Color = Color::Rgb(0x94, 0xA3, 0xB8);
pub const TEXT_MUTED: Color = Color::Rgb(0x64, 0x74, 0x8B);

/// Máximo de muestras de historial de temperatura.
pub const HISTORY_LEN: usize = 60;

const PYWAL_MODES: [&str; 3] = ["fixed", "breathing", "fading"];

/// Datos refrescados periódicamente por el hilo de sondeo.
#[derive(Debug, Clone, Default)]
pub struct TuiSnapshot {
    pub devices: Vec<DeviceStatus>,
    pub hwmon: Vec<HwmonSensorInfo>,
    pub hwmon_fans: Vec<HwmonFanInfo>,
    pub nvidia: Vec<NvidiaGpuInfo>,
    pub daemon: Option<DaemonStatus>,
    pub channels: Vec<ChannelInfo>,
    pub lighting_info: HashMap<String, DeviceLightingInfo>,
    pub pywal: Option<PywalPalette>,
    pub error: Option<String>,
}

#[derive(Default)]
struct LightingCache {
    loaded: bool,
    prev_descs: Vec<String>,
    info: HashMap<String, DeviceLightingInfo>,
}

impl LightingCache {
    fn get_or_refresh(&mut self, devices: &[DeviceStatus]) -> HashMap<String, DeviceLightingInfo> {
        let descs: Vec<String> = devices.iter().map(|d| d.description.clone()).collect();
        if !self.loaded || descs != self.prev_descs {
            self.info = list_liquidctl_lighting_info()
                .into_iter()
                .map(|info| (info.description.clone(), info))
                .collect();
            self.prev_descs = descs;
            self.loaded = true;
        }
        self.info.clone()
    }
}

fn push_sample(h: &mut VecDeque<u64>, v: f32) {
    if h.len() >= HISTORY_LEN {
        h.pop_front();
    }
    h.push_back(v.max(0.0).round() as u64);
}

fn liquid_temp(s: &TuiSnapshot) -> Option<f32> {
    s.devices
        .iter()
        .flat_map(|d| &d.status)
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

fn cpu_temp(s: &TuiSnapshot) -> Option<f32> {
    let cpu: Vec<&HwmonSensorInfo> = s
        .hwmon
        .iter()
        .filter(|h| h.name == "k10temp" || h.name == "coretemp" || h.name == "zenpower")
        .collect();
    cpu.iter()
        .find(|h| {
            h.label
                .as_deref()
                .is_some_and(|l| l == "Tctl" || l.starts_with("Package"))
        })
        .or_else(|| cpu.first())
        .map(|h| h.temp_c)
}

fn gpu_temp(s: &TuiSnapshot) -> Option<f32> {
    s.nvidia.first().map(|g| g.temp_c).or_else(|| {
        s.hwmon
            .iter()
            .find(|h| matches!(h.name.as_str(), "amdgpu" | "radeon" | "xe" | "i915"))
            .map(|h| h.temp_c)
    })
}

/// Estado completo de la interfaz.
#[derive(Debug, Clone)]
pub struct TuiState {
    pub view: TuiView,
    pub config_path: Option<PathBuf>,
    pub lang: Language,
    pub featured_sensors: Vec<String>,
    pub dashboard_focus: DashboardFocus,
    pub selected_featured: usize,
    pub selected_other_sensor: usize,
    pub device_curves: HashMap<String, String>,
    pub manual_duty: HashMap<(String, String), f32>,
    pub selected_fan_channel: usize,
    pub rgb_focus: RgbFocus,
    pub selected_swatch: usize,
    pub rgb_per_channel: HashMap<(String, String), ChannelRgbState>,
    pub selected_rgb_channel: usize,
    pub hwmon_history: HashMap<String, VecDeque<u64>>,
    pub profile_names: Vec<String>,
    pub selected_profile: usize,
    pub curve_names: Vec<String>,
    pub selected_curve: usize,
    pub liquid_history: VecDeque<u64>,
    pub cpu_history: VecDeque<u64>,
    pub gpu_history: VecDeque<u64>,
    pub status_message: Option<String>,
    pub status_ok: bool,
    pub show_help: bool,
    pub dry_run: bool,
    pub socket_path: PathBuf,
    pub config: Config,
    pub snapshot: TuiSnapshot,
    pub local_active_profile: Option<String>,
}

impl TuiState {
    pub fn new(
        config: Config,
        config_path: Option<PathBuf>,
        dry_run: bool,
        socket_path: PathBuf,
    ) -> Self {
        let lang = config.language;
        let profile_names: Vec<String> = config.profiles.keys().cloned().collect();
        let curve_names: Vec<String> = config.curves.keys().cloned().collect();
        let selected_profile = profile_names
            .iter()
            .position(|n| *n == config.default_profile)
            .unwrap_or(0);
        Self {
            view: TuiView::default(),
            config_path,
            lang,
            featured_sensors: default_featured_sensors(),
            dashboard_focus: DashboardFocus::default(),
            selected_featured: 0,
            selected_other_sensor: 0,
            device_curves: HashMap::new(),
            manual_duty: HashMap::new(),
            selected_fan_channel: 0,
            rgb_focus: RgbFocus::default(),
            selected_swatch: 0,
            rgb_per_channel: HashMap::new(),
            selected_rgb_channel: 0,
            hwmon_history: HashMap::new(),
            profile_names,
            selected_profile,
            curve_names,
            selected_curve: 0,
            liquid_history: VecDeque::new(),
            cpu_history: VecDeque::new(),
            gpu_history: VecDeque::new(),
            status_message: None,
            status_ok: true,
            show_help: false,
            dry_run,
            socket_path,
            config,
            snapshot: TuiSnapshot::default(),
            local_active_profile: None,
        }
    }

    /// Guarda una nueva instantánea y añade las muestras al historial.
    pub fn push_snapshot(&mut self, snap: TuiSnapshot) {
        if let Some(t) = liquid_temp(&snap) {
            push_sample(&mut self.liquid_history, t);
        }
        if let Some(t) = cpu_temp(&snap) {
            push_sample(&mut self.cpu_history, t);
        }
        if let Some(t) = gpu_temp(&snap) {
            push_sample(&mut self.gpu_history, t);
        }
        for s in &snap.hwmon {
            let entry = self.hwmon_history.entry(hwmon_sensor_id(s)).or_default();
            push_sample(entry, s.temp_c);
        }
        self.snapshot = snap;
        self.clamp_selections();
    }

    fn refresh_config_lists(&mut self) {
        self.profile_names = self.config.profiles.keys().cloned().collect();
        self.curve_names = self.config.curves.keys().cloned().collect();
        self.clamp_selections();
    }

    fn clamp_selections(&mut self) {
        let feat_len = self.visible_featured_ids().len();
        let other_len = self.other_sensor_ids().len();
        let rgb_len = self.rgb_channel_list().len();
        let swatch_len = self.pywal_entries().len();
        clamp_index(&mut self.selected_profile, self.profile_names.len());
        clamp_index(&mut self.selected_curve, self.curve_names.len());
        clamp_index(&mut self.selected_featured, feat_len);
        clamp_index(&mut self.selected_other_sensor, other_len);
        clamp_index(&mut self.selected_fan_channel, self.snapshot.channels.len());
        clamp_index(&mut self.selected_rgb_channel, rgb_len);
        clamp_index(&mut self.selected_swatch, swatch_len);
    }

    pub fn next_profile(&mut self) {
        wrap_next(&mut self.selected_profile, self.profile_names.len());
    }

    pub fn prev_profile(&mut self) {
        wrap_prev(&mut self.selected_profile, self.profile_names.len());
    }

    pub fn selected_name(&self) -> Option<&str> {
        self.profile_names
            .get(self.selected_profile)
            .map(String::as_str)
    }

    pub fn active_profile(&self) -> &str {
        self.snapshot
            .daemon
            .as_ref()
            .map(|d| d.active_profile.as_str())
            .or(self.local_active_profile.as_deref())
            .unwrap_or(&self.config.default_profile)
    }

    fn visible_featured_ids(&self) -> Vec<String> {
        self.featured_sensors
            .iter()
            .filter(|id| match id.as_str() {
                "cpu" | "gpu" => true,
                "aio" => liquid_temp(&self.snapshot).is_some(),
                other => self
                    .snapshot
                    .hwmon
                    .iter()
                    .any(|h| hwmon_sensor_id(h) == other),
            })
            .cloned()
            .collect()
    }

    fn other_sensor_ids(&self) -> Vec<String> {
        let mut ids = Vec::new();
        if !is_featured(&self.featured_sensors, "cpu") {
            ids.push("cpu".to_string());
        }
        if !is_featured(&self.featured_sensors, "gpu") {
            ids.push("gpu".to_string());
        }
        if liquid_temp(&self.snapshot).is_some() && !is_featured(&self.featured_sensors, "aio") {
            ids.push("aio".to_string());
        }
        for h in secondary_hwmon_sensors(&self.snapshot.hwmon) {
            let id = hwmon_sensor_id(h);
            if !is_featured(&self.featured_sensors, &id) {
                ids.push(id);
            }
        }
        ids
    }

    fn ordered_fan_channels(&self) -> Vec<&ChannelInfo> {
        group_channels_by_device(&self.snapshot.channels)
            .into_iter()
            .flat_map(|(_, list)| list)
            .collect()
    }

    fn active_curve_for_device(&self, dev: &str) -> Option<String> {
        self.device_curves
            .get(dev)
            .filter(|name| self.config.curves.contains_key(name.as_str()))
            .cloned()
            .or_else(|| self.curve_names.first().cloned())
    }

    fn pywal_entries(&self) -> Vec<(String, String, [u8; 3])> {
        self.snapshot
            .pywal
            .as_ref()
            .map(|pal| {
                active_palette_entries(
                    pal,
                    self.config.pywal.filter_extremes,
                    self.config.pywal.palette_offset,
                )
            })
            .unwrap_or_default()
    }

    fn pywal_mode(&self) -> String {
        let trimmed = self.config.pywal.mode.trim();
        if trimmed.is_empty() {
            "fixed".to_string()
        } else {
            trimmed.to_string()
        }
    }

    fn rgb_channel_list(&self) -> Vec<(String, String, Vec<String>)> {
        let mut rows = Vec::new();
        for dev in &self.snapshot.devices {
            let (channels, modes) =
                resolve_device_rgb(&self.snapshot.lighting_info, &self.config, &dev.description);
            for ch in channels {
                rows.push((dev.description.clone(), ch, modes.clone()));
            }
        }
        rows
    }

    fn sync_rgb_rows(&mut self, rgb: (u8, u8, u8), mode_name: &str) {
        for dev in &self.snapshot.devices {
            let (channels, modes) =
                resolve_device_rgb(&self.snapshot.lighting_info, &self.config, &dev.description);
            let mode_idx = modes
                .iter()
                .position(|m| m.eq_ignore_ascii_case(mode_name))
                .unwrap_or(0);
            for ch in channels {
                let entry = self
                    .rgb_per_channel
                    .entry((dev.description.clone(), ch))
                    .or_default();
                entry.set_rgb(rgb);
                entry.mode = mode_idx;
            }
        }
    }
}

fn clamp_index(idx: &mut usize, len: usize) {
    if len == 0 {
        *idx = 0;
    } else if *idx >= len {
        *idx = len - 1;
    }
}

fn wrap_next(idx: &mut usize, len: usize) {
    if len > 0 {
        *idx = (*idx + 1) % len;
    } else {
        *idx = 0;
    }
}

fn wrap_prev(idx: &mut usize, len: usize) {
    if len > 0 {
        *idx = (*idx + len - 1) % len;
    } else {
        *idx = 0;
    }
}

/// Recolecta una instantánea de dispositivos, sensores, canales PWM, pywal y daemon.
fn collect_snapshot(
    backend: &CliLiquidctlBackend,
    socket: &Path,
    pywal_path: Option<&Path>,
    lighting_cache: &mut LightingCache,
) -> TuiSnapshot {
    let mut errors = Vec::new();
    let devices = backend.status(None).unwrap_or_else(|e| {
        errors.push(format!("liquidctl: {e}"));
        Vec::new()
    });
    let lighting_info = lighting_cache.get_or_refresh(&devices);
    let hwmon = list_hwmon_sensors().unwrap_or_else(|e| {
        errors.push(format!("hwmon: {e}"));
        Vec::new()
    });
    let hwmon_fans = list_hwmon_fans().unwrap_or_default();
    let nvidia = list_nvidia_gpus().unwrap_or_default();
    let daemon = match send_request(socket, &IpcRequest::Status) {
        Ok(IpcResponse::Status { daemon }) => Some(daemon),
        _ => None,
    };
    let mut channels = extract_channels(&devices);
    channels.extend(extract_hwmon_channels(&hwmon_fans, &devices));
    let pywal = load_pywal_palette(pywal_path).ok();
    TuiSnapshot {
        devices,
        hwmon,
        hwmon_fans,
        nvidia,
        daemon,
        channels,
        lighting_info,
        pywal,
        error: if errors.is_empty() {
            None
        } else {
            Some(errors.join(" | "))
        },
    }
}

/// Lanza el hilo de sondeo; refresca al inicio, cada `interval` o al recibir un trigger.
fn spawn_poller(
    socket: PathBuf,
    pywal_path: Option<PathBuf>,
    interval: Duration,
    tx: Sender<TuiSnapshot>,
    trigger_rx: Receiver<()>,
) {
    std::thread::spawn(move || {
        let backend = CliLiquidctlBackend::new();
        let mut lighting_cache = LightingCache::default();
        loop {
            let snap = collect_snapshot(
                &backend,
                &socket,
                pywal_path.as_deref(),
                &mut lighting_cache,
            );
            if tx.send(snap).is_err() {
                break;
            }
            match trigger_rx.recv_timeout(interval) {
                Ok(()) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    });
}

/// Restaura el terminal al soltarse (incluso ante errores o pánicos).
struct TerminalGuard;

impl TerminalGuard {
    fn new() -> Result<Self> {
        enable_raw_mode()?;
        if let Err(e) = execute!(io::stdout(), EnterAlternateScreen) {
            let _ = disable_raw_mode();
            return Err(e.into());
        }
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

/// Resultado del manejo de una tecla.
#[derive(Debug, PartialEq, Eq)]
enum KeyAction {
    None,
    Quit,
    ApplyProfile(String),
    ReapplyProfile,
    Refresh,
    SaveLanguage,
    SaveConfig(String),
    ApplyFanSpeed {
        device: String,
        channel: String,
        duty: u8,
    },
    SyncPywal,
    ApplyAllColor {
        hex: String,
        mode: String,
    },
    ApplyChannelColor {
        device: String,
        channel: String,
        mode: String,
        colors: Vec<String>,
    },
    ExportProfile(String),
}

fn map_key(state: &mut TuiState, key: KeyEvent) -> KeyAction {
    if key.kind == KeyEventKind::Release {
        return KeyAction::None;
    }
    match key.code {
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => KeyAction::Quit,
        KeyCode::Char('q') => KeyAction::Quit,
        KeyCode::Esc if state.show_help => {
            state.show_help = false;
            KeyAction::None
        }
        KeyCode::Esc => KeyAction::Quit,
        KeyCode::Char('?') => {
            state.show_help = !state.show_help;
            KeyAction::None
        }
        _ if state.show_help => KeyAction::None,
        KeyCode::Char(c @ '1'..='4') => {
            if let Some(v) = TuiView::from_number(c as u8 - b'0') {
                state.view = v;
            }
            KeyAction::None
        }
        KeyCode::PageDown => {
            state.view = state.view.next();
            KeyAction::None
        }
        KeyCode::PageUp => {
            state.view = state.view.prev();
            KeyAction::None
        }
        KeyCode::Char('[') => {
            state.prev_profile();
            state
                .selected_name()
                .map(|n| KeyAction::ApplyProfile(n.to_string()))
                .unwrap_or(KeyAction::None)
        }
        KeyCode::Char(']') => {
            state.next_profile();
            state
                .selected_name()
                .map(|n| KeyAction::ApplyProfile(n.to_string()))
                .unwrap_or(KeyAction::None)
        }
        KeyCode::Char('p') => KeyAction::ReapplyProfile,
        KeyCode::Char('r') => KeyAction::Refresh,
        KeyCode::Char('l') => {
            state.lang = state.lang.toggle();
            state.config.language = state.lang;
            state.status_message = Some(i18n::fmt_language_changed(state.lang));
            state.status_ok = true;
            KeyAction::SaveLanguage
        }
        _ => match state.view {
            TuiView::Dashboard => map_dashboard_key(state, key.code),
            TuiView::Fans => map_fans_key(state, key.code),
            TuiView::Rgb => map_rgb_key(state, key.code),
            TuiView::Profiles => map_profiles_key(state, key.code),
        },
    }
}

fn map_dashboard_key(state: &mut TuiState, code: KeyCode) -> KeyAction {
    match code {
        KeyCode::Tab => {
            state.dashboard_focus = match state.dashboard_focus {
                DashboardFocus::Featured => DashboardFocus::OtherSensors,
                DashboardFocus::OtherSensors => DashboardFocus::Featured,
            };
        }
        KeyCode::Up | KeyCode::Char('k') => match state.dashboard_focus {
            DashboardFocus::Featured => {
                let len = state.visible_featured_ids().len();
                wrap_prev(&mut state.selected_featured, len);
            }
            DashboardFocus::OtherSensors => {
                let len = state.other_sensor_ids().len();
                wrap_prev(&mut state.selected_other_sensor, len);
            }
        },
        KeyCode::Down | KeyCode::Char('j') => match state.dashboard_focus {
            DashboardFocus::Featured => {
                let len = state.visible_featured_ids().len();
                wrap_next(&mut state.selected_featured, len);
            }
            DashboardFocus::OtherSensors => {
                let len = state.other_sensor_ids().len();
                wrap_next(&mut state.selected_other_sensor, len);
            }
        },
        KeyCode::Char('f') | KeyCode::Enter => match state.dashboard_focus {
            DashboardFocus::Featured => {
                let visible = state.visible_featured_ids();
                if let Some(id) = visible.get(state.selected_featured).cloned() {
                    demote_sensor(&mut state.featured_sensors, &id);
                    state.clamp_selections();
                    state.status_message = Some(i18n::fmt_sensor_demoted(state.lang, &id));
                    state.status_ok = true;
                }
            }
            DashboardFocus::OtherSensors => {
                let others = state.other_sensor_ids();
                if let Some(id) = others.get(state.selected_other_sensor).cloned() {
                    promote_sensor(&mut state.featured_sensors, &id);
                    state.clamp_selections();
                    state.status_message = Some(i18n::fmt_sensor_promoted(state.lang, &id));
                    state.status_ok = true;
                }
            }
        },
        _ => {}
    }
    KeyAction::None
}

fn map_fans_key(state: &mut TuiState, code: KeyCode) -> KeyAction {
    let ordered: Vec<(String, String, Option<f32>)> = state
        .ordered_fan_channels()
        .into_iter()
        .map(|c| (c.device.clone(), c.channel.clone(), c.duty))
        .collect();
    if ordered.is_empty() {
        return KeyAction::None;
    }
    clamp_index(&mut state.selected_fan_channel, ordered.len());
    match code {
        KeyCode::Up | KeyCode::Char('k') => {
            wrap_prev(&mut state.selected_fan_channel, ordered.len());
        }
        KeyCode::Down | KeyCode::Char('j') => {
            wrap_next(&mut state.selected_fan_channel, ordered.len());
        }
        KeyCode::Char('c') => {
            if !state.curve_names.is_empty() {
                let dev = &ordered[state.selected_fan_channel].0;
                let current = state.active_curve_for_device(dev);
                let next_idx = current
                    .as_deref()
                    .and_then(|c| state.curve_names.iter().position(|n| n == c))
                    .map(|i| (i + 1) % state.curve_names.len())
                    .unwrap_or(0);
                let next_curve = state.curve_names[next_idx].clone();
                state.device_curves.insert(dev.clone(), next_curve.clone());
                state.status_message = Some(i18n::fmt_curve_changed(state.lang, dev, &next_curve));
                state.status_ok = true;
            }
        }
        KeyCode::Left | KeyCode::Char('-') => {
            let (dev, ch, default_duty) = &ordered[state.selected_fan_channel];
            let entry = state
                .manual_duty
                .entry((dev.clone(), ch.clone()))
                .or_insert_with(|| default_duty.unwrap_or(0.0));
            *entry = (*entry - 5.0).clamp(0.0, 100.0);
        }
        KeyCode::Right | KeyCode::Char('+') | KeyCode::Char('=') => {
            let (dev, ch, default_duty) = &ordered[state.selected_fan_channel];
            let entry = state
                .manual_duty
                .entry((dev.clone(), ch.clone()))
                .or_insert_with(|| default_duty.unwrap_or(0.0));
            *entry = (*entry + 5.0).clamp(0.0, 100.0);
        }
        KeyCode::Enter | KeyCode::Char('a') => {
            let (dev, ch, default_duty) = &ordered[state.selected_fan_channel];
            let duty = state
                .manual_duty
                .get(&(dev.clone(), ch.clone()))
                .copied()
                .or(*default_duty)
                .unwrap_or(0.0)
                .round()
                .clamp(0.0, 100.0) as u8;
            return KeyAction::ApplyFanSpeed {
                device: dev.clone(),
                channel: ch.clone(),
                duty,
            };
        }
        _ => {}
    }
    KeyAction::None
}

fn map_rgb_key(state: &mut TuiState, code: KeyCode) -> KeyAction {
    if code == KeyCode::Tab {
        state.rgb_focus = match state.rgb_focus {
            RgbFocus::Pywal => RgbFocus::Channels,
            RgbFocus::Channels => RgbFocus::Pywal,
        };
        return KeyAction::None;
    }

    match state.rgb_focus {
        RgbFocus::Pywal => match code {
            KeyCode::Char('t') => {
                state.config.pywal.enabled = !state.config.pywal.enabled;
                let msg = i18n::fmt_pywal_toggled(state.lang, state.config.pywal.enabled);
                state.status_message = Some(msg.clone());
                state.status_ok = true;
                KeyAction::SaveConfig(msg)
            }
            KeyCode::Char('m') => {
                let current = state.pywal_mode();
                let idx = PYWAL_MODES
                    .iter()
                    .position(|m| *m == current)
                    .map(|i| (i + 1) % PYWAL_MODES.len())
                    .unwrap_or(0);
                let next_mode = PYWAL_MODES[idx].to_string();
                state.config.pywal.mode = next_mode.clone();
                let msg = i18n::fmt_pywal_mode_changed(state.lang, &next_mode);
                state.status_message = Some(msg.clone());
                state.status_ok = true;
                KeyAction::SaveConfig(msg)
            }
            KeyCode::Char('x') => {
                state.config.pywal.filter_extremes = !state.config.pywal.filter_extremes;
                let len = state.pywal_entries().len();
                clamp_index(&mut state.selected_swatch, len);
                let msg = i18n::fmt_pywal_palette_updated(state.lang);
                state.status_message = Some(msg.clone());
                state.status_ok = true;
                KeyAction::SaveConfig(msg)
            }
            KeyCode::Char('-') => {
                let len = state.pywal_entries().len().max(1);
                state.config.pywal.palette_offset =
                    (state.config.pywal.palette_offset + len - 1) % len;
                let msg = i18n::fmt_pywal_palette_updated(state.lang);
                state.status_message = Some(msg.clone());
                state.status_ok = true;
                KeyAction::SaveConfig(msg)
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                let len = state.pywal_entries().len().max(1);
                state.config.pywal.palette_offset = (state.config.pywal.palette_offset + 1) % len;
                let msg = i18n::fmt_pywal_palette_updated(state.lang);
                state.status_message = Some(msg.clone());
                state.status_ok = true;
                KeyAction::SaveConfig(msg)
            }
            KeyCode::Left => {
                let len = state.pywal_entries().len();
                wrap_prev(&mut state.selected_swatch, len);
                KeyAction::None
            }
            KeyCode::Right => {
                let len = state.pywal_entries().len();
                wrap_next(&mut state.selected_swatch, len);
                KeyAction::None
            }
            KeyCode::Char('s') => KeyAction::SyncPywal,
            KeyCode::Enter | KeyCode::Char('a') => {
                let entries = state.pywal_entries();
                if let Some((_, hex, [r, g, b])) = entries.get(state.selected_swatch).cloned() {
                    let mode = state.pywal_mode();
                    state.sync_rgb_rows((r, g, b), &mode);
                    KeyAction::ApplyAllColor { hex, mode }
                } else {
                    KeyAction::None
                }
            }
            _ => KeyAction::None,
        },
        RgbFocus::Channels => {
            let rows = state.rgb_channel_list();
            if rows.is_empty() {
                return KeyAction::None;
            }
            clamp_index(&mut state.selected_rgb_channel, rows.len());
            match code {
                KeyCode::Up | KeyCode::Char('k') => {
                    wrap_prev(&mut state.selected_rgb_channel, rows.len());
                    KeyAction::None
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    wrap_next(&mut state.selected_rgb_channel, rows.len());
                    KeyAction::None
                }
                KeyCode::Char('m') => {
                    let (dev, ch, modes) = &rows[state.selected_rgb_channel];
                    if !modes.is_empty() {
                        let entry = state
                            .rgb_per_channel
                            .entry((dev.clone(), ch.clone()))
                            .or_default();
                        entry.mode = (entry.mode + 1) % modes.len();
                    }
                    KeyAction::None
                }
                KeyCode::Left => {
                    let (dev, ch, _) = &rows[state.selected_rgb_channel];
                    let entry = state
                        .rgb_per_channel
                        .entry((dev.clone(), ch.clone()))
                        .or_default();
                    entry.prev_preset();
                    KeyAction::None
                }
                KeyCode::Right | KeyCode::Char('c') => {
                    let (dev, ch, _) = &rows[state.selected_rgb_channel];
                    let entry = state
                        .rgb_per_channel
                        .entry((dev.clone(), ch.clone()))
                        .or_default();
                    entry.next_preset();
                    KeyAction::None
                }
                KeyCode::Enter | KeyCode::Char('a') => {
                    let (dev, ch, modes) = &rows[state.selected_rgb_channel];
                    let st = state
                        .rgb_per_channel
                        .entry((dev.clone(), ch.clone()))
                        .or_default();
                    let mode_idx = st.mode.min(modes.len().saturating_sub(1));
                    let mode = modes
                        .get(mode_idx)
                        .cloned()
                        .unwrap_or_else(|| "fixed".to_string());
                    let colors = if mode == "spectrum-wave" || mode == "off" {
                        Vec::new()
                    } else {
                        vec![st.hex()]
                    };
                    KeyAction::ApplyChannelColor {
                        device: dev.clone(),
                        channel: ch.clone(),
                        mode,
                        colors,
                    }
                }
                _ => KeyAction::None,
            }
        }
    }
}

fn map_profiles_key(state: &mut TuiState, code: KeyCode) -> KeyAction {
    match code {
        KeyCode::Up | KeyCode::Char('k') => {
            state.prev_profile();
            KeyAction::None
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.next_profile();
            KeyAction::None
        }
        KeyCode::Enter | KeyCode::Char('a') => state
            .selected_name()
            .map(|n| KeyAction::ApplyProfile(n.to_string()))
            .unwrap_or(KeyAction::None),
        KeyCode::Char('d') => {
            let Some(name) = state.selected_name().map(str::to_string) else {
                return KeyAction::None;
            };
            state.config.default_profile = name.clone();
            let msg = i18n::fmt_default_profile_set(state.lang, &name);
            state.status_message = Some(msg.clone());
            state.status_ok = true;
            KeyAction::SaveConfig(msg)
        }
        KeyCode::Char('n') => {
            let profile = capture_profile_from_state(
                &state.rgb_per_channel,
                &state.manual_duty,
                &state.device_curves,
                &state.snapshot.channels,
                &state.snapshot.lighting_info,
            );
            let new_name = next_available_profile_name(&state.config.profiles, "custom");
            state.config.profiles.insert(new_name.clone(), profile);
            state.refresh_config_lists();
            if let Some(pos) = state.profile_names.iter().position(|n| *n == new_name) {
                state.selected_profile = pos;
            }
            let msg = i18n::fmt_profile_saved(state.lang, &new_name);
            state.status_message = Some(msg.clone());
            state.status_ok = true;
            KeyAction::SaveConfig(msg)
        }
        KeyCode::Char('g') => {
            let close_to_tray = state.config.close_to_tray;
            let mut generated = Config::from_detected(
                &state.snapshot.devices,
                &state.snapshot.hwmon,
                &state.snapshot.hwmon_fans,
                &state.snapshot.nvidia,
            );
            generated.language = state.lang;
            generated.close_to_tray = close_to_tray;
            state.config = generated;
            state.refresh_config_lists();
            let msg = i18n::fmt_config_autogenerated(state.lang);
            state.status_message = Some(msg.clone());
            state.status_ok = true;
            KeyAction::SaveConfig(msg)
        }
        KeyCode::Char('c') => {
            let Some(name) = state.selected_name().map(str::to_string) else {
                return KeyAction::None;
            };
            let Some(orig) = state.config.profiles.get(&name).cloned() else {
                return KeyAction::None;
            };
            let clone_name = next_available_profile_name(&state.config.profiles, &name);
            state.config.profiles.insert(clone_name.clone(), orig);
            state.refresh_config_lists();
            if let Some(pos) = state.profile_names.iter().position(|n| *n == clone_name) {
                state.selected_profile = pos;
            }
            let msg = i18n::fmt_profile_cloned(state.lang, &clone_name);
            state.status_message = Some(msg.clone());
            state.status_ok = true;
            KeyAction::SaveConfig(msg)
        }
        KeyCode::Char('w') => state
            .selected_name()
            .map(|n| KeyAction::ExportProfile(n.to_string()))
            .unwrap_or(KeyAction::None),
        KeyCode::Delete | KeyCode::Backspace => {
            let Some(name) = state.selected_name().map(str::to_string) else {
                return KeyAction::None;
            };
            if is_deletable_profile(
                &name,
                &state.config.default_profile,
                state.config.profiles.len(),
            ) {
                state.config.profiles.remove(&name);
                state.refresh_config_lists();
                let msg = i18n::fmt_profile_deleted(state.lang, &name);
                state.status_message = Some(msg.clone());
                state.status_ok = true;
                KeyAction::SaveConfig(msg)
            } else {
                state.status_message = Some(i18n::fmt_cannot_delete_profile(state.lang, &name));
                state.status_ok = false;
                KeyAction::None
            }
        }
        _ => KeyAction::None,
    }
}

fn apply_profile_named(state: &mut TuiState, backend: &CliLiquidctlBackend, name: &str) {
    if !state.config.profiles.contains_key(name) {
        let s = i18n::tui(state.lang);
        state.status_message = Some(s.no_profiles.into());
        state.status_ok = false;
        return;
    }
    let connected = state.snapshot.daemon.is_some();
    if connected && !state.dry_run {
        match send_request(
            &state.socket_path,
            &IpcRequest::SetProfile {
                profile: name.to_string(),
            },
        ) {
            Ok(IpcResponse::Ok { message }) => {
                state.status_message =
                    Some(i18n::fmt_tui_profile_ipc_ok(state.lang, name, &message));
                state.status_ok = true;
                state.local_active_profile = Some(name.to_string());
            }
            Ok(IpcResponse::Error { message }) => {
                state.status_message = Some(i18n::fmt_tui_error(state.lang, &message));
                state.status_ok = false;
            }
            Ok(_) => {
                state.status_message = Some(i18n::fmt_tui_profile_sent(state.lang, name));
                state.status_ok = true;
                state.local_active_profile = Some(name.to_string());
            }
            Err(e) => {
                state.status_message = Some(i18n::fmt_tui_ipc_error(state.lang, &e));
                state.status_ok = false;
            }
        }
    } else {
        match apply_profile(backend, &state.config, name, state.dry_run) {
            Ok(r) => {
                state.status_message = Some(i18n::fmt_tui_profile_applied(
                    state.lang,
                    name,
                    r.lighting_applied,
                    r.cooling_applied,
                ));
                state.status_ok = true;
                state.local_active_profile = Some(name.to_string());
            }
            Err(e) => {
                state.status_message = Some(i18n::fmt_tui_error(state.lang, &e));
                state.status_ok = false;
            }
        }
    }
}

fn reapply_active(state: &mut TuiState, backend: &CliLiquidctlBackend) {
    if state.snapshot.daemon.is_some() && !state.dry_run {
        match send_request(&state.socket_path, &IpcRequest::Reapply) {
            Ok(IpcResponse::Ok { message }) => {
                state.status_message = Some(message);
                state.status_ok = true;
            }
            Ok(IpcResponse::Error { message }) => {
                state.status_message = Some(i18n::fmt_tui_error(state.lang, &message));
                state.status_ok = false;
            }
            Ok(_) => {
                let active = state.active_profile().to_string();
                state.status_message = Some(i18n::fmt_tui_profile_sent(state.lang, &active));
                state.status_ok = true;
            }
            Err(e) => {
                state.status_message = Some(i18n::fmt_tui_ipc_error(state.lang, &e));
                state.status_ok = false;
            }
        }
    } else {
        let active = state.active_profile().to_string();
        apply_profile_named(state, backend, &active);
    }
}

fn reload_config_and_daemon(state: &mut TuiState) {
    let s = i18n::tui(state.lang);
    let mut loaded_ok = false;
    match Config::load(state.config_path.as_deref()) {
        Ok(cfg) => {
            state.config = cfg;
            state.lang = state.config.language;
            state.refresh_config_lists();
            loaded_ok = true;
        }
        Err(e) => {
            state.status_message = Some(i18n::fmt_tui_error(state.lang, &e));
            state.status_ok = false;
        }
    }
    if state.snapshot.daemon.is_some() && !state.dry_run {
        match send_request(&state.socket_path, &IpcRequest::Reload) {
            Ok(IpcResponse::Ok { message }) => {
                state.status_message = Some(message);
                state.status_ok = true;
            }
            Ok(IpcResponse::Error { message }) => {
                state.status_message = Some(i18n::fmt_tui_error(state.lang, &message));
                state.status_ok = false;
            }
            Ok(_) => {
                state.status_message = Some(s.reloaded.into());
                state.status_ok = true;
            }
            Err(e) => {
                state.status_message = Some(i18n::fmt_tui_ipc_error(state.lang, &e));
                state.status_ok = false;
            }
        }
    } else if loaded_ok {
        state.status_message = Some(i18n::fmt_config_reloaded(state.lang));
        state.status_ok = true;
    }
}

fn save_and_reload_config(state: &mut TuiState, ok_msg: String) {
    match state.config.save(state.config_path.as_deref()) {
        Ok(_) => {
            if state.snapshot.daemon.is_some() && !state.dry_run {
                let _ = send_request(&state.socket_path, &IpcRequest::Reload);
            }
            state.status_message = Some(ok_msg);
            state.status_ok = true;
        }
        Err(e) => {
            state.status_message = Some(i18n::fmt_tui_error(state.lang, &e));
            state.status_ok = false;
        }
    }
}

fn export_profile_to_disk(state: &mut TuiState, name: &str) {
    let base_dir = state
        .config_path
        .as_deref()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .or_else(|| {
            Config::default_path()
                .ok()
                .and_then(|p| p.parent().map(Path::to_path_buf))
        })
        .unwrap_or_else(|| PathBuf::from("."));
    let profiles_dir = base_dir.join("profiles");
    if let Err(e) = std::fs::create_dir_all(&profiles_dir) {
        state.status_message = Some(i18n::fmt_tui_error(state.lang, &e));
        state.status_ok = false;
        return;
    }
    let export_path = profiles_dir.join(format!("{name}.toml"));
    match state.config.export_profile(name, &export_path) {
        Ok(()) => {
            state.status_message = Some(i18n::fmt_profile_exported(
                state.lang,
                name,
                &export_path.display().to_string(),
            ));
            state.status_ok = true;
        }
        Err(e) => {
            state.status_message = Some(i18n::fmt_tui_error(state.lang, &e));
            state.status_ok = false;
        }
    }
}

fn execute_action(
    action: KeyAction,
    state: &mut TuiState,
    backend: &CliLiquidctlBackend,
    trigger: &Sender<()>,
) -> bool {
    match action {
        KeyAction::None => false,
        KeyAction::Quit => true,
        KeyAction::ApplyProfile(name) => {
            apply_profile_named(state, backend, &name);
            let _ = trigger.send(());
            false
        }
        KeyAction::ReapplyProfile => {
            reapply_active(state, backend);
            let _ = trigger.send(());
            false
        }
        KeyAction::Refresh => {
            reload_config_and_daemon(state);
            let _ = trigger.send(());
            false
        }
        KeyAction::SaveLanguage => {
            let exists = state.config_path.as_ref().is_some_and(|p| p.exists())
                || (state.config_path.is_none()
                    && Config::default_path().ok().is_some_and(|p| p.exists()));
            if exists {
                save_and_reload_config(state, i18n::fmt_language_changed(state.lang));
            }
            false
        }
        KeyAction::SaveConfig(msg) => {
            save_and_reload_config(state, msg);
            false
        }
        KeyAction::ApplyFanSpeed {
            device,
            channel,
            duty,
        } => {
            match backend.set_speed(&device, &channel, duty, state.dry_run) {
                Ok(()) => {
                    if !state.dry_run {
                        state.status_message =
                            Some(i18n::fmt_speed_set(state.lang, &device, &channel, duty));
                    } else {
                        state.status_message = Some(format!(
                            "[dry-run] {}",
                            i18n::fmt_speed_set(state.lang, &device, &channel, duty)
                        ));
                    }
                    state.status_ok = true;
                }
                Err(e) => {
                    state.status_message = Some(i18n::fmt_tui_error(state.lang, &e));
                    state.status_ok = false;
                }
            }
            let _ = trigger.send(());
            false
        }
        KeyAction::SyncPywal => {
            if let Some(palette) = state.snapshot.pywal.clone() {
                let entries = state.pywal_entries();
                let mode = state.pywal_mode();
                let primary_rgb = if !state.config.pywal.filter_extremes
                    && state.config.pywal.palette_offset == 0
                {
                    palette
                        .color_rgb("color4")
                        .or_else(|| palette.color_rgb("color1"))
                } else {
                    entries.first().map(|(_, _, rgb)| *rgb)
                };
                match apply_pywal_palette(backend, &state.config.pywal, &palette, state.dry_run) {
                    Ok(count) => {
                        if let Some([r, g, b]) = primary_rgb {
                            state.sync_rgb_rows((r, g, b), &mode);
                        }
                        state.status_message = Some(i18n::fmt_pywal_synced(state.lang, count));
                        state.status_ok = true;
                    }
                    Err(e) => {
                        state.status_message = Some(i18n::fmt_tui_error(state.lang, &e));
                        state.status_ok = false;
                    }
                }
            }
            let _ = trigger.send(());
            false
        }
        KeyAction::ApplyAllColor { hex, mode } => {
            let mut last_err = None;
            for dev in &state.snapshot.devices {
                if let Err(e) = backend.set_color(
                    &dev.description,
                    "sync",
                    &mode,
                    std::slice::from_ref(&hex),
                    state.dry_run,
                ) {
                    last_err = Some(e);
                }
            }
            match last_err {
                None => {
                    state.status_message = Some(i18n::fmt_color_all(state.lang, &hex, &mode));
                    state.status_ok = true;
                }
                Some(e) => {
                    state.status_message = Some(i18n::fmt_tui_error(state.lang, &e));
                    state.status_ok = false;
                }
            }
            let _ = trigger.send(());
            false
        }
        KeyAction::ApplyChannelColor {
            device,
            channel,
            mode,
            colors,
        } => {
            match backend.set_color(&device, &channel, &mode, &colors, state.dry_run) {
                Ok(()) => {
                    state.status_message =
                        Some(i18n::fmt_color_set(state.lang, &device, &channel, &mode));
                    state.status_ok = true;
                }
                Err(e) => {
                    state.status_message = Some(i18n::fmt_tui_error(state.lang, &e));
                    state.status_ok = false;
                }
            }
            let _ = trigger.send(());
            false
        }
        KeyAction::ExportProfile(name) => {
            export_profile_to_disk(state, &name);
            false
        }
    }
}

/// Punto de entrada de `nxctl tui`.
pub fn run(
    config: Config,
    config_path: Option<PathBuf>,
    dry_run: bool,
    socket_path: PathBuf,
    interval: u64,
) -> Result<()> {
    let pywal_path = config.pywal.colors_path.as_deref().map(PathBuf::from);
    let mut state = TuiState::new(config, config_path, dry_run, socket_path.clone());
    let backend = CliLiquidctlBackend::new();
    let (tx, rx) = mpsc::channel();
    let (trigger_tx, trigger_rx) = mpsc::channel();
    spawn_poller(
        socket_path,
        pywal_path,
        Duration::from_secs(interval.max(1)),
        tx,
        trigger_rx,
    );

    let guard = TerminalGuard::new()?;
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        default_hook(info);
    }));
    let mut terminal: Terminal<CrosstermBackend<Stdout>> =
        Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let result = event_loop(&mut terminal, &mut state, &backend, &rx, &trigger_tx);
    drop(guard);
    let _ = terminal.show_cursor();
    result
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    state: &mut TuiState,
    backend: &CliLiquidctlBackend,
    rx: &Receiver<TuiSnapshot>,
    trigger: &Sender<()>,
) -> Result<()> {
    loop {
        while let Ok(snap) = rx.try_recv() {
            state.push_snapshot(snap);
        }
        terminal.draw(|f| draw(f, state))?;
        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
        {
            let action = map_key(state, key);
            if execute_action(action, state, backend, trigger) {
                return Ok(());
            }
        }
    }
}

fn temp_color(t: Option<f32>) -> Color {
    match t {
        Some(v) if v >= 70.0 => CRIMSON,
        Some(v) if v >= 50.0 => AMBER,
        Some(_) => EMERALD,
        None => TEXT_MUTED,
    }
}

fn category_meta(cat: SensorCategory, lang: Language) -> (&'static str, Color) {
    let s = i18n::tui(lang);
    match cat {
        SensorCategory::Ram => ("RAM", CYAN),
        SensorCategory::Ssd => ("SSD", MAGENTA),
        SensorCategory::Hdd => ("HDD", MAGENTA),
        SensorCategory::IGpu => ("iGPU", CYAN),
        SensorCategory::Wifi => ("WI-FI", EMERALD),
        SensorCategory::Net => (s.net_tag, EMERALD),
        SensorCategory::Sys => ("SYS", TEXT_SECONDARY),
    }
}

fn card_block<'a>(title: &'a str, focused: bool) -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if focused { BORDER_ACTIVE } else { BORDER_MUTED }))
        .style(Style::default().bg(BG_CARD).fg(TEXT_PRIMARY))
        .title(Span::styled(
            format!(" {} ", title.to_uppercase()),
            Style::default()
                .fg(if focused { CYAN } else { TEXT_SECONDARY })
                .add_modifier(Modifier::BOLD),
        ))
}

fn centered_rect(w: u16, h: u16, area: Rect) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect::new(
        area.x + area.width.saturating_sub(w) / 2,
        area.y + area.height.saturating_sub(h) / 2,
        w,
        h,
    )
}

fn draw_sidebar(frame: &mut Frame, state: &TuiState, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER_MUTED))
        .style(Style::default().bg(BG_SIDEBAR).fg(TEXT_PRIMARY));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let s = i18n::tui(state.lang);
    let extra_lines = u16::from(state.snapshot.daemon.is_some()) * 3
        + u16::from(state.status_message.is_some()) * 2
        + u16::from(state.snapshot.error.is_some()) * 2
        + u16::from(state.dry_run);
    let bottom_h = (6 + extra_lines).min(inner.height.saturating_sub(2).max(3));
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(2), Constraint::Length(bottom_h)])
        .split(inner);

    // Top: logo + 4 navigation views
    let nav_area = chunks[0];
    let mut y = nav_area.y;
    if y < nav_area.y.saturating_add(nav_area.height) {
        let header = Paragraph::new(Line::from(Span::styled(
            "NXCTL",
            Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
        )));
        frame.render_widget(header, Rect::new(nav_area.x, y, nav_area.width, 1));
        y = y.saturating_add(2);
    }

    for (idx, &view) in TuiView::ALL.iter().enumerate() {
        let num = idx + 1;
        let label = match view {
            TuiView::Dashboard => s.nav_dashboard,
            TuiView::Fans => s.nav_fans,
            TuiView::Rgb => s.nav_rgb,
            TuiView::Profiles => s.nav_profiles,
        };
        let avail_h = nav_area.y.saturating_add(nav_area.height).saturating_sub(y);
        if avail_h == 0 {
            break;
        }
        let active = state.view == view;
        if active && avail_h >= 3 {
            let r = Rect::new(nav_area.x, y, nav_area.width, 3);
            let active_block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_ACTIVE))
                .style(Style::default().bg(BG_CONTROL));
            let text = Paragraph::new(Line::from(vec![
                Span::styled(
                    format!("{num} "),
                    Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    label,
                    Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
                ),
            ]))
            .block(active_block);
            frame.render_widget(text, r);
            y = y.saturating_add(3);
        } else {
            let r = Rect::new(nav_area.x, y, nav_area.width, 1);
            let style = if active {
                Style::default()
                    .fg(CYAN)
                    .bg(BG_CONTROL)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(TEXT_SECONDARY)
            };
            frame.render_widget(Paragraph::new(format!(" {num} {label}")).style(style), r);
            y = y.saturating_add(1);
        }
    }

    // Bottom: DAEMON card + IDIOMA + DRY-RUN + status_message
    let bottom_area = chunks[1];
    if bottom_area.height >= 3 {
        let daemon_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(BORDER_MUTED))
            .style(Style::default().bg(BG_CARD));
        let daemon_inner = daemon_block.inner(bottom_area);
        frame.render_widget(daemon_block, bottom_area);
        if daemon_inner.width > 0 && daemon_inner.height > 0 {
            let (badge_txt, badge_col) = if state.snapshot.daemon.is_some() {
                (s.sys_ok, EMERALD)
            } else {
                (s.offline, CRIMSON)
            };
            let mut lines = vec![
                Line::from(vec![
                    Span::styled(
                        format!("{} ", s.daemon_label),
                        Style::default()
                            .fg(TEXT_SECONDARY)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("[{badge_txt}]"),
                        Style::default().fg(badge_col).add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(vec![
                    Span::styled(
                        format!("{}: ", s.language_label),
                        Style::default().fg(TEXT_MUTED),
                    ),
                    Span::styled(state.lang.display_name(), Style::default().fg(TEXT_PRIMARY)),
                ]),
            ];
            if state.dry_run {
                lines.push(Line::from(Span::styled(
                    format!("[{}]", s.dry_run_badge),
                    Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
                )));
            }
            if let Some(d) = &state.snapshot.daemon {
                lines.push(Line::from(Span::styled(
                    i18n::fmt_daemon_connected(
                        state.lang,
                        &d.active_profile,
                        d.poll_interval_secs,
                        d.hysteresis_celsius,
                    ),
                    Style::default().fg(TEXT_MUTED),
                )));
            }
            if let Some(msg) = &state.status_message {
                let col = if state.status_ok { EMERALD } else { AMBER };
                lines.push(Line::from(Span::styled(
                    msg.clone(),
                    Style::default().fg(col),
                )));
            }
            if let Some(err) = &state.snapshot.error {
                lines.push(Line::from(Span::styled(
                    format!("! {err}"),
                    Style::default().fg(CRIMSON),
                )));
            }
            frame.render_widget(
                Paragraph::new(lines).wrap(Wrap { trim: true }),
                daemon_inner,
            );
        }
    }
}

fn draw_top_bar(frame: &mut Frame, state: &TuiState, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let s = i18n::tui(state.lang);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER_MUTED))
        .style(Style::default().bg(BG_SIDEBAR).fg(TEXT_PRIMARY));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let fmt = |v: Option<f32>| v.map_or_else(|| "--".to_string(), |t| format!("{t:.0}°C"));
    let left_txt = i18n::fmt_top_summary(
        state.lang,
        &fmt(cpu_temp(&state.snapshot)),
        &fmt(gpu_temp(&state.snapshot)),
        state.snapshot.devices.len(),
    );

    let active = state.active_profile();
    let right_line = Line::from(vec![
        Span::styled(
            format!("{}: ", s.active_profile_label),
            Style::default().fg(TEXT_SECONDARY),
        ),
        Span::styled(
            active,
            Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ([/])", Style::default().fg(TEXT_MUTED)),
    ]);

    let right_w = (right_line.width() as u16).min(inner.width);
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(right_w)])
        .split(inner);

    frame.render_widget(
        Paragraph::new(Span::styled(left_txt, Style::default().fg(TEXT_SECONDARY))),
        cols[0],
    );
    frame.render_widget(Paragraph::new(right_line), cols[1]);
}

fn draw_footer(frame: &mut Frame, state: &TuiState, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let s = i18n::tui(state.lang);
    let text = match state.view {
        TuiView::Dashboard => s.footer_dashboard,
        TuiView::Fans => s.footer_fans,
        TuiView::Rgb => s.footer_rgb,
        TuiView::Profiles => s.footer_profiles,
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER_MUTED))
        .style(Style::default().bg(BG_SIDEBAR).fg(TEXT_SECONDARY));
    frame.render_widget(Paragraph::new(text).block(block), area);
}

fn draw_dashboard(frame: &mut Frame, state: &TuiState, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    draw_dashboard_featured(frame, state, cols[0]);
    draw_dashboard_right(frame, state, cols[1]);
}

struct FeaturedCard {
    title: String,
    sub: String,
    temp: Option<f32>,
    accent: Color,
    history: Vec<u64>,
}

fn build_featured_card(state: &TuiState, id: &str) -> Option<FeaturedCard> {
    let s = i18n::tui(state.lang);
    match id {
        "cpu" => Some(FeaturedCard {
            title: "CPU".into(),
            sub: "PACKAGE".into(),
            temp: cpu_temp(&state.snapshot),
            accent: CYAN,
            history: state.cpu_history.iter().copied().collect(),
        }),
        "gpu" => Some(FeaturedCard {
            title: "GPU".into(),
            sub: "NVIDIA".into(),
            temp: gpu_temp(&state.snapshot),
            accent: MAGENTA,
            history: state.gpu_history.iter().copied().collect(),
        }),
        "aio" => liquid_temp(&state.snapshot).map(|t| FeaturedCard {
            title: s.aio_liquid.into(),
            sub: s.liquid_sub.into(),
            temp: Some(t),
            accent: EMERALD,
            history: state.liquid_history.iter().copied().collect(),
        }),
        other => state
            .snapshot
            .hwmon
            .iter()
            .find(|h| hwmon_sensor_id(h) == other)
            .map(|h| {
                let (tag, accent) = category_meta(classify_hwmon_sensor(&h.name), state.lang);
                let label = h.label.as_deref().unwrap_or(&h.input_file);
                let hist = state
                    .hwmon_history
                    .get(other)
                    .map(|d| d.iter().copied().collect())
                    .unwrap_or_default();
                FeaturedCard {
                    title: format!("{} ({label})", h.name),
                    sub: tag.into(),
                    temp: Some(h.temp_c),
                    accent,
                    history: hist,
                }
            }),
    }
}

fn draw_dashboard_featured(frame: &mut Frame, state: &TuiState, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let s = i18n::tui(state.lang);
    let ids = state.visible_featured_ids();
    let card_h: u16 = 5;
    let max_cards = (area.height / card_h).max(1) as usize;
    let start = if state.selected_featured >= max_cards {
        state.selected_featured + 1 - max_cards
    } else {
        0
    };
    let mut y = area.y;

    for (idx, id) in ids.iter().enumerate().skip(start) {
        let avail = area.y.saturating_add(area.height).saturating_sub(y);
        if avail < 3 {
            break;
        }
        let h = card_h.min(avail);
        let card_rect = Rect::new(area.x, y, area.width, h);
        y = y.saturating_add(h);

        let Some(card) = build_featured_card(state, id) else {
            continue;
        };
        let is_selected =
            state.dashboard_focus == DashboardFocus::Featured && idx == state.selected_featured;
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(if is_selected { CYAN } else { BORDER_MUTED }))
            .style(Style::default().bg(BG_CARD).fg(TEXT_PRIMARY))
            .title(Line::from(vec![
                Span::styled(
                    format!(" {} ", card.title),
                    Style::default()
                        .fg(card.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("[{}] ", card.sub), Style::default().fg(TEXT_MUTED)),
            ]));
        let inner = block.inner(card_rect);
        frame.render_widget(block, card_rect);
        if inner.width == 0 || inner.height == 0 {
            continue;
        }

        let temp_txt = card
            .temp
            .map_or_else(|| "-- °C".to_string(), |t| format!("{t:.1}°C"));
        let mut header_spans = vec![Span::styled(
            temp_txt,
            Style::default()
                .fg(temp_color(card.temp))
                .add_modifier(Modifier::BOLD),
        )];
        if is_selected {
            header_spans.push(Span::styled(
                format!("   [f] {}", s.demote),
                Style::default().fg(TEXT_MUTED),
            ));
        }

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(0),
            ])
            .split(inner);

        frame.render_widget(Paragraph::new(Line::from(header_spans)), rows[0]);

        if rows[1].height > 0 {
            let ratio = card
                .temp
                .map(|t| f64::from((t / 100.0).clamp(0.0, 1.0)))
                .unwrap_or(0.0);
            frame.render_widget(
                LineGauge::default()
                    .ratio(ratio)
                    .label("")
                    .filled_style(Style::default().fg(card.accent))
                    .unfilled_style(Style::default().fg(BG_CONTROL)),
                rows[1],
            );
        }

        if rows[2].height > 0 {
            frame.render_widget(
                Sparkline::default()
                    .data(&card.history)
                    .style(Style::default().fg(card.accent)),
                rows[2],
            );
        }
    }
}

fn draw_dashboard_right(frame: &mut Frame, state: &TuiState, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let pwm_h = (state.snapshot.channels.len() as u16 * 2 + 2)
        .max(4)
        .min(area.height / 2 + 2);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(pwm_h), Constraint::Min(3)])
        .split(area);

    draw_pwm_headers_card(frame, state, chunks[0]);
    draw_other_sensors_card(frame, state, chunks[1]);
}

fn draw_pwm_headers_card(frame: &mut Frame, state: &TuiState, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let s = i18n::tui(state.lang);
    let block = card_block(s.pwm_headers, false);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    if state.snapshot.channels.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                s.no_nzxt_channels,
                Style::default().fg(TEXT_MUTED),
            )),
            inner,
        );
        return;
    }

    let mut y = inner.y;
    for ch in &state.snapshot.channels {
        let avail = inner.y.saturating_add(inner.height).saturating_sub(y);
        if avail == 0 {
            break;
        }
        let accent = if is_pump_channel(&ch.channel) {
            MAGENTA
        } else {
            CYAN
        };
        let rpm_txt = ch
            .rpm
            .map_or_else(|| "-- RPM".to_string(), |r| format!("{r:.0} RPM"));
        let duty = ch.duty.unwrap_or(0.0).clamp(0.0, 100.0);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(&ch.label, Style::default().fg(TEXT_PRIMARY)),
                Span::styled(
                    format!("  {rpm_txt} · {duty:.0}%"),
                    Style::default().fg(TEXT_SECONDARY),
                ),
            ])),
            Rect::new(inner.x, y, inner.width, 1),
        );
        y = y.saturating_add(1);
        if y < inner.y.saturating_add(inner.height) {
            frame.render_widget(
                LineGauge::default()
                    .ratio(f64::from(duty) / 100.0)
                    .label("")
                    .filled_style(Style::default().fg(accent))
                    .unfilled_style(Style::default().fg(BG_CONTROL)),
                Rect::new(inner.x, y, inner.width, 1),
            );
            y = y.saturating_add(1);
        }
    }
}

fn draw_other_sensors_card(frame: &mut Frame, state: &TuiState, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let s = i18n::tui(state.lang);
    let focused = state.dashboard_focus == DashboardFocus::OtherSensors;
    let block = card_block(s.other_sensors, focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let ids = state.other_sensor_ids();
    if ids.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                s.no_additional_sensors,
                Style::default().fg(TEXT_MUTED),
            )),
            inner,
        );
        return;
    }

    let mut items = Vec::with_capacity(ids.len());
    for (idx, id) in ids.iter().enumerate() {
        let selected = focused && idx == state.selected_other_sensor;
        let (tag, tag_col, name, temp) = match id.as_str() {
            "cpu" => (
                "CPU",
                CYAN,
                "CPU (PACKAGE)".to_string(),
                cpu_temp(&state.snapshot),
            ),
            "gpu" => (
                "GPU",
                MAGENTA,
                "GPU (NVIDIA)".to_string(),
                gpu_temp(&state.snapshot),
            ),
            "aio" => (
                "AIO",
                EMERALD,
                format!("AIO ({})", s.liquid_sub),
                liquid_temp(&state.snapshot),
            ),
            other => {
                if let Some(h) = state
                    .snapshot
                    .hwmon
                    .iter()
                    .find(|h| hwmon_sensor_id(h) == other)
                {
                    let (tag, col) = category_meta(classify_hwmon_sensor(&h.name), state.lang);
                    let label = h.label.as_deref().unwrap_or(&h.input_file);
                    (tag, col, format!("{} ({label})", h.name), Some(h.temp_c))
                } else {
                    ("SYS", TEXT_SECONDARY, other.to_string(), None)
                }
            }
        };

        let temp_txt = temp.map_or_else(|| "-- °C".to_string(), |t| format!("{t:.1}°C"));
        let mut spans = vec![
            Span::styled(
                if selected { "▸ " } else { "  " },
                Style::default().fg(CYAN),
            ),
            Span::styled(
                format!("[{tag}] "),
                Style::default().fg(tag_col).add_modifier(Modifier::BOLD),
            ),
            Span::styled(name, Style::default().fg(TEXT_PRIMARY)),
            Span::raw("  "),
            Span::styled(
                temp_txt,
                Style::default()
                    .fg(temp_color(temp))
                    .add_modifier(Modifier::BOLD),
            ),
        ];
        if selected {
            spans.push(Span::styled(
                format!("  [f] {}", s.feature),
                Style::default().fg(TEXT_MUTED),
            ));
        }
        let item_style = if selected {
            Style::default().bg(BG_CONTROL)
        } else {
            Style::default()
        };
        items.push(ListItem::new(Line::from(spans)).style(item_style));
    }

    frame.render_widget(List::new(items), inner);
}

fn draw_fans(frame: &mut Frame, state: &TuiState, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let s = i18n::tui(state.lang);
    if state.snapshot.channels.is_empty() {
        let block = card_block(s.nav_fans, false);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.width > 0 && inner.height > 0 {
            frame.render_widget(
                Paragraph::new(Span::styled(
                    s.no_cooling_channels,
                    Style::default().fg(TEXT_MUTED),
                )),
                inner,
            );
        }
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(52), Constraint::Min(4)])
        .split(area);

    draw_fan_curve_chart(frame, state, chunks[0]);
    draw_fan_channels_list(frame, state, chunks[1]);
}

fn draw_fan_curve_chart(frame: &mut Frame, state: &TuiState, area: Rect) {
    if area.width < 12 || area.height < 5 {
        return;
    }
    let s = i18n::tui(state.lang);
    let ordered = state.ordered_fan_channels();
    let selected_ch = ordered
        .get(state.selected_fan_channel)
        .or_else(|| ordered.first());
    let Some(selected_ch) = selected_ch else {
        return;
    };
    let dev = &selected_ch.device;
    let curve_name = state.active_curve_for_device(dev);
    let curve = curve_name
        .as_deref()
        .and_then(|n| state.config.curves.get(n))
        .or_else(|| state.config.curves.values().next());

    let pts: Vec<(f64, f64)> = curve
        .map(|c| {
            c.points
                .iter()
                .map(|p| (f64::from(p.temp_c), f64::from(p.duty)))
                .collect()
        })
        .unwrap_or_default();

    let dev_lower = dev.to_ascii_lowercase();
    let live_temp = if dev_lower.contains("nvidia")
        || dev_lower.contains("amdgpu")
        || dev_lower.contains("gpu")
    {
        gpu_temp(&state.snapshot)
    } else {
        liquid_temp(&state.snapshot).or_else(|| cpu_temp(&state.snapshot))
    };

    let (op_pts, op_label) = match (curve, live_temp) {
        (Some(c), Some(t)) => {
            let duty = evaluate_curve(c, t);
            (
                vec![(f64::from(t), f64::from(duty))],
                Some(i18n::fmt_operating_point(state.lang, t, duty)),
            )
        }
        _ => (Vec::new(), None),
    };

    let accent = if is_pump_channel(&selected_ch.channel) {
        MAGENTA
    } else {
        CYAN
    };

    let mut datasets = vec![
        Dataset::default()
            .name(curve_name.as_deref().unwrap_or(s.curve_label))
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(accent))
            .data(&pts),
    ];
    if !op_pts.is_empty() {
        datasets.push(
            Dataset::default()
                .name(s.curve_current_label)
                .marker(symbols::Marker::Block)
                .graph_type(GraphType::Scatter)
                .style(Style::default().fg(AMBER))
                .data(&op_pts),
        );
    }

    let max_t = pts
        .iter()
        .map(|p| p.0)
        .chain(op_pts.iter().map(|p| p.0))
        .fold(100.0_f64, f64::max);

    let title = match (&curve_name, &op_label) {
        (Some(cn), Some(op)) => format!(" {dev} · {cn} · {op} "),
        (Some(cn), None) => format!(" {dev} · {cn} "),
        (None, Some(op)) => format!(" {dev} · {op} "),
        (None, None) => format!(" {dev} "),
    };

    let chart = Chart::new(datasets)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_ACTIVE))
                .style(Style::default().bg(BG_CARD).fg(TEXT_PRIMARY))
                .title(Span::styled(
                    title,
                    Style::default().fg(accent).add_modifier(Modifier::BOLD),
                )),
        )
        .x_axis(
            Axis::default()
                .title("°C")
                .style(Style::default().fg(TEXT_MUTED))
                .bounds([0.0, max_t])
                .labels(["0".to_string(), format!("{max_t:.0}")]),
        )
        .y_axis(
            Axis::default()
                .title("%")
                .style(Style::default().fg(TEXT_MUTED))
                .bounds([0.0, 100.0])
                .labels(["0", "100"]),
        );
    frame.render_widget(chart, area);
}

fn draw_fan_channels_list(frame: &mut Frame, state: &TuiState, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let s = i18n::tui(state.lang);
    let block = card_block(s.nav_fans, true);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let groups = group_channels_by_device(&state.snapshot.channels);
    let mut y = inner.y;
    let max_y = inner.y.saturating_add(inner.height);
    let mut flat_idx = 0usize;

    for (dev, chs) in groups {
        if y >= max_y {
            break;
        }
        let all_pumps = !chs.is_empty() && chs.iter().all(|c| is_pump_channel(&c.channel));
        let header_col = if all_pumps { MAGENTA } else { CYAN };
        let curve_name = state
            .active_curve_for_device(&dev)
            .unwrap_or_else(|| "—".to_string());
        let badge = i18n::fmt_channels_badge(state.lang, chs.len());
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    dev.clone(),
                    Style::default().fg(header_col).add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("  [{badge}]"), Style::default().fg(TEXT_MUTED)),
                Span::styled(
                    format!("  {}: {curve_name} [c]", s.curve_label),
                    Style::default().fg(TEXT_SECONDARY),
                ),
            ])),
            Rect::new(inner.x, y, inner.width, 1),
        );
        y = y.saturating_add(1);

        for ch in chs {
            if y >= max_y {
                break;
            }
            let selected = flat_idx == state.selected_fan_channel;
            flat_idx += 1;
            let accent = if is_pump_channel(&ch.channel) {
                MAGENTA
            } else {
                CYAN
            };
            let duty = state
                .manual_duty
                .get(&(ch.device.clone(), ch.channel.clone()))
                .copied()
                .or(ch.duty)
                .unwrap_or(0.0)
                .clamp(0.0, 100.0);
            let rpm_txt = ch
                .rpm
                .map_or_else(|| "-- RPM".to_string(), |r| format!("{r:.0} RPM"));

            let row_rect = Rect::new(inner.x, y, inner.width, 1);
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
                .split(row_rect);

            let row_bg = if selected { BG_CONTROL } else { BG_CARD };
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(
                        if selected { "▸ " } else { "  " },
                        Style::default().fg(CYAN),
                    ),
                    Span::styled(
                        &ch.label,
                        Style::default().fg(if selected { CYAN } else { TEXT_PRIMARY }),
                    ),
                    Span::styled(format!("  {rpm_txt}"), Style::default().fg(TEXT_SECONDARY)),
                ]))
                .style(Style::default().bg(row_bg)),
                cols[0],
            );

            if cols[1].width > 0 {
                frame.render_widget(
                    LineGauge::default()
                        .ratio(f64::from(duty) / 100.0)
                        .label(format!("{}: {duty:.0}%", s.manual_duty_label))
                        .filled_style(Style::default().fg(accent))
                        .unfilled_style(Style::default().fg(BG_CONTROL)),
                    cols[1],
                );
            }
            y = y.saturating_add(1);
        }
    }
}

fn draw_rgb(frame: &mut Frame, state: &TuiState, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(9), Constraint::Min(4)])
        .split(area);
    draw_pywal_card(frame, state, chunks[0]);
    draw_rgb_devices_card(frame, state, chunks[1]);
}

fn draw_pywal_card(frame: &mut Frame, state: &TuiState, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let s = i18n::tui(state.lang);
    let focused = state.rgb_focus == RgbFocus::Pywal;
    let badge_txt = if state.config.pywal.enabled {
        s.enabled
    } else {
        s.disabled
    };
    let badge_col = if state.config.pywal.enabled {
        EMERALD
    } else {
        TEXT_MUTED
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if focused { BORDER_ACTIVE } else { BORDER_MUTED }))
        .style(Style::default().bg(BG_CARD).fg(TEXT_PRIMARY))
        .title(Line::from(vec![
            Span::styled(
                format!(" {} ", s.pywal_sync_title.to_uppercase()),
                Style::default()
                    .fg(if focused { CYAN } else { MAGENTA })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("[{badge_txt}] "),
                Style::default().fg(badge_col).add_modifier(Modifier::BOLD),
            ),
        ]));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let check_auto = if state.config.pywal.enabled {
        "[✓]"
    } else {
        "[ ]"
    };
    let check_filter = if state.config.pywal.filter_extremes {
        "[✓]"
    } else {
        "[ ]"
    };
    let mode = state.pywal_mode();
    let entries = state.pywal_entries();
    let offset_display = if entries.is_empty() {
        0
    } else {
        state.config.pywal.palette_offset % entries.len()
    };

    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                format!("[t] {check_auto} {}   ", s.pywal_auto_sync),
                Style::default().fg(TEXT_PRIMARY),
            ),
            Span::styled(
                format!("[m] {} ", s.global_mode),
                Style::default().fg(TEXT_SECONDARY),
            ),
            Span::styled(mode, Style::default().fg(CYAN).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled(
                format!("[x] {check_filter} {}   ", s.filter_extremes),
                Style::default().fg(TEXT_PRIMARY),
            ),
            Span::styled(
                format!("[-/+] {} ", s.shift_palette),
                Style::default().fg(TEXT_SECONDARY),
            ),
            Span::styled(
                format!("+{offset_display}"),
                Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
            ),
        ]),
    ];

    if let Some(palette) = &state.snapshot.pywal {
        let wp_name = palette
            .wallpaper
            .as_deref()
            .and_then(|w| Path::new(w).file_name().and_then(|n| n.to_str()))
            .unwrap_or("—");
        lines.push(Line::from(vec![
            Span::styled(
                i18n::fmt_wallpaper(state.lang, wp_name),
                Style::default().fg(TEXT_SECONDARY),
            ),
            Span::styled(
                format!("   [s] {}", s.sync_now),
                Style::default().fg(EMERALD),
            ),
        ]));

        let mut swatch_spans = Vec::new();
        for (idx, (_, hex, [r, g, b])) in entries.iter().enumerate() {
            let sel = focused && idx == state.selected_swatch;
            if sel {
                swatch_spans.push(Span::styled("[", Style::default().fg(CYAN)));
            }
            swatch_spans.push(Span::styled(
                "██",
                Style::default().fg(Color::Rgb(*r, *g, *b)),
            ));
            if sel {
                swatch_spans.push(Span::styled("]", Style::default().fg(CYAN)));
            } else {
                swatch_spans.push(Span::raw(" "));
            }
            let _ = hex;
        }
        if let Some((key, hex, _)) = entries.get(state.selected_swatch) {
            swatch_spans.push(Span::styled(
                format!("  {key}: #{hex}"),
                Style::default().fg(TEXT_SECONDARY),
            ));
        }
        lines.push(Line::from(swatch_spans));
    } else {
        lines.push(Line::from(Span::styled(
            s.pywal_not_detected,
            Style::default().fg(TEXT_MUTED),
        )));
    }

    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

fn draw_rgb_devices_card(frame: &mut Frame, state: &TuiState, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let s = i18n::tui(state.lang);
    let focused = state.rgb_focus == RgbFocus::Channels;
    let block = card_block(s.rgb_channels_title, focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    if state.snapshot.devices.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                s.no_nzxt_devices,
                Style::default().fg(TEXT_MUTED),
            )),
            inner,
        );
        return;
    }

    let mut lines = Vec::new();
    let mut flat_idx = 0usize;

    for dev in &state.snapshot.devices {
        let (channels, modes) = resolve_device_rgb(
            &state.snapshot.lighting_info,
            &state.config,
            &dev.description,
        );
        let badge = i18n::fmt_channels_badge(state.lang, channels.len());
        lines.push(Line::from(vec![
            Span::styled(
                dev.description.clone(),
                Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("  [{badge}]"), Style::default().fg(MAGENTA)),
        ]));

        for ch in channels {
            let selected = focused && flat_idx == state.selected_rgb_channel;
            flat_idx += 1;
            let st = state
                .rgb_per_channel
                .get(&(dev.description.clone(), ch.clone()))
                .cloned()
                .unwrap_or_default();
            let mode_idx = st.mode.min(modes.len().saturating_sub(1));
            let mode = modes
                .get(mode_idx)
                .map(String::as_str)
                .unwrap_or("fixed")
                .to_string();
            let (r, g, b) = st.rgb();
            let hex = st.hex();
            let ch_label = if ch == "sync" {
                s.sync_whole_device
            } else {
                ch.as_str()
            };
            let ch_col = if ch == "sync" { MAGENTA } else { TEXT_PRIMARY };
            let bg = if selected { BG_CONTROL } else { BG_CARD };
            lines.push(Line::from(vec![
                Span::styled(
                    if selected { "  ▸ " } else { "    " },
                    Style::default().fg(CYAN).bg(bg),
                ),
                Span::styled(
                    format!("{ch_label:<24}"),
                    Style::default().fg(ch_col).bg(bg),
                ),
                Span::styled(
                    format!(" [{mode:<13}] "),
                    Style::default().fg(TEXT_SECONDARY).bg(bg),
                ),
                Span::styled("██", Style::default().fg(Color::Rgb(r, g, b)).bg(bg)),
                Span::styled(format!(" #{hex}"), Style::default().fg(TEXT_PRIMARY).bg(bg)),
            ]));
        }
    }

    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_profiles(frame: &mut Frame, state: &TuiState, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);
    draw_profiles_list(frame, state, cols[0]);
    draw_profile_detail(frame, state, cols[1]);
}

fn draw_profiles_list(frame: &mut Frame, state: &TuiState, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let s = i18n::tui(state.lang);
    let block = card_block(s.profiles_title, true);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    if state.profile_names.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(s.no_profiles, Style::default().fg(TEXT_MUTED))),
            inner,
        );
        return;
    }

    let active = state.active_profile();
    let mut items = Vec::with_capacity(state.profile_names.len());
    for (i, name) in state.profile_names.iter().enumerate() {
        let selected = i == state.selected_profile;
        let bg = if selected { BG_CONTROL } else { BG_CARD };
        let mut header_spans = vec![
            Span::styled(
                if selected { "▸ " } else { "  " },
                Style::default().fg(CYAN).bg(bg),
            ),
            Span::styled(
                name.clone(),
                Style::default()
                    .fg(if selected { CYAN } else { TEXT_PRIMARY })
                    .bg(bg)
                    .add_modifier(Modifier::BOLD),
            ),
        ];
        if name == active {
            header_spans.push(Span::styled(
                format!(" [{}]", s.badge_active),
                Style::default().fg(EMERALD).bg(bg),
            ));
        }
        if name == &state.config.default_profile {
            header_spans.push(Span::styled(
                format!(" [{}]", s.badge_default),
                Style::default().fg(CYAN).bg(bg),
            ));
        }

        let mut lines = vec![Line::from(header_spans)];
        if let Some(p) = state.config.profiles.get(name) {
            if let Some(desc) = &p.description {
                lines.push(Line::from(Span::styled(
                    format!("    {desc}"),
                    Style::default().fg(TEXT_SECONDARY).bg(bg),
                )));
            }
            let summary =
                i18n::fmt_profile_rules_summary(state.lang, p.lighting.len(), p.cooling.len());
            lines.push(Line::from(Span::styled(
                format!("    {summary}"),
                Style::default().fg(TEXT_MUTED).bg(bg),
            )));
        }
        items.push(ListItem::new(lines).style(Style::default().bg(bg)));
    }

    frame.render_widget(List::new(items), inner);
}

fn draw_profile_detail(frame: &mut Frame, state: &TuiState, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let s = i18n::tui(state.lang);
    let block = card_block(s.profile_detail_title, false);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let Some(name) = state.selected_name() else {
        frame.render_widget(
            Paragraph::new(Span::styled(s.no_profiles, Style::default().fg(TEXT_MUTED))),
            inner,
        );
        return;
    };
    let Some(profile) = state.config.profiles.get(name) else {
        return;
    };

    let cli_s = i18n::cli(state.lang);
    let mut lines = vec![Line::from(Span::styled(
        name.to_string(),
        Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
    ))];
    if let Some(desc) = &profile.description {
        lines.push(Line::from(Span::styled(
            desc.clone(),
            Style::default().fg(TEXT_SECONDARY),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        cli_s.label_lighting,
        Style::default().fg(MAGENTA).add_modifier(Modifier::BOLD),
    )));
    if profile.lighting.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("  {}", s.no_lighting_rules),
            Style::default().fg(TEXT_MUTED),
        )));
    } else {
        for r in &profile.lighting {
            lines.push(Line::from(Span::styled(
                format!("  • {}", crate::format_lighting_rule_i18n(r, state.lang)),
                Style::default().fg(TEXT_PRIMARY),
            )));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        cli_s.label_cooling,
        Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
    )));
    if profile.cooling.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("  {}", s.no_cooling_rules),
            Style::default().fg(TEXT_MUTED),
        )));
    } else {
        for r in &profile.cooling {
            lines.push(Line::from(Span::styled(
                format!("  • {}", crate::format_cooling_rule_i18n(r, state.lang)),
                Style::default().fg(TEXT_PRIMARY),
            )));
        }
    }

    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

/// Dibuja la interfaz completa.
pub fn draw(frame: &mut Frame, state: &mut TuiState) {
    let area = frame.area();
    frame.render_widget(
        Block::default().style(Style::default().bg(BG_CANVAS).fg(TEXT_PRIMARY)),
        area,
    );
    if area.width == 0 || area.height == 0 {
        return;
    }

    let main_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(24), Constraint::Min(20)])
        .split(area);

    draw_sidebar(frame, state, main_cols[0]);

    let right_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(6),
            Constraint::Length(3),
        ])
        .split(main_cols[1]);

    draw_top_bar(frame, state, right_rows[0]);

    match state.view {
        TuiView::Dashboard => draw_dashboard(frame, state, right_rows[1]),
        TuiView::Fans => draw_fans(frame, state, right_rows[1]),
        TuiView::Rgb => draw_rgb(frame, state, right_rows[1]),
        TuiView::Profiles => draw_profiles(frame, state, right_rows[1]),
    }

    draw_footer(frame, state, right_rows[2]);

    if state.show_help {
        let s = i18n::tui(state.lang);
        let popup = centered_rect(58, s.help_lines.len() as u16 + 2, area);
        if popup.width > 0 && popup.height > 0 {
            frame.render_widget(Clear, popup);
            let lines: Vec<Line> = s
                .help_lines
                .iter()
                .map(|l| Line::from(Span::styled(*l, Style::default().fg(TEXT_PRIMARY))))
                .collect();
            frame.render_widget(
                Paragraph::new(lines).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(Span::styled(
                            format!(" {} ", s.help_title),
                            Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
                        ))
                        .border_style(Style::default().fg(AMBER))
                        .style(Style::default().bg(BG_CARD)),
                ),
                popup,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nxctl_core::liquidctl::StatusReading;
    use ratatui::backend::TestBackend;
    use std::collections::BTreeMap;

    fn state() -> TuiState {
        TuiState::new(
            Config::starter_nzxt(),
            None,
            false,
            PathBuf::from("/tmp/x.sock"),
        )
    }

    fn buffer_text(term: &Terminal<TestBackend>) -> String {
        term.backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect()
    }

    fn snap_full(liquid: f64, cpu: f32, gpu: f32) -> TuiSnapshot {
        let devices = vec![DeviceStatus {
            address: None,
            bus: None,
            description: "NZXT Kraken X53".into(),
            status: vec![
                StatusReading {
                    key: "Liquid temperature".into(),
                    value: serde_json::json!(liquid),
                    unit: Some("°C".into()),
                },
                StatusReading {
                    key: "Pump speed".into(),
                    value: serde_json::json!(2100.0),
                    unit: Some("rpm".into()),
                },
                StatusReading {
                    key: "Pump duty".into(),
                    value: serde_json::json!(60.0),
                    unit: Some("%".into()),
                },
                StatusReading {
                    key: "Fan 1 speed".into(),
                    value: serde_json::json!(950.0),
                    unit: Some("rpm".into()),
                },
                StatusReading {
                    key: "Fan 1 duty".into(),
                    value: serde_json::json!(45.0),
                    unit: Some("%".into()),
                },
            ],
        }];
        let channels = extract_channels(&devices);
        let mut colors = BTreeMap::new();
        colors.insert("color0".into(), "090a0f".into());
        colors.insert("color1".into(), "ef4444".into());
        colors.insert("color2".into(), "10b981".into());
        colors.insert("color4".into(), "00f0ff".into());
        let mut lighting_info = HashMap::new();
        lighting_info.insert(
            "NZXT Kraken X53".into(),
            DeviceLightingInfo {
                description: "NZXT Kraken X53".into(),
                channels: vec!["sync".into(), "ring".into(), "logo".into()],
                modes: vec!["fixed".into(), "breathing".into(), "spectrum-wave".into()],
            },
        );
        TuiSnapshot {
            devices,
            hwmon: vec![
                HwmonSensorInfo {
                    device_dir: "hwmon0".into(),
                    name: "k10temp".into(),
                    input_file: "temp1_input".into(),
                    label: Some("Tctl".into()),
                    temp_c: cpu,
                },
                HwmonSensorInfo {
                    device_dir: "hwmon3".into(),
                    name: "nvme".into(),
                    input_file: "temp1_input".into(),
                    label: Some("Composite".into()),
                    temp_c: 41.5,
                },
            ],
            hwmon_fans: vec![],
            nvidia: vec![NvidiaGpuInfo {
                index: 0,
                name: "RTX 4080".into(),
                temp_c: gpu,
            }],
            daemon: Some(DaemonStatus {
                active_profile: "silent".into(),
                config_path: None,
                poll_interval_secs: 2,
                hysteresis_celsius: 2.0,
                min_duty_delta: 3,
            }),
            channels,
            lighting_info,
            pywal: Some(PywalPalette {
                wallpaper: Some("/home/user/wall.png".into()),
                colors,
            }),
            error: None,
        }
    }

    #[test]
    fn obsidian_palette_constants_match_theme() {
        assert_eq!(BG_CANVAS, Color::Rgb(0x09, 0x0A, 0x0F));
        assert_eq!(BG_SIDEBAR, Color::Rgb(0x0C, 0x0E, 0x16));
        assert_eq!(BG_CARD, Color::Rgb(0x12, 0x14, 0x1C));
        assert_eq!(BG_CONTROL, Color::Rgb(0x18, 0x1B, 0x26));
        assert_eq!(BORDER_MUTED, Color::Rgb(0x23, 0x29, 0x36));
        assert_eq!(BORDER_ACTIVE, Color::Rgb(0x2F, 0x37, 0x4A));
        assert_eq!(CYAN, Color::Rgb(0x00, 0xF0, 0xFF));
        assert_eq!(MAGENTA, Color::Rgb(0xD9, 0x46, 0xEF));
        assert_eq!(EMERALD, Color::Rgb(0x10, 0xB9, 0x81));
        assert_eq!(AMBER, Color::Rgb(0xF5, 0x9E, 0x0B));
        assert_eq!(CRIMSON, Color::Rgb(0xEF, 0x44, 0x44));
        assert_eq!(TEXT_PRIMARY, Color::Rgb(0xF1, 0xF5, 0xF9));
        assert_eq!(TEXT_SECONDARY, Color::Rgb(0x94, 0xA3, 0xB8));
        assert_eq!(TEXT_MUTED, Color::Rgb(0x64, 0x74, 0x8B));
    }

    #[test]
    fn profile_navigation_wraps() {
        let mut s = state();
        let n = s.profile_names.len();
        assert!(n > 1);
        s.selected_profile = 0;
        s.prev_profile();
        assert_eq!(s.selected_profile, n - 1);
        s.next_profile();
        assert_eq!(s.selected_profile, 0);
        s.next_profile();
        assert_eq!(s.selected_profile, 1);
    }

    #[test]
    fn navigation_empty_is_noop() {
        let mut s = state();
        s.profile_names.clear();
        s.selected_profile = 0;
        s.next_profile();
        s.prev_profile();
        assert_eq!(s.selected_profile, 0);
    }

    #[test]
    fn history_tracks_and_caps_at_60() {
        assert_eq!(HISTORY_LEN, 60);
        let mut s = state();
        for i in 0..75 {
            s.push_snapshot(snap_full(30.2 + f64::from(i) * 0.001, 60.6, 45.0));
        }
        assert_eq!(s.liquid_history.len(), HISTORY_LEN);
        assert_eq!(s.cpu_history.back(), Some(&61));
        assert_eq!(s.gpu_history.back(), Some(&45));
        assert_eq!(s.liquid_history.front(), Some(&30));
        assert_eq!(
            s.hwmon_history
                .get("hwmon:hwmon3:temp1_input")
                .map(VecDeque::len),
            Some(HISTORY_LEN)
        );
    }

    #[test]
    fn view_shortcuts_and_help_toggle() {
        let mut s = state();
        let k = |c| KeyEvent::new(c, KeyModifiers::NONE);

        assert_eq!(s.view, TuiView::Dashboard);
        map_key(&mut s, k(KeyCode::Char('2')));
        assert_eq!(s.view, TuiView::Fans);
        map_key(&mut s, k(KeyCode::Char('3')));
        assert_eq!(s.view, TuiView::Rgb);
        map_key(&mut s, k(KeyCode::Char('4')));
        assert_eq!(s.view, TuiView::Profiles);
        map_key(&mut s, k(KeyCode::Char('1')));
        assert_eq!(s.view, TuiView::Dashboard);
        map_key(&mut s, k(KeyCode::PageDown));
        assert_eq!(s.view, TuiView::Fans);
        map_key(&mut s, k(KeyCode::PageUp));
        assert_eq!(s.view, TuiView::Dashboard);

        map_key(&mut s, k(KeyCode::Char('?')));
        assert!(s.show_help);
        assert_eq!(map_key(&mut s, k(KeyCode::Esc)), KeyAction::None);
        assert!(!s.show_help);
        assert_eq!(map_key(&mut s, k(KeyCode::Esc)), KeyAction::Quit);
        assert_eq!(map_key(&mut s, k(KeyCode::Char('q'))), KeyAction::Quit);
        assert_eq!(
            map_key(
                &mut s,
                KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
            ),
            KeyAction::Quit
        );
    }

    #[test]
    fn dashboard_promotes_and_demotes_sensors() {
        let mut s = state();
        s.push_snapshot(snap_full(31.0, 62.0, 50.0));
        let k = |c| KeyEvent::new(c, KeyModifiers::NONE);

        assert_eq!(s.dashboard_focus, DashboardFocus::Featured);
        assert!(is_featured(&s.featured_sensors, "cpu"));

        // Demote CPU
        map_key(&mut s, k(KeyCode::Char('f')));
        assert!(!is_featured(&s.featured_sensors, "cpu"));
        assert_eq!(
            s.status_message.as_deref(),
            Some("Sensor 'cpu' movido al listado")
        );

        // Switch focus to OtherSensors and promote CPU back
        map_key(&mut s, k(KeyCode::Tab));
        assert_eq!(s.dashboard_focus, DashboardFocus::OtherSensors);
        map_key(&mut s, k(KeyCode::Enter));
        assert!(is_featured(&s.featured_sensors, "cpu"));
        assert_eq!(s.status_message.as_deref(), Some("Sensor 'cpu' destacado"));
    }

    #[test]
    fn fans_controls_adjust_duty_curve_and_apply() {
        let mut s = state();
        s.push_snapshot(snap_full(34.0, 55.0, 48.0));
        s.view = TuiView::Fans;
        let k = |c| KeyEvent::new(c, KeyModifiers::NONE);

        // Cycle curve for device
        map_key(&mut s, k(KeyCode::Char('c')));
        let dev_curve = s.device_curves.get("NZXT Kraken X53").cloned().unwrap();
        assert!(!dev_curve.is_empty());

        // Adjust duty by +5% and -5%
        map_key(&mut s, k(KeyCode::Right));
        let ch = &s.ordered_fan_channels()[0];
        let key = (ch.device.clone(), ch.channel.clone());
        let after_inc = s.manual_duty[&key];
        map_key(&mut s, k(KeyCode::Char('-')));
        assert!((s.manual_duty[&key] - (after_inc - 5.0)).abs() < f32::EPSILON);

        let action = map_key(&mut s, k(KeyCode::Enter));
        assert!(matches!(action, KeyAction::ApplyFanSpeed { .. }));
    }

    #[test]
    fn rgb_controls_and_profiles_controls_work() {
        let mut s = state();
        s.push_snapshot(snap_full(31.0, 55.0, 48.0));
        let k = |c| KeyEvent::new(c, KeyModifiers::NONE);

        // RGB Pywal focus
        s.view = TuiView::Rgb;
        assert_eq!(s.rgb_focus, RgbFocus::Pywal);
        let initial_enabled = s.config.pywal.enabled;
        assert!(matches!(
            map_key(&mut s, k(KeyCode::Char('t'))),
            KeyAction::SaveConfig(_)
        ));
        assert_eq!(s.config.pywal.enabled, !initial_enabled);

        map_key(&mut s, k(KeyCode::Right));
        assert_eq!(s.selected_swatch, 1);
        assert!(matches!(
            map_key(&mut s, k(KeyCode::Enter)),
            KeyAction::ApplyAllColor { .. }
        ));

        // RGB Channels focus
        map_key(&mut s, k(KeyCode::Tab));
        assert_eq!(s.rgb_focus, RgbFocus::Channels);
        map_key(&mut s, k(KeyCode::Char('m')));
        map_key(&mut s, k(KeyCode::Right));
        assert!(matches!(
            map_key(&mut s, k(KeyCode::Char('a'))),
            KeyAction::ApplyChannelColor { .. }
        ));

        // Profiles view: create, clone, default, export, cannot delete builtin, can delete custom
        s.view = TuiView::Profiles;
        let orig_count = s.config.profiles.len();
        assert!(matches!(
            map_key(&mut s, k(KeyCode::Char('n'))),
            KeyAction::SaveConfig(_)
        ));
        assert_eq!(s.config.profiles.len(), orig_count + 1);
        assert!(matches!(
            map_key(&mut s, k(KeyCode::Char('c'))),
            KeyAction::SaveConfig(_)
        ));
        assert_eq!(s.config.profiles.len(), orig_count + 2);
        assert!(matches!(
            map_key(&mut s, k(KeyCode::Char('w'))),
            KeyAction::ExportProfile(_)
        ));
        assert!(matches!(
            map_key(&mut s, k(KeyCode::Char('d'))),
            KeyAction::SaveConfig(_)
        ));
        // Clean up created custom profile (reset default first so it's deletable)
        s.config.default_profile = "default".into();
        let custom_idx = s
            .profile_names
            .iter()
            .position(|n| n.starts_with("custom"))
            .unwrap();
        s.selected_profile = custom_idx;
        assert!(matches!(
            map_key(&mut s, k(KeyCode::Delete)),
            KeyAction::SaveConfig(_)
        ));
        assert_eq!(s.config.profiles.len(), orig_count + 1);

        // Try deleting builtin default
        s.selected_profile = s.profile_names.iter().position(|n| n == "default").unwrap();
        assert_eq!(map_key(&mut s, k(KeyCode::Delete)), KeyAction::None);
        assert!(!s.status_ok);

        // Auto-generate config preserves language and close_to_tray
        s.config.close_to_tray = true;
        s.lang = Language::En;
        assert!(matches!(
            map_key(&mut s, k(KeyCode::Char('g'))),
            KeyAction::SaveConfig(_)
        ));
        assert_eq!(s.config.language, Language::En);
        assert!(s.config.close_to_tray);
        assert!(s.status_ok);
    }

    #[test]
    fn global_shortcuts_and_pywal_controls_work() {
        let mut s = state();
        s.push_snapshot(snap_full(31.0, 55.0, 48.0));
        let k = |c| KeyEvent::new(c, KeyModifiers::NONE);

        assert!(matches!(
            map_key(&mut s, k(KeyCode::Char('['))),
            KeyAction::ApplyProfile(_)
        ));
        assert!(matches!(
            map_key(&mut s, k(KeyCode::Char(']'))),
            KeyAction::ApplyProfile(_)
        ));
        assert_eq!(
            map_key(&mut s, k(KeyCode::Char('p'))),
            KeyAction::ReapplyProfile
        );
        assert_eq!(map_key(&mut s, k(KeyCode::Char('r'))), KeyAction::Refresh);

        s.view = TuiView::Rgb;
        assert!(matches!(
            map_key(&mut s, k(KeyCode::Char('m'))),
            KeyAction::SaveConfig(_)
        ));
        assert_eq!(s.config.pywal.mode, "breathing");
        let prev_filter = s.config.pywal.filter_extremes;
        assert!(matches!(
            map_key(&mut s, k(KeyCode::Char('x'))),
            KeyAction::SaveConfig(_)
        ));
        assert_eq!(s.config.pywal.filter_extremes, !prev_filter);
        assert!(matches!(
            map_key(&mut s, k(KeyCode::Char('+'))),
            KeyAction::SaveConfig(_)
        ));
        assert_eq!(s.config.pywal.palette_offset, 1);
        assert!(matches!(
            map_key(&mut s, k(KeyCode::Char('-'))),
            KeyAction::SaveConfig(_)
        ));
        assert_eq!(s.config.pywal.palette_offset, 0);
        assert_eq!(map_key(&mut s, k(KeyCode::Char('s'))), KeyAction::SyncPywal);
    }

    #[test]
    fn export_and_save_config_work_in_temp_dir() {
        let dir = std::env::temp_dir().join(format!("nxctl_tui_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cfg_path = dir.join("config.toml");

        let mut s = TuiState::new(
            Config::starter_nzxt(),
            Some(cfg_path.clone()),
            true,
            dir.join("missing.sock"),
        );
        save_and_reload_config(&mut s, "saved".into());
        assert!(s.status_ok);
        assert!(cfg_path.exists());

        export_profile_to_disk(&mut s, "default");
        assert!(s.status_ok);
        assert!(dir.join("profiles").join("default.toml").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn language_key_toggles_state_and_status_message() {
        let mut s = state();
        assert_eq!(s.lang, Language::Es);
        let action = map_key(
            &mut s,
            KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE),
        );
        assert_eq!(action, KeyAction::SaveLanguage);
        assert_eq!(s.lang, Language::En);
        assert_eq!(s.config.language, Language::En);
        assert_eq!(s.status_message.as_deref(), Some("language: English"));

        let action = map_key(
            &mut s,
            KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE),
        );
        assert_eq!(action, KeyAction::SaveLanguage);
        assert_eq!(s.lang, Language::Es);
        assert_eq!(s.config.language, Language::Es);
        assert_eq!(s.status_message.as_deref(), Some("idioma: Español"));
    }

    #[test]
    fn draw_renders_all_four_views_es_and_en_and_small_terminal() {
        for lang in [Language::Es, Language::En] {
            let mut cfg = Config::starter_nzxt();
            cfg.language = lang;
            let mut s = TuiState::new(cfg, None, true, PathBuf::from("/tmp/x.sock"));
            s.push_snapshot(snap_full(32.5, 65.0, 52.0));
            let tr = i18n::tui(lang);

            let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();

            // 1. Dashboard
            s.view = TuiView::Dashboard;
            term.draw(|f| draw(f, &mut s)).unwrap();
            let text = buffer_text(&term);
            assert!(text.contains("NXCTL"));
            assert!(text.contains(tr.sys_ok));
            assert!(text.contains(&tr.pwm_headers.to_uppercase()));
            assert!(text.contains(&tr.other_sensors.to_uppercase()));
            assert!(text.contains("nvme"));

            // 2. Fans
            s.view = TuiView::Fans;
            term.draw(|f| draw(f, &mut s)).unwrap();
            let text = buffer_text(&term);
            assert!(text.contains("NZXT Kraken X53"));
            assert!(text.contains(tr.manual_duty_label));

            // 3. RGB
            s.view = TuiView::Rgb;
            term.draw(|f| draw(f, &mut s)).unwrap();
            let text = buffer_text(&term);
            assert!(text.contains("PYWAL SYNC"));
            assert!(text.contains("wall.png"));
            assert!(text.contains(tr.sync_whole_device));

            // 4. Profiles
            s.view = TuiView::Profiles;
            term.draw(|f| draw(f, &mut s)).unwrap();
            let text = buffer_text(&term);
            assert!(text.contains(tr.badge_active));
            assert!(text.contains(tr.badge_default));

            // Help modal
            s.show_help = true;
            term.draw(|f| draw(f, &mut s)).unwrap();
            let help_text = buffer_text(&term);
            assert!(help_text.contains(tr.help_title));
        }

        // Small terminal 30x10 must never panic on any view or help modal
        let mut small_term = Terminal::new(TestBackend::new(30, 10)).unwrap();
        let mut s = state();
        s.push_snapshot(snap_full(32.0, 55.0, 44.0));
        for v in TuiView::ALL {
            s.view = v;
            s.show_help = false;
            small_term.draw(|f| draw(f, &mut s)).unwrap();
            s.show_help = true;
            small_term.draw(|f| draw(f, &mut s)).unwrap();
        }
        // Empty snapshot on 30x10
        let mut empty = state();
        for v in TuiView::ALL {
            empty.view = v;
            small_term.draw(|f| draw(f, &mut empty)).unwrap();
        }
    }
}
