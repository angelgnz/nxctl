//! Localización (Español e Inglés) para `nxctl` (CLI y TUI).

use std::fmt::Display;
use std::path::Path;

use nxctl_core::{DaemonStatus, Language};

/// Cadenas estáticas de la interfaz TUI.
pub struct TuiStrings {
    pub history_title: &'static str,
    pub devices_title: &'static str,
    pub no_devices: &'static str,
    pub sensors_title: &'static str,
    pub sensors_headers: [&'static str; 3],
    pub profiles_title: &'static str,
    pub curves_title: &'static str,
    pub curve_current_label: &'static str,
    pub daemon_disconnected: &'static str,
    pub help_title: &'static str,
    pub help_lines: [&'static str; 10],
    pub footer: &'static str,
    pub no_profiles: &'static str,
    pub reloaded: &'static str,
    pub refreshing: &'static str,
}

static TUI_ES: TuiStrings = TuiStrings {
    history_title: "Historial °C",
    devices_title: "Dispositivos NZXT (liquidctl)",
    no_devices: "(sin dispositivos)",
    sensors_title: "Sensores (hwmon / NVIDIA)",
    sensors_headers: ["Dispositivo", "Sensor", "Temp"],
    profiles_title: "Perfiles",
    curves_title: "Curvas (c)",
    curve_current_label: "actual",
    daemon_disconnected: "daemon desconectado",
    help_title: "Ayuda",
    help_lines: [
        "q / Ctrl+C   salir",
        "Esc          salir / cerrar ayuda",
        "Tab          cambiar foco Perfiles ↔ Sensores",
        "↑/↓  j/k     desplazar panel enfocado",
        "Enter / a    aplicar perfil",
        "c            siguiente curva",
        "r            recargar / actualizar",
        "l            cambiar idioma (ES/EN)",
        "?            mostrar/ocultar ayuda",
        "",
    ],
    footer: "q salir · Tab foco · ↑/↓ j/k desplazar · Enter/a aplicar · c curva · r recargar · l idioma · ? ayuda",
    no_profiles: "sin perfiles",
    reloaded: "recargado",
    refreshing: "actualizando…",
};

static TUI_EN: TuiStrings = TuiStrings {
    history_title: "History °C",
    devices_title: "NZXT Devices (liquidctl)",
    no_devices: "(no devices)",
    sensors_title: "Sensors (hwmon / NVIDIA)",
    sensors_headers: ["Device", "Sensor", "Temp"],
    profiles_title: "Profiles",
    curves_title: "Curves (c)",
    curve_current_label: "current",
    daemon_disconnected: "daemon disconnected",
    help_title: "Help",
    help_lines: [
        "q / Ctrl+C   quit",
        "Esc          quit / close help",
        "Tab          switch focus Profiles ↔ Sensors",
        "↑/↓  j/k     scroll focused panel",
        "Enter / a    apply profile",
        "c            next curve",
        "r            reload / refresh",
        "l            switch language (ES/EN)",
        "?            toggle help",
        "",
    ],
    footer: "q quit · Tab focus · ↑/↓ j/k scroll · Enter/a apply · c curve · r reload · l lang · ? help",
    no_profiles: "no profiles",
    reloaded: "reloaded",
    refreshing: "refreshing…",
};

/// Devuelve las cadenas TUI correspondientes al idioma indicado.
pub fn tui(lang: Language) -> &'static TuiStrings {
    match lang {
        Language::Es => &TUI_ES,
        Language::En => &TUI_EN,
    }
}

pub fn fmt_daemon_connected(
    lang: Language,
    active_profile: &str,
    poll_interval_secs: u64,
    hysteresis_celsius: f32,
) -> String {
    match lang {
        Language::Es => format!(
            "conectado · perfil: {active_profile} · intervalo: {poll_interval_secs}s · histéresis: {hysteresis_celsius}°C"
        ),
        Language::En => format!(
            "connected · profile: {active_profile} · interval: {poll_interval_secs}s · hysteresis: {hysteresis_celsius}°C"
        ),
    }
}

pub fn fmt_tui_profile_applied(
    lang: Language,
    name: &str,
    lighting: usize,
    cooling: usize,
) -> String {
    match lang {
        Language::Es => {
            format!("perfil '{name}' aplicado: {lighting} iluminación, {cooling} refrigeración")
        }
        Language::En => {
            format!("profile '{name}' applied: {lighting} lighting, {cooling} cooling")
        }
    }
}

pub fn fmt_tui_profile_ipc_ok(lang: Language, name: &str, message: &str) -> String {
    match lang {
        Language::Es => format!("perfil '{name}': {message}"),
        Language::En => format!("profile '{name}': {message}"),
    }
}

pub fn fmt_tui_profile_sent(lang: Language, name: &str) -> String {
    match lang {
        Language::Es => format!("perfil '{name}' enviado"),
        Language::En => format!("profile '{name}' sent"),
    }
}

pub fn fmt_tui_ipc_error(lang: Language, err: &dyn Display) -> String {
    match lang {
        Language::Es => format!("error IPC: {err}"),
        Language::En => format!("IPC error: {err}"),
    }
}

pub fn fmt_tui_error(_lang: Language, err: &dyn Display) -> String {
    format!("error: {err}")
}

pub fn fmt_language_changed(lang: Language) -> String {
    match lang {
        Language::Es => format!("idioma: {}", lang.display_name()),
        Language::En => format!("language: {}", lang.display_name()),
    }
}

/// Cadenas estáticas de la CLI.
pub struct CliStrings {
    pub err_listing_hwmon: &'static str,
    pub err_listing_nvidia: &'static str,
    pub no_temp_sensors: &'static str,
    pub sensors_temp_headers: [&'static str; 4],
    pub sensors_fan_headers: [&'static str; 5],
    pub devices_initialized: &'static str,
    pub warning_prefix: &'static str,
    pub err_duty_range: &'static str,
    pub label_lighting: &'static str,
    pub label_cooling: &'static str,
    pub no_rules: &'static str,
    pub no_curves: &'static str,
    pub daemon_reloaded: &'static str,
    pub daemon_reapplied: &'static str,
    pub daemon_profile_updated: &'static str,
    pub err_loading_pywal: &'static str,
}

static CLI_ES: CliStrings = CliStrings {
    err_listing_hwmon: "listando sensores hwmon",
    err_listing_nvidia: "listando sensores nvidia",
    no_temp_sensors: "No se encontraron sensores de temperatura.",
    sensors_temp_headers: ["DISPOSITIVO", "NOMBRE", "ETIQUETA", "TEMP"],
    sensors_fan_headers: ["DISPOSITIVO", "NOMBRE", "CANAL", "RPM", "DUTY"],
    devices_initialized: "Dispositivos inicializados",
    warning_prefix: "Aviso",
    err_duty_range: "duty debe estar entre 0 y 100",
    label_lighting: "Iluminación:",
    label_cooling: "Refrigeración:",
    no_rules: "  (sin reglas)",
    no_curves: "No hay curvas configuradas.",
    daemon_reloaded: "Configuración recargada",
    daemon_reapplied: "Perfil reaplicado",
    daemon_profile_updated: "Perfil actualizado",
    err_loading_pywal: "cargando paleta de colores de pywal",
};

static CLI_EN: CliStrings = CliStrings {
    err_listing_hwmon: "listing hwmon sensors",
    err_listing_nvidia: "listing nvidia sensors",
    no_temp_sensors: "No temperature sensors found.",
    sensors_temp_headers: ["DEVICE", "NAME", "LABEL", "TEMP"],
    sensors_fan_headers: ["DEVICE", "NAME", "CHANNEL", "RPM", "DUTY"],
    devices_initialized: "Devices initialized",
    warning_prefix: "Warning",
    err_duty_range: "duty must be between 0 and 100",
    label_lighting: "Lighting:",
    label_cooling: "Cooling:",
    no_rules: "  (no rules)",
    no_curves: "No curves configured.",
    daemon_reloaded: "Configuration reloaded",
    daemon_reapplied: "Profile reapplied",
    daemon_profile_updated: "Profile updated",
    err_loading_pywal: "loading pywal color palette",
};

/// Devuelve las cadenas CLI correspondientes al idioma indicado.
pub fn cli(lang: Language) -> &'static CliStrings {
    match lang {
        Language::Es => &CLI_ES,
        Language::En => &CLI_EN,
    }
}

pub fn fmt_config_exists(lang: Language, path: &Path) -> String {
    match lang {
        Language::Es => format!(
            "{} ya existe; usa --force para sobrescribir",
            path.display()
        ),
        Language::En => format!(
            "{} already exists; use --force to overwrite",
            path.display()
        ),
    }
}

pub fn fmt_config_created(lang: Language, path: &Path) -> String {
    match lang {
        Language::Es => format!("Configuración creada en {}", path.display()),
        Language::En => format!("Configuration created at {}", path.display()),
    }
}

pub fn fmt_devices_initialized_match(lang: Language, pattern: &str) -> String {
    match lang {
        Language::Es => format!("Dispositivos '{pattern}' inicializados"),
        Language::En => format!("Devices '{pattern}' initialized"),
    }
}

pub fn fmt_config_valid(
    lang: Language,
    default_profile: &str,
    profiles: usize,
    curves: usize,
) -> String {
    match lang {
        Language::Es => format!(
            "Configuración válida (perfil por defecto: '{default_profile}', {profiles} perfiles, {curves} curvas)"
        ),
        Language::En => format!(
            "Valid configuration (default profile: '{default_profile}', {profiles} profiles, {curves} curves)"
        ),
    }
}

pub fn fmt_applying_profile(lang: Language, name: &str) -> String {
    match lang {
        Language::Es => format!("aplicando perfil '{name}'"),
        Language::En => format!("applying profile '{name}'"),
    }
}

pub fn fmt_daemon_synced(lang: Language, name: &str) -> String {
    match lang {
        Language::Es => format!("Daemon sincronizado con perfil '{name}'"),
        Language::En => format!("Daemon synced with profile '{name}'"),
    }
}

pub fn fmt_daemon_sync_failed(lang: Language, err: &dyn Display) -> String {
    match lang {
        Language::Es => format!("Aviso: no se pudo sincronizar el daemon: {err}"),
        Language::En => format!("Warning: could not sync daemon: {err}"),
    }
}

pub fn fmt_color_applied(lang: Language, match_: &str, channel: &str) -> String {
    match lang {
        Language::Es => format!("Color aplicado a {match_}/{channel}"),
        Language::En => format!("Color applied to {match_}/{channel}"),
    }
}

pub fn fmt_speed_applied(lang: Language, duty: u8, match_: &str, channel: &str) -> String {
    match lang {
        Language::Es => format!("Velocidad {duty}% aplicada a {match_}/{channel}"),
        Language::En => format!("Speed {duty}% applied to {match_}/{channel}"),
    }
}

pub fn fmt_profile_not_found(lang: Language, name: &str) -> String {
    match lang {
        Language::Es => format!("perfil '{name}' no existe"),
        Language::En => format!("profile '{name}' does not exist"),
    }
}

pub fn fmt_profile_header(lang: Language, name: &str) -> String {
    match lang {
        Language::Es => format!("Perfil: {name}"),
        Language::En => format!("Profile: {name}"),
    }
}

pub fn fmt_profile_description(lang: Language, desc: &str) -> String {
    match lang {
        Language::Es => format!("Descripción: {desc}"),
        Language::En => format!("Description: {desc}"),
    }
}

pub fn fmt_curve_not_found(lang: Language, name: &str) -> String {
    match lang {
        Language::Es => format!("curva '{name}' no existe"),
        Language::En => format!("curve '{name}' does not exist"),
    }
}

pub fn fmt_curve_header(lang: Language, name: &str) -> String {
    match lang {
        Language::Es => format!("Curva: {name}"),
        Language::En => format!("Curve: {name}"),
    }
}

pub fn fmt_connecting_daemon(lang: Language, sock: &Path) -> String {
    match lang {
        Language::Es => format!("conectando con nxctld en {}", sock.display()),
        Language::En => format!("connecting to nxctld at {}", sock.display()),
    }
}

pub fn fmt_daemon_status_lines(lang: Language, daemon: &DaemonStatus) -> [String; 5] {
    match lang {
        Language::Es => [
            format!("Perfil activo:      {}", daemon.active_profile),
            format!(
                "Configuración:      {}",
                daemon.config_path.as_deref().unwrap_or("(por defecto)")
            ),
            format!("Intervalo de sondeo: {}s", daemon.poll_interval_secs),
            format!("Histéresis:         {:.1} °C", daemon.hysteresis_celsius),
            format!("Delta mínimo duty:  {}%", daemon.min_duty_delta),
        ],
        Language::En => [
            format!("Active profile:     {}", daemon.active_profile),
            format!(
                "Configuration:      {}",
                daemon.config_path.as_deref().unwrap_or("(default)")
            ),
            format!("Poll interval:      {}s", daemon.poll_interval_secs),
            format!("Hysteresis:         {:.1} °C", daemon.hysteresis_celsius),
            format!("Min duty delta:     {}%", daemon.min_duty_delta),
        ],
    }
}

pub fn fmt_pywal_applied(lang: Language, applied: usize) -> String {
    match lang {
        Language::Es => format!("Colores de pywal aplicados ({applied} objetivo(s))"),
        Language::En => format!("Pywal colors applied ({applied} target(s))"),
    }
}

pub fn fmt_lighting_rule(
    lang: Language,
    match_device: &str,
    channel: &str,
    mode: &str,
    colors: &[String],
) -> String {
    let word = match lang {
        Language::Es => "modo",
        Language::En => "mode",
    };
    if colors.is_empty() {
        format!("{match_device}/{channel} -> {word} {mode}")
    } else {
        format!(
            "{match_device}/{channel} -> {word} {mode} [{}]",
            colors.join(", ")
        )
    }
}

pub fn fmt_cooling_fixed(lang: Language, match_device: &str, channel: &str, duty: u8) -> String {
    let word = match lang {
        Language::Es => "fijo",
        Language::En => "fixed",
    };
    format!("{match_device}/{channel} -> {word} {duty}%")
}

pub fn fmt_cooling_curve(
    lang: Language,
    match_device: &str,
    channel: &str,
    curve: &str,
    sensor_desc: Option<&str>,
) -> String {
    let word = match lang {
        Language::Es => "curva",
        Language::En => "curve",
    };
    match sensor_desc {
        Some(desc) => format!("{match_device}/{channel} -> {word} '{curve}' (sensor {desc})"),
        None => format!("{match_device}/{channel} -> {word} '{curve}'"),
    }
}

pub fn fmt_apply_report(lang: Language, name: &str, lighting: usize, cooling: usize) -> String {
    match lang {
        Language::Es => {
            format!("Perfil '{name}' aplicado: {lighting} iluminación, {cooling} refrigeración")
        }
        Language::En => {
            format!("Profile '{name}' applied: {lighting} lighting, {cooling} cooling")
        }
    }
}
