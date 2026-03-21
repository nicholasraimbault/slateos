// Wayland clipboard integration.
//
// Copies text to the system clipboard via `wl-copy`, the standard
// Wayland clipboard tool from wl-clipboard. Falls back gracefully
// when wl-copy is not installed.

use anyhow::{Context, Result};

/// Copy the given text to the Wayland clipboard via `wl-copy`.
///
/// The text is piped through stdin so arbitrarily large or multi-line
/// content is handled correctly (no shell quoting issues).
///
/// Returns `Ok(())` on success, or an error if `wl-copy` is missing
/// or fails.
pub async fn copy_to_clipboard(text: &str) -> Result<()> {
    use tokio::io::AsyncWriteExt;

    let mut child = tokio::process::Command::new("wl-copy")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("failed to run wl-copy — is wl-clipboard installed?")?;

    // Write the text to wl-copy's stdin, then close it so wl-copy
    // knows the input is complete.
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(text.as_bytes())
            .await
            .context("failed to write to wl-copy stdin")?;
        // Dropping stdin closes the pipe.
    }

    let output = child
        .wait_with_output()
        .await
        .context("failed to wait for wl-copy")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("wl-copy failed (status {}): {stderr}", output.status);
    }

    tracing::debug!("copied {} bytes to clipboard via wl-copy", text.len());
    Ok(())
}

/// Check whether the given window title or app_id looks like a terminal.
///
/// Used to decide whether to also inject text via `wtype` in addition
/// to copying to clipboard.
pub fn is_terminal_window(app_id: &str, title: &str) -> bool {
    let terminal_ids = [
        "kitty",
        "alacritty",
        "foot",
        "wezterm",
        "gnome-terminal",
        "konsole",
        "xterm",
        "terminator",
        "tilix",
        "st",
        "urxvt",
        "sakura",
    ];

    let app_lower = app_id.to_lowercase();
    let title_lower = title.to_lowercase();

    for id in &terminal_ids {
        if app_lower.contains(id) || title_lower.contains(id) {
            return true;
        }
    }

    // Also catch generic "terminal" in the title
    title_lower.contains("terminal")
}

/// Inject text into the focused window via `wtype`.
///
/// This is used when the focused window is a terminal so the user gets
/// the code block typed directly into their shell. Uses synchronous
/// spawning wrapped in `spawn_blocking` because wtype is fast and
/// doing I/O on a blocking thread avoids pulling in the inject module
/// from slate-suggest.
pub async fn inject_text_wtype(text: &str) -> Result<()> {
    if text.is_empty() {
        return Ok(());
    }

    let text = text.to_string();
    tokio::task::spawn_blocking(move || {
        let output = std::process::Command::new("wtype")
            .arg("--")
            .arg(&text)
            .output()
            .context("failed to run wtype — is it installed?")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("wtype failed (status {}): {stderr}", output.status);
        }

        tracing::debug!("injected {} bytes via wtype", text.len());
        Ok(())
    })
    .await
    .context("wtype spawn_blocking task panicked")?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_terminal_detects_kitty() {
        assert!(is_terminal_window("kitty", "~"));
    }

    #[test]
    fn is_terminal_detects_alacritty() {
        assert!(is_terminal_window("Alacritty", "bash"));
    }

    #[test]
    fn is_terminal_detects_foot() {
        assert!(is_terminal_window("foot", "user@host: ~/code"));
    }

    #[test]
    fn is_terminal_detects_wezterm() {
        assert!(is_terminal_window("org.wezfurlong.wezterm", "zsh"));
    }

    #[test]
    fn is_terminal_detects_terminal_in_title() {
        assert!(is_terminal_window("some-app", "My Terminal"));
    }

    #[test]
    fn is_terminal_rejects_firefox() {
        assert!(!is_terminal_window("firefox", "GitHub - Mozilla Firefox"));
    }

    #[test]
    fn is_terminal_rejects_empty() {
        assert!(!is_terminal_window("", ""));
    }

    #[test]
    fn is_terminal_case_insensitive() {
        assert!(is_terminal_window("KITTY", "session"));
        assert!(is_terminal_window("Foot", "bash"));
    }

    #[test]
    fn copy_to_clipboard_is_async() {
        // Verify the function signature compiles as async
        fn _assert_async() -> impl std::future::Future<Output = Result<()>> {
            copy_to_clipboard("test")
        }
    }

    #[test]
    fn inject_text_wtype_is_async() {
        // Verify the function signature compiles as async
        fn _assert_async() -> impl std::future::Future<Output = Result<()>> {
            inject_text_wtype("test")
        }
    }
}
