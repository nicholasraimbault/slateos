//! Magnification math for Shoal's dock icons.
//!
//! Implements the macOS-style Gaussian magnification effect: icons near the
//! touch/cursor position scale up smoothly, falling off with distance.

/// Calculate the visual scale factor for each icon given a touch position.
///
/// Uses a Gaussian falloff so icons directly under the touch point reach
/// `max_scale` while distant icons stay at 1.0.
///
/// # Arguments
/// * `touch_x` - horizontal touch/cursor position, or `None` if no touch is
///   active (all icons return scale 1.0)
/// * `icon_centers` - the X coordinate of each icon's centre
/// * `base_size` - icon size at rest (e.g. 44 px)
/// * `max_scale` - maximum scale factor (e.g. 1.5)
/// * `spread` - Gaussian sigma controlling how far the magnification extends
///   (default ~80 px)
pub fn calculate_scales(
    touch_x: Option<f64>,
    icon_centers: &[f64],
    _base_size: f64,
    max_scale: f64,
    spread: f64,
) -> Vec<f64> {
    let tx = match touch_x {
        Some(x) => x,
        None => return vec![1.0; icon_centers.len()],
    };

    icon_centers
        .iter()
        .map(|&center| {
            let distance = (center - tx).abs();
            let exponent = -(distance / spread).powi(2);
            1.0 + (max_scale - 1.0) * exponent.exp()
        })
        .collect()
}

/// Total width of the dock given per-icon scales and base size.
///
/// Useful for re-centering the dock when magnification changes the overall
/// width.
#[allow(dead_code)] // Public utility, will be used when dock centering is refined
pub fn total_dock_width(scales: &[f64], base_size: f64, gap: f64) -> f64 {
    if scales.is_empty() {
        return 0.0;
    }

    let icons_width: f64 = scales.iter().map(|s| s * base_size).sum();
    let gaps = (scales.len() as f64 - 1.0) * gap;
    icons_width + gaps
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: f64 = 44.0;
    const MAX_SCALE: f64 = 1.5;
    const SPREAD: f64 = 80.0;

    fn icon_centers(count: usize) -> Vec<f64> {
        // Evenly spaced icons starting at x=50, gap of 50px
        (0..count).map(|i| 50.0 + i as f64 * 50.0).collect()
    }

    #[test]
    fn touch_directly_over_icon_gives_max_scale() {
        let centers = icon_centers(5);
        let scales = calculate_scales(Some(centers[2]), &centers, BASE, MAX_SCALE, SPREAD);

        // The icon directly under the touch should be at max scale
        let expected_max = MAX_SCALE;
        assert!(
            (scales[2] - expected_max).abs() < 0.001,
            "expected {expected_max}, got {}",
            scales[2]
        );
    }

    #[test]
    fn no_touch_gives_all_ones() {
        let centers = icon_centers(5);
        let scales = calculate_scales(None, &centers, BASE, MAX_SCALE, SPREAD);

        for (i, &s) in scales.iter().enumerate() {
            assert!(
                (s - 1.0).abs() < f64::EPSILON,
                "icon {i} should be 1.0, got {s}"
            );
        }
    }

    #[test]
    fn touch_far_away_gives_near_ones() {
        let centers = icon_centers(5);
        // Touch position very far from all icons
        let scales = calculate_scales(Some(10_000.0), &centers, BASE, MAX_SCALE, SPREAD);

        for (i, &s) in scales.iter().enumerate() {
            assert!(
                (s - 1.0).abs() < 0.001,
                "icon {i} should be ~1.0 when touch is far away, got {s}"
            );
        }
    }

    #[test]
    fn gaussian_falloff_is_symmetric() {
        let centers = icon_centers(5);
        let touch_x = centers[2]; // middle icon
        let scales = calculate_scales(Some(touch_x), &centers, BASE, MAX_SCALE, SPREAD);

        // Icons equidistant from the touch should have equal scales
        assert!(
            (scales[1] - scales[3]).abs() < 0.001,
            "symmetry: icon 1 ({}) != icon 3 ({})",
            scales[1],
            scales[3]
        );
        assert!(
            (scales[0] - scales[4]).abs() < 0.001,
            "symmetry: icon 0 ({}) != icon 4 ({})",
            scales[0],
            scales[4]
        );
    }

    #[test]
    fn scales_decrease_with_distance() {
        let centers = icon_centers(5);
        let touch_x = centers[2];
        let scales = calculate_scales(Some(touch_x), &centers, BASE, MAX_SCALE, SPREAD);

        // Center icon > adjacent > further
        assert!(
            scales[2] > scales[1],
            "center should be larger than adjacent"
        );
        assert!(scales[1] > scales[0], "adjacent should be larger than far");
    }

    #[test]
    fn all_scales_are_at_least_one() {
        let centers = icon_centers(10);
        let scales = calculate_scales(Some(centers[3]), &centers, BASE, MAX_SCALE, SPREAD);

        for (i, &s) in scales.iter().enumerate() {
            assert!(s >= 1.0, "icon {i} scale {s} should be >= 1.0");
        }
    }

    #[test]
    fn empty_icon_list() {
        let scales = calculate_scales(Some(100.0), &[], BASE, MAX_SCALE, SPREAD);
        assert!(scales.is_empty());
    }

    #[test]
    fn total_dock_width_no_magnification() {
        let scales = vec![1.0; 5];
        let width = total_dock_width(&scales, 44.0, 4.0);
        // 5 icons * 44 + 4 gaps * 4 = 220 + 16 = 236
        assert!((width - 236.0).abs() < 0.01);
    }

    #[test]
    fn total_dock_width_empty() {
        let width = total_dock_width(&[], 44.0, 4.0);
        assert!((width - 0.0).abs() < 0.01);
    }
}
