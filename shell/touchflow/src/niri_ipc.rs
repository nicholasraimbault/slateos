/// Niri IPC client — dispatches window-management actions to Niri.
///
/// Rather than speaking the raw Niri socket protocol, this module shells out
/// to `niri msg action <command>`, which is simpler and more robust across
/// Niri versions.
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use anyhow::{Context, Result};
use tracing::{debug, error, info, warn};

/// Maximum consecutive failures before we consider Niri unresponsive.
const MAX_CONSECUTIVE_FAILURES: u32 = 3;
/// Time window for tracking consecutive failures.
const FAILURE_WINDOW_SECS: f64 = 10.0;

pub struct NiriClient {
    #[allow(dead_code)]
    socket_path: PathBuf,
    consecutive_failures: AtomicU32,
    first_failure_time: Mutex<Option<Instant>>,
}

impl NiriClient {
    /// Create a new Niri IPC client.
    ///
    /// Reads `$NIRI_SOCKET` (or accepts an override) to know where Niri
    /// listens. On construction we do **not** attempt a connection — that
    /// happens lazily on the first action.
    #[allow(dead_code)]
    pub async fn new() -> Result<Self> {
        Self::with_socket_override(None).await
    }

    /// Create a client with an explicit socket path override.
    pub async fn with_socket_override(override_path: Option<String>) -> Result<Self> {
        let socket_path = match override_path {
            Some(p) => PathBuf::from(p),
            None => {
                let p = std::env::var("NIRI_SOCKET")
                    .context("NIRI_SOCKET not set and no override provided")?;
                PathBuf::from(p)
            }
        };

        info!(?socket_path, "NiriClient initialised");

        Ok(Self {
            socket_path,
            consecutive_failures: AtomicU32::new(0),
            first_failure_time: Mutex::new(None),
        })
    }

    // -----------------------------------------------------------------------
    // High-level gesture actions
    // -----------------------------------------------------------------------

    #[allow(dead_code)]
    pub async fn focus_column_left(&self) -> Result<()> {
        self.run_action("focus-column-left").await
    }

    #[allow(dead_code)]
    pub async fn focus_column_right(&self) -> Result<()> {
        self.run_action("focus-column-right").await
    }

    pub async fn toggle_overview(&self) -> Result<()> {
        self.run_action("toggle-overview").await
    }

    pub async fn show_desktop(&self) -> Result<()> {
        // Niri doesn't have a single "show-desktop" action; we use
        // "focus-workspace-down" as a reasonable placeholder.  The real
        // implementation can be swapped later without touching dispatch.
        self.run_action("focus-workspace-down").await
    }

    #[allow(dead_code)]
    pub async fn move_column_left(&self) -> Result<()> {
        self.run_action("move-column-left").await
    }

    #[allow(dead_code)]
    pub async fn move_column_right(&self) -> Result<()> {
        self.run_action("move-column-right").await
    }

    pub async fn focus_workspace_up(&self) -> Result<()> {
        self.run_action("focus-workspace-up").await
    }

    pub async fn focus_workspace_down(&self) -> Result<()> {
        self.run_action("focus-workspace-down").await
    }

    pub async fn close_window(&self) -> Result<()> {
        self.run_action("close-window").await
    }

    // -----------------------------------------------------------------------
    // Internals
    // -----------------------------------------------------------------------

    async fn run_action(&self, action: &str) -> Result<()> {
        debug!(action, "dispatching niri action");

        let output = tokio::process::Command::new("niri")
            .args(["msg", "action", action])
            .output()
            .await
            .with_context(|| format!("spawning niri msg action {action}"))?;

        if output.status.success() {
            self.record_success();
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            self.record_failure(action, &stderr);
            anyhow::bail!("niri action {action} failed: {stderr}")
        }
    }

    fn record_success(&self) {
        self.consecutive_failures.store(0, Ordering::Relaxed);
        *self.first_failure_time.lock().unwrap() = None;
    }

    fn record_failure(&self, action: &str, stderr: &str) {
        let now = Instant::now();
        let mut first = self.first_failure_time.lock().unwrap();

        // Reset window if it has expired.
        if let Some(t) = *first {
            if now.duration_since(t).as_secs_f64() > FAILURE_WINDOW_SECS {
                self.consecutive_failures.store(0, Ordering::Relaxed);
                *first = None;
            }
        }

        if first.is_none() {
            *first = Some(now);
        }

        let count = self.consecutive_failures.fetch_add(1, Ordering::Relaxed) + 1;
        warn!(action, count, %stderr, "niri action failed");

        if count >= MAX_CONSECUTIVE_FAILURES {
            error!(
                "niri has failed {} consecutive times within {:.0}s — \
                 window manager may be unresponsive",
                count, FAILURE_WINDOW_SECS
            );
        }
    }

    /// Returns `true` when Niri has exceeded the failure threshold and should
    /// be considered unresponsive.
    #[allow(dead_code)]
    pub fn is_unresponsive(&self) -> bool {
        self.consecutive_failures.load(Ordering::Relaxed) >= MAX_CONSECUTIVE_FAILURES
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn client_construction_with_override() {
        let client = NiriClient::with_socket_override(Some("/tmp/test.sock".into()))
            .await
            .unwrap();
        assert_eq!(client.socket_path, PathBuf::from("/tmp/test.sock"));
    }

    #[tokio::test]
    async fn client_construction_without_env_fails() {
        // Temporarily ensure NIRI_SOCKET is not set.
        std::env::remove_var("NIRI_SOCKET");
        let result = NiriClient::new().await;
        // Should fail because NIRI_SOCKET is not set.
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn unresponsive_flag_default() {
        let client = NiriClient::with_socket_override(Some("/tmp/test.sock".into()))
            .await
            .unwrap();
        assert!(!client.is_unresponsive());
    }

    #[test]
    fn failure_tracking() {
        let client = NiriClient {
            socket_path: PathBuf::from("/tmp/test.sock"),
            consecutive_failures: AtomicU32::new(0),
            first_failure_time: Mutex::new(None),
        };

        // Record failures.
        client.record_failure("test-action", "test error");
        client.record_failure("test-action", "test error");
        assert!(!client.is_unresponsive());

        client.record_failure("test-action", "test error");
        assert!(client.is_unresponsive());

        // A success resets the counter.
        client.record_success();
        assert!(!client.is_unresponsive());
    }
}
