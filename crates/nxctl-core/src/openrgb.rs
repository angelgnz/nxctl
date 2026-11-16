//! OpenRGB CLI lighting backend (`openrgb`).

use std::process::Command;

use crate::error::{CoreError, Result};

/// Parses an OpenRGB `match_device` target (`"openrgb"` or `"openrgb:<device>"`).
pub fn parse_openrgb_target(match_device: &str) -> Option<Option<&str>> {
    let trimmed = match_device.trim();
    if trimmed.eq_ignore_ascii_case("openrgb") {
        return Some(None);
    }
    let prefix = trimmed.get(..8)?;
    if !prefix.eq_ignore_ascii_case("openrgb:") {
        return None;
    }
    let rest = trimmed[8..].trim();
    (!rest.is_empty()).then_some(Some(rest))
}

/// Builds `openrgb` CLI arguments for setting color/mode.
pub fn build_openrgb_color_args(
    device: Option<&str>,
    channel: &str,
    mode: &str,
    colors: &[String],
) -> Result<Vec<String>> {
    let mode_trimmed = mode.trim();
    if mode_trimmed.is_empty() {
        return Err(CoreError::Validation(
            "openrgb mode must not be empty".to_string(),
        ));
    }

    let mut args = vec!["--noautoconnect".to_string()];
    if let Some(dev) = device {
        args.push("--device".to_string());
        args.push(dev.to_string());
    }
    let ch = channel.trim();
    if !ch.is_empty()
        && !matches!(
            ch.to_ascii_lowercase().as_str(),
            "sync" | "all" | "default"
        )
    {
        args.push("--zone".to_string());
        args.push(ch.to_string());
    }
    args.push("--mode".to_string());
    args.push(mode_trimmed.to_string());
    if !colors.is_empty() {
        args.push("--color".to_string());
        args.push(colors.join(","));
    }
    Ok(args)
}

/// Applies RGB lighting via the `openrgb` CLI binary.
pub fn set_openrgb_color(
    device: Option<&str>,
    channel: &str,
    mode: &str,
    colors: &[String],
    dry_run: bool,
) -> Result<()> {
    let args = build_openrgb_color_args(device, channel, mode, colors)?;
    if dry_run {
        tracing::info!(?args, "dry-run: openrgb set_color");
        return Ok(());
    }
    let output = Command::new("openrgb").args(&args).output()?;
    if !output.status.success() {
        return Err(CoreError::Io(std::io::Error::other(format!(
            "openrgb failed (exit code: {:?}): {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        ))));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_openrgb_targets() {
        assert_eq!(parse_openrgb_target("openrgb"), Some(None));
        assert_eq!(parse_openrgb_target("OpenRGB"), Some(None));
        assert_eq!(parse_openrgb_target("openrgb:0"), Some(Some("0")));
        assert_eq!(
            parse_openrgb_target("openrgb:ASUS Aura"),
            Some(Some("ASUS Aura"))
        );
        assert_eq!(parse_openrgb_target("openrgb:"), None);
        assert_eq!(parse_openrgb_target("kraken"), None);
    }

    #[test]
    fn builds_openrgb_args_and_supports_dry_run() {
        assert_eq!(
            build_openrgb_color_args(None, "sync", "static", &["7400b8".into()]).unwrap(),
            vec![
                "--noautoconnect",
                "--mode",
                "static",
                "--color",
                "7400b8"
            ]
        );
        assert_eq!(
            build_openrgb_color_args(
                Some("0"),
                "DRAM",
                "breathing",
                &["ff0000".into(), "00ff00".into()]
            )
            .unwrap(),
            vec![
                "--noautoconnect",
                "--device",
                "0",
                "--zone",
                "DRAM",
                "--mode",
                "breathing",
                "--color",
                "ff0000,00ff00"
            ]
        );
        assert!(matches!(
            build_openrgb_color_args(None, "sync", " ", &[]),
            Err(CoreError::Validation(_))
        ));
        set_openrgb_color(Some("0"), "sync", "static", &["00ffff".into()], true)
            .expect("dry-run should succeed");
    }
}
