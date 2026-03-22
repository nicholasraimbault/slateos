/// slate-notifyd — notification daemon for Slate OS.
///
/// Implements the freedesktop notification D-Bus spec and a custom Slate OS
/// extension interface. Notifications are persisted to disk and history is
/// stored in daily TOML files.
use std::path::{Path, PathBuf};
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
// Async persistence helpers
// ---------------------------------------------------------------------------

/// Write serialized store content to disk asynchronously.
///
/// Creates parent directories if needed. Runs in a `spawn_blocking` task so
/// directory creation (which has no async API in tokio) does not block the executor.
async fn write_active(path: &Path, content: String) {
    let path = path.to_path_buf();
    let result = tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, content)?;
        Ok(())
    })
    .await;

    match result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => warn!("failed to write active notifications: {e}"),
        Err(e) => warn!("spawn_blocking panicked writing active notifications: {e}"),
    }
}

// ---------------------------------------------------------------------------
// Debounced persistence
// ---------------------------------------------------------------------------

/// Periodically flush dirty store state to disk (at most once per second).
///
/// Snapshots the serialized content while holding the write lock, then drops
/// the lock before performing I/O so the store is not blocked during writes.
async fn persistence_loop(store: SharedStore, path: PathBuf) {
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
    loop {
        interval.tick().await;
        let snapshot = {
            let mut s = store.write().await;
            if !s.dirty {
                continue;
            }
            match s.serialize_active() {
                Ok(content) => {
                    s.dirty = false;
                    Some(content)
                }
                Err(e) => {
                    warn!("failed to serialize active notifications: {e}");
                    None
                }
            }
        };

        if let Some(content) = snapshot {
            write_active(&path, content).await;
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

    // Load persisted state or start fresh using async I/O.
    let store = if active_file.exists() {
        match tokio::fs::read_to_string(&active_file).await {
            Ok(content) => match NotificationStore::deserialize_active(&content) {
                Ok(s) => {
                    info!("loaded {} active notifications", s.get_active().len());
                    s
                }
                Err(e) => {
                    warn!("failed to parse active notifications, starting fresh: {e}");
                    NotificationStore::new()
                }
            },
            Err(e) => {
                warn!("failed to read active notifications file, starting fresh: {e}");
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

    // Register freedesktop interface at the standard path.
    // The connection is stored so cross-interface signals reach the correct path.
    let fd_iface = FreedesktopNotifications {
        store: shared_store.clone(),
        history_dir: history_dir.clone(),
        connection: conn.clone(),
    };
    conn.object_server()
        .at("/org/freedesktop/Notifications", fd_iface)
        .await
        .context("failed to register freedesktop interface")?;

    // Register custom Slate interface.
    // The connection is stored so cross-interface signals reach the correct path.
    let slate_iface = SlateNotifications {
        store: shared_store.clone(),
        history_dir: history_dir.clone(),
        connection: conn.clone(),
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

    // Final flush — snapshot while holding the lock, then release before writing.
    let snapshot = {
        let s = shared_store.read().await;
        s.serialize_active()
    };
    match snapshot {
        Ok(content) => {
            write_active(&active_file, content).await;
            info!("state persisted, shutting down");
        }
        Err(e) => warn!("failed to serialize state on shutdown: {e}"),
    }

    Ok(())
}
