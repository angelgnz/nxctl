use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::CoreError;

/// Supported UI and CLI localization languages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    /// Spanish (default).
    #[default]
    Es,
    /// English.
    En,
}

impl Language {
    /// All supported languages in display order.
    pub const ALL: [Language; 2] = [Language::Es, Language::En];

    /// Returns the canonical lowercase ISO 639-1 code (`"es"` or `"en"`).
    pub fn code(self) -> &'static str {
        match self {
            Self::Es => "es",
            Self::En => "en",
        }
    }

    /// Returns the short uppercase label (`"ES"` or `"EN"`).
    pub fn short_label(self) -> &'static str {
        match self {
            Self::Es => "ES",
            Self::En => "EN",
        }
    }

    /// Returns the native display name (`"Español"` or `"English"`).
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Es => "Español",
            Self::En => "English",
        }
    }

    /// Toggles between Spanish and English.
    pub fn toggle(self) -> Self {
        match self {
            Self::Es => Self::En,
            Self::En => Self::Es,
        }
    }

    /// Parses a language code or locale string (e.g., `"es_MX.UTF-8"`, `"en-US"`, `"English"`).
    pub fn from_code(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        let without_encoding = trimmed.split_once('.').map_or(trimmed, |(head, _)| head);
        let base = without_encoding
            .split_once(['_', '-'])
            .map_or(without_encoding, |(head, _)| head);
        match base.to_lowercase().as_str() {
            "es" | "esp" | "español" | "espanol" | "spanish" => Some(Self::Es),
            "en" | "eng" | "english" => Some(Self::En),
            _ => None,
        }
    }

    /// Detects the preferred language from `NXCTL_LANG`, `LC_ALL`, `LC_MESSAGES`, and `LANG`.
    pub fn detect_from_env() -> Self {
        Self::detect_from_lookup(|key| std::env::var(key).ok())
    }

    fn detect_from_lookup<F>(mut lookup: F) -> Self
    where
        F: FnMut(&str) -> Option<String>,
    {
        for key in ["NXCTL_LANG", "LC_ALL", "LC_MESSAGES", "LANG"] {
            if let Some(val) = lookup(key)
                && let Some(lang) = Self::from_code(&val)
            {
                return lang;
            }
        }
        Self::Es
    }
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl FromStr for Language {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_code(s).ok_or_else(|| {
            CoreError::Validation(format!("unsupported language '{s}'; expected 'es' or 'en'"))
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn metadata_and_toggle() {
        assert_eq!(Language::default(), Language::Es);
        assert_eq!(Language::ALL, [Language::Es, Language::En]);

        assert_eq!(Language::Es.code(), "es");
        assert_eq!(Language::En.code(), "en");

        assert_eq!(Language::Es.short_label(), "ES");
        assert_eq!(Language::En.short_label(), "EN");

        assert_eq!(Language::Es.display_name(), "Español");
        assert_eq!(Language::En.display_name(), "English");

        assert_eq!(Language::Es.toggle(), Language::En);
        assert_eq!(Language::En.toggle(), Language::Es);

        assert_eq!(Language::Es.to_string(), "es");
        assert_eq!(Language::En.to_string(), "en");
    }

    #[test]
    fn from_code_parses_variants_and_rejects_unknown() {
        for raw in [
            "es",
            "ES",
            " esp ",
            "español",
            "ESPAÑOL",
            "espanol",
            "spanish",
            "es_MX",
            "es-419",
            "es_ES.UTF-8",
        ] {
            assert_eq!(
                Language::from_code(raw),
                Some(Language::Es),
                "failed: {raw}"
            );
        }

        for raw in [
            "en",
            "EN",
            " eng ",
            "English",
            "en-US",
            "en_GB.UTF-8",
            "en_US.utf8",
        ] {
            assert_eq!(
                Language::from_code(raw),
                Some(Language::En),
                "failed: {raw}"
            );
        }

        for raw in ["", "   ", "fr", "de_DE.UTF-8", "C.UTF-8", "POSIX"] {
            assert_eq!(Language::from_code(raw), None, "expected None: {raw}");
        }
    }

    #[test]
    fn from_str_returns_validation_error_on_invalid() {
        assert_eq!("en_US.UTF-8".parse::<Language>().ok(), Some(Language::En));
        assert_eq!("es".parse::<Language>().ok(), Some(Language::Es));

        let err = "fr".parse::<Language>().expect_err("invalid language");
        assert!(matches!(err, CoreError::Validation(_)));
    }

    #[test]
    fn detect_from_lookup_respects_precedence_and_fallback() {
        let vars = HashMap::from([
            ("NXCTL_LANG", "en_US.UTF-8"),
            ("LC_ALL", "es_MX.UTF-8"),
            ("LC_MESSAGES", "es_ES"),
            ("LANG", "es"),
        ]);
        assert_eq!(
            Language::detect_from_lookup(|k| vars.get(k).map(|v| (*v).to_string())),
            Language::En
        );

        let vars = HashMap::from([
            ("NXCTL_LANG", "invalid"),
            ("LC_ALL", "en-GB"),
            ("LC_MESSAGES", "es"),
            ("LANG", "es"),
        ]);
        assert_eq!(
            Language::detect_from_lookup(|k| vars.get(k).map(|v| (*v).to_string())),
            Language::En
        );

        let vars = HashMap::from([("LC_MESSAGES", "en.UTF-8"), ("LANG", "es")]);
        assert_eq!(
            Language::detect_from_lookup(|k| vars.get(k).map(|v| (*v).to_string())),
            Language::En
        );

        let vars = HashMap::from([("LANG", "C.UTF-8")]);
        assert_eq!(
            Language::detect_from_lookup(|k| vars.get(k).map(|v| (*v).to_string())),
            Language::Es
        );

        assert_eq!(Language::detect_from_lookup(|_| None), Language::Es);
    }
}
