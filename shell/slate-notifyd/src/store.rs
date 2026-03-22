/// In-memory notification store with persistence support.
///
/// Holds active notifications keyed by UUID, with a monotonic fd_id counter
/// that never resets. Supports DND state, dismissal, and TOML serialization
/// for crash recovery.
use std::collections::HashMap;

use slate_common::notifications::Notification;
use uuid::Uuid;

use crate::error::StoreError;

// ---------------------------------------------------------------------------
// Persistence wrapper
// ---------------------------------------------------------------------------

/// On-disk representation of the active notification store.
/// Wraps a Vec because TOML requires a table or array at the top level.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct PersistedStore {
    next_fd_id: u32,
    dnd: bool,
    notifications: Vec<Notification>,
}

// ---------------------------------------------------------------------------
// NotificationStore
// ---------------------------------------------------------------------------

/// Core in-memory store for active notifications.
pub struct NotificationStore {
    /// Active notifications keyed by UUID for O(1) lookup.
    notifications: HashMap<Uuid, Notification>,
    /// Monotonic counter for freedesktop IDs. Never resets.
    next_fd_id: u32,
    /// Do-not-disturb mode.
    pub dnd: bool,
    /// Set when a mutation occurs; cleared after persistence flush.
    pub dirty: bool,
}

impl NotificationStore {
    /// Create an empty store.
    pub fn new() -> Self {
        Self {
            notifications: HashMap::new(),
            next_fd_id: 1,
            dnd: false,
            dirty: false,
        }
    }

    /// Allocate the next freedesktop ID and advance the counter.
    pub fn next_fd_id(&mut self) -> u32 {
        let id = self.next_fd_id;
        self.next_fd_id = self.next_fd_id.wrapping_add(1);
        id
    }

    /// Insert a new notification, returning a reference to the stored value.
    pub fn add(
        &mut self,
        app_name: impl Into<String>,
        summary: impl Into<String>,
        body: impl Into<String>,
    ) -> &Notification {
        let fd_id = self.next_fd_id();
        let n = Notification::new(fd_id, app_name, summary, body);
        let uuid = n.uuid;
        self.notifications.insert(uuid, n);
        self.dirty = true;
        self.notifications.get(&uuid).expect("just inserted")
    }

    /// Return all active (non-dismissed) notifications.
    pub fn get_active(&self) -> Vec<&Notification> {
        self.notifications.values().collect()
    }

    /// Look up a notification by its Slate UUID.
    pub fn get_by_uuid(&self, uuid: Uuid) -> Option<&Notification> {
        self.notifications.get(&uuid)
    }

    /// Look up a notification by its freedesktop ID.
    pub fn get_by_fd_id(&self, id: u32) -> Option<&Notification> {
        self.notifications.values().find(|n| n.fd_id == id)
    }

    /// Remove a notification by UUID, returning it if found.
    pub fn dismiss(&mut self, uuid: Uuid) -> Option<Notification> {
        let removed = self.notifications.remove(&uuid);
        if removed.is_some() {
            self.dirty = true;
        }
        removed
    }

    /// Dismiss all non-persistent notifications. Returns dismissed list.
    pub fn dismiss_all(&mut self) -> Vec<Notification> {
        let to_remove: Vec<Uuid> = self
            .notifications
            .values()
            .filter(|n| !n.persistent)
            .map(|n| n.uuid)
            .collect();

        let mut dismissed = Vec::with_capacity(to_remove.len());
        for uuid in to_remove {
            if let Some(n) = self.notifications.remove(&uuid) {
                dismissed.push(n);
            }
        }
        if !dismissed.is_empty() {
            self.dirty = true;
        }
        dismissed
    }

    /// Dismiss all notifications from a given app. Returns dismissed list.
    pub fn dismiss_group(&mut self, app_name: &str) -> Vec<Notification> {
        let to_remove: Vec<Uuid> = self
            .notifications
            .values()
            .filter(|n| n.app_name == app_name)
            .map(|n| n.uuid)
            .collect();

        let mut dismissed = Vec::with_capacity(to_remove.len());
        for uuid in to_remove {
            if let Some(n) = self.notifications.remove(&uuid) {
                dismissed.push(n);
            }
        }
        if !dismissed.is_empty() {
            self.dirty = true;
        }
        dismissed
    }

    /// Replace a notification in the store (e.g. after updating fields).
    pub fn update(&mut self, notification: Notification) {
        self.notifications.insert(notification.uuid, notification);
        self.dirty = true;
    }

    /// Mark a notification as read.
    pub fn mark_read(&mut self, uuid: Uuid) -> bool {
        if let Some(n) = self.notifications.get_mut(&uuid) {
            n.read = true;
            self.dirty = true;
            true
        } else {
            false
        }
    }

    /// Serialize active notifications to a TOML string.
    ///
    /// The caller is responsible for writing the string to disk, preferably
    /// using `tokio::fs::write` to avoid blocking the async executor.
    pub fn serialize_active(&self) -> Result<String, StoreError> {
        let persisted = PersistedStore {
            next_fd_id: self.next_fd_id,
            dnd: self.dnd,
            notifications: self.notifications.values().cloned().collect(),
        };
        Ok(toml::to_string_pretty(&persisted)?)
    }

    /// Deserialize active notifications from a TOML string.
    ///
    /// The caller is responsible for reading the string from disk, preferably
    /// using `tokio::fs::read_to_string` to avoid blocking the async executor.
    pub fn deserialize_active(content: &str) -> Result<Self, StoreError> {
        let persisted: PersistedStore = toml::from_str(content)?;

        let mut notifications = HashMap::new();
        for n in persisted.notifications {
            notifications.insert(n.uuid, n);
        }

        Ok(Self {
            notifications,
            next_fd_id: persisted.next_fd_id,
            dnd: persisted.dnd,
            dirty: false,
        })
    }
}

impl Default for NotificationStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_store_is_empty() {
        let store = NotificationStore::new();
        assert!(store.get_active().is_empty());
        assert!(!store.dnd);
        assert!(!store.dirty);
    }

    #[test]
    fn add_returns_notification_with_fd_id() {
        let mut store = NotificationStore::new();
        let n = store.add("Firefox", "New tab", "Tab opened");
        assert_eq!(n.fd_id, 1);
        assert_eq!(n.app_name, "Firefox");
        assert_eq!(n.summary, "New tab");
        assert_eq!(n.body, "Tab opened");
    }

    #[test]
    fn add_increments_fd_id_monotonically() {
        let mut store = NotificationStore::new();
        let n1 = store.add("app", "s1", "b1");
        assert_eq!(n1.fd_id, 1);
        let n2 = store.add("app", "s2", "b2");
        assert_eq!(n2.fd_id, 2);
        let n3 = store.add("app", "s3", "b3");
        assert_eq!(n3.fd_id, 3);
    }

    #[test]
    fn add_marks_store_dirty() {
        let mut store = NotificationStore::new();
        assert!(!store.dirty);
        let _ = store.add("app", "s", "b");
        assert!(store.dirty);
    }

    #[test]
    fn get_active_returns_all() {
        let mut store = NotificationStore::new();
        let _ = store.add("a", "s1", "b1");
        let _ = store.add("b", "s2", "b2");
        assert_eq!(store.get_active().len(), 2);
    }

    #[test]
    fn get_by_uuid_found() {
        let mut store = NotificationStore::new();
        let uuid = store.add("app", "s", "b").uuid;
        assert!(store.get_by_uuid(uuid).is_some());
    }

    #[test]
    fn get_by_uuid_not_found() {
        let store = NotificationStore::new();
        assert!(store.get_by_uuid(Uuid::new_v4()).is_none());
    }

    #[test]
    fn get_by_fd_id_found() {
        let mut store = NotificationStore::new();
        let _ = store.add("app", "s", "b");
        assert!(store.get_by_fd_id(1).is_some());
    }

    #[test]
    fn get_by_fd_id_not_found() {
        let store = NotificationStore::new();
        assert!(store.get_by_fd_id(999).is_none());
    }

    #[test]
    fn dismiss_removes_and_returns() {
        let mut store = NotificationStore::new();
        let uuid = store.add("app", "s", "b").uuid;
        let dismissed = store.dismiss(uuid);
        assert!(dismissed.is_some());
        assert_eq!(dismissed.as_ref().map(|n| n.uuid), Some(uuid));
        assert!(store.get_by_uuid(uuid).is_none());
    }

    #[test]
    fn dismiss_nonexistent_returns_none() {
        let mut store = NotificationStore::new();
        assert!(store.dismiss(Uuid::new_v4()).is_none());
    }

    #[test]
    fn dismiss_all_skips_persistent() {
        let mut store = NotificationStore::new();
        let uuid1 = store.add("a", "s1", "b1").uuid;

        // Make second notification persistent
        let uuid2 = store.add("b", "s2", "b2").uuid;
        if let Some(n) = store.notifications.get_mut(&uuid2) {
            n.persistent = true;
        }

        let dismissed = store.dismiss_all();
        assert_eq!(dismissed.len(), 1);
        assert_eq!(dismissed[0].uuid, uuid1);
        // Persistent one remains
        assert!(store.get_by_uuid(uuid2).is_some());
    }

    #[test]
    fn dismiss_group_removes_all_from_app() {
        let mut store = NotificationStore::new();
        let _ = store.add("Firefox", "s1", "b1");
        let _ = store.add("Firefox", "s2", "b2");
        let _ = store.add("Signal", "s3", "b3");

        let dismissed = store.dismiss_group("Firefox");
        assert_eq!(dismissed.len(), 2);
        assert_eq!(store.get_active().len(), 1);
    }

    #[test]
    fn dismiss_group_empty_when_no_match() {
        let mut store = NotificationStore::new();
        let _ = store.add("Firefox", "s", "b");
        let dismissed = store.dismiss_group("Signal");
        assert!(dismissed.is_empty());
        assert_eq!(store.get_active().len(), 1);
    }

    #[test]
    fn update_replaces_notification() {
        let mut store = NotificationStore::new();
        let uuid = store.add("app", "old summary", "b").uuid;

        let mut updated = store.get_by_uuid(uuid).cloned().expect("exists");
        updated.summary = "new summary".to_string();
        store.update(updated);

        let fetched = store.get_by_uuid(uuid).expect("still exists");
        assert_eq!(fetched.summary, "new summary");
    }

    #[test]
    fn mark_read_sets_flag() {
        let mut store = NotificationStore::new();
        let uuid = store.add("app", "s", "b").uuid;
        assert!(!store.get_by_uuid(uuid).expect("exists").read);

        assert!(store.mark_read(uuid));
        assert!(store.get_by_uuid(uuid).expect("exists").read);
    }

    #[test]
    fn mark_read_nonexistent_returns_false() {
        let mut store = NotificationStore::new();
        assert!(!store.mark_read(Uuid::new_v4()));
    }

    #[test]
    fn fd_id_survives_dismissals() {
        let mut store = NotificationStore::new();
        let uuid1 = store.add("a", "s1", "b1").uuid;
        assert_eq!(store.get_by_uuid(uuid1).map(|n| n.fd_id), Some(1));

        store.dismiss(uuid1);
        let n2 = store.add("b", "s2", "b2");
        // fd_id should be 2, not 1 — counter never resets
        assert_eq!(n2.fd_id, 2);
    }

    #[test]
    fn serialize_and_deserialize_round_trip() {
        let mut store = NotificationStore::new();
        let _ = store.add("Firefox", "Tab", "Opened");
        let _ = store.add("Signal", "Message", "Hello");
        store.dnd = true;
        let next_fd_id = store.next_fd_id;

        let content = store.serialize_active().expect("serialize");
        let loaded = NotificationStore::deserialize_active(&content).expect("deserialize");
        assert_eq!(loaded.get_active().len(), 2);
        assert!(loaded.dnd);
        assert!(!loaded.dirty);
        // fd_id counter should be preserved
        assert_eq!(loaded.next_fd_id, next_fd_id);
    }

    #[test]
    fn deserialize_invalid_returns_error() {
        let result = NotificationStore::deserialize_active("not valid toml {{{{");
        assert!(result.is_err());
    }

    #[test]
    fn dnd_toggle() {
        let mut store = NotificationStore::new();
        assert!(!store.dnd);
        store.dnd = true;
        assert!(store.dnd);
    }

    #[test]
    fn default_is_same_as_new() {
        let store = NotificationStore::default();
        assert!(store.get_active().is_empty());
        assert!(!store.dnd);
    }

    #[test]
    fn dismiss_marks_dirty() {
        let mut store = NotificationStore::new();
        let uuid = store.add("a", "s", "b").uuid;
        store.dirty = false;
        store.dismiss(uuid);
        assert!(store.dirty);
    }

    #[test]
    fn dismiss_nonexistent_does_not_mark_dirty() {
        let mut store = NotificationStore::new();
        store.dirty = false;
        store.dismiss(Uuid::new_v4());
        assert!(!store.dirty);
    }
}
