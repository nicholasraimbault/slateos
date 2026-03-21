/// D-Bus listener for palette change signals.
///
/// Subscribes to `org.slate.Palette.Changed` signals emitted by the
/// slate-palette daemon. When a new palette arrives, it is deserialised
/// and forwarded to the iced app via a channel.
use anyhow::{Context, Result};
use slate_common::dbus::{PALETTE_BUS_NAME, PALETTE_INTERFACE, PALETTE_PATH};
use slate_common::Palette;
use tokio::sync::mpsc;
use zbus::Connection;

/// Fetch the current palette from the D-Bus palette service property.
///
/// Returns `None` if the service is not running or the property is unreadable.
pub async fn fetch_current_palette(connection: &Connection) -> Option<Palette> {
    let proxy = zbus::fdo::PropertiesProxy::builder(connection)
        .destination(PALETTE_BUS_NAME)
        .ok()?
        .path(PALETTE_PATH)
        .ok()?
        .build()
        .await
        .ok()?;

    let iface_name = zbus::names::InterfaceName::try_from(PALETTE_INTERFACE).ok()?;
    let value = proxy.get(iface_name, "PaletteToml").await.ok()?;

    let toml_str: String = value.try_into().ok()?;
    toml::from_str(&toml_str).ok()
}

/// Start listening for palette change signals on the session bus.
///
/// Returns a receiver channel that yields new `Palette` values whenever
/// the palette daemon broadcasts a change. The listener runs in a
/// background tokio task.
pub async fn listen_for_palette_changes(connection: Connection) -> Result<mpsc::Receiver<Palette>> {
    let (tx, rx) = mpsc::channel(4);

    // Build a match rule for the palette changed signal.
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface(PALETTE_INTERFACE)
        .context("invalid interface name")?
        .member("changed")
        .context("invalid member name")?
        .path(PALETTE_PATH)
        .context("invalid path")?
        .build();

    let proxy = zbus::fdo::DBusProxy::new(&connection)
        .await
        .context("create DBusProxy")?;

    proxy
        .add_match_rule(rule)
        .await
        .context("add match rule for palette changed signal")?;

    tokio::spawn(async move {
        use futures_util::StreamExt;

        let mut stream = zbus::MessageStream::from(&connection);
        while let Some(Ok(msg)) = stream.next().await {
            let header = msg.header();
            let member = header.member();
            if member.as_ref().map(|m| m.as_str()) == Some("changed") {
                // The signal carries a single string argument: the palette TOML.
                if let Ok(body) = msg.body().deserialize::<(String,)>() {
                    match toml::from_str::<Palette>(&body.0) {
                        Ok(palette) => {
                            if tx.send(palette).await.is_err() {
                                tracing::debug!("palette channel closed, stopping listener");
                                break;
                            }
                        }
                        Err(err) => {
                            tracing::warn!("received invalid palette TOML: {err}");
                        }
                    }
                }
            }
        }
        tracing::debug!("palette D-Bus listener ended");
    });

    Ok(rx)
}

#[cfg(test)]
mod tests {
    use slate_common::dbus::{PALETTE_BUS_NAME, PALETTE_INTERFACE, PALETTE_PATH};

    #[test]
    fn dbus_constants_are_consistent() {
        assert_eq!(PALETTE_INTERFACE, "org.slate.Palette");
        assert_eq!(PALETTE_PATH, "/org/slate/Palette");
        assert_eq!(PALETTE_BUS_NAME, "org.slate.Palette");
    }

    #[test]
    fn match_rule_builds_successfully() {
        let rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface(PALETTE_INTERFACE)
            .unwrap()
            .member("changed")
            .unwrap()
            .path(PALETTE_PATH)
            .unwrap()
            .build();

        let rule_str = rule.to_string();
        assert!(rule_str.contains("signal"));
        assert!(rule_str.contains("org.slate.Palette"));
        assert!(rule_str.contains("changed"));
    }
}
