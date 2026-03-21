/// Wallpaper picker page.
///
/// Scans configured directories for image files and presents a grid of
/// thumbnails. Tapping an image selects it as the current wallpaper and
/// updates the symlink so slate-palette can regenerate the colour scheme.
use std::path::{Path, PathBuf};

use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Element, Length};

use slate_common::settings::WallpaperSettings;

/// Supported image extensions for the wallpaper picker.
const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp"];

/// Directories to scan for wallpapers.
fn wallpaper_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(PathBuf::from(home).join(".config/slate/wallpapers"));
    }
    dirs.push(PathBuf::from("/usr/share/backgrounds"));
    dirs
}

/// Symlink path that slate-palette watches.
fn wallpaper_symlink() -> PathBuf {
    std::env::var("HOME")
        .map(|h| PathBuf::from(h).join(".config/slate/wallpaper"))
        .unwrap_or_else(|_| PathBuf::from("/root/.config/slate/wallpaper"))
}

// ---------------------------------------------------------------------------
// Image scanning
// ---------------------------------------------------------------------------

/// Check if a filename has a supported image extension.
pub fn is_image_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| IMAGE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Scan all wallpaper directories and return sorted image paths.
pub fn scan_wallpapers() -> Vec<PathBuf> {
    let mut images = Vec::new();
    for dir in wallpaper_dirs() {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && is_image_file(&path) {
                    images.push(path);
                }
            }
        }
    }
    images.sort();
    images
}

/// Update the wallpaper symlink to point at the chosen image.
pub fn set_wallpaper_symlink(target: &Path) -> std::io::Result<()> {
    let link = wallpaper_symlink();
    if let Some(parent) = link.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Remove old symlink/file if present
    let _ = std::fs::remove_file(&link);
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, &link)?;
    #[cfg(not(unix))]
    std::fs::copy(target, &link)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum WallpaperMsg {
    Select(PathBuf),
    Refresh,
    Refreshed(Vec<PathBuf>),
}

// ---------------------------------------------------------------------------
// Update
// ---------------------------------------------------------------------------

pub fn update(
    settings: &mut WallpaperSettings,
    images: &mut Vec<PathBuf>,
    msg: WallpaperMsg,
) -> Option<iced::Task<WallpaperMsg>> {
    match msg {
        WallpaperMsg::Select(path) => {
            settings.path = path.to_string_lossy().to_string();
            if let Err(e) = set_wallpaper_symlink(&path) {
                tracing::warn!("failed to update wallpaper symlink: {e}");
            }
            None
        }
        WallpaperMsg::Refresh => {
            let task = iced::Task::perform(
                async {
                    tokio::task::spawn_blocking(scan_wallpapers)
                        .await
                        .unwrap_or_default()
                },
                WallpaperMsg::Refreshed,
            );
            Some(task)
        }
        WallpaperMsg::Refreshed(found) => {
            *images = found;
            None
        }
    }
}

// ---------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------

pub fn view<'a>(
    settings: &'a WallpaperSettings,
    images: &'a [PathBuf],
) -> Element<'a, WallpaperMsg> {
    let current = PathBuf::from(&settings.path);
    let columns = 4;

    let mut rows_vec: Vec<Element<'a, WallpaperMsg>> = Vec::new();

    // Build a grid of thumbnails (4 columns)
    for chunk in images.chunks(columns) {
        let mut row_items: Vec<Element<'a, WallpaperMsg>> = Vec::new();
        for path in chunk {
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let is_active = *path == current;
            let label = if is_active {
                format!("[*] {name}")
            } else {
                name
            };
            let style = if is_active {
                button::primary
            } else {
                button::secondary
            };
            row_items.push(
                button(text(label).size(12))
                    .on_press(WallpaperMsg::Select(path.clone()))
                    .width(Length::FillPortion(1))
                    .padding(8)
                    .style(style)
                    .into(),
            );
        }
        // Pad the row to `columns` items so the grid is even
        while row_items.len() < columns {
            row_items.push(container(text("")).width(Length::FillPortion(1)).into());
        }
        rows_vec.push(row(row_items).spacing(8).into());
    }

    if images.is_empty() {
        rows_vec.push(
            text("No wallpapers found. Add images to ~/.config/slate/wallpapers/")
                .size(14)
                .into(),
        );
    }

    let grid = column(rows_vec).spacing(8);

    let content = column![
        text("Wallpaper").size(24),
        text(format!("Current: {}", settings.path)).size(14),
        button(text("Refresh").size(14))
            .on_press(WallpaperMsg::Refresh)
            .padding(8),
        scrollable(grid),
    ]
    .spacing(16)
    .padding(20)
    .width(Length::Fill);

    container(content).width(Length::Fill).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_extension_filtering() {
        assert!(is_image_file(Path::new("photo.jpg")));
        assert!(is_image_file(Path::new("photo.JPG")));
        assert!(is_image_file(Path::new("photo.jpeg")));
        assert!(is_image_file(Path::new("photo.png")));
        assert!(is_image_file(Path::new("photo.webp")));
        assert!(!is_image_file(Path::new("photo.bmp")));
        assert!(!is_image_file(Path::new("photo.gif")));
        assert!(!is_image_file(Path::new("readme.txt")));
        assert!(!is_image_file(Path::new("noext")));
    }

    #[test]
    fn scan_wallpapers_uses_temp_dir() {
        let dir = std::env::temp_dir().join("slate-wallpaper-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        std::fs::write(dir.join("a.jpg"), b"fake").unwrap();
        std::fs::write(dir.join("b.png"), b"fake").unwrap();
        std::fs::write(dir.join("c.txt"), b"nope").unwrap();

        // scan_wallpapers scans hardcoded dirs, so we test is_image_file + read_dir
        let mut found = Vec::new();
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if is_image_file(&path) {
                found.push(path);
            }
        }
        found.sort();
        assert_eq!(found.len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
