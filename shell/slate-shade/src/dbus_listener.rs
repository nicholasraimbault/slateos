// D-Bus listener for slate-shade.
//
// Subscribes to signals from slate-notifyd (org.slate.Notifications),
// Rhea (org.slate.Rhea), TouchFlow (org.slate.TouchFlow), and the
// palette daemon (org.slate.Palette). On non-Linux platforms this module
// is a no-op stub that keeps the subscription future alive without
// connecting to any bus.

use slate_common::notifications::Notification;
use slate_common::Palette;
use uuid::Uuid;

#[cfg(target_os = "linux")]
use slate_common::dbus::{
    NOTIFICATIONS_BUS_NAME, NOTIFICATIONS_INTERFACE, NOTIFICATIONS_PATH, PALETTE_BUS_NAME,
    PALETTE_INTERFACE, PALETTE_PATH, RHEA_BUS_NAME, RHEA_INTERFACE, RHEA_PATH, TOUCHFLOW_INTERFACE,
    TOUCHFLOW_PATH,
};

#[cfg(all(test, not(target_os = "linux")))]
use slate_common::dbus::{NOTIFICATIONS_INTERFACE, NOTIFICATIONS_PATH};

// ---------------------------------------------------------------------------
// Event type
// ---------------------------------------------------------------------------

/// Events delivered by the D-Bus listener to the shade application.
#[derive(Debug, Clone)]
#[allow(dead_code)] // variants only constructed on Linux
pub enum DbusEvent {
    /// A new notification arrived.
    NotificationAdded(Notification),
    /// An existing notification was updated (e.g. marked read).
    NotificationUpdated(Notification),
    /// A notification was dismissed.
    NotificationDismissed(Uuid),
    /// A notification group's count changed (app_name, new_count).
    GroupChanged(String, u32),
    /// Rhea produced a summary for a group key.
    AiSummaryReady(String, String),
    /// Rhea produced smart-reply options for a notification.
    SmartRepliesReady(Uuid, Vec<String>),
    /// An edge gesture phase/progress/velocity update from TouchFlow.
    EdgeGesture {
        phase: String,
        progress: f64,
        velocity: f64,
    },
    /// The system palette changed.
    PaletteChanged(Palette),
}

// ---------------------------------------------------------------------------
// Linux D-Bus listener implementation
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
pub async fn run(event_tx: tokio::sync::mpsc::UnboundedSender<DbusEvent>) {
    let conn = match zbus::Connection::session().await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("slate-shade: failed to connect to session bus: {e}");
            return;
        }
    };

    // Spawn one task per signal source. If a source fails the others keep
    // running. We do not propagate errors out of tokio::spawn — the task logs
    // a warning and the subscription simply stops delivering events.
    let tx_palette = event_tx.clone();
    let conn_palette = conn.clone();
    tokio::spawn(async move {
        if let Err(e) = watch_palette(conn_palette, tx_palette).await {
            tracing::warn!("slate-shade: palette watcher ended: {e}");
        }
    });

    let tx_notif = event_tx.clone();
    let conn_notif = conn.clone();
    tokio::spawn(async move {
        if let Err(e) = watch_notifications(conn_notif, tx_notif).await {
            tracing::warn!("slate-shade: notification watcher ended: {e}");
        }
    });

    let tx_rhea = event_tx.clone();
    let conn_rhea = conn.clone();
    tokio::spawn(async move {
        if let Err(e) = watch_rhea(conn_rhea, tx_rhea).await {
            tracing::warn!("slate-shade: Rhea watcher ended: {e}");
        }
    });

    let tx_tf = event_tx;
    let conn_tf = conn;
    tokio::spawn(async move {
        if let Err(e) = watch_touchflow(conn_tf, tx_tf).await {
            tracing::warn!("slate-shade: TouchFlow watcher ended: {e}");
        }
    });

    // Keep the task alive so the connection is not dropped.
    std::future::pending::<()>().await;
}

/// Subscribe to org.slate.Palette Changed signal and initial value.
#[cfg(target_os = "linux")]
async fn watch_palette(
    conn: zbus::Connection,
    tx: tokio::sync::mpsc::UnboundedSender<DbusEvent>,
) -> anyhow::Result<()> {
    use futures_util::StreamExt;

    let proxy = zbus::Proxy::new(&conn, PALETTE_BUS_NAME, PALETTE_PATH, PALETTE_INTERFACE).await?;

    // Fetch the initial palette via the PaletteToml property.
    match proxy.get_property::<String>("PaletteToml").await {
        Ok(toml_str) => {
            if let Ok(palette) = toml::from_str::<Palette>(&toml_str) {
                let _ = tx.send(DbusEvent::PaletteChanged(palette));
            }
        }
        Err(e) => {
            tracing::warn!("slate-shade: could not fetch initial palette: {e}");
        }
    }

    let mut stream = proxy.receive_signal("Changed").await?;
    while let Some(signal) = stream.next().await {
        if let Ok(toml_str) = signal.body().deserialize::<String>() {
            if let Ok(palette) = toml::from_str::<Palette>(&toml_str) {
                let _ = tx.send(DbusEvent::PaletteChanged(palette));
            }
        }
    }

    Ok(())
}

/// Subscribe to org.slate.Notifications signals: Added, Updated, Dismissed, GroupChanged.
#[cfg(target_os = "linux")]
async fn watch_notifications(
    conn: zbus::Connection,
    tx: tokio::sync::mpsc::UnboundedSender<DbusEvent>,
) -> anyhow::Result<()> {
    use futures_util::StreamExt;

    let proxy = zbus::Proxy::new(
        &conn,
        NOTIFICATIONS_BUS_NAME,
        NOTIFICATIONS_PATH,
        NOTIFICATIONS_INTERFACE,
    )
    .await?;

    // Subscribe to all four signals concurrently using a select loop.
    let mut added = proxy.receive_signal("Added").await?;
    let mut updated = proxy.receive_signal("Updated").await?;
    let mut dismissed = proxy.receive_signal("Dismissed").await?;
    let mut group_changed = proxy.receive_signal("GroupChanged").await?;

    loop {
        tokio::select! {
            Some(signal) = added.next() => {
                // Body: (uuid: &str, notification_data: &str)
                if let Ok((_, data)) = signal.body().deserialize::<(String, String)>() {
                    if let Ok(notification) = toml::from_str::<Notification>(&data) {
                        let _ = tx.send(DbusEvent::NotificationAdded(notification));
                    }
                }
            }
            Some(signal) = updated.next() => {
                // Body: (uuid: &str, notification_data: &str)
                if let Ok((_, data)) = signal.body().deserialize::<(String, String)>() {
                    if let Ok(notification) = toml::from_str::<Notification>(&data) {
                        let _ = tx.send(DbusEvent::NotificationUpdated(notification));
                    }
                }
            }
            Some(signal) = dismissed.next() => {
                // Body: (uuid: &str, reason: &str)
                if let Ok((uuid_str, _)) = signal.body().deserialize::<(String, String)>() {
                    if let Ok(uuid) = Uuid::parse_str(&uuid_str) {
                        let _ = tx.send(DbusEvent::NotificationDismissed(uuid));
                    }
                }
            }
            Some(signal) = group_changed.next() => {
                // Body: (app_name: &str, count: u32)
                if let Ok((app_name, count)) = signal.body().deserialize::<(String, u32)>() {
                    let _ = tx.send(DbusEvent::GroupChanged(app_name, count));
                }
            }
            else => break,
        }
    }

    Ok(())
}

/// Subscribe to org.slate.Rhea signals: CompletionChunk, CompletionDone, CompletionError.
///
/// Rhea carries the notification group_key and generated text so the shade
/// can display AI summaries and smart-reply suggestions.
#[cfg(target_os = "linux")]
async fn watch_rhea(
    conn: zbus::Connection,
    tx: tokio::sync::mpsc::UnboundedSender<DbusEvent>,
) -> anyhow::Result<()> {
    use futures_util::StreamExt;

    let proxy = zbus::Proxy::new(&conn, RHEA_BUS_NAME, RHEA_PATH, RHEA_INTERFACE).await?;

    let mut summary = proxy.receive_signal("SummaryReady").await?;
    let mut replies = proxy.receive_signal("SmartRepliesReady").await?;

    loop {
        tokio::select! {
            Some(signal) = summary.next() => {
                // Body: (group_key: &str, summary: &str)
                if let Ok((group_key, text)) = signal.body().deserialize::<(String, String)>() {
                    let _ = tx.send(DbusEvent::AiSummaryReady(group_key, text));
                }
            }
            Some(signal) = replies.next() => {
                // Body: (uuid: &str, replies: Vec<String>)
                if let Ok((uuid_str, options)) =
                    signal.body().deserialize::<(String, Vec<String>)>()
                {
                    if let Ok(uuid) = Uuid::parse_str(&uuid_str) {
                        let _ = tx.send(DbusEvent::SmartRepliesReady(uuid, options));
                    }
                }
            }
            else => break,
        }
    }

    Ok(())
}

/// Subscribe to org.slate.TouchFlow EdgeGesture signals.
///
/// TouchFlow emits edge gesture events with a phase string ("begin", "update",
/// "end", "cancel"), a progress fraction [0,1], and velocity in px/s.
/// The shade uses these to drive its pull-down animation.
#[cfg(target_os = "linux")]
async fn watch_touchflow(
    conn: zbus::Connection,
    tx: tokio::sync::mpsc::UnboundedSender<DbusEvent>,
) -> anyhow::Result<()> {
    use futures_util::StreamExt;

    let proxy = zbus::Proxy::new(
        &conn,
        "org.slate.TouchFlow",
        TOUCHFLOW_PATH,
        TOUCHFLOW_INTERFACE,
    )
    .await?;

    let mut edge_gesture = proxy.receive_signal("EdgeGesture").await?;

    while let Some(signal) = edge_gesture.next().await {
        // Body: (phase: &str, progress: f64, velocity: f64)
        if let Ok((phase, progress, velocity)) = signal.body().deserialize::<(String, f64, f64)>() {
            let _ = tx.send(DbusEvent::EdgeGesture {
                phase,
                progress,
                velocity,
            });
        }
    }

    Ok(())
}

/// Non-Linux stub: keeps the subscription alive without doing anything.
#[cfg(not(target_os = "linux"))]
pub async fn run(_event_tx: tokio::sync::mpsc::UnboundedSender<DbusEvent>) {
    tracing::warn!("slate-shade: D-Bus listener is not available on this platform");
    std::future::pending::<()>().await;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dbus_event_debug_and_clone() {
        let event = DbusEvent::PaletteChanged(Palette::default());
        let _cloned = event.clone();
        let _debug = format!("{event:?}");
    }

    #[test]
    fn dbus_event_notification_added_constructible() {
        let n = Notification::new(1, "TestApp", "Hello", "World");
        let event = DbusEvent::NotificationAdded(n);
        let _cloned = event.clone();
    }

    #[test]
    fn dbus_event_notification_dismissed_constructible() {
        let uuid = Uuid::new_v4();
        let event = DbusEvent::NotificationDismissed(uuid);
        assert!(matches!(event, DbusEvent::NotificationDismissed(_)));
    }

    #[test]
    fn dbus_event_group_changed_constructible() {
        let event = DbusEvent::GroupChanged("App".to_string(), 3);
        assert!(matches!(event, DbusEvent::GroupChanged(_, _)));
    }

    #[test]
    fn dbus_event_edge_gesture_constructible() {
        let event = DbusEvent::EdgeGesture {
            phase: "begin".to_string(),
            progress: 0.5,
            velocity: 200.0,
        };
        if let DbusEvent::EdgeGesture {
            phase,
            progress,
            velocity,
        } = event
        {
            assert_eq!(phase, "begin");
            assert!((progress - 0.5).abs() < 1e-9);
            assert!((velocity - 200.0).abs() < 1e-9);
        } else {
            panic!("expected EdgeGesture");
        }
    }

    #[test]
    fn dbus_event_ai_summary_constructible() {
        let event = DbusEvent::AiSummaryReady("group-key".to_string(), "summary text".to_string());
        assert!(matches!(event, DbusEvent::AiSummaryReady(_, _)));
    }

    #[test]
    fn dbus_event_smart_replies_constructible() {
        let uuid = Uuid::new_v4();
        let event = DbusEvent::SmartRepliesReady(uuid, vec!["Yes".to_string(), "No".to_string()]);
        if let DbusEvent::SmartRepliesReady(_, replies) = event {
            assert_eq!(replies.len(), 2);
        } else {
            panic!("expected SmartRepliesReady");
        }
    }

    #[cfg(any(target_os = "linux", test))]
    #[test]
    fn notifications_path_constant_is_correct() {
        assert_eq!(NOTIFICATIONS_PATH, "/org/slate/Notifications");
        assert_eq!(NOTIFICATIONS_INTERFACE, "org.slate.Notifications");
    }
}
