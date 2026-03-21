//! Adaptive layout system for Slate OS.
//!
//! Maps physical screen dimensions and DPI scale to concrete UI parameters so
//! every shell component can render correctly on phones, tablets, and desktops
//! without embedding device-specific logic in application code.
//!
//! The thresholds and target sizes follow Apple's Human Interface Guidelines
//! (minimum 44 px touch target) as a well-established baseline that is equally
//! applicable on Android-class hardware.

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Convert physical pixels to logical pixels using the compositor scale factor.
///
/// We use `.round()` rather than truncation so that a 1-pixel rounding error
/// in the physical dimension cannot push a layout over (or under) a threshold.
/// Both `FormFactor::detect` and `compute_layout` call this function to
/// guarantee they always agree on the logical width.
fn to_logical(physical: u32, scale: f32) -> u32 {
    (physical as f32 / scale).round() as u32
}

// ---------------------------------------------------------------------------
// Form factor
// ---------------------------------------------------------------------------

/// Coarse category inferred from the logical (post-scale) width of the screen.
///
/// We use *logical* pixels (physical / scale) because that is what layout code
/// operates on at render time. The thresholds match industry convention:
///   - < 600 lp  → phone-class  (portrait or narrow landscape)
///   - 600-1400  → tablet-class (the primary Slate OS target)
///   - > 1400    → desktop      (large monitor or desktop mode)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormFactor {
    Phone,
    Tablet,
    Desktop,
}

impl FormFactor {
    /// Detect the form factor from raw physical pixels and a HiDPI scale factor.
    ///
    /// `width` is the *horizontal* physical extent as reported by the Wayland
    /// output (i.e. the number of pixels along the x-axis in the compositor's
    /// current orientation). `height` is used only indirectly — pass the value
    /// from the same output event. Detection is based on `width` alone so that
    /// the caller can pass rotated dimensions and get a layout matching the
    /// current screen orientation.
    pub fn detect(width: u32, height: u32, scale: f32) -> Self {
        // `height` is accepted so the signature matches compositor output events
        // but classification uses width: the horizontal extent determines whether
        // UI panels, dock, and launcher columns fit side-by-side.
        let _ = height;
        Self::from_logical_width(to_logical(width, scale))
    }

    /// Classify from pre-computed logical width. Exposed so callers that
    /// already know their logical width can skip the pixel-to-logical step.
    pub fn from_logical_width(logical_width: u32) -> Self {
        // Exact boundary values (600, 1400) belong to the *larger* category so
        // that a "600 px tablet" gets tablet layout rather than phone layout.
        if logical_width < 600 {
            FormFactor::Phone
        } else if logical_width <= 1400 {
            FormFactor::Tablet
        } else {
            FormFactor::Desktop
        }
    }
}

// ---------------------------------------------------------------------------
// Panel position
// ---------------------------------------------------------------------------

/// Determines where the Claw AI panel is anchored.
///
/// Side panels work well on wide screens but would leave no usable space on a
/// phone, so phones get a fullscreen overlay instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelPosition {
    /// Attached to the right edge, visible alongside app content.
    Right,
    /// Covers the whole screen; dismissed by gesture or close button.
    Fullscreen,
}

// ---------------------------------------------------------------------------
// Layout parameters
// ---------------------------------------------------------------------------

/// Concrete pixel values computed for a specific screen and form factor.
///
/// All values are in logical pixels. Components should multiply by their own
/// scale factor if they need physical pixels for rendering.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutParams {
    /// Which form factor was detected, retained so callers can branch on it.
    pub form_factor: FormFactor,

    // -- Dock -----------------------------------------------------------------
    /// Full height of the dock bar including padding.
    pub dock_height: u32,
    /// Visual size of each app icon in the dock.
    pub dock_icon_size: u32,
    /// Minimum touch-target size for dock icons (always >= 44 per Apple HIG).
    ///
    /// Separating this from `dock_icon_size` lets the visual icon be smaller
    /// than the interactive hit area, which improves aesthetics without
    /// sacrificing reachability.
    pub dock_hit_area: u32,
    /// Symmetric padding around the icon row within the dock.
    pub dock_padding: u32,

    // -- App launcher grid ---------------------------------------------------
    /// Number of icon columns in the fullscreen launcher.
    pub launcher_columns: u32,
    /// Size of each app icon in the launcher grid.
    pub launcher_icon_size: u32,
    /// Gap between grid cells (both horizontal and vertical).
    pub launcher_gap: u32,

    // -- Claw AI panel -------------------------------------------------------
    /// Width of the panel when `position == Right`; ignored for Fullscreen.
    pub panel_width: u32,
    /// Where the panel attaches.
    pub panel_position: PanelPosition,

    // -- Suggestion bar ------------------------------------------------------
    /// Height of the inline keyboard suggestion bar.
    pub suggest_bar_height: u32,

    // -- General layout ------------------------------------------------------
    /// Minimum touch target size (44 px per Apple HIG; never go smaller).
    pub touch_target_min: u32,
    /// Inset from screen edges for scrollable content.
    pub content_padding: u32,
    /// Corner radius for cards, sheets, and overlay surfaces.
    pub border_radius: u32,
}

// ---------------------------------------------------------------------------
// Computation
// ---------------------------------------------------------------------------

/// Compute the full set of layout parameters for the given screen.
///
/// `width` and `height` are physical pixels as reported by the Wayland output;
/// `scale` is the compositor's advertised HiDPI scale factor (e.g. `2.0` for a
/// 2× display). All returned values are in *logical* pixels.
///
/// Degenerate inputs (zero dimensions or extreme scale) are handled gracefully:
/// `panel_width` is clamped to a minimum of 320 so the panel is never invisible.
pub fn compute_layout(width: u32, height: u32, scale: f32) -> LayoutParams {
    let form_factor = FormFactor::detect(width, height, scale);

    // Use logical width for dimension-dependent column/panel adjustments.
    // `width` is the horizontal extent in the compositor's current orientation.
    // We call to_logical() here (same as detect()) so both always agree.
    let logical_width = to_logical(width, scale);

    let mut params = match form_factor {
        FormFactor::Phone => phone_layout(logical_width),
        FormFactor::Tablet => tablet_layout(logical_width),
        FormFactor::Desktop => desktop_layout(logical_width),
    };

    // Clamp panel_width so degenerate inputs (e.g. zero-width screen) never
    // produce an invisible panel.
    params.panel_width = params.panel_width.max(320);

    params
}

/// Phone layout: compact, single-column-friendly, fullscreen panels.
///
/// Every measurement stays above the 44 px touch floor so one-handed use is
/// comfortable on small screens without requiring a stylus. The dock icon is
/// visually 36 px (fits the compact bar) but its hit area is 44 px so the
/// touch target requirement is met without inflating the icon render size.
fn phone_layout(logical_width: u32) -> LayoutParams {
    // Launcher columns scale with the available width while staying at 3 for
    // most phones and bumping to 4 only on wider phablet-class devices.
    let launcher_columns = if logical_width >= 480 { 4 } else { 3 };

    LayoutParams {
        form_factor: FormFactor::Phone,

        dock_height: 48,
        dock_icon_size: 36,
        // Hit area expands to the HIG minimum even though the visual icon is smaller.
        dock_hit_area: 44,
        // Vertical centering: (48 - 36) / 2 = 6, round up to 8 for breathing room.
        dock_padding: 8,

        launcher_columns,
        launcher_icon_size: 52,
        launcher_gap: 16,

        // Phones have no room for a sidebar, so the panel covers the screen.
        // panel_width is set to logical_width here; compute_layout() will clamp
        // it to at least 320 for the degenerate zero-width case.
        panel_width: logical_width,
        panel_position: PanelPosition::Fullscreen,

        suggest_bar_height: 44,

        touch_target_min: 44,
        content_padding: 16,
        border_radius: 16,
    }
}

/// Tablet layout: the primary Slate OS target; side panel fits alongside apps.
///
/// Column count adjusts within the tablet range so an 8" tablet and a 12"
/// tablet feel proportional rather than identically cramped or stretched.
fn tablet_layout(logical_width: u32) -> LayoutParams {
    // Scale columns from 5 (narrow tablet, ~600 lp) to 7 (wide, ~1400 lp).
    // This prevents excessive whitespace on wide tablets while avoiding icon
    // overload on narrower ones.
    let launcher_columns = if logical_width >= 1200 {
        7
    } else if logical_width >= 900 {
        6
    } else {
        5
    };

    LayoutParams {
        form_factor: FormFactor::Tablet,

        dock_height: 64,
        dock_icon_size: 44,
        dock_hit_area: 44,
        dock_padding: 10,

        launcher_columns,
        launcher_icon_size: 64,
        launcher_gap: 20,

        panel_width: 380,
        panel_position: PanelPosition::Right,

        suggest_bar_height: 44,

        touch_target_min: 44,
        content_padding: 24,
        border_radius: 20,
    }
}

/// Desktop layout: pointer-centric, more content density, wider panel.
///
/// Touch targets remain at 44 px minimum because Slate OS desktops may be
/// touch-enabled; layout density increases but not below the HIG floor.
fn desktop_layout(logical_width: u32) -> LayoutParams {
    // Very wide monitors (ultrawide, multi-monitor) get more columns.
    let launcher_columns = if logical_width >= 2560 { 8 } else { 6 };

    LayoutParams {
        form_factor: FormFactor::Desktop,

        dock_height: 56,
        dock_icon_size: 40,
        dock_hit_area: 44,
        dock_padding: 8,

        launcher_columns,
        launcher_icon_size: 56,
        launcher_gap: 16,

        // Wider panel fits naturally on large screens without crowding content.
        panel_width: 420,
        panel_position: PanelPosition::Right,

        suggest_bar_height: 44,

        touch_target_min: 44,
        content_padding: 32,
        border_radius: 12,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- FormFactor::detect --------------------------------------------------

    #[test]
    fn detect_phone_portrait() {
        // Pixel phone portrait: width=1080, height=2400, scale=2.5
        // logical width = round(1080/2.5) = round(432.0) = 432 → phone
        let ff = FormFactor::detect(1080, 2400, 2.5);
        assert_eq!(ff, FormFactor::Phone);
    }

    #[test]
    fn detect_phone_landscape() {
        // Same phone rotated to landscape: width=2400, height=1080, scale=2.5
        // logical width = round(2400/2.5) = round(960.0) = 960 → tablet-class layout,
        // which is correct because landscape phone mode benefits from wider columns.
        let ff = FormFactor::detect(2400, 1080, 2.5);
        assert_eq!(ff, FormFactor::Tablet);
    }

    #[test]
    fn detect_tablet_portrait() {
        // Pixel Tablet portrait: width=1600, height=2560, scale=2.0
        // logical width = round(1600/2.0) = 800 → tablet
        let ff = FormFactor::detect(1600, 2560, 2.0);
        assert_eq!(ff, FormFactor::Tablet);
    }

    #[test]
    fn detect_tablet_landscape() {
        // Pixel Tablet landscape: width=2560, height=1600, scale=2.0
        // logical width = round(2560/2.0) = 1280 → tablet
        let ff = FormFactor::detect(2560, 1600, 2.0);
        assert_eq!(ff, FormFactor::Tablet);
    }

    #[test]
    fn detect_desktop() {
        // 1080p monitor at 1.0 scale: width=1920 → desktop
        let ff = FormFactor::detect(1920, 1080, 1.0);
        assert_eq!(ff, FormFactor::Desktop);
    }

    // -- Exact boundary values -----------------------------------------------

    #[test]
    fn boundary_599_is_phone() {
        // Width exactly 599 logical px → phone
        let ff = FormFactor::from_logical_width(599);
        assert_eq!(ff, FormFactor::Phone);
    }

    #[test]
    fn boundary_600_is_tablet() {
        // Exact 600 belongs to the tablet category (inclusive lower bound)
        let ff = FormFactor::from_logical_width(600);
        assert_eq!(ff, FormFactor::Tablet);
    }

    #[test]
    fn boundary_1400_is_tablet() {
        // Exact 1400 belongs to tablet (inclusive upper bound)
        let ff = FormFactor::from_logical_width(1400);
        assert_eq!(ff, FormFactor::Tablet);
    }

    #[test]
    fn boundary_1401_is_desktop() {
        let ff = FormFactor::from_logical_width(1401);
        assert_eq!(ff, FormFactor::Desktop);
    }

    // -- compute_layout: phone -----------------------------------------------

    #[test]
    fn phone_layout_values() {
        // 1080×2400 at 2.5× → logical round(432.0) = 432 wide → phone
        let p = compute_layout(1080, 2400, 2.5);

        assert_eq!(p.form_factor, FormFactor::Phone);
        assert_eq!(p.dock_height, 48);
        assert_eq!(p.dock_icon_size, 36);
        assert_eq!(p.dock_hit_area, 44);
        assert_eq!(p.suggest_bar_height, 44);
        assert_eq!(p.panel_position, PanelPosition::Fullscreen);
        assert_eq!(p.launcher_columns, 3);
    }

    #[test]
    fn phone_wide_gets_four_columns() {
        // 960×600 physical at 2.0× → logical width = round(960/2) = 480 → phone.
        // 480 >= 480 threshold triggers 4-column grid for wider phone screens.
        let p = compute_layout(960, 600, 2.0);
        assert_eq!(p.form_factor, FormFactor::Phone);
        assert_eq!(p.launcher_columns, 4);
    }

    #[test]
    fn phone_narrow_gets_three_columns() {
        // 800×1280 physical at 3.0× → logical width = round(800/3) = round(266.7) = 267 → phone, < 480
        // → 3-column grid for narrow portrait phones.
        let p = compute_layout(800, 1280, 3.0);
        assert_eq!(p.form_factor, FormFactor::Phone);
        assert_eq!(p.launcher_columns, 3);
    }

    // -- compute_layout: tablet ----------------------------------------------

    #[test]
    fn tablet_layout_values() {
        // Pixel Tablet 2560×1600 at 2.0 → logical 1280 wide → tablet
        let p = compute_layout(2560, 1600, 2.0);

        assert_eq!(p.form_factor, FormFactor::Tablet);
        assert_eq!(p.dock_height, 64);
        assert_eq!(p.dock_icon_size, 44);
        assert_eq!(p.dock_hit_area, 44);
        assert_eq!(p.suggest_bar_height, 44);
        assert_eq!(p.panel_position, PanelPosition::Right);
        assert_eq!(p.panel_width, 380);
    }

    #[test]
    fn tablet_narrow_gets_five_columns() {
        // 1200×900 physical at 2.0× → logical width = 600 → tablet, narrow (< 900)
        // → 5-column grid
        let p = compute_layout(1200, 900, 2.0);
        assert_eq!(p.form_factor, FormFactor::Tablet);
        assert_eq!(p.launcher_columns, 5);
    }

    #[test]
    fn tablet_mid_gets_six_columns() {
        // 1800×1200 physical at 2.0× → logical width = 900 → tablet, mid [900,1200)
        // → 6-column grid
        let p = compute_layout(1800, 1200, 2.0);
        assert_eq!(p.form_factor, FormFactor::Tablet);
        assert_eq!(p.launcher_columns, 6);
    }

    #[test]
    fn tablet_wide_gets_seven_columns() {
        // 2400×1600 physical at 2.0× → logical width = 1200 → tablet, wide [1200,1400]
        // → 7-column grid
        let p = compute_layout(2400, 1600, 2.0);
        assert_eq!(p.form_factor, FormFactor::Tablet);
        assert_eq!(p.launcher_columns, 7);
    }

    // -- compute_layout: desktop ---------------------------------------------

    #[test]
    fn desktop_layout_values() {
        // 3840×2160 at 2.0 → logical 1920 → desktop
        let p = compute_layout(3840, 2160, 2.0);

        assert_eq!(p.form_factor, FormFactor::Desktop);
        assert_eq!(p.dock_height, 56);
        assert_eq!(p.dock_icon_size, 40);
        assert_eq!(p.dock_hit_area, 44);
        assert_eq!(p.suggest_bar_height, 44);
        assert_eq!(p.panel_position, PanelPosition::Right);
        assert_eq!(p.panel_width, 420);
        assert_eq!(p.launcher_columns, 6);
    }

    #[test]
    fn desktop_ultrawide_gets_eight_columns() {
        // 5120×1440 at 1.0 → logical 5120 → desktop, ultrawide
        let p = compute_layout(5120, 1440, 1.0);
        assert_eq!(p.form_factor, FormFactor::Desktop);
        assert_eq!(p.launcher_columns, 8);
    }

    // -- Touch target floor --------------------------------------------------

    #[test]
    fn touch_target_min_never_below_44() {
        // All form factors must honour the Apple HIG minimum.
        for (w, h, scale) in [
            (1080u32, 2400u32, 2.5f32), // phone
            (2560, 1600, 2.0),          // tablet
            (3840, 2160, 2.0),          // desktop
        ] {
            let p = compute_layout(w, h, scale);
            assert!(
                p.touch_target_min >= 44,
                "touch_target_min below 44 for {:?}",
                p.form_factor
            );
            assert!(
                p.dock_hit_area >= 44,
                "dock_hit_area below 44 for {:?}",
                p.form_factor
            );
            assert!(
                p.suggest_bar_height >= 44,
                "suggest_bar_height below 44 for {:?}",
                p.form_factor
            );
        }
    }

    // -- Degenerate input: zero dimensions -----------------------------------

    #[test]
    fn degenerate_zero_dimensions_clamps_panel_width() {
        // compute_layout(0, 0, 1.0): logical width = 0 → phone form factor.
        // panel_width would be 0 (logical_width), but must be clamped to 320
        // so the panel is never invisible.
        let p = compute_layout(0, 0, 1.0);
        assert_eq!(p.form_factor, FormFactor::Phone);
        assert!(
            p.panel_width >= 320,
            "panel_width {} is below minimum 320 for degenerate input",
            p.panel_width
        );
        // All other invariants still hold.
        assert!(p.touch_target_min >= 44);
        assert!(p.dock_hit_area >= 44);
    }

    // -- Landscape vs portrait consistency -----------------------------------

    #[test]
    fn tablet_portrait_and_landscape_both_tablet() {
        // Rotating a Pixel Tablet should keep both orientations within the
        // tablet form factor (landscape is wider, portrait is narrower but
        // still above the 600 lp phone boundary at this scale).
        let landscape = FormFactor::detect(2560, 1600, 2.0); // logical width 1280
        let portrait = FormFactor::detect(1600, 2560, 2.0); // logical width 800
        assert_eq!(landscape, FormFactor::Tablet);
        assert_eq!(portrait, FormFactor::Tablet);
    }

    #[test]
    fn landscape_layout_is_wider_than_portrait() {
        // Landscape and portrait produce different layouts (more columns when
        // the screen is wider) — this is intentional, not a bug.
        let landscape = compute_layout(2560, 1600, 2.0); // logical width 1280
        let portrait = compute_layout(1600, 2560, 2.0); // logical width 800
                                                        // Both are tablet form factor
        assert_eq!(landscape.form_factor, FormFactor::Tablet);
        assert_eq!(portrait.form_factor, FormFactor::Tablet);
        // Landscape gets more launcher columns because it has more horizontal space
        assert!(
            landscape.launcher_columns >= portrait.launcher_columns,
            "landscape should have at least as many columns as portrait"
        );
    }

    // -- Scale factor sensitivity --------------------------------------------

    #[test]
    fn scale_factor_affects_form_factor() {
        // Physical width 1500 px:
        //   at scale 1.0 → logical 1500 > 1400 → desktop
        //   at scale 2.0 → logical  750 in [600,1400] → tablet
        let hi_dpi = FormFactor::detect(1500, 900, 2.0);
        let lo_dpi = FormFactor::detect(1500, 900, 1.0);
        assert_eq!(hi_dpi, FormFactor::Tablet);
        assert_eq!(lo_dpi, FormFactor::Desktop);
    }

    // -- from_logical_width exhaustive sweep ---------------------------------

    #[test]
    fn logical_width_sweep() {
        // Verify the full classification table without gaps or overlaps.
        for w in 0u32..=3000 {
            let ff = FormFactor::from_logical_width(w);
            let expected = if w < 600 {
                FormFactor::Phone
            } else if w <= 1400 {
                FormFactor::Tablet
            } else {
                FormFactor::Desktop
            };
            assert_eq!(ff, expected, "mismatch at logical width {w}");
        }
    }

    // -- Copy trait ----------------------------------------------------------

    #[test]
    fn layout_params_is_copy() {
        // Verify that LayoutParams, FormFactor, and PanelPosition are all Copy.
        // If any field were non-Copy this would fail to compile.
        let p = compute_layout(2560, 1600, 2.0);
        let _copy = p; // move
        let _also = p; // would fail to compile if LayoutParams were not Copy
        let ff = p.form_factor;
        let _ff2 = ff;
        let _ff3 = ff;
        let pos = p.panel_position;
        let _pos2 = pos;
        let _pos3 = pos;
    }
}
