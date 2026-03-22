/// Icon theme resolver for Slate OS.
///
/// Resolves freedesktop icon names (as found in `.desktop` files' `Icon=`
/// field) to actual file paths on disk. Searches icon theme directories in
/// the standard order: hicolor at the requested size, then common fallback
/// sizes, then scalable, then the legacy `/usr/share/pixmaps/` directory.
///
/// Results are cached in an `IconCache` so repeated lookups for the same
/// icon name are O(1) after the first resolution.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tracing::debug;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Base directory for freedesktop icon themes.
const ICON_THEME_DIR: &str = "/usr/share/icons";

/// The mandatory fallback theme per the freedesktop icon theme spec.
const HICOLOR_THEME: &str = "hicolor";

/// Legacy pixmaps directory for apps that install icons outside of themes.
const PIXMAPS_DIR: &str = "/usr/share/pixmaps";

/// File extensions to probe, in priority order.
/// PNG is preferred over SVG for raster performance; XPM is a legacy fallback.
const EXTENSIONS: &[&str] = &["png", "svg"];

/// Extensions accepted in the pixmaps directory (includes legacy XPM).
const PIXMAPS_EXTENSIONS: &[&str] = &["png", "svg", "xpm"];

/// Fallback sizes to try when the requested size is not available.
/// Ordered from large to small so we get the best quality first.
const FALLBACK_SIZES: &[u32] = &[128, 96, 72, 64, 48, 32];

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors that can occur during icon resolution.
#[derive(Debug, thiserror::Error)]
pub enum IconError {
    #[error("icon name is empty")]
    EmptyName,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Resolve an icon name to a file path.
///
/// Searches icon themes in order: hicolor at the requested `size`, then
/// common fallback sizes, then scalable, then the pixmaps directory.
///
/// If `name` is already an absolute path to an existing file, it is
/// returned directly. Returns `None` when no matching icon file is found
/// on disk.
///
/// # Arguments
/// * `name` - Icon name from a `.desktop` file's `Icon=` field, or an
///   absolute path to an icon file.
/// * `size` - Preferred icon size in pixels (e.g. 48, 64, 128).
pub fn resolve_icon(name: &str, size: u32) -> Option<PathBuf> {
    resolve_icon_in(name, size, ICON_THEME_DIR, PIXMAPS_DIR)
}

/// Testable inner implementation that accepts custom base directories.
fn resolve_icon_in(
    name: &str,
    size: u32,
    icon_theme_dir: &str,
    pixmaps_dir: &str,
) -> Option<PathBuf> {
    if name.is_empty() {
        return None;
    }

    // Absolute path: return directly if it exists on disk.
    if name.starts_with('/') {
        let path = PathBuf::from(name);
        if path.is_file() {
            return Some(path);
        }
        return None;
    }

    // Build the list of sizes to try: requested size first, then fallbacks.
    let sizes = candidate_sizes(size);

    // Search hicolor theme at each candidate size.
    let theme_dir = Path::new(icon_theme_dir).join(HICOLOR_THEME);
    for &sz in &sizes {
        let dir = theme_dir.join(format!("{sz}x{sz}")).join("apps");
        for ext in EXTENSIONS {
            let candidate = dir.join(format!("{name}.{ext}"));
            if candidate.is_file() {
                debug!(icon = name, path = %candidate.display(), "resolved icon");
                return Some(candidate);
            }
        }
    }

    // Try scalable directory.
    let scalable_dir = theme_dir.join("scalable").join("apps");
    for ext in EXTENSIONS {
        let candidate = scalable_dir.join(format!("{name}.{ext}"));
        if candidate.is_file() {
            debug!(icon = name, path = %candidate.display(), "resolved scalable icon");
            return Some(candidate);
        }
    }

    // Pixmaps fallback.
    let pixmaps = Path::new(pixmaps_dir);
    for ext in PIXMAPS_EXTENSIONS {
        let candidate = pixmaps.join(format!("{name}.{ext}"));
        if candidate.is_file() {
            debug!(icon = name, path = %candidate.display(), "resolved pixmap icon");
            return Some(candidate);
        }
    }

    debug!(icon = name, "icon not found");
    None
}

/// Build the ordered list of sizes to search.
///
/// Starts with the requested size, then appends any fallback sizes that
/// are not already present.
fn candidate_sizes(requested: u32) -> Vec<u32> {
    let mut sizes = vec![requested];
    for &fallback in FALLBACK_SIZES {
        if fallback != requested {
            sizes.push(fallback);
        }
    }
    sizes
}

// ---------------------------------------------------------------------------
// IconCache
// ---------------------------------------------------------------------------

/// Thread-safe cache for resolved icon paths.
///
/// Wraps a `HashMap<String, Option<PathBuf>>` behind a `Mutex` so multiple
/// components can share a single cache. Icon themes do not change at
/// runtime, so entries never need invalidation.
#[derive(Debug)]
pub struct IconCache {
    size: u32,
    cache: Mutex<HashMap<String, Option<PathBuf>>>,
}

impl IconCache {
    /// Create a new empty cache that resolves icons at the given `size`.
    pub fn new(size: u32) -> Self {
        Self {
            size,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Resolve an icon name, returning a cached result if available.
    ///
    /// On the first call for a given `name`, performs a full filesystem
    /// search and stores the result. Subsequent calls return the cached
    /// value without touching the filesystem.
    pub fn resolve(&self, name: &str) -> Option<PathBuf> {
        self.resolve_in(name, ICON_THEME_DIR, PIXMAPS_DIR)
    }

    /// Testable inner implementation that accepts custom base directories.
    fn resolve_in(&self, name: &str, icon_theme_dir: &str, pixmaps_dir: &str) -> Option<PathBuf> {
        if name.is_empty() {
            return None;
        }

        // Fast path: check the cache first.
        {
            let guard = self.cache.lock().ok()?;
            if let Some(cached) = guard.get(name) {
                return cached.clone();
            }
        }

        // Slow path: resolve and cache.
        let result = resolve_icon_in(name, self.size, icon_theme_dir, pixmaps_dir);

        if let Ok(mut guard) = self.cache.lock() {
            guard.insert(name.to_string(), result.clone());
        }

        result
    }

    /// Number of entries currently in the cache.
    pub fn len(&self) -> usize {
        self.cache.lock().map(|g| g.len()).unwrap_or(0)
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Remove all cached entries, forcing fresh lookups on next access.
    pub fn clear(&self) {
        if let Ok(mut guard) = self.cache.lock() {
            guard.clear();
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Helper: create a temporary icon theme tree and return the base
    /// icon theme dir and pixmaps dir paths.
    fn setup_icon_dirs(base: &Path) -> (PathBuf, PathBuf) {
        let icons_dir = base.join("icons");
        let pixmaps_dir = base.join("pixmaps");

        // hicolor/48x48/apps/
        let hicolor_48 = icons_dir.join("hicolor/48x48/apps");
        fs::create_dir_all(&hicolor_48).expect("create hicolor 48 dir");

        // hicolor/64x64/apps/
        let hicolor_64 = icons_dir.join("hicolor/64x64/apps");
        fs::create_dir_all(&hicolor_64).expect("create hicolor 64 dir");

        // hicolor/scalable/apps/
        let scalable = icons_dir.join("hicolor/scalable/apps");
        fs::create_dir_all(&scalable).expect("create scalable dir");

        // pixmaps/
        fs::create_dir_all(&pixmaps_dir).expect("create pixmaps dir");

        (icons_dir, pixmaps_dir)
    }

    #[test]
    fn resolve_icon_returns_none_for_empty_name() {
        assert!(resolve_icon("", 48).is_none());
    }

    #[test]
    fn resolve_icon_returns_absolute_path_if_file_exists() {
        let dir = std::env::temp_dir().join("slate-icon-test-abs");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");

        let icon_file = dir.join("custom-icon.png");
        fs::write(&icon_file, b"fake png").expect("write icon");

        let path_str = icon_file.to_str().expect("path to str");
        let result = resolve_icon(path_str, 48);
        assert_eq!(result, Some(icon_file.clone()));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_icon_returns_none_for_missing_absolute_path() {
        let result = resolve_icon("/nonexistent/path/to/icon.png", 48);
        assert!(result.is_none());
    }

    #[test]
    fn resolve_icon_in_finds_exact_size() {
        let base = std::env::temp_dir().join("slate-icon-test-exact");
        let _ = fs::remove_dir_all(&base);
        let (icons_dir, pixmaps_dir) = setup_icon_dirs(&base);

        // Place a 48x48 icon.
        let icon_path = icons_dir.join("hicolor/48x48/apps/firefox.png");
        fs::write(&icon_path, b"fake png").expect("write icon");

        let result = resolve_icon_in(
            "firefox",
            48,
            icons_dir.to_str().expect("icons path"),
            pixmaps_dir.to_str().expect("pixmaps path"),
        );
        assert_eq!(result, Some(icon_path));

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn resolve_icon_in_falls_back_to_other_size() {
        let base = std::env::temp_dir().join("slate-icon-test-fallback-size");
        let _ = fs::remove_dir_all(&base);
        let (icons_dir, pixmaps_dir) = setup_icon_dirs(&base);

        // Only a 64x64 icon exists; request 48.
        let icon_path = icons_dir.join("hicolor/64x64/apps/terminal.png");
        fs::write(&icon_path, b"fake png").expect("write icon");

        let result = resolve_icon_in(
            "terminal",
            48,
            icons_dir.to_str().expect("icons path"),
            pixmaps_dir.to_str().expect("pixmaps path"),
        );
        assert_eq!(result, Some(icon_path));

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn resolve_icon_in_falls_back_to_scalable() {
        let base = std::env::temp_dir().join("slate-icon-test-scalable");
        let _ = fs::remove_dir_all(&base);
        let (icons_dir, pixmaps_dir) = setup_icon_dirs(&base);

        // Only a scalable SVG exists.
        let icon_path = icons_dir.join("hicolor/scalable/apps/editor.svg");
        fs::write(&icon_path, b"<svg/>").expect("write icon");

        let result = resolve_icon_in(
            "editor",
            48,
            icons_dir.to_str().expect("icons path"),
            pixmaps_dir.to_str().expect("pixmaps path"),
        );
        assert_eq!(result, Some(icon_path));

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn resolve_icon_in_falls_back_to_pixmaps() {
        let base = std::env::temp_dir().join("slate-icon-test-pixmaps");
        let _ = fs::remove_dir_all(&base);
        let (icons_dir, pixmaps_dir) = setup_icon_dirs(&base);

        // Only a pixmap icon exists.
        let icon_path = pixmaps_dir.join("legacy-app.png");
        fs::write(&icon_path, b"fake png").expect("write icon");

        let result = resolve_icon_in(
            "legacy-app",
            48,
            icons_dir.to_str().expect("icons path"),
            pixmaps_dir.to_str().expect("pixmaps path"),
        );
        assert_eq!(result, Some(icon_path));

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn resolve_icon_in_finds_xpm_in_pixmaps() {
        let base = std::env::temp_dir().join("slate-icon-test-xpm");
        let _ = fs::remove_dir_all(&base);
        let (icons_dir, pixmaps_dir) = setup_icon_dirs(&base);

        let icon_path = pixmaps_dir.join("old-app.xpm");
        fs::write(&icon_path, b"fake xpm").expect("write icon");

        let result = resolve_icon_in(
            "old-app",
            48,
            icons_dir.to_str().expect("icons path"),
            pixmaps_dir.to_str().expect("pixmaps path"),
        );
        assert_eq!(result, Some(icon_path));

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn resolve_icon_in_returns_none_when_not_found() {
        let base = std::env::temp_dir().join("slate-icon-test-missing");
        let _ = fs::remove_dir_all(&base);
        let (icons_dir, pixmaps_dir) = setup_icon_dirs(&base);

        let result = resolve_icon_in(
            "nonexistent-app",
            48,
            icons_dir.to_str().expect("icons path"),
            pixmaps_dir.to_str().expect("pixmaps path"),
        );
        assert!(result.is_none());

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn resolve_icon_in_prefers_png_over_svg() {
        let base = std::env::temp_dir().join("slate-icon-test-prefer-png");
        let _ = fs::remove_dir_all(&base);
        let (icons_dir, pixmaps_dir) = setup_icon_dirs(&base);

        // Place both PNG and SVG at the same size.
        let png_path = icons_dir.join("hicolor/48x48/apps/both.png");
        let svg_path = icons_dir.join("hicolor/48x48/apps/both.svg");
        fs::write(&png_path, b"fake png").expect("write png");
        fs::write(&svg_path, b"<svg/>").expect("write svg");

        let result = resolve_icon_in(
            "both",
            48,
            icons_dir.to_str().expect("icons path"),
            pixmaps_dir.to_str().expect("pixmaps path"),
        );
        // PNG should be preferred.
        assert_eq!(result, Some(png_path));

        let _ = fs::remove_dir_all(&base);
    }

    // -- candidate_sizes -----------------------------------------------------

    #[test]
    fn candidate_sizes_starts_with_requested() {
        let sizes = candidate_sizes(48);
        assert_eq!(sizes[0], 48);
    }

    #[test]
    fn candidate_sizes_does_not_duplicate_requested() {
        let sizes = candidate_sizes(64);
        let count = sizes.iter().filter(|&&s| s == 64).count();
        assert_eq!(count, 1, "requested size should appear exactly once");
    }

    #[test]
    fn candidate_sizes_includes_fallbacks() {
        let sizes = candidate_sizes(48);
        // All fallback sizes except 48 (already the requested) should appear.
        for &fb in FALLBACK_SIZES {
            assert!(
                sizes.contains(&fb),
                "fallback size {fb} should be in candidates"
            );
        }
    }

    // -- IconCache -----------------------------------------------------------

    #[test]
    fn icon_cache_new_is_empty() {
        let cache = IconCache::new(48);
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn icon_cache_resolve_caches_result() {
        let base = std::env::temp_dir().join("slate-icon-test-cache");
        let _ = fs::remove_dir_all(&base);
        let (icons_dir, pixmaps_dir) = setup_icon_dirs(&base);

        let icon_path = icons_dir.join("hicolor/48x48/apps/cached.png");
        fs::write(&icon_path, b"fake png").expect("write icon");

        let cache = IconCache::new(48);
        let result1 = cache.resolve_in(
            "cached",
            icons_dir.to_str().expect("icons path"),
            pixmaps_dir.to_str().expect("pixmaps path"),
        );
        assert_eq!(result1, Some(icon_path.clone()));
        assert_eq!(cache.len(), 1);

        // Second call should return the same result from cache.
        let result2 = cache.resolve_in(
            "cached",
            icons_dir.to_str().expect("icons path"),
            pixmaps_dir.to_str().expect("pixmaps path"),
        );
        assert_eq!(result1, result2);
        // Cache size should not have grown.
        assert_eq!(cache.len(), 1);

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn icon_cache_caches_none_results() {
        let base = std::env::temp_dir().join("slate-icon-test-cache-none");
        let _ = fs::remove_dir_all(&base);
        let (icons_dir, pixmaps_dir) = setup_icon_dirs(&base);

        let cache = IconCache::new(48);
        let result = cache.resolve_in(
            "missing",
            icons_dir.to_str().expect("icons path"),
            pixmaps_dir.to_str().expect("pixmaps path"),
        );
        assert!(result.is_none());
        // None results are also cached.
        assert_eq!(cache.len(), 1);

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn icon_cache_resolve_returns_none_for_empty_name() {
        let cache = IconCache::new(48);
        assert!(cache.resolve("").is_none());
        // Empty names are not cached (early return).
        assert!(cache.is_empty());
    }

    #[test]
    fn icon_cache_clear_removes_all_entries() {
        let base = std::env::temp_dir().join("slate-icon-test-cache-clear");
        let _ = fs::remove_dir_all(&base);
        let (icons_dir, pixmaps_dir) = setup_icon_dirs(&base);

        let cache = IconCache::new(48);
        let _ = cache.resolve_in(
            "something",
            icons_dir.to_str().expect("icons path"),
            pixmaps_dir.to_str().expect("pixmaps path"),
        );
        assert!(!cache.is_empty());

        cache.clear();
        assert!(cache.is_empty());

        let _ = fs::remove_dir_all(&base);
    }

    // -- IconError -----------------------------------------------------------

    #[test]
    fn icon_error_display_empty_name() {
        let err = IconError::EmptyName;
        let msg = format!("{err}");
        assert!(
            msg.contains("empty"),
            "error message should mention 'empty'"
        );
    }
}
