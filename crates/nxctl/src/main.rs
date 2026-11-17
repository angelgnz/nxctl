mod i18n;
mod tui;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use nxctl_core::config::{
    Config, CoolingRule, CoolingTarget, FanCurve, LightingRule, TempSensorRef,
};
use nxctl_core::engine::{ApplyReport, apply_profile};
use nxctl_core::liquidctl::{CliLiquidctlBackend, LiquidctlBackend};
use nxctl_core::{
    IpcRequest, IpcResponse, Language, apply_pywal_palette, default_socket_path, list_hwmon_fans,
    list_hwmon_sensors, list_nvidia_gpus, load_pywal_palette, send_request,
};

#[derive(Parser)]
#[command(
    name = "nxctl",
    version,
    about = "Control de dispositivos NZXT vía liquidctl"
)]
struct Cli {
    /// Ruta del archivo de configuración
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    /// No ejecuta cambios reales
    #[arg(long, global = true)]
    dry_run: bool,
    /// Aumenta la verbosidad (-v, -vv)
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,
    /// Idioma de salida (es, en)
    #[arg(long, global = true)]
    lang: Option<Language>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Crea la configuración inicial
    Init {
        #[arg(long)]
        force: bool,
        /// Detecta dispositivos liquidctl, sensores/PWM hwmon y GPUs NVIDIA automáticamente
        #[arg(long)]
        detect: bool,
    },
    /// Inicializa los dispositivos vía liquidctl
    InitDevices {
        #[arg(long = "match")]
        match_: Option<String>,
    },
    /// Valida el archivo de configuración sin aplicar cambios
    Validate,
    /// Muestra el estado de los dispositivos
    Status {
        #[arg(long = "match")]
        match_: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Lista los sensores de temperatura del sistema (hwmon)
    Sensors {
        #[arg(long)]
        json: bool,
    },
    /// Aplica un perfil (por defecto config.default_profile)
    Apply { profile: Option<String> },
    /// Ajustes manuales
    #[command(subcommand)]
    Set(SetCmd),
    /// Gestión de perfiles
    #[command(subcommand)]
    Profiles(ProfilesCmd),
    /// Inspección de curvas de ventilación
    #[command(subcommand)]
    Curves(CurvesCmd),
    /// Control del daemon nxctld en ejecución
    #[command(subcommand)]
    Daemon(DaemonCmd),
    /// Interfaz de terminal interactiva (TUI)
    Tui {
        #[arg(long)]
        socket: Option<PathBuf>,
        /// Intervalo de refresco en segundos
        #[arg(long, default_value_t = 2)]
        interval: u64,
    },
    /// Inspecciona o aplica la paleta de colores de pywal
    Pywal {
        /// Aplica la paleta a los dispositivos según la configuración
        #[arg(long)]
        apply: bool,
        /// Muestra la paleta en formato JSON
        #[arg(long)]
        json: bool,
        /// Ruta al archivo colors.json de pywal
        #[arg(long)]
        path: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum SetCmd {
    /// Cambia color/modo de un canal
    Color {
        #[arg(name = "MATCH")]
        match_: String,
        channel: String,
        mode: String,
        #[arg(required = true, num_args = 1..)]
        colors: Vec<String>,
    },
    /// Cambia la velocidad (0-100) de un canal
    Speed {
        #[arg(name = "MATCH")]
        match_: String,
        channel: String,
        duty: u8,
    },
}

#[derive(Subcommand)]
enum ProfilesCmd {
    List,
    Show { name: String },
}

#[derive(Subcommand)]
enum CurvesCmd {
    List,
    Show { name: String },
}

#[derive(Subcommand)]
enum DaemonCmd {
    /// Muestra el estado actual del daemon
    Status {
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Recarga la configuración del daemon
    Reload {
        #[arg(long)]
        socket: Option<PathBuf>,
    },
    /// Reaplica el perfil activo del daemon (útil tras suspensión)
    Reapply {
        #[arg(long)]
        socket: Option<PathBuf>,
    },
    /// Cambia el perfil activo del daemon
    Profile {
        name: String,
        #[arg(long)]
        socket: Option<PathBuf>,
    },
}

fn format_lighting_rule_i18n(rule: &LightingRule, lang: Language) -> String {
    i18n::fmt_lighting_rule(
        lang,
        &rule.match_device,
        &rule.channel,
        &rule.mode,
        &rule.colors,
    )
}

#[cfg_attr(not(test), allow(dead_code))]
fn format_lighting_rule(rule: &LightingRule) -> String {
    format_lighting_rule_i18n(rule, Language::Es)
}

fn format_cooling_rule_i18n(rule: &CoolingRule, lang: Language) -> String {
    match &rule.target {
        CoolingTarget::Fixed { duty } => {
            i18n::fmt_cooling_fixed(lang, &rule.match_device, &rule.channel, *duty)
        }
        CoolingTarget::Curve { curve, sensor } => {
            let sensor_desc = sensor.as_ref().map(|s| match s {
                TempSensorRef::Liquidctl { match_device, key } => format!("{match_device}/{key}"),
                TempSensorRef::Hwmon {
                    hwmon,
                    label: Some(label),
                } => format!("hwmon:{hwmon}/{label}"),
                TempSensorRef::Hwmon { hwmon, label: None } => format!("hwmon:{hwmon}"),
                TempSensorRef::Nvidia { gpu: Some(idx), .. } => format!("nvidia:gpu{idx}"),
                TempSensorRef::Nvidia { gpu: None, .. } => "nvidia".to_string(),
            });
            i18n::fmt_cooling_curve(
                lang,
                &rule.match_device,
                &rule.channel,
                curve,
                sensor_desc.as_deref(),
            )
        }
    }
}

#[cfg_attr(not(test), allow(dead_code))]
fn format_cooling_rule(rule: &CoolingRule) -> String {
    format_cooling_rule_i18n(rule, Language::Es)
}

fn format_curve_summary(name: &str, curve: &FanCurve) -> String {
    let points: Vec<String> = curve
        .points
        .iter()
        .map(|p| format!("{}°C -> {}%", p.temp_c, p.duty))
        .collect();
    format!("{name:<16} {}", points.join(", "))
}

fn format_apply_report_i18n(name: &str, report: &ApplyReport, lang: Language) -> String {
    i18n::fmt_apply_report(lang, name, report.lighting_applied, report.cooling_applied)
}

#[cfg_attr(not(test), allow(dead_code))]
fn format_apply_report(name: &str, report: &ApplyReport) -> String {
    format_apply_report_i18n(name, report, Language::Es)
}

fn resolve_lang(cli_lang: Option<Language>, cfg_path: Option<&Path>) -> Language {
    cli_lang
        .or_else(|| Config::load(cfg_path).ok().map(|c| c.language))
        .unwrap_or_else(Language::detect_from_env)
}

fn init_tracing(verbose: u8) {
    let level = match verbose {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(level));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    init_tracing(cli.verbose);
    let cfg_path = cli.config.as_deref();
    let backend = CliLiquidctlBackend::new();

    match cli.command {
        Command::Init { force, detect } => {
            let lang = cli.lang.unwrap_or_else(Language::detect_from_env);
            let path = match cfg_path {
                Some(p) => p.to_path_buf(),
                None => Config::default_path()?,
            };
            if path.exists() && !force {
                bail!("{}", i18n::fmt_config_exists(lang, &path));
            }
            let mut cfg = if detect {
                let devices = backend.status(None).unwrap_or_default();
                let sensors = list_hwmon_sensors().unwrap_or_default();
                let fans = list_hwmon_fans().unwrap_or_default();
                let nvidia = list_nvidia_gpus().unwrap_or_default();
                Config::from_detected(&devices, &sensors, &fans, &nvidia)
            } else {
                Config::starter_nzxt()
            };
            if let Some(l) = cli.lang {
                cfg.language = l;
            }
            let saved = cfg.save(Some(&path))?;
            println!("{}", i18n::fmt_config_created(lang, &saved));
        }
        Command::InitDevices { match_ } => {
            let lang = resolve_lang(cli.lang, cfg_path);
            let s = i18n::cli(lang);
            backend.initialize(match_.as_deref(), cli.dry_run)?;
            match match_ {
                Some(pattern) => {
                    println!("{}", i18n::fmt_devices_initialized_match(lang, &pattern))
                }
                None => println!("{}", s.devices_initialized),
            }
        }
        Command::Validate => {
            let config = Config::load(cfg_path)?;
            let lang = cli.lang.unwrap_or(config.language);
            println!(
                "{}",
                i18n::fmt_config_valid(
                    lang,
                    &config.default_profile,
                    config.profiles.len(),
                    config.curves.len()
                )
            );
        }
        Command::Status { match_, json } => {
            let devices = backend.status(match_.as_deref())?;
            if json {
                println!("{}", serde_json::to_string_pretty(&devices)?);
            } else {
                for d in &devices {
                    println!("{}", d.description);
                    for r in &d.status {
                        let value = match &r.value {
                            serde_json::Value::String(s) => s.clone(),
                            v => v.to_string(),
                        };
                        println!(
                            "  {:<28} {} {}",
                            r.key,
                            value,
                            r.unit.as_deref().unwrap_or("")
                        );
                    }
                }
            }
        }
        Command::Sensors { json } => {
            let lang = resolve_lang(cli.lang, cfg_path);
            let s = i18n::cli(lang);
            let sensors = list_hwmon_sensors().context(s.err_listing_hwmon)?;
            let fans = list_hwmon_fans().unwrap_or_default();
            let nvidia_gpus = list_nvidia_gpus().context(s.err_listing_nvidia)?;
            if json {
                let payload = serde_json::json!({
                    "hwmon": sensors,
                    "hwmon_fans": fans,
                    "nvidia": nvidia_gpus,
                });
                println!("{}", serde_json::to_string_pretty(&payload)?);
            } else if sensors.is_empty() && nvidia_gpus.is_empty() && fans.is_empty() {
                println!("{}", s.no_temp_sensors);
            } else {
                if !sensors.is_empty() || !nvidia_gpus.is_empty() {
                    let [h_dev, h_name, h_label, h_temp] = s.sensors_temp_headers;
                    println!("{h_dev:<12} {h_name:<16} {h_label:<24} {h_temp:>8}");
                    for sensor in &sensors {
                        let label = sensor.label.as_deref().unwrap_or(&sensor.input_file);
                        let temp = format!("{:.1} °C", sensor.temp_c);
                        println!(
                            "{:<12} {:<16} {:<24} {:>8}",
                            sensor.device_dir, sensor.name, label, temp
                        );
                    }
                    for gpu in &nvidia_gpus {
                        let device = format!("nvidia{}", gpu.index);
                        let name = format!("GPU {}", gpu.index);
                        let temp = format!("{:.1} °C", gpu.temp_c);
                        println!("{device:<12} {name:<16} {:<24} {temp:>8}", gpu.name);
                    }
                }
                if !fans.is_empty() {
                    if !sensors.is_empty() || !nvidia_gpus.is_empty() {
                        println!();
                    }
                    let [h_dev, h_name, h_chan, h_rpm, h_duty] = s.sensors_fan_headers;
                    println!("{h_dev:<12} {h_name:<16} {h_chan:<12} {h_rpm:>10} {h_duty:>6}");
                    for f in &fans {
                        let rpm = f
                            .rpm
                            .map_or_else(|| "--".to_string(), |r| format!("{r} RPM"));
                        let duty = f.duty.map_or_else(|| "--".to_string(), |d| format!("{d}%"));
                        println!(
                            "{:<12} {:<16} {:<12} {:>10} {:>6}",
                            f.device_dir, f.name, f.channel, rpm, duty
                        );
                    }
                }
            }
        }
        Command::Apply { profile } => {
            let config = Config::load(cfg_path)?;
            let lang = cli.lang.unwrap_or(config.language);
            let s = i18n::cli(lang);
            let name = profile.unwrap_or_else(|| config.default_profile.clone());
            let report = apply_profile(&backend, &config, &name, cli.dry_run)
                .with_context(|| i18n::fmt_applying_profile(lang, &name))?;
            println!("{}", format_apply_report_i18n(&name, &report, lang));
            for warning in &report.warnings {
                eprintln!("{}: {warning}", s.warning_prefix);
            }
            let sock = default_socket_path();
            if !cli.dry_run && sock.exists() {
                match send_request(
                    &sock,
                    &IpcRequest::SetProfile {
                        profile: name.clone(),
                    },
                ) {
                    Ok(IpcResponse::Ok { .. }) => {
                        println!("{}", i18n::fmt_daemon_synced(lang, &name));
                    }
                    Ok(IpcResponse::Error { message }) => {
                        eprintln!("{}", i18n::fmt_daemon_sync_failed(lang, &message));
                    }
                    Ok(IpcResponse::Status { .. }) => {}
                    Err(e) => {
                        eprintln!("{}", i18n::fmt_daemon_sync_failed(lang, &e));
                    }
                }
            }
        }
        Command::Set(SetCmd::Color {
            match_,
            channel,
            mode,
            colors,
        }) => {
            let lang = resolve_lang(cli.lang, cfg_path);
            backend.set_color(&match_, &channel, &mode, &colors, cli.dry_run)?;
            println!("{}", i18n::fmt_color_applied(lang, &match_, &channel));
        }
        Command::Set(SetCmd::Speed {
            match_,
            channel,
            duty,
        }) => {
            let lang = resolve_lang(cli.lang, cfg_path);
            let s = i18n::cli(lang);
            if duty > 100 {
                bail!("{}", s.err_duty_range);
            }
            backend.set_speed(&match_, &channel, duty, cli.dry_run)?;
            println!("{}", i18n::fmt_speed_applied(lang, duty, &match_, &channel));
        }
        Command::Profiles(ProfilesCmd::List) => {
            let config = Config::load(cfg_path)?;
            for (name, p) in &config.profiles {
                let marker = if *name == config.default_profile {
                    "*"
                } else {
                    " "
                };
                println!(
                    "{marker} {name:<16} {}",
                    p.description.as_deref().unwrap_or("")
                );
            }
        }
        Command::Profiles(ProfilesCmd::Show { name }) => {
            let config = Config::load(cfg_path)?;
            let lang = cli.lang.unwrap_or(config.language);
            let s = i18n::cli(lang);
            let p = config
                .profiles
                .get(&name)
                .with_context(|| i18n::fmt_profile_not_found(lang, &name))?;
            println!("{}", i18n::fmt_profile_header(lang, &name));
            if let Some(d) = &p.description {
                println!("{}", i18n::fmt_profile_description(lang, d));
            }
            println!("{}", s.label_lighting);
            if p.lighting.is_empty() {
                println!("{}", s.no_rules);
            } else {
                for r in &p.lighting {
                    println!("  {}", format_lighting_rule_i18n(r, lang));
                }
            }
            println!("{}", s.label_cooling);
            if p.cooling.is_empty() {
                println!("{}", s.no_rules);
            } else {
                for r in &p.cooling {
                    println!("  {}", format_cooling_rule_i18n(r, lang));
                }
            }
        }
        Command::Curves(CurvesCmd::List) => {
            let config = Config::load(cfg_path)?;
            let lang = cli.lang.unwrap_or(config.language);
            let s = i18n::cli(lang);
            if config.curves.is_empty() {
                println!("{}", s.no_curves);
            } else {
                for (name, curve) in &config.curves {
                    println!("{}", format_curve_summary(name, curve));
                }
            }
        }
        Command::Curves(CurvesCmd::Show { name }) => {
            let config = Config::load(cfg_path)?;
            let lang = cli.lang.unwrap_or(config.language);
            let curve = config
                .curves
                .get(&name)
                .with_context(|| i18n::fmt_curve_not_found(lang, &name))?;
            println!("{}", i18n::fmt_curve_header(lang, &name));
            for p in &curve.points {
                println!("  {:>5.1}°C -> {:>3}%", p.temp_c, p.duty);
            }
        }
        Command::Tui { socket, interval } => {
            let mut config = Config::load(cfg_path)?;
            if let Some(l) = cli.lang {
                config.language = l;
            }
            let sock = socket.unwrap_or_else(default_socket_path);
            tui::run(config, cli.dry_run, sock, interval)?;
        }
        Command::Daemon(cmd) => {
            let lang = resolve_lang(cli.lang, cfg_path);
            let s = i18n::cli(lang);
            match cmd {
                DaemonCmd::Status { socket, json } => {
                    let sock = socket.unwrap_or_else(default_socket_path);
                    let resp = send_request(&sock, &IpcRequest::Status)
                        .with_context(|| i18n::fmt_connecting_daemon(lang, &sock))?;
                    match resp {
                        IpcResponse::Status { daemon } => {
                            if json {
                                println!("{}", serde_json::to_string_pretty(&daemon)?);
                            } else {
                                for line in i18n::fmt_daemon_status_lines(lang, &daemon) {
                                    println!("{line}");
                                }
                            }
                        }
                        IpcResponse::Error { message } => bail!("{message}"),
                        IpcResponse::Ok { message } => println!("{message}"),
                    }
                }
                DaemonCmd::Reload { socket } => {
                    let sock = socket.unwrap_or_else(default_socket_path);
                    let resp = send_request(&sock, &IpcRequest::Reload)
                        .with_context(|| i18n::fmt_connecting_daemon(lang, &sock))?;
                    match resp {
                        IpcResponse::Ok { message } => println!("{message}"),
                        IpcResponse::Error { message } => bail!("{message}"),
                        IpcResponse::Status { .. } => println!("{}", s.daemon_reloaded),
                    }
                }
                DaemonCmd::Reapply { socket } => {
                    let sock = socket.unwrap_or_else(default_socket_path);
                    let resp = send_request(&sock, &IpcRequest::Reapply)
                        .with_context(|| i18n::fmt_connecting_daemon(lang, &sock))?;
                    match resp {
                        IpcResponse::Ok { message } => println!("{message}"),
                        IpcResponse::Error { message } => bail!("{message}"),
                        IpcResponse::Status { .. } => println!("{}", s.daemon_reapplied),
                    }
                }
                DaemonCmd::Profile { name, socket } => {
                    let sock = socket.unwrap_or_else(default_socket_path);
                    let resp = send_request(&sock, &IpcRequest::SetProfile { profile: name })
                        .with_context(|| i18n::fmt_connecting_daemon(lang, &sock))?;
                    match resp {
                        IpcResponse::Ok { message } => println!("{message}"),
                        IpcResponse::Error { message } => bail!("{message}"),
                        IpcResponse::Status { .. } => println!("{}", s.daemon_profile_updated),
                    }
                }
            }
        }
        Command::Pywal { apply, json, path } => {
            let lang = resolve_lang(cli.lang, cfg_path);
            let s = i18n::cli(lang);
            let palette = load_pywal_palette(path.as_deref()).context(s.err_loading_pywal)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&palette)?);
            } else {
                if let Some(wp) = &palette.wallpaper {
                    println!("Wallpaper: {wp}");
                }
                for i in 0..16 {
                    let key = format!("color{i}");
                    if let Some(hex) = palette.color_hex(&key) {
                        println!("{key}: #{hex}");
                    }
                }
            }
            if apply {
                let config = Config::load(cfg_path).unwrap_or_else(|_| Config::starter_nzxt());
                let applied = apply_pywal_palette(&backend, &config.pywal, &palette, cli.dry_run)?;
                if !json {
                    println!("{}", i18n::fmt_pywal_applied(lang, applied));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nxctl_core::config::{CurvePoint, TempSensorRef};

    #[test]
    fn cli_parses_new_subcommands() {
        let validate = Cli::try_parse_from(["nxctl", "--dry-run", "validate"]).unwrap();
        assert!(validate.dry_run);
        assert!(matches!(validate.command, Command::Validate));

        let init_detect = Cli::try_parse_from(["nxctl", "init", "--detect"]).unwrap();
        assert!(matches!(
            init_detect.command,
            Command::Init {
                force: false,
                detect: true
            }
        ));

        let pywal_apply = Cli::try_parse_from(["nxctl", "pywal", "--apply"]).unwrap();
        assert!(matches!(
            pywal_apply.command,
            Command::Pywal {
                apply: true,
                json: false,
                path: None
            }
        ));

        let init_dev = Cli::try_parse_from(["nxctl", "init-devices", "--match", "kraken"]).unwrap();
        assert!(matches!(
            init_dev.command,
            Command::InitDevices { match_: Some(ref m) } if m == "kraken"
        ));

        let curves_list = Cli::try_parse_from(["nxctl", "curves", "list"]).unwrap();
        assert!(matches!(
            curves_list.command,
            Command::Curves(CurvesCmd::List)
        ));

        let curves_show = Cli::try_parse_from(["nxctl", "curves", "show", "silent"]).unwrap();
        assert!(matches!(
            curves_show.command,
            Command::Curves(CurvesCmd::Show { ref name }) if name == "silent"
        ));

        let sensors_json = Cli::try_parse_from(["nxctl", "sensors", "--json"]).unwrap();
        assert!(matches!(
            sensors_json.command,
            Command::Sensors { json: true }
        ));

        let tui_cmd = Cli::try_parse_from(["nxctl", "tui", "--interval", "5"]).unwrap();
        assert!(matches!(
            tui_cmd.command,
            Command::Tui {
                socket: None,
                interval: 5
            }
        ));

        let sensors = Cli::try_parse_from(["nxctl", "sensors"]).unwrap();
        assert!(matches!(sensors.command, Command::Sensors { json: false }));

        let daemon_status = Cli::try_parse_from([
            "nxctl",
            "daemon",
            "status",
            "--socket",
            "/tmp/test.sock",
            "--json",
        ])
        .unwrap();
        assert!(matches!(
            daemon_status.command,
            Command::Daemon(DaemonCmd::Status {
                socket: Some(ref p),
                json: true
            }) if p == std::path::Path::new("/tmp/test.sock")
        ));

        let daemon_reload = Cli::try_parse_from(["nxctl", "daemon", "reload"]).unwrap();
        assert!(matches!(
            daemon_reload.command,
            Command::Daemon(DaemonCmd::Reload { socket: None })
        ));

        let daemon_reapply = Cli::try_parse_from(["nxctl", "daemon", "reapply"]).unwrap();
        assert!(matches!(
            daemon_reapply.command,
            Command::Daemon(DaemonCmd::Reapply { socket: None })
        ));

        let daemon_reapply_sock =
            Cli::try_parse_from(["nxctl", "daemon", "reapply", "--socket", "/tmp/test.sock"])
                .unwrap();
        assert!(matches!(
            daemon_reapply_sock.command,
            Command::Daemon(DaemonCmd::Reapply { socket: Some(ref p) })
                if p == std::path::Path::new("/tmp/test.sock")
        ));

        let daemon_profile = Cli::try_parse_from([
            "nxctl",
            "daemon",
            "profile",
            "performance",
            "--socket",
            "/tmp/test.sock",
        ])
        .unwrap();
        assert!(matches!(
            daemon_profile.command,
            Command::Daemon(DaemonCmd::Profile {
                ref name,
                socket: Some(ref p)
            }) if name == "performance" && p == std::path::Path::new("/tmp/test.sock")
        ));
    }

    #[test]
    fn formatting_helpers_produce_human_readable_output() {
        let lighting = LightingRule {
            match_device: "kraken".into(),
            channel: "ring".into(),
            mode: "fixed".into(),
            colors: vec!["ff0000".into()],
        };
        assert_eq!(
            format_lighting_rule(&lighting),
            "kraken/ring -> modo fixed [ff0000]"
        );

        let cooling_fixed = CoolingRule {
            match_device: "kraken".into(),
            channel: "pump".into(),
            target: CoolingTarget::Fixed { duty: 75 },
        };
        assert_eq!(
            format_cooling_rule(&cooling_fixed),
            "kraken/pump -> fijo 75%"
        );

        let cooling_curve = CoolingRule {
            match_device: "smart".into(),
            channel: "fan1".into(),
            target: CoolingTarget::Curve {
                curve: "quiet".into(),
                sensor: Some(TempSensorRef::Liquidctl {
                    match_device: "kraken".into(),
                    key: "Liquid temperature".into(),
                }),
            },
        };
        assert_eq!(
            format_cooling_rule(&cooling_curve),
            "smart/fan1 -> curva 'quiet' (sensor kraken/Liquid temperature)"
        );

        let cooling_hwmon = CoolingRule {
            match_device: "smart".into(),
            channel: "fan2".into(),
            target: CoolingTarget::Curve {
                curve: "perf".into(),
                sensor: Some(TempSensorRef::Hwmon {
                    hwmon: "k10temp".into(),
                    label: Some("Tctl".into()),
                }),
            },
        };
        assert_eq!(
            format_cooling_rule(&cooling_hwmon),
            "smart/fan2 -> curva 'perf' (sensor hwmon:k10temp/Tctl)"
        );

        let cooling_hwmon_nolabel = CoolingRule {
            match_device: "smart".into(),
            channel: "fan3".into(),
            target: CoolingTarget::Curve {
                curve: "perf".into(),
                sensor: Some(TempSensorRef::Hwmon {
                    hwmon: "amdgpu".into(),
                    label: None,
                }),
            },
        };
        assert_eq!(
            format_cooling_rule(&cooling_hwmon_nolabel),
            "smart/fan3 -> curva 'perf' (sensor hwmon:amdgpu)"
        );

        let cooling_nvidia = CoolingRule {
            match_device: "smart".into(),
            channel: "fan4".into(),
            target: CoolingTarget::Curve {
                curve: "perf".into(),
                sensor: Some(TempSensorRef::Nvidia {
                    nvidia: true,
                    gpu: Some(0),
                }),
            },
        };
        assert_eq!(
            format_cooling_rule(&cooling_nvidia),
            "smart/fan4 -> curva 'perf' (sensor nvidia:gpu0)"
        );

        let cooling_nvidia_default = CoolingRule {
            match_device: "smart".into(),
            channel: "fan5".into(),
            target: CoolingTarget::Curve {
                curve: "perf".into(),
                sensor: Some(TempSensorRef::Nvidia {
                    nvidia: true,
                    gpu: None,
                }),
            },
        };
        assert_eq!(
            format_cooling_rule(&cooling_nvidia_default),
            "smart/fan5 -> curva 'perf' (sensor nvidia)"
        );

        let curve = FanCurve {
            points: vec![
                CurvePoint {
                    temp_c: 30.0,
                    duty: 40,
                },
                CurvePoint {
                    temp_c: 50.0,
                    duty: 100,
                },
            ],
        };
        assert!(format_curve_summary("quiet", &curve).contains("30°C -> 40%, 50°C -> 100%"));

        let report = ApplyReport {
            lighting_applied: 2,
            cooling_applied: 3,
            warnings: vec![],
        };
        assert_eq!(
            format_apply_report("default", &report),
            "Perfil 'default' aplicado: 2 iluminación, 3 refrigeración"
        );
    }

    #[test]
    fn cli_parses_lang_flag() {
        let en = Cli::try_parse_from(["nxctl", "--lang", "en", "validate"]).unwrap();
        assert_eq!(en.lang, Some(Language::En));
        assert!(matches!(en.command, Command::Validate));

        let es_after = Cli::try_parse_from(["nxctl", "status", "--lang", "es"]).unwrap();
        assert_eq!(es_after.lang, Some(Language::Es));

        let default = Cli::try_parse_from(["nxctl", "validate"]).unwrap();
        assert_eq!(default.lang, None);
    }

    #[test]
    fn formatting_helpers_english_i18n() {
        let lighting = LightingRule {
            match_device: "kraken".into(),
            channel: "ring".into(),
            mode: "fixed".into(),
            colors: vec!["ff0000".into()],
        };
        assert_eq!(
            format_lighting_rule_i18n(&lighting, Language::En),
            "kraken/ring -> mode fixed [ff0000]"
        );

        let cooling_fixed = CoolingRule {
            match_device: "kraken".into(),
            channel: "pump".into(),
            target: CoolingTarget::Fixed { duty: 75 },
        };
        assert_eq!(
            format_cooling_rule_i18n(&cooling_fixed, Language::En),
            "kraken/pump -> fixed 75%"
        );

        let cooling_curve = CoolingRule {
            match_device: "smart".into(),
            channel: "fan1".into(),
            target: CoolingTarget::Curve {
                curve: "quiet".into(),
                sensor: Some(TempSensorRef::Liquidctl {
                    match_device: "kraken".into(),
                    key: "Liquid temperature".into(),
                }),
            },
        };
        assert_eq!(
            format_cooling_rule_i18n(&cooling_curve, Language::En),
            "smart/fan1 -> curve 'quiet' (sensor kraken/Liquid temperature)"
        );

        let report = ApplyReport {
            lighting_applied: 2,
            cooling_applied: 3,
            warnings: vec![],
        };
        assert_eq!(
            format_apply_report_i18n("default", &report, Language::En),
            "Profile 'default' applied: 2 lighting, 3 cooling"
        );
    }
}
