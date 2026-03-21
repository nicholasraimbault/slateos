/// slate-palette — Dynamic theming daemon for Slate OS.
///
/// Watches the wallpaper symlink, extracts Material You colours (via matugen or
/// a built-in fallback), generates theme files (TOML, CSS, KDL), broadcasts
/// over D-Bus, and signals Waybar to reload.
///
/// Modes:
///   (default)   — watch mode: run continuously, re-extract on wallpaper changes
///   --oneshot   — extract once from the current wallpaper, write files, broadcast, exit
mod broadcast;
mod builtin_extract;
mod extractor;
mod output;
mod reload;
mod watcher;

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use slate_common::Palette;

/// Dynamic theming daemon for Slate OS.
#[derive(Parser, Debug)]
#[command(name = "slate-palette", about = "Dynamic Material You theming")]
struct Cli {
    /// Extract palette once, write output files, broadcast via D-Bus, then exit.
    #[arg(long)]
    oneshot: bool,
}

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

/// Run --oneshot mode: resolve the wallpaper, extract palette, write outputs,
/// broadcast via D-Bus, then exit.
async fn run_oneshot() -> Result<()> {
    let symlink = wallpaper_symlink();

    // Resolve the wallpaper symlink to its target image.
    let image_path = std::fs::canonicalize(&symlink).with_context(|| {
        format!(
            "resolve wallpaper symlink {} — does the wallpaper exist?",
            symlink.display()
        )
    })?;
    tracing::info!("oneshot: wallpaper is {}", image_path.display());

    // Extract palette from the wallpaper image.
    let palette = extractor::extract_palette(&image_path)
        .await
        .context("extract palette in oneshot mode")?;

    // Write output files.
    write_all_outputs(&palette).context("write palette files in oneshot mode")?;

    // Broadcast via D-Bus (best-effort — during first-boot there may be
    // no session bus, so we log and continue rather than failing).
    match broadcast::create_connection(&palette).await {
        Ok(connection) => {
            if let Err(err) = broadcast::broadcast_palette(&connection, &palette).await {
                tracing::warn!("oneshot: D-Bus broadcast failed (non-fatal): {err:#}");
            }
        }
        Err(err) => {
            tracing::warn!("oneshot: could not connect to D-Bus (non-fatal): {err:#}");
        }
    }

    reload::reload_waybar();

    tracing::info!("oneshot: palette written successfully");
    Ok(())
}

/// Run the continuous watch-mode daemon.
async fn run_watch() -> Result<()> {
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

    tracing::info!("slate-palette ready (watch mode)");

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

#[tokio::main]
async fn main() -> Result<()> {
    // Initialise tracing.
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    tracing::info!("slate-palette starting");

    if cli.oneshot {
        tracing::info!("running in oneshot mode");
        run_oneshot().await
    } else {
        run_watch().await
    }
}
