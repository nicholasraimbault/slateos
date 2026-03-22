// slate-lock — lock screen for Slate OS.
//
// Runs as a persistent daemon with a hidden layer-shell surface. When an
// external caller (slate-power, swayidle) invokes the `Lock()` D-Bus method,
// the surface expands to fullscreen with exclusive keyboard capture.
//
// Authentication: hybrid PAM (Linux) / argon2 PIN (cross-platform).
// Layout: adaptive — phone shows clock-then-PIN, tablet shows split view.
#![allow(dead_code)]

mod auth;
mod clock;
mod dbus;
mod notifications;
mod pin_pad;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use iced::widget::{column, container, row, text};
use iced::{Alignment, Color, Element, Length, Subscription, Task, Theme};

use auth::AuthResult;
use dbus::{dbus_subscription, LockDbusEvent};
use pin_pad::{PinPadAction, PinPadState};
use slate_common::notifications::Notification;
use slate_common::theme::create_theme;
use slate_common::Palette;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// How often to update the clock while locked (1 s).
const CLOCK_TICK_MS: u64 = 1000;

/// Animation step interval for the wrong-PIN shake (~60 fps).
const SHAKE_TICK_MS: u64 = 16;

/// Width threshold: narrower than this is treated as phone layout.
const PHONE_WIDTH_THRESHOLD: f32 = 600.0;

/// Starting surface height on Linux (hidden). Expands on lock.
#[cfg(target_os = "linux")]
const HIDDEN_SURFACE_HEIGHT: u32 = 1;

/// Shake animation: total duration in ms.
const SHAKE_DURATION_MS: u64 = 400;

/// Shake amplitude in logical pixels.
const SHAKE_AMPLITUDE: f32 = 16.0;

// ---------------------------------------------------------------------------
// Lock screen state
// ---------------------------------------------------------------------------

/// Phase of the lock screen lifecycle.
#[derive(Debug, Clone, PartialEq)]
enum LockPhase {
    /// Daemon running, surface invisible.
    Hidden,
    /// Phone layout: showing clock + notification previews. Tap to unlock.
    ShowingClock,
    /// PIN pad visible (always on tablet, after tap on phone).
    ShowingPinPad,
    /// Waiting for an authentication result.
    Authenticating,
}

struct SlateLock {
    phase: LockPhase,
    palette: Palette,
    pin_pad: PinPadState,
    /// Notifications fetched on lock — max 3, privacy-filtered.
    lock_notifications: Vec<Notification>,
    /// Failed attempt counter — drives rate-limiting cooldown.
    failure_count: u32,
    /// When the next attempt is allowed (rate-limiting after failures).
    cooldown_until: Option<Instant>,
    /// Shake animation start time.
    shake_start: Option<Instant>,
    /// Path to the PIN credential file (~/.config/slate/lock.toml).
    credential_path: PathBuf,
    /// Current username for PAM auth.
    username: String,
    /// Current window width (for adaptive layout).
    window_width: f32,
}

impl Default for SlateLock {
    fn default() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
        let credential_path = PathBuf::from(&home).join(".config/slate/lock.toml");
        let username = std::env::var("USER").unwrap_or_else(|_| "root".to_string());

        Self {
            phase: LockPhase::Hidden,
            palette: Palette::default(),
            pin_pad: PinPadState::default(),
            lock_notifications: Vec::new(),
            failure_count: 0,
            cooldown_until: None,
            shake_start: None,
            credential_path,
            username,
            window_width: 1280.0,
        }
    }
}

impl SlateLock {
    /// Whether the current layout is phone-sized.
    fn is_phone(&self) -> bool {
        self.window_width < PHONE_WIDTH_THRESHOLD
    }

    /// The cooldown delay based on consecutive failure count.
    fn cooldown_for_failures(count: u32) -> Duration {
        match count {
            0..=2 => Duration::ZERO,
            3..=4 => Duration::from_millis(500),
            5..=9 => Duration::from_secs(5),
            _ => Duration::from_secs(30),
        }
    }
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Message {
    /// D-Bus event (lock signal, palette change, notification).
    DbusEvent(LockDbusEvent),
    /// PIN pad button press.
    PinPad(PinPadAction),
    /// Phone: user tapped to transition from clock to PIN pad.
    TapToUnlock,
    /// Auth result returned from background thread.
    AuthCompleted(AuthResult),
    /// 1-second clock tick while locked.
    ClockTick,
    /// ~60fps shake animation tick.
    ShakeTick,
}

// ---------------------------------------------------------------------------
// Update
// ---------------------------------------------------------------------------

fn update_app(app: &mut SlateLock, message: Message) -> Task<Message> {
    match message {
        Message::DbusEvent(LockDbusEvent::LockRequested) => {
            tracing::info!("lock requested via D-Bus");
            if app.is_phone() {
                app.phase = LockPhase::ShowingClock;
            } else {
                app.phase = LockPhase::ShowingPinPad;
            }
            app.pin_pad.clear();
        }

        Message::DbusEvent(LockDbusEvent::PaletteChanged(palette)) => {
            app.palette = palette;
        }

        Message::DbusEvent(LockDbusEvent::NotificationAdded(data)) => {
            // Parse the TOML notification data and add to lock previews.
            if let Ok(notif) = toml::from_str::<Notification>(&data) {
                app.lock_notifications.push(notif);
                // Keep only the most recent MAX_LOCK_NOTIFICATIONS.
                let max = notifications::MAX_LOCK_NOTIFICATIONS;
                if app.lock_notifications.len() > max {
                    let drain = app.lock_notifications.len() - max;
                    app.lock_notifications.drain(..drain);
                }
            }
        }

        Message::TapToUnlock => {
            if app.phase == LockPhase::ShowingClock {
                app.phase = LockPhase::ShowingPinPad;
            }
        }

        Message::PinPad(PinPadAction::Digit(d)) => {
            if app.phase != LockPhase::ShowingPinPad {
                return Task::none();
            }

            // Check cooldown
            if let Some(until) = app.cooldown_until {
                if Instant::now() < until {
                    return Task::none();
                }
                app.cooldown_until = None;
            }

            app.pin_pad.push_digit(d);

            // Auto-submit when PIN reaches 4+ digits.
            if app.pin_pad.entered.len() >= 4 {
                app.phase = LockPhase::Authenticating;
                let pin = app.pin_pad.entered.clone();
                let username = app.username.clone();
                let cred_path = app.credential_path.clone();

                return Task::perform(
                    async move { auth::authenticate_sync(&pin, &username, &cred_path) },
                    Message::AuthCompleted,
                );
            }
        }

        Message::PinPad(PinPadAction::Backspace) => {
            app.pin_pad.backspace();
        }

        Message::AuthCompleted(AuthResult::Success) => {
            tracing::info!("authentication succeeded, unlocking");
            app.phase = LockPhase::Hidden;
            app.pin_pad.clear();
            app.failure_count = 0;
            app.cooldown_until = None;
            app.lock_notifications.clear();
        }

        Message::AuthCompleted(AuthResult::WrongCredential) => {
            app.failure_count += 1;
            tracing::warn!("authentication failed (attempt {})", app.failure_count);
            app.pin_pad.set_error("Wrong PIN");
            app.phase = LockPhase::ShowingPinPad;

            // Start shake animation
            app.shake_start = Some(Instant::now());

            // Apply rate-limiting cooldown
            let cooldown = SlateLock::cooldown_for_failures(app.failure_count);
            if !cooldown.is_zero() {
                app.cooldown_until = Some(Instant::now() + cooldown);
            }
        }

        Message::AuthCompleted(AuthResult::NotConfigured) => {
            // No credential is set — unlock with a warning.
            tracing::warn!("no credential configured, unlocking (first-boot grace)");
            app.phase = LockPhase::Hidden;
            app.pin_pad.clear();
            app.lock_notifications.clear();
        }

        Message::ClockTick => {
            // Forces a view redraw so the clock updates.
        }

        Message::ShakeTick => {
            if let Some(start) = app.shake_start {
                let elapsed = start.elapsed().as_millis() as f32;
                let total = SHAKE_DURATION_MS as f32;

                if elapsed >= total {
                    app.pin_pad.shake_offset = 0.0;
                    app.shake_start = None;
                } else {
                    // Damped sine wave for the shake effect.
                    let progress = elapsed / total;
                    let decay = 1.0 - progress;
                    let frequency = 4.0 * std::f32::consts::PI;
                    app.pin_pad.shake_offset =
                        SHAKE_AMPLITUDE * decay * (frequency * progress).sin();
                }
            }
        }
    }

    Task::none()
}

// ---------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------

fn view_app(app: &SlateLock) -> Element<'_, Message> {
    match &app.phase {
        LockPhase::Hidden => {
            // Invisible — render minimal content.
            text("").into()
        }

        LockPhase::ShowingClock => {
            // Phone: large clock + notification previews + "tap to unlock" hint.
            let clock_el = clock::view_clock::<Message>(true);
            let notifs = notifications::view_lock_notifications::<Message>(&app.lock_notifications);
            let hint = container(
                text("Tap to unlock")
                    .size(14)
                    .color(Color::from_rgba(1.0, 1.0, 1.0, 0.5)),
            )
            .center_x(Length::Fill)
            .padding(20);

            let content = column![clock_el, notifs, hint]
                .spacing(24)
                .align_x(Alignment::Center)
                .width(Length::Fill);

            let wrapped = container(content)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .width(Length::Fill)
                .height(Length::Fill)
                .style(lock_background_style);

            iced::widget::mouse_area(wrapped)
                .on_press(Message::TapToUnlock)
                .into()
        }

        LockPhase::ShowingPinPad | LockPhase::Authenticating => {
            if app.is_phone() {
                // Phone: smaller clock on top, PIN pad below.
                let clock_el = clock::view_clock::<Message>(false);
                let pin_pad = pin_pad::view_pin_pad(&app.pin_pad).map(Message::PinPad);

                let cooldown_hint: Element<'_, Message> = if let Some(until) = app.cooldown_until {
                    let remaining = until.saturating_duration_since(Instant::now());
                    if !remaining.is_zero() {
                        text(format!("Try again in {}s", remaining.as_secs()))
                            .size(12)
                            .color(Color::from_rgba(1.0, 0.5, 0.5, 0.8))
                            .into()
                    } else {
                        text("").into()
                    }
                } else {
                    text("").into()
                };

                let content = column![clock_el, pin_pad, cooldown_hint]
                    .spacing(24)
                    .align_x(Alignment::Center)
                    .width(Length::Fill);

                container(content)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .style(lock_background_style)
                    .into()
            } else {
                // Tablet/desktop: split layout — clock+notifs left, PIN pad right.
                let left = column![
                    clock::view_clock::<Message>(true),
                    notifications::view_lock_notifications::<Message>(&app.lock_notifications,),
                ]
                .spacing(24)
                .align_x(Alignment::Center)
                .width(Length::FillPortion(1));

                let pin_pad = pin_pad::view_pin_pad(&app.pin_pad).map(Message::PinPad);

                let right = container(pin_pad)
                    .center_x(Length::FillPortion(1))
                    .center_y(Length::Fill);

                let content = row![left, right]
                    .spacing(32)
                    .align_y(Alignment::Center)
                    .width(Length::Fill)
                    .height(Length::Fill);

                container(content)
                    .padding(48)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .style(lock_background_style)
                    .into()
            }
        }
    }
}

/// Dark background style for the lock screen.
fn lock_background_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(iced::Background::Color(Color::from_rgb(0.05, 0.05, 0.08))),
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Subscription
// ---------------------------------------------------------------------------

fn subscription_app(app: &SlateLock) -> Subscription<Message> {
    let dbus_sub = dbus_subscription().map(Message::DbusEvent);

    let mut subs = vec![dbus_sub];

    // Clock tick only while locked.
    if app.phase != LockPhase::Hidden {
        subs.push(
            iced::time::every(Duration::from_millis(CLOCK_TICK_MS)).map(|_| Message::ClockTick),
        );
    }

    // Shake animation tick.
    if app.shake_start.is_some() {
        subs.push(
            iced::time::every(Duration::from_millis(SHAKE_TICK_MS)).map(|_| Message::ShakeTick),
        );
    }

    Subscription::batch(subs)
}

// ---------------------------------------------------------------------------
// Layer-shell (Linux)
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
impl TryInto<iced_layershell::actions::LayershellCustomActions> for Message {
    type Error = Self;
    fn try_into(self) -> Result<iced_layershell::actions::LayershellCustomActions, Self> {
        Err(self)
    }
}

#[cfg(target_os = "linux")]
impl iced_layershell::Application for SlateLock {
    type Executor = iced::executor::Default;
    type Message = Message;
    type Theme = Theme;
    type Flags = ();

    fn new(_flags: ()) -> (Self, Task<Message>) {
        (Self::default(), Task::none())
    }

    fn namespace(&self) -> String {
        "slate-lock".to_string()
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
// macOS fallback (windowed, for development)
// ---------------------------------------------------------------------------

#[cfg(not(target_os = "linux"))]
impl SlateLock {
    fn title(&self) -> String {
        "Slate Lock".to_string()
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
        .with_env_filter("slate_lock=info,warn")
        .init();

    tracing::info!("slate-lock starting");

    run_app()
}

#[cfg(target_os = "linux")]
fn run_app() -> anyhow::Result<()> {
    use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
    use iced_layershell::settings::{LayerShellSettings, Settings};
    use iced_layershell::Application as _;

    let settings: Settings<()> = Settings {
        layer_settings: LayerShellSettings {
            size: Some((0, HIDDEN_SURFACE_HEIGHT)),
            anchor: Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right,
            exclusive_zone: -1,
            layer: Layer::Overlay,
            keyboard_interactivity: KeyboardInteractivity::Exclusive,
            ..Default::default()
        },
        ..Default::default()
    };

    SlateLock::run(settings).map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn run_app() -> anyhow::Result<()> {
    iced::application(
        SlateLock::title,
        SlateLock::update_iced,
        SlateLock::view_iced,
    )
    .subscription(SlateLock::subscription_iced)
    .theme(SlateLock::theme_iced)
    .window_size(iced::Size::new(400.0, 800.0))
    .run_with(|| (SlateLock::default(), Task::none()))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_hidden() {
        let app = SlateLock::default();
        assert_eq!(app.phase, LockPhase::Hidden);
        assert_eq!(app.failure_count, 0);
    }

    #[test]
    fn lock_requested_transitions_to_pin_pad() {
        // Default width is 1280 (tablet), so should go straight to PIN pad.
        let mut app = SlateLock::default();
        update_app(&mut app, Message::DbusEvent(LockDbusEvent::LockRequested));
        assert_eq!(app.phase, LockPhase::ShowingPinPad);
    }

    #[test]
    fn lock_requested_phone_shows_clock_first() {
        let mut app = SlateLock::default();
        app.window_width = 400.0; // phone
        update_app(&mut app, Message::DbusEvent(LockDbusEvent::LockRequested));
        assert_eq!(app.phase, LockPhase::ShowingClock);
    }

    #[test]
    fn tap_to_unlock_transitions_from_clock() {
        let mut app = SlateLock::default();
        app.phase = LockPhase::ShowingClock;
        update_app(&mut app, Message::TapToUnlock);
        assert_eq!(app.phase, LockPhase::ShowingPinPad);
    }

    #[test]
    fn auth_success_unlocks() {
        let mut app = SlateLock::default();
        app.phase = LockPhase::Authenticating;
        app.failure_count = 3;
        update_app(&mut app, Message::AuthCompleted(AuthResult::Success));
        assert_eq!(app.phase, LockPhase::Hidden);
        assert_eq!(app.failure_count, 0);
    }

    #[test]
    fn auth_wrong_increments_failure_count() {
        let mut app = SlateLock::default();
        app.phase = LockPhase::Authenticating;
        update_app(
            &mut app,
            Message::AuthCompleted(AuthResult::WrongCredential),
        );
        assert_eq!(app.failure_count, 1);
        assert_eq!(app.phase, LockPhase::ShowingPinPad);
        assert!(app.pin_pad.error_message.is_some());
    }

    #[test]
    fn auth_not_configured_unlocks_gracefully() {
        let mut app = SlateLock::default();
        app.phase = LockPhase::Authenticating;
        update_app(&mut app, Message::AuthCompleted(AuthResult::NotConfigured));
        assert_eq!(app.phase, LockPhase::Hidden);
    }

    #[test]
    fn cooldown_applied_after_failures() {
        assert_eq!(SlateLock::cooldown_for_failures(0), Duration::ZERO);
        assert_eq!(SlateLock::cooldown_for_failures(2), Duration::ZERO);
        assert_eq!(
            SlateLock::cooldown_for_failures(3),
            Duration::from_millis(500)
        );
        assert_eq!(SlateLock::cooldown_for_failures(5), Duration::from_secs(5));
        assert_eq!(
            SlateLock::cooldown_for_failures(10),
            Duration::from_secs(30)
        );
    }

    #[test]
    fn digit_during_cooldown_is_ignored() {
        let mut app = SlateLock::default();
        app.phase = LockPhase::ShowingPinPad;
        app.cooldown_until = Some(Instant::now() + Duration::from_secs(60));
        update_app(&mut app, Message::PinPad(PinPadAction::Digit('1')));
        assert!(app.pin_pad.entered.is_empty());
    }

    #[test]
    fn phone_width_detection() {
        let mut app = SlateLock::default();
        app.window_width = 400.0;
        assert!(app.is_phone());
        app.window_width = 800.0;
        assert!(!app.is_phone());
    }

    #[test]
    fn palette_change_updates_state() {
        let mut app = SlateLock::default();
        let new_palette = Palette::default();
        update_app(
            &mut app,
            Message::DbusEvent(LockDbusEvent::PaletteChanged(new_palette.clone())),
        );
        assert_eq!(app.palette, new_palette);
    }
}
