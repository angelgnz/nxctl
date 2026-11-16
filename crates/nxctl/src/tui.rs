//! Interfaz de terminal (`nxctl tui`) basada en `ratatui` + `crossterm`.

use std::collections::VecDeque;
use std::io::{self, Stdout};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use anyhow::Result;
use nxctl_core::config::Config;
use nxctl_core::engine::apply_profile;
use nxctl_core::liquidctl::{CliLiquidctlBackend, DeviceStatus, LiquidctlBackend};
use nxctl_core::{
    DaemonStatus, HwmonSensorInfo, IpcRequest, IpcResponse, NvidiaGpuInfo, evaluate_curve,
    list_hwmon_sensors, list_nvidia_gpus, send_request,
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
    Row, Sparkline, Table, TableState, Tabs,
};

/// Datos refrescados periódicamente por el hilo de sondeo.
#[derive(Debug, Clone, Default)]
pub struct TuiSnapshot {
    pub devices: Vec<DeviceStatus>,
    pub hwmon: Vec<HwmonSensorInfo>,
    pub nvidia: Vec<NvidiaGpuInfo>,
    pub daemon: Option<DaemonStatus>,
    pub error: Option<String>,
}

/// Máximo de muestras de historial de temperatura.
pub const HISTORY_LEN: usize = 40;

/// Panel con foco para el desplazamiento con ↑/↓.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Profiles,
    Sensors,
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
    s.hwmon
        .iter()
        .find(|h| h.name == "k10temp" || h.name == "coretemp" || h.name == "zenpower")
        .or_else(|| s.hwmon.first())
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
    pub config: Config,
    pub profile_names: Vec<String>,
    pub selected_profile: usize,
    pub snapshot: TuiSnapshot,
    pub status_message: Option<String>,
    pub dry_run: bool,
    pub socket_path: PathBuf,
    pub liquid_history: VecDeque<u64>,
    pub cpu_history: VecDeque<u64>,
    pub gpu_history: VecDeque<u64>,
    pub curve_names: Vec<String>,
    pub selected_curve: usize,
    pub focus: Focus,
    pub sensor_state: TableState,
    pub show_help: bool,
}

impl TuiState {
    pub fn new(config: Config, dry_run: bool, socket_path: PathBuf) -> Self {
        let profile_names: Vec<String> = config.profiles.keys().cloned().collect();
        let curve_names: Vec<String> = config.curves.keys().cloned().collect();
        let selected_profile = profile_names
            .iter()
            .position(|n| *n == config.default_profile)
            .unwrap_or(0);
        Self {
            config,
            profile_names,
            selected_profile,
            snapshot: TuiSnapshot::default(),
            status_message: None,
            dry_run,
            socket_path,
            liquid_history: VecDeque::new(),
            cpu_history: VecDeque::new(),
            gpu_history: VecDeque::new(),
            curve_names,
            selected_curve: 0,
            focus: Focus::Profiles,
            sensor_state: TableState::default(),
            show_help: false,
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
        self.snapshot = snap;
    }

    pub fn next_curve(&mut self) {
        if !self.curve_names.is_empty() {
            self.selected_curve = (self.selected_curve + 1) % self.curve_names.len();
        }
    }

    pub fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Profiles => Focus::Sensors,
            Focus::Sensors => Focus::Profiles,
        };
    }

    fn sensor_count(&self) -> usize {
        self.snapshot.hwmon.len() + self.snapshot.nvidia.len()
    }

    fn scroll(&mut self, down: bool) {
        match self.focus {
            Focus::Profiles => {
                if down {
                    self.next_profile();
                } else {
                    self.prev_profile();
                }
            }
            Focus::Sensors => {
                let n = self.sensor_count();
                if n == 0 {
                    return;
                }
                let cur = self.sensor_state.selected().unwrap_or(0).min(n - 1);
                let next = if down {
                    (cur + 1) % n
                } else {
                    (cur + n - 1) % n
                };
                self.sensor_state.select(Some(next));
            }
        }
    }

    pub fn next_profile(&mut self) {
        if !self.profile_names.is_empty() {
            self.selected_profile = (self.selected_profile + 1) % self.profile_names.len();
        }
    }

    pub fn prev_profile(&mut self) {
        let n = self.profile_names.len();
        if n > 0 {
            self.selected_profile = (self.selected_profile + n - 1) % n;
        }
    }

    pub fn selected_name(&self) -> Option<&str> {
        self.profile_names
            .get(self.selected_profile)
            .map(String::as_str)
    }

    fn active_profile(&self) -> &str {
        self.snapshot
            .daemon
            .as_ref()
            .map(|d| d.active_profile.as_str())
            .unwrap_or(&self.config.default_profile)
    }
}

/// Recolecta una instantánea de dispositivos, sensores y daemon.
fn collect_snapshot(backend: &CliLiquidctlBackend, socket: &std::path::Path) -> TuiSnapshot {
    let mut errors = Vec::new();
    let devices = backend.status(None).unwrap_or_else(|e| {
        errors.push(format!("liquidctl: {e}"));
        Vec::new()
    });
    let hwmon = list_hwmon_sensors().unwrap_or_else(|e| {
        errors.push(format!("hwmon: {e}"));
        Vec::new()
    });
    let nvidia = list_nvidia_gpus().unwrap_or_default();
    let daemon = match send_request(socket, &IpcRequest::Status) {
        Ok(IpcResponse::Status { daemon }) => Some(daemon),
        _ => None,
    };
    TuiSnapshot {
        devices,
        hwmon,
        nvidia,
        daemon,
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
    interval: Duration,
    tx: Sender<TuiSnapshot>,
    trigger_rx: Receiver<()>,
) {
    std::thread::spawn(move || {
        let backend = CliLiquidctlBackend::new();
        loop {
            if tx.send(collect_snapshot(&backend, &socket)).is_err() {
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
    Apply,
    Refresh,
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
        KeyCode::Up | KeyCode::Char('k') => {
            state.scroll(false);
            KeyAction::None
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.scroll(true);
            KeyAction::None
        }
        KeyCode::Tab => {
            state.toggle_focus();
            KeyAction::None
        }
        KeyCode::Char('c') => {
            state.next_curve();
            KeyAction::None
        }
        KeyCode::Enter | KeyCode::Char('a') => KeyAction::Apply,
        KeyCode::Char('r') => KeyAction::Refresh,
        _ => KeyAction::None,
    }
}

fn apply_selected(state: &mut TuiState, backend: &CliLiquidctlBackend) {
    let Some(name) = state.selected_name().map(str::to_string) else {
        state.status_message = Some("sin perfiles".into());
        return;
    };
    let connected = state.snapshot.daemon.is_some();
    state.status_message = Some(if connected && !state.dry_run {
        match send_request(
            &state.socket_path,
            &IpcRequest::SetProfile {
                profile: name.clone(),
            },
        ) {
            Ok(IpcResponse::Ok { message }) => format!("perfil '{name}': {message}"),
            Ok(IpcResponse::Error { message }) => format!("error: {message}"),
            Ok(_) => format!("perfil '{name}' enviado"),
            Err(e) => format!("error IPC: {e}"),
        }
    } else {
        match apply_profile(backend, &state.config, &name, state.dry_run) {
            Ok(r) => format!(
                "perfil '{name}' aplicado: {} iluminación, {} refrigeración",
                r.lighting_applied, r.cooling_applied
            ),
            Err(e) => format!("error: {e}"),
        }
    });
}

fn reload_daemon(state: &mut TuiState) {
    state.status_message = Some(if state.snapshot.daemon.is_some() && !state.dry_run {
        match send_request(&state.socket_path, &IpcRequest::Reload) {
            Ok(IpcResponse::Ok { message }) => message,
            Ok(IpcResponse::Error { message }) => format!("error: {message}"),
            Ok(_) => "recargado".into(),
            Err(e) => format!("error IPC: {e}"),
        }
    } else {
        "actualizando…".into()
    });
}

/// Punto de entrada de `nxctl tui`.
pub fn run(config: Config, dry_run: bool, socket_path: PathBuf, interval: u64) -> Result<()> {
    let mut state = TuiState::new(config, dry_run, socket_path.clone());
    let backend = CliLiquidctlBackend::new();
    let (tx, rx) = mpsc::channel();
    let (trigger_tx, trigger_rx) = mpsc::channel();
    spawn_poller(
        socket_path,
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
            match map_key(state, key) {
                KeyAction::Quit => return Ok(()),
                KeyAction::Apply => {
                    apply_selected(state, backend);
                    let _ = trigger.send(());
                }
                KeyAction::Refresh => {
                    reload_daemon(state);
                    let _ = trigger.send(());
                }
                KeyAction::None => {}
            }
        }
    }
}

fn temp_color(t: f32) -> Color {
    if t < 45.0 {
        Color::Green
    } else if t <= 70.0 {
        Color::Yellow
    } else {
        Color::Red
    }
}

fn format_value(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => "-".into(),
        other => other.to_string(),
    }
}

fn panel_block(title: &str, focused: bool) -> Block<'_> {
    let style = if focused {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    Block::default()
        .borders(Borders::ALL)
        .border_style(style)
        .title(title)
}

fn centered_rect(w: u16, h: u16, area: Rect) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    )
}

fn draw_history(frame: &mut Frame, state: &TuiState, area: Rect) {
    let block = Block::default().borders(Borders::ALL).title("Historial °C");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1); 3])
        .split(inner);
    let snap = &state.snapshot;
    let entries = [
        (
            "Liquid",
            &state.liquid_history,
            liquid_temp(snap),
            Color::Cyan,
        ),
        ("CPU", &state.cpu_history, cpu_temp(snap), Color::Yellow),
        ("GPU", &state.gpu_history, gpu_temp(snap), Color::Magenta),
    ];
    for (row, (name, hist, cur, color)) in rows.iter().zip(entries) {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(14), Constraint::Min(1)])
            .split(*row);
        let label = match cur {
            Some(t) => Span::styled(
                format!("{name} {t:.0}°C"),
                Style::default().fg(temp_color(t)),
            ),
            None => Span::raw(format!("{name} -")),
        };
        frame.render_widget(Paragraph::new(Line::from(label)), cols[0]);
        let data: Vec<u64> = hist.iter().copied().collect();
        frame.render_widget(
            Sparkline::default()
                .data(&data)
                .style(Style::default().fg(color)),
            cols[1],
        );
    }
}

fn draw_devices(frame: &mut Frame, state: &TuiState, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("Dispositivos NZXT (liquidctl)");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let mut lines: Vec<Line> = Vec::new();
    let mut gauges: Vec<(String, f64)> = Vec::new();
    if state.snapshot.devices.is_empty() {
        lines.push(Line::from("(sin dispositivos)"));
    }
    for dev in &state.snapshot.devices {
        lines.push(Line::from(Span::styled(
            dev.description.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        )));
        for r in &dev.status {
            if r.key.to_lowercase().contains("duty") {
                let v = r.value.as_f64().unwrap_or(0.0).clamp(0.0, 100.0);
                gauges.push((r.key.clone(), v));
                continue;
            }
            let unit = r.unit.as_deref().unwrap_or("");
            lines.push(Line::from(format!(
                "  {}: {} {}",
                r.key,
                format_value(&r.value),
                unit
            )));
        }
    }
    let gauge_rows = (gauges.len() as u16).min(inner.height.saturating_sub(1));
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(gauge_rows)])
        .split(inner);
    frame.render_widget(Paragraph::new(lines), parts[0]);
    for (i, (key, v)) in gauges.iter().take(gauge_rows as usize).enumerate() {
        let r = Rect::new(parts[1].x, parts[1].y + i as u16, parts[1].width, 1);
        frame.render_widget(
            LineGauge::default()
                .ratio(v / 100.0)
                .label(format!("{key} {v:.0}%"))
                .filled_style(Style::default().fg(Color::Green))
                .unfilled_style(Style::default().fg(Color::DarkGray)),
            r,
        );
    }
}

fn draw_sensors(frame: &mut Frame, state: &mut TuiState, area: Rect) {
    let mut rows: Vec<Row> = Vec::new();
    for h in &state.snapshot.hwmon {
        let label = h.label.as_deref().unwrap_or(&h.input_file);
        rows.push(Row::new(vec![
            Span::raw(h.name.clone()),
            Span::raw(label.to_string()),
            Span::styled(
                format!("{:.1}°C", h.temp_c),
                Style::default().fg(temp_color(h.temp_c)),
            ),
        ]));
    }
    for g in &state.snapshot.nvidia {
        rows.push(Row::new(vec![
            Span::raw(format!("nvidia{}", g.index)),
            Span::raw(g.name.clone()),
            Span::styled(
                format!("{:.1}°C", g.temp_c),
                Style::default().fg(temp_color(g.temp_c)),
            ),
        ]));
    }
    let table = Table::new(
        rows,
        [
            Constraint::Percentage(30),
            Constraint::Percentage(45),
            Constraint::Percentage(25),
        ],
    )
    .header(
        Row::new(["Dispositivo", "Sensor", "Temp"])
            .style(Style::default().add_modifier(Modifier::BOLD)),
    )
    .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
    .block(panel_block(
        "Sensores (hwmon / NVIDIA)",
        state.focus == Focus::Sensors,
    ));
    frame.render_stateful_widget(table, area, &mut state.sensor_state);
}

fn draw_curves(frame: &mut Frame, state: &TuiState, area: Rect) {
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(4)])
        .split(area);
    frame.render_widget(
        Tabs::new(state.curve_names.iter().map(String::as_str))
            .select(state.selected_curve)
            .highlight_style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        parts[0],
    );
    let curve = state
        .curve_names
        .get(state.selected_curve)
        .and_then(|n| state.config.curves.get(n));
    let pts: Vec<(f64, f64)> = curve
        .map(|c| {
            c.points
                .iter()
                .map(|p| (f64::from(p.temp_c), f64::from(p.duty)))
                .collect()
        })
        .unwrap_or_default();
    let snap = &state.snapshot;
    let cur = liquid_temp(snap)
        .or_else(|| cpu_temp(snap))
        .or_else(|| gpu_temp(snap));
    let op: Vec<(f64, f64)> = match (curve, cur) {
        (Some(c), Some(t)) => vec![(f64::from(t), f64::from(evaluate_curve(c, t)))],
        _ => Vec::new(),
    };
    let mut datasets = vec![
        Dataset::default()
            .name(
                state
                    .curve_names
                    .get(state.selected_curve)
                    .map_or("", String::as_str),
            )
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(Color::Cyan))
            .data(&pts),
    ];
    if !op.is_empty() {
        datasets.push(
            Dataset::default()
                .name("actual")
                .marker(symbols::Marker::Block)
                .graph_type(GraphType::Scatter)
                .style(Style::default().fg(Color::Red))
                .data(&op),
        );
    }
    let max_t = pts
        .iter()
        .map(|p| p.0)
        .chain(op.iter().map(|p| p.0))
        .fold(100.0_f64, f64::max);
    let chart = Chart::new(datasets)
        .block(Block::default().borders(Borders::ALL).title("Curvas (c)"))
        .x_axis(
            Axis::default()
                .title("°C")
                .bounds([0.0, max_t])
                .labels(["0".to_string(), format!("{max_t:.0}")]),
        )
        .y_axis(
            Axis::default()
                .title("%")
                .bounds([0.0, 100.0])
                .labels(["0", "100"]),
        );
    frame.render_widget(chart, parts[1]);
}

const HELP_LINES: [&str; 9] = [
    "q / Ctrl+C   salir",
    "Esc          salir / cerrar ayuda",
    "Tab          cambiar foco Perfiles ↔ Sensores",
    "↑/↓  j/k     desplazar panel enfocado",
    "Enter / a    aplicar perfil",
    "c            siguiente curva",
    "r            recargar / actualizar",
    "?            mostrar/ocultar ayuda",
    "",
];

/// Dibuja la interfaz completa.
pub fn draw(frame: &mut Frame, state: &mut TuiState) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(frame.area());

    // Header
    let mut spans = vec![Span::styled(
        "nxctl TUI  ",
        Style::default().add_modifier(Modifier::BOLD),
    )];
    match &state.snapshot.daemon {
        Some(d) => spans.push(Span::styled(
            format!(
                "conectado · perfil: {} · intervalo: {}s · histéresis: {}°C",
                d.active_profile, d.poll_interval_secs, d.hysteresis_celsius
            ),
            Style::default().fg(Color::Green),
        )),
        None => spans.push(Span::styled(
            "daemon desconectado",
            Style::default().fg(Color::Red),
        )),
    }
    if state.dry_run {
        spans.push(Span::styled(
            "  [dry-run]",
            Style::default().fg(Color::Black).bg(Color::Yellow),
        ));
    }
    if let Some(m) = &state.status_message {
        spans.push(Span::raw(format!("  {m}")));
    }
    if let Some(e) = &state.snapshot.error {
        spans.push(Span::styled(
            format!("  ! {e}"),
            Style::default().fg(Color::Red),
        ));
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).block(Block::default().borders(Borders::ALL)),
        rows[0],
    );

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[1]);
    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(45),
            Constraint::Length(5),
            Constraint::Min(3),
        ])
        .split(cols[0]);
    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(8), Constraint::Min(6)])
        .split(cols[1]);

    draw_devices(frame, state, left[0]);
    draw_history(frame, state, left[1]);
    draw_sensors(frame, state, left[2]);

    // Perfiles
    let active = state.active_profile();
    let items: Vec<ListItem> = state
        .profile_names
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let mark = if n == active { "*" } else { " " };
            let style = if i == state.selected_profile {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            ListItem::new(format!("{mark} {n}")).style(style)
        })
        .collect();
    frame.render_widget(
        List::new(items).block(panel_block("Perfiles", state.focus == Focus::Profiles)),
        right[0],
    );

    draw_curves(frame, state, right[1]);

    // Footer
    frame.render_widget(
        Paragraph::new(
            "q salir · Tab foco · ↑/↓ j/k desplazar · Enter/a aplicar · c curva · r recargar · ? ayuda",
        )
        .block(Block::default().borders(Borders::ALL)),
        rows[2],
    );

    if state.show_help {
        let area = centered_rect(56, HELP_LINES.len() as u16 + 2, frame.area());
        frame.render_widget(Clear, area);
        frame.render_widget(
            Paragraph::new(HELP_LINES.map(Line::from).to_vec()).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Ayuda")
                    .border_style(Style::default().fg(Color::Yellow)),
            ),
            area,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nxctl_core::liquidctl::StatusReading;
    use ratatui::backend::TestBackend;

    fn state() -> TuiState {
        TuiState::new(Config::starter_nzxt(), false, PathBuf::from("/tmp/x.sock"))
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
    fn draw_renders_sample_data() {
        let mut s = state();
        s.dry_run = true;
        s.status_message = Some("ok".into());
        s.snapshot = TuiSnapshot {
            devices: vec![DeviceStatus {
                address: None,
                bus: None,
                description: "NZXT Kraken X".into(),
                status: vec![StatusReading {
                    key: "Liquid temperature".into(),
                    value: serde_json::json!(31.2),
                    unit: Some("°C".into()),
                }],
            }],
            hwmon: vec![HwmonSensorInfo {
                device_dir: "hwmon0".into(),
                name: "k10temp".into(),
                input_file: "temp1_input".into(),
                label: Some("Tctl".into()),
                temp_c: 75.0,
            }],
            nvidia: vec![NvidiaGpuInfo {
                index: 0,
                name: "RTX".into(),
                temp_c: 50.0,
            }],
            daemon: Some(DaemonStatus {
                active_profile: "silent".into(),
                config_path: None,
                poll_interval_secs: 2,
                hysteresis_celsius: 2.0,
                min_duty_delta: 3,
            }),
            error: None,
        };
        let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
        term.draw(|f| draw(f, &mut s)).unwrap();
        let text: String = term
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(text.contains("nxctl TUI"));
        assert!(text.contains("conectado"));
        assert!(text.contains("Liquid"));
        assert!(text.contains("Sensor"));
        s.show_help = true;
        term.draw(|f| draw(f, &mut s)).unwrap();
        let text: String = term
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(text.contains("Ayuda"));
        let mut term = Terminal::new(TestBackend::new(30, 10)).unwrap();
        term.draw(|f| draw(f, &mut state())).unwrap();
    }

    #[test]
    fn keys_map() {
        let mut s = state();
        let k = |c| KeyEvent::new(c, KeyModifiers::NONE);
        assert_eq!(map_key(&mut s, k(KeyCode::Char('q'))), KeyAction::Quit);
        assert_eq!(map_key(&mut s, k(KeyCode::Enter)), KeyAction::Apply);
        assert_eq!(map_key(&mut s, k(KeyCode::Char('r'))), KeyAction::Refresh);
        assert_eq!(
            map_key(
                &mut s,
                KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
            ),
            KeyAction::Quit
        );
    }

    fn snap(liquid: f64, cpu: f32, gpu: f32) -> TuiSnapshot {
        TuiSnapshot {
            devices: vec![DeviceStatus {
                address: None,
                bus: None,
                description: "d".into(),
                status: vec![
                    StatusReading {
                        key: "Liquid temperature".into(),
                        value: serde_json::json!(liquid),
                        unit: None,
                    },
                    StatusReading {
                        key: "Pump duty".into(),
                        value: serde_json::json!(60),
                        unit: Some("%".into()),
                    },
                ],
            }],
            hwmon: vec![HwmonSensorInfo {
                device_dir: "hwmon0".into(),
                name: "k10temp".into(),
                input_file: "temp1_input".into(),
                label: None,
                temp_c: cpu,
            }],
            nvidia: vec![NvidiaGpuInfo {
                index: 0,
                name: "RTX".into(),
                temp_c: gpu,
            }],
            ..TuiSnapshot::default()
        }
    }

    #[test]
    fn history_tracks_and_caps() {
        let mut s = state();
        for i in 0..50 {
            s.push_snapshot(snap(30.2 + f64::from(i) * 0.001, 60.6, 45.0));
        }
        assert_eq!(s.liquid_history.len(), HISTORY_LEN);
        assert_eq!(s.cpu_history.back(), Some(&61));
        assert_eq!(s.gpu_history.back(), Some(&45));
        assert_eq!(s.liquid_history.front(), Some(&30));
    }

    #[test]
    fn curve_cycling_wraps() {
        let mut s = state();
        let n = s.curve_names.len();
        assert!(n > 0);
        let k = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE);
        for _ in 0..n {
            map_key(&mut s, k);
        }
        assert_eq!(s.selected_curve, 0);
        map_key(&mut s, k);
        assert_eq!(s.selected_curve, 1 % n);
    }

    #[test]
    fn focus_and_help_toggle() {
        let mut s = state();
        s.push_snapshot(snap(30.0, 50.0, 40.0));
        let k = |c| KeyEvent::new(c, KeyModifiers::NONE);
        assert_eq!(s.focus, Focus::Profiles);
        map_key(&mut s, k(KeyCode::Tab));
        assert_eq!(s.focus, Focus::Sensors);
        map_key(&mut s, k(KeyCode::Down));
        assert_eq!(s.sensor_state.selected(), Some(1));
        map_key(&mut s, k(KeyCode::Down));
        assert_eq!(s.sensor_state.selected(), Some(0));
        map_key(&mut s, k(KeyCode::Char('k')));
        assert_eq!(s.sensor_state.selected(), Some(1));
        map_key(&mut s, k(KeyCode::Tab));
        assert_eq!(s.focus, Focus::Profiles);
        map_key(&mut s, k(KeyCode::Char('?')));
        assert!(s.show_help);
        assert_eq!(map_key(&mut s, k(KeyCode::Esc)), KeyAction::None);
        assert!(!s.show_help);
        assert_eq!(map_key(&mut s, k(KeyCode::Esc)), KeyAction::Quit);
    }

    #[test]
    fn draw_with_gauges_and_operating_point() {
        let mut s = state();
        s.push_snapshot(snap(40.0, 50.0, 40.0));
        let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
        term.draw(|f| draw(f, &mut s)).unwrap();
        let text: String = term
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(text.contains("Pump duty 60%"));
    }
}
