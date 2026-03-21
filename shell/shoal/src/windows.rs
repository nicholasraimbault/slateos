/// Window tracker for Shoal.
///
/// Polls the Niri compositor via `niri msg --json windows` to discover which
/// applications are currently running and which one has focus. The dock uses
/// this to show running indicators and to decide whether a tap should launch
/// or focus an app.
use serde::Deserialize;

/// A running application window as reported by Niri.
#[derive(Debug, Clone, PartialEq)]
pub struct RunningApp {
    /// The Wayland `app_id`, used to match against desktop entries.
    pub app_id: String,
    /// Window title (may be empty).
    pub title: String,
    /// Whether this window currently has keyboard focus.
    pub is_focused: bool,
}

/// Raw JSON shape returned by `niri msg --json windows`.
#[derive(Debug, Deserialize)]
struct NiriWindow {
    app_id: Option<String>,
    title: Option<String>,
    is_focused: Option<bool>,
}

/// Query Niri for the list of open windows.
///
/// Spawns `niri msg --json windows` and parses the JSON array output. Returns
/// an error if the command fails or produces invalid JSON.
pub async fn poll_running_apps() -> anyhow::Result<Vec<RunningApp>> {
    let output = tokio::process::Command::new("niri")
        .args(["msg", "--json", "windows"])
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("niri msg failed: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_niri_json(&stdout)
}

/// Parse the JSON array produced by `niri msg --json windows` into our
/// domain type. Windows without an `app_id` are skipped.
pub fn parse_niri_json(json: &str) -> anyhow::Result<Vec<RunningApp>> {
    let windows: Vec<NiriWindow> = serde_json::from_str(json)?;

    let apps = windows
        .into_iter()
        .filter_map(|w| {
            let app_id = w.app_id.filter(|id| !id.is_empty())?;
            Some(RunningApp {
                app_id,
                title: w.title.unwrap_or_default(),
                is_focused: w.is_focused.unwrap_or(false),
            })
        })
        .collect();

    Ok(apps)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_typical_niri_output() {
        let json = r#"[
            {
                "app_id": "firefox",
                "title": "Mozilla Firefox",
                "is_focused": true
            },
            {
                "app_id": "Alacritty",
                "title": "~",
                "is_focused": false
            }
        ]"#;

        let apps = parse_niri_json(json).expect("should parse");
        assert_eq!(apps.len(), 2);

        assert_eq!(apps[0].app_id, "firefox");
        assert_eq!(apps[0].title, "Mozilla Firefox");
        assert!(apps[0].is_focused);

        assert_eq!(apps[1].app_id, "Alacritty");
        assert_eq!(apps[1].title, "~");
        assert!(!apps[1].is_focused);
    }

    #[test]
    fn parse_empty_window_list() {
        let json = "[]";
        let apps = parse_niri_json(json).expect("should parse");
        assert!(apps.is_empty());
    }

    #[test]
    fn parse_window_without_app_id_skipped() {
        let json = r#"[
            {
                "app_id": null,
                "title": "Ghost Window",
                "is_focused": false
            },
            {
                "app_id": "",
                "title": "Empty ID",
                "is_focused": false
            },
            {
                "app_id": "real-app",
                "title": "Real App",
                "is_focused": true
            }
        ]"#;

        let apps = parse_niri_json(json).expect("should parse");
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].app_id, "real-app");
    }

    #[test]
    fn parse_missing_optional_fields() {
        // Niri may omit fields entirely
        let json = r#"[
            {
                "app_id": "minimal"
            }
        ]"#;

        let apps = parse_niri_json(json).expect("should parse");
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].app_id, "minimal");
        assert_eq!(apps[0].title, "");
        assert!(!apps[0].is_focused);
    }

    #[test]
    fn parse_invalid_json_returns_error() {
        let json = "not json at all";
        let result = parse_niri_json(json);
        assert!(result.is_err());
    }

    #[test]
    fn parse_niri_output_with_extra_fields() {
        // Niri may add new fields in the future; we should ignore them
        let json = r#"[
            {
                "app_id": "firefox",
                "title": "Tab Title",
                "is_focused": false,
                "workspace_id": 1,
                "pid": 12345,
                "geometry": {"x": 0, "y": 0, "width": 1920, "height": 1080}
            }
        ]"#;

        let apps = parse_niri_json(json).expect("should parse with extra fields");
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].app_id, "firefox");
    }
}
