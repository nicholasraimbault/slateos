/// Notification types shared across the Slate OS notification stack.
///
/// These are the canonical data types for notifications. slate-notifyd
/// creates them from D-Bus freedesktop calls, slate-shade displays them,
/// and Rhea can read them for AI context.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Urgency
// ---------------------------------------------------------------------------

/// Maps to the freedesktop notification urgency byte (0/1/2).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Urgency {
    Low,
    #[default]
    Normal,
    Critical,
}

// ---------------------------------------------------------------------------
// NotificationAction
// ---------------------------------------------------------------------------

/// An action button exposed by a notification (e.g. "Reply", "Mark as read").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationAction {
    /// Machine-readable key sent back to the app on activation.
    pub key: String,
    /// Human-readable label shown on the button.
    pub label: String,
}

impl NotificationAction {
    pub fn new(key: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Notification
// ---------------------------------------------------------------------------

/// A single notification in the Slate OS notification system.
///
/// Created by slate-notifyd when a freedesktop Notify call arrives.
/// The `uuid` and `timestamp` are auto-generated at construction time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notification {
    /// Slate-internal unique ID (survives daemon restarts).
    pub uuid: Uuid,
    /// freedesktop notification ID (u32, recycled by the spec).
    pub fd_id: u32,
    pub app_name: String,
    pub app_icon: String,
    pub summary: String,
    pub body: String,
    pub actions: Vec<NotificationAction>,
    pub urgency: Urgency,
    /// freedesktop category hint (e.g. "email", "im.received").
    pub category: Option<String>,
    pub timestamp: DateTime<Utc>,
    /// Whether the user has seen/acknowledged this notification.
    pub read: bool,
    /// If true, the notification stays in the shade until dismissed.
    pub persistent: bool,
    /// Desktop entry of the sending app (for icon/launch resolution).
    pub desktop_entry: Option<String>,
    /// Whether this notification should appear as a heads-up banner.
    pub heads_up: bool,
    /// Grouping key for collapsing related notifications.
    pub group_key: Option<String>,
}

impl Notification {
    /// Create a new notification with auto-generated UUID and timestamp.
    pub fn new(
        fd_id: u32,
        app_name: impl Into<String>,
        summary: impl Into<String>,
        body: impl Into<String>,
    ) -> Self {
        Self {
            uuid: Uuid::new_v4(),
            fd_id,
            app_name: app_name.into(),
            app_icon: String::new(),
            summary: summary.into(),
            body: body.into(),
            actions: Vec::new(),
            urgency: Urgency::default(),
            category: None,
            timestamp: Utc::now(),
            read: false,
            persistent: false,
            desktop_entry: None,
            heads_up: true,
            group_key: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_new_generates_uuid() {
        let n = Notification::new(1, "app", "subject", "body text");
        // UUID should not be nil (all zeros).
        assert_ne!(n.uuid, Uuid::nil());
    }

    #[test]
    fn notification_new_generates_timestamp() {
        let before = Utc::now();
        let n = Notification::new(1, "app", "subject", "body");
        let after = Utc::now();
        assert!(n.timestamp >= before);
        assert!(n.timestamp <= after);
    }

    #[test]
    fn notification_new_sets_defaults() {
        let n = Notification::new(42, "Firefox", "New tab", "Tab opened");
        assert_eq!(n.fd_id, 42);
        assert_eq!(n.app_name, "Firefox");
        assert_eq!(n.summary, "New tab");
        assert_eq!(n.body, "Tab opened");
        assert_eq!(n.urgency, Urgency::Normal);
        assert!(!n.read);
        assert!(!n.persistent);
        assert!(n.heads_up);
        assert!(n.actions.is_empty());
        assert!(n.category.is_none());
        assert!(n.desktop_entry.is_none());
        assert!(n.group_key.is_none());
        assert!(n.app_icon.is_empty());
    }

    #[test]
    fn notification_two_have_distinct_uuids() {
        let a = Notification::new(1, "a", "s", "b");
        let b = Notification::new(2, "a", "s", "b");
        assert_ne!(a.uuid, b.uuid);
    }

    #[test]
    fn urgency_default_is_normal() {
        assert_eq!(Urgency::default(), Urgency::Normal);
    }

    #[test]
    fn notification_action_new() {
        let action = NotificationAction::new("reply", "Reply");
        assert_eq!(action.key, "reply");
        assert_eq!(action.label, "Reply");
    }

    #[test]
    fn notification_serialization_round_trip() {
        let mut n = Notification::new(1, "app", "summary", "body");
        n.actions
            .push(NotificationAction::new("dismiss", "Dismiss"));
        n.urgency = Urgency::Critical;
        n.category = Some("im.received".to_string());

        let json = serde_json::to_string(&n).expect("serialize");
        let back: Notification = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(n, back);
    }

    #[test]
    fn urgency_serialization_round_trip() {
        for urgency in [Urgency::Low, Urgency::Normal, Urgency::Critical] {
            let json = serde_json::to_string(&urgency).expect("serialize");
            let back: Urgency = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(urgency, back);
        }
    }
}
