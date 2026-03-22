// Panel UI view layer.
//
// Pure view functions for the Claw Panel. Separated from main.rs to keep the
// iced view logic under the 500-line limit and testable in isolation.

use iced::widget::{button, column, container, row, scrollable, text, text_input, Column};
use iced::{alignment, Color, Element, Length, Theme};

use crate::context::WindowContext;
use crate::conversation::{extract_code_blocks, ChatMessage, ChatRole, Conversation};
use crate::toast::ToastState;

/// Identifiers for iced widgets that need stable IDs.
const INPUT_ID: &str = "claw-input";

/// Minimum panel width (25% of a typical 1400px-wide tablet).
pub const MIN_WIDTH: u32 = 350;
/// Maximum panel width (50% of screen).
pub const MAX_WIDTH: u32 = 700;
/// Default panel width (~30%).
pub const DEFAULT_WIDTH: u32 = 400;

// ---------------------------------------------------------------------------
// Message type for panel-internal events
// ---------------------------------------------------------------------------

/// Messages produced by the panel view. The main app maps these into its own
/// top-level message enum.
#[derive(Debug, Clone)]
pub enum PanelAction {
    Close,
    InputChanged(String),
    Send,
    ApplyCodeBlock(String),
}

// ---------------------------------------------------------------------------
// View functions
// ---------------------------------------------------------------------------

/// Build the full panel view.
pub fn view<'a>(
    conversation: &'a Conversation,
    context: &'a Option<WindowContext>,
    input_text: &'a str,
    is_streaming: bool,
    toast_state: &'a ToastState,
    surface_color: [u8; 4],
    rhea_backend: &'a str,
) -> Element<'a, PanelAction> {
    let header = view_header(rhea_backend);
    let context_badge = view_context_badge(context);
    let messages = view_messages(conversation);
    let input_bar = view_input_bar(input_text, is_streaming);
    let toast = crate::toast::view_toast(toast_state, surface_color);

    column![header, context_badge, messages, toast, input_bar]
        .spacing(8)
        .padding(12)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// Header row: "Claw" label + active Rhea backend indicator + close button.
///
/// The backend name is shown in a small muted label so the user can tell at a
/// glance whether inference is running locally or in the cloud.
fn view_header<'a>(rhea_backend: &'a str) -> Element<'a, PanelAction> {
    let backend_label = if rhea_backend.is_empty() {
        text("").size(11)
    } else {
        text(format!("via {rhea_backend}"))
            .size(11)
            .color(Color::from_rgb(0.55, 0.55, 0.55))
    };

    row![
        text("Claw").size(22),
        backend_label,
        iced::widget::horizontal_space(),
        button(text("X").size(16))
            .on_press(PanelAction::Close)
            .padding(6),
    ]
    .align_y(iced::Alignment::Center)
    .spacing(8)
    .width(Length::Fill)
    .into()
}

/// Context badge showing which app/window the user has focused.
fn view_context_badge<'a>(context: &'a Option<WindowContext>) -> Element<'a, PanelAction> {
    let label = match context {
        Some(ctx) => format!("Seeing: {} \u{2014} {}", ctx.app_id, ctx.title),
        None => "No focused window".to_string(),
    };

    container(text(label).size(12).color(Color::from_rgb(0.6, 0.6, 0.6)))
        .padding(6)
        .width(Length::Fill)
        .into()
}

/// Scrollable conversation area.
fn view_messages<'a>(conversation: &'a Conversation) -> Element<'a, PanelAction> {
    let mut col = Column::new().spacing(8).width(Length::Fill);

    for msg in conversation.messages() {
        col = col.push(view_single_message(msg));
    }

    scrollable(col)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// Render a single chat message bubble.
fn view_single_message(msg: &ChatMessage) -> Element<'_, PanelAction> {
    let content_text = text(&msg.content).size(14);

    let mut bubble = Column::new().spacing(4).push(content_text);

    // For assistant messages, detect code blocks and add "Apply fix" buttons.
    if msg.role == ChatRole::Assistant {
        let blocks = extract_code_blocks(&msg.content);
        for (_lang, code) in blocks {
            bubble = bubble.push(
                button(text("Apply fix").size(12))
                    .on_press(PanelAction::ApplyCodeBlock(code))
                    .padding(4),
            );
        }
    }

    let h_align = match msg.role {
        ChatRole::User => alignment::Horizontal::Right,
        ChatRole::Assistant => alignment::Horizontal::Left,
    };

    let bg_color = match msg.role {
        ChatRole::User => Color::from_rgba(0.25, 0.45, 0.75, 0.3),
        ChatRole::Assistant => Color::from_rgba(0.2, 0.2, 0.25, 0.4),
    };

    container(bubble)
        .padding(10)
        .max_width(500)
        .style(move |_theme: &Theme| container::Style {
            background: Some(iced::Background::Color(bg_color)),
            border: iced::Border {
                radius: 8.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .align_x(h_align)
        .into()
}

/// Input bar: text field + send button.
fn view_input_bar<'a>(input_text: &'a str, is_streaming: bool) -> Element<'a, PanelAction> {
    let input = text_input("Ask Claw...", input_text)
        .id(text_input::Id::new(INPUT_ID))
        .on_input(PanelAction::InputChanged)
        .on_submit(PanelAction::Send)
        .padding(10)
        .size(14)
        .width(Length::Fill);

    let send_btn = if is_streaming {
        button(text("...").size(14)).padding(10)
    } else {
        button(text("Send").size(14))
            .on_press(PanelAction::Send)
            .padding(10)
    };

    row![input, send_btn]
        .spacing(8)
        .align_y(iced::Alignment::Center)
        .width(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_width_within_bounds() {
        assert!(DEFAULT_WIDTH >= MIN_WIDTH);
        assert!(DEFAULT_WIDTH <= MAX_WIDTH);
    }

    #[test]
    fn panel_action_is_debug_clone() {
        let action = PanelAction::InputChanged("test".to_string());
        let _cloned = action.clone();
        let _debug = format!("{action:?}");
    }

    #[test]
    fn view_header_builds_with_empty_backend() {
        // Ensures the header renders without panicking when no backend is known.
        let _element = view_header("");
    }

    #[test]
    fn view_header_builds_with_backend_name() {
        // Ensures the backend label is included without panicking.
        let _element = view_header("local");
    }
}
