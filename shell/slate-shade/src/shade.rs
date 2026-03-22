// Main shade panel view.
//
// Combines the notification list and quick settings panel into a single
// pull-down surface. The shade position is driven by a spring animation
// that follows EdgeGesture progress from TouchFlow.
//
// Layout:
//   Phone        — notifications + quick settings stacked vertically
//   TabletDesktop — split: notifications left, quick settings right

use iced::widget::{column, container, row};
use iced::{Border, Color, Element, Length, Theme};

use slate_common::physics::Spring;
use slate_common::Palette;

use crate::layout::LayoutMode;
use crate::notifications::{view_notifications, NotifAction, NotificationGroup};
use crate::quick_settings::{view_quick_settings, QsAction, QuickSettingsState};

// ---------------------------------------------------------------------------
// Layout constants
// ---------------------------------------------------------------------------

/// Fraction of the screen height the shade occupies when fully open.
const SHADE_OPEN_FRACTION: f32 = 0.65;

/// Spring used for the pull-down animation.
const SHADE_SPRING: Spring = Spring::RESPONSIVE;

/// Settle threshold for the spring (logical pixels).
const SETTLE_THRESHOLD: f64 = 0.5;

/// Width fraction for the notification column in split (tablet/desktop) mode.
const NOTIF_COLUMN_FRACTION: f32 = 0.58;

/// Padding around the shade surface content.
const SHADE_PADDING: f32 = 16.0;

/// Corner radius at the bottom corners of the shade.
const SHADE_CORNER_RADIUS: f32 = 20.0;

/// Drag handle height above the main content.
const HANDLE_HEIGHT: f32 = 24.0;

// ---------------------------------------------------------------------------
// Shade animation state
// ---------------------------------------------------------------------------

/// Direction the shade is currently animating.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShadeDirection {
    Opening,
    Closing,
    /// Not animating; held at a fixed position.
    Idle,
}

/// Full animation state for the shade pull-down.
///
/// `position` is a value in [0, 1] where 0 = fully hidden and 1 = fully open.
/// The spring drives `position` toward `target` on each animation tick.
#[derive(Debug, Clone)]
pub struct ShadeAnimState {
    /// Current open fraction [0.0, 1.0].
    pub position: f64,
    /// Target open fraction (0.0 or 1.0).
    pub target: f64,
    /// Spring velocity in units/second.
    pub velocity: f64,
    /// Spring parameters.
    pub spring: Spring,
    /// Nominal direction (used for gesture commit/cancel logic).
    pub direction: ShadeDirection,
}

impl Default for ShadeAnimState {
    fn default() -> Self {
        Self {
            position: 0.0,
            target: 0.0,
            velocity: 0.0,
            spring: SHADE_SPRING,
            direction: ShadeDirection::Idle,
        }
    }
}

impl ShadeAnimState {
    /// Whether the shade is considered open (position > 0.5 and settled).
    pub fn is_open(&self) -> bool {
        self.position > 0.5
    }

    /// Whether the shade is fully closed and settled.
    pub fn is_closed(&self) -> bool {
        self.spring
            .is_settled(self.position, self.velocity, SETTLE_THRESHOLD)
            && self.position < 0.05
    }

    /// Whether the spring has settled at its target.
    pub fn is_settled(&self) -> bool {
        let disp = self.position - self.target;
        self.spring
            .is_settled(disp, self.velocity, SETTLE_THRESHOLD)
    }

    /// Set the target to open (1.0) and start the opening animation.
    pub fn open(&mut self) {
        self.target = 1.0;
        self.direction = ShadeDirection::Opening;
    }

    /// Set the target to closed (0.0) and start the closing animation.
    pub fn close(&mut self) {
        self.target = 0.0;
        self.direction = ShadeDirection::Closing;
    }

    /// Advance the spring by `dt` seconds.
    ///
    /// Call this on every animation-tick message while `!is_settled()`.
    pub fn step(&mut self, dt: f64) {
        let displacement = self.position - self.target;
        let (new_disp, new_vel) = self.spring.step(displacement, self.velocity, dt);
        self.position = (self.target + new_disp).clamp(0.0, 1.0);
        self.velocity = new_vel;

        // Clamp settled state to avoid floating-point drift.
        if self.is_settled() {
            self.position = self.target;
            self.velocity = 0.0;
            self.direction = ShadeDirection::Idle;
        }
    }

    /// Update position directly from a gesture progress value [0, 1].
    ///
    /// Also captures the gesture velocity so it can seed the spring when the
    /// gesture ends.
    pub fn set_from_gesture(&mut self, progress: f64, velocity: f64) {
        self.position = progress.clamp(0.0, 1.0);
        self.velocity = velocity;
        self.direction = ShadeDirection::Opening;
    }

    /// Commit or cancel the gesture based on current position and velocity.
    ///
    /// If progress is above 0.5 OR velocity is downward (positive), open fully.
    /// Otherwise snap back to closed.
    pub fn commit_gesture(&mut self) {
        if self.position >= 0.5 || self.velocity > 50.0 {
            self.open();
        } else {
            self.close();
        }
    }
}

// ---------------------------------------------------------------------------
// Shade action type
// ---------------------------------------------------------------------------

/// Actions emitted by the shade view.
#[derive(Debug, Clone)]
pub enum ShadeAction {
    Notification(NotifAction),
    QuickSettings(QsAction),
    CloseRequested,
}

// ---------------------------------------------------------------------------
// Shade view
// ---------------------------------------------------------------------------

/// Render the shade panel surface.
///
/// `screen_height` is in logical pixels — used to compute the actual pixel
/// height from the open fraction. The returned element is sized to fill the
/// layer-shell surface width and the computed shade height.
pub fn view_shade<'a>(
    anim: &ShadeAnimState,
    groups: &'a [NotificationGroup],
    smart_replies: &'a std::collections::HashMap<uuid::Uuid, Vec<String>>,
    qs_state: &'a QuickSettingsState,
    layout: LayoutMode,
    palette: &Palette,
    screen_height: f32,
) -> Element<'a, ShadeAction> {
    let shade_height = (anim.position as f32 * screen_height * SHADE_OPEN_FRACTION).max(0.0);

    if shade_height < 2.0 {
        // Fully hidden — return zero-height spacer.
        return iced::widget::Space::new(Length::Fill, Length::Fixed(0.0)).into();
    }

    let bg = shade_background(palette);
    let content = shade_content(groups, smart_replies, qs_state, layout, palette);

    container(
        column![
            drag_handle(palette),
            container(content)
                .height(Length::Fill)
                .padding(SHADE_PADDING)
        ]
        .spacing(0.0),
    )
    .width(Length::Fill)
    .height(Length::Fixed(shade_height))
    .style(move |_theme: &Theme| container::Style {
        background: Some(iced::Background::Color(bg)),
        border: Border {
            radius: iced::border::Radius {
                top_left: 0.0,
                top_right: 0.0,
                bottom_left: SHADE_CORNER_RADIUS,
                bottom_right: SHADE_CORNER_RADIUS,
            },
            ..Default::default()
        },
        ..Default::default()
    })
    .into()
}

/// Render the small drag handle at the top of the shade.
fn drag_handle<'a>(palette: &Palette) -> Element<'a, ShadeAction> {
    let handle_color = {
        let c = Palette::color_to_iced(palette.neutral);
        Color { a: 0.4, ..c }
    };

    container(
        container(iced::widget::Space::new(
            Length::Fixed(40.0),
            Length::Fixed(4.0),
        ))
        .style(move |_theme: &Theme| container::Style {
            background: Some(iced::Background::Color(handle_color)),
            border: Border {
                radius: 2.0.into(),
                ..Default::default()
            },
            ..Default::default()
        }),
    )
    .center_x(Length::Fill)
    .height(Length::Fixed(HANDLE_HEIGHT))
    .into()
}

/// Render the shade interior based on layout mode.
fn shade_content<'a>(
    groups: &'a [NotificationGroup],
    smart_replies: &'a std::collections::HashMap<uuid::Uuid, Vec<String>>,
    qs_state: &'a QuickSettingsState,
    layout: LayoutMode,
    palette: &Palette,
) -> Element<'a, ShadeAction> {
    let notif_view =
        view_notifications(groups, smart_replies, palette).map(ShadeAction::Notification);
    let qs_view = view_quick_settings(qs_state, palette).map(ShadeAction::QuickSettings);

    match layout {
        LayoutMode::Phone => {
            // Stack quick settings above notifications.
            column![
                container(qs_view).width(Length::Fill),
                container(notif_view)
                    .width(Length::Fill)
                    .height(Length::Fill),
            ]
            .spacing(SHADE_PADDING)
            .into()
        }
        LayoutMode::TabletDesktop => {
            // Side-by-side: notifications left, quick settings right.
            let notif_width = Length::FillPortion((NOTIF_COLUMN_FRACTION * 100.0) as u16);
            let qs_width = Length::FillPortion(((1.0 - NOTIF_COLUMN_FRACTION) * 100.0) as u16);
            row![
                container(notif_view)
                    .width(notif_width)
                    .height(Length::Fill),
                container(qs_view).width(qs_width).height(Length::Fill),
            ]
            .spacing(SHADE_PADDING)
            .into()
        }
    }
}

/// Semi-transparent shade background colour.
fn shade_background(palette: &Palette) -> Color {
    let s = palette.surface;
    Color::from_rgba8(s[0], s[1], s[2], 0.92)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shade_anim_default_is_closed() {
        let anim = ShadeAnimState::default();
        assert!((anim.position - 0.0).abs() < 1e-9);
        assert!(!anim.is_open());
    }

    #[test]
    fn shade_anim_open_sets_target_1() {
        let mut anim = ShadeAnimState::default();
        anim.open();
        assert!((anim.target - 1.0).abs() < 1e-9);
        assert_eq!(anim.direction, ShadeDirection::Opening);
    }

    #[test]
    fn shade_anim_close_sets_target_0() {
        let mut anim = ShadeAnimState::default();
        anim.open();
        anim.close();
        assert!((anim.target - 0.0).abs() < 1e-9);
        assert_eq!(anim.direction, ShadeDirection::Closing);
    }

    #[test]
    fn shade_anim_step_moves_toward_target() {
        let mut anim = ShadeAnimState::default();
        anim.open();
        let pos_before = anim.position;
        anim.step(1.0 / 60.0);
        // After one step toward target=1.0 position should have increased.
        assert!(anim.position >= pos_before);
    }

    #[test]
    fn shade_anim_step_settles_eventually() {
        let mut anim = ShadeAnimState::default();
        anim.open();
        anim.position = 1.0;
        anim.velocity = 0.0;
        // Displacement is zero already — should be settled.
        anim.step(1.0 / 60.0);
        assert!(anim.is_settled());
    }

    #[test]
    fn shade_anim_is_closed_when_near_zero() {
        let mut anim = ShadeAnimState::default();
        anim.position = 0.0;
        anim.velocity = 0.0;
        assert!(anim.is_closed());
    }

    #[test]
    fn shade_anim_set_from_gesture_clamps_position() {
        let mut anim = ShadeAnimState::default();
        anim.set_from_gesture(1.5, 0.0);
        assert!((anim.position - 1.0).abs() < 1e-9);
        anim.set_from_gesture(-0.5, 0.0);
        assert!((anim.position - 0.0).abs() < 1e-9);
    }

    #[test]
    fn shade_anim_commit_gesture_opens_when_above_half() {
        let mut anim = ShadeAnimState::default();
        anim.set_from_gesture(0.6, 0.0);
        anim.commit_gesture();
        assert!((anim.target - 1.0).abs() < 1e-9);
    }

    #[test]
    fn shade_anim_commit_gesture_closes_when_below_half_and_slow() {
        let mut anim = ShadeAnimState::default();
        anim.set_from_gesture(0.3, 0.0);
        anim.commit_gesture();
        assert!((anim.target - 0.0).abs() < 1e-9);
    }

    #[test]
    fn shade_anim_commit_gesture_opens_on_fast_downward_velocity() {
        let mut anim = ShadeAnimState::default();
        anim.set_from_gesture(0.2, 200.0);
        anim.commit_gesture();
        assert!((anim.target - 1.0).abs() < 1e-9);
    }

    #[test]
    fn shade_direction_is_copy() {
        let d = ShadeDirection::Opening;
        let _copy = d;
        assert_eq!(d, ShadeDirection::Opening);
    }

    #[test]
    fn shade_action_is_debug_clone() {
        let action = ShadeAction::CloseRequested;
        let _cloned = action.clone();
        let _debug = format!("{action:?}");
    }

    #[test]
    fn view_shade_hidden_when_position_zero() {
        let anim = ShadeAnimState::default();
        let groups = vec![];
        let replies = std::collections::HashMap::new();
        let qs = QuickSettingsState::default();
        let palette = Palette::default();
        let _el: Element<'_, ShadeAction> = view_shade(
            &anim,
            &groups,
            &replies,
            &qs,
            LayoutMode::TabletDesktop,
            &palette,
            800.0,
        );
    }
}
