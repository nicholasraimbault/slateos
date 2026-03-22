/// Notifications settings page.
///
/// Controls do-not-disturb mode, heads-up banner duration, and notification sounds.
use iced::widget::{column, container, text, toggler};
use iced::{Element, Length};

use slate_common::settings::NotificationSettings;

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum NotifMsg {
    DndToggled(bool),
    DurationChanged(f32),
    SoundToggled(bool),
}

// ---------------------------------------------------------------------------
// Update
// ---------------------------------------------------------------------------

pub fn update(settings: &mut NotificationSettings, msg: NotifMsg) {
    match msg {
        NotifMsg::DndToggled(v) => settings.dnd = v,
        NotifMsg::DurationChanged(v) => settings.heads_up_duration_secs = v as u32,
        NotifMsg::SoundToggled(v) => settings.sound_enabled = v,
    }
}

// ---------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------

pub fn view(settings: &NotificationSettings) -> Element<'_, NotifMsg> {
    let items: Vec<Element<'_, NotifMsg>> = vec![
        text("Notifications").size(24).into(),
        toggler(settings.dnd)
            .label("Do Not Disturb")
            .on_toggle(NotifMsg::DndToggled)
            .into(),
        text(format!(
            "Banner duration: {}s",
            settings.heads_up_duration_secs
        ))
        .size(14)
        .into(),
        iced::widget::slider(
            1.0..=10.0,
            settings.heads_up_duration_secs as f32,
            NotifMsg::DurationChanged,
        )
        .step(1.0)
        .into(),
        toggler(settings.sound_enabled)
            .label("Notification sounds")
            .on_toggle(NotifMsg::SoundToggled)
            .into(),
    ];

    let content = column(items).spacing(12).padding(20).width(Length::Fill);
    container(content).width(Length::Fill).into()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dnd_toggled_on() {
        let mut s = NotificationSettings::default();
        update(&mut s, NotifMsg::DndToggled(true));
        assert!(s.dnd);
    }

    #[test]
    fn dnd_toggled_off() {
        let mut s = NotificationSettings {
            dnd: true,
            ..NotificationSettings::default()
        };
        update(&mut s, NotifMsg::DndToggled(false));
        assert!(!s.dnd);
    }

    #[test]
    fn duration_changed_rounds_to_u32() {
        let mut s = NotificationSettings::default();
        update(&mut s, NotifMsg::DurationChanged(7.9));
        assert_eq!(s.heads_up_duration_secs, 7);
    }

    #[test]
    fn duration_min_value() {
        let mut s = NotificationSettings::default();
        update(&mut s, NotifMsg::DurationChanged(1.0));
        assert_eq!(s.heads_up_duration_secs, 1);
    }

    #[test]
    fn duration_max_value() {
        let mut s = NotificationSettings::default();
        update(&mut s, NotifMsg::DurationChanged(10.0));
        assert_eq!(s.heads_up_duration_secs, 10);
    }

    #[test]
    fn sound_toggled_off() {
        let mut s = NotificationSettings::default();
        assert!(s.sound_enabled); // default is true
        update(&mut s, NotifMsg::SoundToggled(false));
        assert!(!s.sound_enabled);
    }

    #[test]
    fn sound_toggled_on() {
        let mut s = NotificationSettings {
            sound_enabled: false,
            ..NotificationSettings::default()
        };
        update(&mut s, NotifMsg::SoundToggled(true));
        assert!(s.sound_enabled);
    }
}
