/// Iced theme generation from a Slate OS palette.
///
/// Every iced GUI app in the workspace calls `create_theme` with the current
/// palette so the entire system looks consistent.
use crate::palette::Palette;

/// Build a custom `iced::Theme` from the given Slate palette.
///
/// Mapping:
/// - `palette.primary`   → iced `primary` (accent colour)
/// - `palette.surface`   → iced `background`
/// - `palette.neutral`   → iced `text`
/// - A muted green derived from primary → iced `success`
/// - A muted red → iced `danger`
pub fn create_theme(palette: &Palette) -> iced::Theme {
    let iced_palette = iced::theme::Palette {
        background: Palette::color_to_iced(palette.surface),
        text: Palette::color_to_iced(palette.neutral),
        primary: Palette::color_to_iced(palette.primary),
        success: iced::Color::from_rgba8(76, 175, 80, 1.0), // Material green 500
        danger: iced::Color::from_rgba8(244, 67, 54, 1.0),  // Material red 500
    };

    iced::Theme::custom("Slate".to_string(), iced_palette)
}

/// Convenience: build a theme from the default palette.
pub fn default_theme() -> iced::Theme {
    create_theme(&Palette::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_theme_does_not_panic() {
        let _theme = create_theme(&Palette::default());
    }

    #[test]
    fn create_theme_with_default_palette_has_expected_primary() {
        let p = Palette::default();
        let theme = create_theme(&p);
        let iced_palette = theme.palette();

        // The primary colour should match our palette's primary
        let expected = Palette::color_to_iced(p.primary);
        assert!(
            (iced_palette.primary.r - expected.r).abs() < 0.01,
            "primary red mismatch"
        );
        assert!(
            (iced_palette.primary.g - expected.g).abs() < 0.01,
            "primary green mismatch"
        );
        assert!(
            (iced_palette.primary.b - expected.b).abs() < 0.01,
            "primary blue mismatch"
        );
    }

    #[test]
    fn default_theme_helper_works() {
        let _theme = default_theme();
    }

    #[test]
    fn create_theme_with_custom_palette() {
        let palette = Palette {
            primary: [255, 0, 0, 255],
            secondary: [0, 255, 0, 255],
            surface: [0, 0, 0, 255],
            container: [30, 30, 30, 255],
            neutral: [255, 255, 255, 255],
        };
        let theme = create_theme(&palette);
        let iced_palette = theme.palette();

        // Background should be black
        assert!(iced_palette.background.r.abs() < 0.01);
        assert!(iced_palette.background.g.abs() < 0.01);
        assert!(iced_palette.background.b.abs() < 0.01);

        // Text should be white
        assert!((iced_palette.text.r - 1.0).abs() < 0.01);
        assert!((iced_palette.text.g - 1.0).abs() < 0.01);
        assert!((iced_palette.text.b - 1.0).abs() < 0.01);
    }
}
