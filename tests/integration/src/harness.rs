// Test harness for spawning and managing D-Bus daemons.
//
// Provides helpers to start slate-notifyd and rhea as child processes,
// wait for them to claim their bus names, and clean up on drop.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use tokio::process::{Child, Command};
use zbus::Connection;

/// How long to wait for a daemon to claim its bus name.
const BUS_NAME_TIMEOUT: Duration = Duration::from_secs(5);

/// How often to poll for bus name availability.
const BUS_NAME_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Check whether a D-Bus session bus is available.
/// Returns None if we're on a system without D-Bus (e.g., macOS).
pub async fn try_session_bus() -> Option<Connection> {
    Connection::session().await.ok()
}

/// Locate a compiled binary in the workspace target directory.
/// Searches target/debug/ first, then target/release/.
pub fn find_binary(name: &str) -> Option<PathBuf> {
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())?;

    let debug_path = workspace_root.join("target/debug").join(name);
    if debug_path.exists() {
        return Some(debug_path);
    }

    let release_path = workspace_root.join("target/release").join(name);
    if release_path.exists() {
        return Some(release_path);
    }

    None
}

/// A managed daemon process that cleans up on drop.
pub struct DaemonProcess {
    child: Child,
    name: String,
}

impl DaemonProcess {
    /// Spawn a daemon binary and wait for it to claim its D-Bus bus name.
    pub async fn spawn(
        binary_name: &str,
        bus_name: &str,
        conn: &Connection,
        env_vars: Vec<(&str, &str)>,
    ) -> anyhow::Result<Self> {
        let binary_path = find_binary(binary_name)
            .ok_or_else(|| anyhow::anyhow!(
                "binary '{}' not found in target/debug or target/release — run `cargo build -p {}` first",
                binary_name, binary_name
            ))?;

        tracing::info!("spawning {binary_name} from {}", binary_path.display());

        let mut cmd = Command::new(&binary_path);
        cmd.stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        for (key, val) in &env_vars {
            cmd.env(key, val);
        }

        let child = cmd.spawn()
            .map_err(|e| anyhow::anyhow!("failed to spawn {binary_name}: {e}"))?;

        // Wait for the daemon to claim its bus name.
        wait_for_bus_name(conn, bus_name).await
            .map_err(|e| anyhow::anyhow!(
                "{binary_name} did not claim bus name '{bus_name}' within {:?}: {e}",
                BUS_NAME_TIMEOUT
            ))?;

        tracing::info!("{binary_name} is ready (bus name: {bus_name})");

        Ok(Self {
            child,
            name: binary_name.to_string(),
        })
    }

    /// Send SIGTERM and wait for graceful shutdown.
    pub async fn shutdown(mut self) -> anyhow::Result<()> {
        tracing::info!("shutting down {}", self.name);

        // Send SIGTERM via nix crate or kill command.
        if let Some(pid) = self.child.id() {
            let _ = Command::new("kill")
                .arg("-TERM")
                .arg(pid.to_string())
                .status()
                .await;
        }

        // Wait up to 3 seconds for graceful exit.
        match tokio::time::timeout(Duration::from_secs(3), self.child.wait()).await {
            Ok(Ok(status)) => {
                tracing::info!("{} exited with {status}", self.name);
                Ok(())
            }
            Ok(Err(e)) => {
                tracing::warn!("{} wait error: {e}", self.name);
                Ok(())
            }
            Err(_) => {
                tracing::warn!("{} did not exit in 3s, killing", self.name);
                let _ = self.child.kill().await;
                Ok(())
            }
        }
    }
}

impl Drop for DaemonProcess {
    fn drop(&mut self) {
        // Best-effort kill if not already shut down.
        // Can't await in Drop, so use start_kill() which is non-blocking.
        let _ = self.child.start_kill();
    }
}

/// Wait until a bus name is owned on the session bus.
async fn wait_for_bus_name(conn: &Connection, bus_name: &str) -> anyhow::Result<()> {
    let deadline = tokio::time::Instant::now() + BUS_NAME_TIMEOUT;

    loop {
        // Use the standard D-Bus method to check name ownership.
        let proxy = zbus::fdo::DBusProxy::new(conn).await?;
        if proxy.name_has_owner(bus_name.try_into()?).await? {
            return Ok(());
        }

        if tokio::time::Instant::now() >= deadline {
            return Err(anyhow::anyhow!("timeout waiting for bus name '{bus_name}'"));
        }

        tokio::time::sleep(BUS_NAME_POLL_INTERVAL).await;
    }
}

/// Helper: create a zbus proxy for a well-known Slate D-Bus interface.
pub async fn slate_proxy(
    conn: &Connection,
    bus_name: &str,
    path: &str,
    interface: &str,
) -> anyhow::Result<zbus::Proxy<'static>> {
    let proxy = zbus::Proxy::new(
        conn,
        bus_name.to_string(),
        path.to_string(),
        interface.to_string(),
    )
    .await
    .map_err(|e| anyhow::anyhow!("proxy build for {bus_name}: {e}"))?;
    Ok(proxy)
}

/// Skip a test gracefully if no D-Bus session bus is available.
#[macro_export]
macro_rules! skip_without_dbus {
    () => {
        if $crate::harness::try_session_bus().await.is_none() {
            eprintln!("SKIP: no D-Bus session bus available");
            return;
        }
    };
}

/// Skip a test if a required binary hasn't been compiled.
#[macro_export]
macro_rules! skip_without_binary {
    ($name:expr) => {
        if $crate::harness::find_binary($name).is_none() {
            eprintln!("SKIP: binary '{}' not found — run `cargo build -p {}`", $name, $name);
            return;
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_binary_returns_none_for_nonexistent() {
        assert!(find_binary("this-binary-does-not-exist-12345").is_none());
    }

    #[tokio::test]
    async fn try_session_bus_does_not_panic() {
        // May return Some or None depending on environment.
        let _ = try_session_bus().await;
    }
}
