/// Settings I/O: load, save (debounced), and broadcast changes.
///
/// Reads/writes `~/.config/slate/settings.toml` via `slate_common::Settings`.
/// After writing, emits a D-Bus signal so every component reloads live.
use std::path::{Path, PathBuf};

use slate_common::Settings;

use crate::notifier;

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

/// Canonical path to the settings file.
pub fn settings_path() -> PathBuf {
    dirs_home().join(".config/slate/settings.toml")
}

/// Resolve the user home directory.
fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/root"))
}

// ---------------------------------------------------------------------------
// Load
// ---------------------------------------------------------------------------

/// Load settings from disk, falling back to defaults if the file is missing
/// or unparseable.
pub fn load_settings() -> Settings {
    load_settings_from(&settings_path())
}

/// Load from a specific path (useful for tests).
pub fn load_settings_from(path: &Path) -> Settings {
    match Settings::load(path) {
        Ok(s) => {
            tracing::info!("loaded settings from {}", path.display());
            s
        }
        Err(e) => {
            tracing::warn!(
                "could not load settings from {}: {e}; using defaults",
                path.display()
            );
            Settings::default()
        }
    }
}

// ---------------------------------------------------------------------------
// Save
// ---------------------------------------------------------------------------

/// Save settings to disk and emit a D-Bus signal for the given section.
///
/// This is an async function so the D-Bus emission does not block the UI.
pub async fn save_and_notify(settings: Settings, section: String) {
    save_and_notify_to(settings, section, settings_path()).await;
}

/// Save to a specific path (useful for tests).
pub async fn save_and_notify_to(settings: Settings, section: String, path: PathBuf) {
    // Write on a blocking thread so we don't stall the async runtime.
    let write_path = path.clone();
    let write_result = tokio::task::spawn_blocking(move || settings.save(&write_path)).await;

    match write_result {
        Ok(Ok(())) => {
            tracing::info!("saved settings to {}", path.display());
        }
        Ok(Err(e)) => {
            tracing::error!("failed to save settings: {e}");
            return;
        }
        Err(e) => {
            tracing::error!("save task panicked: {e}");
            return;
        }
    }

    // Emit D-Bus signal (best-effort)
    notifier::emit_changed(&section).await;
}

// ---------------------------------------------------------------------------
// Service control (arkhe)
// ---------------------------------------------------------------------------

/// Control an arkhe-managed service.
///
/// Actions:
///   - "start":   write to /run/arkhe/<service>/ctl/start, SIGHUP supervisor
///   - "stop":    write to /run/arkhe/<service>/ctl/stop, SIGHUP supervisor
///   - "restart": stop then start
///   - "enable":  remove /etc/sv/<service>/disabled
///   - "disable": create /etc/sv/<service>/disabled
///
/// Returns a status message. Failures are not fatal.
pub async fn arkhe_service_ctl(action: &str, service: &str) -> Result<String, String> {
    match action {
        "restart" => {
            arkhe_ctl_write(service, "stop").await?;
            signal_supervisor().await?;
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            arkhe_ctl_write(service, "start").await?;
            signal_supervisor().await?;
            let msg = format!("{service}: restart requested");
            tracing::info!("{msg}");
            Ok(msg)
        }
        "enable" => {
            let disabled = format!("/etc/sv/{service}/disabled");
            tokio::fs::remove_file(&disabled)
                .await
                .or_else(|e| {
                    if e.kind() == std::io::ErrorKind::NotFound {
                        Ok(())
                    } else {
                        Err(e)
                    }
                })
                .map_err(|e| format!("failed to enable {service}: {e}"))?;
            let msg = format!("{service} enabled");
            tracing::info!("{msg}");
            Ok(msg)
        }
        "disable" => {
            let disabled_path = format!("/etc/sv/{service}/disabled");
            if let Some(parent) = Path::new(&disabled_path).parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|e| format!("failed to create service dir: {e}"))?;
            }
            tokio::fs::write(&disabled_path, "")
                .await
                .map_err(|e| format!("failed to disable {service}: {e}"))?;
            let msg = format!("{service} disabled");
            tracing::info!("{msg}");
            Ok(msg)
        }
        "start" | "stop" => {
            arkhe_ctl_write(service, action).await?;
            signal_supervisor().await?;
            let msg = format!("{service}: {action} requested");
            tracing::info!("{msg}");
            Ok(msg)
        }
        other => Err(format!("unknown action: {other}")),
    }
}

/// Write a control file to signal arkhe to start/stop a service.
async fn arkhe_ctl_write(service: &str, action: &str) -> Result<(), String> {
    let ctl_path = format!("/run/arkhe/{service}/ctl/{action}");
    if let Some(parent) = Path::new(&ctl_path).parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("failed to create ctl dir: {e}"))?;
    }
    tokio::fs::write(&ctl_path, "")
        .await
        .map_err(|e| format!("failed to write {ctl_path}: {e}"))?;
    Ok(())
}

/// Read the arkhd supervisor PID and send it SIGHUP to reload.
async fn signal_supervisor() -> Result<(), String> {
    let pid_str = tokio::fs::read_to_string("/run/arkhe/arkhd.pid")
        .await
        .map_err(|e| format!("failed to read arkhd.pid: {e}"))?;
    let pid: i32 = pid_str
        .trim()
        .parse()
        .map_err(|e| format!("invalid arkhd PID: {e}"))?;

    // Safety: sending SIGHUP to a running process is a standard Unix operation
    #[cfg(target_os = "linux")]
    {
        nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(pid),
            nix::sys::signal::Signal::SIGHUP,
        )
        .map_err(|e| format!("failed to signal arkhd (pid {pid}): {e}"))?;
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        tracing::warn!("signal_supervisor: no-op on non-Linux platform");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_path_points_to_config_dir() {
        let path = settings_path();
        let path_str = path.to_string_lossy();
        assert!(
            path_str.ends_with(".config/slate/settings.toml"),
            "unexpected path: {path_str}"
        );
    }

    #[test]
    fn save_and_reload_round_trip() {
        let dir = std::env::temp_dir().join("slate-settings-io-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.toml");

        let settings = Settings {
            display: slate_common::settings::DisplaySettings {
                scale_factor: 2.0,
                rotation_lock: false,
            },
            dock: slate_common::settings::DockSettings {
                auto_hide: true,
                icon_size: 56,
                ..Default::default()
            },
            ..Settings::default()
        };

        settings.save(&path).expect("save");
        let loaded = load_settings_from(&path);
        assert_eq!(settings, loaded);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_missing_file_returns_defaults() {
        let loaded = load_settings_from(Path::new("/nonexistent/settings.toml"));
        assert_eq!(loaded, Settings::default());
    }
}
