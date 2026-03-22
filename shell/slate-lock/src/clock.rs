// Clock and date display for the lock screen.
//
// Renders the current time (24-hour format) and date as a centered column.
// The `large` flag controls font sizing so the clock can shrink when the
// PIN entry field is visible.

use chrono::Local;
use iced::widget::{column, text};
use iced::{Alignment, Element};

/// Render the clock/date widget.
///
/// When `large` is true the time is displayed at 72px and the date at 16px,
/// matching the prominent idle lock screen. When false the sizes drop to
/// 36px / 12px so the clock stays visible above the PIN entry area without
/// dominating the layout.
pub fn view_clock<'a, M: 'a>(large: bool) -> Element<'a, M> {
    let now = Local::now();

    let time_str = now.format("%H:%M").to_string();
    let date_str = now.format("%A, %B %-d").to_string();

    let time_size: f32 = if large { 72.0 } else { 36.0 };
    let date_size: f32 = if large { 16.0 } else { 12.0 };

    column![
        text(time_str).size(time_size),
        text(date_str).size(date_size),
    ]
    .spacing(4)
    .align_x(Alignment::Center)
    .into()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use chrono::Local;

    #[test]
    fn clock_format_is_24h() {
        // "%H:%M" always produces "HH:MM" — exactly 5 characters.
        let s = Local::now().format("%H:%M").to_string();
        assert_eq!(s.len(), 5, "expected 5-char time string, got: {s}");
    }

    #[test]
    fn date_format_has_weekday() {
        // "%A, %B %-d" starts with the full weekday name followed by a comma.
        let s = Local::now().format("%A, %B %-d").to_string();
        assert!(
            s.contains(','),
            "expected a comma separating weekday from month: {s}"
        );
    }
}
