// Toast notification system.
//
// Provides a simple timed flash message that appears briefly after an
// action (e.g. "Copied!" after copying a code block to clipboard).
// Auto-dismisses after a configurable duration.

use std::time::{Duration, Instant};

use iced::widget::{container, text};
use iced::{Color, Element, Length, Theme};

/// How long a toast message stays visible before auto-dismissing.
pub const TOAST_DURATION: Duration = Duration::from_secs(2);

/// Visual style of a toast notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    /// Success feedback (e.g. "Copied!").
    Success,
    /// Error feedback (e.g. "wl-copy not found").
    Error,
}

/// A toast notification with a message and expiry time.
#[derive(Debug, Clone)]
pub struct Toast {
    pub message: String,
    pub kind: ToastKind,
    pub expires_at: Instant,
}

impl Toast {
    /// Create a new toast that expires after `TOAST_DURATION`.
    pub fn new(message: String, kind: ToastKind) -> Self {
        Self {
            message,
            kind,
            expires_at: Instant::now() + TOAST_DURATION,
        }
    }

    /// Whether this toast has expired and should be removed.
    pub fn is_expired(&self) -> bool {
        Instant::now() >= self.expires_at
    }
}

/// Manages the currently visible toast, if any.
#[derive(Debug, Clone, Default)]
pub struct ToastState {
    current: Option<Toast>,
}

impl ToastState {
    /// Create a new empty toast state.
    pub fn new() -> Self {
        Self { current: None }
    }

    /// Show a success toast with the given message.
    pub fn show_success(&mut self, message: String) {
        self.current = Some(Toast::new(message, ToastKind::Success));
    }

    /// Show an error toast with the given message.
    pub fn show_error(&mut self, message: String) {
        self.current = Some(Toast::new(message, ToastKind::Error));
    }

    /// Dismiss the toast if it has expired. Called on each tick.
    pub fn tick(&mut self) {
        if let Some(ref toast) = self.current {
            if toast.is_expired() {
                self.current = None;
            }
        }
    }

    /// Whether there is an active (non-expired) toast to display.
    pub fn is_visible(&self) -> bool {
        self.current.as_ref().is_some_and(|t| !t.is_expired())
    }

    /// Get the current toast, if any and not expired.
    pub fn current(&self) -> Option<&Toast> {
        self.current.as_ref().filter(|t| !t.is_expired())
    }
}

/// Render the toast overlay. Returns an empty element when no toast is active.
///
/// The toast is a small rounded pill that sits at the bottom of the panel,
/// coloured according to the toast kind (green for success, red for error).
pub fn view_toast<'a, M: 'a>(state: &'a ToastState, surface: [u8; 4]) -> Element<'a, M> {
    let toast = match state.current() {
        Some(t) => t,
        None => {
            return container(text("")).width(0).height(0).into();
        }
    };

    let (bg_color, text_color) = match toast.kind {
        ToastKind::Success => (
            Color::from_rgba8(76, 175, 80, 0.9), // Material green
            Color::from_rgb(1.0, 1.0, 1.0),
        ),
        ToastKind::Error => (
            Color::from_rgba8(244, 67, 54, 0.9), // Material red
            Color::from_rgb(1.0, 1.0, 1.0),
        ),
    };

    // Blend slightly with the surface colour for cohesion
    let _ = surface; // Reserved for future palette-aware styling

    let label = text(&toast.message).size(13).color(text_color);

    container(label)
        .padding([6, 16])
        .center_x(Length::Fill)
        .style(move |_theme: &Theme| container::Style {
            background: Some(iced::Background::Color(bg_color)),
            border: iced::Border {
                radius: 16.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toast_new_is_not_expired() {
        let toast = Toast::new("Hello".to_string(), ToastKind::Success);
        assert!(!toast.is_expired());
    }

    #[test]
    fn toast_state_default_is_empty() {
        let state = ToastState::new();
        assert!(!state.is_visible());
        assert!(state.current().is_none());
    }

    #[test]
    fn toast_state_show_success() {
        let mut state = ToastState::new();
        state.show_success("Copied!".to_string());
        assert!(state.is_visible());

        let toast = state.current().expect("should have toast");
        assert_eq!(toast.message, "Copied!");
        assert_eq!(toast.kind, ToastKind::Success);
    }

    #[test]
    fn toast_state_show_error() {
        let mut state = ToastState::new();
        state.show_error("Failed".to_string());
        assert!(state.is_visible());

        let toast = state.current().expect("should have toast");
        assert_eq!(toast.message, "Failed");
        assert_eq!(toast.kind, ToastKind::Error);
    }

    #[test]
    fn toast_state_tick_clears_expired() {
        let mut state = ToastState::new();
        // Create a toast that is already expired
        state.current = Some(Toast {
            message: "Gone".to_string(),
            kind: ToastKind::Success,
            expires_at: Instant::now() - Duration::from_secs(1),
        });

        state.tick();
        assert!(!state.is_visible());
        assert!(state.current().is_none());
    }

    #[test]
    fn toast_state_new_toast_replaces_old() {
        let mut state = ToastState::new();
        state.show_success("First".to_string());
        state.show_error("Second".to_string());

        let toast = state.current().expect("should have toast");
        assert_eq!(toast.message, "Second");
        assert_eq!(toast.kind, ToastKind::Error);
    }

    #[test]
    fn toast_duration_is_two_seconds() {
        assert_eq!(TOAST_DURATION, Duration::from_secs(2));
    }

    #[test]
    fn toast_kind_is_debug_clone_eq() {
        let kind = ToastKind::Success;
        let _cloned = kind;
        let _debug = format!("{kind:?}");
        assert_eq!(kind, ToastKind::Success);
        assert_ne!(kind, ToastKind::Error);
    }
}
