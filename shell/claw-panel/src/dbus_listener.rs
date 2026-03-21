// D-Bus listener for external show/hide and palette signals.
//
// TouchFlow dispatches gesture events over D-Bus to control the Claw Panel
// (right-edge swipe to show, swipe-right-on-panel to hide, etc.).
// The palette daemon broadcasts colour changes that we pick up here.

use futures_util::StreamExt;

use slate_common::dbus::{CLAW_INTERFACE, CLAW_PATH, PALETTE_BUS_NAME, PALETTE_PATH};
use slate_common::Palette;

// Unused import silenced — CLAW_INTERFACE is used in the attribute string
// literal; we still verify it in tests.
#[allow(unused_imports)]
use slate_common::dbus::PALETTE_INTERFACE;

/// Events received over D-Bus that the panel cares about.
#[derive(Debug, Clone)]
pub enum DbusEvent {
    Show,
    Hide,
    Toggle,
    PaletteChanged(Palette),
}

// ---------------------------------------------------------------------------
// Claw Panel D-Bus interface (server-side, receives calls from TouchFlow)
// ---------------------------------------------------------------------------

/// The D-Bus object that TouchFlow calls to show/hide the panel.
pub struct ClawService {
    event_tx: tokio::sync::mpsc::UnboundedSender<DbusEvent>,
}

impl ClawService {
    pub fn new(event_tx: tokio::sync::mpsc::UnboundedSender<DbusEvent>) -> Self {
        Self { event_tx }
    }
}

#[zbus::interface(name = "org.slate.Claw")]
impl ClawService {
    async fn show(&self) {
        let _ = self.event_tx.send(DbusEvent::Show);
    }

    async fn hide(&self) {
        let _ = self.event_tx.send(DbusEvent::Hide);
    }

    async fn toggle(&self) {
        let _ = self.event_tx.send(DbusEvent::Toggle);
    }
}

// ---------------------------------------------------------------------------
// Palette change listener
// ---------------------------------------------------------------------------

/// Subscribe to palette changes from the slate-palette daemon.
///
/// Fetches the initial palette, then listens for the `Changed` signal.
/// Each palette (initial + every update) is sent to `event_tx`.
pub async fn watch_palette(
    event_tx: tokio::sync::mpsc::UnboundedSender<DbusEvent>,
) -> Result<(), anyhow::Error> {
    let connection = zbus::Connection::session().await?;

    let proxy =
        zbus::Proxy::new(&connection, PALETTE_BUS_NAME, PALETTE_PATH, CLAW_INTERFACE).await?;

    // Fetch initial palette via the PaletteToml property.
    match proxy.get_property::<String>("PaletteToml").await {
        Ok(toml_str) => {
            if let Ok(palette) = toml::from_str::<Palette>(&toml_str) {
                let _ = event_tx.send(DbusEvent::PaletteChanged(palette));
            }
        }
        Err(e) => {
            tracing::warn!("Could not fetch initial palette: {e}");
        }
    }

    // Listen for the Changed signal (carries the palette TOML as a string).
    let mut stream = proxy.receive_signal("Changed").await?;

    while let Some(signal) = stream.next().await {
        let body = signal.body();
        if let Ok(toml_str) = body.deserialize::<String>() {
            if let Ok(palette) = toml::from_str::<Palette>(&toml_str) {
                let _ = event_tx.send(DbusEvent::PaletteChanged(palette));
            }
        }
    }

    Ok(())
}

/// Register the Claw D-Bus service and start the palette watcher.
///
/// Returns the event receiver that the iced app polls for D-Bus events.
pub async fn start() -> Result<tokio::sync::mpsc::UnboundedReceiver<DbusEvent>, anyhow::Error> {
    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();

    let connection = zbus::Connection::session().await?;

    // Publish our own interface so TouchFlow can call Show/Hide/Toggle.
    let service = ClawService::new(event_tx.clone());
    connection.object_server().at(CLAW_PATH, service).await?;

    // Start palette watcher in background.
    let palette_tx = event_tx.clone();
    tokio::spawn(async move {
        if let Err(e) = watch_palette(palette_tx).await {
            tracing::warn!("Palette watcher failed: {e}");
        }
    });

    Ok(event_rx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dbus_event_variants_are_debug_clone() {
        let show = DbusEvent::Show;
        let _cloned = show.clone();
        let _debug = format!("{show:?}");
    }

    #[test]
    fn claw_interface_path_constants_match_spec() {
        assert_eq!(CLAW_INTERFACE, "org.slate.Claw");
        assert_eq!(CLAW_PATH, "/org/slate/Claw");
    }
}
