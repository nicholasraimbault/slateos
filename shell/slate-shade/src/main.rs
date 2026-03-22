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
mod actions;
mod dbus_listener;
mod heads_up;
mod layout;
mod notifications;
mod quick_settings;
mod shade;
mod style;
mod update;

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use iced::widget::column;
use iced::{Element, Subscription, Task, Theme};

use heads_up::{HeadsUp, HeadsUpAction};
use layout::{detect_layout, detect_layout_from_niri, LayoutMode};
use notifications::{NotifAction, NotificationGroup};
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
    /// App names waiting for an AI summary from Rhea, in request order.
    ///
    /// When the shade calls `Rhea.Summarize(app_name, text)`, it pushes
    /// `app_name` here. When `RheaCompletionDone` arrives, we pop the
    /// front entry and route the summary to that group. This gives correct
    /// ordering for sequential requests.
    ///
    /// TODO: a proper correlation ID in the Rhea protocol would be the ideal
    /// long-term fix (avoids any ordering assumptions entirely).
    pending_summaries: VecDeque<String>,
    /// Timestamp of the last brightness write, used to debounce slider events.
    last_brightness_write: Instant,
    /// Timestamp of the last volume write, used to debounce slider events.
    last_volume_write: Instant,
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
            pending_summaries: VecDeque::new(),
            last_brightness_write: Instant::now(),
            last_volume_write: Instant::now(),
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
    // Layout detected from niri IPC at startup
    LayoutDetected(LayoutMode),
    // Result of a Notifications.Dismiss D-Bus call. Errors are logged; no UI update needed
    // because the local state was already updated optimistically.
    DismissResult(Result<(), String>),
    // Result of a Notifications.DismissGroup D-Bus call.
    DismissGroupResult(Result<(), String>),
    // Result of a Notifications.InvokeAction D-Bus call.
    InvokeActionResult(Result<(), String>),
    // Smart-reply suggestions returned by Rhea.SuggestReplies.
    SmartRepliesResult(uuid::Uuid, Vec<String>),
    // Result of a quick-settings system call (brightness, volume, WiFi, etc.).
    SystemResult(Result<(), String>),
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
        // Query niri for the real output width so the shade renders the
        // correct layout on first paint rather than relying on the compile-time
        // DEFAULT_SCREEN_WIDTH constant.
        let layout_task = Task::perform(detect_layout_from_niri(), Message::LayoutDetected);
        (Self::default(), layout_task)
    }

    fn namespace(&self) -> String {
        "slate-shade".to_string()
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        update::update_app(self, message)
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
        update::update_app(self, message)
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
    .run_with(|| {
        let layout_task = Task::perform(detect_layout_from_niri(), Message::LayoutDetected);
        (SlateShade::default(), layout_task)
    })?;
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
#[path = "main_tests.rs"]
mod tests;
