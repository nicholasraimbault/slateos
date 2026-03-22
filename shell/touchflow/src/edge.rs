//! Edge detection for touchscreen input.
//!
//! Determines whether a touch position falls within an "edge zone" — a narrow
//! strip along the screen border that triggers edge-specific gestures (dock
//! reveal, back navigation, quick settings, OpenClaw sidebar).

/// Which physical screen edge a touch started near.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

impl Edge {
    /// String label for D-Bus serialization.
    pub fn as_str(&self) -> &'static str {
        match self {
            Edge::Top => "top",
            Edge::Bottom => "bottom",
            Edge::Left => "left",
            Edge::Right => "right",
        }
    }
}

/// Phase of a continuous edge gesture lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GesturePhase {
    /// Finger touched down in an edge zone.
    Start,
    /// Finger moved while tracking an edge gesture.
    Update,
    /// Finger lifted — gesture complete.
    End,
    /// Gesture was cancelled (e.g. another finger joined).
    Cancel,
}

impl GesturePhase {
    /// String label for D-Bus serialization.
    pub fn as_str(&self) -> &'static str {
        match self {
            GesturePhase::Start => "start",
            GesturePhase::Update => "update",
            GesturePhase::End => "end",
            GesturePhase::Cancel => "cancel",
        }
    }
}

/// A continuous edge gesture event emitted during each phase of a top-edge
/// swipe. Other edges still use the single-shot `EdgeSwipe` path; only the
/// top edge emits continuous updates so the quick-settings panel can track
/// finger position in real time.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeGesture {
    /// Which screen edge the gesture started from.
    pub edge: Edge,
    /// Current lifecycle phase.
    pub phase: GesturePhase,
    /// Normalized travel: 0.0 at the edge, 1.0 at full screen height.
    pub progress: f64,
    /// Instantaneous velocity in pixels/second.
    pub velocity: f64,
}

impl EdgeGesture {
    /// Create a new edge gesture event.
    pub fn new(edge: Edge, phase: GesturePhase, progress: f64, velocity: f64) -> Self {
        Self {
            edge,
            phase,
            progress: progress.clamp(0.0, 1.0),
            velocity,
        }
    }
}

/// Configuration for edge zone detection.
#[derive(Debug, Clone)]
pub struct EdgeConfig {
    /// Width of the edge zone in pixels (distance from screen border).
    pub size: u32,
    /// Total screen width in pixels.
    pub screen_width: u32,
    /// Total screen height in pixels.
    pub screen_height: u32,
}

impl Default for EdgeConfig {
    fn default() -> Self {
        Self {
            size: 50,
            screen_width: 2560,
            screen_height: 1600,
        }
    }
}

/// Classify a touch position as being in an edge zone, or `None` if it is in
/// the interior of the screen.
///
/// When a position sits in a corner (overlapping two edge zones), the edge
/// whose border is closest wins.
pub fn classify_edge(x: i32, y: i32, config: &EdgeConfig) -> Option<Edge> {
    let size = config.size as i32;
    let w = config.screen_width as i32;
    let h = config.screen_height as i32;

    // Distance from each border (clamped to non-negative so off-screen
    // coordinates still resolve to the nearest edge).
    let dist_left = x.max(0);
    let dist_right = (w - 1 - x).max(0);
    let dist_top = y.max(0);
    let dist_bottom = (h - 1 - y).max(0);

    // Collect edges the point actually falls within.
    let mut candidates: Vec<(Edge, i32)> = Vec::new();

    if dist_left < size {
        candidates.push((Edge::Left, dist_left));
    }
    if dist_right < size {
        candidates.push((Edge::Right, dist_right));
    }
    if dist_top < size {
        candidates.push((Edge::Top, dist_top));
    }
    if dist_bottom < size {
        candidates.push((Edge::Bottom, dist_bottom));
    }

    // Pick the candidate whose border is closest.
    candidates
        .into_iter()
        .min_by_key(|&(_, dist)| dist)
        .map(|(edge, _)| edge)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> EdgeConfig {
        EdgeConfig::default()
    }

    #[test]
    fn left_edge() {
        let cfg = default_config();
        assert_eq!(classify_edge(10, 600, &cfg), Some(Edge::Left));
        assert_eq!(classify_edge(0, 600, &cfg), Some(Edge::Left));
        assert_eq!(classify_edge(49, 600, &cfg), Some(Edge::Left));
    }

    #[test]
    fn right_edge() {
        let cfg = default_config();
        // 2560 wide: right edge zone starts at 2560 - 50 = 2510
        assert_eq!(classify_edge(2550, 800, &cfg), Some(Edge::Right));
        assert_eq!(classify_edge(2559, 800, &cfg), Some(Edge::Right));
        assert_eq!(classify_edge(2511, 800, &cfg), Some(Edge::Right));
    }

    #[test]
    fn top_edge() {
        let cfg = default_config();
        assert_eq!(classify_edge(640, 0, &cfg), Some(Edge::Top));
        assert_eq!(classify_edge(640, 30, &cfg), Some(Edge::Top));
        assert_eq!(classify_edge(640, 49, &cfg), Some(Edge::Top));
    }

    #[test]
    fn bottom_edge() {
        let cfg = default_config();
        // 1840 tall: bottom edge zone starts at 1840 - 50 = 1790
        assert_eq!(classify_edge(640, 1839, &cfg), Some(Edge::Bottom));
        assert_eq!(classify_edge(640, 1800, &cfg), Some(Edge::Bottom));
    }

    #[test]
    fn center_is_none() {
        let cfg = default_config();
        assert_eq!(classify_edge(640, 920, &cfg), None);
        assert_eq!(classify_edge(500, 300, &cfg), None);
        // Just outside left edge zone
        assert_eq!(classify_edge(50, 600, &cfg), None);
    }

    #[test]
    fn corner_picks_closer_edge() {
        let cfg = default_config();
        // Top-left corner, closer to left border (x=5) than to top (y=20).
        assert_eq!(classify_edge(5, 20, &cfg), Some(Edge::Left));
        // Top-left corner, closer to top border (y=5) than to left (x=20).
        assert_eq!(classify_edge(20, 5, &cfg), Some(Edge::Top));
        // Equidistant: both 10px from border. min_by_key is stable, so Left
        // wins because it appears first in the candidate list.
        assert_eq!(classify_edge(10, 10, &cfg), Some(Edge::Left));
    }

    #[test]
    fn custom_edge_size() {
        let cfg = EdgeConfig {
            size: 100,
            screen_width: 1920,
            screen_height: 1080,
        };
        // 75px from left — inside 100px zone
        assert_eq!(classify_edge(75, 540, &cfg), Some(Edge::Left));
        // 75px from left — outside 50px default but inside 100px custom
        let small = EdgeConfig {
            size: 50,
            ..cfg.clone()
        };
        assert_eq!(classify_edge(75, 540, &small), None);
    }

    #[test]
    fn boundary_values() {
        let cfg = default_config();
        // Exactly at edge zone boundary (size=50, so x=50 is NOT in zone)
        assert_eq!(classify_edge(50, 600, &cfg), None);
        // One pixel inside
        assert_eq!(classify_edge(49, 600, &cfg), Some(Edge::Left));
    }

    // ---- Edge string labels ----

    #[test]
    fn edge_as_str() {
        assert_eq!(Edge::Top.as_str(), "top");
        assert_eq!(Edge::Bottom.as_str(), "bottom");
        assert_eq!(Edge::Left.as_str(), "left");
        assert_eq!(Edge::Right.as_str(), "right");
    }

    // ---- GesturePhase ----

    #[test]
    fn gesture_phase_as_str() {
        assert_eq!(GesturePhase::Start.as_str(), "start");
        assert_eq!(GesturePhase::Update.as_str(), "update");
        assert_eq!(GesturePhase::End.as_str(), "end");
        assert_eq!(GesturePhase::Cancel.as_str(), "cancel");
    }

    // ---- EdgeGesture ----

    #[test]
    fn edge_gesture_constructor_clamps_progress() {
        let g = EdgeGesture::new(Edge::Top, GesturePhase::Update, 1.5, 100.0);
        assert!((g.progress - 1.0).abs() < f64::EPSILON);

        let g = EdgeGesture::new(Edge::Top, GesturePhase::Update, -0.3, 0.0);
        assert!(g.progress.abs() < f64::EPSILON);
    }

    #[test]
    fn edge_gesture_constructor_preserves_valid_progress() {
        let g = EdgeGesture::new(Edge::Top, GesturePhase::Start, 0.0, 0.0);
        assert!(g.progress.abs() < f64::EPSILON);
        assert!(g.velocity.abs() < f64::EPSILON);
        assert_eq!(g.edge, Edge::Top);
        assert_eq!(g.phase, GesturePhase::Start);

        let g = EdgeGesture::new(Edge::Bottom, GesturePhase::End, 0.75, 500.0);
        assert!((g.progress - 0.75).abs() < f64::EPSILON);
        assert!((g.velocity - 500.0).abs() < f64::EPSILON);
    }

    #[test]
    fn edge_gesture_phases_round_trip() {
        for phase in [
            GesturePhase::Start,
            GesturePhase::Update,
            GesturePhase::End,
            GesturePhase::Cancel,
        ] {
            let g = EdgeGesture::new(Edge::Left, phase, 0.5, 200.0);
            assert_eq!(g.phase, phase);
        }
    }
}
