// Tests for the notifications module.
//
// Kept in a separate file so notifications.rs stays under 500 lines.

use super::*;
use slate_common::notifications::Notification;

fn make_notif(app: &str) -> Notification {
    Notification::new(1, app, "Summary", "Body")
}

#[test]
fn notification_group_new_is_empty() {
    let g = NotificationGroup::new("App");
    assert_eq!(g.app_name, "App");
    assert!(g.notifications.is_empty());
    assert!(g.expanded);
    assert!(g.ai_summary.is_none());
}

#[test]
fn notification_group_count() {
    let mut g = NotificationGroup::new("App");
    g.notifications.push(make_notif("App"));
    g.notifications.push(make_notif("App"));
    assert_eq!(g.count(), 2);
}

#[test]
fn notification_group_unread_count() {
    let mut g = NotificationGroup::new("App");
    let mut n1 = make_notif("App");
    n1.read = true;
    let n2 = make_notif("App");
    g.notifications.push(n1);
    g.notifications.push(n2);
    assert_eq!(g.unread_count(), 1);
}

#[test]
fn upsert_notification_creates_group() {
    let mut groups: Vec<NotificationGroup> = Vec::new();
    let n = make_notif("TestApp");
    upsert_notification(&mut groups, n.clone());
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].app_name, "TestApp");
    assert_eq!(groups[0].notifications[0].uuid, n.uuid);
}

#[test]
fn upsert_notification_updates_existing() {
    let mut groups: Vec<NotificationGroup> = Vec::new();
    let mut n = make_notif("TestApp");
    upsert_notification(&mut groups, n.clone());
    n.summary = "Updated".to_string();
    upsert_notification(&mut groups, n.clone());
    // Should still be one group with one notification (updated, not duplicated).
    assert_eq!(groups[0].notifications.len(), 1);
    assert_eq!(groups[0].notifications[0].summary, "Updated");
}

#[test]
fn upsert_notification_adds_to_existing_group() {
    let mut groups: Vec<NotificationGroup> = Vec::new();
    upsert_notification(&mut groups, make_notif("App"));
    upsert_notification(&mut groups, make_notif("App"));
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].notifications.len(), 2);
}

#[test]
fn upsert_notification_creates_separate_groups_for_different_apps() {
    let mut groups: Vec<NotificationGroup> = Vec::new();
    upsert_notification(&mut groups, make_notif("App1"));
    upsert_notification(&mut groups, make_notif("App2"));
    assert_eq!(groups.len(), 2);
}

#[test]
fn remove_notification_by_uuid() {
    let mut groups: Vec<NotificationGroup> = Vec::new();
    let n = make_notif("App");
    let uuid = n.uuid;
    upsert_notification(&mut groups, n);
    let removed = remove_notification(&mut groups, uuid);
    assert!(removed);
    // Group should be cleaned up since it's empty.
    assert!(groups.is_empty());
}

#[test]
fn remove_notification_unknown_uuid_returns_false() {
    let mut groups: Vec<NotificationGroup> = Vec::new();
    upsert_notification(&mut groups, make_notif("App"));
    let unknown = Uuid::new_v4();
    assert!(!remove_notification(&mut groups, unknown));
    assert_eq!(groups.len(), 1);
}

#[test]
fn remove_group_removes_all_notifications() {
    let mut groups: Vec<NotificationGroup> = Vec::new();
    upsert_notification(&mut groups, make_notif("App1"));
    upsert_notification(&mut groups, make_notif("App2"));
    remove_group(&mut groups, "App1");
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].app_name, "App2");
}

#[test]
fn toggle_group_flips_expanded() {
    let mut groups: Vec<NotificationGroup> = Vec::new();
    upsert_notification(&mut groups, make_notif("App"));
    assert!(groups[0].expanded);
    toggle_group(&mut groups, "App");
    assert!(!groups[0].expanded);
    toggle_group(&mut groups, "App");
    assert!(groups[0].expanded);
}

#[test]
fn set_ai_summary_updates_group() {
    let mut groups: Vec<NotificationGroup> = Vec::new();
    upsert_notification(&mut groups, make_notif("App"));
    set_ai_summary(&mut groups, "App", "AI summary text".to_string());
    assert_eq!(groups[0].ai_summary.as_deref(), Some("AI summary text"));
}

#[test]
fn view_notifications_empty_produces_element() {
    let groups: Vec<NotificationGroup> = Vec::new();
    let replies = std::collections::HashMap::new();
    let palette = Palette::default();
    let _el: Element<'_, NotifAction> = view_notifications(&groups, &replies, &palette);
}

#[test]
fn view_notifications_with_data_produces_element() {
    let mut groups: Vec<NotificationGroup> = Vec::new();
    upsert_notification(&mut groups, make_notif("App"));
    let replies = std::collections::HashMap::new();
    let palette = Palette::default();
    let _el: Element<'_, NotifAction> = view_notifications(&groups, &replies, &palette);
}

#[test]
fn notif_action_is_debug_clone() {
    let action = NotifAction::Dismiss(Uuid::new_v4());
    let _cloned = action.clone();
    let _debug = format!("{action:?}");
}
