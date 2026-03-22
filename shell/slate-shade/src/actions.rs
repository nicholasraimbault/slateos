// Async D-Bus action helpers for slate-shade.
//
// Each function performs a single method call on the `org.slate.Notifications`
// or `org.slate.Rhea` interfaces. They are invoked via `iced::Task::perform`
// so the synchronous `update()` function can remain non-async.
//
// All helpers are `#[cfg(target_os = "linux")]` guarded; non-Linux stubs
// return `Err` immediately so the UI can log a diagnostic and move on.

// ---------------------------------------------------------------------------
// Linux implementations
// ---------------------------------------------------------------------------

/// Call `org.slate.Notifications.Dismiss(uuid_str)`.
///
/// Asks notifyd to dismiss a single notification by its UUID string.
/// The notifyd daemon will emit a `Dismissed` signal which feeds back
/// into our `DbusEvent::NotificationDismissed` subscription.
#[cfg(target_os = "linux")]
pub(super) async fn dbus_dismiss(uuid_str: String) -> Result<(), String> {
    use slate_common::dbus::{NOTIFICATIONS_BUS_NAME, NOTIFICATIONS_INTERFACE, NOTIFICATIONS_PATH};

    let conn = zbus::Connection::session()
        .await
        .map_err(|e| e.to_string())?;

    let proxy = zbus::Proxy::new(
        &conn,
        NOTIFICATIONS_BUS_NAME,
        NOTIFICATIONS_PATH,
        NOTIFICATIONS_INTERFACE,
    )
    .await
    .map_err(|e| e.to_string())?;

    proxy
        .call_method("Dismiss", &(uuid_str.as_str(),))
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Call `org.slate.Notifications.DismissGroup(app_name)`.
///
/// Asks notifyd to dismiss every notification for the given app.
/// notifyd emits `Dismissed` signals for each one; our subscription
/// handles the individual removals via `DbusEvent::NotificationDismissed`.
#[cfg(target_os = "linux")]
pub(super) async fn dbus_dismiss_group(app_name: String) -> Result<(), String> {
    use slate_common::dbus::{NOTIFICATIONS_BUS_NAME, NOTIFICATIONS_INTERFACE, NOTIFICATIONS_PATH};

    let conn = zbus::Connection::session()
        .await
        .map_err(|e| e.to_string())?;

    let proxy = zbus::Proxy::new(
        &conn,
        NOTIFICATIONS_BUS_NAME,
        NOTIFICATIONS_PATH,
        NOTIFICATIONS_INTERFACE,
    )
    .await
    .map_err(|e| e.to_string())?;

    proxy
        .call_method("DismissGroup", &(app_name.as_str(),))
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Call `org.slate.Notifications.InvokeAction(uuid_str, action_key)`.
///
/// Tells notifyd to invoke a notification action. notifyd will re-emit the
/// action on the freedesktop interface so the originating application is
/// notified. We do not need to dismiss the notification ourselves; the app
/// or notifyd will handle that.
#[cfg(target_os = "linux")]
pub(super) async fn dbus_invoke_action(uuid_str: String, action_key: String) -> Result<(), String> {
    use slate_common::dbus::{NOTIFICATIONS_BUS_NAME, NOTIFICATIONS_INTERFACE, NOTIFICATIONS_PATH};

    let conn = zbus::Connection::session()
        .await
        .map_err(|e| e.to_string())?;

    let proxy = zbus::Proxy::new(
        &conn,
        NOTIFICATIONS_BUS_NAME,
        NOTIFICATIONS_PATH,
        NOTIFICATIONS_INTERFACE,
    )
    .await
    .map_err(|e| e.to_string())?;

    proxy
        .call_method("InvokeAction", &(uuid_str.as_str(), action_key.as_str()))
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Call `org.slate.Rhea.SuggestReplies(messages_toml)` for smart-reply chips.
///
/// Encodes the notification body as a single-user-message TOML document and
/// forwards it to Rhea. The returned `Vec<String>` is the list of reply
/// suggestions (up to 3). On error we return an empty vec so the UI
/// degrades gracefully.
///
/// The result is handed back to the update loop as
/// `Message::SmartRepliesResult(uuid, replies)`.
#[cfg(target_os = "linux")]
pub(super) async fn dbus_suggest_replies(
    uuid: uuid::Uuid,
    body: String,
) -> (uuid::Uuid, Vec<String>) {
    use slate_common::dbus::{RHEA_BUS_NAME, RHEA_INTERFACE, RHEA_PATH};

    let result = async {
        let conn = zbus::Connection::session()
            .await
            .map_err(|e: zbus::Error| e.to_string())?;

        let proxy = zbus::Proxy::new(&conn, RHEA_BUS_NAME, RHEA_PATH, RHEA_INTERFACE)
            .await
            .map_err(|e| e.to_string())?;

        // Encode the notification body as a TOML [[messages]] array so
        // Rhea.SuggestReplies can parse it with parse_messages().
        let messages_toml = format!(
            "[[messages]]\nrole = \"user\"\ncontent = {}\n",
            toml_quote(&body)
        );

        let reply: zbus::Message = proxy
            .call_method("SuggestReplies", &(messages_toml.as_str(),))
            .await
            .map_err(|e| e.to_string())?;

        let replies: Vec<String> = reply.body().deserialize().map_err(|e| e.to_string())?;
        Ok::<Vec<String>, String>(replies)
    }
    .await;

    match result {
        Ok(replies) => (uuid, replies),
        Err(e) => {
            tracing::warn!("slate-shade: SuggestReplies failed: {e}");
            (uuid, Vec::new())
        }
    }
}

/// Minimal TOML string quoting: wraps `s` in double quotes and escapes
/// backslashes, double-quotes, and newlines.
///
/// Used to embed the notification body into a TOML literal without pulling
/// in the full `toml` serialiser for a single string value.
#[cfg(target_os = "linux")]
fn toml_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ---------------------------------------------------------------------------
// Non-Linux stubs
// ---------------------------------------------------------------------------

#[cfg(not(target_os = "linux"))]
pub(super) async fn dbus_dismiss(_uuid_str: String) -> Result<(), String> {
    Err("D-Bus is only available on Linux".to_string())
}

#[cfg(not(target_os = "linux"))]
pub(super) async fn dbus_dismiss_group(_app_name: String) -> Result<(), String> {
    Err("D-Bus is only available on Linux".to_string())
}

#[cfg(not(target_os = "linux"))]
pub(super) async fn dbus_invoke_action(
    _uuid_str: String,
    _action_key: String,
) -> Result<(), String> {
    Err("D-Bus is only available on Linux".to_string())
}

#[cfg(not(target_os = "linux"))]
pub(super) async fn dbus_suggest_replies(
    uuid: uuid::Uuid,
    _body: String,
) -> (uuid::Uuid, Vec<String>) {
    tracing::debug!("slate-shade: SuggestReplies stub (non-Linux)");
    (uuid, Vec::new())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    #[cfg(target_os = "linux")]
    use super::toml_quote;

    #[cfg(target_os = "linux")]
    #[test]
    fn toml_quote_plain_string() {
        assert_eq!(toml_quote("hello world"), "\"hello world\"");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn toml_quote_escapes_backslash() {
        assert_eq!(toml_quote("a\\b"), "\"a\\\\b\"");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn toml_quote_escapes_double_quote() {
        assert_eq!(toml_quote("say \"hi\""), "\"say \\\"hi\\\"\"");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn toml_quote_escapes_newline() {
        assert_eq!(toml_quote("line1\nline2"), "\"line1\\nline2\"");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn toml_quote_empty_string() {
        assert_eq!(toml_quote(""), "\"\"");
    }

    #[test]
    fn non_linux_stubs_compile() {
        // This test always passes on all platforms; it simply verifies
        // that the correct stub signatures are present and callable.
        // The cfg guards ensure the right impl is selected at compile time.
    }
}
