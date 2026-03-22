/// rhea — AI engine daemon for Slate OS.
///
/// Loads configuration from settings.toml, creates the appropriate backend
/// via the Router, registers the org.slate.Rhea D-Bus interface, and runs
/// the event loop until SIGINT/SIGTERM.
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::sync::RwLock;
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

    let router = Router::from_config(&config)
        .await
        .context("failed to create router")?;

    let shared_router = Arc::new(RwLock::new(router));

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

    info!("rhea shutdown complete");
    Ok(())
}
