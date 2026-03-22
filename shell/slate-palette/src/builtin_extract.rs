/// Built-in fallback color extractor for when matugen is not available.
///
/// Reads an image, samples pixels at regular intervals, buckets them by hue,
/// and generates a Material-You-inspired dark palette from the dominant hue.
/// This is intentionally simple — it exists so first-boot and matugen-less
/// installs get something better than the static default grey palette.
use std::path::Path;

use anyhow::{Context, Result};
use slate_common::Palette;

/// Number of hue buckets (each covers 360/NUM_BUCKETS degrees).
const NUM_BUCKETS: usize = 36;

/// Sample every Nth pixel for performance on large images.
const SAMPLE_STEP: u32 = 8;

/// Minimum saturation (0.0-1.0) for a pixel to be considered "chromatic".
/// Below this threshold, a pixel is grey-ish and we skip it for hue voting.
const MIN_SATURATION: f32 = 0.15;

/// Convert an RGB pixel to HSL. Returns `(hue: 0-360, saturation: 0-1, lightness: 0-1)`.
fn rgb_to_hsl(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let rf = r as f32 / 255.0;
    let gf = g as f32 / 255.0;
    let bf = b as f32 / 255.0;

    let max = rf.max(gf).max(bf);
    let min = rf.min(gf).min(bf);
    let delta = max - min;
    let lightness = (max + min) / 2.0;

    if delta < f32::EPSILON {
        return (0.0, 0.0, lightness);
    }

    let saturation = if lightness < 0.5 {
        delta / (max + min)
    } else {
        delta / (2.0 - max - min)
    };

    let mut hue = if (max - rf).abs() < f32::EPSILON {
        (gf - bf) / delta + if gf < bf { 6.0 } else { 0.0 }
    } else if (max - gf).abs() < f32::EPSILON {
        (bf - rf) / delta + 2.0
    } else {
        (rf - gf) / delta + 4.0
    };
    hue *= 60.0;
    if hue < 0.0 {
        hue += 360.0;
    }

    (hue, saturation, lightness)
}

/// Convert HSL back to an RGB `[R, G, B, 255]` colour.
fn hsl_to_rgba(hue: f32, saturation: f32, lightness: f32) -> [u8; 4] {
    if saturation < f32::EPSILON {
        let v = (lightness * 255.0).round() as u8;
        return [v, v, v, 255];
    }

    let q = if lightness < 0.5 {
        lightness * (1.0 + saturation)
    } else {
        lightness + saturation - lightness * saturation
    };
    let p = 2.0 * lightness - q;

    let hue_norm = hue / 360.0;
    let r = hue_channel(p, q, hue_norm + 1.0 / 3.0);
    let g = hue_channel(p, q, hue_norm);
    let b = hue_channel(p, q, hue_norm - 1.0 / 3.0);

    [
        (r * 255.0).round() as u8,
        (g * 255.0).round() as u8,
        (b * 255.0).round() as u8,
        255,
    ]
}

/// Helper for HSL-to-RGB channel conversion.
fn hue_channel(p: f32, q: f32, mut t: f32) -> f32 {
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }
    if t < 1.0 / 6.0 {
        return p + (q - p) * 6.0 * t;
    }
    if t < 0.5 {
        return q;
    }
    if t < 2.0 / 3.0 {
        return p + (q - p) * (2.0 / 3.0 - t) * 6.0;
    }
    p
}

/// Find the dominant hue from an image by sampling pixels and bucketing by hue.
/// Returns `None` if the image is entirely achromatic.
fn dominant_hue(img: &image::RgbaImage) -> Option<f32> {
    let mut buckets = [0u32; NUM_BUCKETS];
    let bucket_width = 360.0 / NUM_BUCKETS as f32;

    for y in (0..img.height()).step_by(SAMPLE_STEP as usize) {
        for x in (0..img.width()).step_by(SAMPLE_STEP as usize) {
            let px = img.get_pixel(x, y);
            let (hue, sat, _lum) = rgb_to_hsl(px[0], px[1], px[2]);

            if sat < MIN_SATURATION {
                continue;
            }

            let bucket_idx = ((hue / bucket_width) as usize).min(NUM_BUCKETS - 1);
            buckets[bucket_idx] += 1;
        }
    }

    let (max_idx, &max_count) = buckets.iter().enumerate().max_by_key(|(_, c)| *c)?;
    if max_count == 0 {
        return None;
    }

    // Return the centre of the winning bucket.
    Some(max_idx as f32 * bucket_width + bucket_width / 2.0)
}

/// Generate a Material-You-inspired dark palette from a single dominant hue.
///
/// Maps the hue into five palette roles at different saturations and lightness
/// values typical of a Material You dark scheme.
pub fn palette_from_hue(hue: f32) -> Palette {
    Palette {
        // Primary: vivid accent at moderate lightness
        primary: hsl_to_rgba(hue, 0.70, 0.72),
        // Secondary: same hue family, lower saturation
        secondary: hsl_to_rgba(hue, 0.30, 0.70),
        // Surface: very dark, slight hue tint
        surface: hsl_to_rgba(hue, 0.15, 0.07),
        // Container: slightly lighter surface
        container: hsl_to_rgba(hue, 0.15, 0.12),
        // Neutral: near-white with a hint of the hue
        neutral: hsl_to_rgba(hue, 0.05, 0.90),
    }
}

/// Extract a palette from an image file using built-in pixel sampling.
///
/// If the image is entirely achromatic (greyscale), falls back to
/// `Palette::default()`.
pub fn extract_from_image(image_path: &Path) -> Result<Palette> {
    let img = image::open(image_path)
        .with_context(|| format!("open image {}", image_path.display()))?
        .to_rgba8();

    match dominant_hue(&img) {
        Some(hue) => {
            tracing::info!("built-in extractor: dominant hue = {hue:.0}°");
            Ok(palette_from_hue(hue))
        }
        None => {
            tracing::info!("built-in extractor: image is achromatic, using default palette");
            Ok(Palette::default())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb_to_hsl_pure_red() {
        let (h, s, l) = rgb_to_hsl(255, 0, 0);
        assert!((h - 0.0).abs() < 1.0, "red hue should be ~0, got {h}");
        assert!((s - 1.0).abs() < 0.01, "full saturation expected");
        assert!((l - 0.5).abs() < 0.01, "lightness should be 0.5");
    }

    #[test]
    fn rgb_to_hsl_pure_green() {
        let (h, s, _l) = rgb_to_hsl(0, 255, 0);
        assert!((h - 120.0).abs() < 1.0, "green hue should be ~120, got {h}");
        assert!((s - 1.0).abs() < 0.01);
    }

    #[test]
    fn rgb_to_hsl_pure_blue() {
        let (h, s, _l) = rgb_to_hsl(0, 0, 255);
        assert!((h - 240.0).abs() < 1.0, "blue hue should be ~240, got {h}");
        assert!((s - 1.0).abs() < 0.01);
    }

    #[test]
    fn rgb_to_hsl_grey() {
        let (_, s, _) = rgb_to_hsl(128, 128, 128);
        assert!(s < 0.01, "grey should have ~0 saturation, got {s}");
    }

    #[test]
    fn hsl_to_rgba_round_trip_red() {
        let rgba = hsl_to_rgba(0.0, 1.0, 0.5);
        assert_eq!(rgba, [255, 0, 0, 255]);
    }

    #[test]
    fn hsl_to_rgba_round_trip_grey() {
        let rgba = hsl_to_rgba(0.0, 0.0, 0.5);
        assert_eq!(rgba[0], rgba[1]);
        assert_eq!(rgba[1], rgba[2]);
        assert_eq!(rgba[3], 255);
    }

    #[test]
    fn hsl_to_rgba_round_trip_white() {
        let rgba = hsl_to_rgba(0.0, 0.0, 1.0);
        assert_eq!(rgba, [255, 255, 255, 255]);
    }

    #[test]
    fn palette_from_hue_produces_valid_palette() {
        let palette = palette_from_hue(210.0); // blue-ish
                                               // All alpha channels should be 255.
        for colour in [
            palette.primary,
            palette.secondary,
            palette.surface,
            palette.container,
            palette.neutral,
        ] {
            assert_eq!(colour[3], 255, "alpha should be fully opaque");
        }
        // Surface should be dark.
        let surface_brightness =
            palette.surface[0] as u16 + palette.surface[1] as u16 + palette.surface[2] as u16;
        assert!(
            surface_brightness < 150,
            "surface should be dark, got brightness sum {surface_brightness}"
        );
        // Neutral should be light.
        let neutral_brightness =
            palette.neutral[0] as u16 + palette.neutral[1] as u16 + palette.neutral[2] as u16;
        assert!(
            neutral_brightness > 600,
            "neutral should be light, got brightness sum {neutral_brightness}"
        );
    }

    #[test]
    fn dominant_hue_solid_colour() {
        // Create a 4x4 solid blue image.
        let mut img = image::RgbaImage::new(4, 4);
        for px in img.pixels_mut() {
            *px = image::Rgba([0, 100, 255, 255]);
        }
        let hue = dominant_hue(&img);
        assert!(hue.is_some());
        let h = hue.unwrap();
        // Should be in the blue range (200-260 degrees).
        assert!(
            (200.0..=260.0).contains(&h),
            "expected blue range hue, got {h}"
        );
    }

    #[test]
    fn dominant_hue_grey_image_returns_none() {
        // All grey pixels have saturation below threshold.
        let mut img = image::RgbaImage::new(4, 4);
        for px in img.pixels_mut() {
            *px = image::Rgba([128, 128, 128, 255]);
        }
        assert!(
            dominant_hue(&img).is_none(),
            "grey image should return None"
        );
    }

    #[test]
    fn extract_from_image_nonexistent_path() {
        let result = extract_from_image(Path::new("/nonexistent/image.png"));
        assert!(result.is_err());
    }

    #[test]
    fn extract_from_image_with_temp_file() {
        // Create a small red PNG in a temp file.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.png");
        let mut img = image::RgbaImage::new(16, 16);
        for px in img.pixels_mut() {
            *px = image::Rgba([220, 50, 30, 255]);
        }
        img.save(&path).unwrap();

        let palette = extract_from_image(&path).unwrap();
        // Primary should be reddish (high R, lower G and B).
        assert!(
            palette.primary[0] > palette.primary[2],
            "primary should be reddish"
        );
    }
}
