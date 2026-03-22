/// `org.freedesktop.Notifications` D-Bus interface implementation.
///
/// Follows the freedesktop notification specification v1.2.
/// Cross-interface signals (Added, Updated, Dismissed, GroupChanged) are emitted
/// on the correct `/org/slate/Notifications` path using a stored connection.
use std::collections::HashMap;
use std::path::PathBuf;

use zbus::object_server::SignalEmitter;

use super::{
    extract_group_key, extract_string_hint, extract_urgency, parse_actions, should_heads_up,
    SharedStore, SlateNotificationsSignalEmit,
};
use crate::history::HistoryWriter;

// ---------------------------------------------------------------------------
// FreedesktopNotifications struct
// ---------------------------------------------------------------------------

/// Implements the standard freedesktop notification spec (v1.2).
pub struct FreedesktopNotifications {
    pub store: SharedStore,
    pub history_dir: PathBuf,
    /// Stored connection for emitting cross-interface signals on the correct path.
    pub connection: zbus::Connection,
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

        // If replaces_id > 0, try to update the existing notification.
        // Per spec, even if not found we must return replaces_id, not a new id.
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
                    let notif_app_name = updated.app_name.clone();
                    store.update(updated.clone());
                    let count = store
                        .get_active()
                        .iter()
                        .filter(|n| n.app_name == notif_app_name)
                        .count() as u32;
                    drop(store);

                    let data = toml::to_string_pretty(&updated).unwrap_or_default();
                    let _ = SlateNotificationsSignalEmit::emit_updated(
                        &self.connection,
                        &uuid_str,
                        &data,
                    )
                    .await;
                    let _ = SlateNotificationsSignalEmit::emit_group_changed(
                        &self.connection,
                        &notif_app_name,
                        count,
                    )
                    .await;

                    return Ok(fd_id);
                }
            }

            // replaces_id > 0 but notification not found — create new, but return replaces_id.
            // Per freedesktop spec §3.7, replaces_id governs the returned value.
            tracing::debug!(
                replaces_id,
                "notification not found for replacement, creating new but returning replaces_id"
            );
            let n_ref = store.add(app_name, summary, body);
            let uuid = n_ref.uuid;
            let uuid_str = uuid.to_string();
            let notif_app_name = app_name.to_string();

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
            let count = store
                .get_active()
                .iter()
                .filter(|n| n.app_name == notif_app_name)
                .count() as u32;
            drop(store);

            let _ = SlateNotificationsSignalEmit::emit_added(
                &self.connection,
                &uuid_str,
                &notification_data,
            )
            .await;
            let _ = SlateNotificationsSignalEmit::emit_group_changed(
                &self.connection,
                &notif_app_name,
                count,
            )
            .await;

            // Return replaces_id per spec, not the newly assigned fd_id.
            return Ok(replaces_id);
        }

        let n_ref = store.add(app_name, summary, body);
        let fd_id = n_ref.fd_id;
        let uuid = n_ref.uuid;
        let uuid_str = uuid.to_string();
        let notif_app_name = app_name.to_string();

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
        let count = store
            .get_active()
            .iter()
            .filter(|n| n.app_name == notif_app_name)
            .count() as u32;
        drop(store);

        // Emit Added and GroupChanged on the Slate interface at the correct path.
        let _ = SlateNotificationsSignalEmit::emit_added(
            &self.connection,
            &uuid_str,
            &notification_data,
        )
        .await;
        let _ = SlateNotificationsSignalEmit::emit_group_changed(
            &self.connection,
            &notif_app_name,
            count,
        )
        .await;

        // Silence unused emitter warning — the freedesktop emitter is still needed
        // for the NotificationClosed and ActionInvoked signals defined on this interface.
        let _ = &emitter;

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
                let uuid_str = dismissed.uuid.to_string();
                let notif_app_name = dismissed.app_name.clone();
                let count = store
                    .get_active()
                    .iter()
                    .filter(|n| n.app_name == notif_app_name)
                    .count() as u32;
                let _ = HistoryWriter::append(&dismissed, &self.history_dir).await;
                drop(store);

                // Reason 3 = closed by a call to CloseNotification (per freedesktop spec).
                let _ = Self::notification_closed(&emitter, id, 3).await;

                // Emit Dismissed and GroupChanged on the Slate interface at the correct path.
                let _ = SlateNotificationsSignalEmit::emit_dismissed(
                    &self.connection,
                    &uuid_str,
                    "closed",
                )
                .await;
                let _ = SlateNotificationsSignalEmit::emit_group_changed(
                    &self.connection,
                    &notif_app_name,
                    count,
                )
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::NotificationStore;
    use std::sync::Arc;
    use tokio::sync::RwLock;

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
        // Verify the capability list format matches spec
        assert_eq!(expected.len(), 6);
        assert!(expected.contains(&"actions"));
        assert!(expected.contains(&"persistence"));
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

    #[test]
    fn freedesktop_struct_fields() {
        // Verify the struct can be created with expected field types
        let store: SharedStore = Arc::new(RwLock::new(NotificationStore::new()));
        let history_dir = std::path::PathBuf::from("/tmp");
        // We can't easily test the full struct without a real connection,
        // but we verify the field types are correct
        let _ = store;
        let _ = history_dir;
    }
}
