/// Display settings page: scale factor, rotation lock, and brightness.
///
/// Brightness is controlled via sysfs on Linux and is a system-level
/// operation that can fail if the backlight device is missing or
/// permissions are insufficient.
use iced::widget::{checkbox, column, container, slider, text};
use iced::{Element, Length};

use slate_common::settings::DisplaySettings;

/// Standard sysfs path for the backlight device.
const BACKLIGHT_BRIGHTNESS: &str = "/sys/class/backlight/panel0-backlight/brightness";
const BACKLIGHT_MAX: &str = "/sys/class/backlight/panel0-backlight/max_brightness";

// ---------------------------------------------------------------------------
// Brightness state
// ---------------------------------------------------------------------------

/// Brightness information gathered from sysfs. Kept separate from the
/// persisted DisplaySettings because brightness is ephemeral system state.
#[derive(Debug, Clone)]
pub struct BrightnessState {
    /// Current brightness 0-100 as a percentage.
    pub current: f64,
    /// Maximum raw value from sysfs (used to scale the percentage).
    pub max_raw: u32,
    /// Whether the backlight sysfs node was found.
    pub available: bool,
    /// Last error from a brightness operation, shown inline.
    pub error: Option<String>,
}

impl Default for BrightnessState {
    fn default() -> Self {
        Self {
            current: 50.0,
            max_raw: 255,
            available: false,
            error: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum DisplayMsg {
    ScaleChanged(f64),
    RotationLockToggled(bool),
    BrightnessChanged(f64),
    BrightnessLoaded(BrightnessState),
    BrightnessApplyResult(Result<(), String>),
}

// ---------------------------------------------------------------------------
// System interaction
// ---------------------------------------------------------------------------

/// Read the current brightness from sysfs and return state. Gracefully
/// produces `available = false` when the backlight node does not exist.
pub async fn read_brightness() -> BrightnessState {
    let max_result = tokio::fs::read_to_string(BACKLIGHT_MAX).await;
    let cur_result = tokio::fs::read_to_string(BACKLIGHT_BRIGHTNESS).await;

    let (max_raw, current_raw) = match (max_result, cur_result) {
        (Ok(max_str), Ok(cur_str)) => {
            let max_val = max_str.trim().parse::<u32>().unwrap_or(255);
            let cur_val = cur_str.trim().parse::<u32>().unwrap_or(0);
            (max_val, cur_val)
        }
        _ => {
            tracing::debug!("backlight sysfs not found, brightness control unavailable");
            return BrightnessState {
                available: false,
                ..Default::default()
            };
        }
    };

    let pct = if max_raw > 0 {
        (current_raw as f64 / max_raw as f64) * 100.0
    } else {
        0.0
    };

    BrightnessState {
        current: pct,
        max_raw,
        available: true,
        error: None,
    }
}

/// Write a brightness percentage to sysfs. Requires appropriate
/// permissions (typically root or a udev rule granting backlight access).
pub async fn apply_brightness(pct: f64, max_raw: u32) -> Result<(), String> {
    let raw = ((pct / 100.0) * max_raw as f64).round() as u32;
    // Clamp to at least 1 so the screen never goes fully dark
    let raw = raw.clamp(1, max_raw);

    tokio::fs::write(BACKLIGHT_BRIGHTNESS, raw.to_string())
        .await
        .map_err(|e| format!("Cannot set brightness: {e}"))
}

// ---------------------------------------------------------------------------
// Update
// ---------------------------------------------------------------------------

pub fn update(
    settings: &mut DisplaySettings,
    brightness: &mut BrightnessState,
    msg: DisplayMsg,
) -> Option<iced::Task<DisplayMsg>> {
    match msg {
        DisplayMsg::ScaleChanged(v) => {
            // Snap to nearest 0.25 step
            settings.scale_factor = (v * 4.0).round() / 4.0;
            settings.scale_factor = settings.scale_factor.clamp(0.5, 3.0);
            None
        }
        DisplayMsg::RotationLockToggled(v) => {
            settings.rotation_lock = v;
            None
        }
        DisplayMsg::BrightnessChanged(pct) => {
            brightness.current = pct.clamp(1.0, 100.0);
            brightness.error = None;
            let max_raw = brightness.max_raw;
            let target = brightness.current;
            let task = iced::Task::perform(
                async move { apply_brightness(target, max_raw).await },
                DisplayMsg::BrightnessApplyResult,
            );
            Some(task)
        }
        DisplayMsg::BrightnessLoaded(state) => {
            *brightness = state;
            None
        }
        DisplayMsg::BrightnessApplyResult(result) => {
            if let Err(e) = result {
                tracing::warn!("brightness control failed: {e}");
                brightness.error = Some(e);
            }
            None
        }
    }
}

// ---------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------

pub fn view<'a>(
    settings: &'a DisplaySettings,
    brightness: &'a BrightnessState,
) -> Element<'a, DisplayMsg> {
    let scale_label = format!("Scale factor: {:.2}", settings.scale_factor);

    let mut items: Vec<Element<'a, DisplayMsg>> = vec![
        text("Display").size(24).into(),
        text(scale_label).size(14).into(),
        slider(0.5..=3.0, settings.scale_factor, DisplayMsg::ScaleChanged)
            .step(0.25)
            .into(),
        checkbox("Rotation lock", settings.rotation_lock)
            .on_toggle(DisplayMsg::RotationLockToggled)
            .into(),
    ];

    // Brightness section — only shown when backlight hardware exists
    if brightness.available {
        let bright_label = format!("Brightness: {:.0}%", brightness.current);
        items.push(text(bright_label).size(14).into());
        items.push(
            slider(
                1.0..=100.0,
                brightness.current,
                DisplayMsg::BrightnessChanged,
            )
            .step(1.0)
            .into(),
        );

        if let Some(err) = &brightness.error {
            items.push(
                text(err)
                    .size(12)
                    .color(iced::Color::from_rgb(0.9, 0.2, 0.2))
                    .into(),
            );
        }
    } else {
        items.push(
            text("Brightness control not available on this device")
                .size(12)
                .color(iced::Color::from_rgb(0.5, 0.5, 0.5))
                .into(),
        );
    }

    let content = column(items).spacing(16).padding(20).width(Length::Fill);

    container(content).width(Length::Fill).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_clamps_to_range() {
        let mut s = DisplaySettings::default();
        let mut b = BrightnessState::default();

        update(&mut s, &mut b, DisplayMsg::ScaleChanged(0.1));
        assert!(s.scale_factor >= 0.5);

        update(&mut s, &mut b, DisplayMsg::ScaleChanged(5.0));
        assert!(s.scale_factor <= 3.0);
    }

    #[test]
    fn scale_snaps_to_quarter_steps() {
        let mut s = DisplaySettings::default();
        let mut b = BrightnessState::default();
        update(&mut s, &mut b, DisplayMsg::ScaleChanged(1.13));
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
        let mut b = BrightnessState::default();
        let original = s.rotation_lock;
        update(&mut s, &mut b, DisplayMsg::RotationLockToggled(!original));
        assert_ne!(s.rotation_lock, original);
    }

    #[test]
    fn brightness_clamps_to_valid_range() {
        let mut s = DisplaySettings::default();
        let mut b = BrightnessState {
            available: true,
            max_raw: 255,
            ..Default::default()
        };

        update(&mut s, &mut b, DisplayMsg::BrightnessChanged(0.0));
        assert!(b.current >= 1.0);

        update(&mut s, &mut b, DisplayMsg::BrightnessChanged(200.0));
        assert!(b.current <= 100.0);
    }

    #[test]
    fn brightness_error_stored_on_failure() {
        let mut s = DisplaySettings::default();
        let mut b = BrightnessState {
            available: true,
            ..Default::default()
        };

        update(
            &mut s,
            &mut b,
            DisplayMsg::BrightnessApplyResult(Err("permission denied".into())),
        );
        assert_eq!(b.error.as_deref(), Some("permission denied"));
    }

    #[test]
    fn default_brightness_state_is_unavailable() {
        let b = BrightnessState::default();
        assert!(!b.available);
    }

    #[test]
    fn brightness_change_clears_previous_error() {
        let mut s = DisplaySettings::default();
        let mut b = BrightnessState {
            available: true,
            error: Some("old error".into()),
            max_raw: 255,
            ..Default::default()
        };
        update(&mut s, &mut b, DisplayMsg::BrightnessChanged(50.0));
        assert!(b.error.is_none());
    }
}
