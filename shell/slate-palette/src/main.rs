/// slate-palette — Dynamic theming daemon for Slate OS.
///
/// Watches the wallpaper symlink, extracts Material You colours via matugen,
/// generates theme files (TOML, CSS, KDL), broadcasts over D-Bus, and signals
/// Waybar to reload.
mod broadcast;
mod extractor;
mod output;
mod reload;
mod watcher;

use std::path::PathBuf;

use anyhow::{Context, Result};
use slate_common::Palette;

/// Canonical location of the wallpaper symlink.
fn wallpaper_symlink() -> PathBuf {
    dirs_slate("wallpaper")
}

/// Canonical location of the palette TOML file.
fn palette_toml_path() -> PathBuf {
    dirs_slate("palette.toml")
}

/// Canonical location of the palette CSS file.
fn palette_css_path() -> PathBuf {
    dirs_slate("palette.css")
}

/// Canonical location of the Niri palette KDL file.
fn palette_kdl_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
    PathBuf::from(home)
        .join(".config")
        .join("niri")
        .join("palette.kdl")
}

fn dirs_slate(name: &str) -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
    PathBuf::from(home).join(".config").join("slate").join(name)
}

/// Load the palette from disk, falling back to the default.
fn load_initial_palette() -> Palette {
    let path = palette_toml_path();
    if path.exists() {
        match std::fs::read_to_string(&path) {
            Ok(content) => match toml::from_str::<Palette>(&content) {
                Ok(palette) => {
                    tracing::info!("loaded palette from {}", path.display());
                    return palette;
                }
                Err(err) => {
                    tracing::warn!("invalid palette.toml, using default: {err}");
                }
            },
            Err(err) => {
                tracing::warn!("could not read palette.toml, using default: {err}");
            }
        }
    } else {
        tracing::info!("no palette.toml found, using default palette");
    }
    Palette::default()
}

/// Write all palette output files.
fn write_all_outputs(palette: &Palette) -> Result<()> {
    output::write_palette_toml(palette, &palette_toml_path())?;
    output::write_palette_css(palette, &palette_css_path())?;
    output::write_palette_kdl(palette, &palette_kdl_path())?;
    Ok(())
}

/// Handle a wallpaper change: extract colours, write files, broadcast, reload.
async fn handle_wallpaper_change(
    path: &std::path::Path,
    connection: &zbus::Connection,
    current_palette: &mut Palette,
) {
    tracing::info!("wallpaper changed: {}", path.display());

    match extractor::extract_palette(path).await {
        Ok(new_palette) => {
            if new_palette == *current_palette {
                tracing::debug!("palette unchanged, skipping update");
                return;
            }

            *current_palette = new_palette;

            if let Err(err) = write_all_outputs(current_palette) {
                tracing::error!("failed to write palette files: {err:#}");
                return;
            }

            if let Err(err) = broadcast::broadcast_palette(connection, current_palette).await {
                tracing::error!("failed to broadcast palette: {err:#}");
            }

            reload::reload_waybar();
        }
        Err(err) => {
            tracing::error!("colour extraction failed: {err:#}");
            tracing::info!("keeping current palette");
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialise tracing.
    tracing_subscriber::fmt::init();

    tracing::info!("slate-palette starting");

    // Load initial palette.
    let mut palette = load_initial_palette();

    // Write initial palette files so downstream consumers have something.
    if let Err(err) = write_all_outputs(&palette) {
        tracing::warn!("failed to write initial palette files: {err:#}");
    }

    // Set up D-Bus.
    let connection = broadcast::create_connection(&palette)
        .await
        .context("set up D-Bus connection")?;

    // Start watching the wallpaper symlink.
    let (mut rx, _watcher) =
        watcher::start_watching(wallpaper_symlink()).context("start wallpaper watcher")?;

    tracing::info!("slate-palette ready");

    // Event loop with graceful shutdown.
    loop {
        tokio::select! {
            // Wallpaper changed.
            maybe_event = watcher::debounced_recv(&mut rx) => {
                match maybe_event {
                    Some(event) => {
                        handle_wallpaper_change(&event.path, &connection, &mut palette).await;
                    }
                    None => {
                        tracing::info!("watcher channel closed, shutting down");
                        break;
                    }
                }
            }

            // SIGTERM / SIGINT.
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("received shutdown signal");
                break;
            }
        }
    }

    tracing::info!("slate-palette stopped");
    Ok(())
}
