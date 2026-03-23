/// D-Bus listener for Shoal.
///
/// Subscribes to palette change signals from slate-palette, show/hide
/// signals from TouchFlow, and notification count signals from slate-notifyd
/// so the dock can re-theme, toggle visibility, and show badge counts.
use slate_common::Palette;

// D-Bus constants are only used on Linux where we connect to the session bus.
#[cfg(target_os = "linux")]
use slate_common::dbus::{
    DOCK_INTERFACE, DOCK_PATH, NOTIFICATIONS_BUS_NAME, NOTIFICATIONS_INTERFACE, NOTIFICATIONS_PATH,
    PALETTE_BUS_NAME, PALETTE_PATH,
};

/// Events produced by the D-Bus listener for the dock to process.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Used at runtime on Linux via D-Bus listeners
pub enum DockDbusEvent {
    /// The system palette changed; dock should re-theme.
    PaletteChanged(Palette),
    /// TouchFlow requested the dock to show.
    Show,
    /// TouchFlow requested the dock to hide.
    Hide,
    /// The notification count for an app changed (app_name, count).
    NotificationCountChanged(String, u32),
}

/// Subscribe to palette-changed signals and return palette updates.
///
/// This connects to the session bus, subscribes to property changes on the
/// palette service, and yields events when the palette TOML changes.
#[cfg(target_os = "linux")]
pub async fn listen_palette(
    sender: tokio::sync::mpsc::UnboundedSender<DockDbusEvent>,
) -> anyhow::Result<()> {
    use zbus::Connection;

    let connection = Connection::session().await?;

    // Subscribe to the PaletteChanged signal
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(PALETTE_BUS_NAME)?
        .path(PALETTE_PATH)?
        .interface("org.slate.Palette")?
        .member("Changed")?
        .build();

    let mut stream = zbus::MessageStream::for_match_rule(rule, &connection, None).await?;

    use iced::futures::StreamExt;
    while let Some(msg_result) = stream.next().await {
        let msg = match msg_result {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("D-Bus message error: {e}");
                continue;
            }
        };
        if let Ok(toml_str) = msg.body().deserialize::<String>() {
            match toml::from_str::<Palette>(&toml_str) {
                Ok(palette) => {
                    let _ = sender.send(DockDbusEvent::PaletteChanged(palette));
                }
                Err(e) => {
                    tracing::warn!("failed to parse palette TOML from D-Bus: {e}");
                }
            }
        }
    }

    Ok(())
}

/// Subscribe to dock show/hide signals from TouchFlow.
#[cfg(target_os = "linux")]
pub async fn listen_dock_signals(
    sender: tokio::sync::mpsc::UnboundedSender<DockDbusEvent>,
) -> anyhow::Result<()> {
    use zbus::Connection;

    let connection = Connection::session().await?;

    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .path(DOCK_PATH)?
        .interface(DOCK_INTERFACE)?
        .build();

    let mut stream = zbus::MessageStream::for_match_rule(rule, &connection, None).await?;

    use iced::futures::StreamExt;
    while let Some(msg_result) = stream.next().await {
        let msg = match msg_result {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("D-Bus message error: {e}");
                continue;
            }
        };
        if let Some(member) = msg.header().member() {
            match member.as_str() {
                "Show" => {
                    let _ = sender.send(DockDbusEvent::Show);
                }
                "Hide" => {
                    let _ = sender.send(DockDbusEvent::Hide);
                }
                other => {
                    tracing::debug!("unknown dock signal: {other}");
                }
            }
        }
    }

    Ok(())
}

/// Subscribe to GroupChanged signals from slate-notifyd and send count updates.
///
/// slate-notifyd emits GroupChanged(app_name, count) whenever a notification is
/// added, dismissed, or cleared for an app. The dock uses these to update badges.
#[cfg(target_os = "linux")]
pub async fn listen_notification_counts(
    sender: tokio::sync::mpsc::UnboundedSender<DockDbusEvent>,
) -> anyhow::Result<()> {
    use zbus::Connection;

    let connection = Connection::session().await?;

    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(NOTIFICATIONS_BUS_NAME)?
        .path(NOTIFICATIONS_PATH)?
        .interface(NOTIFICATIONS_INTERFACE)?
        .member("GroupChanged")?
        .build();

    let mut stream = zbus::MessageStream::for_match_rule(rule, &connection, None).await?;

    use iced::futures::StreamExt;
    while let Some(msg_result) = stream.next().await {
        let msg = match msg_result {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("D-Bus notification count message error: {e}");
                continue;
            }
        };
        if let Ok((app_name, count)) = msg.body().deserialize::<(String, u32)>() {
            let _ = sender.send(DockDbusEvent::NotificationCountChanged(app_name, count));
        }
    }

    Ok(())
}

/// Try to load the current palette from the running palette daemon.
#[cfg(target_os = "linux")]
#[allow(dead_code)] // Will be used once palette prefetch is wired in
pub async fn fetch_current_palette() -> anyhow::Result<Palette> {
    use zbus::Connection;

    let connection = Connection::session().await?;
    let proxy = zbus::Proxy::new(
        &connection,
        PALETTE_BUS_NAME,
        PALETTE_PATH,
        "org.slate.Palette",
    )
    .await?;

    let toml_str: String = proxy.get_property("PaletteToml").await?;
    let palette: Palette = toml::from_str(&toml_str)?;
    Ok(palette)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dock_dbus_event_debug_format() {
        // Ensure the event enum is Debug-printable (compile-time check)
        let event = DockDbusEvent::Show;
        let _ = format!("{event:?}");

        let event = DockDbusEvent::Hide;
        let _ = format!("{event:?}");

        let event = DockDbusEvent::PaletteChanged(Palette::default());
        let _ = format!("{event:?}");
    }

    #[test]
    fn dock_dbus_event_notification_count_changed_constructible() {
        let event = DockDbusEvent::NotificationCountChanged("slack".to_string(), 5);
        let _ = format!("{event:?}");
        if let DockDbusEvent::NotificationCountChanged(app, count) = event {
            assert_eq!(app, "slack");
            assert_eq!(count, 5);
        } else {
            panic!("expected NotificationCountChanged");
        }
    }
}
