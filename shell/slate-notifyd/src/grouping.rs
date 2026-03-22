/// Notification grouping logic.
///
/// Groups notifications by (app_name, group_key) for display in the
/// notification shade. If group_key is None, all notifications from
/// the same app_name are grouped together.
use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use slate_common::notifications::Notification;

// ---------------------------------------------------------------------------
// NotificationGroup
// ---------------------------------------------------------------------------

/// A group of related notifications from the same app.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationGroup {
    /// The app that sent these notifications.
    pub app_name: String,
    /// Optional sub-grouping key (e.g. conversation ID).
    pub group_key: Option<String>,
    /// Notifications in this group, sorted newest first.
    pub notifications: Vec<Notification>,
    /// Total number of notifications in the group.
    pub count: usize,
}

// ---------------------------------------------------------------------------
// Grouping function
// ---------------------------------------------------------------------------

/// Group a slice of notifications by (app_name, group_key).
///
/// - If group_key is None, groups by app_name alone.
/// - Groups are sorted by the newest notification timestamp (newest first).
/// - Within each group, notifications are sorted newest first.
pub fn group_notifications(notifications: &[Notification]) -> Vec<NotificationGroup> {
    // Build groups keyed by (app_name, group_key)
    let mut map: HashMap<(String, Option<String>), Vec<Notification>> = HashMap::new();

    for n in notifications {
        let key = (n.app_name.clone(), n.group_key.clone());
        map.entry(key).or_default().push(n.clone());
    }

    let mut groups: Vec<NotificationGroup> = map
        .into_iter()
        .map(|((app_name, group_key), mut notifs)| {
            // Sort within group: newest first
            notifs.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
            let count = notifs.len();
            NotificationGroup {
                app_name,
                group_key,
                notifications: notifs,
                count,
            }
        })
        .collect();

    // Sort groups by newest notification timestamp (newest group first)
    groups.sort_by(|a, b| {
        let a_newest = a.notifications.first().map(|n| n.timestamp);
        let b_newest = b.notifications.first().map(|n| n.timestamp);
        b_newest.cmp(&a_newest)
    });

    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};

    /// Helper to create a notification with a specific timestamp offset.
    fn make_notification(
        app: &str,
        summary: &str,
        group_key: Option<&str>,
        offset_secs: i64,
    ) -> Notification {
        let mut n = Notification::new(0, app, summary, "body");
        n.group_key = group_key.map(String::from);
        n.timestamp = Utc::now() + Duration::seconds(offset_secs);
        n
    }

    #[test]
    fn empty_input_returns_empty() {
        let groups = group_notifications(&[]);
        assert!(groups.is_empty());
    }

    #[test]
    fn single_notification_forms_one_group() {
        let n = make_notification("Firefox", "Tab opened", None, 0);
        let groups = group_notifications(&[n]);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].app_name, "Firefox");
        assert_eq!(groups[0].count, 1);
    }

    #[test]
    fn same_app_no_group_key_grouped_together() {
        let n1 = make_notification("Signal", "Alice", None, -2);
        let n2 = make_notification("Signal", "Bob", None, -1);
        let groups = group_notifications(&[n1, n2]);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].app_name, "Signal");
        assert_eq!(groups[0].count, 2);
    }

    #[test]
    fn different_apps_form_separate_groups() {
        let n1 = make_notification("Firefox", "Tab", None, 0);
        let n2 = make_notification("Signal", "Msg", None, 0);
        let groups = group_notifications(&[n1, n2]);
        assert_eq!(groups.len(), 2);
    }

    #[test]
    fn same_app_different_group_keys_separate() {
        let n1 = make_notification("Signal", "Alice", Some("conv-1"), 0);
        let n2 = make_notification("Signal", "Bob", Some("conv-2"), 0);
        let groups = group_notifications(&[n1, n2]);
        assert_eq!(groups.len(), 2);
    }

    #[test]
    fn same_app_same_group_key_merged() {
        let n1 = make_notification("Signal", "Alice: Hi", Some("conv-1"), -1);
        let n2 = make_notification("Signal", "Alice: Hey", Some("conv-1"), 0);
        let groups = group_notifications(&[n1, n2]);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].count, 2);
        assert_eq!(groups[0].group_key, Some("conv-1".to_string()));
    }

    #[test]
    fn groups_sorted_newest_first() {
        let old = make_notification("OldApp", "Old", None, -100);
        let new = make_notification("NewApp", "New", None, 0);
        let groups = group_notifications(&[old, new]);
        assert_eq!(groups[0].app_name, "NewApp");
        assert_eq!(groups[1].app_name, "OldApp");
    }

    #[test]
    fn within_group_sorted_newest_first() {
        let older = make_notification("App", "First", None, -10);
        let newer = make_notification("App", "Second", None, 0);
        let groups = group_notifications(&[older, newer]);
        assert_eq!(groups[0].notifications[0].summary, "Second");
        assert_eq!(groups[0].notifications[1].summary, "First");
    }

    #[test]
    fn count_matches_notifications_len() {
        let n1 = make_notification("App", "A", None, 0);
        let n2 = make_notification("App", "B", None, 1);
        let n3 = make_notification("App", "C", None, 2);
        let groups = group_notifications(&[n1, n2, n3]);
        assert_eq!(groups[0].count, groups[0].notifications.len());
        assert_eq!(groups[0].count, 3);
    }

    #[test]
    fn mixed_group_keys_and_none() {
        // App with some grouped, some not
        let n1 = make_notification("Signal", "Alice", Some("conv-1"), 0);
        let n2 = make_notification("Signal", "System", None, -1);
        let groups = group_notifications(&[n1, n2]);
        // "conv-1" and None are different keys, so 2 groups
        assert_eq!(groups.len(), 2);
    }
}
