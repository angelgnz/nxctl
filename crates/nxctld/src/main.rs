use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::Parser;
use nxctl_core::config::Config;
use nxctl_core::curve::HysteresisController;
use nxctl_core::engine::{ApplyReport, apply_profile, step_daemon_tick};
use nxctl_core::liquidctl::{CliLiquidctlBackend, LiquidctlBackend};
use nxctl_core::{
    DaemonStatus, IpcRequest, IpcResponse, ResumeDetector, apply_pywal_palette,
    default_colors_json_path, default_socket_path, load_pywal_palette,
};
use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM, SIGUSR1};

const POLL_SLEEP_STEP: Duration = Duration::from_millis(100);
const IPC_IO_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Parser)]
#[command(
    name = "nxctld",
    version,
    about = "Daemon de control térmico/iluminación NZXT"
)]
struct Cli {
    #[arg(long)]
    config: Option<PathBuf>,
    /// Perfil a usar (por defecto config.default_profile)
    #[arg(long)]
    profile: Option<String>,
    /// Ruta del socket Unix para IPC (por defecto $XDG_RUNTIME_DIR/nxctld.sock)
    #[arg(long)]
    socket: Option<PathBuf>,
    /// Inicializa los dispositivos con `liquidctl initialize` antes de aplicar el perfil
    #[arg(long)]
    init_devices: bool,
    /// Re-inicializa los dispositivos con `liquidctl initialize` al reanudar tras suspensión
    #[arg(long)]
    init_on_resume: bool,
    /// Ejecuta un solo tick y sale
    #[arg(long)]
    once: bool,
    #[arg(long)]
    dry_run: bool,
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
}

type IpcJob = (IpcRequest, mpsc::Sender<IpcResponse>);

struct DaemonState {
    config: Config,
    config_path: Option<PathBuf>,
    active_profile: String,
    controller: HysteresisController,
    interval: Duration,
    dry_run: bool,
    cli_init_on_resume: bool,
    last_pywal_mtime: Option<std::time::SystemTime>,
}

impl DaemonState {
    fn new(
        config: Config,
        config_path: Option<PathBuf>,
        active_profile: String,
        dry_run: bool,
    ) -> Self {
        let controller = HysteresisController::new(
            config.daemon.hysteresis_celsius,
            config.daemon.min_duty_delta,
        );
        let interval = Duration::from_secs(config.daemon.poll_interval_secs);
        Self {
            config,
            config_path,
            active_profile,
            controller,
            interval,
            dry_run,
            cli_init_on_resume: false,
            last_pywal_mtime: None,
        }
    }

    fn with_init_on_resume(mut self, init_on_resume: bool) -> Self {
        self.cli_init_on_resume = init_on_resume;
        self
    }

    fn should_init_on_resume(&self) -> bool {
        self.cli_init_on_resume || self.config.daemon.init_on_resume
    }

    fn reapply(
        &mut self,
        backend: &dyn LiquidctlBackend,
        init_devices: bool,
    ) -> Result<ApplyReport> {
        if init_devices {
            backend
                .initialize(None, self.dry_run)
                .context("inicializando dispositivos vía liquidctl tras reanudación")?;
            tracing::info!("dispositivos re-inicializados tras reanudación");
        }
        self.controller = HysteresisController::new(
            self.config.daemon.hysteresis_celsius,
            self.config.daemon.min_duty_delta,
        );
        let report = apply_profile(backend, &self.config, &self.active_profile, self.dry_run)
            .with_context(|| format!("reaplicando perfil '{}'", self.active_profile))?;
        tracing::info!(?report, "perfil '{}' reaplicado", self.active_profile);
        self.last_pywal_mtime = None;
        self.sync_pywal_if_changed(backend);
        Ok(report)
    }

    fn reload(&mut self, backend: &dyn LiquidctlBackend) -> Result<()> {
        let new_config = Config::load(self.config_path.as_deref())?;
        if !new_config.profiles.contains_key(&self.active_profile) {
            bail!(
                "perfil activo '{}' no existe en la nueva configuración",
                self.active_profile
            );
        }
        let new_controller = HysteresisController::new(
            new_config.daemon.hysteresis_celsius,
            new_config.daemon.min_duty_delta,
        );
        let new_interval = Duration::from_secs(new_config.daemon.poll_interval_secs);
        let report = apply_profile(backend, &new_config, &self.active_profile, self.dry_run)
            .with_context(|| format!("reaplicando perfil '{}'", self.active_profile))?;
        tracing::info!(
            ?report,
            "configuración recargada y perfil '{}' reaplicado",
            self.active_profile
        );
        self.config = new_config;
        self.controller = new_controller;
        self.interval = new_interval;
        self.last_pywal_mtime = None;
        self.sync_pywal_if_changed(backend);
        Ok(())
    }

    fn sync_pywal_if_changed(&mut self, backend: &dyn LiquidctlBackend) {
        if !self.config.pywal.enabled {
            return;
        }
        let Some(path) = self
            .config
            .pywal
            .colors_path
            .as_deref()
            .map(PathBuf::from)
            .or_else(default_colors_json_path)
        else {
            return;
        };

        let Ok(mtime) = std::fs::metadata(&path).and_then(|m| m.modified()) else {
            return;
        };

        if Some(mtime) != self.last_pywal_mtime {
            match load_pywal_palette(Some(&path)) {
                Ok(palette) => {
                    match apply_pywal_palette(backend, &self.config.pywal, &palette, self.dry_run) {
                        Ok(applied) => {
                            self.last_pywal_mtime = Some(mtime);
                            tracing::info!(
                                path = %path.display(),
                                applied,
                                "paleta pywal sincronizada"
                            );
                        }
                        Err(e) => {
                            tracing::warn!("error aplicando paleta pywal: {e}");
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("error cargando paleta pywal desde {}: {e}", path.display());
                }
            }
        }
    }

    fn set_profile(&mut self, backend: &dyn LiquidctlBackend, profile: &str) -> Result<()> {
        if !self.config.profiles.contains_key(profile) {
            bail!("perfil '{profile}' no existe");
        }
        let report = apply_profile(backend, &self.config, profile, self.dry_run)
            .with_context(|| format!("aplicando perfil '{profile}'"))?;
        tracing::info!(?report, "perfil cambiado a '{profile}'");
        self.active_profile = profile.to_string();
        self.controller = HysteresisController::new(
            self.config.daemon.hysteresis_celsius,
            self.config.daemon.min_duty_delta,
        );
        Ok(())
    }

    fn status(&self) -> DaemonStatus {
        DaemonStatus {
            active_profile: self.active_profile.clone(),
            config_path: self.config_path.as_ref().map(|p| p.display().to_string()),
            poll_interval_secs: self.config.daemon.poll_interval_secs,
            hysteresis_celsius: self.config.daemon.hysteresis_celsius,
            min_duty_delta: self.config.daemon.min_duty_delta,
        }
    }

    fn handle_ipc(&mut self, backend: &dyn LiquidctlBackend, req: IpcRequest) -> IpcResponse {
        match req {
            IpcRequest::Status => IpcResponse::Status {
                daemon: self.status(),
            },
            IpcRequest::Reload => match self.reload(backend) {
                Ok(()) => IpcResponse::Ok {
                    message: format!(
                        "Configuración recargada (perfil activo: '{}')",
                        self.active_profile
                    ),
                },
                Err(e) => IpcResponse::Error {
                    message: e.to_string(),
                },
            },
            IpcRequest::Reapply => {
                let init = self.should_init_on_resume();
                match self.reapply(backend, init) {
                    Ok(report) => IpcResponse::Ok {
                        message: format!(
                            "Perfil '{}' reaplicado ({} iluminación, {} refrigeración)",
                            self.active_profile, report.lighting_applied, report.cooling_applied
                        ),
                    },
                    Err(e) => IpcResponse::Error {
                        message: e.to_string(),
                    },
                }
            }
            IpcRequest::SetProfile { profile } => match self.set_profile(backend, &profile) {
                Ok(()) => IpcResponse::Ok {
                    message: format!("Perfil cambiado a '{profile}'"),
                },
                Err(e) => IpcResponse::Error {
                    message: e.to_string(),
                },
            },
        }
    }
}

fn spawn_ipc_listener(
    socket_path: PathBuf,
    stop_requested: Arc<AtomicBool>,
    tx: mpsc::Sender<IpcJob>,
) -> Result<std::thread::JoinHandle<()>> {
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creando directorio del socket {}", parent.display()))?;
    }
    if socket_path.exists() {
        std::fs::remove_file(&socket_path)
            .with_context(|| format!("eliminando socket previo {}", socket_path.display()))?;
    }
    let listener = UnixListener::bind(&socket_path)
        .with_context(|| format!("abriendo socket IPC en {}", socket_path.display()))?;
    listener
        .set_nonblocking(true)
        .context("configurando socket IPC como no bloqueante")?;

    Ok(std::thread::spawn(move || {
        while !stop_requested.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, _)) => handle_ipc_connection(stream, &tx),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(POLL_SLEEP_STEP);
                }
                Err(e) => {
                    tracing::warn!("error aceptando conexión IPC: {e}");
                    std::thread::sleep(POLL_SLEEP_STEP);
                }
            }
        }
        remove_socket_quietly(&socket_path);
    }))
}

fn remove_socket_quietly(socket_path: &Path) {
    if let Err(e) = std::fs::remove_file(socket_path)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!("no se pudo eliminar socket {}: {e}", socket_path.display());
    }
}

fn handle_ipc_connection(mut stream: UnixStream, tx: &mpsc::Sender<IpcJob>) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(IPC_IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IPC_IO_TIMEOUT));

    let response = {
        let mut reader = BufReader::new(&stream);
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => return,
            Ok(_) => match serde_json::from_str::<IpcRequest>(line.trim()) {
                Ok(req) => {
                    let (resp_tx, resp_rx) = mpsc::channel();
                    if tx.send((req, resp_tx)).is_err() {
                        IpcResponse::Error {
                            message: "daemon deteniéndose".into(),
                        }
                    } else {
                        resp_rx
                            .recv_timeout(Duration::from_secs(5))
                            .unwrap_or_else(|_| IpcResponse::Error {
                                message: "tiempo de espera agotado en daemon".into(),
                            })
                    }
                }
                Err(e) => IpcResponse::Error {
                    message: format!("solicitud IPC inválida: {e}"),
                },
            },
            Err(e) => {
                tracing::warn!("error leyendo conexión IPC: {e}");
                return;
            }
        }
    };

    match serde_json::to_string(&response) {
        Ok(mut payload) => {
            payload.push('\n');
            if let Err(e) = stream
                .write_all(payload.as_bytes())
                .and_then(|()| stream.flush())
            {
                tracing::warn!("error enviando respuesta IPC: {e}");
            }
        }
        Err(e) => tracing::warn!("error serializando respuesta IPC: {e}"),
    }
}

fn process_pending_events(
    state: &mut DaemonState,
    backend: &dyn LiquidctlBackend,
    reload_requested: &AtomicBool,
    reapply_requested: &AtomicBool,
    resume_detector: &mut ResumeDetector,
    ipc_rx: &mpsc::Receiver<IpcJob>,
) -> bool {
    let mut state_changed = false;
    if reload_requested.swap(false, Ordering::SeqCst) {
        if let Err(e) = state.reload(backend) {
            tracing::warn!("error recargando configuración tras SIGHUP: {e}");
        } else {
            state_changed = true;
        }
    }
    if reapply_requested.swap(false, Ordering::SeqCst) {
        let init = state.should_init_on_resume();
        if let Err(e) = state.reapply(backend, init) {
            tracing::warn!("error reaplicando perfil tras SIGUSR1: {e}");
        } else {
            state_changed = true;
        }
    }
    if resume_detector.check() && state.config.daemon.reapply_on_resume {
        tracing::info!(
            "reanudación tras suspensión detectada; reaplicando perfil '{}'",
            state.active_profile
        );
        let init = state.should_init_on_resume();
        if let Err(e) = state.reapply(backend, init) {
            tracing::warn!("error reaplicando perfil tras reanudación: {e}");
        } else {
            state_changed = true;
        }
    }
    while let Ok((req, resp_tx)) = ipc_rx.try_recv() {
        let is_mutating = matches!(
            req,
            IpcRequest::Reload | IpcRequest::Reapply | IpcRequest::SetProfile { .. }
        );
        let resp = state.handle_ipc(backend, req);
        if is_mutating && matches!(resp, IpcResponse::Ok { .. }) {
            state_changed = true;
        }
        let _ = resp_tx.send(resp);
    }
    state_changed
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
    let profile = cli
        .profile
        .unwrap_or_else(|| config.default_profile.clone());
    if !config.profiles.contains_key(&profile) {
        bail!("perfil '{profile}' no existe");
    }
    let socket_path = cli.socket.unwrap_or_else(default_socket_path);
    let backend = CliLiquidctlBackend::new();

    let stop_requested = Arc::new(AtomicBool::new(false));
    let reload_requested = Arc::new(AtomicBool::new(false));
    let reapply_requested = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(SIGINT, Arc::clone(&stop_requested))
        .context("instalando manejador de SIGINT")?;
    signal_hook::flag::register(SIGTERM, Arc::clone(&stop_requested))
        .context("instalando manejador de SIGTERM")?;
    signal_hook::flag::register(SIGHUP, Arc::clone(&reload_requested))
        .context("instalando manejador de SIGHUP")?;
    signal_hook::flag::register(SIGUSR1, Arc::clone(&reapply_requested))
        .context("instalando manejador de SIGUSR1")?;

    let (ipc_tx, ipc_rx) = mpsc::channel();
    let ipc_handle = spawn_ipc_listener(socket_path.clone(), Arc::clone(&stop_requested), ipc_tx)?;

    if cli.init_devices {
        backend
            .initialize(None, cli.dry_run)
            .context("inicializando dispositivos vía liquidctl")?;
        tracing::info!("dispositivos inicializados");
    }

    let report = apply_profile(&backend, &config, &profile, cli.dry_run)
        .with_context(|| format!("aplicando perfil inicial '{profile}'"))?;
    tracing::info!(?report, "perfil '{profile}' aplicado");

    let mut state = DaemonState::new(config, cli.config, profile, cli.dry_run)
        .with_init_on_resume(cli.init_on_resume);
    state.sync_pywal_if_changed(&backend);
    let mut resume_detector = ResumeDetector::default();

    while !stop_requested.load(Ordering::SeqCst) {
        process_pending_events(
            &mut state,
            &backend,
            &reload_requested,
            &reapply_requested,
            &mut resume_detector,
            &ipc_rx,
        );

        state.sync_pywal_if_changed(&backend);

        match step_daemon_tick(
            &backend,
            &state.config,
            &state.active_profile,
            &mut state.controller,
            state.dry_run,
        ) {
            Ok(changes) => tracing::debug!(?changes, "tick"),
            Err(e) => tracing::warn!("error en tick: {e}"),
        }
        if cli.once {
            stop_requested.store(true, Ordering::SeqCst);
            break;
        }

        // Dormir en tramos cortos para responder rápido a señales e IPC.
        let mut left = state.interval;
        while !stop_requested.load(Ordering::SeqCst) && !left.is_zero() {
            if process_pending_events(
                &mut state,
                &backend,
                &reload_requested,
                &reapply_requested,
                &mut resume_detector,
                &ipc_rx,
            ) {
                break;
            }
            let d = left.min(POLL_SLEEP_STEP);
            std::thread::sleep(d);
            left -= d;
        }
    }

    stop_requested.store(true, Ordering::SeqCst);
    let _ = ipc_handle.join();
    remove_socket_quietly(&socket_path);
    tracing::info!("nxctld detenido");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nxctl_core::liquidctl::{MockCall, MockLiquidctlBackend};
    use std::collections::BTreeMap;

    #[test]
    fn cli_parses_init_devices_and_runtime_flags() {
        let cli = Cli::try_parse_from([
            "nxctld",
            "--init-devices",
            "--once",
            "--dry-run",
            "--profile",
            "gaming",
            "--socket",
            "/tmp/custom-nxctld.sock",
            "-vv",
        ])
        .unwrap();

        assert!(cli.init_devices);
        assert!(cli.once);
        assert!(cli.dry_run);
        assert_eq!(cli.profile.as_deref(), Some("gaming"));
        assert_eq!(
            cli.socket.as_deref(),
            Some(Path::new("/tmp/custom-nxctld.sock"))
        );
        assert_eq!(cli.verbose, 2);
    }

    #[test]
    fn cli_defaults_have_init_devices_disabled() {
        let cli = Cli::try_parse_from(["nxctld"]).unwrap();
        assert!(!cli.init_devices);
        assert!(!cli.init_on_resume);
        assert!(!cli.once);
        assert!(!cli.dry_run);
        assert!(cli.socket.is_none());
        assert_eq!(cli.verbose, 0);
    }

    #[test]
    fn cli_parses_init_on_resume_flag() {
        let cli = Cli::try_parse_from(["nxctld", "--init-on-resume"]).unwrap();
        assert!(cli.init_on_resume);
        assert!(!cli.init_devices);
    }

    #[test]
    fn daemon_state_reapply_with_and_without_init() {
        let cfg = Config::starter_nzxt();
        let backend_no_init = MockLiquidctlBackend::new();
        let mut state = DaemonState::new(cfg.clone(), None, "default".into(), false);

        let report_no_init = state.reapply(&backend_no_init, false).unwrap();
        assert!(report_no_init.lighting_applied + report_no_init.cooling_applied > 0);
        let calls_no_init = backend_no_init.calls();
        assert!(
            !calls_no_init
                .iter()
                .any(|c| matches!(c, MockCall::Initialize { .. }))
        );
        assert!(
            calls_no_init
                .iter()
                .any(|c| matches!(c, MockCall::SetColor { .. } | MockCall::SetSpeed { .. }))
        );

        let backend_with_init = MockLiquidctlBackend::new();
        let mut state2 = DaemonState::new(cfg, None, "default".into(), false);
        let report_with_init = state2.reapply(&backend_with_init, true).unwrap();
        assert!(report_with_init.lighting_applied + report_with_init.cooling_applied > 0);
        let calls_with_init = backend_with_init.calls();
        assert!(
            calls_with_init
                .iter()
                .any(|c| matches!(c, MockCall::Initialize { .. }))
        );
        assert!(
            calls_with_init
                .iter()
                .any(|c| matches!(c, MockCall::SetColor { .. } | MockCall::SetSpeed { .. }))
        );
    }

    #[test]
    fn daemon_state_set_profile_switches_and_rejects_unknown() {
        let config = Config::starter_nzxt();
        let backend = MockLiquidctlBackend::new();
        let mut state = DaemonState::new(config, None, "default".into(), false);

        let err_resp = state.handle_ipc(
            &backend,
            IpcRequest::SetProfile {
                profile: "nonexistent".into(),
            },
        );
        assert!(matches!(err_resp, IpcResponse::Error { .. }));
        assert_eq!(state.active_profile, "default");

        let ok_resp = state.handle_ipc(
            &backend,
            IpcRequest::SetProfile {
                profile: "performance".into(),
            },
        );
        assert!(matches!(ok_resp, IpcResponse::Ok { .. }));
        assert_eq!(state.active_profile, "performance");
        assert!(!backend.calls().is_empty());

        let status_resp = state.handle_ipc(&backend, IpcRequest::Status);
        match status_resp {
            IpcResponse::Status { daemon } => {
                assert_eq!(daemon.active_profile, "performance");
                assert!(daemon.config_path.is_none());
            }
            other => panic!("respuesta inesperada: {other:?}"),
        }
    }

    #[test]
    fn daemon_state_reload_reads_updated_config_and_reapplies() {
        let temp_path =
            std::env::temp_dir().join(format!("nxctld-reload-test-{}.toml", std::process::id()));
        let mut cfg = Config::starter_nzxt();
        cfg.daemon.poll_interval_secs = 2;
        cfg.save(Some(&temp_path)).unwrap();

        let backend = MockLiquidctlBackend::new();
        let mut state = DaemonState::new(cfg, Some(temp_path.clone()), "default".into(), false);

        let mut updated = Config::starter_nzxt();
        updated.daemon.poll_interval_secs = 7;
        updated.daemon.hysteresis_celsius = 3.5;
        updated.daemon.min_duty_delta = 5;
        updated.profiles.insert(
            "custom".into(),
            updated.profiles.get("default").unwrap().clone(),
        );
        updated.save(Some(&temp_path)).unwrap();

        let resp = state.handle_ipc(&backend, IpcRequest::Reload);
        let _ = std::fs::remove_file(&temp_path);

        assert!(matches!(resp, IpcResponse::Ok { .. }));
        assert_eq!(state.interval, Duration::from_secs(7));
        assert_eq!(state.config.daemon.hysteresis_celsius, 3.5);
        assert_eq!(state.config.daemon.min_duty_delta, 5);
        assert!(state.config.profiles.contains_key("custom"));
        assert!(
            backend
                .calls()
                .iter()
                .any(|c| matches!(c, MockCall::SetColor { .. } | MockCall::SetSpeed { .. }))
        );
    }

    #[test]
    fn daemon_state_reload_keeps_old_config_if_active_profile_removed() {
        let temp_path = std::env::temp_dir().join(format!(
            "nxctld-reload-missing-profile-{}.toml",
            std::process::id()
        ));
        let cfg = Config::starter_nzxt();
        cfg.save(Some(&temp_path)).unwrap();

        let backend = MockLiquidctlBackend::new();
        let mut state = DaemonState::new(cfg, Some(temp_path.clone()), "performance".into(), false);

        let mut updated = Config::starter_nzxt();
        updated.profiles = BTreeMap::from([(
            "default".into(),
            updated.profiles.get("default").unwrap().clone(),
        )]);
        updated.default_profile = "default".into();
        updated.save(Some(&temp_path)).unwrap();

        let resp = state.handle_ipc(&backend, IpcRequest::Reload);
        let _ = std::fs::remove_file(&temp_path);

        assert!(matches!(resp, IpcResponse::Error { .. }));
        assert_eq!(state.active_profile, "performance");
        assert!(state.config.profiles.contains_key("performance"));
    }

    #[test]
    fn daemon_state_sync_pywal_if_changed_tracks_mtime() {
        let temp_dir =
            std::env::temp_dir().join(format!("nxctld-pywal-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&temp_dir);
        let colors_json = temp_dir.join("colors.json");

        let wal_content = r##"{
            "wallpaper": "/tmp/test.png",
            "colors": {
                "color0": "#000000",
                "color1": "#111111",
                "color4": "#444444"
            }
        }"##;
        std::fs::write(&colors_json, wal_content).unwrap();

        let mut cfg = Config::starter_nzxt();
        cfg.pywal.enabled = true;
        cfg.pywal.colors_path = Some(colors_json.display().to_string());

        let backend = MockLiquidctlBackend::new();
        let mut state = DaemonState::new(cfg, None, "default".into(), false);

        assert_eq!(state.last_pywal_mtime, None);
        state.sync_pywal_if_changed(&backend);
        assert!(state.last_pywal_mtime.is_some());
        let initial_calls = backend.calls().len();
        assert!(initial_calls > 0);

        // Calling again without file modification should not issue calls
        state.sync_pywal_if_changed(&backend);
        assert_eq!(backend.calls().len(), initial_calls);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
