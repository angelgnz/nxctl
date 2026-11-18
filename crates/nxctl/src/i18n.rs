//! Localización (Español e Inglés) para `nxctl` (CLI y TUI).

use std::fmt::Display;
use std::path::Path;

use nxctl_core::{DaemonStatus, Language};

/// Cadenas estáticas de la interfaz TUI.
#[allow(dead_code)] // Campos nuevos reservados para la ampliación de `tui.rs`
pub struct TuiStrings {
    // Existentes (compatibilidad con vista legacy)
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
    pub help_lines: [&'static str; 15],
    pub footer: &'static str,
    pub no_profiles: &'static str,
    pub reloaded: &'static str,
    pub refreshing: &'static str,

    // Navegación (4 vistas)
    pub nav_dashboard: &'static str,
    pub nav_fans: &'static str,
    pub nav_rgb: &'static str,
    pub nav_profiles: &'static str,

    // Sidebar & Top bar
    pub daemon_label: &'static str,
    pub sys_ok: &'static str,
    pub offline: &'static str,
    pub language_label: &'static str,
    pub dry_run_badge: &'static str,
    pub active_profile_label: &'static str,
    pub btn_reload: &'static str,
    pub btn_reapply: &'static str,

    // Dashboard
    pub aio_liquid: &'static str,
    pub liquid_sub: &'static str,
    pub pwm_headers: &'static str,
    pub no_nzxt_channels: &'static str,
    pub other_sensors: &'static str,
    pub no_additional_sensors: &'static str,
    pub feature: &'static str,
    pub demote: &'static str,
    pub net_tag: &'static str,

    // Ventiladores (Fans)
    pub no_cooling_channels: &'static str,
    pub curve_label: &'static str,
    pub manual_duty_label: &'static str,

    // Iluminación RGB (Rgb)
    pub pywal_sync_title: &'static str,
    pub enabled: &'static str,
    pub disabled: &'static str,
    pub pywal_auto_sync: &'static str,
    pub global_mode: &'static str,
    pub filter_extremes: &'static str,
    pub saturate_label: &'static str,
    pub shift_palette: &'static str,
    pub sync_now: &'static str,
    pub pywal_not_detected: &'static str,
    pub no_nzxt_devices: &'static str,
    pub sync_whole_device: &'static str,
    pub rgb_channels_title: &'static str,

    // Perfiles (Profiles)
    pub badge_active: &'static str,
    pub badge_default: &'static str,
    pub profile_detail_title: &'static str,
    pub no_lighting_rules: &'static str,
    pub no_cooling_rules: &'static str,

    // Pies de página contextuales por vista
    pub footer_dashboard: &'static str,
    pub footer_fans: &'static str,
    pub footer_rgb: &'static str,
    pub footer_profiles: &'static str,
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
        "1-4          cambiar vista (Dash/Fans/RGB/Perf.)",
        "[ / ]        cambiar perfil activo",
        "Tab          cambiar foco entre paneles",
        "↑/↓  j/k     mover selección",
        "←/→  -/+     ajustar duty o paleta",
        "Enter / a    aplicar selección",
        "f            destacar / quitar sensor",
        "c / m        ciclar curva o modo RGB",
        "t / x / s    pywal: auto / filtro / sync ahora",
        "n / d / g    nuevo / default / autogen TOML",
        "w / Del      exportar / eliminar perfil",
        "p            reaplicar perfil actual",
        "r            recargar / actualizar",
        "l            cambiar idioma (ES/EN)",
        "? / q / Esc  ayuda / salir / cerrar",
    ],
    footer: "q salir · Tab foco · ↑/↓ j/k desplazar · Enter/a aplicar · c curva · r recargar · l idioma · ? ayuda",
    no_profiles: "sin perfiles",
    reloaded: "recargado",
    refreshing: "actualizando…",

    nav_dashboard: "Dashboard",
    nav_fans: "Ventiladores",
    nav_rgb: "Iluminación RGB",
    nav_profiles: "Perfiles",

    daemon_label: "DAEMON",
    sys_ok: "SYS OK",
    offline: "OFFLINE",
    language_label: "IDIOMA",
    dry_run_badge: "DRY-RUN",
    active_profile_label: "PERFIL ACTIVO",
    btn_reload: "Recargar",
    btn_reapply: "Reaplicar",

    aio_liquid: "AIO (Líquido)",
    liquid_sub: "LÍQUIDO",
    pwm_headers: "Headers PWM",
    no_nzxt_channels: "Sin canales NZXT detectados",
    other_sensors: "Otros Sensores (RAM / SSD / Sistema)",
    no_additional_sensors: "Sin sensores adicionales detectados",
    feature: "+ Anclar",
    demote: "− Desanclar",
    net_tag: "RED",

    no_cooling_channels: "Sin canales de ventilación detectados",
    curve_label: "Curva",
    manual_duty_label: "Duty manual",

    pywal_sync_title: "Pywal Sync",
    enabled: "HABILITADO",
    disabled: "DESHABILITADO",
    pywal_auto_sync: "Auto-sync con pywal (daemon)",
    global_mode: "Modo global:",
    filter_extremes: "Filtrar muy blancos / negros",
    saturate_label: "Saturar:",
    shift_palette: "Mover paleta:",
    sync_now: "Sincronizar ahora",
    pywal_not_detected: "pywal no detectado (~/.cache/wal/colors.json)",
    no_nzxt_devices: "Sin dispositivos NZXT detectados",
    sync_whole_device: "sync (todo el dispositivo)",
    rgb_channels_title: "Canales RGB",

    badge_active: "ACTIVO",
    badge_default: "DEFAULT",
    profile_detail_title: "Detalle del perfil",
    no_lighting_rules: "Sin reglas de iluminación",
    no_cooling_rules: "Sin reglas de refrigeración",

    footer_dashboard: "1-4 vistas · [/] perfil · Tab foco · ↑/↓ navegar · f destacar · p reaplicar · r recargar · l idioma · ? ayuda · q salir",
    footer_fans: "1-4 vistas · ↑/↓ canal · c curva · ←/→ -/+ duty · Enter aplicar · p reaplicar · r recargar · l idioma · ? ayuda · q salir",
    footer_rgb: "1-4 vistas · Tab foco · ↑/↓ canal · m modo · ←/→ paleta · t auto · x filtro · v saturar · s sync · Enter aplicar · ? ayuda · q salir",
    footer_profiles: "1-4 vistas · ↑/↓ perfil · Enter/a aplicar · n nuevo · g autogen · d default · w exportar · Del eliminar · r recargar · ? ayuda · q salir",
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
        "1-4          switch view (Dash/Fans/RGB/Prof.)",
        "[ / ]        switch active profile",
        "Tab          switch panel focus",
        "↑/↓  j/k     move selection",
        "←/→  -/+     adjust duty or palette",
        "Enter / a    apply selection",
        "f            feature / demote sensor",
        "c / m        cycle curve or RGB mode",
        "t / x / s    pywal: auto / filter / sync now",
        "n / d / g    new / default / autogen TOML",
        "w / Del      export / delete profile",
        "p            reapply active profile",
        "r            reload / refresh",
        "l            switch language (ES/EN)",
        "? / q / Esc  help / quit / close",
    ],
    footer: "q quit · Tab focus · ↑/↓ j/k scroll · Enter/a apply · c curve · r reload · l lang · ? help",
    no_profiles: "no profiles",
    reloaded: "reloaded",
    refreshing: "refreshing…",

    nav_dashboard: "Dashboard",
    nav_fans: "Fans",
    nav_rgb: "RGB Lighting",
    nav_profiles: "Profiles",

    daemon_label: "DAEMON",
    sys_ok: "SYS OK",
    offline: "OFFLINE",
    language_label: "LANGUAGE",
    dry_run_badge: "DRY-RUN",
    active_profile_label: "ACTIVE PROFILE",
    btn_reload: "Reload",
    btn_reapply: "Reapply",

    aio_liquid: "AIO (Liquid)",
    liquid_sub: "LIQUID",
    pwm_headers: "PWM Headers",
    no_nzxt_channels: "No NZXT channels detected",
    other_sensors: "Other Sensors (RAM / SSD / System)",
    no_additional_sensors: "No additional sensors detected",
    feature: "+ Pin",
    demote: "− Unpin",
    net_tag: "NET",

    no_cooling_channels: "No cooling channels detected",
    curve_label: "Curve",
    manual_duty_label: "Manual duty",

    pywal_sync_title: "Pywal Sync",
    enabled: "ENABLED",
    disabled: "DISABLED",
    pywal_auto_sync: "Auto-sync with pywal (daemon)",
    global_mode: "Global mode:",
    filter_extremes: "Filter extreme whites / blacks",
    saturate_label: "Saturate:",
    shift_palette: "Shift palette:",
    sync_now: "Sync now",
    pywal_not_detected: "pywal not detected (~/.cache/wal/colors.json)",
    no_nzxt_devices: "No NZXT devices detected",
    sync_whole_device: "sync (whole device)",
    rgb_channels_title: "RGB Channels",

    badge_active: "ACTIVE",
    badge_default: "DEFAULT",
    profile_detail_title: "Profile detail",
    no_lighting_rules: "No lighting rules",
    no_cooling_rules: "No cooling rules",

    footer_dashboard: "1-4 views · [/] profile · Tab focus · ↑/↓ navigate · f feature · p reapply · r reload · l lang · ? help · q quit",
    footer_fans: "1-4 views · ↑/↓ channel · c curve · ←/→ -/+ duty · Enter apply · p reapply · r reload · l lang · ? help · q quit",
    footer_rgb: "1-4 views · Tab focus · ↑/↓ channel · m mode · ←/→ palette · t auto · x filter · v saturate · s sync · Enter apply · ? help · q quit",
    footer_profiles: "1-4 views · ↑/↓ profile · Enter/a apply · n new · g autogen · d default · w export · Del delete · r reload · ? help · q quit",
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

/// Formatea la línea resumen de telemetría de la barra superior.
#[allow(dead_code)]
pub fn fmt_top_summary(lang: Language, cpu: &str, gpu: &str, devs: usize) -> String {
    match lang {
        Language::Es => format!("CPU {cpu}   GPU {gpu}   NZXT {devs} disp."),
        Language::En => format!("CPU {cpu}   GPU {gpu}   NZXT {devs} dev."),
    }
}

/// Formatea la insignia con el número de canales en tarjetas de ventiladores y RGB.
#[allow(dead_code)]
pub fn fmt_channels_badge(lang: Language, count: usize) -> String {
    match lang {
        Language::Es => format!("{count} CANALES"),
        Language::En => format!("{count} CHANNELS"),
    }
}

/// Formatea la lectura del punto operativo bajo la curva de ventilación.
#[allow(dead_code)]
pub fn fmt_operating_point(lang: Language, temp_c: f32, duty: u8) -> String {
    match lang {
        Language::Es => format!("Punto operativo: {temp_c:.1}°C -> {duty}%"),
        Language::En => format!("Operating point: {temp_c:.1}°C -> {duty}%"),
    }
}

/// Formatea la etiqueta del fondo de pantalla activo en pywal.
#[allow(dead_code)]
pub fn fmt_wallpaper(lang: Language, name: &str) -> String {
    match lang {
        Language::Es => format!("Fondo: {name}"),
        Language::En => format!("Wallpaper: {name}"),
    }
}

/// Formatea el mensaje de estado tras sincronizar pywal.
#[allow(dead_code)]
pub fn fmt_pywal_synced(lang: Language, count: usize) -> String {
    match lang {
        Language::Es => format!("Pywal sincronizado ({count} objetivo(s))"),
        Language::En => format!("Pywal synced ({count} target(s))"),
    }
}

/// Formatea el mensaje de estado tras fijar la velocidad manual de un canal.
#[allow(dead_code)]
pub fn fmt_speed_set(lang: Language, dev: &str, ch: &str, duty: u8) -> String {
    match lang {
        Language::Es => format!("{dev}/{ch}: velocidad {duty}% aplicada"),
        Language::En => format!("{dev}/{ch}: speed {duty}% applied"),
    }
}

/// Formatea el mensaje de estado tras aplicar un modo RGB a un canal.
#[allow(dead_code)]
pub fn fmt_color_set(lang: Language, dev: &str, ch: &str, mode: &str) -> String {
    match lang {
        Language::Es => format!("{dev}/{ch}: {mode} aplicado"),
        Language::En => format!("{dev}/{ch}: {mode} applied"),
    }
}

/// Formatea el mensaje de estado tras aplicar un color a todos los dispositivos.
#[allow(dead_code)]
pub fn fmt_color_all(lang: Language, hex: &str, mode: &str) -> String {
    let hex = hex.trim_start_matches('#');
    match lang {
        Language::Es => format!("Color #{hex} ({mode}) aplicado a todos los dispositivos"),
        Language::En => format!("Color #{hex} ({mode}) applied to all devices"),
    }
}

/// Formatea el resumen de reglas mostrado en la lista/tarjeta de cada perfil.
#[allow(dead_code)]
pub fn fmt_profile_rules_summary(
    lang: Language,
    lighting_len: usize,
    cooling_len: usize,
) -> String {
    match lang {
        Language::Es => {
            format!("{lighting_len} reglas de iluminación · {cooling_len} reglas de refrigeración")
        }
        Language::En => {
            format!("{lighting_len} lighting rules · {cooling_len} cooling rules")
        }
    }
}

/// Formatea el mensaje de perfil guardado.
#[allow(dead_code)]
pub fn fmt_profile_saved(lang: Language, name: &str) -> String {
    match lang {
        Language::Es => format!("Perfil '{name}' guardado"),
        Language::En => format!("Profile '{name}' saved"),
    }
}

/// Formatea el mensaje de perfil clonado.
#[allow(dead_code)]
pub fn fmt_profile_cloned(lang: Language, name: &str) -> String {
    match lang {
        Language::Es => format!("Perfil '{name}' clonado"),
        Language::En => format!("Profile '{name}' cloned"),
    }
}

/// Formatea el mensaje de perfil exportado a disco.
#[allow(dead_code)]
pub fn fmt_profile_exported(lang: Language, name: &str, path: &str) -> String {
    match lang {
        Language::Es => format!("Perfil '{name}' exportado a {path}"),
        Language::En => format!("Profile '{name}' exported to {path}"),
    }
}

/// Formatea el mensaje de perfil eliminado.
#[allow(dead_code)]
pub fn fmt_profile_deleted(lang: Language, name: &str) -> String {
    match lang {
        Language::Es => format!("Perfil '{name}' eliminado"),
        Language::En => format!("Profile '{name}' deleted"),
    }
}

/// Formatea el mensaje al establecer un perfil por defecto.
#[allow(dead_code)]
pub fn fmt_default_profile_set(lang: Language, name: &str) -> String {
    match lang {
        Language::Es => format!("'{name}' establecido como perfil por defecto"),
        Language::En => format!("'{name}' set as default profile"),
    }
}

/// Formatea el mensaje de configuración guardada.
#[allow(dead_code)]
pub fn fmt_config_saved(lang: Language) -> String {
    match lang {
        Language::Es => "Configuración guardada".to_string(),
        Language::En => "Configuration saved".to_string(),
    }
}

/// Formatea el mensaje al autogenerar la configuración TOML desde el hardware.
pub fn fmt_config_autogenerated(lang: Language) -> String {
    match lang {
        Language::Es => "Configuración TOML autogenerada desde hardware".to_string(),
        Language::En => "TOML configuration auto-generated from hardware".to_string(),
    }
}

/// Formatea el mensaje de configuración recargada.
#[allow(dead_code)]
pub fn fmt_config_reloaded(lang: Language) -> String {
    match lang {
        Language::Es => "Configuración recargada".to_string(),
        Language::En => "Configuration reloaded".to_string(),
    }
}

/// Formatea el aviso cuando no se puede eliminar un perfil.
#[allow(dead_code)]
pub fn fmt_cannot_delete_profile(lang: Language, name: &str) -> String {
    match lang {
        Language::Es => format!("No se puede eliminar el perfil '{name}'"),
        Language::En => format!("Cannot delete profile '{name}'"),
    }
}

/// Formatea el mensaje de cambio de curva para un dispositivo/canal.
#[allow(dead_code)]
pub fn fmt_curve_changed(lang: Language, dev: &str, curve: &str) -> String {
    match lang {
        Language::Es => format!("{dev}: curva cambiada a '{curve}'"),
        Language::En => format!("{dev}: curve changed to '{curve}'"),
    }
}

/// Formatea el mensaje al destacar un sensor en el dashboard.
#[allow(dead_code)]
pub fn fmt_sensor_promoted(lang: Language, id: &str) -> String {
    match lang {
        Language::Es => format!("Sensor '{id}' destacado"),
        Language::En => format!("Sensor '{id}' featured"),
    }
}

/// Formatea el mensaje al mover un sensor al listado general.
#[allow(dead_code)]
pub fn fmt_sensor_demoted(lang: Language, id: &str) -> String {
    match lang {
        Language::Es => format!("Sensor '{id}' movido al listado"),
        Language::En => format!("Sensor '{id}' demoted to list"),
    }
}

/// Formatea el mensaje al activar/desactivar auto-sync de pywal.
#[allow(dead_code)]
pub fn fmt_pywal_toggled(lang: Language, enabled: bool) -> String {
    match (lang, enabled) {
        (Language::Es, true) => "Pywal auto-sync habilitado".to_string(),
        (Language::Es, false) => "Pywal auto-sync deshabilitado".to_string(),
        (Language::En, true) => "Pywal auto-sync enabled".to_string(),
        (Language::En, false) => "Pywal auto-sync disabled".to_string(),
    }
}

/// Formatea el mensaje al cambiar el modo global de pywal.
#[allow(dead_code)]
pub fn fmt_pywal_mode_changed(lang: Language, mode: &str) -> String {
    match lang {
        Language::Es => format!("Modo pywal cambiado a '{mode}'"),
        Language::En => format!("Pywal mode changed to '{mode}'"),
    }
}

/// Formatea el mensaje al rotar/actualizar la paleta de pywal.
#[allow(dead_code)]
pub fn fmt_pywal_palette_updated(lang: Language) -> String {
    match lang {
        Language::Es => "Paleta pywal actualizada".to_string(),
        Language::En => "Pywal palette updated".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tui_tables_have_expected_translations() {
        let es = tui(Language::Es);
        let en = tui(Language::En);

        // Navegación
        assert_eq!(es.nav_dashboard, "Dashboard");
        assert_eq!(en.nav_dashboard, "Dashboard");
        assert_eq!(es.nav_fans, "Ventiladores");
        assert_eq!(en.nav_fans, "Fans");
        assert_eq!(es.nav_rgb, "Iluminación RGB");
        assert_eq!(en.nav_rgb, "RGB Lighting");
        assert_eq!(es.nav_profiles, "Perfiles");
        assert_eq!(en.nav_profiles, "Profiles");

        // Sidebar & Top bar
        assert_eq!(es.daemon_label, "DAEMON");
        assert_eq!(en.daemon_label, "DAEMON");
        assert_eq!(es.sys_ok, "SYS OK");
        assert_eq!(en.sys_ok, "SYS OK");
        assert_eq!(es.offline, "OFFLINE");
        assert_eq!(en.offline, "OFFLINE");
        assert_eq!(es.language_label, "IDIOMA");
        assert_eq!(en.language_label, "LANGUAGE");
        assert_eq!(es.dry_run_badge, "DRY-RUN");
        assert_eq!(en.dry_run_badge, "DRY-RUN");
        assert_eq!(es.active_profile_label, "PERFIL ACTIVO");
        assert_eq!(en.active_profile_label, "ACTIVE PROFILE");
        assert_eq!(es.btn_reload, "Recargar");
        assert_eq!(en.btn_reload, "Reload");
        assert_eq!(es.btn_reapply, "Reaplicar");
        assert_eq!(en.btn_reapply, "Reapply");

        // Dashboard
        assert_eq!(es.aio_liquid, "AIO (Líquido)");
        assert_eq!(en.aio_liquid, "AIO (Liquid)");
        assert_eq!(es.liquid_sub, "LÍQUIDO");
        assert_eq!(en.liquid_sub, "LIQUID");
        assert_eq!(es.pwm_headers, "Headers PWM");
        assert_eq!(en.pwm_headers, "PWM Headers");
        assert_eq!(es.no_nzxt_channels, "Sin canales NZXT detectados");
        assert_eq!(en.no_nzxt_channels, "No NZXT channels detected");
        assert_eq!(es.other_sensors, "Otros Sensores (RAM / SSD / Sistema)");
        assert_eq!(en.other_sensors, "Other Sensors (RAM / SSD / System)");
        assert_eq!(
            es.no_additional_sensors,
            "Sin sensores adicionales detectados"
        );
        assert_eq!(en.no_additional_sensors, "No additional sensors detected");
        assert_eq!(es.feature, "+ Anclar");
        assert_eq!(en.feature, "+ Pin");
        assert_eq!(es.demote, "− Desanclar");
        assert_eq!(en.demote, "− Unpin");
        assert_eq!(es.net_tag, "RED");
        assert_eq!(en.net_tag, "NET");

        // Ventiladores
        assert_eq!(
            es.no_cooling_channels,
            "Sin canales de ventilación detectados"
        );
        assert_eq!(en.no_cooling_channels, "No cooling channels detected");
        assert_eq!(es.curve_label, "Curva");
        assert_eq!(en.curve_label, "Curve");
        assert_eq!(es.curve_current_label, "actual");
        assert_eq!(en.curve_current_label, "current");
        assert_eq!(es.manual_duty_label, "Duty manual");
        assert_eq!(en.manual_duty_label, "Manual duty");

        // Iluminación RGB
        assert_eq!(es.pywal_sync_title, "Pywal Sync");
        assert_eq!(en.pywal_sync_title, "Pywal Sync");
        assert_eq!(es.enabled, "HABILITADO");
        assert_eq!(en.enabled, "ENABLED");
        assert_eq!(es.disabled, "DESHABILITADO");
        assert_eq!(en.disabled, "DISABLED");
        assert_eq!(es.pywal_auto_sync, "Auto-sync con pywal (daemon)");
        assert_eq!(en.pywal_auto_sync, "Auto-sync with pywal (daemon)");
        assert_eq!(es.global_mode, "Modo global:");
        assert_eq!(en.global_mode, "Global mode:");
        assert_eq!(es.filter_extremes, "Filtrar muy blancos / negros");
        assert_eq!(en.filter_extremes, "Filter extreme whites / blacks");
        assert_eq!(es.saturate_label, "Saturar:");
        assert_eq!(en.saturate_label, "Saturate:");
        assert_eq!(es.shift_palette, "Mover paleta:");
        assert_eq!(en.shift_palette, "Shift palette:");
        assert_eq!(es.sync_now, "Sincronizar ahora");
        assert_eq!(en.sync_now, "Sync now");
        assert_eq!(
            es.pywal_not_detected,
            "pywal no detectado (~/.cache/wal/colors.json)"
        );
        assert_eq!(
            en.pywal_not_detected,
            "pywal not detected (~/.cache/wal/colors.json)"
        );
        assert_eq!(es.no_nzxt_devices, "Sin dispositivos NZXT detectados");
        assert_eq!(en.no_nzxt_devices, "No NZXT devices detected");
        assert_eq!(es.sync_whole_device, "sync (todo el dispositivo)");
        assert_eq!(en.sync_whole_device, "sync (whole device)");
        assert_eq!(es.rgb_channels_title, "Canales RGB");
        assert_eq!(en.rgb_channels_title, "RGB Channels");

        // Perfiles
        assert_eq!(es.badge_active, "ACTIVO");
        assert_eq!(en.badge_active, "ACTIVE");
        assert_eq!(es.badge_default, "DEFAULT");
        assert_eq!(en.badge_default, "DEFAULT");
        assert_eq!(es.profile_detail_title, "Detalle del perfil");
        assert_eq!(en.profile_detail_title, "Profile detail");
        assert_eq!(es.no_lighting_rules, "Sin reglas de iluminación");
        assert_eq!(en.no_lighting_rules, "No lighting rules");
        assert_eq!(es.no_cooling_rules, "Sin reglas de refrigeración");
        assert_eq!(en.no_cooling_rules, "No cooling rules");

        // Footers y ayuda
        for (footer_es, footer_en) in [
            (es.footer_dashboard, en.footer_dashboard),
            (es.footer_fans, en.footer_fans),
            (es.footer_rgb, en.footer_rgb),
            (es.footer_profiles, en.footer_profiles),
        ] {
            assert!(!footer_es.is_empty());
            assert!(!footer_en.is_empty());
            assert_ne!(footer_es, footer_en);
        }

        assert_eq!(es.help_lines.len(), 15);
        assert_eq!(en.help_lines.len(), 15);
        for line in es.help_lines.iter().chain(en.help_lines.iter()) {
            assert!(!line.is_empty());
            assert!(
                line.chars().count() <= 54,
                "help line exceeds popup width: {line}"
            );
        }
        let joined_es = es.help_lines.join("\n");
        let joined_en = en.help_lines.join("\n");
        for token in [
            "1-4", "[ / ]", "Tab", "j/k", "-/+", "Enter", "f", "c", "m", "t", "x", "s", "n", "d",
            "g", "w", "Del", "p", "r", "l", "?", "q", "Esc",
        ] {
            assert!(joined_es.contains(token), "ES help missing {token}");
            assert!(joined_en.contains(token), "EN help missing {token}");
        }
        assert!(joined_en.contains("switch language (ES/EN)"));
    }

    #[test]
    fn tui_formatters_render_es_and_en() {
        assert_eq!(
            fmt_top_summary(Language::Es, "45°C", "52°C", 2),
            "CPU 45°C   GPU 52°C   NZXT 2 disp."
        );
        assert_eq!(
            fmt_top_summary(Language::En, "45°C", "52°C", 2),
            "CPU 45°C   GPU 52°C   NZXT 2 dev."
        );

        assert_eq!(fmt_channels_badge(Language::Es, 4), "4 CANALES");
        assert_eq!(fmt_channels_badge(Language::En, 4), "4 CHANNELS");

        assert_eq!(
            fmt_operating_point(Language::Es, 34.5, 60),
            "Punto operativo: 34.5°C -> 60%"
        );
        assert_eq!(
            fmt_operating_point(Language::En, 34.5, 60),
            "Operating point: 34.5°C -> 60%"
        );

        assert_eq!(fmt_wallpaper(Language::Es, "bg.png"), "Fondo: bg.png");
        assert_eq!(fmt_wallpaper(Language::En, "bg.png"), "Wallpaper: bg.png");

        assert_eq!(
            fmt_pywal_synced(Language::Es, 3),
            "Pywal sincronizado (3 objetivo(s))"
        );
        assert_eq!(
            fmt_pywal_synced(Language::En, 3),
            "Pywal synced (3 target(s))"
        );

        assert_eq!(
            fmt_speed_set(Language::Es, "Kraken", "pump", 75),
            "Kraken/pump: velocidad 75% aplicada"
        );
        assert_eq!(
            fmt_speed_set(Language::En, "Kraken", "pump", 75),
            "Kraken/pump: speed 75% applied"
        );

        assert_eq!(
            fmt_color_set(Language::Es, "Kraken", "ring", "breathing"),
            "Kraken/ring: breathing aplicado"
        );
        assert_eq!(
            fmt_color_set(Language::En, "Kraken", "ring", "breathing"),
            "Kraken/ring: breathing applied"
        );

        assert_eq!(
            fmt_color_all(Language::Es, "#00f0ff", "fixed"),
            "Color #00f0ff (fixed) aplicado a todos los dispositivos"
        );
        assert_eq!(
            fmt_color_all(Language::En, "00f0ff", "fixed"),
            "Color #00f0ff (fixed) applied to all devices"
        );

        assert_eq!(
            fmt_profile_rules_summary(Language::Es, 2, 3),
            "2 reglas de iluminación · 3 reglas de refrigeración"
        );
        assert_eq!(
            fmt_profile_rules_summary(Language::En, 2, 3),
            "2 lighting rules · 3 cooling rules"
        );

        assert_eq!(
            fmt_profile_saved(Language::Es, "p1"),
            "Perfil 'p1' guardado"
        );
        assert_eq!(fmt_profile_saved(Language::En, "p1"), "Profile 'p1' saved");

        assert_eq!(
            fmt_profile_cloned(Language::Es, "p1_copia"),
            "Perfil 'p1_copia' clonado"
        );
        assert_eq!(
            fmt_profile_cloned(Language::En, "p1_copy"),
            "Profile 'p1_copy' cloned"
        );

        assert_eq!(
            fmt_profile_exported(Language::Es, "p1", "/tmp/p1.toml"),
            "Perfil 'p1' exportado a /tmp/p1.toml"
        );
        assert_eq!(
            fmt_profile_exported(Language::En, "p1", "/tmp/p1.toml"),
            "Profile 'p1' exported to /tmp/p1.toml"
        );

        assert_eq!(
            fmt_profile_deleted(Language::Es, "p1"),
            "Perfil 'p1' eliminado"
        );
        assert_eq!(
            fmt_profile_deleted(Language::En, "p1"),
            "Profile 'p1' deleted"
        );

        assert_eq!(
            fmt_default_profile_set(Language::Es, "silent"),
            "'silent' establecido como perfil por defecto"
        );
        assert_eq!(
            fmt_default_profile_set(Language::En, "silent"),
            "'silent' set as default profile"
        );

        assert_eq!(fmt_config_saved(Language::Es), "Configuración guardada");
        assert_eq!(fmt_config_saved(Language::En), "Configuration saved");
        assert_eq!(
            fmt_config_autogenerated(Language::Es),
            "Configuración TOML autogenerada desde hardware"
        );
        assert_eq!(
            fmt_config_autogenerated(Language::En),
            "TOML configuration auto-generated from hardware"
        );

        assert_eq!(fmt_config_reloaded(Language::Es), "Configuración recargada");
        assert_eq!(fmt_config_reloaded(Language::En), "Configuration reloaded");

        assert_eq!(
            fmt_cannot_delete_profile(Language::Es, "default"),
            "No se puede eliminar el perfil 'default'"
        );
        assert_eq!(
            fmt_cannot_delete_profile(Language::En, "default"),
            "Cannot delete profile 'default'"
        );

        assert_eq!(
            fmt_curve_changed(Language::Es, "Kraken", "performance"),
            "Kraken: curva cambiada a 'performance'"
        );
        assert_eq!(
            fmt_curve_changed(Language::En, "Kraken", "performance"),
            "Kraken: curve changed to 'performance'"
        );

        assert_eq!(
            fmt_sensor_promoted(Language::Es, "nvme0"),
            "Sensor 'nvme0' destacado"
        );
        assert_eq!(
            fmt_sensor_promoted(Language::En, "nvme0"),
            "Sensor 'nvme0' featured"
        );

        assert_eq!(
            fmt_sensor_demoted(Language::Es, "nvme0"),
            "Sensor 'nvme0' movido al listado"
        );
        assert_eq!(
            fmt_sensor_demoted(Language::En, "nvme0"),
            "Sensor 'nvme0' demoted to list"
        );

        assert_eq!(
            fmt_pywal_toggled(Language::Es, true),
            "Pywal auto-sync habilitado"
        );
        assert_eq!(
            fmt_pywal_toggled(Language::Es, false),
            "Pywal auto-sync deshabilitado"
        );
        assert_eq!(
            fmt_pywal_toggled(Language::En, true),
            "Pywal auto-sync enabled"
        );
        assert_eq!(
            fmt_pywal_toggled(Language::En, false),
            "Pywal auto-sync disabled"
        );

        assert_eq!(
            fmt_pywal_mode_changed(Language::Es, "fading"),
            "Modo pywal cambiado a 'fading'"
        );
        assert_eq!(
            fmt_pywal_mode_changed(Language::En, "fading"),
            "Pywal mode changed to 'fading'"
        );

        assert_eq!(
            fmt_pywal_palette_updated(Language::Es),
            "Paleta pywal actualizada"
        );
        assert_eq!(
            fmt_pywal_palette_updated(Language::En),
            "Pywal palette updated"
        );
    }
}
