/// Gestures settings page: master toggle, sensitivity, edge zone size.
use iced::widget::{checkbox, column, container, slider, text};
use iced::{Element, Length};

use slate_common::settings::GestureSettings;

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum GestureMsg {
    EnabledToggled(bool),
    SensitivityChanged(f64),
    EdgeSizeChanged(f64),
}

// ---------------------------------------------------------------------------
// Update
// ---------------------------------------------------------------------------

pub fn update(settings: &mut GestureSettings, msg: GestureMsg) {
    match msg {
        GestureMsg::EnabledToggled(v) => {
            settings.enabled = v;
        }
        GestureMsg::SensitivityChanged(v) => {
            settings.sensitivity = clamp_sensitivity(v);
        }
        GestureMsg::EdgeSizeChanged(v) => {
            // Snap to step of 10, clamp to 20-100
            let snapped = ((v / 10.0).round() * 10.0) as u32;
            settings.edge_size = snapped.clamp(20, 100);
        }
    }
}

/// Clamp sensitivity to valid range and round to 0.1 step.
pub fn clamp_sensitivity(v: f64) -> f64 {
    let rounded = (v * 10.0).round() / 10.0;
    rounded.clamp(0.5, 2.0)
}

// ---------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------

pub fn view(settings: &GestureSettings) -> Element<'_, GestureMsg> {
    let sens_label = format!("Sensitivity: {:.1}", settings.sensitivity);
    let edge_label = format!("Edge zone: {}px", settings.edge_size);

    let content = column![
        text("Gestures").size(24),
        checkbox("Enable gestures", settings.enabled).on_toggle(GestureMsg::EnabledToggled),
        text(sens_label).size(14),
        slider(
            0.5..=2.0,
            settings.sensitivity,
            GestureMsg::SensitivityChanged
        )
        .step(0.1),
        text(edge_label).size(14),
        slider(
            20.0..=100.0,
            settings.edge_size as f64,
            GestureMsg::EdgeSizeChanged
        )
        .step(10.0),
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
    fn sensitivity_clamped_to_valid_range() {
        assert!((0.5..=2.0).contains(&clamp_sensitivity(0.1)));
        assert!((0.5..=2.0).contains(&clamp_sensitivity(3.0)));
        assert!((0.5..=2.0).contains(&clamp_sensitivity(1.0)));
        assert_eq!(clamp_sensitivity(0.1), 0.5);
        assert_eq!(clamp_sensitivity(3.0), 2.0);
        assert_eq!(clamp_sensitivity(1.35), 1.4);
    }

    #[test]
    fn edge_size_snaps_to_step_of_ten() {
        let mut s = GestureSettings::default();
        update(&mut s, GestureMsg::EdgeSizeChanged(37.0));
        assert_eq!(s.edge_size % 10, 0);
    }

    #[test]
    fn edge_size_clamped() {
        let mut s = GestureSettings::default();
        update(&mut s, GestureMsg::EdgeSizeChanged(5.0));
        assert!(s.edge_size >= 20);
        update(&mut s, GestureMsg::EdgeSizeChanged(200.0));
        assert!(s.edge_size <= 100);
    }

    #[test]
    fn enable_toggle_works() {
        let mut s = GestureSettings::default();
        assert!(s.enabled);
        update(&mut s, GestureMsg::EnabledToggled(false));
        assert!(!s.enabled);
    }
}
