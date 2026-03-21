/// FEX-Emu (x86 Compatibility) settings page.
///
/// Controls binfmt_misc registration for running x86 binaries.
/// These settings are not stored in settings.toml — they are managed
/// via system commands directly. Gracefully handles missing tools by
/// showing an "unavailable" status instead of crashing.
use iced::widget::{column, container, text, toggler};
use iced::{Element, Length};

/// State for the FEX settings page.
#[derive(Debug, Clone, Default)]
pub struct FexState {
    /// Whether FEX binfmt_misc is currently registered.
    pub registered: bool,
    /// Path to the x86 rootfs (read from FEX config or hardcoded).
    pub rootfs_path: String,
    /// Status message from last operation.
    pub status_message: String,
    /// Error from the last failed command, shown inline.
    pub error_message: Option<String>,
    /// Whether the FEX tooling is present on the system.
    pub available: bool,
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum FexMsg {
    ToggleRegistration(bool),
    StatusChecked(FexState),
    CommandResult(Result<String, String>),
}

// ---------------------------------------------------------------------------
// System interaction
// ---------------------------------------------------------------------------

/// Check if FEX is registered in binfmt_misc. Also detects whether the
/// binfmt_misc filesystem is mounted, which determines if the toggle
/// should be presented.
pub async fn check_fex_status() -> FexState {
    // Check whether binfmt_misc is mounted at all (kernel feature)
    let binfmt_available = tokio::fs::metadata("/proc/sys/fs/binfmt_misc")
        .await
        .is_ok();

    if !binfmt_available {
        tracing::debug!("binfmt_misc not mounted, FEX controls unavailable");
        return FexState {
            available: false,
            status_message: "binfmt_misc is not available on this system".to_string(),
            ..Default::default()
        };
    }

    let registered = tokio::fs::metadata("/proc/sys/fs/binfmt_misc/FEX-x86")
        .await
        .is_ok()
        || tokio::fs::metadata("/proc/sys/fs/binfmt_misc/FEX-x86_64")
            .await
            .is_ok();

    // Verify FEXBash is on PATH so we can actually toggle registration
    let fex_installed = tokio::process::Command::new("sh")
        .args(["-c", "command -v FEXBash"])
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);

    let rootfs_path = std::env::var("HOME")
        .map(|h| format!("{h}/.fex-emu/RootFS/"))
        .unwrap_or_else(|_| "/root/.fex-emu/RootFS/".to_string());

    let (available, status_message) = if !fex_installed && !registered {
        (false, "FEX-Emu is not installed on this system".to_string())
    } else if registered {
        (true, "FEX-Emu is registered in binfmt_misc".to_string())
    } else {
        (true, "FEX-Emu is installed but not registered".to_string())
    };

    FexState {
        registered,
        rootfs_path,
        status_message,
        error_message: None,
        available,
    }
}

/// Register or unregister FEX binfmt_misc entries using shell commands
/// that work on Chimera Linux without systemctl.
async fn toggle_fex(enable: bool) -> Result<String, String> {
    let script = if enable {
        "FEXBash -c 'echo 1' 2>/dev/null || echo 'FEX registration attempted'"
    } else {
        concat!(
            "echo -1 > /proc/sys/fs/binfmt_misc/FEX-x86 2>/dev/null; ",
            "echo -1 > /proc/sys/fs/binfmt_misc/FEX-x86_64 2>/dev/null; ",
            "echo 'FEX unregistered'"
        )
    };

    let output = tokio::process::Command::new("sh")
        .args(["-c", script])
        .output()
        .await
        .map_err(|e| format!("Failed to run command: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if output.status.success() {
        Ok(if stdout.is_empty() {
            "Done".to_string()
        } else {
            stdout
        })
    } else {
        let detail = if stderr.is_empty() { &stdout } else { &stderr };
        Err(format!("Command failed: {detail}"))
    }
}

// ---------------------------------------------------------------------------
// Update
// ---------------------------------------------------------------------------

pub fn update(state: &mut FexState, msg: FexMsg) -> Option<iced::Task<FexMsg>> {
    match msg {
        FexMsg::ToggleRegistration(enable) => {
            state.error_message = None;
            let task = iced::Task::perform(
                async move { toggle_fex(enable).await },
                FexMsg::CommandResult,
            );
            Some(task)
        }
        FexMsg::StatusChecked(new_state) => {
            *state = new_state;
            None
        }
        FexMsg::CommandResult(result) => {
            match &result {
                Ok(msg) => {
                    state.status_message = msg.clone();
                    state.error_message = None;
                    tracing::info!("FEX: {msg}");
                }
                Err(msg) => {
                    tracing::warn!("FEX: {msg}");
                    state.error_message = Some(msg.clone());
                }
            }
            // Re-check status after the command
            let task =
                iced::Task::perform(async { check_fex_status().await }, FexMsg::StatusChecked);
            Some(task)
        }
    }
}

// ---------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------

pub fn view(state: &FexState) -> Element<'_, FexMsg> {
    let mut items: Vec<Element<'_, FexMsg>> = Vec::new();

    items.push(text("x86 Compatibility (FEX-Emu)").size(24).into());

    if !state.available {
        // FEX tooling or binfmt_misc is not present — show informational
        // message instead of a broken toggle
        items.push(
            text(&state.status_message)
                .size(14)
                .color(iced::Color::from_rgb(0.5, 0.5, 0.5))
                .into(),
        );
        items.push(
            text("Install FEX-Emu to enable x86 application support.")
                .size(12)
                .color(iced::Color::from_rgb(0.5, 0.5, 0.5))
                .into(),
        );
    } else {
        let status_indicator = if state.registered {
            "Status: Active"
        } else {
            "Status: Inactive"
        };

        items.push(
            toggler(state.registered)
                .label("Enable FEX-Emu binfmt_misc")
                .on_toggle(FexMsg::ToggleRegistration)
                .into(),
        );
        items.push(text(status_indicator).size(14).into());
        items.push(
            text(format!("Rootfs: {}", state.rootfs_path))
                .size(14)
                .into(),
        );
        items.push(text(&state.status_message).size(12).into());

        // Inline error from last failed toggle
        if let Some(err) = &state.error_message {
            items.push(
                text(err)
                    .size(12)
                    .color(iced::Color::from_rgb(0.9, 0.2, 0.2))
                    .into(),
            );
        }
    }

    let content = column(items).spacing(16).padding(20).width(Length::Fill);

    container(content).width(Length::Fill).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_fex_state_is_unregistered() {
        let state = FexState::default();
        assert!(!state.registered);
        assert!(!state.available);
    }

    #[test]
    fn status_message_updates_on_command_result() {
        let mut state = FexState::default();
        let _ = update(&mut state, FexMsg::CommandResult(Ok("done".to_string())));
        assert_eq!(state.status_message, "done");
        assert!(state.error_message.is_none());
    }

    #[test]
    fn error_message_set_on_command_failure() {
        let mut state = FexState::default();
        let _ = update(
            &mut state,
            FexMsg::CommandResult(Err("permission denied".into())),
        );
        assert_eq!(state.error_message.as_deref(), Some("permission denied"));
    }

    #[test]
    fn toggle_clears_previous_error() {
        let mut state = FexState {
            error_message: Some("old error".into()),
            ..Default::default()
        };
        let _ = update(&mut state, FexMsg::ToggleRegistration(true));
        assert!(state.error_message.is_none());
    }

    #[test]
    fn status_checked_replaces_entire_state() {
        let mut state = FexState {
            registered: true,
            error_message: Some("stale".into()),
            ..Default::default()
        };
        let new = FexState {
            registered: false,
            available: true,
            status_message: "fresh".into(),
            ..Default::default()
        };
        update(&mut state, FexMsg::StatusChecked(new));
        assert!(!state.registered);
        assert_eq!(state.status_message, "fresh");
        assert!(state.error_message.is_none());
    }
}
