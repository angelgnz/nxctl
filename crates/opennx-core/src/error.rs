use thiserror::Error;

/// Errors returned by `opennx-core` operations.
#[derive(Debug, Error)]
pub enum CoreError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("failed to deserialize TOML configuration: {0}")]
    TomlDe(#[from] toml::de::Error),

    #[error("failed to serialize TOML configuration: {0}")]
    TomlSer(#[from] toml::ser::Error),

    #[error("failed to parse JSON output from liquidctl: {0}")]
    Json(#[from] serde_json::Error),

    #[error("liquidctl executable was not found in PATH")]
    LiquidctlNotFound,

    #[error("liquidctl command {args:?} failed (exit code: {code:?}): {stderr}")]
    LiquidctlFailed {
        args: Vec<String>,
        code: Option<i32>,
        stderr: String,
    },

    #[error("profile '{0}' not found in configuration")]
    ProfileNotFound(String),

    #[error("validation error: {0}")]
    Validation(String),
}

/// Convenience result alias for `opennx-core`.
pub type Result<T> = std::result::Result<T, CoreError>;
