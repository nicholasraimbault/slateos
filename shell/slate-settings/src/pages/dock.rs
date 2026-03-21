/// Dock settings page: auto-hide toggle and icon size slider.
use iced::widget::{checkbox, column, container, slider, text};
use iced::{Element, Length};

use slate_common::settings::DockSettings;

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum DockMsg {
    AutoHideToggled(bool),
    IconSizeChanged(f64),
}

// ---------------------------------------------------------------------------
// Update
// ---------------------------------------------------------------------------

pub fn update(settings: &mut DockSettings, msg: DockMsg) {
    match msg {
        DockMsg::AutoHideToggled(v) => {
            settings.auto_hide = v;
        }
        DockMsg::IconSizeChanged(v) => {
            // Snap to step of 4, clamp to 32-64
            let snapped = ((v / 4.0).round() * 4.0) as u32;
            settings.icon_size = snapped.clamp(32, 64);
        }
    }
}

// ---------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------

pub fn view(settings: &DockSettings) -> Element<'_, DockMsg> {
    let size_label = format!("Icon size: {}px", settings.icon_size);

    let content = column![
        text("Dock").size(24),
        checkbox("Auto-hide dock", settings.auto_hide).on_toggle(DockMsg::AutoHideToggled),
        text(size_label).size(14),
        slider(
            32.0..=64.0,
            settings.icon_size as f64,
            DockMsg::IconSizeChanged
        )
        .step(4.0),
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
    fn icon_size_snaps_to_step_of_four() {
        let mut s = DockSettings::default();
        update(&mut s, DockMsg::IconSizeChanged(45.0));
        assert_eq!(s.icon_size % 4, 0);
    }

    #[test]
    fn icon_size_clamped() {
        let mut s = DockSettings::default();
        update(&mut s, DockMsg::IconSizeChanged(10.0));
        assert!(s.icon_size >= 32);
        update(&mut s, DockMsg::IconSizeChanged(100.0));
        assert!(s.icon_size <= 64);
    }

    #[test]
    fn auto_hide_toggles() {
        let mut s = DockSettings::default();
        assert!(!s.auto_hide);
        update(&mut s, DockMsg::AutoHideToggled(true));
        assert!(s.auto_hide);
    }
}
