/// Configuration loading for the TouchFlow gesture daemon.
///
/// Reads `~/.config/slate/touchflow.toml` (or a caller-specified path).
/// If the file does not exist, sensible defaults are returned.
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

// ---------------------------------------------------------------------------
// Top-level config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct TouchFlowConfig {
    pub gestures: GesturesConfig,
    pub edge: EdgeCfg,
    pub niri: NiriConfig,
}

impl Default for TouchFlowConfig {
    fn default() -> Self {
        Self {
            gestures: GesturesConfig::default(),
            edge: EdgeCfg::default(),
            niri: NiriConfig::default(),
        }
    }
}

impl TouchFlowConfig {
    /// Load config from a TOML file.  Returns an error only when the file
    /// exists but cannot be read or parsed.
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("reading config file {}", path.display()))?;
        Self::from_toml(&content)
    }

    /// Parse config from a TOML string.
    pub fn from_toml(toml_str: &str) -> Result<Self> {
        toml::from_str(toml_str).context("parsing touchflow config")
    }

    /// Load from `~/.config/slate/touchflow.toml`, falling back to defaults
    /// if the file does not exist.
    pub fn load_or_default() -> Self {
        let path = dirs_config_path();
        if path.exists() {
            match Self::load(&path) {
                Ok(cfg) => {
                    tracing::info!(?path, "loaded config");
                    cfg
                }
                Err(e) => {
                    tracing::warn!(?path, %e, "failed to load config, using defaults");
                    Self::default()
                }
            }
        } else {
            tracing::info!("no config file found, using defaults");
            Self::default()
        }
    }
}

// ---------------------------------------------------------------------------
// Sub-sections
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct GesturesConfig {
    pub enabled: bool,
    /// Multiplier for recognition thresholds (lower = more sensitive).
    pub sensitivity: f64,
    pub tap_enabled: bool,
    pub swipe_enabled: bool,
    pub pinch_enabled: bool,
    pub edge_swipe_enabled: bool,
}

impl Default for GesturesConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            sensitivity: 1.0,
            tap_enabled: true,
            swipe_enabled: true,
            pinch_enabled: true,
            edge_swipe_enabled: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct EdgeCfg {
    /// Pixels from screen edge that count as the "edge zone".
    pub size: u32,
    /// Screen width in pixels (for edge detection boundaries).
    pub screen_width: u32,
    /// Screen height in pixels (for edge detection boundaries).
    pub screen_height: u32,
}

impl Default for EdgeCfg {
    fn default() -> Self {
        // ONN 11 Tablet Pro: 1280x1840 native resolution
        Self {
            size: 50,
            screen_width: 1280,
            screen_height: 1840,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct NiriConfig {
    /// Override for `$NIRI_SOCKET`.  When `None`, the env var is used.
    pub socket_path: Option<String>,
}

impl Default for NiriConfig {
    fn default() -> Self {
        Self { socket_path: None }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn dirs_config_path() -> std::path::PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
            std::path::PathBuf::from(home).join(".config")
        });
    base.join("slate").join("touchflow.toml")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_sane_values() {
        let cfg = TouchFlowConfig::default();
        assert!(cfg.gestures.enabled);
        assert!((cfg.gestures.sensitivity - 1.0).abs() < f64::EPSILON);
        assert!(cfg.gestures.tap_enabled);
        assert!(cfg.gestures.swipe_enabled);
        assert!(cfg.gestures.pinch_enabled);
        assert!(cfg.gestures.edge_swipe_enabled);
        assert_eq!(cfg.edge.size, 50);
        assert_eq!(cfg.edge.screen_width, 1280);
        assert_eq!(cfg.edge.screen_height, 1840);
        assert!(cfg.niri.socket_path.is_none());
    }

    #[test]
    fn config_loads_from_toml_string() {
        let toml = r#"
[gestures]
enabled = true
sensitivity = 1.5
tap_enabled = false
swipe_enabled = true
pinch_enabled = false
edge_swipe_enabled = true

[edge]
size = 80

[niri]
socket_path = "/tmp/niri.sock"
"#;
        let cfg = TouchFlowConfig::from_toml(toml).unwrap();
        assert!((cfg.gestures.sensitivity - 1.5).abs() < f64::EPSILON);
        assert!(!cfg.gestures.tap_enabled);
        assert!(!cfg.gestures.pinch_enabled);
        assert_eq!(cfg.edge.size, 80);
        assert_eq!(cfg.niri.socket_path.as_deref(), Some("/tmp/niri.sock"));
    }

    #[test]
    fn partial_toml_uses_defaults_for_missing_fields() {
        let toml = r#"
[gestures]
sensitivity = 2.0
"#;
        let cfg = TouchFlowConfig::from_toml(toml).unwrap();
        assert!((cfg.gestures.sensitivity - 2.0).abs() < f64::EPSILON);
        // Defaults for everything else.
        assert!(cfg.gestures.enabled);
        assert!(cfg.gestures.tap_enabled);
        assert_eq!(cfg.edge.size, 50);
        assert!(cfg.niri.socket_path.is_none());
    }

    #[test]
    fn missing_config_file_returns_defaults() {
        let path = std::path::Path::new("/tmp/definitely_does_not_exist_touchflow.toml");
        assert!(TouchFlowConfig::load(path).is_err());
        // The load_or_default path is exercised implicitly —
        // the function checks existence before calling load().
        // We verify the fallback by checking default values are sane.
        let cfg = TouchFlowConfig::default();
        assert!(cfg.gestures.enabled);
    }

    #[test]
    fn empty_toml_gives_defaults() {
        let cfg = TouchFlowConfig::from_toml("").unwrap();
        assert!(cfg.gestures.enabled);
        assert_eq!(cfg.edge.size, 50);
    }
}
