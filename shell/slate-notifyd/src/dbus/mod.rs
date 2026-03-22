/// D-Bus interfaces for the notification daemon.
///
/// Implements two interfaces:
/// 1. `org.freedesktop.Notifications` — the standard freedesktop spec
/// 2. `org.slate.Notifications` — custom Slate OS extensions
///
/// Both share the same `NotificationStore` via `Arc<RwLock<...>>`.
use std::collections::HashMap;

use slate_common::notifications::{NotificationAction, Urgency};
use tokio::sync::RwLock;

use std::sync::Arc;

use crate::store::NotificationStore;

mod freedesktop;
mod slate;

pub use freedesktop::FreedesktopNotifications;
pub use slate::SlateNotifications;

// ---------------------------------------------------------------------------
// Shared state type alias
// ---------------------------------------------------------------------------

/// Shared reference to the notification store.
pub type SharedStore = Arc<RwLock<NotificationStore>>;

// Freedesktop notifications object path — needed when emitting cross-interface signals.
pub(crate) const FREEDESKTOP_PATH: &str = "/org/freedesktop/Notifications";

// Slate notifications object path — needed when emitting cross-interface signals.
pub(crate) const SLATE_PATH: &str = "/org/slate/Notifications";

// ---------------------------------------------------------------------------
// Shared helper functions
// ---------------------------------------------------------------------------

/// Extract urgency from the hints dict (key "urgency", byte value 0/1/2).
pub(crate) fn extract_urgency(hints: &HashMap<String, zbus::zvariant::OwnedValue>) -> Urgency {
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
pub(crate) fn extract_group_key(
    hints: &HashMap<String, zbus::zvariant::OwnedValue>,
) -> Option<String> {
    extract_string_hint(hints, "x-slate-group-key")
}

/// Extract a string value from the hints dict.
pub(crate) fn extract_string_hint(
    hints: &HashMap<String, zbus::zvariant::OwnedValue>,
    key: &str,
) -> Option<String> {
    hints
        .get(key)
        .and_then(|val| <&str as TryFrom<_>>::try_from(val).ok().map(String::from))
}

/// Extract a boolean value from the hints dict.
///
/// Returns `false` when the key is absent or the value is not a boolean.
pub(crate) fn extract_bool_hint(
    hints: &HashMap<String, zbus::zvariant::OwnedValue>,
    key: &str,
) -> bool {
    hints
        .get(key)
        .and_then(|val| <bool as TryFrom<_>>::try_from(val).ok())
        .unwrap_or(false)
}

/// Determine whether a notification should appear as a heads-up banner.
pub(crate) fn should_heads_up(urgency: Urgency, dnd: bool) -> bool {
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
pub(crate) fn parse_actions(actions: &[String]) -> Vec<NotificationAction> {
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

// ---------------------------------------------------------------------------
// Helper to emit Slate interface signals from other interfaces.
//
// When a FreedesktopNotifications method needs to emit a signal on the
// org.slate.Notifications interface (different path), we must construct a new
// SignalEmitter with the correct path from the connection.
// ---------------------------------------------------------------------------

pub(crate) struct SlateNotificationsSignalEmit;

impl SlateNotificationsSignalEmit {
    /// Emit org.slate.Notifications.Added via the low-level emit API.
    pub(crate) async fn emit_added(
        conn: &zbus::Connection,
        uuid: &str,
        notification_data: &str,
    ) -> zbus::Result<()> {
        let emitter = zbus::object_server::SignalEmitter::new(conn, SLATE_PATH)?;
        emitter
            .emit(
                "org.slate.Notifications",
                "Added",
                &(uuid, notification_data),
            )
            .await
    }

    /// Emit org.slate.Notifications.Updated.
    pub(crate) async fn emit_updated(
        conn: &zbus::Connection,
        uuid: &str,
        notification_data: &str,
    ) -> zbus::Result<()> {
        let emitter = zbus::object_server::SignalEmitter::new(conn, SLATE_PATH)?;
        emitter
            .emit(
                "org.slate.Notifications",
                "Updated",
                &(uuid, notification_data),
            )
            .await
    }

    /// Emit org.slate.Notifications.Dismissed.
    pub(crate) async fn emit_dismissed(
        conn: &zbus::Connection,
        uuid: &str,
        reason: &str,
    ) -> zbus::Result<()> {
        let emitter = zbus::object_server::SignalEmitter::new(conn, SLATE_PATH)?;
        emitter
            .emit("org.slate.Notifications", "Dismissed", &(uuid, reason))
            .await
    }

    /// Emit org.slate.Notifications.GroupChanged.
    pub(crate) async fn emit_group_changed(
        conn: &zbus::Connection,
        app_name: &str,
        count: u32,
    ) -> zbus::Result<()> {
        let emitter = zbus::object_server::SignalEmitter::new(conn, SLATE_PATH)?;
        emitter
            .emit(
                "org.slate.Notifications",
                "GroupChanged",
                &(app_name, count),
            )
            .await
    }
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
    fn extract_bool_hint_absent_returns_false() {
        let hints: HashMap<String, zbus::zvariant::OwnedValue> = HashMap::new();
        assert!(!extract_bool_hint(&hints, "suppress-sound"));
    }
}
