/// App actions for Shoal.
///
/// Handles launching apps, focusing running windows, and closing windows via
/// Niri IPC. All operations that mutate the compositor state live here.
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
        // Should log a warning but not panic
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
        // Should log an error but not panic
        launch_app(&entry);
    }

    #[test]
    fn test_entry_has_valid_fields() {
        let entry = test_entry();
        assert!(!entry.exec.is_empty());
        assert!(!entry.desktop_id.is_empty());
    }
}
