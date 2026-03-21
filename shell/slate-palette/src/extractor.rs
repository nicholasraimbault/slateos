/// Color extraction from wallpaper images.
///
/// Tries matugen first (high-quality Material You extraction). If matugen is
/// not installed or fails, falls back to a built-in pixel-sampling extractor
/// that finds the dominant hue and generates a palette from it.
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::Value;
use slate_common::Palette;

use crate::builtin_extract;

/// Parse a hex color string like `#aabbcc` into `[R, G, B, 255]`.
fn hex_to_rgba(hex: &str) -> Result<[u8; 4]> {
    let hex = hex.trim_start_matches('#');
    anyhow::ensure!(hex.len() == 6, "expected 6-char hex string, got {hex:?}");
    let r = u8::from_str_radix(&hex[0..2], 16).context("parse red")?;
    let g = u8::from_str_radix(&hex[2..4], 16).context("parse green")?;
    let b = u8::from_str_radix(&hex[4..6], 16).context("parse blue")?;
    Ok([r, g, b, 255])
}

/// Extract a `Palette` from the matugen JSON output.
///
/// Matugen's `--json` output contains colour schemes keyed by mode (light/dark).
/// We pull from `colors.dark` and map:
///   - `primary`           -> Palette::primary
///   - `secondary`         -> Palette::secondary
///   - `surface`           -> Palette::surface
///   - `surface_container` -> Palette::container
///   - `on_surface`        -> Palette::neutral
pub fn parse_matugen_json(json_str: &str) -> Result<Palette> {
    let root: Value = serde_json::from_str(json_str).context("parse matugen JSON")?;

    let dark = root
        .get("colors")
        .and_then(|c| c.get("dark"))
        .context("missing colors.dark in matugen output")?;

    let get_color = |key: &str| -> Result<[u8; 4]> {
        let hex = dark
            .get(key)
            .and_then(|v| v.get("hex"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing colors.dark.{key}.hex"))?;
        hex_to_rgba(hex).with_context(|| format!("parse {key}"))
    };

    Ok(Palette {
        primary: get_color("primary")?,
        secondary: get_color("secondary")?,
        surface: get_color("surface")?,
        container: get_color("surface_container")?,
        neutral: get_color("on_surface")?,
    })
}

/// Try to extract a palette using matugen. Returns `Err` if matugen is not
/// installed or fails.
async fn extract_via_matugen(image_path: &Path) -> Result<Palette> {
    let path_str = image_path
        .to_str()
        .context("wallpaper path is not valid UTF-8")?;

    let output = tokio::process::Command::new("matugen")
        .args(["image", path_str, "--mode", "dark", "--json"])
        .output()
        .await
        .context("failed to run matugen — is it installed?")?;

    anyhow::ensure!(
        output.status.success(),
        "matugen exited with status {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr),
    );

    let json_str = String::from_utf8(output.stdout).context("matugen output is not UTF-8")?;
    parse_matugen_json(&json_str)
}

/// Extract a palette from a wallpaper image.
///
/// Strategy:
/// 1. Try matugen (highest-quality Material You extraction)
/// 2. If matugen fails, use built-in pixel-sampling extractor
///
/// Only returns `Err` if both methods fail.
pub async fn extract_palette(image_path: &Path) -> Result<Palette> {
    match extract_via_matugen(image_path).await {
        Ok(palette) => {
            tracing::info!("extracted palette via matugen");
            Ok(palette)
        }
        Err(matugen_err) => {
            tracing::info!("matugen unavailable ({matugen_err:#}), trying built-in extractor");
            let path = image_path.to_path_buf();
            tokio::task::spawn_blocking(move || builtin_extract::extract_from_image(&path))
                .await
                .context("built-in extractor task panicked")?
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Realistic (trimmed) matugen JSON output for testing.
    const SAMPLE_MATUGEN_JSON: &str = r##"{
        "colors": {
            "dark": {
                "primary": { "hex": "#aec6ff", "rgb": "rgb(174,198,255)" },
                "secondary": { "hex": "#bdc6dc", "rgb": "rgb(189,198,220)" },
                "surface": { "hex": "#111318", "rgb": "rgb(17,19,24)" },
                "surface_container": { "hex": "#1e2026", "rgb": "rgb(30,32,38)" },
                "on_surface": { "hex": "#e2e2e9", "rgb": "rgb(226,226,233)" }
            },
            "light": {
                "primary": { "hex": "#3b5ba9", "rgb": "rgb(59,91,169)" },
                "secondary": { "hex": "#575e71", "rgb": "rgb(87,94,113)" },
                "surface": { "hex": "#f9f9ff", "rgb": "rgb(249,249,255)" },
                "surface_container": { "hex": "#ededf4", "rgb": "rgb(237,237,244)" },
                "on_surface": { "hex": "#1a1b21", "rgb": "rgb(26,27,33)" }
            }
        }
    }"##;

    #[test]
    fn parse_matugen_json_extracts_dark_palette() {
        let palette = parse_matugen_json(SAMPLE_MATUGEN_JSON).unwrap();

        assert_eq!(palette.primary, [0xae, 0xc6, 0xff, 255]);
        assert_eq!(palette.secondary, [0xbd, 0xc6, 0xdc, 255]);
        assert_eq!(palette.surface, [0x11, 0x13, 0x18, 255]);
        assert_eq!(palette.container, [0x1e, 0x20, 0x26, 255]);
        assert_eq!(palette.neutral, [0xe2, 0xe2, 0xe9, 255]);
    }

    #[test]
    fn hex_to_rgba_valid() {
        assert_eq!(hex_to_rgba("#ff0080").unwrap(), [255, 0, 128, 255]);
        assert_eq!(hex_to_rgba("00ff00").unwrap(), [0, 255, 0, 255]);
    }

    #[test]
    fn hex_to_rgba_invalid_length() {
        assert!(hex_to_rgba("#fff").is_err());
        assert!(hex_to_rgba("").is_err());
    }

    #[test]
    fn parse_matugen_json_missing_field() {
        let json = r#"{ "colors": { "dark": {} } }"#;
        assert!(parse_matugen_json(json).is_err());
    }

    #[test]
    fn parse_matugen_json_invalid_json() {
        assert!(parse_matugen_json("not json at all").is_err());
    }

    #[test]
    fn parse_matugen_json_missing_dark() {
        let json = r#"{ "colors": { "light": {} } }"#;
        assert!(parse_matugen_json(json).is_err());
    }
}
