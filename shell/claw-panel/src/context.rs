// Focused-window context tracker.
//
// Polls `niri msg --json focused-window` to determine which application the
// user is currently looking at. The resulting WindowContext is sent alongside
// each OpenClaw query so the AI has awareness of the user's environment.

use serde::{Deserialize, Serialize};

/// Information about the currently focused Wayland window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowContext {
    pub app_id: String,
    pub title: String,
}

/// Intermediate struct matching the JSON shape emitted by
/// `niri msg --json focused-window`.
#[derive(Debug, Deserialize)]
struct NiriFocusedWindow {
    app_id: Option<String>,
    title: Option<String>,
}

/// Poll the Niri compositor for the currently focused window.
///
/// Returns `Ok(None)` when no window is focused (e.g. the desktop is showing).
/// Returns `Err` on I/O or parse failures.
pub async fn poll_focused_window() -> Result<Option<WindowContext>, anyhow::Error> {
    let output = tokio::process::Command::new("niri")
        .args(["msg", "--json", "focused-window"])
        .output()
        .await?;

    if !output.status.success() {
        // niri exits non-zero when no window is focused
        return Ok(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_focused_window(&stdout)
}

/// Parse the JSON output of `niri msg --json focused-window`.
///
/// Separated from I/O so it can be unit-tested without spawning a process.
pub fn parse_focused_window(json: &str) -> Result<Option<WindowContext>, anyhow::Error> {
    // niri returns "null" when no window is focused
    let trimmed = json.trim();
    if trimmed == "null" || trimmed.is_empty() {
        return Ok(None);
    }

    let niri: NiriFocusedWindow = serde_json::from_str(trimmed)?;

    let app_id = niri.app_id.unwrap_or_default();
    let title = niri.title.unwrap_or_default();

    if app_id.is_empty() && title.is_empty() {
        return Ok(None);
    }

    Ok(Some(WindowContext { app_id, title }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sample_focused_window_json() {
        let json = r#"{
            "app_id": "org.mozilla.firefox",
            "title": "Rust Documentation - Mozilla Firefox",
            "is_focused": true
        }"#;

        let ctx = parse_focused_window(json)
            .expect("should parse")
            .expect("should have context");

        assert_eq!(ctx.app_id, "org.mozilla.firefox");
        assert_eq!(ctx.title, "Rust Documentation - Mozilla Firefox");
    }

    #[test]
    fn parse_null_returns_none() {
        let result = parse_focused_window("null").expect("should parse");
        assert!(result.is_none());
    }

    #[test]
    fn parse_empty_string_returns_none() {
        let result = parse_focused_window("").expect("should parse");
        assert!(result.is_none());
    }

    #[test]
    fn parse_missing_fields_returns_none() {
        let json = r#"{"app_id": null, "title": null}"#;
        let result = parse_focused_window(json).expect("should parse");
        assert!(result.is_none());
    }

    #[test]
    fn parse_partial_fields() {
        let json = r#"{"app_id": "kitty", "title": null}"#;
        let ctx = parse_focused_window(json)
            .expect("should parse")
            .expect("should have context");
        assert_eq!(ctx.app_id, "kitty");
        assert_eq!(ctx.title, "");
    }

    #[test]
    fn window_context_serializes_to_json() {
        let ctx = WindowContext {
            app_id: "code".to_string(),
            title: "main.rs".to_string(),
        };
        let json = serde_json::to_string(&ctx).expect("serialize");
        assert!(json.contains("code"));
        assert!(json.contains("main.rs"));
    }
}
