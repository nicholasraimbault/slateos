/// `slate config` — read, write, and display the slate CLI configuration.
///
/// Persists per-user settings to `$XDG_CONFIG_HOME/slate/cli.toml` (or
/// `~/.config/slate/cli.toml`).  Keys control defaults for other
/// subcommands so they don't need to be repeated on every invocation.
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Args;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::device::Device;

// ---------------------------------------------------------------------------
// Args
// ---------------------------------------------------------------------------

/// Arguments for `slate config`.
#[derive(Debug, Args)]
pub struct ConfigArgs {
    /// Print only the config file path, then exit.
    #[arg(long)]
    pub path: bool,

    /// Set the default target device (e.g. `generic-x86`, `pixel-tablet`).
    #[arg(long)]
    pub set_device: Option<Device>,

    /// Set the path to the aarch64-musl cross toolchain.
    #[arg(long)]
    pub set_cross_toolchain: Option<String>,

    /// Reset configuration to built-in defaults.
    #[arg(long)]
    pub reset: bool,
}

// ---------------------------------------------------------------------------
// Config struct
// ---------------------------------------------------------------------------

/// Persisted CLI configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CliConfig {
    /// Default `--device` flag when none is specified.
    #[serde(default = "default_device_str")]
    pub default_device: String,

    /// Path to aarch64-unknown-linux-musl cross toolchain (empty = not set).
    #[serde(default)]
    pub cross_toolchain: String,

    /// Where build-rootfs.sh caches downloads (empty = default temp dir).
    #[serde(default)]
    pub rootfs_cache: String,
}

fn default_device_str() -> String {
    "generic-x86".to_string()
}

impl Default for CliConfig {
    fn default() -> Self {
        Self {
            default_device: default_device_str(),
            cross_toolchain: String::new(),
            rootfs_cache: String::new(),
        }
    }
}

impl CliConfig {
    /// Load config from disk, returning defaults if the file doesn't exist.
    pub fn load() -> Result<Self> {
        let path = config_path();
        if !path.exists() {
            debug!(path = %path.display(), "no config file found, using defaults");
            return Ok(Self::default());
        }

        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let config: Self = toml::from_str(&contents)
            .with_context(|| format!("failed to parse {}", path.display()))?;

        debug!(path = %path.display(), "loaded config");
        Ok(config)
    }

    /// Save config to disk, creating parent directories if needed.
    pub fn save(&self) -> Result<()> {
        let path = config_path();

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        let contents = toml::to_string_pretty(self).context("failed to serialize config")?;
        std::fs::write(&path, contents)
            .with_context(|| format!("failed to write {}", path.display()))?;

        info!(path = %path.display(), "saved config");
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Run
// ---------------------------------------------------------------------------

/// Execute `slate config`.
pub fn run(args: ConfigArgs) -> Result<()> {
    let path = config_path();

    if args.path {
        println!("{}", path.display());
        return Ok(());
    }

    if args.reset {
        let config = CliConfig::default();
        config.save()?;
        println!("  Config reset to defaults.");
        println!("  Saved to: {}", path.display());
        return Ok(());
    }

    let mut config = CliConfig::load()?;
    let mut changed = false;

    if let Some(device) = args.set_device {
        config.default_device = device.to_string();
        changed = true;
    }

    if let Some(toolchain) = args.set_cross_toolchain {
        config.cross_toolchain = toolchain;
        changed = true;
    }

    if changed {
        config.save()?;
        println!("  Config updated.");
    }

    print_config(&config, &path);
    Ok(())
}

// ---------------------------------------------------------------------------
// Display
// ---------------------------------------------------------------------------

fn print_config(config: &CliConfig, path: &std::path::Path) {
    println!();
    println!("  slate config");
    println!("  ------------");
    println!("  file             : {}", path.display());
    println!(
        "  status           : {}",
        if path.exists() {
            "loaded"
        } else {
            "defaults (no file)"
        }
    );
    println!();
    println!("  default_device   : {}", config.default_device);
    println!(
        "  cross_toolchain  : {}",
        if config.cross_toolchain.is_empty() {
            "(not set)"
        } else {
            &config.cross_toolchain
        }
    );
    println!(
        "  rootfs_cache     : {}",
        if config.rootfs_cache.is_empty() {
            "(not set)"
        } else {
            &config.rootfs_cache
        }
    );
    println!();
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Canonical path for the slate CLI config file.
///
/// Respects `$XDG_CONFIG_HOME`; falls back to `~/.config/slate/cli.toml`.
pub fn config_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            home_dir()
                .unwrap_or_else(|| PathBuf::from("/root"))
                .join(".config")
        });
    base.join("slate").join("cli.toml")
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_generic_x86() {
        let config = CliConfig::default();
        assert_eq!(config.default_device, "generic-x86");
    }

    #[test]
    fn config_roundtrip_toml() {
        let config = CliConfig {
            default_device: "pixel-tablet".to_string(),
            cross_toolchain: "/opt/musl".to_string(),
            rootfs_cache: "/tmp/cache".to_string(),
        };
        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: CliConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(config, deserialized);
    }

    #[test]
    fn config_deserialize_empty_uses_defaults() {
        let config: CliConfig = toml::from_str("").unwrap();
        assert_eq!(config.default_device, "generic-x86");
        assert!(config.cross_toolchain.is_empty());
    }

    #[test]
    fn config_deserialize_partial_fills_defaults() {
        let config: CliConfig = toml::from_str("default_device = \"pixel-tablet\"").unwrap();
        assert_eq!(config.default_device, "pixel-tablet");
        assert!(config.cross_toolchain.is_empty());
    }

    #[test]
    fn config_path_ends_with_cli_toml() {
        let p = config_path();
        assert!(
            p.to_string_lossy().ends_with("slate/cli.toml"),
            "unexpected path: {p:?}"
        );
    }

    #[test]
    fn config_path_honours_xdg_config_home() {
        let _guard = EnvGuard::set("XDG_CONFIG_HOME", "/tmp/xdg-test");
        let p = config_path();
        assert_eq!(p, PathBuf::from("/tmp/xdg-test/slate/cli.toml"));
    }

    #[test]
    fn config_display_runs_without_error() {
        let args = ConfigArgs {
            path: false,
            set_device: None,
            set_cross_toolchain: None,
            reset: false,
        };
        assert!(run(args).is_ok());
    }

    #[test]
    fn config_path_flag_runs_without_error() {
        let args = ConfigArgs {
            path: true,
            set_device: None,
            set_cross_toolchain: None,
            reset: false,
        };
        assert!(run(args).is_ok());
    }

    struct EnvGuard {
        key: &'static str,
        original: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let original = std::env::var_os(key);
            unsafe { std::env::set_var(key, value) };
            EnvGuard { key, original }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.original {
                Some(v) => unsafe { std::env::set_var(self.key, v) },
                None => unsafe { std::env::remove_var(self.key) },
            }
        }
    }
}
