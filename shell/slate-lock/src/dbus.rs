// D-Bus server and listener for slate-lock.
//
// slate-lock SERVES the `org.slate.LockScreen` interface so that external
// callers (slate-power, swayidle) can invoke Lock() to trigger the lock
// screen. The module also subscribes to:
//
//   - org.slate.Notifications.Added — to surface incoming notifications while
//     the screen is locked.
//   - org.slate.Palette.Changed    — to keep the lock screen visually in sync
//     with dynamic theming.
//
// On non-Linux platforms the entire module is a no-op stub that holds the
// subscription future alive without touching any bus.

use tokio::sync::mpsc::UnboundedSender;

#[cfg(target_os = "linux")]
use slate_common::dbus::{
    LOCKSCREEN_BUS_NAME, LOCKSCREEN_PATH, NOTIFICATIONS_BUS_NAME, NOTIFICATIONS_INTERFACE,
    NOTIFICATIONS_PATH, PALETTE_BUS_NAME, PALETTE_INTERFACE, PALETTE_PATH,
};

// ---------------------------------------------------------------------------
// Event type
// ---------------------------------------------------------------------------

/// Events delivered by the D-Bus layer to the lock-screen application.
#[derive(Debug, Clone)]
#[allow(dead_code)] // variants only constructed on Linux
pub enum LockDbusEvent {
    /// An external caller requested the screen be locked.
    LockRequested,
    /// A new notification arrived (TOML-serialised `Notification`).
    NotificationAdded(String),
    /// The system palette changed.
    PaletteChanged(slate_common::Palette),
}

// ---------------------------------------------------------------------------
// D-Bus service (server side)
// ---------------------------------------------------------------------------

/// Serves `org.slate.LockScreen` so that slate-power and swayidle can call
/// `Lock()` to engage the lock screen.
struct LockScreenService {
    sender: UnboundedSender<LockDbusEvent>,
    is_locked: bool,
}

#[zbus::interface(name = "org.slate.LockScreen")]
impl LockScreenService {
    /// Called by external parties (slate-power, swayidle) to lock the screen.
    async fn lock(&mut self) {
        let _ = self.sender.send(LockDbusEvent::LockRequested);
        self.is_locked = true;
    }

    /// Whether the screen is currently locked.
    #[zbus(property)]
    fn is_locked(&self) -> bool {
        self.is_locked
    }

    /// Emitted after the screen has been successfully locked.
    #[zbus(signal)]
    async fn locked(emitter: &zbus::object_server::SignalEmitter<'_>) -> zbus::Result<()>;

    /// Emitted after the screen has been unlocked (e.g. PIN/biometric success).
    #[zbus(signal)]
    async fn unlocked(emitter: &zbus::object_server::SignalEmitter<'_>) -> zbus::Result<()>;
}

// ---------------------------------------------------------------------------
// iced subscription entry point
// ---------------------------------------------------------------------------

/// Returns an iced `Subscription` that drives all D-Bus activity for
/// slate-lock. Uses the `iced::stream::channel` / `Subscription::run_with_id`
/// pattern consistent with the rest of the Slate shell.
pub fn dbus_subscription() -> iced::Subscription<LockDbusEvent> {
    use iced::futures::SinkExt;

    let stream = iced::stream::channel(32, |mut output| async move {
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();

        #[cfg(target_os = "linux")]
        tokio::spawn(run(event_tx));

        #[cfg(not(target_os = "linux"))]
        drop(event_tx);

        loop {
            match event_rx.recv().await {
                Some(event) => {
                    if output.send(event).await.is_err() {
                        tracing::warn!("slate-lock: D-Bus subscription channel closed");
                        break;
                    }
                }
                None => {
                    tracing::warn!("slate-lock: D-Bus event channel closed unexpectedly");
                    std::future::pending::<()>().await;
                }
            }
        }
    });

    iced::Subscription::run_with_id("slate-lock-dbus", stream)
}

// ---------------------------------------------------------------------------
// Linux implementation
// ---------------------------------------------------------------------------

/// Connect to the session bus, register the LockScreen service, then listen
/// for notification and palette signals concurrently.
#[cfg(target_os = "linux")]
pub async fn run(event_tx: tokio::sync::mpsc::UnboundedSender<LockDbusEvent>) {
    let conn = match zbus::Connection::session().await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("slate-lock: failed to connect to session bus: {e}");
            return;
        }
    };

    // Register the LockScreen service so callers can invoke Lock().
    let service = LockScreenService {
        sender: event_tx.clone(),
        is_locked: false,
    };

    if let Err(e) = conn.object_server().at(LOCKSCREEN_PATH, service).await {
        tracing::error!("slate-lock: failed to register LockScreen object: {e}");
        return;
    }

    // Request the well-known bus name so callers can find us.
    if let Err(e) = conn.request_name(LOCKSCREEN_BUS_NAME).await {
        tracing::error!("slate-lock: failed to request bus name {LOCKSCREEN_BUS_NAME}: {e}");
        return;
    }

    tracing::info!("slate-lock: D-Bus service registered at {LOCKSCREEN_PATH}");

    // Spawn individual watchers for signals we subscribe to. Each watcher runs
    // independently; if one fails the others keep going.
    let tx_palette = event_tx.clone();
    let conn_palette = conn.clone();
    tokio::spawn(async move {
        if let Err(e) = watch_palette(conn_palette, tx_palette).await {
            tracing::warn!("slate-lock: palette watcher ended: {e}");
        }
    });

    let tx_notif = event_tx;
    let conn_notif = conn;
    tokio::spawn(async move {
        if let Err(e) = watch_notifications(conn_notif, tx_notif).await {
            tracing::warn!("slate-lock: notification watcher ended: {e}");
        }
    });

    // Keep the task alive so the connection (and registered object) are not dropped.
    std::future::pending::<()>().await;
}

/// Subscribe to `org.slate.Palette.Changed` and deliver the initial palette value.
#[cfg(target_os = "linux")]
async fn watch_palette(
    conn: zbus::Connection,
    tx: tokio::sync::mpsc::UnboundedSender<LockDbusEvent>,
) -> anyhow::Result<()> {
    use futures_util::StreamExt;

    let proxy = zbus::Proxy::new(&conn, PALETTE_BUS_NAME, PALETTE_PATH, PALETTE_INTERFACE).await?;

    // Fetch the initial palette so the lock screen has the right colours on
    // first paint rather than waiting for the next change signal.
    match proxy.get_property::<String>("PaletteToml").await {
        Ok(toml_str) => {
            if let Ok(palette) = toml::from_str::<slate_common::Palette>(&toml_str) {
                let _ = tx.send(LockDbusEvent::PaletteChanged(palette));
            }
        }
        Err(e) => {
            tracing::warn!("slate-lock: could not fetch initial palette: {e}");
        }
    }

    let mut stream = proxy.receive_signal("Changed").await?;
    while let Some(signal) = stream.next().await {
        if let Ok(toml_str) = signal.body().deserialize::<String>() {
            if let Ok(palette) = toml::from_str::<slate_common::Palette>(&toml_str) {
                let _ = tx.send(LockDbusEvent::PaletteChanged(palette));
            }
        }
    }

    Ok(())
}

/// Subscribe to `org.slate.Notifications.Added` so new notifications can be
/// surfaced on the lock screen without being missed.
#[cfg(target_os = "linux")]
async fn watch_notifications(
    conn: zbus::Connection,
    tx: tokio::sync::mpsc::UnboundedSender<LockDbusEvent>,
) -> anyhow::Result<()> {
    use futures_util::StreamExt;

    let proxy = zbus::Proxy::new(
        &conn,
        NOTIFICATIONS_BUS_NAME,
        NOTIFICATIONS_PATH,
        NOTIFICATIONS_INTERFACE,
    )
    .await?;

    let mut added = proxy.receive_signal("Added").await?;

    while let Some(signal) = added.next().await {
        // Body: (uuid: &str, notification_data: &str)
        if let Ok((_, data)) = signal.body().deserialize::<(String, String)>() {
            let _ = tx.send(LockDbusEvent::NotificationAdded(data));
        }
    }

    Ok(())
}

/// Non-Linux stub: keeps the subscription alive without touching any bus.
#[cfg(not(target_os = "linux"))]
pub async fn run(_event_tx: tokio::sync::mpsc::UnboundedSender<LockDbusEvent>) {
    tracing::warn!("slate-lock: D-Bus listener is not available on this platform");
    std::future::pending::<()>().await;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_dbus_event_variants_constructible() {
        // LockRequested
        let e = LockDbusEvent::LockRequested;
        let _cloned = e.clone();
        let _debug = format!("{e:?}");

        // NotificationAdded
        let e2 = LockDbusEvent::NotificationAdded("some toml".to_string());
        let _cloned2 = e2.clone();
        assert!(matches!(e2, LockDbusEvent::NotificationAdded(_)));

        // PaletteChanged
        let e3 = LockDbusEvent::PaletteChanged(slate_common::Palette::default());
        let _cloned3 = e3.clone();
        assert!(matches!(e3, LockDbusEvent::PaletteChanged(_)));
    }

    #[test]
    fn lock_dbus_event_debug_format() {
        let e = LockDbusEvent::LockRequested;
        assert!(format!("{e:?}").contains("LockRequested"));
    }
}
