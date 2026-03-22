// Tests for the main slate-shade application — state transitions and message handling.
//
// Kept in a separate file so main.rs stays under 500 lines.

use super::*;
use slate_common::notifications::Notification;

fn make_notif(app: &str) -> Notification {
    Notification::new(1, app, "Subject", "Body")
}

#[test]
fn slate_shade_default_has_empty_groups() {
    let s = SlateShade::default();
    assert!(s.groups.is_empty());
}

#[test]
fn slate_shade_default_shade_is_closed() {
    let s = SlateShade::default();
    assert!(!s.anim.is_open());
}

#[test]
fn slate_shade_open_message_sets_target() {
    let mut s = SlateShade::default();
    let _ = update::update_app(&mut s, Message::Open);
    assert!((s.anim.target - 1.0).abs() < 1e-9);
}

#[test]
fn slate_shade_close_message_sets_target_zero() {
    let mut s = SlateShade::default();
    s.anim.open();
    let _ = update::update_app(&mut s, Message::Close);
    assert!((s.anim.target - 0.0).abs() < 1e-9);
}

#[test]
fn slate_shade_notification_added_via_dbus() {
    let mut s = SlateShade::default();
    let notif = make_notif("TestApp");
    let _ = update::update_app(
        &mut s,
        Message::DbusEvent(dbus_listener::DbusEvent::NotificationAdded(notif)),
    );
    assert_eq!(s.groups.len(), 1);
    assert_eq!(s.groups[0].app_name, "TestApp");
}

#[test]
fn slate_shade_notification_dismissed_via_dbus() {
    let mut s = SlateShade::default();
    let notif = make_notif("App");
    let uuid = notif.uuid;
    let _ = update::update_app(
        &mut s,
        Message::DbusEvent(dbus_listener::DbusEvent::NotificationAdded(notif)),
    );
    assert_eq!(s.groups.len(), 1);
    let _ = update::update_app(
        &mut s,
        Message::DbusEvent(dbus_listener::DbusEvent::NotificationDismissed(uuid)),
    );
    assert!(s.groups.is_empty());
}

#[test]
fn slate_shade_palette_changed_via_dbus() {
    let mut s = SlateShade::default();
    let new_palette = Palette {
        primary: [255, 0, 0, 255],
        ..Palette::default()
    };
    let _ = update::update_app(
        &mut s,
        Message::DbusEvent(dbus_listener::DbusEvent::PaletteChanged(
            new_palette.clone(),
        )),
    );
    assert_eq!(s.palette, new_palette);
}

#[test]
fn slate_shade_qs_tile_toggle() {
    let mut s = SlateShade::default();
    let kind = quick_settings::TileKind::WiFi;
    let _ = update::update_app(&mut s, Message::QsAction(QsAction::TileToggled(kind)));
    assert!(s.qs.tiles.iter().find(|t| t.kind == kind).unwrap().active);
}

#[test]
fn slate_shade_qs_brightness_changed() {
    let mut s = SlateShade::default();
    let _ = update::update_app(&mut s, Message::QsAction(QsAction::BrightnessChanged(0.3)));
    assert!((s.qs.brightness - 0.3).abs() < 1e-4);
}

#[test]
fn slate_shade_hun_dismissed_by_action() {
    let mut s = SlateShade::default();
    let notif = {
        let mut n = make_notif("App");
        n.heads_up = true;
        n
    };
    let _ = update::update_app(
        &mut s,
        Message::DbusEvent(dbus_listener::DbusEvent::NotificationAdded(notif)),
    );
    assert!(s.hun.is_visible());
    let _ = update::update_app(&mut s, Message::HeadsUpAction(HeadsUpAction::Dismiss));
    assert!(!s.hun.is_visible());
}

#[test]
fn slate_shade_edge_gesture_begin_sets_position() {
    let mut s = SlateShade::default();
    let _ = update::update_app(
        &mut s,
        Message::DbusEvent(dbus_listener::DbusEvent::EdgeGesture {
            phase: "begin".to_string(),
            progress: 0.4,
            velocity: 100.0,
        }),
    );
    assert!((s.anim.position - 0.4).abs() < 1e-9);
}

#[test]
fn slate_shade_edge_gesture_end_commits() {
    let mut s = SlateShade::default();
    // A progress >= 0.5 should commit to open.
    let _ = update::update_app(
        &mut s,
        Message::DbusEvent(dbus_listener::DbusEvent::EdgeGesture {
            phase: "end".to_string(),
            progress: 0.6,
            velocity: 0.0,
        }),
    );
    assert!((s.anim.target - 1.0).abs() < 1e-9);
}

#[test]
fn slate_shade_edge_gesture_cancel_closes() {
    let mut s = SlateShade::default();
    s.anim.open();
    let _ = update::update_app(
        &mut s,
        Message::DbusEvent(dbus_listener::DbusEvent::EdgeGesture {
            phase: "cancel".to_string(),
            progress: 0.5,
            velocity: 0.0,
        }),
    );
    assert!((s.anim.target - 0.0).abs() < 1e-9);
}

#[test]
fn slate_shade_group_changed_zero_removes_group() {
    let mut s = SlateShade::default();
    let _ = update::update_app(
        &mut s,
        Message::DbusEvent(dbus_listener::DbusEvent::NotificationAdded(make_notif(
            "App",
        ))),
    );
    let _ = update::update_app(
        &mut s,
        Message::DbusEvent(dbus_listener::DbusEvent::GroupChanged("App".to_string(), 0)),
    );
    assert!(s.groups.is_empty());
}

#[test]
fn slate_shade_rhea_completion_done_logged() {
    // RheaCompletionDone carries the AI response text; the shade currently
    // logs it at debug level and discards it (future: update UI state).
    let mut s = SlateShade::default();
    let _ = update::update_app(
        &mut s,
        Message::DbusEvent(dbus_listener::DbusEvent::RheaCompletionDone(
            "AI summary".to_string(),
        )),
    );
    // No crash and no state corruption.
    assert!(s.groups.is_empty());
}

#[test]
fn slate_shade_rhea_completion_error_logged() {
    let mut s = SlateShade::default();
    let _ = update::update_app(
        &mut s,
        Message::DbusEvent(dbus_listener::DbusEvent::RheaCompletionError(
            "model unavailable".to_string(),
        )),
    );
    assert!(s.groups.is_empty());
}

#[test]
fn anim_tick_steps_animation() {
    let mut s = SlateShade::default();
    s.anim.open();
    let pos_before = s.anim.position;
    let _ = update::update_app(&mut s, Message::AnimTick);
    // After one tick toward target=1.0 position should have increased.
    assert!(s.anim.position >= pos_before);
}

#[test]
fn hun_tick_removes_expired_banner() {
    let mut s = SlateShade::default();
    let state = heads_up::HeadsUpState::with_duration(make_notif("App"), Duration::ZERO);
    s.hun.active = Some(state);
    let _ = update::update_app(&mut s, Message::HunTick);
    assert!(!s.hun.is_visible());
}

#[test]
fn notif_action_dismiss_removes_from_groups() {
    let mut s = SlateShade::default();
    let notif = make_notif("App");
    let uuid = notif.uuid;
    let _ = update::update_app(
        &mut s,
        Message::DbusEvent(dbus_listener::DbusEvent::NotificationAdded(notif)),
    );
    let _ = update::update_app(&mut s, Message::NotifAction(NotifAction::Dismiss(uuid)));
    assert!(s.groups.is_empty());
}

#[test]
fn notif_action_dismiss_group_removes_all() {
    let mut s = SlateShade::default();
    let _ = update::update_app(
        &mut s,
        Message::DbusEvent(dbus_listener::DbusEvent::NotificationAdded(make_notif(
            "App",
        ))),
    );
    let _ = update::update_app(
        &mut s,
        Message::NotifAction(NotifAction::DismissGroup("App".to_string())),
    );
    assert!(s.groups.is_empty());
}

#[test]
fn smart_replies_cleared_on_dismiss() {
    let mut s = SlateShade::default();
    let notif = make_notif("App");
    let uuid = notif.uuid;
    s.smart_replies.insert(uuid, vec!["OK".to_string()]);
    let _ = update::update_app(
        &mut s,
        Message::DbusEvent(dbus_listener::DbusEvent::NotificationDismissed(uuid)),
    );
    assert!(!s.smart_replies.contains_key(&uuid));
}

#[test]
fn default_layout_uses_configured_width() {
    let s = SlateShade::default();
    assert_eq!(s.layout, layout::detect_layout(DEFAULT_SCREEN_WIDTH));
}
