/// Wallpaper file watcher.
///
/// Watches `~/.config/slate/wallpaper` (a symlink) for changes using the
/// `notify` crate. Events are debounced (500ms) so rapid filesystem churn
/// produces at most one extraction trigger.
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;

/// Debounce window — ignore events within this duration of the last trigger.
const DEBOUNCE_MS: u64 = 500;

/// Resolved wallpaper path message sent when the symlink changes.
#[derive(Debug, Clone)]
pub struct WallpaperChanged {
    pub path: PathBuf,
}

/// Start watching the wallpaper symlink.
///
/// Returns an `mpsc::Receiver` that yields `WallpaperChanged` messages.
/// The watcher handle is returned so the caller can keep it alive; dropping
/// it stops the watch.
pub fn start_watching(
    wallpaper_symlink: PathBuf,
) -> Result<(mpsc::Receiver<WallpaperChanged>, RecommendedWatcher)> {
    let (tx, rx) = mpsc::channel::<WallpaperChanged>(16);

    // Watch the parent directory since the symlink itself may be replaced
    // (rm + ln -sf) rather than modified in-place.
    let watch_dir = wallpaper_symlink
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));

    let symlink_name = wallpaper_symlink
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();

    // Clone symlink path for use inside the closure (the original is needed
    // later for logging).
    let symlink_for_closure = wallpaper_symlink.clone();

    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let event = match res {
            Ok(e) => e,
            Err(err) => {
                tracing::warn!("watcher error: {err}");
                return;
            }
        };

        // We only care about create/modify/remove events that touch the
        // wallpaper symlink (or its name in the directory).
        let dominated = matches!(
            event.kind,
            EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
        );
        if !dominated {
            return;
        }

        let touches_symlink = event
            .paths
            .iter()
            .any(|p| p.file_name().map(|n| n == symlink_name).unwrap_or(false));
        if !touches_symlink {
            return;
        }

        // Resolve the symlink target. If the symlink was just deleted
        // (pre-replace), ignore it — we'll get a Create event next.
        let resolved = match std::fs::canonicalize(&symlink_for_closure) {
            Ok(p) => p,
            Err(_) => return,
        };

        let msg = WallpaperChanged { path: resolved };
        // Best-effort send; if the channel is full the event is dropped
        // (the next event will catch up).
        let _ = tx.try_send(msg);
    })
    .context("create filesystem watcher")?;

    // Create the watch directory if it doesn't exist yet so we can start
    // watching before the user has ever set a wallpaper.
    if !watch_dir.exists() {
        std::fs::create_dir_all(&watch_dir)
            .with_context(|| format!("create watch dir {}", watch_dir.display()))?;
    }

    watcher
        .watch(&watch_dir, RecursiveMode::NonRecursive)
        .with_context(|| format!("watch {}", watch_dir.display()))?;

    tracing::info!(
        "watching {} for wallpaper changes",
        wallpaper_symlink.display()
    );

    Ok((rx, watcher))
}

/// Debounce wrapper around the watcher receiver.
///
/// Waits for a message, then drains any further messages arriving within
/// `DEBOUNCE_MS`, returning only the last one (most up-to-date path).
pub async fn debounced_recv(rx: &mut mpsc::Receiver<WallpaperChanged>) -> Option<WallpaperChanged> {
    // Wait for the first event.
    let mut last = rx.recv().await?;

    // Drain any events that arrive within the debounce window.
    let deadline = tokio::time::Instant::now() + Duration::from_millis(DEBOUNCE_MS);
    while let Ok(Some(msg)) = tokio::time::timeout_at(deadline, rx.recv()).await {
        last = msg;
    }

    Some(last)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Debounce collapses multiple rapid events into one.
    #[tokio::test]
    async fn debounce_collapses_rapid_events() {
        let (tx, mut rx) = mpsc::channel::<WallpaperChanged>(16);

        // Send three events in quick succession.
        for i in 0..3 {
            tx.send(WallpaperChanged {
                path: PathBuf::from(format!("/tmp/wall_{i}.jpg")),
            })
            .await
            .unwrap();
        }

        let result = debounced_recv(&mut rx).await.unwrap();
        // Should get the last one (most recent).
        assert_eq!(result.path, PathBuf::from("/tmp/wall_2.jpg"));
    }

    /// Debounce returns None when the channel is closed without messages.
    #[tokio::test]
    async fn debounce_returns_none_on_closed_channel() {
        let (tx, mut rx) = mpsc::channel::<WallpaperChanged>(1);
        drop(tx);

        let result = debounced_recv(&mut rx).await;
        assert!(result.is_none());
    }

    /// The watcher can be created for a temporary directory.
    #[test]
    fn start_watching_creates_watcher() {
        let dir = tempfile::tempdir().unwrap();
        let symlink = dir.path().join("wallpaper");

        // Create a dummy file so the symlink target exists.
        let target = dir.path().join("image.jpg");
        std::fs::write(&target, b"fake image").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &symlink).unwrap();
        #[cfg(not(unix))]
        std::fs::write(&symlink, b"fake symlink").unwrap();

        let result = start_watching(symlink);
        assert!(result.is_ok());
    }

    /// The watcher works even if the symlink doesn't exist yet.
    #[test]
    fn start_watching_handles_missing_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let symlink = dir.path().join("wallpaper");
        // Symlink doesn't exist — watcher should still start (watching parent).
        let result = start_watching(symlink);
        assert!(result.is_ok());
    }
}
