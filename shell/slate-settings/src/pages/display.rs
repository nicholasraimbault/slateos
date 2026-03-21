/// Display settings page: scale factor and rotation lock.
use iced::widget::{checkbox, column, container, slider, text};
use iced::{Element, Length};

use slate_common::settings::DisplaySettings;

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum DisplayMsg {
    ScaleChanged(f64),
    RotationLockToggled(bool),
}

// ---------------------------------------------------------------------------
// Update
// ---------------------------------------------------------------------------

pub fn update(settings: &mut DisplaySettings, msg: DisplayMsg) {
    match msg {
        DisplayMsg::ScaleChanged(v) => {
            // Snap to nearest 0.25 step
            settings.scale_factor = (v * 4.0).round() / 4.0;
            settings.scale_factor = settings.scale_factor.clamp(0.5, 3.0);
        }
        DisplayMsg::RotationLockToggled(v) => {
            settings.rotation_lock = v;
        }
    }
}

// ---------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------

pub fn view(settings: &DisplaySettings) -> Element<'_, DisplayMsg> {
    let scale_label = format!("Scale factor: {:.2}", settings.scale_factor);

    let content = column![
        text("Display").size(24),
        text(scale_label).size(14),
        slider(0.5..=3.0, settings.scale_factor, DisplayMsg::ScaleChanged).step(0.25),
        checkbox("Rotation lock", settings.rotation_lock)
            .on_toggle(DisplayMsg::RotationLockToggled),
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
    fn scale_clamps_to_range() {
        let mut s = DisplaySettings::default();

        update(&mut s, DisplayMsg::ScaleChanged(0.1));
        assert!(s.scale_factor >= 0.5);

        update(&mut s, DisplayMsg::ScaleChanged(5.0));
        assert!(s.scale_factor <= 3.0);
    }

    #[test]
    fn scale_snaps_to_quarter_steps() {
        let mut s = DisplaySettings::default();
        update(&mut s, DisplayMsg::ScaleChanged(1.13));
        // Should snap to 1.0 or 1.25
        let frac = (s.scale_factor * 4.0).fract();
        assert!(
            frac.abs() < 0.01,
            "scale {:.4} not on 0.25 grid",
            s.scale_factor
        );
    }

    #[test]
    fn rotation_lock_toggles() {
        let mut s = DisplaySettings::default();
        let original = s.rotation_lock;
        update(&mut s, DisplayMsg::RotationLockToggled(!original));
        assert_ne!(s.rotation_lock, original);
    }
}
