//! `nxctl-gui`: native desktop dashboard with the "Obsidian Telemetry" look.

mod i18n;
mod poller;
mod theme;

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use anyhow::anyhow;
use clap::Parser;
use eframe::egui::{self, Color32, RichText};
use egui_plot::{HLine, Line, Plot, PlotPoints, Points, VLine};
use nxctl_core::liquidctl::{CliLiquidctlBackend, LiquidctlBackend};
use nxctl_core::{
    Config, CoolingRule, CoolingTarget, FanCurve, IpcRequest, IpcResponse, Language, LightingRule,
    Profile, active_palette_entries, apply_profile, apply_pywal_palette, default_socket_path,
    evaluate_curve, load_pywal_palette, send_request,
};

use poller::{ChannelInfo, HISTORY_LEN, Poller, Snapshot, hwmon_sensor_id};
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
    /// Idioma de la interfaz (es, en).
    #[arg(long)]
    lang: Option<Language>,
    /// Vista inicial (dashboard, fans, rgb, profiles).
    #[arg(long)]
    view: Option<View>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum View {
    Dashboard,
    Fans,
    Rgb,
    Profiles,
}

impl View {
    const ALL: [View; 4] = [View::Dashboard, View::Fans, View::Rgb, View::Profiles];

    fn label(self, lang: Language) -> &'static str {
        let t = i18n::tr(lang);
        match self {
            Self::Dashboard => t.nav_dashboard,
            Self::Fans => t.nav_fans,
            Self::Rgb => t.nav_rgb,
            Self::Profiles => t.nav_profiles,
        }
    }
}

const RGB_MODES: [&str; 4] = ["fixed", "breathing", "fading", "spectrum-wave"];
const RGB_CHANNELS: [&str; 4] = ["sync", "led", "ring", "logo"];

/// Result of a background action.
struct ActionResult {
    message: String,
    ok: bool,
    applied_profile: Option<String>,
}

#[derive(Debug, Clone)]
struct ChannelRgbState {
    mode: usize,
    color: Color32,
}

impl Default for ChannelRgbState {
    fn default() -> Self {
        Self {
            mode: 0,
            color: CYAN,
        }
    }
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
    device_curves: HashMap<String, String>,
    manual_duty: HashMap<(String, String), f32>,
    rgb_per_channel: HashMap<(String, String), ChannelRgbState>,
    featured_sensors: Vec<String>,
    show_new_profile: bool,
    new_profile_name: String,
    new_profile_desc: String,
    draft_profile: Profile,
    export_dir: String,
    import_path: String,
    local_active_profile: Option<String>,
}

fn default_featured_sensors() -> Vec<String> {
    vec!["cpu".into(), "gpu".into(), "aio".into()]
}

fn is_featured(featured: &[String], id: &str) -> bool {
    featured.iter().any(|s| s == id)
}

fn promote_sensor(featured: &mut Vec<String>, id: &str) {
    if !is_featured(featured, id) {
        featured.push(id.to_string());
    }
}

fn demote_sensor(featured: &mut Vec<String>, id: &str) {
    featured.retain(|s| s != id);
}

fn group_channels_by_device(channels: &[ChannelInfo]) -> Vec<(String, Vec<&ChannelInfo>)> {
    let mut groups: Vec<(String, Vec<&ChannelInfo>)> = Vec::new();
    for ch in channels {
        if let Some((_, list)) = groups.iter_mut().find(|(dev, _)| dev == &ch.device) {
            list.push(ch);
        } else {
            groups.push((ch.device.clone(), vec![ch]));
        }
    }
    groups
}

fn is_pump_channel(channel: &str) -> bool {
    let lower = channel.to_ascii_lowercase();
    lower == "pump" || lower.contains("pump") || lower.contains("bomba")
}

fn capture_profile_from_state(
    rgb_per_channel: &HashMap<(String, String), ChannelRgbState>,
    manual_duty: &HashMap<(String, String), f32>,
    device_curves: &HashMap<String, String>,
    snap: &Snapshot,
) -> Profile {
    let mut cooling = Vec::new();
    let mut seen_cooling = std::collections::HashSet::new();

    for ch in &snap.channels {
        let key = (ch.device.clone(), ch.channel.clone());
        seen_cooling.insert(key.clone());
        let target = if let Some(curve) = device_curves.get(&ch.device) {
            CoolingTarget::Curve {
                curve: curve.clone(),
                sensor: None,
            }
        } else if let Some(&duty) = manual_duty.get(&key) {
            CoolingTarget::Fixed {
                duty: duty.round().clamp(0.0, 100.0) as u8,
            }
        } else if let Some(duty) = ch.duty {
            CoolingTarget::Fixed {
                duty: duty.round().clamp(0.0, 100.0) as u8,
            }
        } else {
            continue;
        };
        cooling.push(CoolingRule {
            match_device: ch.device.clone(),
            channel: ch.channel.clone(),
            target,
        });
    }

    let mut extra_manual: Vec<_> = manual_duty
        .iter()
        .filter(|(k, _)| !seen_cooling.contains(*k))
        .collect();
    extra_manual.sort_by_key(|(a, _)| *a);
    for ((dev, ch), &duty) in extra_manual {
        let target = if let Some(curve) = device_curves.get(dev) {
            CoolingTarget::Curve {
                curve: curve.clone(),
                sensor: None,
            }
        } else {
            CoolingTarget::Fixed {
                duty: duty.round().clamp(0.0, 100.0) as u8,
            }
        };
        cooling.push(CoolingRule {
            match_device: dev.clone(),
            channel: ch.clone(),
            target,
        });
    }

    let mut rgb_keys: Vec<_> = rgb_per_channel.keys().collect();
    rgb_keys.sort();
    let mut lighting = Vec::with_capacity(rgb_keys.len());
    for key in rgb_keys {
        let (dev, ch) = key;
        let state = &rgb_per_channel[key];
        let modes: Vec<&str> = snap
            .lighting_info
            .get(dev)
            .filter(|info| !info.modes.is_empty())
            .map(|info| info.modes.iter().map(String::as_str).collect())
            .unwrap_or_else(|| RGB_MODES.to_vec());
        let idx = state.mode.min(modes.len().saturating_sub(1));
        let mode = modes.get(idx).copied().unwrap_or("fixed").to_string();
        let colors = if mode == "spectrum-wave" || mode == "off" {
            Vec::new()
        } else {
            vec![format!(
                "{:02x}{:02x}{:02x}",
                state.color.r(),
                state.color.g(),
                state.color.b()
            )]
        };
        lighting.push(LightingRule {
            match_device: dev.clone(),
            channel: ch.clone(),
            mode,
            colors,
        });
    }

    Profile {
        description: None,
        lighting,
        cooling,
    }
}

impl NxctlApp {
    fn new(cc: &eframe::CreationContext<'_>, cli: Cli) -> Self {
        apply_obsidian_theme(&cc.egui_ctx);
        let socket = cli.socket.unwrap_or_else(default_socket_path);
        let mut config_err = None;
        let mut config = match Config::load(cli.config.as_deref()) {
            Ok(c) => c,
            Err(e) => {
                config_err = Some(e.to_string());
                Config::starter_nzxt()
            }
        };
        if let Some(lang) = cli.lang {
            config.language = lang;
        }
        let (status_msg, status_ok) = match config_err {
            None => (i18n::config_loaded(config.language), true),
            Some(e) => (i18n::config_unavailable(config.language, &e), false),
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
            view: cli.view.unwrap_or(View::Dashboard),
            status_msg,
            status_ok,
            action_tx,
            action_rx,
            ctx: cc.egui_ctx.clone(),
            device_curves: HashMap::new(),
            manual_duty: HashMap::new(),
            rgb_per_channel: HashMap::new(),
            featured_sensors: default_featured_sensors(),
            show_new_profile: false,
            new_profile_name: String::new(),
            new_profile_desc: String::new(),
            draft_profile: Profile {
                description: None,
                lighting: Vec::new(),
                cooling: Vec::new(),
            },
            export_dir: Config::default_path()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join("profiles").display().to_string()))
                .unwrap_or_else(|| "/tmp/nxctl-profiles".into()),
            import_path: String::new(),
            local_active_profile: None,
        }
    }

    fn daemon_online(&self) -> bool {
        self.snap.daemon.is_some()
    }

    fn capture_current_profile(&self) -> Profile {
        capture_profile_from_state(
            &self.rgb_per_channel,
            &self.manual_duty,
            &self.device_curves,
            &self.snap,
        )
    }

    fn save_and_reload_config(&mut self, ok_msg: String) {
        let lang = self.config.language;
        match self.config.save(self.config_path.as_deref()) {
            Ok(_) => {
                if self.daemon_online() {
                    let sock = self.socket.clone();
                    self.spawn_action(move || ipc(&sock, &IpcRequest::Reload, lang));
                }
                self.status_msg = ok_msg;
                self.status_ok = true;
            }
            Err(e) => {
                self.status_msg = i18n::config_save_error(lang, &e);
                self.status_ok = false;
            }
        }
    }

    fn active_profile(&self) -> String {
        self.snap
            .daemon
            .as_ref()
            .map(|d| d.active_profile.clone())
            .or_else(|| self.local_active_profile.clone())
            .unwrap_or_else(|| self.config.default_profile.clone())
    }

    /// Runs `f` on a worker thread and reports the outcome in the status line.
    fn spawn_action(&self, f: impl FnOnce() -> anyhow::Result<String> + Send + 'static) {
        let tx = self.action_tx.clone();
        let ctx = self.ctx.clone();
        thread::spawn(move || {
            let res = match f() {
                Ok(message) => ActionResult {
                    message,
                    ok: true,
                    applied_profile: None,
                },
                Err(e) => ActionResult {
                    message: e.to_string(),
                    ok: false,
                    applied_profile: None,
                },
            };
            let _ = tx.send(res);
            ctx.request_repaint();
        });
    }

    fn do_reload(&mut self) {
        let load_ok = match Config::load(self.config_path.as_deref()) {
            Ok(c) => {
                self.config = c;
                true
            }
            Err(e) => {
                self.status_msg = i18n::config_reload_error(self.config.language, &e);
                self.status_ok = false;
                false
            }
        };
        let lang = self.config.language;
        if self.daemon_online() {
            let sock = self.socket.clone();
            self.spawn_action(move || ipc(&sock, &IpcRequest::Reload, lang));
        } else if load_ok {
            self.status_msg = i18n::config_reloaded_offline(lang);
            self.status_ok = true;
        }
        self.poller.trigger();
    }

    fn do_reapply(&mut self) {
        if self.daemon_online() {
            let sock = self.socket.clone();
            let lang = self.config.language;
            self.spawn_action(move || ipc(&sock, &IpcRequest::Reapply, lang));
        } else {
            self.apply_local(self.active_profile());
        }
    }

    fn apply_local(&self, name: String) {
        let backend = self.backend.clone();
        let config = self.config.clone();
        let lang = config.language;
        let dry = self.dry_run;
        let tx = self.action_tx.clone();
        let ctx = self.ctx.clone();
        thread::spawn(move || {
            let res = match apply_profile(backend.as_ref(), &config, &name, dry) {
                Ok(r) => ActionResult {
                    message: i18n::fmt_profile_applied(
                        lang,
                        &name,
                        r.lighting_applied,
                        r.cooling_applied,
                        r.warnings.len(),
                    ),
                    ok: true,
                    applied_profile: Some(name),
                },
                Err(e) => ActionResult {
                    message: e.to_string(),
                    ok: false,
                    applied_profile: None,
                },
            };
            let _ = tx.send(res);
            ctx.request_repaint();
        });
    }

    fn apply_profile_named(&self, name: &str) {
        if self.daemon_online() {
            let sock = self.socket.clone();
            let profile = name.to_string();
            let lang = self.config.language;
            let tx = self.action_tx.clone();
            let ctx = self.ctx.clone();
            thread::spawn(move || {
                let res = match ipc(
                    &sock,
                    &IpcRequest::SetProfile {
                        profile: profile.clone(),
                    },
                    lang,
                ) {
                    Ok(message) => ActionResult {
                        message,
                        ok: true,
                        applied_profile: Some(profile),
                    },
                    Err(e) => ActionResult {
                        message: e.to_string(),
                        ok: false,
                        applied_profile: None,
                    },
                };
                let _ = tx.send(res);
                ctx.request_repaint();
            });
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
            if let Some(p) = r.applied_profile {
                self.local_active_profile = Some(p);
            }
            refresh = true;
        }
        if refresh {
            self.poller.trigger();
        }
    }

    // ---------------------------------------------------------------- panels

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        let lang = self.config.language;
        let t = i18n::tr(lang);
        ui.add_space(8.0);
        ui.label(RichText::new("NXCTL").color(CYAN).strong().size(15.0));
        ui.add_space(14.0);
        for v in View::ALL {
            let selected = self.view == v;
            let text = RichText::new(v.label(lang)).size(14.0).color(if selected {
                CYAN
            } else {
                TEXT_SECONDARY
            });
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
        let mut selected_lang: Option<Language> = None;
        let (daemon_txt, daemon_col) = if self.daemon_online() {
            ("SYS OK", EMERALD)
        } else {
            ("OFFLINE", CRIMSON)
        };

        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.add_space(8.0);
            if self.dry_run {
                small_muted(ui, t.dry_run_mode);
                ui.add_space(4.0);
            }
            egui::Frame::new()
                .fill(BG_CARD)
                .stroke(egui::Stroke::new(1.0, BORDER_MUTED))
                .corner_radius(egui::CornerRadius::same(4))
                .inner_margin(egui::Margin::same(8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        draw_shield_icon(ui, 13.0, daemon_col);
                        ui.label(
                            RichText::new("DAEMON")
                                .color(TEXT_SECONDARY)
                                .size(11.0)
                                .strong(),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            badge(ui, daemon_txt, daemon_col);
                        });
                    });
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(t.language_label)
                            .color(TEXT_SECONDARY)
                            .size(11.0)
                            .strong(),
                    );
                    ui.add_space(4.0);
                    let mut current_lang = lang;
                    egui::ComboBox::from_id_salt("sidebar_language")
                        .width(ui.available_width())
                        .selected_text(current_lang.display_name())
                        .show_ui(ui, |ui| {
                            for option in Language::ALL {
                                if ui
                                    .selectable_value(
                                        &mut current_lang,
                                        option,
                                        option.display_name(),
                                    )
                                    .changed()
                                    && option != lang
                                {
                                    selected_lang = Some(option);
                                }
                            }
                        });
                    if !self.status_msg.is_empty() {
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(&self.status_msg)
                                .size(11.0)
                                .color(if self.status_ok { EMERALD } else { AMBER }),
                        );
                    }
                });
        });

        if let Some(new_lang) = selected_lang {
            self.config.language = new_lang;
            self.save_and_reload_config(i18n::language_changed(new_lang));
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let lang = self.config.language;
        let t = i18n::tr(lang);
        let active = self.active_profile();
        let profile_names: Vec<String> = self.config.profiles.keys().cloned().collect();
        let mut selected_to_apply: Option<String> = None;
        ui.horizontal_centered(|ui| {
            ui.add_space(12.0);
            let fmt = |v: Option<f32>| v.map_or("--".to_string(), |t| format!("{t:.0}°C"));
            ui.label(
                RichText::new(i18n::fmt_top_summary(
                    lang,
                    &fmt(self.snap.cpu_c),
                    &fmt(self.snap.gpu_c),
                    self.snap.devices.len(),
                ))
                .color(TEXT_SECONDARY)
                .monospace(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(12.0);
                if ui.button(t.reapply).clicked() {
                    self.do_reapply();
                }
                if ui.button(t.reload).clicked() {
                    self.do_reload();
                }
                let mut current = active.clone();
                egui::ComboBox::from_id_salt("topbar_quick_profile")
                    .selected_text(RichText::new(&current).color(CYAN).strong())
                    .show_ui(ui, |ui| {
                        for name in &profile_names {
                            if ui
                                .selectable_value(&mut current, name.clone(), name)
                                .changed()
                            {
                                selected_to_apply = Some(name.clone());
                            }
                        }
                    });
                ui.label(
                    RichText::new(format!("{}:", t.quick_profile_title))
                        .color(TEXT_SECONDARY)
                        .size(11.0)
                        .strong(),
                );
            });
        });
        if let Some(profile) = selected_to_apply {
            self.apply_profile_named(&profile);
        }
    }

    // ----------------------------------------------------------------- views

    fn dashboard(&mut self, ui: &mut egui::Ui) {
        let lang = self.config.language;
        let t = i18n::tr(lang);
        let snap = self.snap.clone();
        let empty_hist = VecDeque::new();
        let mut to_demote: Option<String> = None;
        let featured_snapshot = self.featured_sensors.clone();
        ui.columns(2, |cols| {
            let ui = &mut cols[0];
            for id in &featured_snapshot {
                match id.as_str() {
                    "cpu" => {
                        if temp_card(
                            ui,
                            "cpu",
                            DashboardIcon::Cpu,
                            "CPU",
                            snap.cpu_c,
                            100.0,
                            "PACKAGE",
                            CYAN,
                            &snap.cpu_history,
                            t.demote,
                        ) {
                            to_demote = Some("cpu".into());
                        }
                        ui.add_space(8.0);
                    }
                    "gpu" => {
                        if temp_card(
                            ui,
                            "gpu",
                            DashboardIcon::Gpu,
                            "GPU",
                            snap.gpu_c,
                            100.0,
                            "NVIDIA",
                            MAGENTA,
                            &snap.gpu_history,
                            t.demote,
                        ) {
                            to_demote = Some("gpu".into());
                        }
                        ui.add_space(8.0);
                    }
                    "aio" => {
                        if snap.liquid_c.is_some() {
                            if temp_card(
                                ui,
                                "aio",
                                DashboardIcon::Pump,
                                t.aio_liquid,
                                snap.liquid_c,
                                100.0,
                                t.liquid_sub,
                                EMERALD,
                                &snap.liquid_history,
                                t.demote,
                            ) {
                                to_demote = Some("aio".into());
                            }
                            ui.add_space(8.0);
                        }
                    }
                    _ => {
                        if let Some(s) = snap.hwmon.iter().find(|s| hwmon_sensor_id(s) == *id) {
                            let (tag, accent) = classify_hwmon_sensor(&s.name, lang);
                            let icon = if tag == "iGPU" {
                                DashboardIcon::Gpu
                            } else {
                                DashboardIcon::Sensor
                            };
                            let label = s.label.as_deref().unwrap_or(&s.input_file);
                            let title = format!("{} ({label})", s.name);
                            let hist = snap.hwmon_history.get(id).unwrap_or(&empty_hist);
                            if temp_card(
                                ui,
                                id,
                                icon,
                                &title,
                                Some(s.temp_c),
                                100.0,
                                tag,
                                accent,
                                hist,
                                t.demote,
                            ) {
                                to_demote = Some(id.clone());
                            }
                            ui.add_space(8.0);
                        }
                    }
                }
            }

            let ui = &mut cols[1];
            card_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    draw_fan_icon(ui, 15.0, CYAN);
                    card_title(ui, t.pwm_headers);
                });
                if snap.channels.is_empty() {
                    small_muted(ui, t.no_nzxt_channels);
                }
                for c in &snap.channels {
                    let is_pump = is_pump_channel(&c.channel);
                    let accent = if is_pump { MAGENTA } else { CYAN };
                    draw_pwm_bar(ui, &c.label, c.rpm, c.duty.unwrap_or(0.0), accent, is_pump);
                }
            });
            ui.add_space(8.0);
            other_sensors_card(ui, &snap, &mut self.featured_sensors, lang);
        });
        if let Some(id) = to_demote {
            demote_sensor(&mut self.featured_sensors, &id);
        }
    }

    fn fans(&mut self, ui: &mut egui::Ui) {
        let lang = self.config.language;
        let t = i18n::tr(lang);
        let snap = self.snap.clone();
        if snap.channels.is_empty() {
            card_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                card_title(ui, t.nav_fans);
                small_muted(ui, t.no_cooling_channels);
            });
            return;
        }

        let curve_names: Vec<String> = self.config.curves.keys().cloned().collect();
        let groups = group_channels_by_device(&snap.channels);

        egui::ScrollArea::vertical().show(ui, |ui| {
            for (dev, channels) in groups {
                let all_pumps =
                    !channels.is_empty() && channels.iter().all(|c| is_pump_channel(&c.channel));
                let header_accent = if all_pumps { MAGENTA } else { CYAN };

                let selected_name = {
                    let entry = self
                        .device_curves
                        .entry(dev.clone())
                        .or_insert_with(|| curve_names.first().cloned().unwrap_or_default());
                    if !self.config.curves.contains_key(entry.as_str())
                        && let Some(first) = curve_names.first()
                    {
                        *entry = first.clone();
                    }
                    entry.clone()
                };

                card_frame().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        if all_pumps {
                            draw_pump_icon(ui, 18.0, header_accent);
                        } else {
                            draw_fan_icon(ui, 18.0, header_accent);
                        }
                        card_title(ui, &dev);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let mut current = selected_name.clone();
                            egui::ComboBox::from_id_salt(("fan_curve", &dev))
                                .selected_text(if current.is_empty() {
                                    "—".to_string()
                                } else {
                                    current.clone()
                                })
                                .show_ui(ui, |ui| {
                                    for n in &curve_names {
                                        if ui.selectable_value(&mut current, n.clone(), n).changed()
                                        {
                                            self.device_curves.insert(dev.clone(), n.clone());
                                        }
                                    }
                                });
                            badge(
                                ui,
                                &i18n::fmt_channels_badge(lang, channels.len()),
                                header_accent,
                            );
                        });
                    });

                    let curve: Option<&FanCurve> = self
                        .config
                        .curves
                        .get(&selected_name)
                        .or_else(|| self.config.curves.values().next());
                    if let Some(curve) = curve {
                        let pts: Vec<[f64; 2]> = curve
                            .points
                            .iter()
                            .map(|p| [f64::from(p.temp_c), f64::from(p.duty)])
                            .collect();
                        let dev_lower = dev.to_ascii_lowercase();
                        let live = if dev_lower.contains("nvidia")
                            || dev_lower.contains("amdgpu")
                            || dev_lower.contains("gpu")
                        {
                            snap.gpu_c
                        } else {
                            snap.liquid_c.or(snap.cpu_c)
                        };
                        Plot::new(("curve_plot", &dev))
                            .height(155.0)
                            .allow_drag(false)
                            .allow_zoom(false)
                            .allow_scroll(false)
                            .include_y(0.0)
                            .include_y(100.0)
                            .show(ui, |p| {
                                p.line(
                                    Line::new(t.curve, PlotPoints::from(pts.clone()))
                                        .color(header_accent)
                                        .width(2.0),
                                );
                                p.points(
                                    Points::new(t.nodes, PlotPoints::from(pts))
                                        .color(header_accent)
                                        .radius(3.0),
                                );
                                if let Some(temp) = live {
                                    let duty = evaluate_curve(curve, temp);
                                    p.vline(VLine::new("Temp", f64::from(temp)).color(TEXT_MUTED));
                                    p.hline(HLine::new("Duty", f64::from(duty)).color(TEXT_MUTED));
                                    p.points(
                                        Points::new(
                                            t.operating,
                                            PlotPoints::from(vec![[
                                                f64::from(temp),
                                                f64::from(duty),
                                            ]]),
                                        )
                                        .color(AMBER)
                                        .radius(6.0),
                                    );
                                }
                            });
                        if let Some(temp) = live {
                            small_muted(
                                ui,
                                &i18n::fmt_operating_point(lang, temp, evaluate_curve(curve, temp)),
                            );
                        }
                    }

                    ui.add_space(6.0);
                    for c in &channels {
                        let is_pump = is_pump_channel(&c.channel);
                        let accent = if is_pump { MAGENTA } else { CYAN };
                        let key = (c.device.clone(), c.channel.clone());
                        let entry = self
                            .manual_duty
                            .entry(key)
                            .or_insert_with(|| c.duty.unwrap_or(0.0));

                        let mut set_duty: Option<u8> = None;
                        egui::Frame::new()
                            .fill(BG_SIDEBAR)
                            .corner_radius(egui::CornerRadius::same(4))
                            .stroke(egui::Stroke::new(1.0, BORDER_MUTED))
                            .inner_margin(egui::Margin::symmetric(10, 6))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    if is_pump {
                                        draw_pump_icon(ui, 15.0, accent);
                                    } else {
                                        draw_fan_icon(ui, 15.0, accent);
                                    }
                                    ui.label(
                                        RichText::new(&c.label).color(TEXT_PRIMARY).size(13.0),
                                    );
                                    if let Some(rpm) = c.rpm {
                                        ui.label(
                                            RichText::new(format!("{rpm:.0} RPM"))
                                                .color(TEXT_SECONDARY)
                                                .monospace()
                                                .size(12.0),
                                        );
                                    }
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            let resp = ui.add(
                                                egui::Slider::new(entry, 0.0..=100.0).suffix("%"),
                                            );
                                            if resp.drag_stopped()
                                                || (resp.changed() && !resp.dragged())
                                            {
                                                set_duty = Some(*entry as u8);
                                            }
                                        },
                                    );
                                });
                            });
                        if let Some(duty) = set_duty {
                            let backend = self.backend.clone();
                            let (dev_name, ch_name, dry) =
                                (c.device.clone(), c.channel.clone(), self.dry_run);
                            self.spawn_action(move || {
                                backend.set_speed(&dev_name, &ch_name, duty, dry)?;
                                Ok(format!("{dev_name}/{ch_name} → {duty}%"))
                            });
                        }
                        ui.add_space(4.0);
                    }
                });
                ui.add_space(8.0);
            }
        });
    }

    fn resolve_device_rgb(&self, snap: &Snapshot, dev: &str) -> (Vec<String>, Vec<String>) {
        let mut channels: Vec<String> = snap
            .lighting_info
            .get(dev)
            .filter(|info| !info.channels.is_empty())
            .map(|info| info.channels.clone())
            .unwrap_or_else(|| {
                let mut dev_channels: Vec<String> =
                    RGB_CHANNELS.iter().map(|s| (*s).to_string()).collect();
                for p in self.config.profiles.values() {
                    for l in &p.lighting {
                        let matches = l.match_device.is_empty()
                            || dev
                                .to_ascii_lowercase()
                                .contains(&l.match_device.to_ascii_lowercase())
                            || l.match_device
                                .to_ascii_lowercase()
                                .contains(&dev.to_ascii_lowercase());
                        if matches && !dev_channels.contains(&l.channel) {
                            dev_channels.push(l.channel.clone());
                        }
                    }
                }
                dev_channels
            });
        if let Some(pos) = channels.iter().position(|c| c == "sync") {
            let sync_ch = channels.remove(pos);
            channels.insert(0, sync_ch);
        }
        let modes: Vec<String> = snap
            .lighting_info
            .get(dev)
            .filter(|info| !info.modes.is_empty())
            .map(|info| info.modes.clone())
            .unwrap_or_else(|| RGB_MODES.iter().map(|s| (*s).to_string()).collect());
        (channels, modes)
    }

    fn sync_rgb_rows(
        &mut self,
        snap: &Snapshot,
        devices: &[String],
        color: Color32,
        mode_name: &str,
    ) {
        for dev in devices {
            let (channels, modes) = self.resolve_device_rgb(snap, dev);
            let mode_idx = modes
                .iter()
                .position(|m| m.eq_ignore_ascii_case(mode_name))
                .unwrap_or(0);
            for ch in channels {
                let entry = self.rgb_per_channel.entry((dev.clone(), ch)).or_default();
                entry.color = color;
                entry.mode = mode_idx;
            }
        }
    }

    fn rgb(&mut self, ui: &mut egui::Ui) {
        let lang = self.config.language;
        let t = i18n::tr(lang);
        let snap = self.snap.clone();
        let devices: Vec<String> = snap.devices.iter().map(|d| d.description.clone()).collect();

        let mut pending: Option<(String, String, String, Vec<String>)> = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            card_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    draw_palette_icon(ui, 15.0, MAGENTA);
                    card_title(ui, "Pywal Sync");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.config.pywal.enabled {
                            badge(ui, t.enabled, EMERALD);
                        } else {
                            badge(ui, t.disabled, TEXT_MUTED);
                        }
                    });
                });

                let mut pywal_enabled = self.config.pywal.enabled;
                let mut pywal_mode = if self.config.pywal.mode.trim().is_empty() {
                    "fixed".to_string()
                } else {
                    self.config.pywal.mode.clone()
                };
                let mut mode_changed = false;
                ui.horizontal_wrapped(|ui| {
                    if ui.checkbox(&mut pywal_enabled, t.pywal_auto_sync).changed() {
                        self.config.pywal.enabled = pywal_enabled;
                        self.save_and_reload_config(i18n::pywal_config_saved(lang));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        egui::ComboBox::from_id_salt("pywal_global_mode")
                            .selected_text(&pywal_mode)
                            .show_ui(ui, |ui| {
                                for m in ["fixed", "breathing", "fading"] {
                                    if ui
                                        .selectable_value(&mut pywal_mode, m.to_string(), m)
                                        .changed()
                                    {
                                        mode_changed = true;
                                    }
                                }
                            });
                        ui.label(
                            RichText::new(t.global_mode)
                                .size(12.0)
                                .color(TEXT_SECONDARY),
                        );
                    });
                });
                if mode_changed {
                    self.config.pywal.mode = pywal_mode.clone();
                    self.save_and_reload_config(i18n::pywal_mode_changed(lang, &pywal_mode));
                }

                match load_pywal_palette(self.config.pywal.colors_path.as_deref().map(Path::new)) {
                    Ok(palette) => {
                        let mut filter_extremes = self.config.pywal.filter_extremes;
                        let mut palette_changed = false;
                        ui.add_space(4.0);
                        ui.horizontal_wrapped(|ui| {
                            if ui
                                .checkbox(&mut filter_extremes, t.filter_extremes)
                                .changed()
                            {
                                self.config.pywal.filter_extremes = filter_extremes;
                                palette_changed = true;
                            }
                            let entries = active_palette_entries(
                                &palette,
                                self.config.pywal.filter_extremes,
                                self.config.pywal.palette_offset,
                            );
                            let len = entries.len().max(1);
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.small_button("+").clicked() {
                                        self.config.pywal.palette_offset =
                                            (self.config.pywal.palette_offset + 1) % len;
                                        palette_changed = true;
                                    }
                                    ui.label(
                                        RichText::new(format!(
                                            "+{}",
                                            self.config.pywal.palette_offset % len
                                        ))
                                        .monospace()
                                        .size(11.0)
                                        .color(CYAN),
                                    );
                                    if ui.small_button("-").clicked() {
                                        self.config.pywal.palette_offset =
                                            (self.config.pywal.palette_offset + len - 1) % len;
                                        palette_changed = true;
                                    }
                                    ui.label(
                                        RichText::new(t.shift_palette)
                                            .size(11.0)
                                            .color(TEXT_SECONDARY),
                                    );
                                    draw_palette_icon(ui, 13.0, CYAN);
                                },
                            );
                        });
                        if palette_changed {
                            self.save_and_reload_config(i18n::pywal_palette_updated(lang));
                        }

                        let entries = active_palette_entries(
                            &palette,
                            self.config.pywal.filter_extremes,
                            self.config.pywal.palette_offset,
                        );

                        ui.add_space(6.0);
                        ui.horizontal_wrapped(|ui| {
                            if let Some(ref wp) = palette.wallpaper {
                                let wp_name = Path::new(wp)
                                    .file_name()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or(wp);
                                ui.label(
                                    RichText::new(i18n::fmt_wallpaper(lang, wp_name))
                                        .size(11.0)
                                        .color(TEXT_SECONDARY),
                                );
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.button(t.sync_now).clicked() {
                                        let primary_rgb = if !self.config.pywal.filter_extremes
                                            && self.config.pywal.palette_offset == 0
                                        {
                                            palette
                                                .color_rgb("color4")
                                                .or_else(|| palette.color_rgb("color1"))
                                        } else {
                                            entries.first().map(|(_, _, rgb)| *rgb)
                                        };
                                        if let Some([r, g, b]) = primary_rgb {
                                            let c = Color32::from_rgb(r, g, b);
                                            self.sync_rgb_rows(&snap, &devices, c, &pywal_mode);
                                        }
                                        let backend = self.backend.clone();
                                        let pywal_cfg = self.config.pywal.clone();
                                        let pal = palette.clone();
                                        let dry = self.dry_run;
                                        self.spawn_action(move || {
                                            let count = apply_pywal_palette(
                                                &*backend, &pywal_cfg, &pal, dry,
                                            )?;
                                            Ok(i18n::fmt_pywal_synced(lang, count))
                                        });
                                    }
                                },
                            );
                        });

                        ui.add_space(6.0);
                        ui.horizontal_wrapped(|ui| {
                            for (key, hex_str, [r, g, b]) in &entries {
                                let color32 = Color32::from_rgb(*r, *g, *b);
                                let (rect, resp) = ui.allocate_exact_size(
                                    egui::vec2(22.0, 22.0),
                                    egui::Sense::click(),
                                );
                                ui.painter().rect_filled(
                                    rect,
                                    egui::CornerRadius::same(3),
                                    color32,
                                );
                                if resp.hovered() {
                                    ui.painter().rect_stroke(
                                        rect,
                                        egui::CornerRadius::same(3),
                                        egui::Stroke::new(1.5, Color32::WHITE),
                                        egui::StrokeKind::Inside,
                                    );
                                }
                                let resp = resp.on_hover_text(format!("{key}: #{hex_str}"));
                                if resp.clicked() {
                                    self.sync_rgb_rows(&snap, &devices, color32, &pywal_mode);
                                    let backend = self.backend.clone();
                                    let dry = self.dry_run;
                                    let hex = hex_str.clone();
                                    let devs = devices.clone();
                                    let mode = pywal_mode.clone();
                                    self.spawn_action(move || {
                                        for d in &devs {
                                            let _ = backend.set_color(
                                                d,
                                                "sync",
                                                &mode,
                                                std::slice::from_ref(&hex),
                                                dry,
                                            );
                                        }
                                        Ok(i18n::fmt_color_applied_all(lang, &hex, &mode))
                                    });
                                }
                            }
                        });
                    }
                    Err(_) => {
                        ui.add_space(4.0);
                        small_muted(ui, t.pywal_not_detected);
                    }
                }
            });

            ui.add_space(8.0);

            if devices.is_empty() {
                card_frame().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    card_title(ui, t.nav_rgb);
                    small_muted(ui, t.no_nzxt_devices);
                });
                return;
            }

            for dev in &devices {
                let (channels, modes) = self.resolve_device_rgb(&snap, dev);

                card_frame().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let is_aio = dev.to_ascii_lowercase().contains("kraken")
                            || dev.to_ascii_lowercase().contains("corsair")
                            || dev.to_ascii_lowercase().contains("aio");
                        if is_aio {
                            draw_pump_icon(ui, 15.0, CYAN);
                        } else {
                            draw_led_icon(ui, 15.0, MAGENTA);
                        }
                        card_title(ui, dev);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            badge(ui, &i18n::fmt_channels_badge(lang, channels.len()), MAGENTA);
                        });
                    });

                    for ch in &channels {
                        let state = self
                            .rgb_per_channel
                            .entry((dev.clone(), ch.clone()))
                            .or_default();
                        state.mode = state.mode.min(modes.len().saturating_sub(1));
                        let is_sync = ch == "sync";

                        egui::Frame::new()
                            .fill(if is_sync { BG_CONTROL } else { BG_SIDEBAR })
                            .stroke(egui::Stroke::new(
                                1.0,
                                if is_sync { BORDER_ACTIVE } else { BORDER_MUTED },
                            ))
                            .corner_radius(egui::CornerRadius::same(4))
                            .inner_margin(egui::Margin::symmetric(10, 6))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    let ch_lower = ch.to_ascii_lowercase();
                                    if is_sync {
                                        draw_sync_icon(ui, 14.0, state.color);
                                    } else if ch_lower.contains("ring") {
                                        draw_ring_icon(ui, 14.0, state.color);
                                    } else {
                                        draw_led_icon(ui, 14.0, state.color);
                                    }
                                    let label = if is_sync {
                                        t.sync_whole_device.to_string()
                                    } else {
                                        ch.clone()
                                    };
                                    ui.label(
                                        RichText::new(label)
                                            .monospace()
                                            .strong()
                                            .color(if is_sync { MAGENTA } else { TEXT_PRIMARY }),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui.button(t.apply).clicked() {
                                                let c = state.color;
                                                let hex = format!(
                                                    "{:02x}{:02x}{:02x}",
                                                    c.r(),
                                                    c.g(),
                                                    c.b()
                                                );
                                                let mode = &modes[state.mode];
                                                let colors =
                                                    if mode == "spectrum-wave" || mode == "off" {
                                                        vec![]
                                                    } else {
                                                        vec![hex]
                                                    };
                                                pending = Some((
                                                    dev.clone(),
                                                    ch.clone(),
                                                    mode.clone(),
                                                    colors,
                                                ));
                                            }
                                            egui::color_picker::color_edit_button_srgba(
                                                ui,
                                                &mut state.color,
                                                egui::color_picker::Alpha::Opaque,
                                            );
                                            egui::ComboBox::from_id_salt(("rgb_mode", dev, ch))
                                                .selected_text(&modes[state.mode])
                                                .show_ui(ui, |ui| {
                                                    for (i, m) in modes.iter().enumerate() {
                                                        ui.selectable_value(&mut state.mode, i, m);
                                                    }
                                                });
                                        },
                                    );
                                });
                            });
                        ui.add_space(4.0);
                    }
                });
                ui.add_space(8.0);
            }
        });

        if let Some((dev_name, ch, mode_str, colors)) = pending {
            let backend = self.backend.clone();
            let dry = self.dry_run;
            self.spawn_action(move || {
                backend.set_color(&dev_name, &ch, &mode_str, &colors, dry)?;
                Ok(i18n::fmt_channel_mode_applied(
                    lang, &dev_name, &ch, &mode_str,
                ))
            });
        }
    }

    fn profiles(&mut self, ui: &mut egui::Ui) {
        let lang = self.config.language;
        let t = i18n::tr(lang);
        let active = self.active_profile();
        let curve_names: Vec<String> = self.config.curves.keys().cloned().collect();

        // Top action card
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                if ui.button(t.new_profile).clicked() {
                    self.show_new_profile = !self.show_new_profile;
                    if self.show_new_profile {
                        self.new_profile_name.clear();
                        self.new_profile_desc.clear();
                        self.draft_profile = self.capture_current_profile();
                    }
                }
                if ui.button(t.save_config).clicked() {
                    self.save_and_reload_config(i18n::config_saved(lang));
                }
                ui.separator();
                ui.label(t.import_toml);
                ui.add(
                    egui::TextEdit::singleline(&mut self.import_path)
                        .desired_width(240.0)
                        .hint_text(t.import_hint),
                );
                if ui.button(t.load).clicked() {
                    let path = self.import_path.trim().to_string();
                    if path.is_empty() {
                        self.status_msg = i18n::profile_import_path_empty(lang);
                        self.status_ok = false;
                    } else {
                        match self.config.import_profile(Path::new(&path), true) {
                            Ok(imported) => {
                                self.save_and_reload_config(i18n::profile_imported(
                                    lang, &imported,
                                ));
                            }
                            Err(e) => {
                                self.status_msg = i18n::profile_import_error(lang, &e);
                                self.status_ok = false;
                            }
                        }
                    }
                }
            });
        });
        ui.add_space(8.0);

        // Profile Creator / Editor card
        if self.show_new_profile {
            let mut save_requested = false;
            let mut capture_requested = false;
            card_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    card_title(ui, t.profile_editor);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(t.capture_current_state).clicked() {
                            capture_requested = true;
                        }
                    });
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(t.name_label);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_profile_name)
                            .desired_width(180.0)
                            .hint_text(t.profile_name_hint),
                    );
                    ui.label(t.description_label);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_profile_desc)
                            .desired_width(260.0)
                            .hint_text(t.optional_hint),
                    );
                });

                ui.add_space(6.0);
                small_muted(
                    ui,
                    &i18n::fmt_cooling_rules_header(lang, self.draft_profile.cooling.len()),
                );
                for (idx, rule) in self.draft_profile.cooling.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("{} / {}", rule.match_device, rule.channel))
                                .monospace()
                                .color(TEXT_PRIMARY),
                        );
                        let is_curve = matches!(rule.target, CoolingTarget::Curve { .. });
                        if ui.selectable_label(!is_curve, t.fixed).clicked() && is_curve {
                            rule.target = CoolingTarget::Fixed { duty: 50 };
                        }
                        if ui
                            .add_enabled(
                                !curve_names.is_empty(),
                                egui::Button::selectable(is_curve, t.curve),
                            )
                            .clicked()
                            && !is_curve
                            && let Some(first_curve) = curve_names.first()
                        {
                            rule.target = CoolingTarget::Curve {
                                curve: first_curve.clone(),
                                sensor: None,
                            };
                        }
                        match &mut rule.target {
                            CoolingTarget::Fixed { duty } => {
                                let mut d = *duty as f32;
                                if ui
                                    .add(egui::Slider::new(&mut d, 0.0..=100.0).suffix("%"))
                                    .changed()
                                {
                                    *duty = d.round().clamp(0.0, 100.0) as u8;
                                }
                            }
                            CoolingTarget::Curve { curve, .. } => {
                                egui::ComboBox::from_id_salt(("draft_curve", idx))
                                    .selected_text(curve.as_str())
                                    .show_ui(ui, |ui| {
                                        for c in &curve_names {
                                            ui.selectable_value(curve, c.clone(), c);
                                        }
                                    });
                            }
                        }
                    });
                }

                ui.add_space(6.0);
                small_muted(
                    ui,
                    &i18n::fmt_lighting_rules_header(lang, self.draft_profile.lighting.len()),
                );
                for rule in &self.draft_profile.lighting {
                    let color_txt = if rule.colors.is_empty() {
                        String::new()
                    } else {
                        format!(" ({})", rule.colors.join(", "))
                    };
                    small_muted(
                        ui,
                        &format!(
                            "  • {} / {}: {}{}",
                            rule.match_device, rule.channel, rule.mode, color_txt
                        ),
                    );
                }

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button(t.save_profile).clicked() {
                        save_requested = true;
                    }
                    if ui.button(t.cancel).clicked() {
                        self.show_new_profile = false;
                    }
                });
            });
            ui.add_space(8.0);

            if capture_requested {
                self.draft_profile = self.capture_current_profile();
            }
            if save_requested {
                let name = self.new_profile_name.trim().to_string();
                if name.is_empty() {
                    self.status_msg = i18n::profile_name_empty(lang);
                    self.status_ok = false;
                } else {
                    let mut profile = self.draft_profile.clone();
                    let desc = self.new_profile_desc.trim();
                    profile.description = if desc.is_empty() {
                        None
                    } else {
                        Some(desc.to_string())
                    };
                    self.config.profiles.insert(name.clone(), profile);
                    self.show_new_profile = false;
                    self.save_and_reload_config(i18n::profile_saved(lang, &name));
                }
            }
        }

        // Profile list cards
        let mut to_apply = None;
        let mut to_set_default = None;
        let mut to_export = None;
        let mut to_clone: Option<(String, Profile)> = None;
        let mut to_delete = None;
        let profile_count = self.config.profiles.len();

        egui::ScrollArea::vertical().show(ui, |ui| {
            for (name, p) in &self.config.profiles {
                let can_delete =
                    is_deletable_profile(name, &self.config.default_profile, profile_count);
                let is_default = *name == self.config.default_profile;
                card_frame().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        card_title(ui, name);
                        if *name == active {
                            badge(ui, t.active, EMERALD);
                        }
                        if is_default {
                            badge(ui, "DEFAULT", CYAN);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add_enabled(can_delete, egui::Button::new(t.delete))
                                .clicked()
                            {
                                to_delete = Some(name.clone());
                            }
                            if ui.button(t.clone).clicked() {
                                to_clone = Some((name.clone(), p.clone()));
                            }
                            if ui.button(t.export).clicked() {
                                to_export = Some(name.clone());
                            }
                            if ui
                                .add_enabled(!is_default, egui::Button::new(t.set_default))
                                .clicked()
                            {
                                to_set_default = Some(name.clone());
                            }
                            if ui.button(t.apply).clicked() {
                                to_apply = Some(name.clone());
                            }
                        });
                    });
                    if let Some(d) = &p.description {
                        small_muted(ui, d);
                    }
                    small_muted(
                        ui,
                        &i18n::fmt_profile_rules_summary(lang, p.lighting.len(), p.cooling.len()),
                    );
                });
                ui.add_space(8.0);
            }
        });

        if let Some(n) = to_apply {
            self.apply_profile_named(&n);
        }
        if let Some(n) = to_set_default {
            self.config.default_profile = n.clone();
            self.save_and_reload_config(i18n::default_profile_set(lang, &n));
        }
        if let Some(n) = to_export {
            let path = Path::new(&self.export_dir).join(format!("{n}.toml"));
            match self.config.export_profile(&n, &path) {
                Ok(()) => {
                    self.import_path = path.display().to_string();
                    self.status_msg = i18n::profile_exported(lang, &n, &self.import_path);
                    self.status_ok = true;
                }
                Err(e) => {
                    self.status_msg = i18n::profile_export_error(lang, &n, &e);
                    self.status_ok = false;
                }
            }
        }
        if let Some((orig, profile)) = to_clone {
            self.new_profile_name = i18n::fmt_cloned_profile_name(lang, &orig);
            self.new_profile_desc = profile.description.clone().unwrap_or_default();
            self.draft_profile = profile;
            self.show_new_profile = true;
        }
        if let Some(n) = to_delete {
            self.config.profiles.remove(&n);
            self.save_and_reload_config(i18n::profile_deleted(lang, &n));
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

fn ipc(socket: &std::path::Path, req: &IpcRequest, lang: Language) -> anyhow::Result<String> {
    match send_request(socket, req)? {
        IpcResponse::Ok { message } => Ok(message),
        IpcResponse::Status { daemon } => {
            Ok(i18n::fmt_ipc_active_profile(lang, &daemon.active_profile))
        }
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

#[derive(Clone, Copy)]
enum DashboardIcon {
    Cpu,
    Gpu,
    Pump,
    Sensor,
}

fn draw_dashboard_icon(ui: &mut egui::Ui, icon: DashboardIcon, size: f32, color: Color32) {
    match icon {
        DashboardIcon::Cpu => draw_cpu_icon(ui, size, color),
        DashboardIcon::Gpu => draw_gpu_icon(ui, size, color),
        DashboardIcon::Pump => draw_pump_icon(ui, size, color),
        DashboardIcon::Sensor => draw_sensor_icon(ui, size, color),
    }
}

#[allow(clippy::too_many_arguments)]
fn temp_card(
    ui: &mut egui::Ui,
    id: &str,
    icon: DashboardIcon,
    title: &str,
    value: Option<f32>,
    max: f32,
    sub: &str,
    accent: Color32,
    history: &VecDeque<f64>,
    demote_label: &str,
) -> bool {
    let mut demote = false;
    card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            draw_dashboard_icon(ui, icon, 15.0, accent);
            card_title(ui, title);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button(demote_label).clicked() {
                    demote = true;
                }
            });
        });
        ui.horizontal(|ui| {
            draw_radial_gauge(ui, value, max, sub, accent);
            let pts: Vec<[f64; 2]> = history
                .iter()
                .enumerate()
                .map(|(i, v)| [i as f64, *v])
                .collect();
            Plot::new(format!("{id}_hist"))
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
    demote
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let cli = Cli::parse();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("NXCTL")
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

/// Classifies a `hwmon` driver name into a friendly category label and accent color.
fn classify_hwmon_sensor(name: &str, lang: Language) -> (&'static str, Color32) {
    let lower = name.to_ascii_lowercase();
    if lower.contains("spd5118") || lower.contains("jc42") || lower.contains("ddr") {
        ("RAM", CYAN)
    } else if lower.contains("nvme") {
        ("SSD", MAGENTA)
    } else if lower.contains("drivetemp") || lower.contains("sata") || lower.contains("hdd") {
        ("HDD", MAGENTA)
    } else if lower.contains("amdgpu") || lower.contains("radeon") || lower.contains("nouveau") {
        ("iGPU", CYAN)
    } else if lower.contains("wifi")
        || lower.contains("mt79")
        || lower.contains("iwlwifi")
        || lower.contains("ath")
    {
        ("WI-FI", EMERALD)
    } else if lower.contains("r8169")
        || lower.contains("igc")
        || lower.contains("e1000")
        || lower.contains("eth")
    {
        (i18n::tr(lang).net_tag, EMERALD)
    } else {
        ("SYS", TEXT_SECONDARY)
    }
}

/// Filters out primary CPU/liquid sensors already shown in the hero cards.
fn secondary_hwmon_sensors(
    sensors: &[nxctl_core::HwmonSensorInfo],
) -> Vec<&nxctl_core::HwmonSensorInfo> {
    sensors
        .iter()
        .filter(|s| {
            let lower = s.name.to_ascii_lowercase();
            let is_cpu = matches!(
                lower.as_str(),
                "k10temp" | "coretemp" | "zenpower" | "cpu_thermal"
            );
            let is_aio = lower.contains("kraken")
                || matches!(
                    lower.as_str(),
                    "x53" | "x63" | "x73" | "z53" | "z63" | "z73"
                )
                || lower.contains("corsair")
                || lower.contains("aquacomputer");
            !is_cpu && !is_aio
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn compact_sensor_row(
    ui: &mut egui::Ui,
    icon: DashboardIcon,
    tag: &str,
    tag_color: Color32,
    name: &str,
    sub: &str,
    temp: Option<f32>,
    feature_label: &str,
) -> bool {
    let mut promote = false;
    let temp_color = match temp {
        Some(t) if t >= 70.0 => CRIMSON,
        Some(t) if t >= 50.0 => AMBER,
        Some(_) => EMERALD,
        None => TEXT_MUTED,
    };
    ui.horizontal(|ui| {
        draw_dashboard_icon(ui, icon, 13.0, tag_color);
        badge(ui, tag, tag_color);
        ui.label(RichText::new(name).color(TEXT_PRIMARY).size(12.0));
        ui.label(
            RichText::new(format!("({sub})"))
                .color(TEXT_MUTED)
                .size(11.0),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button(feature_label).clicked() {
                promote = true;
            }
            let txt = temp.map_or_else(|| "-- °C".to_string(), |t| format!("{t:.1} °C"));
            ui.label(RichText::new(txt).monospace().strong().color(temp_color));
        });
    });
    promote
}

fn other_sensors_card(
    ui: &mut egui::Ui,
    snap: &Snapshot,
    featured: &mut Vec<String>,
    lang: Language,
) {
    let t = i18n::tr(lang);
    let show_cpu = !is_featured(featured, "cpu");
    let show_gpu = !is_featured(featured, "gpu");
    let show_aio = snap.liquid_c.is_some() && !is_featured(featured, "aio");
    let secondary: Vec<(&nxctl_core::HwmonSensorInfo, String)> =
        secondary_hwmon_sensors(&snap.hwmon)
            .into_iter()
            .filter_map(|s| {
                let id = hwmon_sensor_id(s);
                (!is_featured(featured, &id)).then_some((s, id))
            })
            .collect();
    let total =
        usize::from(show_cpu) + usize::from(show_gpu) + usize::from(show_aio) + secondary.len();

    let mut to_promote: Option<String> = None;
    card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            draw_sensor_icon(ui, 15.0, CYAN);
            card_title(ui, t.other_sensors);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                badge(ui, &format!("{total}"), CYAN);
            });
        });
        if total == 0 {
            small_muted(ui, t.no_additional_sensors);
            return;
        }
        if show_cpu
            && compact_sensor_row(
                ui,
                DashboardIcon::Cpu,
                "CPU",
                CYAN,
                "CPU",
                "PACKAGE",
                snap.cpu_c,
                t.feature,
            )
        {
            to_promote = Some("cpu".into());
        }
        if show_gpu
            && compact_sensor_row(
                ui,
                DashboardIcon::Gpu,
                "GPU",
                MAGENTA,
                "GPU",
                "NVIDIA",
                snap.gpu_c,
                t.feature,
            )
        {
            to_promote = Some("gpu".into());
        }
        if show_aio
            && compact_sensor_row(
                ui,
                DashboardIcon::Pump,
                "AIO",
                EMERALD,
                "AIO",
                t.liquid_label,
                snap.liquid_c,
                t.feature,
            )
        {
            to_promote = Some("aio".into());
        }
        for (s, id) in secondary {
            let (tag, tag_color) = classify_hwmon_sensor(&s.name, lang);
            let icon = if tag == "iGPU" {
                DashboardIcon::Gpu
            } else {
                DashboardIcon::Sensor
            };
            let label = s.label.as_deref().unwrap_or(&s.input_file);
            if compact_sensor_row(
                ui,
                icon,
                tag,
                tag_color,
                &s.name,
                label,
                Some(s.temp_c),
                t.feature,
            ) {
                to_promote = Some(id);
            }
        }
    });
    if let Some(id) = to_promote {
        promote_sensor(featured, &id);
    }
}

/// Returns `false` for built-in (`default`, `performance`, `silent`) or active default profiles.
fn is_deletable_profile(name: &str, default_profile: &str, profile_count: usize) -> bool {
    let lower = name.to_ascii_lowercase();
    profile_count > 1
        && name != default_profile
        && !matches!(lower.as_str(), "default" | "performance" | "silent")
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
    fn classifies_and_filters_secondary_hwmon_sensors() {
        assert_eq!(classify_hwmon_sensor("spd5118", Language::Es).0, "RAM");
        assert_eq!(classify_hwmon_sensor("nvme", Language::Es).0, "SSD");
        assert_eq!(classify_hwmon_sensor("drivetemp", Language::Es).0, "HDD");
        assert_eq!(classify_hwmon_sensor("amdgpu", Language::Es).0, "iGPU");
        assert_eq!(
            classify_hwmon_sensor("mt7921_phy0", Language::Es).0,
            "WI-FI"
        );
        assert_eq!(
            classify_hwmon_sensor("r8169_0_c00:00", Language::Es).0,
            "RED"
        );
        assert_eq!(
            classify_hwmon_sensor("r8169_0_c00:00", Language::En).0,
            "NET"
        );
        assert_eq!(classify_hwmon_sensor("prom21_xhci", Language::Es).0, "SYS");

        let sensors = [
            sensor("k10temp", 45.0),
            sensor("x53", 29.5),
            sensor("nvme", 42.0),
            sensor("spd5118", 35.0),
        ];
        let secondary = secondary_hwmon_sensors(&sensors);
        assert_eq!(secondary.len(), 2);
        assert_eq!(secondary[0].name, "nvme");
        assert_eq!(secondary[1].name, "spd5118");
    }

    #[test]
    fn cli_parses_lang_flag() {
        let cli_default = Cli::try_parse_from(["nxctl-gui"]).expect("default cli");
        assert_eq!(cli_default.lang, None);

        let cli_en = Cli::try_parse_from(["nxctl-gui", "--lang", "en"]).expect("--lang en");
        assert_eq!(cli_en.lang, Some(Language::En));

        let cli_es = Cli::try_parse_from(["nxctl-gui", "--lang", "es"]).expect("--lang es");
        assert_eq!(cli_es.lang, Some(Language::Es));
    }

    #[test]
    fn view_labels_are_localized() {
        assert_eq!(View::Dashboard.label(Language::Es), "Dashboard");
        assert_eq!(View::Dashboard.label(Language::En), "Dashboard");
        assert_eq!(View::Fans.label(Language::Es), "Ventiladores");
        assert_eq!(View::Fans.label(Language::En), "Fans");
        assert_eq!(View::Rgb.label(Language::Es), "Iluminación RGB");
        assert_eq!(View::Rgb.label(Language::En), "RGB Lighting");
        assert_eq!(View::Profiles.label(Language::Es), "Perfiles");
        assert_eq!(View::Profiles.label(Language::En), "Profiles");
    }

    #[test]
    fn default_featured_and_promote_demote_sensors() {
        let mut featured = default_featured_sensors();
        assert_eq!(featured, vec!["cpu", "gpu", "aio"]);
        assert!(is_featured(&featured, "cpu"));
        assert!(is_featured(&featured, "gpu"));
        assert!(is_featured(&featured, "aio"));

        let nvme = sensor("nvme", 42.0);
        let nvme_id = hwmon_sensor_id(&nvme);
        assert!(!is_featured(&featured, &nvme_id));

        promote_sensor(&mut featured, &nvme_id);
        assert!(is_featured(&featured, &nvme_id));
        // Promoting an already-featured sensor does not duplicate it.
        promote_sensor(&mut featured, &nvme_id);
        assert_eq!(featured.iter().filter(|id| *id == &nvme_id).count(), 1);

        demote_sensor(&mut featured, "cpu");
        demote_sensor(&mut featured, "gpu");
        demote_sensor(&mut featured, "aio");
        assert!(!is_featured(&featured, "cpu"));
        assert!(!is_featured(&featured, "gpu"));
        assert!(!is_featured(&featured, "aio"));
        assert_eq!(featured, vec![nvme_id.clone()]);

        promote_sensor(&mut featured, "aio");
        promote_sensor(&mut featured, "cpu");
        demote_sensor(&mut featured, &nvme_id);
        assert_eq!(featured, vec!["aio", "cpu"]);
    }

    #[test]
    fn groups_channels_by_device_preserving_order() {
        let ch = |dev: &str, channel: &str| ChannelInfo {
            device: dev.into(),
            channel: channel.into(),
            label: channel.into(),
            rpm: Some(1200.0),
            duty: Some(50.0),
        };
        let input = vec![
            ch("Kraken X53", "pump"),
            ch("Smart Device V2", "fan1"),
            ch("Kraken X53", "fan"),
            ch("Smart Device V2", "fan2"),
        ];
        let grouped = group_channels_by_device(&input);
        assert_eq!(grouped.len(), 2);
        assert_eq!(grouped[0].0, "Kraken X53");
        assert_eq!(
            grouped[0]
                .1
                .iter()
                .map(|c| c.channel.as_str())
                .collect::<Vec<_>>(),
            vec!["pump", "fan"]
        );
        assert_eq!(grouped[1].0, "Smart Device V2");
        assert_eq!(
            grouped[1]
                .1
                .iter()
                .map(|c| c.channel.as_str())
                .collect::<Vec<_>>(),
            vec!["fan1", "fan2"]
        );
    }

    #[test]
    fn detects_pump_channels() {
        assert!(is_pump_channel("pump"));
        assert!(is_pump_channel("Pump"));
        assert!(is_pump_channel("aio_pump1"));
        assert!(is_pump_channel("bomba"));
        assert!(!is_pump_channel("fan1"));
        assert!(!is_pump_channel("sync"));
    }

    #[test]
    fn capture_current_profile_collects_rgb_cooling_and_curves() {
        let mut rgb_per_channel = HashMap::new();
        rgb_per_channel.insert(
            ("Kraken X53".into(), "ring".into()),
            ChannelRgbState {
                mode: 0,
                color: Color32::from_rgb(0x00, 0xe5, 0xff),
            },
        );
        rgb_per_channel.insert(
            ("Kraken X53".into(), "logo".into()),
            ChannelRgbState {
                mode: 3, // spectrum-wave -> empty colors
                color: Color32::from_rgb(0xff, 0x00, 0xaa),
            },
        );

        let mut manual_duty = HashMap::new();
        manual_duty.insert(("Smart Device V2".into(), "fan1".into()), 64.6);

        let mut device_curves = HashMap::new();
        device_curves.insert("Kraken X53".into(), "silent".into());

        let snap = Snapshot {
            channels: vec![
                ChannelInfo {
                    device: "Kraken X53".into(),
                    channel: "pump".into(),
                    label: "pump".into(),
                    rpm: Some(2200.0),
                    duty: Some(80.0),
                },
                ChannelInfo {
                    device: "Smart Device V2".into(),
                    channel: "fan1".into(),
                    label: "fan1".into(),
                    rpm: Some(1100.0),
                    duty: Some(40.0),
                },
                ChannelInfo {
                    device: "Smart Device V2".into(),
                    channel: "fan2".into(),
                    label: "fan2".into(),
                    rpm: Some(900.0),
                    duty: Some(35.0),
                },
            ],
            ..Snapshot::default()
        };

        let profile =
            capture_profile_from_state(&rgb_per_channel, &manual_duty, &device_curves, &snap);

        assert_eq!(profile.cooling.len(), 3);
        assert_eq!(
            profile.cooling[0],
            CoolingRule {
                match_device: "Kraken X53".into(),
                channel: "pump".into(),
                target: CoolingTarget::Curve {
                    curve: "silent".into(),
                    sensor: None,
                },
            }
        );
        assert_eq!(
            profile.cooling[1],
            CoolingRule {
                match_device: "Smart Device V2".into(),
                channel: "fan1".into(),
                target: CoolingTarget::Fixed { duty: 65 },
            }
        );
        assert_eq!(
            profile.cooling[2],
            CoolingRule {
                match_device: "Smart Device V2".into(),
                channel: "fan2".into(),
                target: CoolingTarget::Fixed { duty: 35 },
            }
        );

        assert_eq!(profile.lighting.len(), 2);
        assert_eq!(
            profile.lighting[0],
            LightingRule {
                match_device: "Kraken X53".into(),
                channel: "logo".into(),
                mode: "spectrum-wave".into(),
                colors: vec![],
            }
        );
        assert_eq!(
            profile.lighting[1],
            LightingRule {
                match_device: "Kraken X53".into(),
                channel: "ring".into(),
                mode: "fixed".into(),
                colors: vec!["00e5ff".into()],
            }
        );
    }

    #[test]
    fn builtin_profiles_cannot_be_deleted() {
        assert!(!is_deletable_profile("default", "default", 4));
        assert!(!is_deletable_profile("performance", "default", 4));
        assert!(!is_deletable_profile("silent", "default", 4));
        assert!(!is_deletable_profile("Silent", "default", 4));
        assert!(!is_deletable_profile("custom", "custom", 4));
        assert!(!is_deletable_profile("custom", "default", 1));
        assert!(is_deletable_profile("custom", "default", 4));
    }
}
