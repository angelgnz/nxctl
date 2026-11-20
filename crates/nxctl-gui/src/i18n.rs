//! Localization strings and formatters for `nxctl-gui` (Spanish & English).

use std::fmt::Display;

use nxctl_core::Language;

/// Static UI strings for `nxctl-gui`.
pub struct GuiStrings {
    // Navigation
    pub nav_section: &'static str,
    pub nav_dashboard: &'static str,
    pub nav_fans: &'static str,
    pub nav_rgb: &'static str,
    pub nav_profiles: &'static str,

    // Sidebar & topbar
    pub daemon_label: &'static str,
    pub daemon_online: &'static str,
    pub daemon_offline: &'static str,
    pub quick_profile_title: &'static str,
    pub language_label: &'static str,
    pub close_to_tray: &'static str,
    pub dry_run_mode: &'static str,

    // Dashboard
    pub aio_liquid: &'static str,
    pub liquid_sub: &'static str,
    pub liquid_label: &'static str,
    pub cpu_sub: &'static str,
    pub gpu_sub: &'static str,
    pub demote: &'static str,
    pub pwm_headers: &'static str,
    pub no_nzxt_channels: &'static str,
    pub other_sensors: &'static str,
    pub no_additional_sensors: &'static str,
    pub feature: &'static str,
    pub net_tag: &'static str,
    pub drag_card_tooltip: &'static str,

    // Fans
    pub no_cooling_channels: &'static str,
    pub curve: &'static str,
    pub nodes: &'static str,
    pub operating: &'static str,

    // RGB
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
    pub apply: &'static str,

    // Profiles
    pub new_profile: &'static str,
    pub save_config: &'static str,
    pub autogen_config: &'static str,
    pub import_toml: &'static str,
    pub import_hint: &'static str,
    pub load: &'static str,
    pub profile_editor: &'static str,
    pub capture_current_state: &'static str,
    pub name_label: &'static str,
    pub profile_name_hint: &'static str,
    pub description_label: &'static str,
    pub optional_hint: &'static str,
    pub fixed: &'static str,
    pub save_profile: &'static str,
    pub cancel: &'static str,
    pub active: &'static str,
    pub default_badge: &'static str,
    pub delete: &'static str,
    pub clone: &'static str,
    pub export: &'static str,
    pub set_default: &'static str,
}

/// Spanish UI string table.
pub const ES_STRINGS: GuiStrings = GuiStrings {
    nav_section: "Navegación",
    nav_dashboard: "Dashboard",
    nav_fans: "Ventiladores",
    nav_rgb: "Iluminación RGB",
    nav_profiles: "Perfiles",

    daemon_label: "Daemon",
    daemon_online: "Conectado",
    daemon_offline: "Desconectado",
    quick_profile_title: "Perfil",
    language_label: "Idioma",
    close_to_tray: "Segundo plano",
    dry_run_mode: "Modo simulación",

    aio_liquid: "Refrigerante AIO",
    liquid_sub: "Circuito",
    liquid_label: "Circuito",
    cpu_sub: "Paquete",
    gpu_sub: "Núcleo",
    demote: "Desanclar del dashboard",
    pwm_headers: "Canales PWM",
    no_nzxt_channels: "Sin canales NZXT detectados",
    other_sensors: "Sensores del sistema",
    no_additional_sensors: "Sin sensores adicionales detectados",
    feature: "Anclar al dashboard",
    net_tag: "Red",
    drag_card_tooltip: "Arrastrar para mover panel",

    no_cooling_channels: "Sin canales de ventilación detectados",
    curve: "Curva",
    nodes: "Nodos",
    operating: "Operación",

    enabled: "Activo",
    disabled: "Inactivo",
    pywal_auto_sync: "Sincronizar automáticamente con pywal (daemon)",
    global_mode: "Modo global",
    filter_extremes: "Filtrar blancos y negros extremos",
    saturate_label: "Saturar colores",
    shift_palette: "Desplazar paleta",
    sync_now: "Sincronizar ahora",
    pywal_not_detected: "pywal no detectado (~/.cache/wal/colors.json)",
    no_nzxt_devices: "Sin dispositivos NZXT detectados",
    sync_whole_device: "sync (todo el dispositivo)",
    apply: "Aplicar",

    new_profile: "Nuevo perfil",
    save_config: "Guardar config",
    autogen_config: "Autogenerar TOML",
    import_toml: "Importar perfil",
    import_hint: "/ruta/al/perfil.toml",
    load: "Cargar",
    profile_editor: "Editor de perfil",
    capture_current_state: "Capturar estado actual",
    name_label: "Nombre",
    profile_name_hint: "mi_perfil",
    description_label: "Descripción",
    optional_hint: "Opcional",
    fixed: "Fijo",
    save_profile: "Guardar perfil",
    cancel: "Cancelar",
    active: "Activo",
    default_badge: "Predeterminado",
    delete: "Eliminar",
    clone: "Clonar",
    export: "Exportar",
    set_default: "Hacer predeterminado",
};

/// English UI string table.
pub const EN_STRINGS: GuiStrings = GuiStrings {
    nav_section: "Navigation",
    nav_dashboard: "Dashboard",
    nav_fans: "Fans",
    nav_rgb: "RGB Lighting",
    nav_profiles: "Profiles",

    daemon_label: "Daemon",
    daemon_online: "Connected",
    daemon_offline: "Offline",
    quick_profile_title: "Profile",
    language_label: "Language",
    close_to_tray: "Run in background",
    dry_run_mode: "Dry-run mode",

    aio_liquid: "AIO coolant",
    liquid_sub: "Loop",
    liquid_label: "Loop",
    cpu_sub: "Package",
    gpu_sub: "Core",
    demote: "Unpin from dashboard",
    pwm_headers: "PWM channels",
    no_nzxt_channels: "No NZXT channels detected",
    other_sensors: "System sensors",
    no_additional_sensors: "No additional sensors detected",
    feature: "Pin to dashboard",
    net_tag: "Net",
    drag_card_tooltip: "Drag to move panel",

    no_cooling_channels: "No cooling channels detected",
    curve: "Curve",
    nodes: "Nodes",
    operating: "Operating",

    enabled: "Enabled",
    disabled: "Disabled",
    pywal_auto_sync: "Auto-sync with pywal (daemon)",
    global_mode: "Global mode",
    filter_extremes: "Filter extreme whites and blacks",
    saturate_label: "Boost saturation",
    shift_palette: "Shift palette",
    sync_now: "Sync now",
    pywal_not_detected: "pywal not detected (~/.cache/wal/colors.json)",
    no_nzxt_devices: "No NZXT devices detected",
    sync_whole_device: "sync (whole device)",
    apply: "Apply",

    new_profile: "New profile",
    save_config: "Save config",
    autogen_config: "Auto-generate TOML",
    import_toml: "Import profile",
    import_hint: "/path/to/profile.toml",
    load: "Load",
    profile_editor: "Profile editor",
    capture_current_state: "Capture current state",
    name_label: "Name",
    profile_name_hint: "my_profile",
    description_label: "Description",
    optional_hint: "Optional",
    fixed: "Fixed",
    save_profile: "Save profile",
    cancel: "Cancel",
    active: "Active",
    default_badge: "Default",
    delete: "Delete",
    clone: "Clone",
    export: "Export",
    set_default: "Set default",
};

/// Returns the static UI string table for `lang`.
pub fn tr(lang: Language) -> &'static GuiStrings {
    match lang {
        Language::Es => &ES_STRINGS,
        Language::En => &EN_STRINGS,
    }
}

/// Formats the top-bar telemetry summary line.
pub fn fmt_top_summary(_lang: Language, cpu: &str, gpu: &str, devs: usize) -> String {
    format!("CPU {cpu}  ·  GPU {gpu}  ·  {devs} NZXT")
}

/// Formats the channel count label on fan and RGB device cards.
pub fn fmt_channels_badge(lang: Language, count: usize) -> String {
    match (lang, count) {
        (Language::Es, 1) => "1 canal".to_string(),
        (Language::Es, n) => format!("{n} canales"),
        (Language::En, 1) => "1 channel".to_string(),
        (Language::En, n) => format!("{n} channels"),
    }
}

/// Formats the current operating point readout below a fan curve plot.
pub fn fmt_operating_point(lang: Language, temp_c: f32, duty: u8) -> String {
    match lang {
        Language::Es => format!("Punto operativo {temp_c:.1}°C — {duty}%"),
        Language::En => format!("Operating point {temp_c:.1}°C — {duty}%"),
    }
}

/// Formats the pywal wallpaper filename label.
pub fn fmt_wallpaper(lang: Language, name: &str) -> String {
    match lang {
        Language::Es => format!("Fondo: {name}"),
        Language::En => format!("Wallpaper: {name}"),
    }
}

/// Formats the pywal sync completion status message.
pub fn fmt_pywal_synced(lang: Language, count: usize) -> String {
    match lang {
        Language::Es => format!("Pywal sincronizado ({count} objetivo(s))"),
        Language::En => format!("Pywal synced ({count} target(s))"),
    }
}

/// Formats the status message when applying a swatch color to all devices.
pub fn fmt_color_applied_all(lang: Language, hex: &str, mode: &str) -> String {
    match lang {
        Language::Es => format!("Color #{hex} ({mode}) aplicado a todos los dispositivos"),
        Language::En => format!("Color #{hex} ({mode}) applied to all devices"),
    }
}

/// Formats the status message when applying an RGB mode to a single channel.
pub fn fmt_channel_mode_applied(lang: Language, dev: &str, ch: &str, mode: &str) -> String {
    match lang {
        Language::Es => format!("{dev}/{ch}: {mode} aplicado"),
        Language::En => format!("{dev}/{ch}: {mode} applied"),
    }
}

/// Formats the status message after applying a profile locally.
pub fn fmt_profile_applied(
    lang: Language,
    name: &str,
    lights: usize,
    fans: usize,
    warnings: usize,
) -> String {
    match lang {
        Language::Es => {
            format!(
                "Perfil '{name}' aplicado: {lights} luces, {fans} ventiladores, {warnings} avisos"
            )
        }
        Language::En => {
            format!("Profile '{name}' applied: {lights} lights, {fans} fans, {warnings} warnings")
        }
    }
}

/// Formats the cooling rules section header in the profile editor.
pub fn fmt_cooling_rules_header(lang: Language, count: usize) -> String {
    match lang {
        Language::Es => format!("Reglas de refrigeración ({count})"),
        Language::En => format!("Cooling rules ({count})"),
    }
}

/// Formats the lighting rules section header in the profile editor.
pub fn fmt_lighting_rules_header(lang: Language, count: usize) -> String {
    match lang {
        Language::Es => format!("Reglas de iluminación ({count})"),
        Language::En => format!("Lighting rules ({count})"),
    }
}

/// Formats the rule count summary shown on each profile card.
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

/// Formats the default name suggestion when cloning a profile.
pub fn fmt_cloned_profile_name(lang: Language, orig: &str) -> String {
    match lang {
        Language::Es => format!("{orig}_copia"),
        Language::En => format!("{orig}_copy"),
    }
}

/// Formats the active profile readout returned by IPC status responses.
pub fn fmt_ipc_active_profile(lang: Language, profile: &str) -> String {
    match lang {
        Language::Es => format!("perfil activo: {profile}"),
        Language::En => format!("active profile: {profile}"),
    }
}

pub fn config_loaded(lang: Language) -> String {
    match lang {
        Language::Es => "Configuración cargada".to_string(),
        Language::En => "Configuration loaded".to_string(),
    }
}

pub fn config_unavailable(lang: Language, err: &dyn Display) -> String {
    match lang {
        Language::Es => format!("Config no disponible ({err}); usando valores iniciales"),
        Language::En => format!("Config unavailable ({err}); using starter defaults"),
    }
}

pub fn config_saved(lang: Language) -> String {
    match lang {
        Language::Es => "Configuración guardada".to_string(),
        Language::En => "Configuration saved".to_string(),
    }
}

pub fn config_save_error(lang: Language, err: &dyn Display) -> String {
    match lang {
        Language::Es => format!("Error guardando config: {err}"),
        Language::En => format!("Error saving config: {err}"),
    }
}

pub fn pywal_config_saved(lang: Language) -> String {
    match lang {
        Language::Es => "Configuración de pywal guardada".to_string(),
        Language::En => "Pywal configuration saved".to_string(),
    }
}

pub fn pywal_mode_changed(lang: Language, mode: &str) -> String {
    match lang {
        Language::Es => format!("Modo pywal cambiado a '{mode}'"),
        Language::En => format!("Pywal mode changed to '{mode}'"),
    }
}

pub fn pywal_palette_updated(lang: Language) -> String {
    match lang {
        Language::Es => "Paleta pywal actualizada".to_string(),
        Language::En => "Pywal palette updated".to_string(),
    }
}

pub fn profile_saved(lang: Language, name: &str) -> String {
    match lang {
        Language::Es => format!("Perfil '{name}' guardado"),
        Language::En => format!("Profile '{name}' saved"),
    }
}

pub fn profile_name_empty(lang: Language) -> String {
    match lang {
        Language::Es => "El nombre del perfil no puede estar vacío".to_string(),
        Language::En => "Profile name cannot be empty".to_string(),
    }
}

pub fn profile_import_path_empty(lang: Language) -> String {
    match lang {
        Language::Es => "Indica una ruta de perfil para importar".to_string(),
        Language::En => "Enter a profile path to import".to_string(),
    }
}

pub fn profile_imported(lang: Language, name: &str) -> String {
    match lang {
        Language::Es => format!("Perfil '{name}' importado y guardado"),
        Language::En => format!("Profile '{name}' imported and saved"),
    }
}

pub fn profile_import_error(lang: Language, err: &dyn Display) -> String {
    match lang {
        Language::Es => format!("Error importando perfil: {err}"),
        Language::En => format!("Error importing profile: {err}"),
    }
}

pub fn profile_exported(lang: Language, name: &str, path: &str) -> String {
    match lang {
        Language::Es => format!("Perfil '{name}' exportado a {path}"),
        Language::En => format!("Profile '{name}' exported to {path}"),
    }
}

pub fn profile_export_error(lang: Language, name: &str, err: &dyn Display) -> String {
    match lang {
        Language::Es => format!("Error exportando perfil '{name}': {err}"),
        Language::En => format!("Error exporting profile '{name}': {err}"),
    }
}

pub fn profile_deleted(lang: Language, name: &str) -> String {
    match lang {
        Language::Es => format!("Perfil '{name}' eliminado"),
        Language::En => format!("Profile '{name}' deleted"),
    }
}

pub fn default_profile_set(lang: Language, name: &str) -> String {
    match lang {
        Language::Es => format!("'{name}' establecido como perfil predeterminado"),
        Language::En => format!("'{name}' set as default profile"),
    }
}

pub fn language_changed(lang: Language) -> String {
    match lang {
        Language::Es => "Idioma cambiado a Español".to_string(),
        Language::En => "Language switched to English".to_string(),
    }
}

pub fn close_to_tray_changed(lang: Language, enabled: bool) -> String {
    match (lang, enabled) {
        (Language::Es, true) => "Cerrar al tray habilitado".to_string(),
        (Language::Es, false) => "Cerrar al tray deshabilitado".to_string(),
        (Language::En, true) => "Close to tray enabled".to_string(),
        (Language::En, false) => "Close to tray disabled".to_string(),
    }
}

pub fn config_autogenerated(lang: Language) -> String {
    match lang {
        Language::Es => "Configuración TOML autogenerada desde hardware".to_string(),
        Language::En => "TOML configuration auto-generated from hardware".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_tables_have_expected_translations() {
        let es = tr(Language::Es);
        let en = tr(Language::En);

        assert_eq!(es.nav_section, "Navegación");
        assert_eq!(en.nav_section, "Navigation");
        assert_eq!(es.nav_dashboard, "Dashboard");
        assert_eq!(en.nav_dashboard, "Dashboard");
        assert_eq!(es.nav_fans, "Ventiladores");
        assert_eq!(en.nav_fans, "Fans");
        assert_eq!(es.nav_rgb, "Iluminación RGB");
        assert_eq!(en.nav_rgb, "RGB Lighting");
        assert_eq!(es.nav_profiles, "Perfiles");
        assert_eq!(en.nav_profiles, "Profiles");

        assert_eq!(es.daemon_online, "Conectado");
        assert_eq!(en.daemon_online, "Connected");
        assert_eq!(es.quick_profile_title, "Perfil");
        assert_eq!(en.quick_profile_title, "Profile");
        assert_eq!(es.language_label, "Idioma");
        assert_eq!(en.language_label, "Language");
        assert_eq!(es.close_to_tray, "Segundo plano");
        assert_eq!(en.close_to_tray, "Run in background");
        assert_eq!(es.dry_run_mode, "Modo simulación");
        assert_eq!(en.dry_run_mode, "Dry-run mode");
        assert_eq!(es.net_tag, "Red");
        assert_eq!(en.net_tag, "Net");
        assert_eq!(es.demote, "Desanclar del dashboard");
        assert_eq!(en.demote, "Unpin from dashboard");
        assert_eq!(es.feature, "Anclar al dashboard");
        assert_eq!(en.feature, "Pin to dashboard");
        assert_eq!(es.active, "Activo");
        assert_eq!(en.active, "Active");
        assert_eq!(es.default_badge, "Predeterminado");
        assert_eq!(en.default_badge, "Default");
        assert_eq!(es.autogen_config, "Autogenerar TOML");
        assert_eq!(en.autogen_config, "Auto-generate TOML");
        assert_eq!(es.saturate_label, "Saturar colores");
        assert_eq!(en.saturate_label, "Boost saturation");
        assert_eq!(es.drag_card_tooltip, "Arrastrar para mover panel");
        assert_eq!(en.drag_card_tooltip, "Drag to move panel");
    }

    #[test]
    fn dynamic_formatters_render_both_languages() {
        assert_eq!(
            fmt_top_summary(Language::Es, "45.0°C", "52.0°C", 2),
            "CPU 45.0°C  ·  GPU 52.0°C  ·  2 NZXT"
        );
        assert_eq!(
            fmt_top_summary(Language::En, "45.0°C", "52.0°C", 2),
            "CPU 45.0°C  ·  GPU 52.0°C  ·  2 NZXT"
        );

        assert_eq!(fmt_channels_badge(Language::Es, 1), "1 canal");
        assert_eq!(fmt_channels_badge(Language::Es, 4), "4 canales");
        assert_eq!(fmt_channels_badge(Language::En, 1), "1 channel");
        assert_eq!(fmt_channels_badge(Language::En, 4), "4 channels");

        assert_eq!(
            fmt_operating_point(Language::Es, 34.5, 60),
            "Punto operativo 34.5°C — 60%"
        );
        assert_eq!(
            fmt_operating_point(Language::En, 34.5, 60),
            "Operating point 34.5°C — 60%"
        );

        assert_eq!(fmt_wallpaper(Language::Es, "bg.png"), "Fondo: bg.png");
        assert_eq!(fmt_wallpaper(Language::En, "bg.png"), "Wallpaper: bg.png");

        assert_eq!(
            fmt_pywal_synced(Language::Es, 2),
            "Pywal sincronizado (2 objetivo(s))"
        );
        assert_eq!(
            fmt_pywal_synced(Language::En, 2),
            "Pywal synced (2 target(s))"
        );

        assert_eq!(
            fmt_color_applied_all(Language::Es, "00f0ff", "fixed"),
            "Color #00f0ff (fixed) aplicado a todos los dispositivos"
        );
        assert_eq!(
            fmt_color_applied_all(Language::En, "00f0ff", "fixed"),
            "Color #00f0ff (fixed) applied to all devices"
        );

        assert_eq!(
            fmt_channel_mode_applied(Language::Es, "Kraken", "ring", "breathing"),
            "Kraken/ring: breathing aplicado"
        );
        assert_eq!(
            fmt_channel_mode_applied(Language::En, "Kraken", "ring", "breathing"),
            "Kraken/ring: breathing applied"
        );

        assert_eq!(
            fmt_profile_applied(Language::Es, "gaming", 3, 2, 1),
            "Perfil 'gaming' aplicado: 3 luces, 2 ventiladores, 1 avisos"
        );
        assert_eq!(
            fmt_profile_applied(Language::En, "gaming", 3, 2, 1),
            "Profile 'gaming' applied: 3 lights, 2 fans, 1 warnings"
        );

        assert_eq!(
            fmt_cooling_rules_header(Language::Es, 2),
            "Reglas de refrigeración (2)"
        );
        assert_eq!(
            fmt_cooling_rules_header(Language::En, 2),
            "Cooling rules (2)"
        );

        assert_eq!(
            fmt_lighting_rules_header(Language::Es, 3),
            "Reglas de iluminación (3)"
        );
        assert_eq!(
            fmt_lighting_rules_header(Language::En, 3),
            "Lighting rules (3)"
        );

        assert_eq!(
            fmt_profile_rules_summary(Language::Es, 2, 3),
            "2 reglas de iluminación · 3 reglas de refrigeración"
        );
        assert_eq!(
            fmt_profile_rules_summary(Language::En, 2, 3),
            "2 lighting rules · 3 cooling rules"
        );
    }

    #[test]
    fn status_messages_cover_es_and_en() {
        assert_eq!(config_loaded(Language::Es), "Configuración cargada");
        assert_eq!(config_loaded(Language::En), "Configuration loaded");
        assert!(config_unavailable(Language::Es, &"missing").contains("Config no disponible"));
        assert!(config_unavailable(Language::En, &"missing").contains("Config unavailable"));
        assert_eq!(config_saved(Language::Es), "Configuración guardada");
        assert_eq!(config_saved(Language::En), "Configuration saved");
        assert!(config_save_error(Language::Es, &"io").contains("Error guardando config"));
        assert!(config_save_error(Language::En, &"io").contains("Error saving config"));
        assert_eq!(
            pywal_config_saved(Language::Es),
            "Configuración de pywal guardada"
        );
        assert_eq!(
            pywal_config_saved(Language::En),
            "Pywal configuration saved"
        );
        assert_eq!(
            pywal_mode_changed(Language::Es, "fading"),
            "Modo pywal cambiado a 'fading'"
        );
        assert_eq!(
            pywal_mode_changed(Language::En, "fading"),
            "Pywal mode changed to 'fading'"
        );
        assert_eq!(
            pywal_palette_updated(Language::Es),
            "Paleta pywal actualizada"
        );
        assert_eq!(pywal_palette_updated(Language::En), "Pywal palette updated");
        assert_eq!(profile_saved(Language::Es, "p1"), "Perfil 'p1' guardado");
        assert_eq!(profile_saved(Language::En, "p1"), "Profile 'p1' saved");
        assert_ne!(
            profile_name_empty(Language::Es),
            profile_name_empty(Language::En)
        );
        assert_ne!(
            profile_import_path_empty(Language::Es),
            profile_import_path_empty(Language::En)
        );
        assert_eq!(
            profile_imported(Language::Es, "p1"),
            "Perfil 'p1' importado y guardado"
        );
        assert_eq!(
            profile_imported(Language::En, "p1"),
            "Profile 'p1' imported and saved"
        );
        assert!(profile_import_error(Language::Es, &"bad").contains("Error importando perfil"));
        assert!(profile_import_error(Language::En, &"bad").contains("Error importing profile"));
        assert_eq!(
            profile_exported(Language::Es, "p1", "/tmp/p1.toml"),
            "Perfil 'p1' exportado a /tmp/p1.toml"
        );
        assert_eq!(
            profile_exported(Language::En, "p1", "/tmp/p1.toml"),
            "Profile 'p1' exported to /tmp/p1.toml"
        );
        assert!(
            profile_export_error(Language::Es, "p1", &"io").contains("Error exportando perfil")
        );
        assert!(
            profile_export_error(Language::En, "p1", &"io").contains("Error exporting profile")
        );
        assert_eq!(profile_deleted(Language::Es, "p1"), "Perfil 'p1' eliminado");
        assert_eq!(profile_deleted(Language::En, "p1"), "Profile 'p1' deleted");
        assert_eq!(
            default_profile_set(Language::Es, "p1"),
            "'p1' establecido como perfil predeterminado"
        );
        assert_eq!(
            default_profile_set(Language::En, "p1"),
            "'p1' set as default profile"
        );
        assert_eq!(language_changed(Language::Es), "Idioma cambiado a Español");
        assert_eq!(
            language_changed(Language::En),
            "Language switched to English"
        );
        assert_eq!(
            close_to_tray_changed(Language::Es, true),
            "Cerrar al tray habilitado"
        );
        assert_eq!(
            close_to_tray_changed(Language::Es, false),
            "Cerrar al tray deshabilitado"
        );
        assert_eq!(
            close_to_tray_changed(Language::En, true),
            "Close to tray enabled"
        );
        assert_eq!(
            close_to_tray_changed(Language::En, false),
            "Close to tray disabled"
        );
        assert_eq!(
            config_autogenerated(Language::Es),
            "Configuración TOML autogenerada desde hardware"
        );
        assert_eq!(
            config_autogenerated(Language::En),
            "TOML configuration auto-generated from hardware"
        );
    }
}
