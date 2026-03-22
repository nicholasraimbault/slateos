/// rhea — AI engine daemon for Slate OS.
///
/// Loads configuration from settings.toml, creates the appropriate backend
/// via the Router, registers the org.slate.Rhea D-Bus interface, and runs
/// the event loop until SIGINT/SIGTERM.
///
/// Model lifecycle signals (ModelLoaded, ModelUnloaded) are wired via mpsc
/// channels: the LocalBackend sends on them on state transitions, and a
/// background task emits the corresponding D-Bus signals.
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use tracing::{info, warn};

mod cloud;
mod config;
mod context;
mod dbus;
mod local;
mod router;

use crate::config::RheaConfig;
use crate::dbus::RheaInterface;
use crate::router::Router;

// ---------------------------------------------------------------------------
// Settings path helper
// ---------------------------------------------------------------------------

fn settings_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    PathBuf::from(home).join(".config/slate/settings.toml")
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    info!("starting rhea AI engine");

    let settings = settings_path();
    let config = RheaConfig::load(&settings).unwrap_or_else(|e| {
        warn!("failed to load settings ({e}), using defaults");
        RheaConfig::from_rhea_settings(&slate_common::settings::RheaSettings::default())
    });

    let backend_name = if config.backend == crate::config::BackendKind::Local {
        "local".to_string()
    } else {
        "cloud".to_string()
    };

    info!(backend = %backend_name, "initializing router");

    // Create signal notification channels before building the router so the
    // LocalBackend can hold senders while we hold receivers for the listener tasks.
    let (model_loaded_tx, mut model_loaded_rx) = tokio::sync::mpsc::channel::<String>(8);
    let (model_unloaded_tx, mut model_unloaded_rx) = tokio::sync::mpsc::channel::<String>(8);

    let router =
        Router::from_config_with_signals(&config, Some(model_loaded_tx), Some(model_unloaded_tx))
            .await
            .context("failed to create router")?;

    let shared_router = Arc::new(router);
    // Keep a reference for graceful shutdown so we can call router.shutdown()
    // after the interface gives up its Arc (which moves into the object server).
    let shutdown_router = Arc::clone(&shared_router);

    // Connect to session D-Bus and register the Rhea interface.
    let conn = zbus::Connection::session()
        .await
        .context("failed to connect to session D-Bus")?;

    let iface = RheaInterface {
        router: shared_router,
        backend_name,
    };

    conn.object_server()
        .at(slate_common::dbus::RHEA_PATH, iface)
        .await
        .context("failed to register org.slate.Rhea interface")?;

    conn.request_name(slate_common::dbus::RHEA_BUS_NAME)
        .await
        .context("failed to acquire org.slate.Rhea bus name")?;

    info!("org.slate.Rhea registered, waiting for requests");

    // Spawn a task that listens for ModelLoaded notifications and emits the
    // corresponding D-Bus signal. The SignalEmitter is created from the
    // connection pointing at the registered Rhea object path.
    {
        let conn = conn.clone();
        tokio::spawn(async move {
            while let Some(model_path) = model_loaded_rx.recv().await {
                if let Ok(emitter) =
                    zbus::object_server::SignalEmitter::new(&conn, slate_common::dbus::RHEA_PATH)
                {
                    let _ = RheaInterface::model_loaded(&emitter, &model_path).await;
                    info!(model = %model_path, "emitted ModelLoaded signal");
                }
            }
        });
    }

    // Spawn a task that listens for ModelUnloaded notifications and emits the
    // corresponding D-Bus signal.
    {
        let conn = conn.clone();
        tokio::spawn(async move {
            while let Some(model_path) = model_unloaded_rx.recv().await {
                if let Ok(emitter) =
                    zbus::object_server::SignalEmitter::new(&conn, slate_common::dbus::RHEA_PATH)
                {
                    let _ = RheaInterface::model_unloaded(&emitter, &model_path).await;
                    info!(model = %model_path, "emitted ModelUnloaded signal");
                }
            }
        });
    }

    // BackendChanged: emitted if the active backend changes at runtime.
    // Runtime reconfiguration is not yet implemented; this signal is reserved
    // for a future hot-swap feature. See: TODO(backend-switch)

    // Graceful shutdown on SIGINT (ctrl-c) or SIGTERM (arkhe stop).
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .context("failed to install SIGTERM handler")?;

    tokio::select! {
        result = tokio::signal::ctrl_c() => {
            match result {
                Ok(()) => info!("received SIGINT, shutting down"),
                Err(e) => warn!("failed to listen for SIGINT: {e}"),
            }
        }
        _ = sigterm.recv() => {
            info!("received SIGTERM, shutting down");
        }
    }

    // Kill the llama-server child process (if running) before the process exits.
    // Without this the child becomes an orphan and holds the model in RAM.
    shutdown_router.shutdown().await;

    info!("rhea shutdown complete");
    Ok(())
}
