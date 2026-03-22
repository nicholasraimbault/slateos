/// PIN pad widget for the lock screen.
///
/// Provides the numeric entry grid and dot indicators used for PIN
/// authentication. The view functions return plain iced `Element`s so
/// the caller can embed them in any layout without coupling to a
/// specific message type (for the dots) or use `PinPadAction` directly
/// (for the pad).
use iced::widget::{button, column, container, row, text};
use iced::{Alignment, Color, Element, Length, Padding};

/// Hard upper bound on PIN length to prevent unbounded input.
pub const MAX_PIN_LENGTH: usize = 8;

/// Minimum number of dots to display so the UI never looks empty.
const MIN_DOT_COUNT: usize = 4;

/// Touch-friendly button size in logical pixels.
const BUTTON_SIZE: f32 = 72.0;

/// Spacing between buttons in the grid.
const BUTTON_SPACING: f32 = 16.0;

/// Font size for digit labels inside buttons.
const DIGIT_FONT_SIZE: f32 = 28.0;

/// Font size for the PIN dot indicators.
const DOT_FONT_SIZE: f32 = 24.0;

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

/// Events produced by the PIN pad buttons.
#[derive(Debug, Clone)]
pub enum PinPadAction {
    /// User tapped a digit key (0-9).
    Digit(char),
    /// User tapped the backspace key.
    Backspace,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Mutable state backing the PIN pad UI.
#[derive(Debug, Clone)]
pub struct PinPadState {
    /// Digits entered so far.
    pub entered: String,
    /// Maximum number of digits accepted.
    pub max_length: usize,
    /// Horizontal pixel offset applied during the wrong-PIN shake animation.
    /// Zero when idle.
    pub shake_offset: f32,
    /// Optional error string displayed below the PIN dots (e.g. "Wrong PIN").
    pub error_message: Option<String>,
}

impl Default for PinPadState {
    fn default() -> Self {
        Self {
            entered: String::new(),
            max_length: MAX_PIN_LENGTH,
            shake_offset: 0.0,
            error_message: None,
        }
    }
}

impl PinPadState {
    /// Append a digit if the PIN is not already at max length.
    /// Non-digit characters are silently ignored.
    pub fn push_digit(&mut self, d: char) {
        if d.is_ascii_digit() && self.entered.len() < self.max_length {
            self.entered.push(d);
        }
    }

    /// Remove the last entered digit. No-op when empty.
    pub fn backspace(&mut self) {
        self.entered.pop();
    }

    /// Reset both the entered digits and any error message.
    pub fn clear(&mut self) {
        self.entered.clear();
        self.error_message = None;
    }

    /// Record an authentication error. Clears the entered digits so the
    /// user can retry immediately without manually deleting.
    pub fn set_error(&mut self, msg: &str) {
        self.entered.clear();
        self.error_message = Some(msg.to_string());
    }
}

// ---------------------------------------------------------------------------
// Views
// ---------------------------------------------------------------------------

/// Render a row of circles representing entered and remaining PIN digits.
///
/// Filled circles (●) for entered digits, empty circles (○) for the
/// remaining slots. The total count is at least `MIN_DOT_COUNT` so
/// short PINs still look balanced. `shake_offset` shifts the row
/// horizontally for the wrong-PIN animation.
pub fn view_pin_dots<'a, M: 'a>(state: &PinPadState) -> Element<'a, M> {
    let total = state.entered.len().max(MIN_DOT_COUNT);

    let mut dots = String::with_capacity(total * 4);
    for i in 0..total {
        if i > 0 {
            dots.push(' ');
        }
        if i < state.entered.len() {
            dots.push('\u{25CF}'); // ● filled
        } else {
            dots.push('\u{25CB}'); // ○ empty
        }
    }

    let dot_text = text(dots)
        .size(DOT_FONT_SIZE)
        .color(Color::WHITE)
        .align_x(iced::alignment::Horizontal::Center);

    // Wrap in a container so we can apply the shake translation via padding.
    // Positive offset shifts right, negative shifts left.
    let shake_padding = Padding::ZERO
        .left(state.shake_offset.max(0.0))
        .right((-state.shake_offset).max(0.0));

    container(dot_text)
        .padding(shake_padding)
        .center_x(Length::Fill)
        .into()
}

/// Render the full PIN pad: dots, optional error message, and 3x4 button grid.
pub fn view_pin_pad(state: &PinPadState) -> Element<'_, PinPadAction> {
    let dots = view_pin_dots::<PinPadAction>(state);

    // Optional error message in red below the dots
    let error_row: Element<'_, PinPadAction> = match &state.error_message {
        Some(msg) => container(
            text(msg.clone())
                .size(14.0)
                .color(Color::from_rgb(1.0, 0.3, 0.3))
                .align_x(iced::alignment::Horizontal::Center),
        )
        .center_x(Length::Fill)
        .padding(Padding::from([4, 0]))
        .into(),
        None => container(text("").size(14.0))
            .padding(Padding::from([4, 0]))
            .into(),
    };

    // 3x4 grid: [1][2][3] / [4][5][6] / [7][8][9] / [<-][0][OK]
    let grid = column![
        digit_row('1', '2', '3'),
        digit_row('4', '5', '6'),
        digit_row('7', '8', '9'),
        bottom_row(),
    ]
    .spacing(BUTTON_SPACING)
    .align_x(Alignment::Center);

    column![dots, error_row, grid]
        .spacing(16)
        .align_x(Alignment::Center)
        .into()
}

/// Build one row of three digit buttons.
fn digit_row<'a>(a: char, b: char, c: char) -> Element<'a, PinPadAction> {
    row![digit_button(a), digit_button(b), digit_button(c),]
        .spacing(BUTTON_SPACING)
        .into()
}

/// Build the bottom row: [backspace] [0] [OK placeholder].
///
/// The OK button is rendered but does nothing here; the parent view
/// handles submission when the PIN reaches the expected length or the
/// user presses Enter.
fn bottom_row<'a>() -> Element<'a, PinPadAction> {
    row![
        action_button("\u{2190}", PinPadAction::Backspace), // ← arrow
        digit_button('0'),
        // OK button is visual-only; submission is handled at the app level.
        // We render a disabled placeholder so the grid stays symmetric.
        ok_placeholder(),
    ]
    .spacing(BUTTON_SPACING)
    .into()
}

/// A single digit button that emits `PinPadAction::Digit(d)` on tap.
fn digit_button<'a>(d: char) -> Element<'a, PinPadAction> {
    let label = d.to_string();

    button(
        container(
            text(label)
                .size(DIGIT_FONT_SIZE)
                .color(Color::WHITE)
                .align_x(iced::alignment::Horizontal::Center),
        )
        .center_x(Length::Fixed(BUTTON_SIZE))
        .center_y(Length::Fixed(BUTTON_SIZE)),
    )
    .width(Length::Fixed(BUTTON_SIZE))
    .height(Length::Fixed(BUTTON_SIZE))
    .on_press(PinPadAction::Digit(d))
    .padding(Padding::ZERO)
    .style(pad_button_style)
    .into()
}

/// An action button (backspace) with a text label.
fn action_button<'a>(label: &str, action: PinPadAction) -> Element<'a, PinPadAction> {
    button(
        container(
            text(label.to_string())
                .size(DIGIT_FONT_SIZE)
                .color(Color::WHITE)
                .align_x(iced::alignment::Horizontal::Center),
        )
        .center_x(Length::Fixed(BUTTON_SIZE))
        .center_y(Length::Fixed(BUTTON_SIZE)),
    )
    .width(Length::Fixed(BUTTON_SIZE))
    .height(Length::Fixed(BUTTON_SIZE))
    .on_press(action)
    .padding(Padding::ZERO)
    .style(pad_button_style)
    .into()
}

/// Visual placeholder for the OK position. Keeps the grid symmetric
/// without binding a message here (submission logic lives in the app).
fn ok_placeholder<'a>() -> Element<'a, PinPadAction> {
    container(
        text("OK")
            .size(DIGIT_FONT_SIZE)
            .color(Color::from_rgba(1.0, 1.0, 1.0, 0.3))
            .align_x(iced::alignment::Horizontal::Center),
    )
    .width(Length::Fixed(BUTTON_SIZE))
    .height(Length::Fixed(BUTTON_SIZE))
    .center_x(Length::Fixed(BUTTON_SIZE))
    .center_y(Length::Fixed(BUTTON_SIZE))
    .style(|_theme: &iced::Theme| container::Style {
        background: Some(iced::Background::Color(Color::from_rgba(
            1.0, 1.0, 1.0, 0.04,
        ))),
        border: iced::Border {
            radius: (BUTTON_SIZE / 2.0).into(),
            ..Default::default()
        },
        ..Default::default()
    })
    .into()
}

/// Shared button style: translucent circular background.
fn pad_button_style(_theme: &iced::Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: Some(iced::Background::Color(Color::from_rgba(
            1.0, 1.0, 1.0, 0.08,
        ))),
        border: iced::Border {
            radius: (BUTTON_SIZE / 2.0).into(),
            ..Default::default()
        },
        text_color: Color::WHITE,
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_digit_appends() {
        let mut state = PinPadState::default();
        state.push_digit('1');
        state.push_digit('2');
        assert_eq!(state.entered, "12");
    }

    #[test]
    fn push_digit_respects_max_length() {
        let mut state = PinPadState::default();
        for d in "123456789".chars() {
            state.push_digit(d);
        }
        // MAX_PIN_LENGTH is 8, so the 9th digit should be rejected
        assert_eq!(state.entered.len(), MAX_PIN_LENGTH);
        assert_eq!(state.entered, "12345678");
    }

    #[test]
    fn push_digit_rejects_non_digit() {
        let mut state = PinPadState::default();
        state.push_digit('a');
        state.push_digit('!');
        state.push_digit(' ');
        assert!(state.entered.is_empty());
    }

    #[test]
    fn backspace_removes_last() {
        let mut state = PinPadState::default();
        state.push_digit('1');
        state.push_digit('2');
        state.backspace();
        assert_eq!(state.entered, "1");
    }

    #[test]
    fn backspace_on_empty_is_noop() {
        let mut state = PinPadState::default();
        state.backspace(); // should not panic
        assert!(state.entered.is_empty());
    }

    #[test]
    fn clear_resets_state() {
        let mut state = PinPadState::default();
        state.push_digit('5');
        state.set_error("bad pin");
        state.clear();
        assert!(state.entered.is_empty());
        assert!(state.error_message.is_none());
    }

    #[test]
    fn set_error_clears_entered() {
        let mut state = PinPadState::default();
        state.push_digit('9');
        state.push_digit('8');
        state.set_error("Wrong PIN");
        assert!(state.entered.is_empty());
        assert_eq!(state.error_message.as_deref(), Some("Wrong PIN"));
    }
}
