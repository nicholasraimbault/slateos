/// Waybar hot-reload via SIGUSR2.
///
/// After writing `palette.css`, Waybar needs a signal to re-read its styles.
/// We use `killall -SIGUSR2 waybar` for simplicity — if Waybar isn't running,
/// we log at debug level and continue.
use std::process::Command;

/// Send SIGUSR2 to all running Waybar instances to trigger a style reload.
///
/// Non-fatal: if Waybar isn't running or killall fails, we just log and move on.
pub fn reload_waybar() {
    match Command::new("killall")
        .args(["-SIGUSR2", "waybar"])
        .output()
    {
        Ok(output) if output.status.success() => {
            tracing::info!("sent SIGUSR2 to waybar");
        }
        Ok(output) => {
            // killall exits non-zero when no matching processes are found.
            tracing::debug!(
                "waybar not running or killall failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Err(err) => {
            tracing::debug!("could not run killall: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Calling reload_waybar when Waybar is not running should not panic.
    #[test]
    fn reload_waybar_does_not_panic() {
        // Waybar won't be running in CI/dev, but this must not crash.
        reload_waybar();
    }
}
