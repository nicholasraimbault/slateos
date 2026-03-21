// Launch feedback state for the app launcher.
//
// Tracks which app (by desktop_id) was most recently tapped so the view can
// render a brief visual pulse. The animation itself is driven by
// iced_anim::AnimationBuilder in the view layer; this module only holds the
// transient state that tells the view *what* to animate.

use std::time::Instant;

/// How long the feedback indicator stays visible before being cleared.
/// This is a safety net; the spring animation may finish sooner.
const FEEDBACK_DURATION_MS: u64 = 600;

/// Tracks the launch-feedback state for a single tap.
#[derive(Debug, Clone)]
pub struct LaunchFeedback {
    /// The desktop_id of the app that was just launched, or `None` when idle.
    active_id: Option<String>,
    /// When the feedback was triggered (used to auto-clear stale feedback).
    triggered_at: Option<Instant>,
}

impl Default for LaunchFeedback {
    fn default() -> Self {
        Self::new()
    }
}

impl LaunchFeedback {
    /// Create a new idle feedback state.
    pub fn new() -> Self {
        Self {
            active_id: None,
            triggered_at: None,
        }
    }

    /// Trigger feedback for the given app. Replaces any existing feedback.
    pub fn trigger(&mut self, desktop_id: &str) {
        self.active_id = Some(desktop_id.to_string());
        self.triggered_at = Some(Instant::now());
    }

    /// Clear the feedback state (e.g. when the launcher is hidden).
    pub fn clear(&mut self) {
        self.active_id = None;
        self.triggered_at = None;
    }

    /// Return the opacity multiplier for a given app cell.
    ///
    /// - Returns a dimmed value (0.4) for the app that was just tapped,
    ///   signalling to the AnimationBuilder that this cell should animate.
    /// - Returns 1.0 for all other apps (fully opaque, no animation).
    ///
    /// Stale feedback (older than `FEEDBACK_DURATION_MS`) is automatically
    /// ignored so the UI never gets stuck in a dimmed state.
    pub fn opacity_for(&self, desktop_id: &str) -> f32 {
        if let (Some(ref active), Some(triggered)) = (&self.active_id, self.triggered_at) {
            let elapsed = triggered.elapsed().as_millis() as u64;
            if active == desktop_id && elapsed < FEEDBACK_DURATION_MS {
                return 0.4;
            }
        }
        1.0
    }

    /// Whether any feedback is currently active and not yet expired.
    pub fn is_active(&self) -> bool {
        if let Some(triggered) = self.triggered_at {
            triggered.elapsed().as_millis() < FEEDBACK_DURATION_MS as u128
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_feedback_is_idle() {
        let fb = LaunchFeedback::new();
        assert!(!fb.is_active());
        assert!((fb.opacity_for("any") - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn default_is_same_as_new() {
        let fb = LaunchFeedback::default();
        assert!(!fb.is_active());
    }

    #[test]
    fn trigger_activates_feedback() {
        let mut fb = LaunchFeedback::new();
        fb.trigger("firefox");
        assert!(fb.is_active());
    }

    #[test]
    fn triggered_app_gets_dimmed_opacity() {
        let mut fb = LaunchFeedback::new();
        fb.trigger("firefox");
        let opacity = fb.opacity_for("firefox");
        assert!(
            opacity < 1.0,
            "triggered app should be dimmed, got {opacity}"
        );
    }

    #[test]
    fn other_apps_stay_fully_opaque() {
        let mut fb = LaunchFeedback::new();
        fb.trigger("firefox");
        let opacity = fb.opacity_for("nautilus");
        assert!(
            (opacity - 1.0).abs() < f32::EPSILON,
            "other apps should be 1.0, got {opacity}"
        );
    }

    #[test]
    fn clear_resets_feedback() {
        let mut fb = LaunchFeedback::new();
        fb.trigger("firefox");
        fb.clear();
        assert!(!fb.is_active());
        assert!((fb.opacity_for("firefox") - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn trigger_replaces_previous() {
        let mut fb = LaunchFeedback::new();
        fb.trigger("firefox");
        fb.trigger("nautilus");

        assert!((fb.opacity_for("firefox") - 1.0).abs() < f32::EPSILON);
        assert!(fb.opacity_for("nautilus") < 1.0);
    }
}
