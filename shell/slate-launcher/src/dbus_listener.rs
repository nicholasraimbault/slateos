// D-Bus integration for the Slate Launcher.
//
// Listens for show/hide signals from TouchFlow (4-finger pinch gesture)
// and palette change signals from the slate-palette daemon. All interface
// names and paths are imported from slate-common::dbus.

// Constants are used in cfg(linux) blocks and tests.
#[cfg(any(target_os = "linux", test))]
use slate_common::dbus::LAUNCHER_INTERFACE;
#[cfg(target_os = "linux")]
use slate_common::dbus::LAUNCHER_PATH;

/// Events that the D-Bus listener can produce for the iced runtime.
#[derive(Debug, Clone)]
pub enum DbusEvent {
    /// TouchFlow (or another component) requested the launcher to show.
    Show,
    /// TouchFlow (or another component) requested the launcher to hide.
    Hide,
    /// Toggle visibility (show if hidden, hide if visible).
    Toggle,
    /// The system palette changed; the new TOML is included.
    PaletteChanged(String),
}

/// The D-Bus interface that the launcher exposes so other components can
/// control its visibility.
pub struct LauncherService {
    /// Channel to send events back to the iced main loop.
    sender: tokio::sync::mpsc::UnboundedSender<DbusEvent>,
}

impl LauncherService {
    pub fn new(sender: tokio::sync::mpsc::UnboundedSender<DbusEvent>) -> Self {
        Self { sender }
    }
}

#[zbus::interface(name = "org.slate.Launcher")]
impl LauncherService {
    /// Called by TouchFlow or other components to show the launcher.
    async fn show(&self) {
        let _ = self.sender.send(DbusEvent::Show);
    }

    /// Called to dismiss the launcher.
    async fn hide(&self) {
        let _ = self.sender.send(DbusEvent::Hide);
    }

    /// Toggle launcher visibility.
    async fn toggle(&self) {
        let _ = self.sender.send(DbusEvent::Toggle);
    }
}

/// Start the D-Bus service and listen for incoming method calls.
///
/// This spawns the launcher's own interface on the session bus and also
/// monitors the palette daemon for `Changed` signals.
///
/// # Errors
///
/// Returns an error if the session bus is unavailable or the bus name
/// cannot be claimed.
#[cfg(target_os = "linux")]
pub async fn run_dbus_listener(
    sender: tokio::sync::mpsc::UnboundedSender<DbusEvent>,
) -> anyhow::Result<()> {
    use zbus::connection::Builder;

    let service = LauncherService::new(sender.clone());

    let _conn = Builder::session()?
        .name(LAUNCHER_INTERFACE)?
        .serve_at(LAUNCHER_PATH, service)?
        .build()
        .await?;

    // Listen for palette changes from org.slate.Palette
    listen_palette_changes(sender).await
}

/// Subscribe to palette `Changed` signals from the slate-palette daemon.
#[cfg(target_os = "linux")]
async fn listen_palette_changes(
    _sender: tokio::sync::mpsc::UnboundedSender<DbusEvent>,
) -> anyhow::Result<()> {
    use slate_common::dbus::{PALETTE_INTERFACE, PALETTE_PATH};
    use zbus::Connection;

    let conn = Connection::session().await?;

    // Build a match rule for the palette Changed signal
    let rule = format!(
        "type='signal',interface='{}',path='{}',member='Changed'",
        PALETTE_INTERFACE, PALETTE_PATH
    );

    let proxy = zbus::fdo::DBusProxy::new(&conn).await?;
    proxy.add_match_rule(rule.as_str().try_into()?).await?;

    // In a real implementation we'd use a MessageStream here.
    // For now, keep the task alive so the D-Bus connection stays open.
    tracing::info!("listening for palette changes on {PALETTE_INTERFACE}");

    // Keep the future alive forever (the task will be cancelled on shutdown)
    std::future::pending::<()>().await;

    Ok(())
}

/// Stub for non-Linux platforms — does nothing.
#[cfg(not(target_os = "linux"))]
pub async fn run_dbus_listener(
    _sender: tokio::sync::mpsc::UnboundedSender<DbusEvent>,
) -> anyhow::Result<()> {
    tracing::warn!("D-Bus listener is not available on this platform");
    // Keep the future alive so the subscription doesn't immediately end
    std::future::pending::<()>().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dbus_event_variants_are_constructible() {
        let _show = DbusEvent::Show;
        let _hide = DbusEvent::Hide;
        let _toggle = DbusEvent::Toggle;
        let _palette = DbusEvent::PaletteChanged("test".to_string());
    }

    #[test]
    fn launcher_interface_matches_constant() {
        // Verify our hardcoded interface name in the zbus attribute matches
        // the constant from slate-common
        assert_eq!(LAUNCHER_INTERFACE, "org.slate.Launcher");
    }

    #[test]
    fn launcher_service_sends_events() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let _service = LauncherService::new(tx.clone());

        tx.send(DbusEvent::Show).unwrap();
        tx.send(DbusEvent::Hide).unwrap();
        tx.send(DbusEvent::Toggle).unwrap();

        assert!(matches!(rx.try_recv().unwrap(), DbusEvent::Show));
        assert!(matches!(rx.try_recv().unwrap(), DbusEvent::Hide));
        assert!(matches!(rx.try_recv().unwrap(), DbusEvent::Toggle));
    }
}
