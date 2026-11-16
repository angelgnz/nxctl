mod tui;

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use nxctl_core::config::{
    Config, CoolingRule, CoolingTarget, FanCurve, LightingRule, TempSensorRef,
};
use nxctl_core::engine::{ApplyReport, apply_profile};
use nxctl_core::liquidctl::{CliLiquidctlBackend, LiquidctlBackend};
use nxctl_core::{
    IpcRequest, IpcResponse, default_socket_path, list_hwmon_fans, list_hwmon_sensors,
    list_nvidia_gpus, send_request,
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

fn format_lighting_rule(rule: &LightingRule) -> String {
    if rule.colors.is_empty() {
        format!(
            "{}/{} -> modo {}",
            rule.match_device, rule.channel, rule.mode
        )
    } else {
        format!(
            "{}/{} -> modo {} [{}]",
            rule.match_device,
            rule.channel,
            rule.mode,
            rule.colors.join(", ")
        )
    }
}

fn format_cooling_rule(rule: &CoolingRule) -> String {
    match &rule.target {
        CoolingTarget::Fixed { duty } => {
            format!("{}/{} -> fijo {}%", rule.match_device, rule.channel, duty)
        }
        CoolingTarget::Curve { curve, sensor } => match sensor {
            Some(TempSensorRef::Liquidctl { match_device, key }) => format!(
                "{}/{} -> curva '{curve}' (sensor {match_device}/{key})",
                rule.match_device, rule.channel
            ),
            Some(TempSensorRef::Hwmon {
                hwmon,
                label: Some(label),
            }) => format!(
                "{}/{} -> curva '{curve}' (sensor hwmon:{hwmon}/{label})",
                rule.match_device, rule.channel
            ),
            Some(TempSensorRef::Hwmon { hwmon, label: None }) => format!(
                "{}/{} -> curva '{curve}' (sensor hwmon:{hwmon})",
                rule.match_device, rule.channel
            ),
            Some(TempSensorRef::Nvidia { gpu: Some(idx), .. }) => format!(
                "{}/{} -> curva '{curve}' (sensor nvidia:gpu{idx})",
                rule.match_device, rule.channel
            ),
            Some(TempSensorRef::Nvidia { gpu: None, .. }) => format!(
                "{}/{} -> curva '{curve}' (sensor nvidia)",
                rule.match_device, rule.channel
            ),
            None => format!("{}/{} -> curva '{curve}'", rule.match_device, rule.channel),
        },
    }
}

fn format_curve_summary(name: &str, curve: &FanCurve) -> String {
    let points: Vec<String> = curve
        .points
        .iter()
        .map(|p| format!("{}°C -> {}%", p.temp_c, p.duty))
        .collect();
    format!("{name:<16} {}", points.join(", "))
}

fn format_apply_report(name: &str, report: &ApplyReport) -> String {
    format!(
        "Perfil '{name}' aplicado: {} iluminación, {} refrigeración",
        report.lighting_applied, report.cooling_applied
    )
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
            let path = match cfg_path {
                Some(p) => p.to_path_buf(),
                None => Config::default_path()?,
            };
            if path.exists() && !force {
                bail!(
                    "{} ya existe; usa --force para sobrescribir",
                    path.display()
                );
            }
            let cfg = if detect {
                let devices = backend.status(None).unwrap_or_default();
                let sensors = list_hwmon_sensors().unwrap_or_default();
                let fans = list_hwmon_fans().unwrap_or_default();
                let nvidia = list_nvidia_gpus().unwrap_or_default();
                Config::from_detected(&devices, &sensors, &fans, &nvidia)
            } else {
                Config::starter_nzxt()
            };
            let saved = cfg.save(Some(&path))?;
            println!("Configuración creada en {}", saved.display());
        }
        Command::InitDevices { match_ } => {
            backend.initialize(match_.as_deref(), cli.dry_run)?;
            match match_ {
                Some(pattern) => println!("Dispositivos '{pattern}' inicializados"),
                None => println!("Dispositivos inicializados"),
            }
        }
        Command::Validate => {
            let config = Config::load(cfg_path)?;
            println!(
                "Configuración válida (perfil por defecto: '{}', {} perfiles, {} curvas)",
                config.default_profile,
                config.profiles.len(),
                config.curves.len()
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
            let sensors = list_hwmon_sensors().context("listando sensores hwmon")?;
            let fans = list_hwmon_fans().unwrap_or_default();
            let nvidia_gpus = list_nvidia_gpus().context("listando sensores nvidia")?;
            if json {
                let payload = serde_json::json!({
                    "hwmon": sensors,
                    "hwmon_fans": fans,
                    "nvidia": nvidia_gpus,
                });
                println!("{}", serde_json::to_string_pretty(&payload)?);
            } else if sensors.is_empty() && nvidia_gpus.is_empty() && fans.is_empty() {
                println!("No se encontraron sensores de temperatura.");
            } else {
                if !sensors.is_empty() || !nvidia_gpus.is_empty() {
                    println!(
                        "{:<10} {:<16} {:<24} {:>8}",
                        "DEVICE", "NAME", "LABEL", "TEMP"
                    );
                    for s in &sensors {
                        let label = s.label.as_deref().unwrap_or(&s.input_file);
                        let temp = format!("{:.1} °C", s.temp_c);
                        println!(
                            "{:<10} {:<16} {:<24} {:>8}",
                            s.device_dir, s.name, label, temp
                        );
                    }
                    for gpu in &nvidia_gpus {
                        let device = format!("nvidia{}", gpu.index);
                        let name = format!("GPU {}", gpu.index);
                        let temp = format!("{:.1} °C", gpu.temp_c);
                        println!("{:<10} {:<16} {:<24} {:>8}", device, name, gpu.name, temp);
                    }
                }
                if !fans.is_empty() {
                    if !sensors.is_empty() || !nvidia_gpus.is_empty() {
                        println!();
                    }
                    println!(
                        "{:<10} {:<16} {:<12} {:>10} {:>6}",
                        "DEVICE", "NAME", "CHANNEL", "RPM", "DUTY"
                    );
                    for f in &fans {
                        let rpm = f
                            .rpm
                            .map_or_else(|| "--".to_string(), |r| format!("{r} RPM"));
                        let duty = f.duty.map_or_else(|| "--".to_string(), |d| format!("{d}%"));
                        println!(
                            "{:<10} {:<16} {:<12} {:>10} {:>6}",
                            f.device_dir, f.name, f.channel, rpm, duty
                        );
                    }
                }
            }
        }
        Command::Apply { profile } => {
            let config = Config::load(cfg_path)?;
            let name = profile.unwrap_or_else(|| config.default_profile.clone());
            let report = apply_profile(&backend, &config, &name, cli.dry_run)
                .with_context(|| format!("aplicando perfil '{name}'"))?;
            println!("{}", format_apply_report(&name, &report));
            for warning in &report.warnings {
                eprintln!("Aviso: {warning}");
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
                        println!("Daemon sincronizado con perfil '{name}'");
                    }
                    Ok(IpcResponse::Error { message }) => {
                        eprintln!("Aviso: no se pudo sincronizar el daemon: {message}");
                    }
                    Ok(IpcResponse::Status { .. }) => {}
                    Err(e) => {
                        eprintln!("Aviso: no se pudo sincronizar el daemon: {e}");
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
            backend.set_color(&match_, &channel, &mode, &colors, cli.dry_run)?;
            println!("Color aplicado a {match_}/{channel}");
        }
        Command::Set(SetCmd::Speed {
            match_,
            channel,
            duty,
        }) => {
            if duty > 100 {
                bail!("duty debe estar entre 0 y 100");
            }
            backend.set_speed(&match_, &channel, duty, cli.dry_run)?;
            println!("Velocidad {duty}% aplicada a {match_}/{channel}");
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
            let p = config
                .profiles
                .get(&name)
                .with_context(|| format!("perfil '{name}' no existe"))?;
            println!("Perfil: {name}");
            if let Some(d) = &p.description {
                println!("Descripción: {d}");
            }
            println!("Iluminación:");
            if p.lighting.is_empty() {
                println!("  (sin reglas)");
            } else {
                for r in &p.lighting {
                    println!("  {}", format_lighting_rule(r));
                }
            }
            println!("Refrigeración:");
            if p.cooling.is_empty() {
                println!("  (sin reglas)");
            } else {
                for r in &p.cooling {
                    println!("  {}", format_cooling_rule(r));
                }
            }
        }
        Command::Curves(CurvesCmd::List) => {
            let config = Config::load(cfg_path)?;
            if config.curves.is_empty() {
                println!("No hay curvas configuradas.");
            } else {
                for (name, curve) in &config.curves {
                    println!("{}", format_curve_summary(name, curve));
                }
            }
        }
        Command::Curves(CurvesCmd::Show { name }) => {
            let config = Config::load(cfg_path)?;
            let curve = config
                .curves
                .get(&name)
                .with_context(|| format!("curva '{name}' no existe"))?;
            println!("Curva: {name}");
            for p in &curve.points {
                println!("  {:>5.1}°C -> {:>3}%", p.temp_c, p.duty);
            }
        }
        Command::Tui { socket, interval } => {
            let config = Config::load(cfg_path)?;
            let sock = socket.unwrap_or_else(default_socket_path);
            tui::run(config, cli.dry_run, sock, interval)?;
        }
        Command::Daemon(cmd) => match cmd {
            DaemonCmd::Status { socket, json } => {
                let sock = socket.unwrap_or_else(default_socket_path);
                let resp = send_request(&sock, &IpcRequest::Status)
                    .with_context(|| format!("conectando con nxctld en {}", sock.display()))?;
                match resp {
                    IpcResponse::Status { daemon } => {
                        if json {
                            println!("{}", serde_json::to_string_pretty(&daemon)?);
                        } else {
                            println!("Perfil activo:      {}", daemon.active_profile);
                            println!(
                                "Configuración:      {}",
                                daemon.config_path.as_deref().unwrap_or("(por defecto)")
                            );
                            println!("Intervalo de sondeo: {}s", daemon.poll_interval_secs);
                            println!("Histéresis:         {:.1} °C", daemon.hysteresis_celsius);
                            println!("Delta mínimo duty:  {}%", daemon.min_duty_delta);
                        }
                    }
                    IpcResponse::Error { message } => bail!("{message}"),
                    IpcResponse::Ok { message } => println!("{message}"),
                }
            }
            DaemonCmd::Reload { socket } => {
                let sock = socket.unwrap_or_else(default_socket_path);
                let resp = send_request(&sock, &IpcRequest::Reload)
                    .with_context(|| format!("conectando con nxctld en {}", sock.display()))?;
                match resp {
                    IpcResponse::Ok { message } => println!("{message}"),
                    IpcResponse::Error { message } => bail!("{message}"),
                    IpcResponse::Status { .. } => println!("Configuración recargada"),
                }
            }
            DaemonCmd::Reapply { socket } => {
                let sock = socket.unwrap_or_else(default_socket_path);
                let resp = send_request(&sock, &IpcRequest::Reapply)
                    .with_context(|| format!("conectando con nxctld en {}", sock.display()))?;
                match resp {
                    IpcResponse::Ok { message } => println!("{message}"),
                    IpcResponse::Error { message } => bail!("{message}"),
                    IpcResponse::Status { .. } => println!("Perfil reaplicado"),
                }
            }
            DaemonCmd::Profile { name, socket } => {
                let sock = socket.unwrap_or_else(default_socket_path);
                let resp = send_request(&sock, &IpcRequest::SetProfile { profile: name })
                    .with_context(|| format!("conectando con nxctld en {}", sock.display()))?;
                match resp {
                    IpcResponse::Ok { message } => println!("{message}"),
                    IpcResponse::Error { message } => bail!("{message}"),
                    IpcResponse::Status { .. } => println!("Perfil actualizado"),
                }
            }
        },
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
}
