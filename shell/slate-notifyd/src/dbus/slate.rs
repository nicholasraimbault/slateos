/// `org.slate.Notifications` D-Bus interface implementation.
///
/// Custom Slate OS extensions: grouping, history, DND, UUID-based operations.
/// Cross-interface signals (NotificationClosed, ActionInvoked) are emitted
/// on the correct `/org/freedesktop/Notifications` path using a stored connection.
use std::path::PathBuf;

use chrono::{TimeZone, Utc};
use uuid::Uuid;
use zbus::object_server::SignalEmitter;

use super::{SharedStore, FREEDESKTOP_PATH};
use crate::grouping;
use crate::history::{HistoryReader, HistoryWriter};

use super::freedesktop::FreedesktopNotifications;

// ---------------------------------------------------------------------------
// SlateNotifications struct
// ---------------------------------------------------------------------------

/// Custom Slate OS notification interface with grouping, history, DND.
pub struct SlateNotifications {
    pub store: SharedStore,
    pub history_dir: PathBuf,
    /// Stored connection for emitting cross-interface signals on the correct path.
    pub connection: zbus::Connection,
}

#[zbus::interface(name = "org.slate.Notifications")]
impl SlateNotifications {
    /// Return all active notifications as TOML.
    async fn get_active(&self) -> String {
        let store = self.store.read().await;
        let active = store.get_active();
        let notifications: Vec<slate_common::notifications::Notification> =
            active.into_iter().cloned().collect();

        #[derive(serde::Serialize)]
        struct Wrapper {
            notifications: Vec<slate_common::notifications::Notification>,
        }

        toml::to_string_pretty(&Wrapper { notifications }).unwrap_or_default()
    }

    /// Return history since a UNIX timestamp, up to limit items, as TOML.
    async fn get_history(&self, since_timestamp: i64, limit: u32) -> String {
        let since = Utc
            .timestamp_opt(since_timestamp, 0)
            .single()
            .unwrap_or_else(Utc::now);
        let notifications = HistoryReader::read(&self.history_dir, since, limit as usize)
            .unwrap_or_default();

        #[derive(serde::Serialize)]
        struct Wrapper {
            notifications: Vec<slate_common::notifications::Notification>,
        }

        toml::to_string_pretty(&Wrapper { notifications }).unwrap_or_default()
    }

    /// Return a group summary for a specific app as TOML.
    async fn get_group_summary(&self, app_name: &str) -> String {
        let store = self.store.read().await;
        let active: Vec<slate_common::notifications::Notification> = store
            .get_active()
            .into_iter()
            .filter(|n| n.app_name == app_name)
            .cloned()
            .collect();

        let groups = grouping::group_notifications(&active);

        #[derive(serde::Serialize)]
        struct Wrapper {
            groups: Vec<grouping::NotificationGroup>,
        }

        toml::to_string_pretty(&Wrapper { groups }).unwrap_or_default()
    }

    /// Dismiss a single notification by UUID string.
    async fn dismiss(
        &self,
        uuid_str: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<bool> {
        let uuid = Uuid::parse_str(uuid_str)
            .map_err(|e| zbus::fdo::Error::InvalidArgs(format!("invalid UUID: {e}")))?;
        let mut store = self.store.write().await;
        if let Some(dismissed) = store.dismiss(uuid) {
            let fd_id = dismissed.fd_id;
            let _ = HistoryWriter::append(&dismissed, &self.history_dir);
            drop(store);

            // Emit Dismissed on the Slate interface (same interface, direct call).
            let _ = Self::dismissed(&emitter, uuid_str, "user").await;

            // Emit NotificationClosed on the freedesktop interface at the correct path.
            // Reason 2 = dismissed by the user (per freedesktop spec).
            if let Ok(fd_emitter) = SignalEmitter::new(&self.connection, FREEDESKTOP_PATH) {
                let _ = FreedesktopNotifications::notification_closed(&fd_emitter, fd_id, 2).await;
            }

            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Dismiss all non-persistent notifications.
    async fn dismiss_all(&self, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) -> u32 {
        let mut store = self.store.write().await;
        let dismissed = store.dismiss_all();

        // Collect per-app counts after dismissal to emit GroupChanged once per app.
        let mut app_counts: std::collections::HashMap<String, u32> =
            std::collections::HashMap::new();
        for n in &dismissed {
            let count = store
                .get_active()
                .iter()
                .filter(|a| a.app_name == n.app_name)
                .count() as u32;
            app_counts.insert(n.app_name.clone(), count);
        }

        for n in &dismissed {
            let _ = HistoryWriter::append(n, &self.history_dir);
        }
        drop(store);

        let fd_emitter = SignalEmitter::new(&self.connection, FREEDESKTOP_PATH).ok();
        for n in &dismissed {
            let uuid_str = n.uuid.to_string();
            let _ = Self::dismissed(&emitter, &uuid_str, "dismiss_all").await;

            // Emit NotificationClosed on the freedesktop interface at the correct path.
            if let Some(ref fde) = fd_emitter {
                let _ = FreedesktopNotifications::notification_closed(fde, n.fd_id, 2).await;
            }
        }

        // Emit GroupChanged once per affected app.
        for (app_name, count) in &app_counts {
            let _ = Self::group_changed(&emitter, app_name, *count).await;
        }

        dismissed.len() as u32
    }

    /// Dismiss all notifications from a specific app.
    async fn dismiss_group(
        &self,
        app_name: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> u32 {
        let mut store = self.store.write().await;
        let dismissed = store.dismiss_group(app_name);
        let remaining_count = store
            .get_active()
            .iter()
            .filter(|n| n.app_name == app_name)
            .count() as u32;
        for n in &dismissed {
            let _ = HistoryWriter::append(n, &self.history_dir);
        }
        drop(store);

        let fd_emitter = SignalEmitter::new(&self.connection, FREEDESKTOP_PATH).ok();
        for n in &dismissed {
            let uuid_str = n.uuid.to_string();
            let _ = Self::dismissed(&emitter, &uuid_str, "dismiss_group").await;

            // Emit NotificationClosed on the freedesktop interface at the correct path.
            if let Some(ref fde) = fd_emitter {
                let _ = FreedesktopNotifications::notification_closed(fde, n.fd_id, 2).await;
            }
        }

        // Emit GroupChanged with updated count (0 after full group dismiss).
        if !dismissed.is_empty() {
            let _ = Self::group_changed(&emitter, app_name, remaining_count).await;
        }

        dismissed.len() as u32
    }

    /// Invoke an action on a notification by UUID and action key.
    async fn invoke_action(
        &self,
        uuid_str: &str,
        action_key: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<bool> {
        let uuid = Uuid::parse_str(uuid_str)
            .map_err(|e| zbus::fdo::Error::InvalidArgs(format!("invalid UUID: {e}")))?;
        let store = self.store.read().await;
        if let Some(n) = store.get_by_uuid(uuid) {
            let has_action = n.actions.iter().any(|a| a.key == action_key);
            if has_action {
                let fd_id = n.fd_id;
                drop(store);

                // Emit ActionInvoked on the Slate interface.
                let _ = Self::action_invoked_signal(&emitter, uuid_str, action_key).await;

                // Emit ActionInvoked on the freedesktop interface at the correct path.
                if let Ok(fd_emitter) = SignalEmitter::new(&self.connection, FREEDESKTOP_PATH) {
                    let _ =
                        FreedesktopNotifications::action_invoked(&fd_emitter, fd_id, action_key)
                            .await;
                }

                Ok(true)
            } else {
                Ok(false)
            }
        } else {
            Ok(false)
        }
    }

    /// Mark a notification as read.
    async fn mark_read(
        &self,
        uuid_str: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<bool> {
        let uuid = Uuid::parse_str(uuid_str)
            .map_err(|e| zbus::fdo::Error::InvalidArgs(format!("invalid UUID: {e}")))?;
        let mut store = self.store.write().await;
        let marked = store.mark_read(uuid);
        if marked {
            // Retrieve the updated notification to include in the signal body.
            let data = store
                .get_by_uuid(uuid)
                .map(|n| toml::to_string_pretty(n).unwrap_or_default())
                .unwrap_or_default();
            drop(store);
            let _ = Self::updated(&emitter, uuid_str, &data).await;
        }
        Ok(marked)
    }

    /// DND property — read.
    #[zbus(property)]
    async fn dnd(&self) -> bool {
        self.store.read().await.dnd
    }

    /// DND property — write.
    #[zbus(property)]
    async fn set_dnd(&self, value: bool) {
        self.store.write().await.dnd = value;
    }

    /// Signal: notification added.
    #[zbus(signal)]
    pub async fn added(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        uuid: &str,
        notification_data: &str,
    ) -> zbus::Result<()>;

    /// Signal: notification updated.
    #[zbus(signal)]
    pub async fn updated(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        uuid: &str,
        notification_data: &str,
    ) -> zbus::Result<()>;

    /// Signal: notification dismissed.
    #[zbus(signal)]
    pub async fn dismissed(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        uuid: &str,
        reason: &str,
    ) -> zbus::Result<()>;

    /// Signal: an action was invoked (Slate interface copy for convenience).
    #[zbus(signal)]
    pub async fn action_invoked_signal(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        uuid: &str,
        action_key: &str,
    ) -> zbus::Result<()>;

    /// Signal: group changed (app's notification count changed).
    #[zbus(signal)]
    pub async fn group_changed(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        app_name: &str,
        count: u32,
    ) -> zbus::Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::NotificationStore;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[test]
    fn slate_notifications_store_accessible() {
        // Verify store type alias works with the struct
        let store: SharedStore = Arc::new(RwLock::new(NotificationStore::new()));
        let _history_dir = std::path::PathBuf::from("/tmp/history");
        // We can't easily test the full struct without a real connection,
        // but we verify the field types are consistent
        let _ = store;
    }

    #[test]
    fn slate_path_constant_is_correct() {
        assert_eq!(super::super::SLATE_PATH, "/org/slate/Notifications");
    }

    #[test]
    fn freedesktop_path_constant_is_correct() {
        assert_eq!(FREEDESKTOP_PATH, "/org/freedesktop/Notifications");
    }
}
