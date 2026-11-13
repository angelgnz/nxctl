use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use nxctl_core::config::Config;
use nxctl_core::curve::HysteresisController;
use nxctl_core::engine::{apply_profile, step_daemon_tick};
use nxctl_core::liquidctl::{CliLiquidctlBackend, LiquidctlBackend};

#[derive(Parser)]
#[command(name = "nxctld", version, about = "Daemon de control térmico/iluminación NZXT")]
struct Cli {
    #[arg(long)]
    config: Option<PathBuf>,
    /// Perfil a usar (por defecto config.default_profile)
    #[arg(long)]
    profile: Option<String>,
    /// Inicializa los dispositivos con `liquidctl initialize` antes de aplicar el perfil
    #[arg(long)]
    init_devices: bool,
    /// Ejecuta un solo tick y sale
    #[arg(long)]
    once: bool,
    #[arg(long)]
    dry_run: bool,
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let level = match cli.verbose {
        0 => "info",
        1 => "debug",
        _ => "trace",
    };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(level)),
        )
        .init();

    let config = Config::load(cli.config.as_deref())?;
    let profile = cli.profile.unwrap_or_else(|| config.default_profile.clone());
    let backend = CliLiquidctlBackend::new();
    let mut controller = HysteresisController::new(
        config.daemon.hysteresis_celsius,
        config.daemon.min_duty_delta,
    );

    let running = Arc::new(AtomicBool::new(true));
    {
        let r = running.clone();
        ctrlc::set_handler(move || r.store(false, Ordering::SeqCst))
            .context("instalando manejador de señales")?;
    }

    if cli.init_devices {
        backend
            .initialize(None, cli.dry_run)
            .context("inicializando dispositivos vía liquidctl")?;
        tracing::info!("dispositivos inicializados");
    }

    let report = apply_profile(&backend, &config, &profile, cli.dry_run)
        .with_context(|| format!("aplicando perfil inicial '{profile}'"))?;
    tracing::info!(?report, "perfil '{profile}' aplicado");

    let interval = Duration::from_secs(config.daemon.poll_interval_secs);
    while running.load(Ordering::SeqCst) {
        match step_daemon_tick(&backend, &config, &profile, &mut controller, cli.dry_run) {
            Ok(changes) => tracing::debug!(?changes, "tick"),
            Err(e) => tracing::warn!("error en tick: {e}"),
        }
        if cli.once {
            break;
        }
        // Dormir en tramos cortos para responder rápido a SIGINT/SIGTERM.
        let mut left = interval;
        let step = Duration::from_millis(250);
        while running.load(Ordering::SeqCst) && !left.is_zero() {
            let d = left.min(step);
            std::thread::sleep(d);
            left -= d;
        }
    }
    tracing::info!("nxctld detenido");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_parses_init_devices_and_runtime_flags() {
        let cli = Cli::try_parse_from([
            "nxctld",
            "--init-devices",
            "--once",
            "--dry-run",
            "--profile",
            "gaming",
            "-vv",
        ])
        .unwrap();

        assert!(cli.init_devices);
        assert!(cli.once);
        assert!(cli.dry_run);
        assert_eq!(cli.profile.as_deref(), Some("gaming"));
        assert_eq!(cli.verbose, 2);
    }

    #[test]
    fn cli_defaults_have_init_devices_disabled() {
        let cli = Cli::try_parse_from(["nxctld"]).unwrap();
        assert!(!cli.init_devices);
        assert!(!cli.once);
        assert!(!cli.dry_run);
        assert_eq!(cli.verbose, 0);
    }
}
