/// slate-notifyd — notification daemon for Slate OS.
///
/// Implements the freedesktop notification D-Bus spec and a custom Slate OS
/// extension interface. Notifications are persisted to disk and history is
/// stored in daily TOML files.
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::signal::unix::SignalKind;
use tokio::sync::RwLock;
use tracing::{info, warn};

mod dbus;
mod error;
mod grouping;
mod history;
mod store;

use crate::dbus::{FreedesktopNotifications, SharedStore, SlateNotifications};
use crate::history::HistoryReader;
use crate::store::NotificationStore;

// ---------------------------------------------------------------------------
// State directory helpers
// ---------------------------------------------------------------------------

/// Return the path to the active notifications file.
fn active_path() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME not set")?;
    Ok(PathBuf::from(home).join(".local/state/slate-notifyd/active.toml"))
}

// ---------------------------------------------------------------------------
// Debounced persistence
// ---------------------------------------------------------------------------

/// Periodically flush dirty store state to disk (at most once per second).
async fn persistence_loop(store: SharedStore, path: PathBuf) {
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
    loop {
        interval.tick().await;
        let mut s = store.write().await;
        if s.dirty {
            if let Err(e) = s.save_active(&path) {
                warn!("failed to persist active notifications: {e}");
            } else {
                s.dirty = false;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    info!("starting slate-notifyd");

    let active_file = active_path()?;
    let history_dir =
        HistoryReader::default_base_dir().context("failed to determine history directory")?;

    // Load persisted state or start fresh
    let store = if active_file.exists() {
        match NotificationStore::load_active(&active_file) {
            Ok(s) => {
                info!("loaded {} active notifications", s.get_active().len());
                s
            }
            Err(e) => {
                warn!("failed to load active notifications, starting fresh: {e}");
                NotificationStore::new()
            }
        }
    } else {
        info!("no persisted state, starting fresh");
        NotificationStore::new()
    };

    let shared_store: SharedStore = Arc::new(RwLock::new(store));

    // Build D-Bus connection
    let conn = zbus::Connection::session()
        .await
        .context("failed to connect to session D-Bus")?;

    // Register freedesktop interface at the standard path
    let fd_iface = FreedesktopNotifications {
        store: shared_store.clone(),
        history_dir: history_dir.clone(),
    };
    conn.object_server()
        .at("/org/freedesktop/Notifications", fd_iface)
        .await
        .context("failed to register freedesktop interface")?;

    // Register custom Slate interface
    let slate_iface = SlateNotifications {
        store: shared_store.clone(),
        history_dir: history_dir.clone(),
    };
    conn.object_server()
        .at(slate_common::dbus::NOTIFICATIONS_PATH, slate_iface)
        .await
        .context("failed to register Slate interface")?;

    // Request bus names
    conn.request_name("org.freedesktop.Notifications")
        .await
        .context("failed to acquire org.freedesktop.Notifications bus name")?;
    conn.request_name(slate_common::dbus::NOTIFICATIONS_BUS_NAME)
        .await
        .context("failed to acquire org.slate.Notifications bus name")?;

    info!("D-Bus interfaces registered, listening for notifications");

    // Start debounced persistence
    let persist_store = shared_store.clone();
    let persist_path = active_file.clone();
    tokio::spawn(async move {
        persistence_loop(persist_store, persist_path).await;
    });

    // Wait for shutdown signal — handle both SIGINT (ctrl-c) and SIGTERM (arkhe stop).
    // arkhe sends SIGTERM when stopping a service, so both must trigger graceful shutdown.
    let mut sigterm = tokio::signal::unix::signal(SignalKind::terminate())
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

    // Final flush
    let s = shared_store.read().await;
    if let Err(e) = s.save_active(&active_file) {
        warn!("failed to persist state on shutdown: {e}");
    } else {
        info!("state persisted, shutting down");
    }

    Ok(())
}
