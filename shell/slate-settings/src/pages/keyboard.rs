/// Keyboard settings page: split mode and suggestion bar toggles.
use iced::widget::{checkbox, column, container, text};
use iced::{Element, Length};

use slate_common::settings::KeyboardSettings;

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum KeyboardMsg {
    SplitModeToggled(bool),
    SuggestionsToggled(bool),
}

// ---------------------------------------------------------------------------
// Update
// ---------------------------------------------------------------------------

pub fn update(settings: &mut KeyboardSettings, msg: KeyboardMsg) {
    match msg {
        KeyboardMsg::SplitModeToggled(v) => {
            settings.split_mode = v;
        }
        KeyboardMsg::SuggestionsToggled(v) => {
            settings.suggestions = v;
        }
    }
}

// ---------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------

pub fn view(settings: &KeyboardSettings) -> Element<'_, KeyboardMsg> {
    let content = column![
        text("Keyboard").size(24),
        checkbox("Split keyboard for thumb typing", settings.split_mode)
            .on_toggle(KeyboardMsg::SplitModeToggled),
        checkbox("Show word suggestions", settings.suggestions)
            .on_toggle(KeyboardMsg::SuggestionsToggled),
    ]
    .spacing(16)
    .padding(20)
    .width(Length::Fill);

    container(content).width(Length::Fill).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_mode_toggles() {
        let mut s = KeyboardSettings::default();
        assert!(!s.split_mode);
        update(&mut s, KeyboardMsg::SplitModeToggled(true));
        assert!(s.split_mode);
    }

    #[test]
    fn suggestions_toggles() {
        let mut s = KeyboardSettings::default();
        assert!(s.suggestions);
        update(&mut s, KeyboardMsg::SuggestionsToggled(false));
        assert!(!s.suggestions);
    }
}
