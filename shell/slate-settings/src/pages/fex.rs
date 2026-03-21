/// FEX-Emu (x86 Compatibility) settings page.
///
/// Controls binfmt_misc registration for running x86 binaries.
/// These settings are not stored in settings.toml — they are managed
/// via system commands directly.
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

/// Check if FEX is registered in binfmt_misc.
pub async fn check_fex_status() -> FexState {
    let registered = tokio::fs::metadata("/proc/sys/fs/binfmt_misc/FEX-x86")
        .await
        .is_ok()
        || tokio::fs::metadata("/proc/sys/fs/binfmt_misc/FEX-x86_64")
            .await
            .is_ok();

    let rootfs_path = std::env::var("HOME")
        .map(|h| format!("{h}/.fex-emu/RootFS/"))
        .unwrap_or_else(|_| "/root/.fex-emu/RootFS/".to_string());

    let status_message = if registered {
        "FEX-Emu is registered in binfmt_misc".to_string()
    } else {
        "FEX-Emu is not registered".to_string()
    };

    FexState {
        registered,
        rootfs_path,
        status_message,
    }
}

/// Register or unregister FEX binfmt_misc entries.
async fn toggle_fex(enable: bool) -> Result<String, String> {
    let script = if enable {
        "FEXBash -c 'echo 1' 2>/dev/null || echo 'FEX registration attempted'"
    } else {
        "echo -1 > /proc/sys/fs/binfmt_misc/FEX-x86 2>/dev/null; echo -1 > /proc/sys/fs/binfmt_misc/FEX-x86_64 2>/dev/null; echo 'FEX unregistered'"
    };

    let output = tokio::process::Command::new("sh")
        .args(["-c", script])
        .output()
        .await
        .map_err(|e| format!("failed to run command: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if output.status.success() {
        Ok(stdout)
    } else {
        Err(format!("command failed: {stderr}"))
    }
}

// ---------------------------------------------------------------------------
// Update
// ---------------------------------------------------------------------------

pub fn update(state: &mut FexState, msg: FexMsg) -> Option<iced::Task<FexMsg>> {
    match msg {
        FexMsg::ToggleRegistration(enable) => {
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
                    tracing::info!("FEX: {msg}");
                }
                Err(msg) => {
                    state.status_message = msg.clone();
                    tracing::warn!("FEX: {msg}");
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
    let status_indicator = if state.registered {
        "Status: Active"
    } else {
        "Status: Inactive"
    };

    let content = column![
        text("x86 Compatibility (FEX-Emu)").size(24),
        toggler(state.registered)
            .label("Enable FEX-Emu binfmt_misc")
            .on_toggle(FexMsg::ToggleRegistration),
        text(status_indicator).size(14),
        text(format!("Rootfs: {}", state.rootfs_path)).size(14),
        text(&state.status_message).size(12),
    ]
    .spacing(16)
    .padding(20)
    .width(Length::Fill);

    container(content).width(Length::Fill).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_fex_state_is_unregistered() {
        let state = FexState::default();
        assert!(!state.registered);
    }

    #[test]
    fn status_message_updates_on_command_result() {
        let mut state = FexState::default();
        let _ = update(&mut state, FexMsg::CommandResult(Ok("done".to_string())));
        assert_eq!(state.status_message, "done");
    }
}
