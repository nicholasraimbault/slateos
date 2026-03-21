/// Toast notification system for Slate OS iced apps.
///
/// Provides a lightweight, palette-aware notification overlay that any iced app
/// can embed. Toasts auto-expire after a configurable duration and render as a
/// vertical stack of rounded cards coloured by severity.
use std::time::{Duration, Instant};

use iced::widget::{column, container, text, Column};
use iced::{Element, Length, Theme};

use crate::palette::Palette;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default time a toast stays visible before expiring.
const DEFAULT_DURATION: Duration = Duration::from_secs(2);

/// Maximum number of toasts rendered simultaneously. When more arrive the
/// oldest is evicted on the next `tick`.
const MAX_VISIBLE: usize = 3;

/// Corner radius for the toast card background.
const CARD_RADIUS: f32 = 10.0;

/// Font size for the toast message text.
const TEXT_SIZE: f32 = 14.0;

/// Vertical spacing between stacked toast cards.
const STACK_SPACING: f32 = 8.0;

/// Inner padding of each toast card.
const CARD_PADDING: f32 = 12.0;

/// Maximum width of a toast card in logical pixels.
const CARD_MAX_WIDTH: f32 = 360.0;

// ---------------------------------------------------------------------------
// Toast position
// ---------------------------------------------------------------------------

/// Where the toast stack is anchored on screen.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ToastPosition {
    /// Top-right corner (desktop convention).
    TopRight,
    /// Bottom-center (mobile / tablet convention).
    #[default]
    BottomCenter,
}

// ---------------------------------------------------------------------------
// Toast kind
// ---------------------------------------------------------------------------

/// Severity / semantic category of a toast notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    /// Positive confirmation (e.g. "Saved!", "Copied!").
    Success,
    /// Neutral informational message.
    Info,
    /// Error or warning requiring attention.
    Error,
}

// ---------------------------------------------------------------------------
// Single toast
// ---------------------------------------------------------------------------

/// A single toast notification with an auto-expire timestamp.
#[derive(Debug, Clone)]
pub struct Toast {
    message: String,
    created_at: Instant,
    duration: Duration,
    kind: ToastKind,
}

impl Toast {
    /// Create a new toast with the given message, kind, and lifetime.
    fn new(message: String, kind: ToastKind, duration: Duration) -> Self {
        Self {
            message,
            created_at: Instant::now(),
            duration,
            kind,
        }
    }

    /// Whether this toast has outlived its display duration.
    pub fn is_expired(&self) -> bool {
        self.created_at.elapsed() >= self.duration
    }

    /// The toast message text.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The semantic kind of this toast.
    pub fn kind(&self) -> ToastKind {
        self.kind
    }
}

// ---------------------------------------------------------------------------
// Toast state
// ---------------------------------------------------------------------------

/// Manages a collection of active toast notifications.
///
/// Embed one `ToastState` per iced app and call `tick` on every frame (or
/// timer subscription) to garbage-collect expired toasts.
#[derive(Debug, Clone)]
pub struct ToastState {
    toasts: Vec<Toast>,
    position: ToastPosition,
}

impl Default for ToastState {
    fn default() -> Self {
        Self::new(ToastPosition::default())
    }
}

impl ToastState {
    /// Create a new empty toast state with the specified position.
    pub fn new(position: ToastPosition) -> Self {
        Self {
            toasts: Vec::new(),
            position,
        }
    }

    /// Push a new toast with the default 2-second duration.
    ///
    /// If the number of visible toasts would exceed `MAX_VISIBLE`, the oldest
    /// toast is removed immediately.
    pub fn push(&mut self, message: impl Into<String>, kind: ToastKind) {
        self.push_with_duration(message, kind, DEFAULT_DURATION);
    }

    /// Push a new toast with a custom duration.
    pub fn push_with_duration(
        &mut self,
        message: impl Into<String>,
        kind: ToastKind,
        duration: Duration,
    ) {
        self.toasts.push(Toast::new(message.into(), kind, duration));
        // Evict the oldest if we exceed the cap.
        while self.toasts.len() > MAX_VISIBLE {
            self.toasts.remove(0);
        }
    }

    /// Remove all expired toasts. Call this on every tick / timer event.
    pub fn tick(&mut self) {
        self.toasts.retain(|t| !t.is_expired());
    }

    /// Whether there are no active toasts.
    pub fn is_empty(&self) -> bool {
        self.toasts.is_empty()
    }

    /// Number of active (not yet expired) toasts.
    pub fn len(&self) -> usize {
        self.toasts.len()
    }

    /// Current toast position setting.
    pub fn position(&self) -> ToastPosition {
        self.position
    }

    /// Change the toast position.
    pub fn set_position(&mut self, position: ToastPosition) {
        self.position = position;
    }

    /// Read-only access to the active toasts (oldest first).
    pub fn toasts(&self) -> &[Toast] {
        &self.toasts
    }

    /// Render the toast stack as an iced `Element`.
    ///
    /// Toast card colours are derived from the current `Palette`:
    /// - **Success**: green tint
    /// - **Info**: blue tint (uses `palette.primary`)
    /// - **Error**: red tint
    ///
    /// The returned element is a `Column` of cards that the caller can layer
    /// on top of their main view using `iced_layershell` or a simple overlay.
    pub fn view<'a, M: 'a>(&self, palette: &Palette) -> Element<'a, M> {
        if self.toasts.is_empty() {
            return column![].into();
        }

        let text_color = Palette::color_to_iced(palette.neutral);

        let mut col = Column::new().spacing(STACK_SPACING);

        for toast in &self.toasts {
            let bg = card_background(toast.kind, palette);

            let card = container(
                text(toast.message.clone())
                    .size(TEXT_SIZE)
                    .color(text_color),
            )
            .padding(CARD_PADDING)
            .max_width(CARD_MAX_WIDTH)
            .style(card_style(bg));

            col = col.push(card);
        }

        let aligned = match self.position {
            ToastPosition::TopRight => container(col)
                .width(Length::Fill)
                .align_x(iced::alignment::Horizontal::Right)
                .align_y(iced::alignment::Vertical::Top)
                .padding(12.0),
            ToastPosition::BottomCenter => container(col)
                .width(Length::Fill)
                .align_x(iced::alignment::Horizontal::Center)
                .align_y(iced::alignment::Vertical::Bottom)
                .padding(12.0),
        };

        aligned.into()
    }
}

// ---------------------------------------------------------------------------
// Style helpers
// ---------------------------------------------------------------------------

/// Pick a background colour for the card based on toast kind and palette.
///
/// Success and Error use fixed Material-inspired hues so they remain
/// recognisable regardless of the dynamic palette. Info uses the palette's
/// primary colour at reduced opacity.
fn card_background(kind: ToastKind, palette: &Palette) -> iced::Color {
    match kind {
        ToastKind::Success => iced::Color::from_rgba8(46, 125, 50, 0.78), // Material green 800
        ToastKind::Info => {
            let p = palette.primary;
            iced::Color::from_rgba8(p[0], p[1], p[2], 0.78)
        }
        ToastKind::Error => iced::Color::from_rgba8(198, 40, 40, 0.78), // Material red 800
    }
}

/// Create a container style closure for a toast card.
fn card_style(bg: iced::Color) -> impl Fn(&Theme) -> container::Style {
    move |_theme| container::Style {
        background: Some(iced::Background::Color(bg)),
        border: iced::Border {
            radius: CARD_RADIUS.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Toast ---------------------------------------------------------------

    #[test]
    fn toast_starts_not_expired() {
        let t = Toast::new("hello".into(), ToastKind::Info, Duration::from_secs(10));
        assert!(!t.is_expired());
    }

    #[test]
    fn toast_expires_after_zero_duration() {
        let t = Toast::new("bye".into(), ToastKind::Error, Duration::ZERO);
        // A zero-duration toast should be expired immediately.
        assert!(t.is_expired());
    }

    #[test]
    fn toast_message_returns_text() {
        let t = Toast::new("msg".into(), ToastKind::Success, DEFAULT_DURATION);
        assert_eq!(t.message(), "msg");
    }

    #[test]
    fn toast_kind_returns_kind() {
        let t = Toast::new("x".into(), ToastKind::Error, DEFAULT_DURATION);
        assert_eq!(t.kind(), ToastKind::Error);
    }

    // -- ToastState ----------------------------------------------------------

    #[test]
    fn new_state_is_empty() {
        let state = ToastState::default();
        assert!(state.is_empty());
        assert_eq!(state.len(), 0);
    }

    #[test]
    fn push_adds_toast() {
        let mut state = ToastState::default();
        state.push("hello", ToastKind::Info);
        assert!(!state.is_empty());
        assert_eq!(state.len(), 1);
    }

    #[test]
    fn push_evicts_oldest_when_over_max() {
        let mut state = ToastState::default();
        for i in 0..5 {
            state.push(format!("toast {i}"), ToastKind::Info);
        }
        assert_eq!(state.len(), MAX_VISIBLE);
        // The remaining toasts should be the newest ones.
        assert_eq!(state.toasts()[0].message(), "toast 2");
        assert_eq!(state.toasts()[1].message(), "toast 3");
        assert_eq!(state.toasts()[2].message(), "toast 4");
    }

    #[test]
    fn tick_removes_expired_toasts() {
        let mut state = ToastState::default();
        state.push_with_duration("gone", ToastKind::Info, Duration::ZERO);
        state.push_with_duration("stays", ToastKind::Success, Duration::from_secs(60));
        state.tick();
        assert_eq!(state.len(), 1);
        assert_eq!(state.toasts()[0].message(), "stays");
    }

    #[test]
    fn tick_on_empty_state_is_safe() {
        let mut state = ToastState::default();
        state.tick();
        assert!(state.is_empty());
    }

    #[test]
    fn is_empty_reflects_state() {
        let mut state = ToastState::default();
        assert!(state.is_empty());
        state.push("x", ToastKind::Info);
        assert!(!state.is_empty());
    }

    #[test]
    fn position_default_is_bottom_center() {
        let state = ToastState::default();
        assert_eq!(state.position(), ToastPosition::BottomCenter);
    }

    #[test]
    fn set_position_changes_position() {
        let mut state = ToastState::default();
        state.set_position(ToastPosition::TopRight);
        assert_eq!(state.position(), ToastPosition::TopRight);
    }

    #[test]
    fn new_with_position_sets_position() {
        let state = ToastState::new(ToastPosition::TopRight);
        assert_eq!(state.position(), ToastPosition::TopRight);
    }

    #[test]
    fn push_with_duration_uses_custom_duration() {
        let mut state = ToastState::default();
        state.push_with_duration("short", ToastKind::Info, Duration::ZERO);
        // Should already be expired.
        assert!(state.toasts()[0].is_expired());
    }

    #[test]
    fn toasts_returns_slice() {
        let mut state = ToastState::default();
        state.push("a", ToastKind::Success);
        state.push("b", ToastKind::Error);
        let toasts = state.toasts();
        assert_eq!(toasts.len(), 2);
        assert_eq!(toasts[0].message(), "a");
        assert_eq!(toasts[1].message(), "b");
    }

    // -- view ----------------------------------------------------------------

    #[test]
    fn view_empty_state_produces_element() {
        let state = ToastState::default();
        let palette = Palette::default();
        let _element: Element<'_, ()> = state.view(&palette);
    }

    #[test]
    fn view_with_toasts_produces_element() {
        let mut state = ToastState::default();
        state.push("Copied!", ToastKind::Success);
        state.push("Connecting...", ToastKind::Info);
        state.push("Failed!", ToastKind::Error);
        let palette = Palette::default();
        let _element: Element<'_, ()> = state.view(&palette);
    }

    #[test]
    fn view_top_right_produces_element() {
        let mut state = ToastState::new(ToastPosition::TopRight);
        state.push("Test", ToastKind::Info);
        let palette = Palette::default();
        let _element: Element<'_, ()> = state.view(&palette);
    }

    #[test]
    fn view_bottom_center_produces_element() {
        let mut state = ToastState::new(ToastPosition::BottomCenter);
        state.push("Test", ToastKind::Success);
        let palette = Palette::default();
        let _element: Element<'_, ()> = state.view(&palette);
    }

    // -- card_background -----------------------------------------------------

    #[test]
    fn card_background_success_is_green() {
        let palette = Palette::default();
        let c = card_background(ToastKind::Success, &palette);
        // Green channel should dominate.
        assert!(c.g > c.r);
        assert!(c.g > c.b);
    }

    #[test]
    fn card_background_error_is_red() {
        let palette = Palette::default();
        let c = card_background(ToastKind::Error, &palette);
        // Red channel should dominate.
        assert!(c.r > c.g);
        assert!(c.r > c.b);
    }

    #[test]
    fn card_background_info_uses_palette_primary() {
        let palette = Palette::default();
        let c = card_background(ToastKind::Info, &palette);
        let expected = Palette::color_to_iced(palette.primary);
        assert!((c.r - expected.r).abs() < 0.01);
        assert!((c.g - expected.g).abs() < 0.01);
        assert!((c.b - expected.b).abs() < 0.01);
    }

    // -- card_style ----------------------------------------------------------

    #[test]
    fn card_style_returns_valid_style() {
        let bg = iced::Color::from_rgb(0.2, 0.5, 0.2);
        let style_fn = card_style(bg);
        let theme = iced::Theme::Dark;
        let style = style_fn(&theme);
        assert!(style.background.is_some());
    }

    // -- ToastKind / ToastPosition derive checks -----------------------------

    #[test]
    fn toast_kind_is_debug_clone_copy() {
        let k = ToastKind::Info;
        let _cloned = k;
        let _debug = format!("{k:?}");
    }

    #[test]
    fn toast_position_is_debug_clone_copy() {
        let p = ToastPosition::TopRight;
        let _cloned = p;
        let _debug = format!("{p:?}");
    }
}
