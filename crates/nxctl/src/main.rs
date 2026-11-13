use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use opennx_core::config::Config;
use opennx_core::engine::apply_profile;
use opennx_core::liquidctl::{CliLiquidctlBackend, LiquidctlBackend};

#[derive(Parser)]
#[command(name = "nxctl", version, about = "Control de dispositivos NZXT vía liquidctl")]
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
    },
    /// Muestra el estado de los dispositivos
    Status {
        #[arg(long = "match")]
        match_: Option<String>,
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
        Command::Init { force } => {
            let path = match cfg_path {
                Some(p) => p.to_path_buf(),
                None => Config::default_path()?,
            };
            if path.exists() && !force {
                bail!("{} ya existe; usa --force para sobrescribir", path.display());
            }
            let saved = Config::starter_nzxt().save(Some(&path))?;
            println!("Configuración creada en {}", saved.display());
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
                        println!("  {:<28} {} {}", r.key, value, r.unit.as_deref().unwrap_or(""));
                    }
                }
            }
        }
        Command::Apply { profile } => {
            let config = Config::load(cfg_path)?;
            let name = profile.unwrap_or_else(|| config.default_profile.clone());
            let report = apply_profile(&backend, &config, &name, cli.dry_run)
                .with_context(|| format!("aplicando perfil '{name}'"))?;
            println!("Perfil '{name}' aplicado: {report:?}");
        }
        Command::Set(SetCmd::Color { match_, channel, mode, colors }) => {
            backend.set_color(&match_, &channel, &mode, &colors, cli.dry_run)?;
            println!("Color aplicado a {match_}/{channel}");
        }
        Command::Set(SetCmd::Speed { match_, channel, duty }) => {
            if duty > 100 {
                bail!("duty debe estar entre 0 y 100");
            }
            backend.set_speed(&match_, &channel, duty, cli.dry_run)?;
            println!("Velocidad {duty}% aplicada a {match_}/{channel}");
        }
        Command::Profiles(ProfilesCmd::List) => {
            let config = Config::load(cfg_path)?;
            for (name, p) in &config.profiles {
                let marker = if *name == config.default_profile { "*" } else { " " };
                println!("{marker} {name:<16} {}", p.description.as_deref().unwrap_or(""));
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
            for r in &p.lighting {
                println!("  {r:?}");
            }
            println!("Refrigeración:");
            for r in &p.cooling {
                println!("  {r:?}");
            }
        }
    }
    Ok(())
}
