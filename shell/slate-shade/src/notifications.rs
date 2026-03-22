// Notification list view for the shade panel.
//
// Renders a scrollable list of notification groups. Each group has an
// app header row (icon + name + count + dismiss-all button) followed by
// individual notification cards. Groups are collapsible so the user can
// fold away verbose apps.
//
// AI summary subtext and smart-reply chips are shown when Rhea has produced
// them for a notification or group.

use iced::widget::{button, container, row, scrollable, text, Column, Row};
use iced::{Alignment, Color, Element, Length, Theme};

use slate_common::notifications::{Notification, Urgency};
use slate_common::Palette;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Layout constants
// ---------------------------------------------------------------------------

const CARD_PADDING: f32 = 12.0;
const CARD_SPACING: f32 = 8.0;
const GROUP_SPACING: f32 = 16.0;
const APP_HEADER_HEIGHT: f32 = 44.0;
const CHIP_PADDING: f32 = 8.0;
const CHIP_SPACING: f32 = 6.0;
const BORDER_RADIUS: f32 = 12.0;
const SUMMARY_FONT_SIZE: f32 = 13.0;
const BODY_FONT_SIZE: f32 = 14.0;
const APP_NAME_FONT_SIZE: f32 = 13.0;

// ---------------------------------------------------------------------------
// Notification group
// ---------------------------------------------------------------------------

/// A grouped set of notifications from the same app.
#[derive(Debug, Clone)]
pub struct NotificationGroup {
    /// Application name (the grouping key).
    pub app_name: String,
    /// Notifications in this group, newest first.
    pub notifications: Vec<Notification>,
    /// Whether the group is expanded (shows all cards) or collapsed (header only).
    pub expanded: bool,
    /// Optional AI summary for the whole group.
    pub ai_summary: Option<String>,
}

impl NotificationGroup {
    /// Create a new group for the given app.
    pub fn new(app_name: impl Into<String>) -> Self {
        Self {
            app_name: app_name.into(),
            notifications: Vec::new(),
            expanded: true,
            ai_summary: None,
        }
    }

    /// Number of notifications in this group.
    pub fn count(&self) -> usize {
        self.notifications.len()
    }

    /// Unread notification count.
    pub fn unread_count(&self) -> usize {
        self.notifications.iter().filter(|n| !n.read).count()
    }
}

// ---------------------------------------------------------------------------
// Messages produced by the notifications view
// ---------------------------------------------------------------------------

/// Actions the user can trigger from the notification list.
#[derive(Debug, Clone)]
pub enum NotifAction {
    /// Dismiss a single notification.
    Dismiss(Uuid),
    /// Dismiss all notifications from an app.
    DismissGroup(String),
    /// Toggle the expanded/collapsed state of a group.
    ToggleGroup(String),
    /// Invoke a notification action button.
    InvokeAction(Uuid, String),
    /// Send a smart-reply.
    SendReply(Uuid, String),
}

// ---------------------------------------------------------------------------
// View functions
// ---------------------------------------------------------------------------

/// Render the full notification list as a scrollable column.
///
/// Returns an empty placeholder when there are no notifications.
pub fn view_notifications<'a>(
    groups: &'a [NotificationGroup],
    smart_replies: &'a std::collections::HashMap<Uuid, Vec<String>>,
    palette: &Palette,
) -> Element<'a, NotifAction> {
    if groups.is_empty() {
        return container(
            text("No notifications")
                .size(BODY_FONT_SIZE)
                .color(muted_color(palette)),
        )
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into();
    }

    let mut list = Column::new().spacing(GROUP_SPACING);

    for group in groups {
        list = list.push(view_group(group, smart_replies, palette));
    }

    scrollable(list.padding(8.0)).into()
}

/// Render a single notification group (header + optional card list).
fn view_group<'a>(
    group: &'a NotificationGroup,
    smart_replies: &'a std::collections::HashMap<Uuid, Vec<String>>,
    palette: &Palette,
) -> Element<'a, NotifAction> {
    let mut col = Column::new().spacing(CARD_SPACING);

    // --- App header row ---
    col = col.push(view_group_header(group, palette));

    // --- AI summary ---
    if let Some(summary) = &group.ai_summary {
        col = col.push(view_ai_summary(summary, palette));
    }

    // --- Cards (only when expanded) ---
    if group.expanded {
        for notif in &group.notifications {
            let replies = smart_replies.get(&notif.uuid).map(|v| v.as_slice());
            col = col.push(view_notification_card(notif, replies, palette));
        }
    }

    col.into()
}

/// Render the app header row: name, unread count, expand/collapse, dismiss-all.
fn view_group_header<'a>(
    group: &'a NotificationGroup,
    palette: &Palette,
) -> Element<'a, NotifAction> {
    let expand_label = if group.expanded { "▲" } else { "▼" };
    let unread = group.unread_count();
    let count_label = if unread > 0 {
        format!("{} ({})", group.count(), unread)
    } else {
        group.count().to_string()
    };

    let header = row![
        text(&group.app_name)
            .size(APP_NAME_FONT_SIZE)
            .color(Palette::color_to_iced(palette.neutral))
            .width(Length::Fill),
        text(count_label)
            .size(APP_NAME_FONT_SIZE)
            .color(muted_color(palette)),
        button(text(expand_label).size(APP_NAME_FONT_SIZE))
            .on_press(NotifAction::ToggleGroup(group.app_name.clone()))
            .style(ghost_button_style),
        button(text("✕").size(APP_NAME_FONT_SIZE))
            .on_press(NotifAction::DismissGroup(group.app_name.clone()))
            .style(ghost_button_style),
    ]
    .align_y(Alignment::Center)
    .spacing(8.0);

    container(header)
        .height(Length::Fixed(APP_HEADER_HEIGHT))
        .padding([0.0, 4.0])
        .into()
}

/// Render an AI summary line beneath the group header.
fn view_ai_summary<'a>(summary: &'a str, palette: &Palette) -> Element<'a, NotifAction> {
    container(
        text(summary)
            .size(SUMMARY_FONT_SIZE)
            .color(muted_color(palette)),
    )
    .padding([0.0_f32, 4.0])
    .into()
}

/// Render a single notification card.
fn view_notification_card<'a>(
    notif: &'a Notification,
    smart_replies: Option<&'a [String]>,
    palette: &Palette,
) -> Element<'a, NotifAction> {
    let bg = card_bg_color(notif.urgency, notif.read, palette);
    let text_color = Palette::color_to_iced(palette.neutral);

    let mut card_col = Column::new().spacing(4.0);

    // Summary row with dismiss button
    let summary_row = row![
        text(&notif.summary)
            .size(BODY_FONT_SIZE)
            .color(text_color)
            .width(Length::Fill),
        button(text("✕").size(12.0))
            .on_press(NotifAction::Dismiss(notif.uuid))
            .style(ghost_button_style),
    ]
    .align_y(Alignment::Center)
    .spacing(4.0);

    card_col = card_col.push(summary_row);

    // Body text (if non-empty)
    if !notif.body.is_empty() {
        card_col = card_col.push(
            text(&notif.body)
                .size(BODY_FONT_SIZE - 1.0)
                .color(muted_color(palette)),
        );
    }

    // Action buttons (if any)
    if !notif.actions.is_empty() {
        let mut action_row = Row::new().spacing(CHIP_SPACING);
        for action in &notif.actions {
            let key = action.key.clone();
            let uuid = notif.uuid;
            action_row = action_row.push(
                button(text(&action.label).size(SUMMARY_FONT_SIZE))
                    .on_press(NotifAction::InvokeAction(uuid, key))
                    .style(chip_button_style),
            );
        }
        card_col = card_col.push(action_row);
    }

    // Smart reply chips
    if let Some(replies) = smart_replies {
        if !replies.is_empty() {
            let mut reply_row = Row::new().spacing(CHIP_SPACING);
            for reply in replies {
                let r = reply.clone();
                let uuid = notif.uuid;
                reply_row = reply_row.push(
                    button(text(reply).size(SUMMARY_FONT_SIZE))
                        .on_press(NotifAction::SendReply(uuid, r))
                        .style(chip_button_style),
                );
            }
            card_col = card_col.push(reply_row);
        }
    }

    container(card_col)
        .padding(CARD_PADDING)
        .width(Length::Fill)
        .style(move |_theme: &Theme| container::Style {
            background: Some(iced::Background::Color(bg)),
            border: iced::Border {
                radius: BORDER_RADIUS.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

// ---------------------------------------------------------------------------
// Style helpers
// ---------------------------------------------------------------------------

/// Background colour for a notification card based on urgency and read state.
fn card_bg_color(urgency: Urgency, read: bool, palette: &Palette) -> Color {
    let alpha = if read { 0.55_f32 } else { 0.78_f32 };
    match urgency {
        Urgency::Critical => Color::from_rgba8(183, 28, 28, alpha),
        Urgency::Normal | Urgency::Low => {
            let s = palette.surface;
            let base = Palette::color_to_iced(s);
            // Slightly lighter than the surface for visual separation.
            Color {
                r: (base.r + 0.08).min(1.0),
                g: (base.g + 0.08).min(1.0),
                b: (base.b + 0.08).min(1.0),
                a: alpha,
            }
        }
    }
}

/// Muted text colour (neutral with reduced opacity).
fn muted_color(palette: &Palette) -> Color {
    let c = Palette::color_to_iced(palette.neutral);
    Color { a: 0.6, ..c }
}

/// Ghost (transparent) button style for dismiss / expand toggles.
fn ghost_button_style(theme: &Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: None,
        text_color: theme.palette().text,
        border: iced::Border::default(),
        shadow: iced::Shadow::default(),
    }
}

/// Chip-style button for action and smart-reply buttons.
fn chip_button_style(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.palette();
    let bg = match status {
        button::Status::Hovered | button::Status::Pressed => iced::Background::Color(Color {
            a: 0.3,
            ..palette.primary
        }),
        _ => iced::Background::Color(Color {
            a: 0.15,
            ..palette.primary
        }),
    };
    button::Style {
        background: Some(bg),
        text_color: palette.primary,
        border: iced::Border {
            radius: (CHIP_PADDING * 2.0).into(),
            ..Default::default()
        },
        shadow: iced::Shadow::default(),
    }
}

// ---------------------------------------------------------------------------
// Notification group management helpers
// ---------------------------------------------------------------------------

/// Add or update a notification in the group list, creating a new group if needed.
///
/// Notifications are inserted at the front (newest first within each group).
pub fn upsert_notification(groups: &mut Vec<NotificationGroup>, notif: Notification) {
    if let Some(group) = groups.iter_mut().find(|g| g.app_name == notif.app_name) {
        // Update existing notification if UUID matches, otherwise prepend.
        if let Some(existing) = group
            .notifications
            .iter_mut()
            .find(|n| n.uuid == notif.uuid)
        {
            *existing = notif;
        } else {
            group.notifications.insert(0, notif);
        }
    } else {
        let mut group = NotificationGroup::new(&notif.app_name);
        group.notifications.push(notif);
        groups.push(group);
    }
}

/// Remove a notification by UUID from whichever group contains it.
///
/// Empty groups are removed. Returns `true` if a notification was removed.
pub fn remove_notification(groups: &mut Vec<NotificationGroup>, uuid: Uuid) -> bool {
    let mut removed = false;
    for group in groups.iter_mut() {
        let before = group.notifications.len();
        group.notifications.retain(|n| n.uuid != uuid);
        if group.notifications.len() < before {
            removed = true;
            break;
        }
    }
    // Prune empty groups.
    groups.retain(|g| !g.notifications.is_empty());
    removed
}

/// Toggle the expanded state for a group by app name.
pub fn toggle_group(groups: &mut [NotificationGroup], app_name: &str) {
    if let Some(group) = groups.iter_mut().find(|g| g.app_name == app_name) {
        group.expanded = !group.expanded;
    }
}

/// Remove all notifications from a named group. The group itself is also removed.
pub fn remove_group(groups: &mut Vec<NotificationGroup>, app_name: &str) {
    groups.retain(|g| g.app_name != app_name);
}

/// Set an AI summary on a group identified by group key.
pub fn set_ai_summary(groups: &mut [NotificationGroup], group_key: &str, summary: String) {
    if let Some(group) = groups.iter_mut().find(|g| g.app_name == group_key) {
        group.ai_summary = Some(summary);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
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
}
