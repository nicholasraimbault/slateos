// On non-Linux platforms (macOS dev machines) the event loop and input reader
// are compiled out. Allow dead_code so the gesture/dispatch modules can still
// be compiled and tested without spurious warnings.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

/// TouchFlow — multitouch gesture daemon for Slate OS.
///
/// Reads raw multitouch events from the touchscreen, recognizes gestures,
/// and dispatches them to Niri IPC / D-Bus.
mod config;
mod dbus_emitter;
mod dispatch;
mod edge;
mod gesture;
mod input;
mod niri_ipc;
mod physics;

use anyhow::Result;
use tracing::{error, info};

use crate::config::TouchFlowConfig;
use crate::gesture::{GestureConfig, Recognizer};
use crate::niri_ipc::NiriClient;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialise tracing (respects RUST_LOG env var).
    tracing_subscriber::fmt::init();

    info!("TouchFlow daemon starting");

    // Load configuration.
    let config = TouchFlowConfig::load_or_default();
    info!(?config, "configuration loaded");

    // Build gesture recognizer with config-driven thresholds.
    let gesture_config = GestureConfig {
        swipe_min_distance: 50.0 * config.gestures.sensitivity,
        edge: crate::edge::EdgeConfig {
            size: config.edge.size,
            screen_width: config.edge.screen_width,
            screen_height: config.edge.screen_height,
        },
        ..GestureConfig::default()
    };
    let mut recognizer = Recognizer::new(gesture_config);

    // Initialise Niri IPC client.
    let niri = match NiriClient::with_socket_override(config.niri.socket_path.clone()).await {
        Ok(c) => c,
        Err(e) => {
            error!(%e, "failed to initialise Niri IPC client — running without WM control");
            // Create a dummy client with a placeholder path so the daemon can
            // still start and process non-Niri gestures (edge swipes, pinch).
            NiriClient::with_socket_override(Some("/dev/null".into())).await?
        }
    };

    // Initialise D-Bus emitter.
    let emitter = match dbus_emitter::TouchFlowEmitter::new().await {
        Ok(e) => e,
        Err(e) => {
            error!(%e, "failed to connect to session bus — gesture signals will not be emitted");
            return Err(e);
        }
    };

    // -- Event loop (Linux only) --
    // On non-Linux (macOS dev), we just log and exit.
    #[cfg(target_os = "linux")]
    {
        use tokio::signal;

        let (tx, mut rx) = tokio::sync::mpsc::channel::<input::InputEvent>(256);

        // Spawn input reader in background.
        tokio::spawn(async move {
            if let Err(e) = input::device::run_input_reader(tx).await {
                error!(%e, "input reader exited");
            }
        });

        info!("entering event loop");

        loop {
            tokio::select! {
                Some(event) = rx.recv() => {
                    if let Some(gesture) = recognizer.on_event(&event) {
                        info!(?gesture, "gesture recognized");
                        if let Err(e) = dispatch::dispatch_gesture(&gesture, &niri, &emitter, &config).await {
                            error!(%e, "dispatch failed");
                        }
                    }
                }
                _ = signal::ctrl_c() => {
                    info!("received SIGINT, shutting down");
                    break;
                }
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        // Suppress unused-variable warnings on non-Linux.
        let _ = (&niri, &emitter, &mut recognizer, &config);
        info!("non-Linux platform — event loop not available, exiting");
    }

    info!("TouchFlow daemon stopped");
    Ok(())
}
