/// Edge detection for touchscreen input.
///
/// Determines whether a touch position falls within an "edge zone" — a narrow
/// strip along the screen border that triggers edge-specific gestures (dock
/// reveal, back navigation, quick settings, OpenClaw sidebar).

/// Which physical screen edge a touch started near.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Top,
    Bottom,
    Left,
    Right,
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
        // ONN 11 Tablet Pro: 1280x1840 native resolution
        Self {
            size: 50,
            screen_width: 1280,
            screen_height: 1840,
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
        // 1280 wide: right edge zone starts at 1280 - 50 = 1230
        assert_eq!(classify_edge(1270, 900, &cfg), Some(Edge::Right));
        assert_eq!(classify_edge(1279, 900, &cfg), Some(Edge::Right));
        // Just inside edge zone
        assert_eq!(classify_edge(1231, 900, &cfg), Some(Edge::Right));
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
}
