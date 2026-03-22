// Layout detection for slate-shade.
//
// The shade renders differently on phone-class (<600 logical px) and
// tablet/desktop (>=600 logical px) screens. On phone: single column,
// notifications and quick settings stack vertically. On tablet/desktop:
// split layout with quick settings on the right and notifications on the left.

/// Coarse layout mode for the shade.
///
/// Phone uses a single-column stacked layout to maximise screen real estate
/// on narrow displays. TabletDesktop uses a side-by-side split so both
/// panels are reachable at once without scrolling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutMode {
    /// Single-column layout for screens narrower than 600 logical pixels.
    Phone,
    /// Split layout for tablets and desktops (>=600 logical pixels wide).
    TabletDesktop,
}

impl LayoutMode {
    /// Whether this is the phone form factor.
    pub fn is_phone(self) -> bool {
        matches!(self, LayoutMode::Phone)
    }

    /// Whether this is the tablet/desktop form factor.
    pub fn is_tablet_desktop(self) -> bool {
        matches!(self, LayoutMode::TabletDesktop)
    }
}

/// Detect the layout mode from a logical pixel width.
///
/// Uses the same 600 lp threshold as `slate_common::layout::FormFactor`.
/// Screens narrower than 600 logical pixels are Phone; everything else is
/// TabletDesktop. The threshold is the lower bound of the tablet range so
/// a 600-px-wide screen is considered TabletDesktop.
pub fn detect_layout(logical_width: u32) -> LayoutMode {
    if logical_width < 600 {
        LayoutMode::Phone
    } else {
        LayoutMode::TabletDesktop
    }
}

/// Fallback layout mode when the screen width is unavailable.
///
/// Defaults to TabletDesktop since the Pixel Tablet is the primary target.
pub fn default_layout() -> LayoutMode {
    LayoutMode::TabletDesktop
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_layout_phone_below_600() {
        assert_eq!(detect_layout(0), LayoutMode::Phone);
        assert_eq!(detect_layout(599), LayoutMode::Phone);
        assert_eq!(detect_layout(480), LayoutMode::Phone);
    }

    #[test]
    fn detect_layout_tablet_desktop_at_600_and_above() {
        assert_eq!(detect_layout(600), LayoutMode::TabletDesktop);
        assert_eq!(detect_layout(1280), LayoutMode::TabletDesktop);
        assert_eq!(detect_layout(1920), LayoutMode::TabletDesktop);
        assert_eq!(detect_layout(3840), LayoutMode::TabletDesktop);
    }

    #[test]
    fn layout_mode_is_phone_helper() {
        assert!(LayoutMode::Phone.is_phone());
        assert!(!LayoutMode::TabletDesktop.is_phone());
    }

    #[test]
    fn layout_mode_is_tablet_desktop_helper() {
        assert!(LayoutMode::TabletDesktop.is_tablet_desktop());
        assert!(!LayoutMode::Phone.is_tablet_desktop());
    }

    #[test]
    fn default_layout_is_tablet_desktop() {
        assert_eq!(default_layout(), LayoutMode::TabletDesktop);
    }

    #[test]
    fn layout_mode_is_copy() {
        let mode = LayoutMode::Phone;
        let copy = mode;
        assert_eq!(mode, copy);
    }

    #[test]
    fn layout_mode_is_debug() {
        let _debug = format!("{:?}", LayoutMode::Phone);
        let _debug2 = format!("{:?}", LayoutMode::TabletDesktop);
    }

    #[test]
    fn boundary_value_599_is_phone() {
        assert_eq!(detect_layout(599), LayoutMode::Phone);
    }

    #[test]
    fn boundary_value_600_is_tablet_desktop() {
        assert_eq!(detect_layout(600), LayoutMode::TabletDesktop);
    }
}
