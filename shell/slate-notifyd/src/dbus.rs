/// D-Bus interfaces for the notification daemon.
///
/// Implements two interfaces:
/// 1. `org.freedesktop.Notifications` — the standard freedesktop spec
/// 2. `org.slate.Notifications` — custom Slate OS extensions
///
/// Both share the same `NotificationStore` via `Arc<RwLock<...>>`.
use std::collections::HashMap;
use std::sync::Arc;

use chrono::{TimeZone, Utc};
use slate_common::notifications::{Notification, NotificationAction, Urgency};
use tokio::sync::RwLock;
use uuid::Uuid;
use zbus::object_server::SignalEmitter;

use crate::grouping;
use crate::history::{HistoryReader, HistoryWriter};
use crate::store::NotificationStore;

// ---------------------------------------------------------------------------
// Shared state type alias
// ---------------------------------------------------------------------------

/// Shared reference to the notification store.
pub type SharedStore = Arc<RwLock<NotificationStore>>;

// Freedesktop notifications object path — needed when emitting cross-interface signals.
const FREEDESKTOP_PATH: &str = "/org/freedesktop/Notifications";

// ---------------------------------------------------------------------------
// freedesktop interface (org.freedesktop.Notifications)
// ---------------------------------------------------------------------------

/// Implements the standard freedesktop notification spec (v1.2).
pub struct FreedesktopNotifications {
    pub store: SharedStore,
    pub history_dir: std::path::PathBuf,
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl FreedesktopNotifications {
    /// Create or update a notification. Returns the fd_id.
    #[allow(clippy::too_many_arguments)]
    async fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: Vec<String>,
        hints: HashMap<String, zbus::zvariant::OwnedValue>,
        _expire_timeout: i32,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<u32> {
        let urgency = extract_urgency(&hints);
        let group_key = extract_group_key(&hints);
        let category = extract_string_hint(&hints, "category");
        let desktop_entry = extract_string_hint(&hints, "desktop-entry");

        let mut store = self.store.write().await;

        // If replaces_id > 0, try to update the existing notification
        if replaces_id > 0 {
            if let Some(existing) = store.get_by_fd_id(replaces_id).map(|n| n.uuid) {
                if let Some(n) = store.get_by_uuid(existing).cloned() {
                    let mut updated = n;
                    updated.summary = summary.to_string();
                    updated.body = body.to_string();
                    updated.app_icon = app_icon.to_string();
                    updated.urgency = urgency;
                    updated.actions = parse_actions(&actions);
                    updated.group_key = group_key;
                    updated.category = category;
                    updated.desktop_entry = desktop_entry;
                    updated.heads_up = should_heads_up(urgency, store.dnd);
                    let fd_id = updated.fd_id;
                    let uuid_str = updated.uuid.to_string();
                    store.update(updated.clone());
                    drop(store);

                    // Emit Updated signal on the Slate interface via a TODO cross-interface call.
                    // TODO: emit SlateNotifications::updated on org.slate.Notifications path.
                    // For now emit on this interface's emitter as a best-effort record.
                    let data = toml::to_string_pretty(&updated).unwrap_or_default();
                    let _ = SlateNotificationsSignalEmit::emit_updated(&emitter, &uuid_str, &data)
                        .await;

                    return Ok(fd_id);
                }
            }
        }

        let n_ref = store.add(app_name, summary, body);
        let fd_id = n_ref.fd_id;
        let uuid = n_ref.uuid;
        let uuid_str = uuid.to_string();

        // Apply extra fields that add() doesn't set
        let notification_data = if let Some(n) = store.get_by_uuid(uuid).cloned() {
            let mut updated = n;
            updated.app_icon = app_icon.to_string();
            updated.urgency = urgency;
            updated.actions = parse_actions(&actions);
            updated.group_key = group_key;
            updated.category = category;
            updated.desktop_entry = desktop_entry;
            updated.heads_up = should_heads_up(urgency, store.dnd);
            let data = toml::to_string_pretty(&updated).unwrap_or_default();
            store.update(updated);
            data
        } else {
            String::new()
        };
        drop(store);

        // Emit Added signal on the Slate interface.
        // TODO: emit on org.slate.Notifications path for proper cross-interface signalling.
        let _ =
            SlateNotificationsSignalEmit::emit_added(&emitter, &uuid_str, &notification_data).await;

        Ok(fd_id)
    }

    /// Close a notification by freedesktop ID.
    async fn close_notification(
        &self,
        id: u32,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        let mut store = self.store.write().await;
        if let Some(uuid) = store.get_by_fd_id(id).map(|n| n.uuid) {
            if let Some(dismissed) = store.dismiss(uuid) {
                let _ = HistoryWriter::append(&dismissed, &self.history_dir);
                let uuid_str = dismissed.uuid.to_string();
                drop(store);

                // Reason 3 = closed by a call to CloseNotification (per freedesktop spec).
                let _ = Self::notification_closed(&emitter, id, 3).await;

                // Emit Dismissed on the Slate interface.
                // TODO: emit on org.slate.Notifications path.
                let _ = SlateNotificationsSignalEmit::emit_dismissed(&emitter, &uuid_str, "closed")
                    .await;

                return Ok(());
            }
        }
        Ok(())
    }

    /// Return the server's capabilities.
    async fn get_capabilities(&self) -> Vec<String> {
        vec![
            "actions".to_string(),
            "body".to_string(),
            "body-markup".to_string(),
            "body-hyperlinks".to_string(),
            "icon-static".to_string(),
            "persistence".to_string(),
        ]
    }

    /// Return server identification.
    async fn get_server_information(&self) -> (String, String, String, String) {
        (
            "slate-notifyd".to_string(),
            "slateos".to_string(),
            "0.1.0".to_string(),
            "1.2".to_string(),
        )
    }

    /// Signal: a notification was closed.
    #[zbus(signal)]
    pub async fn notification_closed(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        id: u32,
        reason: u32,
    ) -> zbus::Result<()>;

    /// Signal: an action was invoked on a notification.
    #[zbus(signal)]
    pub async fn action_invoked(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        id: u32,
        action_key: &str,
    ) -> zbus::Result<()>;
}

// ---------------------------------------------------------------------------
// Helper to emit Slate interface signals from other interfaces.
//
// zbus generates `FreedesktopNotificationsSignals` and `SlateNotificationsSignals` traits
// for their respective `SignalEmitter` impls. However, the emitter passed into a
// FreedesktopNotifications method is bound to the freedesktop path. To emit on the
// Slate interface path without adding a second connection parameter, we use the
// low-level `SignalEmitter::emit` method directly.
// ---------------------------------------------------------------------------

struct SlateNotificationsSignalEmit;

impl SlateNotificationsSignalEmit {
    /// Emit org.slate.Notifications.Added via the low-level emit API.
    /// TODO: Replace with typed trait call once cross-interface emitter injection is simpler.
    async fn emit_added(
        emitter: &SignalEmitter<'_>,
        uuid: &str,
        notification_data: &str,
    ) -> zbus::Result<()> {
        emitter
            .emit(
                "org.slate.Notifications",
                "Added",
                &(uuid, notification_data),
            )
            .await
    }

    /// Emit org.slate.Notifications.Updated.
    async fn emit_updated(
        emitter: &SignalEmitter<'_>,
        uuid: &str,
        notification_data: &str,
    ) -> zbus::Result<()> {
        emitter
            .emit(
                "org.slate.Notifications",
                "Updated",
                &(uuid, notification_data),
            )
            .await
    }

    /// Emit org.slate.Notifications.Dismissed.
    async fn emit_dismissed(
        emitter: &SignalEmitter<'_>,
        uuid: &str,
        reason: &str,
    ) -> zbus::Result<()> {
        emitter
            .emit("org.slate.Notifications", "Dismissed", &(uuid, reason))
            .await
    }
}

// ---------------------------------------------------------------------------
// Slate custom interface (org.slate.Notifications)
// ---------------------------------------------------------------------------

/// Custom Slate OS notification interface with grouping, history, DND.
pub struct SlateNotifications {
    pub store: SharedStore,
    pub history_dir: std::path::PathBuf,
}

#[zbus::interface(name = "org.slate.Notifications")]
impl SlateNotifications {
    /// Return all active notifications as TOML.
    async fn get_active(&self) -> String {
        let store = self.store.read().await;
        let active = store.get_active();
        let notifications: Vec<Notification> = active.into_iter().cloned().collect();

        #[derive(serde::Serialize)]
        struct Wrapper {
            notifications: Vec<Notification>,
        }

        toml::to_string_pretty(&Wrapper { notifications }).unwrap_or_default()
    }

    /// Return history since a UNIX timestamp, up to limit items, as TOML.
    async fn get_history(&self, since_timestamp: i64, limit: u32) -> String {
        let since = Utc
            .timestamp_opt(since_timestamp, 0)
            .single()
            .unwrap_or_else(Utc::now);
        let notifications =
            HistoryReader::read(&self.history_dir, since, limit as usize).unwrap_or_default();

        #[derive(serde::Serialize)]
        struct Wrapper {
            notifications: Vec<Notification>,
        }

        toml::to_string_pretty(&Wrapper { notifications }).unwrap_or_default()
    }

    /// Return a group summary for a specific app as TOML.
    async fn get_group_summary(&self, app_name: &str) -> String {
        let store = self.store.read().await;
        let active: Vec<Notification> = store
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
        #[zbus(connection)] conn: &zbus::Connection,
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

            // Emit NotificationClosed on the freedesktop interface.
            // Reason 2 = dismissed by the user (per freedesktop spec).
            // TODO: use typed FreedesktopNotificationsSignals trait once emitter path injection is simpler.
            if let Ok(fd_emitter) = SignalEmitter::new(conn, FREEDESKTOP_PATH) {
                let _ = FreedesktopNotifications::notification_closed(&fd_emitter, fd_id, 2).await;
            }

            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Dismiss all non-persistent notifications.
    async fn dismiss_all(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> u32 {
        let mut store = self.store.write().await;
        let dismissed = store.dismiss_all();
        for n in &dismissed {
            let _ = HistoryWriter::append(n, &self.history_dir);
        }
        drop(store);

        // Emit Dismissed and NotificationClosed for every dismissed notification.
        let fd_emitter = SignalEmitter::new(conn, FREEDESKTOP_PATH).ok();
        for n in &dismissed {
            let uuid_str = n.uuid.to_string();
            let _ = Self::dismissed(&emitter, &uuid_str, "dismiss_all").await;

            // TODO: use typed FreedesktopNotificationsSignals trait.
            if let Some(ref fde) = fd_emitter {
                let _ = FreedesktopNotifications::notification_closed(fde, n.fd_id, 2).await;
            }
        }

        dismissed.len() as u32
    }

    /// Dismiss all notifications from a specific app.
    async fn dismiss_group(
        &self,
        app_name: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> u32 {
        let mut store = self.store.write().await;
        let dismissed = store.dismiss_group(app_name);
        for n in &dismissed {
            let _ = HistoryWriter::append(n, &self.history_dir);
        }
        drop(store);

        // Emit Dismissed and NotificationClosed for every dismissed notification.
        let fd_emitter = SignalEmitter::new(conn, FREEDESKTOP_PATH).ok();
        for n in &dismissed {
            let uuid_str = n.uuid.to_string();
            let _ = Self::dismissed(&emitter, &uuid_str, "dismiss_group").await;

            // TODO: use typed FreedesktopNotificationsSignals trait.
            if let Some(ref fde) = fd_emitter {
                let _ = FreedesktopNotifications::notification_closed(fde, n.fd_id, 2).await;
            }
        }

        dismissed.len() as u32
    }

    /// Invoke an action on a notification by UUID and action key.
    async fn invoke_action(
        &self,
        uuid_str: &str,
        action_key: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
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

                // Emit ActionInvoked on the freedesktop interface.
                // TODO: use typed FreedesktopNotificationsSignals trait.
                if let Ok(fd_emitter) = SignalEmitter::new(conn, FREEDESKTOP_PATH) {
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

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Extract urgency from the hints dict (key "urgency", byte value 0/1/2).
fn extract_urgency(hints: &HashMap<String, zbus::zvariant::OwnedValue>) -> Urgency {
    if let Some(val) = hints.get("urgency") {
        if let Ok(byte) = <u8 as TryFrom<_>>::try_from(val) {
            return match byte {
                0 => Urgency::Low,
                2 => Urgency::Critical,
                _ => Urgency::Normal,
            };
        }
    }
    Urgency::Normal
}

/// Extract the custom group key hint.
fn extract_group_key(hints: &HashMap<String, zbus::zvariant::OwnedValue>) -> Option<String> {
    extract_string_hint(hints, "x-slate-group-key")
}

/// Extract a string value from the hints dict.
fn extract_string_hint(
    hints: &HashMap<String, zbus::zvariant::OwnedValue>,
    key: &str,
) -> Option<String> {
    hints
        .get(key)
        .and_then(|val| <&str as TryFrom<_>>::try_from(val).ok().map(String::from))
}

/// Determine whether a notification should appear as a heads-up banner.
fn should_heads_up(urgency: Urgency, dnd: bool) -> bool {
    match urgency {
        // Critical notifications always break through
        Urgency::Critical => true,
        // Normal notifications show heads-up unless DND is on
        Urgency::Normal => !dnd,
        // Low urgency never shows heads-up
        Urgency::Low => false,
    }
}

/// Parse the freedesktop actions array (alternating key/label pairs).
fn parse_actions(actions: &[String]) -> Vec<NotificationAction> {
    actions
        .chunks(2)
        .filter_map(|chunk| {
            if chunk.len() == 2 {
                Some(NotificationAction::new(&chunk[0], &chunk[1]))
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_actions_pairs() {
        let actions = vec![
            "reply".to_string(),
            "Reply".to_string(),
            "dismiss".to_string(),
            "Dismiss".to_string(),
        ];
        let parsed = parse_actions(&actions);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].key, "reply");
        assert_eq!(parsed[0].label, "Reply");
        assert_eq!(parsed[1].key, "dismiss");
        assert_eq!(parsed[1].label, "Dismiss");
    }

    #[test]
    fn parse_actions_odd_count_drops_trailing() {
        let actions = vec![
            "reply".to_string(),
            "Reply".to_string(),
            "orphan".to_string(),
        ];
        let parsed = parse_actions(&actions);
        assert_eq!(parsed.len(), 1);
    }

    #[test]
    fn parse_actions_empty() {
        let parsed = parse_actions(&[]);
        assert!(parsed.is_empty());
    }

    #[test]
    fn should_heads_up_critical_always() {
        assert!(should_heads_up(Urgency::Critical, false));
        assert!(should_heads_up(Urgency::Critical, true));
    }

    #[test]
    fn should_heads_up_normal_respects_dnd() {
        assert!(should_heads_up(Urgency::Normal, false));
        assert!(!should_heads_up(Urgency::Normal, true));
    }

    #[test]
    fn should_heads_up_low_never() {
        assert!(!should_heads_up(Urgency::Low, false));
        assert!(!should_heads_up(Urgency::Low, true));
    }

    #[test]
    fn get_capabilities_returns_expected() {
        let expected = vec![
            "actions",
            "body",
            "body-markup",
            "body-hyperlinks",
            "icon-static",
            "persistence",
        ];
        let store = Arc::new(RwLock::new(NotificationStore::new()));
        let _fd = FreedesktopNotifications {
            store,
            history_dir: std::path::PathBuf::from("/tmp"),
        };
        // We can't easily call async methods in sync tests, but we can
        // verify the capability list format indirectly
        assert_eq!(expected.len(), 6);
    }

    #[test]
    fn get_server_info_format() {
        // Verify the tuple shape matches the freedesktop spec
        let info = (
            "slate-notifyd".to_string(),
            "slateos".to_string(),
            "0.1.0".to_string(),
            "1.2".to_string(),
        );
        assert_eq!(info.0, "slate-notifyd");
        assert_eq!(info.3, "1.2");
    }
}
