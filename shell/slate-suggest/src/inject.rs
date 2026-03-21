/// Text injection into the focused application.
///
/// Uses `wtype` to type text via the Wayland virtual keyboard protocol.
/// This is how tapping a suggestion chip sends the completed command
/// into the terminal or text field.
use anyhow::{Context, Result};

/// Inject text into the focused Wayland application via `wtype`.
///
/// `wtype` interprets special characters, so the text is passed as a
/// single argument with proper escaping. On non-Wayland systems or when
/// wtype is not installed, this returns an error.
pub fn inject_text(text: &str) -> Result<()> {
    if text.is_empty() {
        return Ok(());
    }

    let escaped = escape_for_wtype(text);

    let output = std::process::Command::new("wtype")
        .arg("--")
        .arg(&escaped)
        .output()
        .context("failed to run wtype — is it installed?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("wtype failed (status {}): {stderr}", output.status);
    }

    tracing::debug!("injected text via wtype: {text:?}");
    Ok(())
}

/// Escape special characters for safe wtype injection.
///
/// wtype with the `--` argument treats text literally, but we still
/// need to handle the case where the text itself might cause issues
/// with shell interpretation if spawned incorrectly. Since we use
/// `Command::arg` (not shell expansion), the main concern is ensuring
/// the string is passed intact.
fn escape_for_wtype(text: &str) -> String {
    // Command::arg passes the argument directly to the process without
    // shell interpretation, so no shell escaping is needed. However,
    // wtype itself interprets certain sequences. Using `--` before the
    // text argument tells wtype to treat it as literal text.
    text.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_preserves_plain_text() {
        assert_eq!(escape_for_wtype("ls -la"), "ls -la");
    }

    #[test]
    fn escape_preserves_quotes() {
        assert_eq!(
            escape_for_wtype("git commit -m 'hello world'"),
            "git commit -m 'hello world'"
        );
    }

    #[test]
    fn escape_preserves_double_quotes() {
        assert_eq!(escape_for_wtype(r#"echo "hello""#), r#"echo "hello""#);
    }

    #[test]
    fn escape_preserves_backslashes() {
        assert_eq!(escape_for_wtype(r"path\to\file"), r"path\to\file");
    }

    #[test]
    fn escape_preserves_special_chars() {
        assert_eq!(
            escape_for_wtype("echo $HOME && ls | grep foo"),
            "echo $HOME && ls | grep foo"
        );
    }

    #[test]
    fn inject_empty_text_is_noop() {
        // Should return Ok without spawning wtype
        let result = inject_text("");
        assert!(result.is_ok());
    }
}
