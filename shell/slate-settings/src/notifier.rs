/// D-Bus notifier for broadcasting settings changes.
///
/// When the user changes a setting, the notifier emits a signal on the
/// session bus so all Slate OS components can reload their configuration
/// without restarting.
use slate_common::dbus::{SETTINGS_INTERFACE, SETTINGS_PATH};

// ---------------------------------------------------------------------------
// D-Bus service
// ---------------------------------------------------------------------------

/// D-Bus object that emits `Changed(section)` signals.
pub struct SettingsNotifier;

#[zbus::interface(name = "org.slate.Settings")]
impl SettingsNotifier {
    /// Signal emitted when a settings section changes.
    /// `section` is one of: "display", "wallpaper", "dock", "gestures",
    /// "keyboard", "ai".
    #[zbus(signal)]
    pub async fn changed(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        section: &str,
    ) -> zbus::Result<()>;
}

// ---------------------------------------------------------------------------
// Helper to emit a change signal
// ---------------------------------------------------------------------------

/// Emit a settings-changed signal on the session bus.
///
/// This is a best-effort operation: if the D-Bus connection is not
/// available (e.g. in tests or on macOS), the error is logged and
/// swallowed.
pub async fn emit_changed(section: &str) {
    match try_emit(section).await {
        Ok(()) => tracing::info!("emitted settings change for {section}"),
        Err(e) => tracing::warn!("failed to emit D-Bus signal for {section}: {e}"),
    }
}

async fn try_emit(section: &str) -> zbus::Result<()> {
    let conn = zbus::Connection::session().await?;
    conn.object_server()
        .at(SETTINGS_PATH, SettingsNotifier)
        .await?;
    conn.request_name(SETTINGS_INTERFACE).await?;

    let iface_ref = conn
        .object_server()
        .interface::<_, SettingsNotifier>(SETTINGS_PATH)
        .await?;
    SettingsNotifier::changed(iface_ref.signal_emitter(), section).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interface_constants_are_consistent() {
        // The zbus interface name must match our constant
        assert_eq!(SETTINGS_INTERFACE, "org.slate.Settings");
        assert_eq!(SETTINGS_PATH, "/org/slate/Settings");
    }
}
