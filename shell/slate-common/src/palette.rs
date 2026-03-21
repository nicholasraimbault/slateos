/// Palette types for Slate OS dynamic theming.
///
/// The palette is extracted from the current wallpaper by slate-palette daemon
/// and broadcast over D-Bus so every component can theme consistently.
use serde::{Deserialize, Serialize};

/// System-wide colour palette with five Material-You-inspired roles.
/// Each colour is stored as `[R, G, B, A]` in 0-255 range.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Palette {
    /// Main accent colour (buttons, links, active indicators).
    pub primary: [u8; 4],
    /// Secondary accent for less prominent elements.
    pub secondary: [u8; 4],
    /// Background surface colour (app backgrounds, sheets).
    pub surface: [u8; 4],
    /// Container colour (cards, elevated surfaces).
    pub container: [u8; 4],
    /// Neutral colour (text, icons, dividers).
    pub neutral: [u8; 4],
}

impl Default for Palette {
    /// Sensible dark-theme default: dark surface, blue-ish primary.
    fn default() -> Self {
        Self {
            primary: [100, 149, 237, 255],   // Cornflower blue
            secondary: [138, 180, 248, 255], // Lighter blue
            surface: [18, 18, 24, 255],      // Near-black with blue tint
            container: [30, 30, 40, 255],    // Slightly lighter surface
            neutral: [228, 228, 232, 255],   // Off-white text
        }
    }
}

impl Palette {
    /// Convert an RGBA `[u8; 4]` to an `iced::Color` with 0.0-1.0 floats.
    pub fn color_to_iced(rgba: [u8; 4]) -> iced::Color {
        iced::Color::from_rgba8(rgba[0], rgba[1], rgba[2], f32::from(rgba[3]) / 255.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_serialization_round_trip() {
        let palette = Palette::default();
        let toml_str = toml::to_string(&palette).expect("serialize palette");
        let deserialized: Palette = toml::from_str(&toml_str).expect("deserialize palette");
        assert_eq!(palette, deserialized);
    }

    #[test]
    fn default_palette_has_valid_rgba() {
        let p = Palette::default();
        // Alpha channels should be fully opaque
        for colour in [p.primary, p.secondary, p.surface, p.container, p.neutral] {
            assert_eq!(colour[3], 255, "alpha should be fully opaque");
        }
    }

    #[test]
    fn custom_palette_round_trip() {
        let palette = Palette {
            primary: [255, 0, 0, 128],
            secondary: [0, 255, 0, 200],
            surface: [0, 0, 0, 255],
            container: [50, 50, 50, 255],
            neutral: [200, 200, 200, 255],
        };
        let toml_str = toml::to_string(&palette).expect("serialize");
        let back: Palette = toml::from_str(&toml_str).expect("deserialize");
        assert_eq!(palette, back);
    }

    #[test]
    fn color_to_iced_converts_correctly() {
        let c = Palette::color_to_iced([255, 0, 128, 255]);
        // iced::Color fields are f32 0.0-1.0
        assert!((c.r - 1.0).abs() < 0.01);
        assert!(c.g.abs() < 0.01);
        assert!((c.b - 128.0 / 255.0).abs() < 0.01);
        assert!((c.a - 1.0).abs() < 0.01);
    }
}
