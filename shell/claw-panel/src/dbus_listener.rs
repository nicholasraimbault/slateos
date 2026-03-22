// D-Bus listener for external show/hide, palette signals, and Rhea AI signals.
//
// TouchFlow dispatches gesture events over D-Bus to control the Claw Panel
// (right-edge swipe to show, swipe-right-on-panel to hide, etc.).
// The palette daemon broadcasts colour changes that we pick up here.
// Rhea (the AI engine) emits CompletionChunk / CompletionDone / CompletionError
// signals that we forward to the UI message loop.
//
// On non-Linux platforms the listener is a no-op that keeps the subscription
// alive without connecting to any bus.

use slate_common::Palette;

// Constants are used in cfg(linux) blocks and tests.
#[cfg(any(target_os = "linux", test))]
use slate_common::dbus::{CLAW_INTERFACE, CLAW_PATH};
#[cfg(target_os = "linux")]
use slate_common::dbus::{
    PALETTE_BUS_NAME, PALETTE_INTERFACE, PALETTE_PATH, RHEA_BUS_NAME, RHEA_INTERFACE, RHEA_PATH,
};

/// Events received over D-Bus that the panel cares about.
#[derive(Debug, Clone)]
pub enum DbusEvent {
    Show,
    Hide,
    Toggle,
    PaletteChanged(Palette),
    /// A streaming text chunk from the Rhea AI engine.
    RheaChunk(String),
    /// Rhea finished generating a response.
    RheaDone,
    /// Rhea encountered an error while generating a response.
    RheaError(String),
    /// The active Rhea backend name changed.
    RheaBackendChanged(String),
}

// ---------------------------------------------------------------------------
// Claw Panel D-Bus interface (server-side, receives calls from TouchFlow)
// ---------------------------------------------------------------------------

/// The D-Bus object that TouchFlow calls to show/hide the panel.
#[cfg(target_os = "linux")]
pub struct ClawService {
    event_tx: tokio::sync::mpsc::UnboundedSender<DbusEvent>,
}

#[cfg(target_os = "linux")]
impl ClawService {
    pub fn new(event_tx: tokio::sync::mpsc::UnboundedSender<DbusEvent>) -> Self {
        Self { event_tx }
    }
}

#[cfg(target_os = "linux")]
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
#[cfg(target_os = "linux")]
pub async fn watch_palette(
    event_tx: tokio::sync::mpsc::UnboundedSender<DbusEvent>,
) -> Result<(), anyhow::Error> {
    use futures_util::StreamExt;

    let connection = zbus::Connection::session().await?;

    let proxy = zbus::Proxy::new(
        &connection,
        PALETTE_BUS_NAME,
        PALETTE_PATH,
        PALETTE_INTERFACE,
    )
    .await?;

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

// ---------------------------------------------------------------------------
// Rhea AI signal listener
// ---------------------------------------------------------------------------

/// Subscribe to streaming completion signals from the Rhea AI engine.
///
/// Listens for `CompletionChunk`, `CompletionDone`, and `CompletionError`
/// signals on `org.slate.Rhea` and forwards them to `event_tx`. Also listens
/// for `BackendChanged` so the panel can display the active backend name.
///
/// This function returns when the signal stream ends (i.e. the bus connection
/// is dropped), which should only happen during shutdown.
#[cfg(target_os = "linux")]
pub async fn watch_rhea(
    event_tx: tokio::sync::mpsc::UnboundedSender<DbusEvent>,
) -> Result<(), anyhow::Error> {
    use futures_util::StreamExt;

    let connection = zbus::Connection::session().await?;

    let proxy = zbus::Proxy::new(&connection, RHEA_BUS_NAME, RHEA_PATH, RHEA_INTERFACE).await?;

    // Subscribe to all four signals concurrently.
    let mut chunk_stream = proxy.receive_signal("CompletionChunk").await?;
    let mut done_stream = proxy.receive_signal("CompletionDone").await?;
    let mut error_stream = proxy.receive_signal("CompletionError").await?;
    let mut backend_stream = proxy.receive_signal("BackendChanged").await?;

    tracing::info!("Subscribed to Rhea D-Bus signals at {RHEA_PATH}");

    loop {
        tokio::select! {
            Some(signal) = chunk_stream.next() => {
                if let Ok(chunk) = signal.body().deserialize::<String>() {
                    let _ = event_tx.send(DbusEvent::RheaChunk(chunk));
                }
            }
            Some(_signal) = done_stream.next() => {
                // CompletionDone carries full_text as an argument but we only
                // need to know it finished — the chunks already built the text.
                let _ = event_tx.send(DbusEvent::RheaDone);
            }
            Some(signal) = error_stream.next() => {
                let msg = signal
                    .body()
                    .deserialize::<String>()
                    .unwrap_or_else(|_| "Unknown error".to_string());
                let _ = event_tx.send(DbusEvent::RheaError(msg));
            }
            Some(signal) = backend_stream.next() => {
                if let Ok(name) = signal.body().deserialize::<String>() {
                    let _ = event_tx.send(DbusEvent::RheaBackendChanged(name));
                }
            }
            else => {
                // All streams closed — Rhea has shut down or disconnected.
                tracing::warn!("Rhea D-Bus signal streams all closed");
                break;
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// D-Bus subscription entry point
// ---------------------------------------------------------------------------

/// Run the D-Bus listener loop on Linux: register the Claw service,
/// watch for palette changes, and subscribe to Rhea AI signals.
/// Events are forwarded through `event_tx`.
///
/// This function never returns under normal operation; it keeps the D-Bus
/// connection alive so incoming signals and method calls are processed.
#[cfg(target_os = "linux")]
pub async fn run(event_tx: tokio::sync::mpsc::UnboundedSender<DbusEvent>) {
    let connection = match zbus::Connection::session().await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("failed to connect to session bus: {e}");
            return;
        }
    };

    // Publish our own interface so TouchFlow can call Show/Hide/Toggle.
    let service = ClawService::new(event_tx.clone());
    if let Err(e) = connection.object_server().at(CLAW_PATH, service).await {
        tracing::error!("failed to register Claw D-Bus service: {e}");
        return;
    }

    tracing::info!("Claw D-Bus service registered at {CLAW_PATH}");

    // Start palette watcher and Rhea signal watcher concurrently.
    let palette_tx = event_tx.clone();
    let rhea_tx = event_tx;

    tokio::select! {
        result = watch_palette(palette_tx) => {
            if let Err(e) = result {
                tracing::warn!("Palette watcher ended: {e}");
            }
        }
        result = watch_rhea(rhea_tx) => {
            if let Err(e) = result {
                tracing::warn!("Rhea signal watcher ended: {e}");
            }
        }
    }

    // Keep the task alive so the D-Bus connection is not dropped.
    std::future::pending::<()>().await;
}

/// Non-Linux stub: keeps the subscription future alive without doing anything.
#[cfg(not(target_os = "linux"))]
pub async fn run(_event_tx: tokio::sync::mpsc::UnboundedSender<DbusEvent>) {
    tracing::warn!("D-Bus listener is not available on this platform");
    std::future::pending::<()>().await;
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

    #[test]
    fn dbus_event_all_variants_constructible() {
        let _show = DbusEvent::Show;
        let _hide = DbusEvent::Hide;
        let _toggle = DbusEvent::Toggle;
        let _palette = DbusEvent::PaletteChanged(Palette::default());
    }

    #[test]
    fn rhea_dbus_event_variants_constructible() {
        let _chunk = DbusEvent::RheaChunk("hello world".to_string());
        let _done = DbusEvent::RheaDone;
        let _error = DbusEvent::RheaError("timeout".to_string());
        let _backend = DbusEvent::RheaBackendChanged("local".to_string());
    }

    #[test]
    fn rhea_dbus_event_variants_are_debug_clone() {
        let chunk = DbusEvent::RheaChunk("test chunk".to_string());
        let _cloned = chunk.clone();
        let _debug = format!("{chunk:?}");

        let done = DbusEvent::RheaDone;
        let _cloned = done.clone();
        let _debug = format!("{done:?}");
    }
}
