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
// Niri IPC layout detection
// ---------------------------------------------------------------------------

/// Detect the layout by querying niri for the current output width.
///
/// Falls back to `LayoutMode::TabletDesktop` when niri is not running or the
/// platform does not support the IPC (e.g. macOS dev machines).
pub async fn detect_layout_from_niri() -> LayoutMode {
    match get_screen_width_from_niri().await {
        Some(width) => detect_layout(width),
        // Safe default: the Pixel Tablet is the primary target.
        None => LayoutMode::TabletDesktop,
    }
}

/// Run `niri msg outputs` and parse the first output width from its output.
async fn get_screen_width_from_niri() -> Option<u32> {
    let output = tokio::process::Command::new("niri")
        .args(["msg", "outputs"])
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    parse_niri_output(&String::from_utf8_lossy(&output.stdout))
}

/// Extract the first output width from `niri msg outputs` text.
///
/// Niri prints lines such as `  Mode: 2560x1600@60.000` or `  Current mode: 1920x1080`.
/// We scan for the first `WxH` pattern that follows "mode" (case-insensitive) and
/// return the width component.
pub fn parse_niri_output(text: &str) -> Option<u32> {
    for line in text.lines() {
        let lower = line.to_lowercase();
        if lower.contains("mode:") || lower.contains("current mode:") {
            // Extract `WxH` substring, optionally followed by `@refresh`.
            if let Some(dims) = extract_dimensions(line) {
                return Some(dims.0);
            }
        }
    }
    None
}

/// Extract `(width, height)` from a string segment like `"1920x1080"` or `"2560x1600@60"`.
///
/// Iterates over whitespace-separated tokens and returns the first one that
/// parses as `<width>x<height>`.  Uses a nested closure so that `?` only
/// short-circuits the per-token attempt, not the outer loop.
fn extract_dimensions(s: &str) -> Option<(u32, u32)> {
    s.split_whitespace().find_map(|token| {
        // Strip optional `@refresh` suffix then split on `x`.
        let dims_part = token.split('@').next()?;
        let (w_str, h_str) = dims_part.split_once('x')?;
        let w = w_str.parse::<u32>().ok()?;
        let h = h_str.parse::<u32>().ok()?;
        if w > 0 && h > 0 {
            Some((w, h))
        } else {
            None
        }
    })
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

    #[test]
    fn parse_niri_output_typical_mode_line() {
        let text = "Output \"eDP-1\":\n  Mode: 2560x1600@60.000\n  Scale: 2\n";
        assert_eq!(parse_niri_output(text), Some(2560));
    }

    #[test]
    fn parse_niri_output_current_mode_variant() {
        let text = "  Current mode: 1920x1080\n";
        assert_eq!(parse_niri_output(text), Some(1920));
    }

    #[test]
    fn parse_niri_output_phone_width() {
        let text = "  Mode: 1080x2400@120.000\n";
        assert_eq!(parse_niri_output(text), Some(1080));
    }

    #[test]
    fn parse_niri_output_no_mode_line_returns_none() {
        let text = "Output \"eDP-1\":\n  Scale: 2\n  Transform: normal\n";
        assert_eq!(parse_niri_output(text), None);
    }

    #[test]
    fn parse_niri_output_empty_string_returns_none() {
        assert_eq!(parse_niri_output(""), None);
    }

    #[tokio::test]
    async fn detect_layout_from_niri_falls_back_when_niri_absent() {
        // niri is not running in the test environment; must return the safe default.
        let layout = detect_layout_from_niri().await;
        assert_eq!(layout, LayoutMode::TabletDesktop);
    }
}
