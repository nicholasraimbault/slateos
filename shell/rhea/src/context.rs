/// Shell context gatherer for AI request augmentation.
///
/// Populates `AiContext` by querying live shell state:
/// - focused window title from `niri msg focused-window`
/// - clipboard text from `wl-paste`
/// - recent notifications from the org.slate.Notifications D-Bus service
use anyhow::Result;
use tracing::{debug, warn};

use slate_common::ai::AiContext;

// ---------------------------------------------------------------------------
// Clipboard limit — avoid sending huge strings to the AI backend
// ---------------------------------------------------------------------------

const CLIPBOARD_MAX_CHARS: usize = 500;

// ---------------------------------------------------------------------------
// Context gatherer
// ---------------------------------------------------------------------------

/// Gather current shell context asynchronously.
///
/// Individual failures (e.g. no focused window, no clipboard) are non-fatal —
/// only missing fields are left as `None`/empty.
pub async fn gather() -> AiContext {
    let (focused_window, clipboard, recent_notifications) =
        tokio::join!(get_focused_window(), get_clipboard(), get_notifications(),);
    AiContext {
        focused_window,
        clipboard,
        recent_notifications,
    }
}

/// Query niri for the currently focused window.
async fn get_focused_window() -> Option<String> {
    let output = run_subprocess("niri", &["msg", "focused-window"])
        .await
        .ok()?;
    parse_focused_window(&output)
}

/// Parse the `niri msg focused-window` output to extract a display string.
///
/// The output is free-form text; we return a cleaned first non-empty line.
pub fn parse_focused_window(output: &str) -> Option<String> {
    // niri outputs something like:
    //   Focused window:
    //     App ID: firefox
    //     Title: Mozilla Firefox
    // We extract the Title line when present, falling back to App ID.
    let mut title: Option<&str> = None;
    let mut app_id: Option<&str> = None;

    for line in output.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Title:") {
            title = Some(rest.trim());
        } else if let Some(rest) = trimmed.strip_prefix("App ID:") {
            app_id = Some(rest.trim());
        }
    }

    title
        .or(app_id)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Read the clipboard via `wl-paste`.
async fn get_clipboard() -> Option<String> {
    let output = run_subprocess("wl-paste", &["--no-newline"]).await.ok()?;
    if output.is_empty() {
        return None;
    }
    // Truncate to avoid sending huge clipboard contents to AI.
    // Use char-boundary-safe truncation so multi-byte UTF-8 never panics.
    let truncated = if output.chars().count() > CLIPBOARD_MAX_CHARS {
        let truncated: String = output.chars().take(CLIPBOARD_MAX_CHARS).collect();
        truncated + "…"
    } else {
        output
    };
    Some(truncated)
}

/// Fetch recent notifications from the slate-notifyd D-Bus service.
///
/// Returns an empty vec on any error (daemon may not be running).
async fn get_notifications() -> Vec<String> {
    match fetch_notifications_dbus().await {
        Ok(list) => list,
        Err(e) => {
            debug!("could not fetch notifications from D-Bus: {e}");
            Vec::new()
        }
    }
}

async fn fetch_notifications_dbus() -> Result<Vec<String>> {
    let conn = zbus::Connection::session().await?;

    let proxy = zbus::Proxy::new(
        &conn,
        slate_common::dbus::NOTIFICATIONS_BUS_NAME,
        slate_common::dbus::NOTIFICATIONS_PATH,
        slate_common::dbus::NOTIFICATIONS_INTERFACE,
    )
    .await?;

    // GetActive returns a TOML string; we parse the notification summaries.
    let toml_str: String = proxy.call("GetActive", &()).await?;

    let summaries = parse_notification_toml(&toml_str);
    Ok(summaries)
}

/// Extract summary strings from the TOML returned by slate-notifyd GetActive.
///
/// Expects the shape `[[notifications]] ... summary = "..."`.
pub fn parse_notification_toml(toml_str: &str) -> Vec<String> {
    #[derive(serde::Deserialize)]
    struct Wrapper {
        #[serde(default)]
        notifications: Vec<NotifRecord>,
    }
    #[derive(serde::Deserialize)]
    struct NotifRecord {
        #[serde(default)]
        summary: String,
        #[serde(default)]
        body: String,
    }

    match toml::from_str::<Wrapper>(toml_str) {
        Ok(w) => w
            .notifications
            .into_iter()
            .map(|n| {
                if n.body.is_empty() {
                    n.summary
                } else {
                    format!("{}: {}", n.summary, n.body)
                }
            })
            .filter(|s| !s.is_empty())
            .collect(),
        Err(e) => {
            warn!("failed to parse notifications TOML: {e}");
            Vec::new()
        }
    }
}

// ---------------------------------------------------------------------------
// Subprocess helper
// ---------------------------------------------------------------------------

/// Run a subprocess and capture its stdout as a `String`.
async fn run_subprocess(program: &str, args: &[&str]) -> Result<String> {
    let output = tokio::process::Command::new(program)
        .args(args)
        .output()
        .await?;

    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "{program} exited with status {}",
            output.status
        ));
    }

    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    Ok(text)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_focused_window_extracts_title() {
        let output = "Focused window:\n  App ID: firefox\n  Title: Mozilla Firefox\n";
        let result = parse_focused_window(output);
        assert_eq!(result, Some("Mozilla Firefox".to_string()));
    }

    #[test]
    fn parse_focused_window_falls_back_to_app_id() {
        let output = "Focused window:\n  App ID: alacritty\n";
        let result = parse_focused_window(output);
        assert_eq!(result, Some("alacritty".to_string()));
    }

    #[test]
    fn parse_focused_window_returns_none_on_empty() {
        assert!(parse_focused_window("").is_none());
        assert!(parse_focused_window("No focused window").is_none());
    }

    #[test]
    fn parse_notification_toml_extracts_summaries() {
        let toml_str = r#"
[[notifications]]
summary = "New email"
body = "From alice"

[[notifications]]
summary = "Build passed"
body = ""
"#;
        let summaries = parse_notification_toml(toml_str);
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0], "New email: From alice");
        assert_eq!(summaries[1], "Build passed");
    }

    #[test]
    fn parse_notification_toml_empty_string_gives_empty_vec() {
        let summaries = parse_notification_toml("");
        assert!(summaries.is_empty());
    }

    #[test]
    fn parse_notification_toml_invalid_toml_gives_empty_vec() {
        let summaries = parse_notification_toml("not { valid toml }}}");
        assert!(summaries.is_empty());
    }

    #[test]
    fn parse_notification_toml_no_notifications_key() {
        let toml_str = r#"foo = "bar""#;
        let summaries = parse_notification_toml(toml_str);
        assert!(summaries.is_empty());
    }

    #[test]
    fn clipboard_truncation_boundary() {
        // Simulate what the clipboard truncation does to a long ASCII string.
        let long_str = "a".repeat(600);
        let truncated = if long_str.chars().count() > CLIPBOARD_MAX_CHARS {
            let s: String = long_str.chars().take(CLIPBOARD_MAX_CHARS).collect();
            s + "…"
        } else {
            long_str.clone()
        };
        // The returned string starts at the limit and has the ellipsis appended.
        assert!(truncated.starts_with(&"a".repeat(CLIPBOARD_MAX_CHARS)));
        assert!(truncated.ends_with('…'));
    }

    #[test]
    fn truncate_clipboard_multibyte() {
        // "café" has a multi-byte é; ensure char-based truncation never panics
        // and respects the character limit rather than a byte limit.
        let input = "café".repeat(200); // way over 500 chars
        let truncated = if input.chars().count() > CLIPBOARD_MAX_CHARS {
            let s: String = input.chars().take(CLIPBOARD_MAX_CHARS).collect();
            s + "…"
        } else {
            input.clone()
        };
        // Must not exceed CLIPBOARD_MAX_CHARS chars (plus the ellipsis).
        assert!(truncated.chars().count() <= CLIPBOARD_MAX_CHARS + 1);
        // Each char may be up to 4 bytes, so byte length is bounded.
        assert!(truncated.len() <= CLIPBOARD_MAX_CHARS * 4 + "…".len());
        assert!(truncated.ends_with('…'));
    }
}
