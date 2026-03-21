/// D-Bus listener for the suggestion bar.
///
/// Subscribes to palette change signals from slate-palette and show/hide
/// signals from TouchFlow so the bar can re-theme and toggle visibility.
/// Follows the same subscription pattern used in shoal's dbus_listener.
use slate_common::Palette;

// D-Bus constants are only used on Linux where we connect to the session bus.
#[cfg(target_os = "linux")]
use slate_common::dbus::{PALETTE_BUS_NAME, PALETTE_PATH, SUGGEST_INTERFACE, SUGGEST_PATH};

/// Events produced by the D-Bus listener for the suggestion bar to process.
#[derive(Debug, Clone)]
pub enum SuggestDbusEvent {
    /// The system palette changed; bar should re-theme.
    PaletteChanged(Palette),
    /// The bar should become visible (e.g. keyboard appeared).
    Show,
    /// The bar should hide (e.g. keyboard dismissed).
    Hide,
    /// Focused window's input context changed (partial command text).
    InputContext(String),
}

/// Subscribe to palette-changed signals on the session bus.
///
/// Connects to the session bus, listens for palette change signals from
/// slate-palette, and forwards parsed palettes to the iced app via the
/// provided sender channel.
#[cfg(target_os = "linux")]
pub async fn listen_palette(
    sender: tokio::sync::mpsc::UnboundedSender<SuggestDbusEvent>,
) -> anyhow::Result<()> {
    use zbus::Connection;

    let connection = Connection::session().await?;

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
                tracing::warn!("D-Bus palette message error: {e}");
                continue;
            }
        };
        if let Ok(toml_str) = msg.body().deserialize::<String>() {
            match toml::from_str::<Palette>(&toml_str) {
                Ok(palette) => {
                    let _ = sender.send(SuggestDbusEvent::PaletteChanged(palette));
                }
                Err(e) => {
                    tracing::warn!("failed to parse palette TOML from D-Bus: {e}");
                }
            }
        }
    }

    Ok(())
}

/// Subscribe to show/hide signals for the suggestion bar.
///
/// Listens for Show/Hide signals on the suggest bar's D-Bus interface.
/// TouchFlow or the keyboard daemon emits these when the on-screen
/// keyboard appears or disappears.
#[cfg(target_os = "linux")]
pub async fn listen_visibility_signals(
    sender: tokio::sync::mpsc::UnboundedSender<SuggestDbusEvent>,
) -> anyhow::Result<()> {
    use zbus::Connection;

    let connection = Connection::session().await?;

    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .path(SUGGEST_PATH)?
        .interface(SUGGEST_INTERFACE)?
        .build();

    let mut stream = zbus::MessageStream::for_match_rule(rule, &connection, None).await?;

    use iced::futures::StreamExt;
    while let Some(msg_result) = stream.next().await {
        let msg = match msg_result {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("D-Bus suggest message error: {e}");
                continue;
            }
        };
        if let Some(member) = msg.header().member() {
            match member.as_str() {
                "Show" => {
                    let _ = sender.send(SuggestDbusEvent::Show);
                }
                "Hide" => {
                    let _ = sender.send(SuggestDbusEvent::Hide);
                }
                "InputContext" => {
                    // The signal carries the current input text as a string argument.
                    if let Ok(text) = msg.body().deserialize::<String>() {
                        let _ = sender.send(SuggestDbusEvent::InputContext(text));
                    }
                }
                other => {
                    tracing::debug!("unknown suggest signal: {other}");
                }
            }
        }
    }

    Ok(())
}

/// Try to load the current palette from the running palette daemon.
#[cfg(target_os = "linux")]
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
    fn suggest_dbus_event_debug_format() {
        let event = SuggestDbusEvent::Show;
        let _ = format!("{event:?}");

        let event = SuggestDbusEvent::Hide;
        let _ = format!("{event:?}");

        let event = SuggestDbusEvent::PaletteChanged(Palette::default());
        let _ = format!("{event:?}");

        let event = SuggestDbusEvent::InputContext("git st".to_string());
        let _ = format!("{event:?}");
    }

    #[test]
    fn suggest_dbus_event_clone() {
        let event = SuggestDbusEvent::InputContext("cargo build".to_string());
        let cloned = event.clone();
        if let SuggestDbusEvent::InputContext(text) = cloned {
            assert_eq!(text, "cargo build");
        } else {
            panic!("expected InputContext variant");
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn dbus_constants_are_consistent() {
        use slate_common::dbus::{SUGGEST_INTERFACE, SUGGEST_PATH};
        assert_eq!(SUGGEST_INTERFACE, "org.slate.Suggest");
        assert_eq!(SUGGEST_PATH, "/org/slate/Suggest");
    }
}
