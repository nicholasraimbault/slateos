// Heads-up notification (HUN) banner.
//
// A thin banner that slides in from the top of the screen for urgent/heads-up
// notifications. It auto-dismisses after a configurable timeout and supports
// a swipe-up gesture to dismiss early. Action buttons are shown inline so the
// user can respond without opening the shade.

use std::time::{Duration, Instant};

use iced::widget::{button, column, container, row, text};
use iced::{Alignment, Color, Element, Length, Theme};

use slate_common::notifications::{Notification, Urgency};
use slate_common::Palette;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// How long a heads-up banner stays on screen before auto-dismissing.
const HUN_DISPLAY_DURATION: Duration = Duration::from_secs(5);

/// Height of the HUN banner in logical pixels.
const HUN_HEIGHT: f32 = 80.0;

/// Horizontal and vertical padding inside the banner.
const HUN_PADDING: f32 = 12.0;

/// Corner radius of the banner card.
const HUN_RADIUS: f32 = 14.0;

/// Font size for the summary line.
const SUMMARY_SIZE: f32 = 14.0;

/// Font size for the body line.
const BODY_SIZE: f32 = 13.0;

/// Font size for action chip labels.
const ACTION_SIZE: f32 = 12.0;

// ---------------------------------------------------------------------------
// Heads-up state
// ---------------------------------------------------------------------------

/// The active heads-up banner, if any.
#[derive(Debug, Clone)]
pub struct HeadsUpState {
    /// The notification being shown.
    pub notification: Notification,
    /// When the banner was first displayed.
    pub shown_at: Instant,
    /// Custom display duration (defaults to `HUN_DISPLAY_DURATION`).
    pub duration: Duration,
    /// Whether the user has started swiping up to dismiss.
    pub swipe_progress: f32,
}

impl HeadsUpState {
    /// Create a new heads-up banner for the given notification.
    pub fn new(notification: Notification) -> Self {
        Self {
            notification,
            shown_at: Instant::now(),
            duration: HUN_DISPLAY_DURATION,
            swipe_progress: 0.0,
        }
    }

    /// Create a banner with a custom display duration (useful for tests).
    pub fn with_duration(notification: Notification, duration: Duration) -> Self {
        Self {
            notification,
            shown_at: Instant::now(),
            duration,
            swipe_progress: 0.0,
        }
    }

    /// Whether the banner has exceeded its display duration.
    pub fn is_expired(&self) -> bool {
        self.shown_at.elapsed() >= self.duration
    }

    /// Remaining display time.
    ///
    /// Returns `Duration::ZERO` when the banner has already expired.
    pub fn remaining(&self) -> Duration {
        let elapsed = self.shown_at.elapsed();
        self.duration.saturating_sub(elapsed)
    }

    /// UUID of the displayed notification.
    pub fn uuid(&self) -> Uuid {
        self.notification.uuid
    }
}

// ---------------------------------------------------------------------------
// Heads-up manager
// ---------------------------------------------------------------------------

/// Manages the optional heads-up banner.
///
/// Only one banner is shown at a time; new urgent notifications preempt the
/// current one if it has been displayed for at least 2 seconds.
#[derive(Debug, Clone, Default)]
pub struct HeadsUp {
    pub(crate) active: Option<HeadsUpState>,
}

/// Minimum time a HUN must be displayed before it can be preempted.
const MIN_DISPLAY_BEFORE_PREEMPT: Duration = Duration::from_secs(2);

impl HeadsUp {
    /// Show a notification as a heads-up banner.
    ///
    /// If another banner is active and has been shown for less than
    /// `MIN_DISPLAY_BEFORE_PREEMPT`, the new notification is ignored
    /// (the existing one takes priority).
    pub fn show(&mut self, notif: Notification) {
        let can_preempt = self
            .active
            .as_ref()
            .map(|s| s.shown_at.elapsed() >= MIN_DISPLAY_BEFORE_PREEMPT)
            .unwrap_or(true);

        if can_preempt {
            self.active = Some(HeadsUpState::new(notif));
        }
    }

    /// Dismiss the current banner (user action or auto-expire).
    pub fn dismiss(&mut self) {
        self.active = None;
    }

    /// Tick: remove the banner if it has expired.
    ///
    /// Returns `true` if a banner was removed.
    pub fn tick(&mut self) -> bool {
        if self
            .active
            .as_ref()
            .map(|s| s.is_expired())
            .unwrap_or(false)
        {
            self.active = None;
            return true;
        }
        false
    }

    /// Whether a banner is currently shown.
    pub fn is_visible(&self) -> bool {
        self.active.is_some()
    }

    /// Current banner state, if any.
    pub fn current(&self) -> Option<&HeadsUpState> {
        self.active.as_ref()
    }

    /// Update the swipe-up progress (0 = no swipe, 1 = fully swiped away).
    ///
    /// A progress of 1.0 auto-dismisses the banner.
    pub fn update_swipe(&mut self, progress: f32) {
        if let Some(state) = &mut self.active {
            state.swipe_progress = progress.clamp(0.0, 1.0);
            if state.swipe_progress >= 1.0 {
                self.active = None;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

/// Actions produced by the heads-up view.
#[derive(Debug, Clone)]
pub enum HeadsUpAction {
    /// User tapped the dismiss button or banner expired.
    Dismiss,
    /// User invoked a notification action.
    InvokeAction(Uuid, String),
}

// ---------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------

/// Render the heads-up banner.
///
/// Returns an empty spacer if no banner is active.
pub fn view_heads_up<'a>(hun: &'a HeadsUp, palette: &Palette) -> Element<'a, HeadsUpAction> {
    let state = match hun.current() {
        Some(s) => s,
        None => {
            return iced::widget::Space::new(Length::Fill, Length::Fixed(0.0)).into();
        }
    };

    // Compute visual offset from swipe progress (banner slides up).
    let y_offset = state.swipe_progress * HUN_HEIGHT;
    let _ = y_offset; // used in production compositor offset; not in iced layout

    let bg = hun_background(&state.notification, palette);
    let text_color = Palette::color_to_iced(palette.neutral);
    let muted = Color {
        a: 0.7,
        ..text_color
    };

    let mut content = column![row![
        text(&state.notification.summary)
            .size(SUMMARY_SIZE)
            .color(text_color)
            .width(Length::Fill),
        button(text("✕").size(12.0))
            .on_press(HeadsUpAction::Dismiss)
            .style(ghost_button_style),
    ]
    .align_y(Alignment::Center)
    .spacing(8.0),]
    .spacing(4.0);

    if !state.notification.body.is_empty() {
        content = content.push(text(&state.notification.body).size(BODY_SIZE).color(muted));
    }

    if !state.notification.actions.is_empty() {
        let mut action_row = row![].spacing(6.0);
        for action in &state.notification.actions {
            let key = action.key.clone();
            let uuid = state.notification.uuid;
            action_row = action_row.push(
                button(text(&action.label).size(ACTION_SIZE))
                    .on_press(HeadsUpAction::InvokeAction(uuid, key))
                    .style(chip_button_style),
            );
        }
        content = content.push(action_row);
    }

    container(content)
        .width(Length::Fill)
        .height(Length::Fixed(HUN_HEIGHT))
        .padding(HUN_PADDING)
        .style(move |_theme: &Theme| container::Style {
            background: Some(iced::Background::Color(bg)),
            border: iced::Border {
                radius: HUN_RADIUS.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

// ---------------------------------------------------------------------------
// Style helpers
// ---------------------------------------------------------------------------

/// Background colour of the HUN banner based on urgency.
fn hun_background(notif: &Notification, palette: &Palette) -> Color {
    match notif.urgency {
        Urgency::Critical => Color::from_rgba8(183, 28, 28, 0.95),
        Urgency::Normal | Urgency::Low => {
            let s = palette.container;
            Color::from_rgba8(s[0], s[1], s[2], 0.95)
        }
    }
}

fn ghost_button_style(theme: &Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: None,
        text_color: theme.palette().text,
        border: iced::Border::default(),
        shadow: iced::Shadow::default(),
    }
}

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
            radius: 16.0.into(),
            ..Default::default()
        },
        shadow: iced::Shadow::default(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use slate_common::notifications::Notification;

    fn make_notif() -> Notification {
        Notification::new(1, "App", "Summary", "Body")
    }

    #[test]
    fn heads_up_default_is_not_visible() {
        let hun = HeadsUp::default();
        assert!(!hun.is_visible());
    }

    #[test]
    fn heads_up_show_makes_visible() {
        let mut hun = HeadsUp::default();
        hun.show(make_notif());
        assert!(hun.is_visible());
    }

    #[test]
    fn heads_up_dismiss_clears_banner() {
        let mut hun = HeadsUp::default();
        hun.show(make_notif());
        hun.dismiss();
        assert!(!hun.is_visible());
    }

    #[test]
    fn heads_up_tick_removes_expired_banner() {
        let mut hun = HeadsUp::default();
        let state = HeadsUpState::with_duration(make_notif(), Duration::ZERO);
        hun.active = Some(state);
        let removed = hun.tick();
        assert!(removed);
        assert!(!hun.is_visible());
    }

    #[test]
    fn heads_up_tick_keeps_non_expired_banner() {
        let mut hun = HeadsUp::default();
        hun.show(make_notif());
        let removed = hun.tick();
        assert!(!removed);
        assert!(hun.is_visible());
    }

    #[test]
    fn heads_up_swipe_progress_dismisses_at_1() {
        let mut hun = HeadsUp::default();
        hun.show(make_notif());
        hun.update_swipe(1.0);
        assert!(!hun.is_visible());
    }

    #[test]
    fn heads_up_swipe_progress_partial_does_not_dismiss() {
        let mut hun = HeadsUp::default();
        hun.show(make_notif());
        hun.update_swipe(0.5);
        assert!(hun.is_visible());
        assert!((hun.current().unwrap().swipe_progress - 0.5).abs() < 1e-6);
    }

    #[test]
    fn heads_up_new_preempts_after_min_time() {
        let mut hun = HeadsUp::default();
        // Show with zero duration so it's already "old enough" to preempt.
        let old = HeadsUpState::with_duration(make_notif(), Duration::ZERO);
        hun.active = Some(old);
        // Wait for elapsed > MIN_DISPLAY_BEFORE_PREEMPT — we can't actually
        // sleep in a test, but we can directly set shown_at to a past time.
        if let Some(ref mut s) = hun.active {
            s.shown_at = Instant::now()
                .checked_sub(MIN_DISPLAY_BEFORE_PREEMPT + Duration::from_millis(1))
                .unwrap_or_else(Instant::now);
        }
        hun.show(make_notif());
        assert!(hun.is_visible());
    }

    #[test]
    fn heads_up_state_is_expired_with_zero_duration() {
        let state = HeadsUpState::with_duration(make_notif(), Duration::ZERO);
        assert!(state.is_expired());
    }

    #[test]
    fn heads_up_state_is_not_expired_with_long_duration() {
        let state = HeadsUpState::with_duration(make_notif(), Duration::from_secs(600));
        assert!(!state.is_expired());
    }

    #[test]
    fn heads_up_remaining_is_zero_when_expired() {
        let state = HeadsUpState::with_duration(make_notif(), Duration::ZERO);
        assert_eq!(state.remaining(), Duration::ZERO);
    }

    #[test]
    fn view_heads_up_no_banner_produces_element() {
        let hun = HeadsUp::default();
        let palette = Palette::default();
        let _el: Element<'_, HeadsUpAction> = view_heads_up(&hun, &palette);
    }

    #[test]
    fn view_heads_up_with_banner_produces_element() {
        let mut hun = HeadsUp::default();
        hun.show(make_notif());
        let palette = Palette::default();
        let _el: Element<'_, HeadsUpAction> = view_heads_up(&hun, &palette);
    }

    #[test]
    fn heads_up_action_is_debug_clone() {
        let action = HeadsUpAction::Dismiss;
        let _cloned = action.clone();
        let _debug = format!("{action:?}");
    }
}
