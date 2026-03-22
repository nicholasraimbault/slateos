// slate-shade — notification shade and quick settings overlay for Slate OS.
//
// A top-anchored layer-shell panel that the user pulls down from the top
// edge. It shows grouped notifications (with AI summaries from Rhea),
// quick settings tiles, brightness/volume sliders, and heads-up banners.
//
// The panel is gesture-driven: TouchFlow sends EdgeGesture signals that
// drive a spring animation via `slate_common::physics::Spring`.
//
// Most D-Bus infrastructure and layer-shell wiring is Linux-only.
// On macOS/dev machines the shade falls back to a plain windowed iced app.
#![allow(dead_code)] // D-Bus and gesture wiring is Linux-only
mod dbus_listener;
mod heads_up;
mod layout;
mod notifications;
mod quick_settings;
mod shade;

use std::collections::HashMap;
use std::time::Duration;

use iced::widget::column;
use iced::{Element, Subscription, Task, Theme};

use heads_up::{HeadsUp, HeadsUpAction};
use layout::{detect_layout, LayoutMode};
use notifications::{
    remove_group, remove_notification, set_ai_summary, toggle_group, upsert_notification,
    NotifAction, NotificationGroup,
};
use quick_settings::{QsAction, QuickSettingsState};
use shade::{ShadeAction, ShadeAnimState};
use slate_common::theme::create_theme;
use slate_common::Palette;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default screen height (logical px) when the compositor size is not yet known.
const DEFAULT_SCREEN_HEIGHT: f32 = 900.0;

/// Default screen width used for initial layout detection.
const DEFAULT_SCREEN_WIDTH: u32 = 1280;

/// Animation tick interval for the shade spring.
const ANIM_TICK_MS: u64 = 16; // ~60 fps

/// HUN auto-dismiss tick interval.
const HUN_TICK_MS: u64 = 250;

/// Layer-shell surface size (width is set by the compositor, height is overlay).
#[cfg(target_os = "linux")]
const INITIAL_SURFACE_HEIGHT: u32 = 1;

// ---------------------------------------------------------------------------
// Application state
// ---------------------------------------------------------------------------

struct SlateShade {
    /// Notification groups, newest group last.
    groups: Vec<NotificationGroup>,
    /// Smart-reply chips per notification UUID.
    smart_replies: HashMap<Uuid, Vec<String>>,
    /// Quick settings state.
    qs: QuickSettingsState,
    /// Shade pull-down animation state.
    anim: ShadeAnimState,
    /// Heads-up banner state.
    hun: HeadsUp,
    /// Current layout mode (phone vs tablet/desktop).
    layout: LayoutMode,
    /// Current system palette.
    palette: Palette,
    /// Logical screen height used to compute shade pixel height.
    screen_height: f32,
}

impl Default for SlateShade {
    fn default() -> Self {
        Self {
            groups: Vec::new(),
            smart_replies: HashMap::new(),
            qs: QuickSettingsState::default(),
            anim: ShadeAnimState::default(),
            hun: HeadsUp::default(),
            layout: detect_layout(DEFAULT_SCREEN_WIDTH),
            palette: Palette::default(),
            screen_height: DEFAULT_SCREEN_HEIGHT,
        }
    }
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
#[allow(dead_code)]
enum Message {
    // D-Bus events
    DbusEvent(dbus_listener::DbusEvent),
    // Shade open/close
    Open,
    Close,
    // Notification actions (from the shade list)
    NotifAction(NotifAction),
    // Quick settings actions
    QsAction(QsAction),
    // Heads-up banner actions
    HeadsUpAction(HeadsUpAction),
    // Animation tick (drives the spring)
    AnimTick,
    // HUN auto-dismiss tick
    HunTick,
}

// ---------------------------------------------------------------------------
// Update
// ---------------------------------------------------------------------------

fn update_app(app: &mut SlateShade, message: Message) -> Task<Message> {
    match message {
        Message::DbusEvent(event) => handle_dbus_event(app, event),
        Message::Open => {
            app.anim.open();
        }
        Message::Close => {
            app.anim.close();
        }
        Message::NotifAction(action) => handle_notif_action(app, action),
        Message::QsAction(action) => handle_qs_action(app, action),
        Message::HeadsUpAction(action) => handle_hun_action(app, action),
        Message::AnimTick => {
            if !app.anim.is_settled() {
                app.anim.step(ANIM_TICK_MS as f64 / 1000.0);
            }
        }
        Message::HunTick => {
            app.hun.tick();
        }
    }
    Task::none()
}

fn handle_dbus_event(app: &mut SlateShade, event: dbus_listener::DbusEvent) {
    match event {
        dbus_listener::DbusEvent::NotificationAdded(notif) => {
            if notif.heads_up {
                app.hun.show(notif.clone());
            }
            upsert_notification(&mut app.groups, notif);
        }
        dbus_listener::DbusEvent::NotificationUpdated(notif) => {
            upsert_notification(&mut app.groups, notif);
        }
        dbus_listener::DbusEvent::NotificationDismissed(uuid) => {
            remove_notification(&mut app.groups, uuid);
            app.smart_replies.remove(&uuid);
        }
        dbus_listener::DbusEvent::GroupChanged(app_name, count) => {
            if count == 0 {
                remove_group(&mut app.groups, &app_name);
            }
        }
        dbus_listener::DbusEvent::AiSummaryReady(group_key, summary) => {
            set_ai_summary(&mut app.groups, &group_key, summary);
        }
        dbus_listener::DbusEvent::SmartRepliesReady(uuid, replies) => {
            app.smart_replies.insert(uuid, replies);
        }
        dbus_listener::DbusEvent::EdgeGesture {
            phase,
            progress,
            velocity,
        } => match phase.as_str() {
            "begin" | "update" => {
                app.anim.set_from_gesture(progress, velocity);
            }
            "end" => {
                app.anim.set_from_gesture(progress, velocity);
                app.anim.commit_gesture();
            }
            "cancel" => {
                app.anim.close();
            }
            _ => {}
        },
        dbus_listener::DbusEvent::PaletteChanged(palette) => {
            app.palette = palette;
        }
    }
}

fn handle_notif_action(app: &mut SlateShade, action: NotifAction) {
    match action {
        NotifAction::Dismiss(uuid) => {
            remove_notification(&mut app.groups, uuid);
            app.smart_replies.remove(&uuid);
            // D-Bus dismiss call would go here in a full implementation.
        }
        NotifAction::DismissGroup(app_name) => {
            remove_group(&mut app.groups, &app_name);
        }
        NotifAction::ToggleGroup(app_name) => {
            toggle_group(&mut app.groups, &app_name);
        }
        NotifAction::InvokeAction(_uuid, _key) => {
            // D-Bus invoke_action call would go here.
        }
        NotifAction::SendReply(_uuid, _reply) => {
            // Smart reply dispatch would go here.
        }
    }
}

fn handle_qs_action(app: &mut SlateShade, action: QsAction) {
    match action {
        QsAction::TileToggled(kind) => {
            app.qs.toggle_tile(kind);
        }
        QsAction::BrightnessChanged(v) => {
            app.qs.set_brightness(v);
        }
        QsAction::VolumeChanged(v) => {
            app.qs.set_volume(v);
        }
    }
}

fn handle_hun_action(app: &mut SlateShade, action: HeadsUpAction) {
    match action {
        HeadsUpAction::Dismiss => {
            app.hun.dismiss();
        }
        HeadsUpAction::InvokeAction(_uuid, _key) => {
            app.hun.dismiss();
            // D-Bus invoke_action call would go here.
        }
    }
}

// ---------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------

fn view_app(app: &SlateShade) -> Element<'_, Message> {
    // Heads-up banner sits above the shade, always at the top.
    let hun = heads_up::view_heads_up(&app.hun, &app.palette).map(Message::HeadsUpAction);

    let shade_el = shade::view_shade(
        &app.anim,
        &app.groups,
        &app.smart_replies,
        &app.qs,
        app.layout,
        &app.palette,
        app.screen_height,
    )
    .map(|action| match action {
        ShadeAction::Notification(a) => Message::NotifAction(a),
        ShadeAction::QuickSettings(a) => Message::QsAction(a),
        ShadeAction::CloseRequested => Message::Close,
    });

    column![hun, shade_el].into()
}

// ---------------------------------------------------------------------------
// Subscription
// ---------------------------------------------------------------------------

fn subscription_app(app: &SlateShade) -> Subscription<Message> {
    let dbus_sub = dbus_subscription();

    let mut subs = vec![dbus_sub];

    // Only run the animation ticker while the shade is not settled.
    if !app.anim.is_settled() {
        subs.push(
            iced::time::every(Duration::from_millis(ANIM_TICK_MS)).map(|_| Message::AnimTick),
        );
    }

    // Only run the HUN tick while a banner is visible.
    if app.hun.is_visible() {
        subs.push(iced::time::every(Duration::from_millis(HUN_TICK_MS)).map(|_| Message::HunTick));
    }

    Subscription::batch(subs)
}

/// Build a D-Bus subscription using the `iced::stream::channel` pattern.
fn dbus_subscription() -> Subscription<Message> {
    use iced::futures::SinkExt;

    let stream = iced::stream::channel(64, |mut output| async move {
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(dbus_listener::run(event_tx));

        loop {
            match event_rx.recv().await {
                Some(event) => {
                    if output.send(Message::DbusEvent(event)).await.is_err() {
                        tracing::warn!("slate-shade: D-Bus subscription channel closed");
                        break;
                    }
                }
                None => {
                    tracing::warn!("slate-shade: D-Bus event channel closed unexpectedly");
                    std::future::pending::<()>().await;
                }
            }
        }
    });

    Subscription::run_with_id("slate-shade-dbus", stream)
}

// ---------------------------------------------------------------------------
// Linux layer-shell entry point
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
impl TryInto<iced_layershell::actions::LayershellCustomActions> for Message {
    type Error = Self;
    fn try_into(self) -> Result<iced_layershell::actions::LayershellCustomActions, Self> {
        Err(self)
    }
}

#[cfg(target_os = "linux")]
impl iced_layershell::Application for SlateShade {
    type Executor = iced::executor::Default;
    type Message = Message;
    type Theme = Theme;
    type Flags = ();

    fn new(_flags: ()) -> (Self, Task<Message>) {
        (Self::default(), Task::none())
    }

    fn namespace(&self) -> String {
        "slate-shade".to_string()
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        update_app(self, message)
    }

    fn view(&self) -> Element<'_, Message, Theme, iced::Renderer> {
        view_app(self)
    }

    fn theme(&self) -> Theme {
        create_theme(&self.palette)
    }

    fn subscription(&self) -> Subscription<Message> {
        subscription_app(self)
    }
}

// ---------------------------------------------------------------------------
// macOS fallback: plain iced app (no layer shell)
// ---------------------------------------------------------------------------

#[cfg(not(target_os = "linux"))]
impl SlateShade {
    fn title(&self) -> String {
        "Slate Shade".to_string()
    }

    fn update_iced(&mut self, message: Message) -> Task<Message> {
        update_app(self, message)
    }

    fn view_iced(&self) -> Element<'_, Message> {
        view_app(self)
    }

    fn theme_iced(&self) -> Theme {
        create_theme(&self.palette)
    }

    fn subscription_iced(&self) -> Subscription<Message> {
        subscription_app(self)
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("slate_shade=info,warn")
        .init();

    tracing::info!("slate-shade starting");

    run_app()
}

#[cfg(not(target_os = "linux"))]
fn run_app() -> anyhow::Result<()> {
    iced::application(
        SlateShade::title,
        SlateShade::update_iced,
        SlateShade::view_iced,
    )
    .subscription(SlateShade::subscription_iced)
    .theme(SlateShade::theme_iced)
    .window_size(iced::Size::new(
        DEFAULT_SCREEN_WIDTH as f32,
        DEFAULT_SCREEN_HEIGHT * 0.65,
    ))
    .run_with(|| (SlateShade::default(), Task::none()))?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn run_app() -> anyhow::Result<()> {
    use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
    use iced_layershell::settings::{LayerShellSettings, Settings};
    use iced_layershell::Application as _;

    let settings: Settings<()> = Settings {
        layer_settings: LayerShellSettings {
            size: Some((0, INITIAL_SURFACE_HEIGHT)),
            anchor: Anchor::Top | Anchor::Left | Anchor::Right,
            exclusive_zone: 0,
            layer: Layer::Overlay,
            keyboard_interactivity: KeyboardInteractivity::OnDemand,
            ..Default::default()
        },
        ..Default::default()
    };

    SlateShade::run(settings).map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
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
        let _ = update_app(&mut s, Message::Open);
        assert!((s.anim.target - 1.0).abs() < 1e-9);
    }

    #[test]
    fn slate_shade_close_message_sets_target_zero() {
        let mut s = SlateShade::default();
        s.anim.open();
        let _ = update_app(&mut s, Message::Close);
        assert!((s.anim.target - 0.0).abs() < 1e-9);
    }

    #[test]
    fn slate_shade_notification_added_via_dbus() {
        let mut s = SlateShade::default();
        let notif = make_notif("TestApp");
        let _ = update_app(
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
        let _ = update_app(
            &mut s,
            Message::DbusEvent(dbus_listener::DbusEvent::NotificationAdded(notif)),
        );
        assert_eq!(s.groups.len(), 1);
        let _ = update_app(
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
        let _ = update_app(
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
        let _ = update_app(&mut s, Message::QsAction(QsAction::TileToggled(kind)));
        assert!(s.qs.tiles.iter().find(|t| t.kind == kind).unwrap().active);
    }

    #[test]
    fn slate_shade_qs_brightness_changed() {
        let mut s = SlateShade::default();
        let _ = update_app(&mut s, Message::QsAction(QsAction::BrightnessChanged(0.3)));
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
        let _ = update_app(
            &mut s,
            Message::DbusEvent(dbus_listener::DbusEvent::NotificationAdded(notif)),
        );
        assert!(s.hun.is_visible());
        let _ = update_app(&mut s, Message::HeadsUpAction(HeadsUpAction::Dismiss));
        assert!(!s.hun.is_visible());
    }

    #[test]
    fn slate_shade_edge_gesture_begin_sets_position() {
        let mut s = SlateShade::default();
        let _ = update_app(
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
        let _ = update_app(
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
        let _ = update_app(
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
        let _ = update_app(
            &mut s,
            Message::DbusEvent(dbus_listener::DbusEvent::NotificationAdded(make_notif(
                "App",
            ))),
        );
        let _ = update_app(
            &mut s,
            Message::DbusEvent(dbus_listener::DbusEvent::GroupChanged("App".to_string(), 0)),
        );
        assert!(s.groups.is_empty());
    }

    #[test]
    fn slate_shade_ai_summary_sets_on_group() {
        let mut s = SlateShade::default();
        let _ = update_app(
            &mut s,
            Message::DbusEvent(dbus_listener::DbusEvent::NotificationAdded(make_notif(
                "App",
            ))),
        );
        let _ = update_app(
            &mut s,
            Message::DbusEvent(dbus_listener::DbusEvent::AiSummaryReady(
                "App".to_string(),
                "Summary text".to_string(),
            )),
        );
        assert_eq!(s.groups[0].ai_summary.as_deref(), Some("Summary text"));
    }

    #[test]
    fn anim_tick_steps_animation() {
        let mut s = SlateShade::default();
        s.anim.open();
        let pos_before = s.anim.position;
        let _ = update_app(&mut s, Message::AnimTick);
        // After one tick toward target=1.0 position should have increased.
        assert!(s.anim.position >= pos_before);
    }

    #[test]
    fn hun_tick_removes_expired_banner() {
        let mut s = SlateShade::default();
        let state = heads_up::HeadsUpState::with_duration(make_notif("App"), Duration::ZERO);
        s.hun.active = Some(state);
        let _ = update_app(&mut s, Message::HunTick);
        assert!(!s.hun.is_visible());
    }

    #[test]
    fn notif_action_dismiss_removes_from_groups() {
        let mut s = SlateShade::default();
        let notif = make_notif("App");
        let uuid = notif.uuid;
        let _ = update_app(
            &mut s,
            Message::DbusEvent(dbus_listener::DbusEvent::NotificationAdded(notif)),
        );
        let _ = update_app(&mut s, Message::NotifAction(NotifAction::Dismiss(uuid)));
        assert!(s.groups.is_empty());
    }

    #[test]
    fn notif_action_dismiss_group_removes_all() {
        let mut s = SlateShade::default();
        let _ = update_app(
            &mut s,
            Message::DbusEvent(dbus_listener::DbusEvent::NotificationAdded(make_notif(
                "App",
            ))),
        );
        let _ = update_app(
            &mut s,
            Message::NotifAction(NotifAction::DismissGroup("App".to_string())),
        );
        assert!(s.groups.is_empty());
    }

    #[test]
    fn smart_replies_stored_on_event() {
        let mut s = SlateShade::default();
        let notif = make_notif("App");
        let uuid = notif.uuid;
        let _ = update_app(
            &mut s,
            Message::DbusEvent(dbus_listener::DbusEvent::SmartRepliesReady(
                uuid,
                vec!["Yes".to_string()],
            )),
        );
        assert_eq!(s.smart_replies[&uuid], vec!["Yes"]);
    }

    #[test]
    fn smart_replies_cleared_on_dismiss() {
        let mut s = SlateShade::default();
        let notif = make_notif("App");
        let uuid = notif.uuid;
        s.smart_replies.insert(uuid, vec!["OK".to_string()]);
        let _ = update_app(
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
}
