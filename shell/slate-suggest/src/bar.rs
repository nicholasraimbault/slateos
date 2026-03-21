/// Suggestion bar UI built with iced + iced_layershell.
///
/// Renders a horizontal scrollable row of suggestion "chips" anchored
/// just above the on-screen keyboard. Each chip is a tappable rounded
/// rectangle that injects its text into the focused application.
///
/// LLM-sourced chips get a distinct accent border so the user can
/// visually distinguish AI completions from history-based suggestions.
///
/// On macOS (development), the layer-shell code is gated behind cfg
/// and the bar renders as a regular iced widget tree.
use crate::engine::{Suggestion, SuggestionSource};

use iced::widget::{button, container, row, scrollable, text};
use iced::{Element, Length, Padding};
use slate_common::Palette;

/// Height of the suggestion bar in logical pixels.
pub const BAR_HEIGHT: u16 = 36;

/// Padding inside each chip.
const CHIP_PADDING: Padding = Padding {
    top: 4.0,
    right: 12.0,
    bottom: 4.0,
    left: 12.0,
};

/// Spacing between chips.
const CHIP_SPACING: f32 = 8.0;

/// Horizontal padding for the bar container.
const BAR_PADDING: Padding = Padding {
    top: 2.0,
    right: 8.0,
    bottom: 2.0,
    left: 8.0,
};

/// Border width used on LLM suggestion chips to distinguish them visually.
const LLM_CHIP_BORDER_WIDTH: f32 = 2.0;

/// Messages the bar view can produce.
#[derive(Debug, Clone)]
pub enum BarMessage {
    /// User tapped a suggestion chip.
    ChipTapped(String),
}

/// Build the suggestion bar view as an iced Element.
///
/// The palette controls chip colors:
/// - Chip background: `palette.container`
/// - Chip text: `palette.neutral`
/// - Bar background: `palette.surface`
/// - LLM chip border: `palette.secondary` (accent border for AI suggestions)
pub fn view<'a>(suggestions: &[Suggestion], palette: &Palette) -> Element<'a, BarMessage> {
    let chip_bg = Palette::color_to_iced(palette.container);
    let chip_text_color = Palette::color_to_iced(palette.neutral);
    let bar_bg = Palette::color_to_iced(palette.surface);
    let llm_border_color = Palette::color_to_iced(palette.secondary);

    let chips: Vec<Element<'a, BarMessage>> = suggestions
        .iter()
        .map(|suggestion| {
            let is_llm = suggestion.source == SuggestionSource::Llm;
            let label_text = suggestion.text.clone();
            let label = text(label_text).size(14).color(chip_text_color);

            let chip = button(container(label).padding(CHIP_PADDING))
                .on_press(BarMessage::ChipTapped(suggestion.text.clone()))
                .style(chip_style(chip_bg, is_llm, llm_border_color));

            chip.into()
        })
        .collect();

    let chip_row = row(chips).spacing(CHIP_SPACING);

    let scrollable_row = scrollable(chip_row)
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::default(),
        ))
        .width(Length::Fill);

    container(scrollable_row)
        .width(Length::Fill)
        .height(Length::Fixed(BAR_HEIGHT as f32))
        .padding(BAR_PADDING)
        .style(bar_style(bar_bg))
        .into()
}

/// Create a button style closure for suggestion chips.
///
/// LLM-sourced chips get a coloured border to distinguish them from
/// history suggestions visually.
fn chip_style(
    bg: iced::Color,
    is_llm: bool,
    llm_border_color: iced::Color,
) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let border_color = if is_llm {
            llm_border_color
        } else {
            iced::Color::TRANSPARENT
        };
        let border_width = if is_llm { LLM_CHIP_BORDER_WIDTH } else { 1.0 };

        let base = button::Style {
            background: Some(iced::Background::Color(bg)),
            border: iced::Border {
                color: border_color,
                width: border_width,
                radius: 16.0.into(),
            },
            text_color: iced::Color::WHITE,
            ..button::Style::default()
        };

        match status {
            button::Status::Hovered | button::Status::Pressed => button::Style {
                background: Some(iced::Background::Color(iced::Color {
                    a: bg.a * 0.8,
                    ..bg
                })),
                ..base
            },
            _ => base,
        }
    }
}

/// Create a container style closure for the bar background.
fn bar_style(bg: iced::Color) -> impl Fn(&iced::Theme) -> container::Style {
    move |_theme| container::Style {
        background: Some(iced::Background::Color(bg)),
        ..container::Style::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::SuggestionSource;

    #[test]
    fn bar_height_is_reasonable() {
        assert!(BAR_HEIGHT > 20);
        assert!(BAR_HEIGHT < 100);
    }

    #[test]
    fn view_produces_element_with_suggestions() {
        let suggestions = vec![
            Suggestion {
                text: "git status".to_string(),
                source: SuggestionSource::History,
                score: 0.9,
            },
            Suggestion {
                text: "cargo build".to_string(),
                source: SuggestionSource::Static,
                score: 0.5,
            },
        ];
        let palette = Palette::default();
        let _element: Element<'_, BarMessage> = view(&suggestions, &palette);
    }

    #[test]
    fn view_handles_empty_suggestions() {
        let palette = Palette::default();
        let _element: Element<'_, BarMessage> = view(&[], &palette);
    }

    #[test]
    fn view_handles_llm_suggestion() {
        let suggestions = vec![
            Suggestion {
                text: "git status".to_string(),
                source: SuggestionSource::History,
                score: 0.9,
            },
            Suggestion {
                text: "git push origin main".to_string(),
                source: SuggestionSource::Llm,
                score: 0.85,
            },
        ];
        let palette = Palette::default();
        let _element: Element<'_, BarMessage> = view(&suggestions, &palette);
    }

    #[test]
    fn chip_style_returns_valid_style() {
        let bg = iced::Color::from_rgb(0.2, 0.2, 0.3);
        let border = iced::Color::from_rgb(0.5, 0.7, 1.0);
        let style_fn = chip_style(bg, false, border);
        let theme = iced::Theme::Dark;
        let _normal = style_fn(&theme, button::Status::Active);
        let _hovered = style_fn(&theme, button::Status::Hovered);
    }

    #[test]
    fn chip_style_llm_has_visible_border() {
        let bg = iced::Color::from_rgb(0.2, 0.2, 0.3);
        let border = iced::Color::from_rgb(0.5, 0.7, 1.0);
        let style_fn = chip_style(bg, true, border);
        let theme = iced::Theme::Dark;
        let style = style_fn(&theme, button::Status::Active);
        assert!(
            style.border.width > 1.0,
            "LLM chip should have a wider border"
        );
        assert_ne!(
            style.border.color,
            iced::Color::TRANSPARENT,
            "LLM chip should have a visible border color"
        );
    }

    #[test]
    fn chip_style_history_has_transparent_border() {
        let bg = iced::Color::from_rgb(0.2, 0.2, 0.3);
        let border = iced::Color::from_rgb(0.5, 0.7, 1.0);
        let style_fn = chip_style(bg, false, border);
        let theme = iced::Theme::Dark;
        let style = style_fn(&theme, button::Status::Active);
        assert_eq!(
            style.border.color,
            iced::Color::TRANSPARENT,
            "history chip should have transparent border"
        );
    }

    #[test]
    fn bar_style_returns_valid_style() {
        let bg = iced::Color::from_rgb(0.1, 0.1, 0.15);
        let style_fn = bar_style(bg);
        let theme = iced::Theme::Dark;
        let _style = style_fn(&theme);
    }
}
