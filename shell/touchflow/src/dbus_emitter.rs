/// D-Bus signal emitter for TouchFlow.
///
/// Emits method calls to other Slate OS components (dock, launcher, claw panel)
/// in response to recognized gestures.  Also registers a small
/// `org.slate.TouchFlow` interface so other components can query state.
use anyhow::{Context, Result};
use tracing::{debug, warn};
use zbus::Connection;

use slate_common::dbus;

/// Emitter that sends D-Bus signals / method calls to other Slate components.
pub struct TouchFlowEmitter {
    connection: Connection,
}

impl TouchFlowEmitter {
    /// Connect to the session bus.
    pub async fn new() -> Result<Self> {
        let connection = Connection::session()
            .await
            .context("connecting to session D-Bus")?;

        debug!("TouchFlowEmitter connected to session bus");
        Ok(Self { connection })
    }

    // -----------------------------------------------------------------------
    // Dock (Shoal)
    // -----------------------------------------------------------------------

    pub async fn show_dock(&self) -> Result<()> {
        debug!("emitting Show to Dock");
        self.call_method(dbus::DOCK_INTERFACE, dbus::DOCK_PATH, "Show")
            .await
    }

    pub async fn hide_dock(&self) -> Result<()> {
        debug!("emitting Hide to Dock");
        self.call_method(dbus::DOCK_INTERFACE, dbus::DOCK_PATH, "Hide")
            .await
    }

    // -----------------------------------------------------------------------
    // Launcher
    // -----------------------------------------------------------------------

    pub async fn show_launcher(&self) -> Result<()> {
        debug!("emitting Show to Launcher");
        self.call_method(dbus::LAUNCHER_INTERFACE, dbus::LAUNCHER_PATH, "Show")
            .await
    }

    pub async fn hide_launcher(&self) -> Result<()> {
        debug!("emitting Hide to Launcher");
        self.call_method(dbus::LAUNCHER_INTERFACE, dbus::LAUNCHER_PATH, "Hide")
            .await
    }

    // -----------------------------------------------------------------------
    // Claw Panel
    // -----------------------------------------------------------------------

    pub async fn show_claw(&self) -> Result<()> {
        debug!("emitting Show to Claw");
        self.call_method(dbus::CLAW_INTERFACE, dbus::CLAW_PATH, "Show")
            .await
    }

    pub async fn hide_claw(&self) -> Result<()> {
        debug!("emitting Hide to Claw");
        self.call_method(dbus::CLAW_INTERFACE, dbus::CLAW_PATH, "Hide")
            .await
    }

    // -----------------------------------------------------------------------
    // Internal helper
    // -----------------------------------------------------------------------

    async fn call_method(&self, destination: &str, path: &str, method: &str) -> Result<()> {
        let result = self
            .connection
            .call_method(
                Some(destination.to_owned()),
                path,
                Some(destination.to_owned()),
                method,
                &(),
            )
            .await;

        match result {
            Ok(_) => {
                debug!(destination, method, "D-Bus call succeeded");
                Ok(())
            }
            Err(e) => {
                // Non-fatal: the target component may not be running yet.
                warn!(destination, method, %e, "D-Bus call failed (target may not be running)");
                Err(anyhow::anyhow!(
                    "D-Bus call {destination}.{method} failed: {e}"
                ))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Mock for testing
// ---------------------------------------------------------------------------

/// A recording mock that captures calls instead of making actual D-Bus calls.
/// Used in dispatch tests.
#[cfg(test)]
pub mod mock {
    use std::sync::{Arc, Mutex};

    use anyhow::Result;

    #[derive(Debug, Clone, PartialEq)]
    pub enum EmitterCall {
        ShowDock,
        HideDock,
        ShowLauncher,
        HideLauncher,
        ShowClaw,
        HideClaw,
    }

    #[derive(Debug, Clone)]
    pub struct MockEmitter {
        pub calls: Arc<Mutex<Vec<EmitterCall>>>,
    }

    impl MockEmitter {
        pub fn new() -> Self {
            Self {
                calls: Arc::new(Mutex::new(Vec::new())),
            }
        }

        pub async fn show_dock(&self) -> Result<()> {
            self.calls.lock().unwrap().push(EmitterCall::ShowDock);
            Ok(())
        }

        pub async fn hide_dock(&self) -> Result<()> {
            self.calls.lock().unwrap().push(EmitterCall::HideDock);
            Ok(())
        }

        pub async fn show_launcher(&self) -> Result<()> {
            self.calls.lock().unwrap().push(EmitterCall::ShowLauncher);
            Ok(())
        }

        pub async fn hide_launcher(&self) -> Result<()> {
            self.calls.lock().unwrap().push(EmitterCall::HideLauncher);
            Ok(())
        }

        pub async fn show_claw(&self) -> Result<()> {
            self.calls.lock().unwrap().push(EmitterCall::ShowClaw);
            Ok(())
        }

        pub async fn hide_claw(&self) -> Result<()> {
            self.calls.lock().unwrap().push(EmitterCall::HideClaw);
            Ok(())
        }

        pub fn recorded(&self) -> Vec<EmitterCall> {
            self.calls.lock().unwrap().clone()
        }
    }
}
