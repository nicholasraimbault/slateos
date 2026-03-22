// Shared button style helpers for slate-shade.
//
// These styles are used by both the heads-up banner (heads_up.rs) and the
// notification list (notifications.rs). Centralising them here ensures visual
// consistency and avoids duplicate definitions.

use iced::widget::button;
use iced::{Color, Theme};

/// Ghost (transparent) button style for dismiss / expand toggles.
///
/// Used for the ✕ dismiss button and the ▲/▼ expand toggle in group headers.
pub fn ghost_button_style(theme: &Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: None,
        text_color: theme.palette().text,
        border: iced::Border::default(),
        shadow: iced::Shadow::default(),
    }
}

/// Chip-style button for inline action buttons and smart-reply chips.
///
/// Uses a semi-transparent primary colour fill that intensifies on hover/press.
pub fn chip_button_style(theme: &Theme, status: button::Status) -> button::Style {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ghost_button_style_has_no_background() {
        let theme = Theme::default();
        let style = ghost_button_style(&theme, button::Status::Active);
        assert!(style.background.is_none());
    }

    #[test]
    fn chip_button_style_active_has_background() {
        let theme = Theme::default();
        let style = chip_button_style(&theme, button::Status::Active);
        assert!(style.background.is_some());
    }

    #[test]
    fn chip_button_style_hovered_has_higher_alpha() {
        let theme = Theme::default();
        let active = chip_button_style(&theme, button::Status::Active);
        let hovered = chip_button_style(&theme, button::Status::Hovered);
        // Both have a background; hovered should have a higher alpha.
        if let (Some(iced::Background::Color(a)), Some(iced::Background::Color(h))) =
            (active.background, hovered.background)
        {
            assert!(h.a > a.a);
        } else {
            panic!("expected Color backgrounds");
        }
    }
}
