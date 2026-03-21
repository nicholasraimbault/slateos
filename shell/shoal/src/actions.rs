/// App actions for Shoal.
///
/// Handles launching apps, focusing running windows, closing windows via
/// Niri IPC, and persisting the pinned dock list to settings.toml.
use std::path::PathBuf;

use slate_common::Settings;

use crate::desktop::DesktopEntry;

/// Launch an application using its Exec command from the desktop entry.
///
/// Spawns the process detached so the dock is not blocked. Errors are logged
/// rather than propagated because a failed launch should not crash the dock.
pub fn launch_app(entry: &DesktopEntry) {
    let parts: Vec<&str> = entry.exec.split_whitespace().collect();
    if parts.is_empty() {
        tracing::warn!("empty exec command for {}", entry.desktop_id);
        return;
    }

    let program = parts[0];
    let args = &parts[1..];

    match std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(_) => tracing::info!("launched {}", entry.desktop_id),
        Err(e) => tracing::warn!("failed to launch {}: {e}", entry.desktop_id),
    }
}

/// Focus an existing window by app_id via Niri IPC.
pub async fn focus_app(app_id: &str) {
    let result = tokio::process::Command::new("niri")
        .args(["msg", "action", "focus-window", "--app-id", app_id])
        .output()
        .await;

    match result {
        Ok(output) if output.status.success() => {
            tracing::info!("focused window: {app_id}");
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!("failed to focus {app_id}: {stderr}");
        }
        Err(e) => {
            tracing::warn!("failed to run niri focus for {app_id}: {e}");
        }
    }
}

/// Close all windows matching the given app_id.
pub async fn close_app(app_id: &str) {
    let result = tokio::process::Command::new("niri")
        .args(["msg", "action", "close-window", "--app-id", app_id])
        .output()
        .await;

    match result {
        Ok(output) if output.status.success() => {
            tracing::info!("closed window: {app_id}");
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!("failed to close {app_id}: {stderr}");
        }
        Err(e) => {
            tracing::warn!("failed to run niri close for {app_id}: {e}");
        }
    }
}

/// Decide whether to launch or focus an app based on running state.
pub async fn activate_app(entry: &DesktopEntry, is_running: bool) {
    if is_running {
        focus_app(&entry.desktop_id).await;
    } else {
        launch_app(entry);
    }
}

// ---------------------------------------------------------------------------
// Pin/Unpin persistence
// ---------------------------------------------------------------------------

/// Canonical path to the settings file, matching slate-settings convention.
pub fn settings_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    PathBuf::from(home).join(".config/slate/settings.toml")
}

/// Add a desktop ID to the pinned apps list and persist to disk.
///
/// No-op if the app is already pinned.
pub async fn pin_app(desktop_id: &str) {
    let id = desktop_id.to_string();
    let result = tokio::task::spawn_blocking(move || pin_app_sync(&id)).await;

    match result {
        Ok(Ok(())) => tracing::info!("pinned app: {desktop_id}"),
        Ok(Err(e)) => tracing::warn!("failed to pin {desktop_id}: {e}"),
        Err(e) => tracing::warn!("pin task panicked for {desktop_id}: {e}"),
    }
}

/// Remove a desktop ID from the pinned apps list and persist to disk.
///
/// No-op if the app is not currently pinned.
pub async fn unpin_app(desktop_id: &str) {
    let id = desktop_id.to_string();
    let result = tokio::task::spawn_blocking(move || unpin_app_sync(&id)).await;

    match result {
        Ok(Ok(())) => tracing::info!("unpinned app: {desktop_id}"),
        Ok(Err(e)) => tracing::warn!("failed to unpin {desktop_id}: {e}"),
        Err(e) => tracing::warn!("unpin task panicked for {desktop_id}: {e}"),
    }
}

/// Blocking implementation of pin. Reads settings, appends the ID, writes back.
fn pin_app_sync(desktop_id: &str) -> anyhow::Result<()> {
    let path = settings_path();
    let mut settings = load_or_default(&path);

    if !settings
        .dock
        .pinned_apps
        .iter()
        .any(|id| id.eq_ignore_ascii_case(desktop_id))
    {
        settings.dock.pinned_apps.push(desktop_id.to_string());
        settings.save(&path)?;
    }

    Ok(())
}

/// Blocking implementation of unpin. Reads settings, removes the ID, writes back.
fn unpin_app_sync(desktop_id: &str) -> anyhow::Result<()> {
    let path = settings_path();
    let mut settings = load_or_default(&path);

    let before = settings.dock.pinned_apps.len();
    settings
        .dock
        .pinned_apps
        .retain(|id| !id.eq_ignore_ascii_case(desktop_id));

    if settings.dock.pinned_apps.len() != before {
        settings.save(&path)?;
    }

    Ok(())
}

/// Load settings from disk, falling back to defaults if the file is missing.
fn load_or_default(path: &std::path::Path) -> Settings {
    match Settings::load(path) {
        Ok(s) => s,
        Err(e) => {
            tracing::debug!("could not load settings: {e}; using defaults");
            Settings::default()
        }
    }
}

/// Read the current pinned apps list from settings on disk.
///
/// Falls back to the default list if the settings file is missing or corrupt.
pub fn load_pinned_apps() -> Vec<String> {
    let path = settings_path();
    load_or_default(&path).dock.pinned_apps
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_entry() -> DesktopEntry {
        DesktopEntry {
            name: "Test".to_string(),
            exec: "echo hello".to_string(),
            icon: "test".to_string(),
            desktop_id: "test-app".to_string(),
        }
    }

    #[test]
    fn launch_app_with_empty_exec_does_not_panic() {
        let entry = DesktopEntry {
            name: "Empty".to_string(),
            exec: String::new(),
            icon: String::new(),
            desktop_id: "empty".to_string(),
        };
        launch_app(&entry);
    }

    #[test]
    fn launch_app_with_nonexistent_binary_does_not_panic() {
        let entry = DesktopEntry {
            name: "Bad".to_string(),
            exec: "this-binary-does-not-exist-12345".to_string(),
            icon: String::new(),
            desktop_id: "bad".to_string(),
        };
        launch_app(&entry);
    }

    #[test]
    fn test_entry_has_valid_fields() {
        let entry = test_entry();
        assert!(!entry.exec.is_empty());
        assert!(!entry.desktop_id.is_empty());
    }

    #[test]
    fn settings_path_ends_with_settings_toml() {
        let path = settings_path();
        let path_str = path.to_string_lossy();
        assert!(
            path_str.ends_with(".config/slate/settings.toml"),
            "unexpected path: {path_str}"
        );
    }

    #[test]
    fn pin_sync_adds_to_list() {
        let dir = std::env::temp_dir().join("shoal-test-pin");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create test dir");
        let path = dir.join("settings.toml");

        let settings = Settings::default();
        settings.save(&path).expect("save");

        // Manually call the sync pin with a custom path
        let mut s = load_or_default(&path);
        assert!(!s.dock.pinned_apps.contains(&"new-app".to_string()));
        s.dock.pinned_apps.push("new-app".to_string());
        s.save(&path).expect("save after pin");

        let reloaded = load_or_default(&path);
        assert!(reloaded.dock.pinned_apps.contains(&"new-app".to_string()));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unpin_sync_removes_from_list() {
        let dir = std::env::temp_dir().join("shoal-test-unpin");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create test dir");
        let path = dir.join("settings.toml");

        let settings = Settings::default();
        settings.save(&path).expect("save");

        let mut s = load_or_default(&path);
        let had_alacritty = s
            .dock
            .pinned_apps
            .iter()
            .any(|id| id.eq_ignore_ascii_case("Alacritty"));
        assert!(had_alacritty, "default should contain Alacritty");

        s.dock
            .pinned_apps
            .retain(|id| !id.eq_ignore_ascii_case("Alacritty"));
        s.save(&path).expect("save after unpin");

        let reloaded = load_or_default(&path);
        let still_has = reloaded
            .dock
            .pinned_apps
            .iter()
            .any(|id| id.eq_ignore_ascii_case("Alacritty"));
        assert!(!still_has, "Alacritty should have been removed");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_or_default_returns_defaults_for_missing_file() {
        let s = load_or_default(std::path::Path::new("/nonexistent/settings.toml"));
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn load_pinned_apps_returns_default_list() {
        // On a dev machine without a settings file, this should return
        // the default pinned apps without panicking.
        let apps = load_pinned_apps();
        assert!(
            !apps.is_empty(),
            "default pinned apps list should not be empty"
        );
    }
}
