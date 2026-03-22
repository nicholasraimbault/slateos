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
mod sound;
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

/// Return the path to the shared settings file.
fn settings_path() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME not set")?;
    Ok(PathBuf::from(home).join(".config/slate/settings.toml"))
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
// Expiry ticker
// ---------------------------------------------------------------------------

/// Tuple collected during Phase 1 of the expiry scan.
struct ExpiredInfo {
    uuid: uuid::Uuid,
    fd_id: u32,
}

/// Periodically expire notifications whose `expire_timeout_ms` has elapsed.
///
/// Runs every second.  Three phases keep lock hold-time minimal:
/// 1. Read lock — collect expired (uuid, fd_id, app_name) tuples.
/// 2. Write lock — dismiss each expired notification.
/// 3. No lock — emit D-Bus signals.
///
/// `heads_up_duration_secs` is used as the server-default timeout when
/// `expire_timeout_ms == -1`.  Persistent notifications are never auto-expired.
async fn expiry_ticker(store: SharedStore, conn: zbus::Connection, heads_up_duration_secs: u32) {
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
    loop {
        interval.tick().await;

        let now = chrono::Utc::now();
        let server_default_ms = (heads_up_duration_secs as i64) * 1000;

        // Phase 1: read lock — find expired notifications.
        let expired: Vec<ExpiredInfo> = {
            let store = store.read().await;
            store
                .get_active()
                .into_iter()
                .filter(|n| {
                    if n.persistent {
                        return false;
                    }
                    let timeout_ms = match n.expire_timeout_ms {
                        0 => return false, // 0 = never expire
                        -1 => server_default_ms,
                        ms => ms as i64,
                    };
                    let elapsed_ms = now.signed_duration_since(n.timestamp).num_milliseconds();
                    elapsed_ms >= timeout_ms
                })
                .map(|n| ExpiredInfo {
                    uuid: n.uuid,
                    fd_id: n.fd_id,
                })
                .collect()
        };

        if expired.is_empty() {
            continue;
        }

        // Phase 2: write lock — dismiss each expired notification.
        {
            let mut store = store.write().await;
            for info in &expired {
                store.dismiss(info.uuid);
            }
        }

        // Phase 3: emit signals — no lock held.
        for info in &expired {
            // Reason 1 = expired (per freedesktop spec §3.6).
            if let Ok(fd_emitter) =
                zbus::object_server::SignalEmitter::new(&conn, crate::dbus::FREEDESKTOP_PATH)
            {
                let _ =
                    dbus::FreedesktopNotifications::notification_closed(&fd_emitter, info.fd_id, 1)
                        .await;
            }

            let uuid_str = info.uuid.to_string();
            let _ = dbus::SlateNotificationsSignalEmit::emit_dismissed(&conn, &uuid_str, "expired")
                .await;
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

    // Load user settings to get notification preferences.
    // Fall back to defaults when the settings file is absent or unparseable.
    let settings = settings_path()
        .ok()
        .and_then(|p| slate_common::settings::Settings::load(&p).ok())
        .unwrap_or_default();
    let sound_enabled = settings.notifications.sound_enabled;
    let heads_up_duration_secs = settings.notifications.heads_up_duration_secs;

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
        sound_enabled,
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

    // Start notification expiry ticker
    let expiry_store = shared_store.clone();
    let expiry_conn = conn.clone();
    tokio::spawn(async move {
        expiry_ticker(expiry_store, expiry_conn, heads_up_duration_secs).await;
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
