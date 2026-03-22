// D-Bus client helpers for the Rhea AI engine.
//
// Provides one-shot async calls to Rhea methods invoked directly by the panel
// (i.e. not via the subscription signal path). Each function has a Linux
// implementation that opens a transient session-bus connection and a stub for
// other platforms.

// ---------------------------------------------------------------------------
// CompleteStream
// ---------------------------------------------------------------------------

/// Call `org.slate.Rhea.CompleteStream` over D-Bus with the user's prompt.
///
/// The actual response text arrives as `CompletionChunk` D-Bus signals which
/// are picked up by `dbus_listener::watch_rhea` and forwarded as
/// `DbusEvent::RheaChunk` messages. This function only initiates the call and
/// returns once the method returns (or errors).
///
/// A system context string is intentionally left empty here because Rhea
/// gathers shell context internally (focused window, clipboard, etc.).
#[cfg(target_os = "linux")]
pub async fn call_rhea_complete_stream(prompt: String) -> Result<(), String> {
    use slate_common::dbus::{RHEA_BUS_NAME, RHEA_INTERFACE, RHEA_PATH};

    let connection = zbus::Connection::session()
        .await
        .map_err(|e| e.to_string())?;

    let proxy = zbus::Proxy::new(&connection, RHEA_BUS_NAME, RHEA_PATH, RHEA_INTERFACE)
        .await
        .map_err(|e| e.to_string())?;

    // Empty system prompt -- Rhea supplies its own default.
    proxy
        .call_method("CompleteStream", &(prompt.as_str(), ""))
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Non-Linux stub: always returns an error so the UI can show a message.
#[cfg(not(target_os = "linux"))]
pub async fn call_rhea_complete_stream(_prompt: String) -> Result<(), String> {
    Err("Rhea D-Bus is only available on Linux".to_string())
}

// ---------------------------------------------------------------------------
// GetStatus
// ---------------------------------------------------------------------------

/// Call `org.slate.Rhea.GetStatus` over D-Bus and return the raw JSON string.
///
/// Used once on startup to pre-populate the backend label before any
/// `BackendChanged` signal arrives. Returns `{"backend":"...","ready":...}`.
#[cfg(target_os = "linux")]
pub async fn call_rhea_get_status() -> Result<String, String> {
    use slate_common::dbus::{RHEA_BUS_NAME, RHEA_INTERFACE, RHEA_PATH};

    let connection = zbus::Connection::session()
        .await
        .map_err(|e| e.to_string())?;

    let proxy = zbus::Proxy::new(&connection, RHEA_BUS_NAME, RHEA_PATH, RHEA_INTERFACE)
        .await
        .map_err(|e| e.to_string())?;

    proxy
        .call_method("GetStatus", &())
        .await
        .map_err(|e| e.to_string())?
        .body()
        .deserialize::<String>()
        .map_err(|e| e.to_string())
}

/// Non-Linux stub: returns an error so the backend label stays empty on macOS.
#[cfg(not(target_os = "linux"))]
pub async fn call_rhea_get_status() -> Result<String, String> {
    Err("Rhea D-Bus is only available on Linux".to_string())
}

// ---------------------------------------------------------------------------
// JSON helpers
// ---------------------------------------------------------------------------

/// Extract the `backend` string from a `{"backend":"...","ready":...}` JSON blob.
///
/// Uses a minimal hand-rolled parse to avoid pulling in `serde_json` as a
/// dependency of claw-panel (Rhea owns the JSON serialisation on the server
/// side; the panel only needs a single field).
pub fn parse_backend_from_status_json(json: &str) -> Option<String> {
    // Look for `"backend":"<value>"` in the JSON string.
    let key = "\"backend\":\"";
    let start = json.find(key)? + key.len();
    let end = json[start..].find('"')? + start;
    let backend = json[start..end].to_string();
    if backend.is_empty() {
        None
    } else {
        Some(backend)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_backend_extracts_local() {
        let json = r#"{"backend":"local","ready":true}"#;
        assert_eq!(
            parse_backend_from_status_json(json),
            Some("local".to_string())
        );
    }

    #[test]
    fn parse_backend_extracts_cloud() {
        let json = r#"{"backend":"cloud","ready":false}"#;
        assert_eq!(
            parse_backend_from_status_json(json),
            Some("cloud".to_string())
        );
    }

    #[test]
    fn parse_backend_empty_value_returns_none() {
        let json = r#"{"backend":"","ready":true}"#;
        assert!(parse_backend_from_status_json(json).is_none());
    }

    #[test]
    fn parse_backend_missing_key_returns_none() {
        let json = r#"{"ready":true}"#;
        assert!(parse_backend_from_status_json(json).is_none());
    }

    #[test]
    fn parse_backend_malformed_returns_none() {
        assert!(parse_backend_from_status_json("not json at all").is_none());
    }
}
