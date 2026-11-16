//! `nxctl-gui`: native desktop dashboard with the "Obsidian Telemetry" look.

mod poller;
mod theme;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use anyhow::anyhow;
use clap::Parser;
use eframe::egui::{self, Color32, RichText};
use egui_plot::{HLine, Line, Plot, PlotPoints, Points, VLine};
use nxctl_core::liquidctl::{CliLiquidctlBackend, DeviceStatus, LiquidctlBackend};
use nxctl_core::{
    Config, FanCurve, IpcRequest, IpcResponse, apply_profile, default_socket_path, evaluate_curve,
    send_request,
};

use poller::{HISTORY_LEN, Poller, Snapshot};
use theme::*;

#[derive(Parser, Debug)]
#[command(name = "nxctl-gui", version, about = "NZXT cooling & RGB dashboard")]
struct Cli {
    /// Path to the nxctl configuration file.
    #[arg(long)]
    config: Option<PathBuf>,
    /// Path to the nxctld Unix socket.
    #[arg(long)]
    socket: Option<PathBuf>,
    /// Do not execute hardware-changing liquidctl commands.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Dashboard,
    Fans,
    Rgb,
    Profiles,
}

impl View {
    const ALL: [(View, &'static str); 4] = [
        (View::Dashboard, "Dashboard"),
        (View::Fans, "Ventiladores"),
        (View::Rgb, "Iluminación RGB"),
        (View::Profiles, "Perfiles"),
    ];
}

const RGB_MODES: [&str; 4] = ["fixed", "breathing", "fading", "spectrum-wave"];
const RGB_CHANNELS: [&str; 4] = ["sync", "led", "ring", "logo"];

/// Result of a background action.
struct ActionResult {
    message: String,
    ok: bool,
}

struct NxctlApp {
    config_path: Option<PathBuf>,
    config: Config,
    socket: PathBuf,
    dry_run: bool,
    backend: Arc<CliLiquidctlBackend>,
    poller: Poller,
    snap: Arc<Snapshot>,
    view: View,
    status_msg: String,
    status_ok: bool,
    action_tx: Sender<ActionResult>,
    action_rx: Receiver<ActionResult>,
    ctx: egui::Context,
    selected_curve: Option<String>,
    manual_duty: HashMap<(String, String), f32>,
    rgb_color: Color32,
    rgb_mode: usize,
    rgb_device: usize,
    rgb_channel: usize,
}

impl NxctlApp {
    fn new(cc: &eframe::CreationContext<'_>, cli: Cli) -> Self {
        apply_obsidian_theme(&cc.egui_ctx);
        let socket = cli.socket.unwrap_or_else(default_socket_path);
        let (config, status_msg, status_ok) = match Config::load(cli.config.as_deref()) {
            Ok(c) => (c, "Configuración cargada".to_string(), true),
            Err(e) => (
                Config::starter_nzxt(),
                format!("Config no disponible ({e}); usando valores iniciales"),
                false,
            ),
        };
        let (action_tx, action_rx) = mpsc::channel();
        Self {
            config_path: cli.config,
            config,
            poller: Poller::spawn(socket.clone(), Some(cc.egui_ctx.clone())),
            socket,
            dry_run: cli.dry_run,
            backend: Arc::new(CliLiquidctlBackend::new()),
            snap: Arc::new(Snapshot::default()),
            view: View::Dashboard,
            status_msg,
            status_ok,
            action_tx,
            action_rx,
            ctx: cc.egui_ctx.clone(),
            selected_curve: None,
            manual_duty: HashMap::new(),
            rgb_color: CYAN,
            rgb_mode: 0,
            rgb_device: 0,
            rgb_channel: 0,
        }
    }

    fn daemon_online(&self) -> bool {
        self.snap.daemon.is_some()
    }

    fn active_profile(&self) -> String {
        self.snap
            .daemon
            .as_ref()
            .map(|d| d.active_profile.clone())
            .unwrap_or_else(|| self.config.default_profile.clone())
    }

    /// Runs `f` on a worker thread and reports the outcome in the status line.
    fn spawn_action(&self, f: impl FnOnce() -> anyhow::Result<String> + Send + 'static) {
        let tx = self.action_tx.clone();
        let ctx = self.ctx.clone();
        thread::spawn(move || {
            let res = match f() {
                Ok(message) => ActionResult { message, ok: true },
                Err(e) => ActionResult {
                    message: e.to_string(),
                    ok: false,
                },
            };
            let _ = tx.send(res);
            ctx.request_repaint();
        });
    }

    fn do_reload(&mut self) {
        match Config::load(self.config_path.as_deref()) {
            Ok(c) => self.config = c,
            Err(e) => {
                self.status_msg = format!("Error recargando config: {e}");
                self.status_ok = false;
            }
        }
        if self.daemon_online() {
            let sock = self.socket.clone();
            self.spawn_action(move || ipc(&sock, &IpcRequest::Reload));
        } else {
            self.status_msg = "Configuración recargada (daemon offline)".into();
            self.status_ok = true;
        }
        self.poller.trigger();
    }

    fn do_reapply(&mut self) {
        if self.daemon_online() {
            let sock = self.socket.clone();
            self.spawn_action(move || ipc(&sock, &IpcRequest::Reapply));
        } else {
            self.apply_local(self.active_profile());
        }
    }

    fn apply_local(&self, name: String) {
        let backend = self.backend.clone();
        let config = self.config.clone();
        let dry = self.dry_run;
        self.spawn_action(move || {
            let r = apply_profile(backend.as_ref(), &config, &name, dry)?;
            Ok(format!(
                "Perfil '{name}' aplicado: {} luces, {} ventiladores, {} avisos",
                r.lighting_applied,
                r.cooling_applied,
                r.warnings.len()
            ))
        });
    }

    fn apply_profile_named(&self, name: &str) {
        if self.daemon_online() {
            let sock = self.socket.clone();
            let profile = name.to_string();
            self.spawn_action(move || ipc(&sock, &IpcRequest::SetProfile { profile }));
        } else {
            self.apply_local(name.to_string());
        }
    }

    fn drain_channels(&mut self) {
        while let Ok(s) = self.poller.rx.try_recv() {
            self.snap = s;
        }
        let mut refresh = false;
        while let Ok(r) = self.action_rx.try_recv() {
            self.status_msg = r.message;
            self.status_ok = r.ok;
            refresh = true;
        }
        if refresh {
            self.poller.trigger();
        }
    }

    // ---------------------------------------------------------------- panels

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.label(
            RichText::new("NXCTL // DAEMON")
                .color(CYAN)
                .strong()
                .size(15.0),
        );
        ui.add_space(6.0);
        let (txt, col) = if self.daemon_online() {
            ("SYS OK", EMERALD)
        } else {
            ("OFFLINE", CRIMSON)
        };
        badge(ui, txt, col);
        ui.add_space(18.0);
        for (v, label) in View::ALL {
            let selected = self.view == v;
            let text =
                RichText::new(label)
                    .size(14.0)
                    .color(if selected { CYAN } else { TEXT_SECONDARY });
            let btn = egui::Button::new(text)
                .fill(if selected {
                    BG_CONTROL
                } else {
                    Color32::TRANSPARENT
                })
                .stroke(egui::Stroke::new(
                    1.0,
                    if selected {
                        BORDER_ACTIVE
                    } else {
                        Color32::TRANSPARENT
                    },
                ))
                .min_size(egui::vec2(ui.available_width(), 32.0));
            if ui.add(btn).clicked() {
                self.view = v;
            }
        }
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.add_space(8.0);
            let backend = match &self.snap.backend_error {
                Some(_) => "liquidctl: error".to_string(),
                None => format!("liquidctl: {} disp.", self.snap.devices.len()),
            };
            small_muted(ui, &backend);
            small_muted(
                ui,
                &format!(
                    "IPC: {}",
                    self.socket.file_name().map_or_else(
                        || self.socket.display().to_string(),
                        |n| n.to_string_lossy().into_owned()
                    )
                ),
            );
            if self.dry_run {
                small_muted(ui, "MODO DRY-RUN");
            }
        });
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            ui.add_space(12.0);
            let fmt = |v: Option<f32>| v.map_or("--".to_string(), |t| format!("{t:.0}°C"));
            ui.label(
                RichText::new(format!(
                    "CPU {}   GPU {}   NZXT {} disp.",
                    fmt(self.snap.cpu_c),
                    fmt(self.snap.gpu_c),
                    self.snap.devices.len()
                ))
                .color(TEXT_SECONDARY)
                .monospace(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(12.0);
                if ui.button("Reaplicar").clicked() {
                    self.do_reapply();
                }
                if ui.button("Recargar").clicked() {
                    self.do_reload();
                }
                badge(ui, &self.active_profile(), CYAN);
            });
        });
    }

    // ----------------------------------------------------------------- views

    fn dashboard(&mut self, ui: &mut egui::Ui) {
        let snap = self.snap.clone();
        ui.columns(2, |cols| {
            let ui = &mut cols[0];
            temp_card(
                ui,
                "CPU",
                snap.cpu_c,
                100.0,
                "PACKAGE",
                CYAN,
                &snap.cpu_history,
            );
            ui.add_space(8.0);
            temp_card(
                ui,
                "GPU",
                snap.gpu_c,
                100.0,
                "NVIDIA",
                MAGENTA,
                &snap.gpu_history,
            );
            ui.add_space(8.0);
            liquid_card(ui, &snap);

            let ui = &mut cols[1];
            card_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                card_title(ui, "Headers PWM");
                if snap.channels.is_empty() {
                    small_muted(ui, "Sin canales NZXT detectados");
                }
                for c in &snap.channels {
                    let accent = if c.channel == "pump" { MAGENTA } else { CYAN };
                    draw_pwm_bar(ui, &c.label, c.rpm, c.duty.unwrap_or(0.0), accent);
                }
            });
            ui.add_space(8.0);
            card_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                card_title(ui, "Daemon Watchdog");
                match &snap.daemon {
                    Some(d) => {
                        kv(ui, "IPC", "CONECTADO");
                        kv(ui, "Histéresis", &format!("{:.1}°C", d.hysteresis_celsius));
                        kv(ui, "Intervalo", &format!("{} s", d.poll_interval_secs));
                        kv(ui, "Delta mínimo", &format!("{}%", d.min_duty_delta));
                    }
                    None => {
                        kv(ui, "IPC", "DESCONECTADO");
                        let d = &self.config.daemon;
                        kv(
                            ui,
                            "Histéresis",
                            &format!("{:.1}°C (config)", d.hysteresis_celsius),
                        );
                        kv(
                            ui,
                            "Intervalo",
                            &format!("{} s (config)", d.poll_interval_secs),
                        );
                        kv(
                            ui,
                            "Delta mínimo",
                            &format!("{}% (config)", d.min_duty_delta),
                        );
                    }
                }
                ui.add_space(4.0);
                ui.label(
                    RichText::new(&self.status_msg)
                        .size(12.0)
                        .color(if self.status_ok { EMERALD } else { AMBER }),
                );
            });
        });
    }

    fn fans(&mut self, ui: &mut egui::Ui) {
        let snap = self.snap.clone();
        let curve_names: Vec<String> = self.config.curves.keys().cloned().collect();
        if self.selected_curve.is_none() {
            self.selected_curve = curve_names.first().cloned();
        }
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            card_title(ui, "Curva de ventilación");
            egui::ComboBox::from_id_salt("curve_sel")
                .selected_text(self.selected_curve.clone().unwrap_or_else(|| "—".into()))
                .show_ui(ui, |ui| {
                    for n in &curve_names {
                        ui.selectable_value(&mut self.selected_curve, Some(n.clone()), n);
                    }
                });
            let curve: Option<&FanCurve> = self
                .selected_curve
                .as_ref()
                .and_then(|n| self.config.curves.get(n));
            if let Some(curve) = curve {
                let pts: Vec<[f64; 2]> = curve
                    .points
                    .iter()
                    .map(|p| [f64::from(p.temp_c), f64::from(p.duty)])
                    .collect();
                let live = snap.liquid_c.or(snap.cpu_c);
                Plot::new("curve_plot")
                    .height(240.0)
                    .allow_drag(false)
                    .allow_zoom(false)
                    .allow_scroll(false)
                    .include_y(0.0)
                    .include_y(100.0)
                    .show(ui, |p| {
                        p.line(
                            Line::new("Curva", PlotPoints::from(pts.clone()))
                                .color(CYAN)
                                .width(2.0),
                        );
                        p.points(
                            Points::new("Nodos", PlotPoints::from(pts))
                                .color(CYAN)
                                .radius(3.0),
                        );
                        if let Some(t) = live {
                            let duty = evaluate_curve(curve, t);
                            p.vline(VLine::new("Temp", f64::from(t)).color(TEXT_MUTED));
                            p.hline(HLine::new("Duty", f64::from(duty)).color(TEXT_MUTED));
                            p.points(
                                Points::new(
                                    "Operación",
                                    PlotPoints::from(vec![[f64::from(t), f64::from(duty)]]),
                                )
                                .color(AMBER)
                                .radius(6.0),
                            );
                        }
                    });
                if let Some(t) = live {
                    small_muted(
                        ui,
                        &format!("Punto operativo: {t:.1}°C → {}%", evaluate_curve(curve, t)),
                    );
                }
            }
        });
        ui.add_space(8.0);
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            card_title(ui, "Control manual");
            if snap.channels.is_empty() {
                small_muted(ui, "Sin canales detectados");
            }
            for c in &snap.channels {
                let key = (c.device.clone(), c.channel.clone());
                let entry = self
                    .manual_duty
                    .entry(key)
                    .or_insert_with(|| c.duty.unwrap_or(0.0));
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("{} · {}", c.device, c.label)).color(TEXT_SECONDARY),
                    );
                    let resp = ui.add(egui::Slider::new(entry, 0.0..=100.0).suffix("%"));
                    if resp.drag_stopped() || (resp.changed() && !resp.dragged()) {
                        let backend = self.backend.clone();
                        let (dev, ch, duty, dry) = (
                            c.device.clone(),
                            c.channel.clone(),
                            *entry as u8,
                            self.dry_run,
                        );
                        let tx = self.action_tx.clone();
                        let ctx = self.ctx.clone();
                        thread::spawn(move || {
                            let r = backend.set_speed(&dev, &ch, duty, dry);
                            let _ = tx.send(match r {
                                Ok(()) => ActionResult {
                                    message: format!("{dev}/{ch} → {duty}%"),
                                    ok: true,
                                },
                                Err(e) => ActionResult {
                                    message: e.to_string(),
                                    ok: false,
                                },
                            });
                            ctx.request_repaint();
                        });
                    }
                });
            }
        });
    }

    fn rgb(&mut self, ui: &mut egui::Ui) {
        let snap = self.snap.clone();
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            card_title(ui, "Iluminación RGB");
            let devices: Vec<String> = snap.devices.iter().map(|d| d.description.clone()).collect();
            self.rgb_device = self.rgb_device.min(devices.len().saturating_sub(1));
            let mut channels: Vec<String> = RGB_CHANNELS.iter().map(|s| (*s).to_string()).collect();
            for p in self.config.profiles.values() {
                for l in &p.lighting {
                    if !channels.contains(&l.channel) {
                        channels.push(l.channel.clone());
                    }
                }
            }
            self.rgb_channel = self.rgb_channel.min(channels.len() - 1);
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("rgb_dev")
                    .selected_text(devices.get(self.rgb_device).cloned().unwrap_or("—".into()))
                    .show_ui(ui, |ui| {
                        for (i, d) in devices.iter().enumerate() {
                            ui.selectable_value(&mut self.rgb_device, i, d);
                        }
                    });
                egui::ComboBox::from_id_salt("rgb_ch")
                    .selected_text(&channels[self.rgb_channel])
                    .show_ui(ui, |ui| {
                        for (i, c) in channels.iter().enumerate() {
                            ui.selectable_value(&mut self.rgb_channel, i, c);
                        }
                    });
                egui::ComboBox::from_id_salt("rgb_mode")
                    .selected_text(RGB_MODES[self.rgb_mode])
                    .show_ui(ui, |ui| {
                        for (i, m) in RGB_MODES.iter().enumerate() {
                            ui.selectable_value(&mut self.rgb_mode, i, *m);
                        }
                    });
            });
            ui.add_space(8.0);
            egui::color_picker::color_picker_color32(
                ui,
                &mut self.rgb_color,
                egui::color_picker::Alpha::Opaque,
            );
            ui.add_space(8.0);
            let enabled = !devices.is_empty();
            if ui
                .add_enabled(enabled, egui::Button::new("Aplicar color"))
                .clicked()
            {
                let c = self.rgb_color;
                let hex = format!("{:02x}{:02x}{:02x}", c.r(), c.g(), c.b());
                let mode = RGB_MODES[self.rgb_mode];
                let colors = if mode == "spectrum-wave" {
                    vec![]
                } else {
                    vec![hex]
                };
                let backend = self.backend.clone();
                let (dev, ch, mode, dry) = (
                    devices[self.rgb_device].clone(),
                    channels[self.rgb_channel].clone(),
                    mode.to_string(),
                    self.dry_run,
                );
                self.spawn_action(move || {
                    backend.set_color(&dev, &ch, &mode, &colors, dry)?;
                    Ok(format!("{dev}/{ch}: {mode} aplicado"))
                });
            }
        });
    }

    fn profiles(&mut self, ui: &mut egui::Ui) {
        let active = self.active_profile();
        let mut to_apply = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (name, p) in &self.config.profiles {
                card_frame().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        card_title(ui, name);
                        if *name == active {
                            badge(ui, "ACTIVO", EMERALD);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Aplicar").clicked() {
                                to_apply = Some(name.clone());
                            }
                        });
                    });
                    if let Some(d) = &p.description {
                        small_muted(ui, d);
                    }
                    small_muted(
                        ui,
                        &format!(
                            "{} reglas de iluminación · {} reglas de refrigeración",
                            p.lighting.len(),
                            p.cooling.len()
                        ),
                    );
                });
                ui.add_space(8.0);
            }
        });
        if let Some(n) = to_apply {
            self.apply_profile_named(&n);
        }
    }
}

impl eframe::App for NxctlApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.drain_channels();
        egui::Panel::left("sidebar")
            .exact_size(210.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(BG_SIDEBAR)
                    .stroke(egui::Stroke::new(1.0, BORDER_MUTED))
                    .inner_margin(egui::Margin::same(12)),
            )
            .show(ui, |ui| self.sidebar(ui));
        egui::Panel::top("topbar")
            .exact_size(44.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(BG_SIDEBAR)
                    .stroke(egui::Stroke::new(1.0, BORDER_MUTED)),
            )
            .show(ui, |ui| self.top_bar(ui));
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(BG_CANVAS)
                    .inner_margin(egui::Margin::same(16)),
            )
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| match self.view {
                        View::Dashboard => self.dashboard(ui),
                        View::Fans => self.fans(ui),
                        View::Rgb => self.rgb(ui),
                        View::Profiles => self.profiles(ui),
                    });
            });
    }
}

// --------------------------------------------------------------- UI helpers

fn ipc(socket: &std::path::Path, req: &IpcRequest) -> anyhow::Result<String> {
    match send_request(socket, req)? {
        IpcResponse::Ok { message } => Ok(message),
        IpcResponse::Status { daemon } => Ok(format!("perfil activo: {}", daemon.active_profile)),
        IpcResponse::Error { message } => Err(anyhow!(message)),
    }
}

fn badge(ui: &mut egui::Ui, text: &str, color: Color32) {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.15))
        .stroke(egui::Stroke::new(1.0, color))
        .corner_radius(egui::CornerRadius::same(4))
        .inner_margin(egui::Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(text).color(color).size(11.0).strong());
        });
}

fn card_title(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text.to_uppercase())
            .color(TEXT_SECONDARY)
            .size(12.0)
            .strong(),
    );
    ui.add_space(4.0);
}

fn small_muted(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).color(TEXT_MUTED).size(11.0));
}

fn kv(ui: &mut egui::Ui, k: &str, v: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(k).color(TEXT_SECONDARY));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(v).color(TEXT_PRIMARY).monospace());
        });
    });
}

fn temp_card(
    ui: &mut egui::Ui,
    title: &str,
    value: Option<f32>,
    max: f32,
    sub: &str,
    accent: Color32,
    history: &std::collections::VecDeque<f64>,
) {
    card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        card_title(ui, title);
        ui.horizontal(|ui| {
            draw_radial_gauge(ui, value, max, sub, accent);
            let pts: Vec<[f64; 2]> = history
                .iter()
                .enumerate()
                .map(|(i, v)| [i as f64, *v])
                .collect();
            Plot::new(format!("{title}_hist"))
                .height(132.0)
                .allow_drag(false)
                .allow_zoom(false)
                .allow_scroll(false)
                .include_x(0.0)
                .include_x((HISTORY_LEN - 1) as f64)
                .include_y(20.0)
                .include_y(100.0)
                .show(ui, |p| {
                    p.line(
                        Line::new(title, PlotPoints::from(pts))
                            .color(accent)
                            .width(2.0),
                    );
                });
        });
    });
}

/// Ambient-temperature proxy: the coolest DIMM SPD sensor (`spd5118`), which tracks case air.
fn ambient_proxy(sensors: &[nxctl_core::HwmonSensorInfo]) -> Option<f32> {
    sensors
        .iter()
        .filter(|s| s.name.eq_ignore_ascii_case("spd5118"))
        .map(|s| s.temp_c)
        .min_by(|a, b| a.total_cmp(b))
}

/// Formats an integer with `,` thousands separators (e.g. `2450` -> `2,450`).
fn fmt_thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

#[derive(Clone, Copy)]
enum TileIcon {
    Pump,
    Thermometer,
    Delta,
    Gauge,
    Flow,
}

/// Square icon box with a hand-painted glyph (no icon font dependency).
fn icon_box(ui: &mut egui::Ui, size: f32, icon: TileIcon, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, egui::CornerRadius::same(4), BG_CONTROL);
    p.rect_stroke(
        rect,
        egui::CornerRadius::same(4),
        egui::Stroke::new(1.0, BORDER_MUTED),
        egui::StrokeKind::Inside,
    );
    let c = rect.center();
    let s = egui::Stroke::new(1.8, color);
    let u = size / 36.0;
    match icon {
        TileIcon::Pump => {
            p.circle_stroke(c, 8.0 * u, s);
            p.circle_filled(c, 2.5 * u, color);
            for k in 0..3 {
                let a = k as f32 * std::f32::consts::TAU / 3.0;
                p.line_segment(
                    [c, c + egui::vec2(a.cos(), a.sin()) * 8.0 * u],
                    egui::Stroke::new(1.4, color),
                );
            }
        }
        TileIcon::Thermometer => {
            p.line_segment(
                [c + egui::vec2(0.0, -9.0 * u), c + egui::vec2(0.0, 3.0 * u)],
                egui::Stroke::new(3.0, color),
            );
            p.circle_filled(c + egui::vec2(0.0, 7.0 * u), 5.0 * u, color);
            p.circle_stroke(c + egui::vec2(0.0, -9.0 * u), 1.5 * u, s);
        }
        TileIcon::Delta => {
            let pts = vec![
                c + egui::vec2(0.0, -8.0 * u),
                c + egui::vec2(8.0 * u, 7.0 * u),
                c + egui::vec2(-8.0 * u, 7.0 * u),
            ];
            p.add(egui::Shape::closed_line(pts, s));
        }
        TileIcon::Gauge => {
            let pts: Vec<egui::Pos2> = (0..=16)
                .map(|i| {
                    let a = std::f32::consts::PI * (1.0 + i as f32 / 16.0);
                    c + egui::vec2(a.cos(), a.sin()) * 10.0 * u + egui::vec2(0.0, 4.0 * u)
                })
                .collect();
            p.add(egui::Shape::line(pts, s));
            p.line_segment(
                [
                    c + egui::vec2(0.0, 4.0 * u),
                    c + egui::vec2(6.0 * u, -4.0 * u),
                ],
                egui::Stroke::new(2.0, color),
            );
        }
        TileIcon::Flow => {
            for row in 0..3 {
                let y0 = (row as f32 - 1.0) * 6.0 * u;
                let pts: Vec<egui::Pos2> = (0..=12)
                    .map(|i| {
                        let x = -10.0 + i as f32 * (20.0 / 12.0);
                        c + egui::vec2(x * u, y0 + (x * 0.6).sin() * 2.2 * u)
                    })
                    .collect();
                p.add(egui::Shape::line(pts, egui::Stroke::new(1.5, color)));
            }
        }
    }
}

/// One stat tile of the liquid-circuit card: icon box + label + big mono value + unit.
fn stat_tile(
    ui: &mut egui::Ui,
    icon: TileIcon,
    label: &str,
    value: &str,
    unit: &str,
    color: Color32,
) {
    egui::Frame::new()
        .fill(BG_SIDEBAR)
        .stroke(egui::Stroke::new(1.0, BORDER_MUTED))
        .corner_radius(egui::CornerRadius::same(4))
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                icon_box(ui, 36.0, icon, color);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.label(
                        RichText::new(label.to_uppercase())
                            .size(10.0)
                            .color(TEXT_MUTED),
                    );
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 3.0;
                        ui.label(
                            RichText::new(value)
                                .size(22.0)
                                .monospace()
                                .strong()
                                .color(color),
                        );
                        ui.label(RichText::new(unit).size(11.0).color(TEXT_SECONDARY));
                    });
                });
            });
        });
}

/// Detects the primary liquid cooler/AIO device description across brands.
fn detect_aio_description(devices: &[DeviceStatus]) -> String {
    devices
        .iter()
        .find(|d| {
            let desc = d.description.to_ascii_lowercase();
            desc.contains("kraken")
                || desc.contains("corsair")
                || desc.contains("hydro")
                || desc.contains("clc")
                || desc.contains("aquacomputer")
                || desc.contains("aio")
                || d.status.iter().any(|r| {
                    let k = r.key.to_ascii_lowercase();
                    k.contains("liquid")
                        || k.contains("coolant")
                        || k.contains("water")
                        || k.contains("pump")
                })
        })
        .map_or_else(|| "AIO (sin detectar)".to_string(), |d| d.description.clone())
}

/// Finds a flow sensor reading (value and unit) if exposed by the device.
fn liquid_flow(devices: &[DeviceStatus]) -> Option<(f64, String)> {
    devices.iter().flat_map(|d| &d.status).find_map(|r| {
        if r.key.to_ascii_lowercase().contains("flow")
            && let Some(v) = r.value.as_f64()
        {
            let unit = r
                .unit
                .as_deref()
                .filter(|u| !u.trim().is_empty())
                .unwrap_or("L/min")
                .to_string();
            return Some((v, unit));
        }
        None
    })
}

/// "Circuito refrigerante líquido" card mirroring the reference design.
fn liquid_card(ui: &mut egui::Ui, snap: &Snapshot) {
    let pump = snap.channels.iter().find(|c| c.channel.contains("pump"));
    let desc = detect_aio_description(&snap.devices);
    let flow = liquid_flow(&snap.devices);
    let delta = snap
        .liquid_c
        .zip(ambient_proxy(&snap.hwmon))
        .map(|(liq, amb)| liq - amb);
    let delta_color = match delta {
        Some(d) if d >= 15.0 => CRIMSON,
        Some(d) if d >= 10.0 => AMBER,
        _ => EMERALD,
    };

    card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            icon_box(ui, 32.0, TileIcon::Pump, CYAN);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.label(
                    RichText::new("CIRCUITO REFRIGERANTE LÍQUIDO (AIO)")
                        .size(15.0)
                        .strong()
                        .color(TEXT_PRIMARY),
                );
                ui.label(
                    RichText::new(format!("{desc} · Sensor de líquido integrado"))
                        .size(11.0)
                        .monospace()
                        .color(TEXT_MUTED),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let txt = pump
                    .and_then(|p| p.duty)
                    .map_or("Bomba: --".to_string(), |d| format!("Bomba: {d:.0}% PWM"));
                egui::Frame::new()
                    .fill(BG_CONTROL)
                    .stroke(egui::Stroke::new(1.0, BORDER_MUTED))
                    .corner_radius(egui::CornerRadius::same(2))
                    .inner_margin(egui::Margin::symmetric(8, 3))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(txt)
                                .size(11.0)
                                .monospace()
                                .color(TEXT_PRIMARY),
                        );
                    });
            });
        });
        ui.add_space(10.0);
        ui.columns(4, |cols| {
            stat_tile(
                &mut cols[0],
                TileIcon::Thermometer,
                "Liquid temp",
                &snap.liquid_c.map_or("--".into(), |t| format!("{t:.1}")),
                "°C",
                CYAN,
            );
            stat_tile(
                &mut cols[1],
                TileIcon::Delta,
                "Delta ambiente",
                &delta.map_or("--".into(), |d| format!("{d:+.1}")),
                "°C",
                delta_color,
            );
            stat_tile(
                &mut cols[2],
                TileIcon::Gauge,
                "Velocidad",
                &pump
                    .and_then(|p| p.rpm)
                    .map_or("--".into(), |r| fmt_thousands(r.round() as u64)),
                "RPM",
                CYAN,
            );
            let (flow_val, flow_unit, flow_color) = match flow {
                Some((v, u)) => (format!("{v:.1}"), u, CYAN),
                None => ("N/D".to_string(), "L/min".to_string(), TEXT_MUTED),
            };
            stat_tile(
                &mut cols[3],
                TileIcon::Flow,
                "Caudal",
                &flow_val,
                &flow_unit,
                flow_color,
            );
        });
    });
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let cli = Cli::parse();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("NXCTL // DAEMON")
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([900.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "nxctl-gui",
        options,
        Box::new(move |cc| Ok(Box::new(NxctlApp::new(cc, cli)))),
    )
    .map_err(|e| anyhow!("{e}"))
}

#[cfg(test)]
mod liquid_tests {
    use super::*;
    use nxctl_core::HwmonSensorInfo;

    fn sensor(name: &str, t: f32) -> HwmonSensorInfo {
        HwmonSensorInfo {
            device_dir: "hwmon9".into(),
            name: name.into(),
            input_file: "temp1_input".into(),
            label: None,
            temp_c: t,
        }
    }

    #[test]
    fn thousands_separator() {
        assert_eq!(fmt_thousands(0), "0");
        assert_eq!(fmt_thousands(986), "986");
        assert_eq!(fmt_thousands(2450), "2,450");
        assert_eq!(fmt_thousands(1234567), "1,234,567");
    }

    #[test]
    fn ambient_proxy_uses_coolest_spd_sensor() {
        let s = [
            sensor("k10temp", 40.0),
            sensor("spd5118", 35.5),
            sensor("spd5118", 33.5),
        ];
        assert_eq!(ambient_proxy(&s), Some(33.5));
        assert_eq!(ambient_proxy(&[sensor("nvme", 40.0)]), None);
    }

    #[test]
    fn detects_generic_aio_and_flow() {
        use nxctl_core::StatusReading;

        assert_eq!(detect_aio_description(&[]), "AIO (sin detectar)");
        assert_eq!(liquid_flow(&[]), None);

        let aqua = DeviceStatus {
            address: None,
            bus: Some("hid".into()),
            description: "Aquacomputer D5 Next".into(),
            status: vec![
                StatusReading {
                    key: "Coolant temperature".into(),
                    value: serde_json::json!(30.4),
                    unit: Some("°C".into()),
                },
                StatusReading {
                    key: "Flow rate".into(),
                    value: serde_json::json!(120.5),
                    unit: Some("L/h".into()),
                },
            ],
        };
        assert_eq!(
            detect_aio_description(std::slice::from_ref(&aqua)),
            "Aquacomputer D5 Next"
        );
        assert_eq!(
            liquid_flow(std::slice::from_ref(&aqua)),
            Some((120.5, "L/h".to_string()))
        );
    }
}
