/// Gesture dispatch — maps recognized gestures to system actions.
///
/// This is the integration layer between the gesture recognizer (pure logic)
/// and the Niri IPC / D-Bus emitter (system effects).  It consults the
/// configuration to honour enable/disable toggles for each gesture category.
use anyhow::Result;
use tracing::debug;

use crate::config::TouchFlowConfig;
use crate::edge::{Edge, EdgeGesture};
use crate::gesture::{GestureType, SwipeDirection};

// ---------------------------------------------------------------------------
// Dispatch trait — allows mocking in tests
// ---------------------------------------------------------------------------

/// Abstraction over Niri IPC so dispatch logic can be tested without a
/// running compositor.
#[allow(async_fn_in_trait)]
pub trait NiriDispatch {
    async fn focus_column_left(&self) -> Result<()>;
    async fn focus_column_right(&self) -> Result<()>;
    async fn toggle_overview(&self) -> Result<()>;
    async fn show_desktop(&self) -> Result<()>;
    async fn move_column_left(&self) -> Result<()>;
    async fn move_column_right(&self) -> Result<()>;
    async fn focus_workspace_up(&self) -> Result<()>;
    async fn focus_workspace_down(&self) -> Result<()>;
    async fn close_window(&self) -> Result<()>;
}

/// Abstraction over D-Bus emitter so dispatch logic can be tested without a
/// running session bus.
#[allow(async_fn_in_trait)]
pub trait EmitterDispatch {
    async fn show_dock(&self) -> Result<()>;
    async fn hide_dock(&self) -> Result<()>;
    async fn show_launcher(&self) -> Result<()>;
    async fn hide_launcher(&self) -> Result<()>;
    async fn show_claw(&self) -> Result<()>;
    async fn hide_claw(&self) -> Result<()>;
    async fn emit_edge_gesture(
        &self,
        edge: &str,
        phase: &str,
        progress: f64,
        velocity: f64,
    ) -> Result<()>;
}

// Blanket impls for the production types.

impl NiriDispatch for crate::niri_ipc::NiriClient {
    async fn focus_column_left(&self) -> Result<()> {
        self.focus_column_left().await
    }
    async fn focus_column_right(&self) -> Result<()> {
        self.focus_column_right().await
    }
    async fn toggle_overview(&self) -> Result<()> {
        self.toggle_overview().await
    }
    async fn show_desktop(&self) -> Result<()> {
        self.show_desktop().await
    }
    async fn move_column_left(&self) -> Result<()> {
        self.move_column_left().await
    }
    async fn move_column_right(&self) -> Result<()> {
        self.move_column_right().await
    }
    async fn focus_workspace_up(&self) -> Result<()> {
        self.focus_workspace_up().await
    }
    async fn focus_workspace_down(&self) -> Result<()> {
        self.focus_workspace_down().await
    }
    async fn close_window(&self) -> Result<()> {
        self.close_window().await
    }
}

impl EmitterDispatch for crate::dbus_emitter::TouchFlowEmitter {
    async fn show_dock(&self) -> Result<()> {
        self.show_dock().await
    }
    async fn hide_dock(&self) -> Result<()> {
        self.hide_dock().await
    }
    async fn show_launcher(&self) -> Result<()> {
        self.show_launcher().await
    }
    async fn hide_launcher(&self) -> Result<()> {
        self.hide_launcher().await
    }
    async fn show_claw(&self) -> Result<()> {
        self.show_claw().await
    }
    async fn hide_claw(&self) -> Result<()> {
        self.hide_claw().await
    }
    async fn emit_edge_gesture(
        &self,
        edge: &str,
        phase: &str,
        progress: f64,
        velocity: f64,
    ) -> Result<()> {
        self.emit_edge_gesture(edge, phase, progress, velocity)
            .await
    }
}

// ---------------------------------------------------------------------------
// Core dispatch function
// ---------------------------------------------------------------------------

/// Result of dispatching a gesture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchResult {
    /// The gesture was dispatched to a system action.
    Dispatched,
    /// The gesture should be passed through (1/2-finger, or disabled).
    Passthrough,
}

/// Map a recognized gesture to a system action.
///
/// Returns [`DispatchResult::Passthrough`] for 1-finger and 2-finger
/// gestures (those are handled by the compositor directly), and for
/// gesture categories that are disabled in config.
pub async fn dispatch_gesture<N: NiriDispatch, E: EmitterDispatch>(
    gesture: &GestureType,
    niri: &N,
    emitter: &E,
    config: &TouchFlowConfig,
) -> Result<DispatchResult> {
    if !config.gestures.enabled {
        debug!("gestures globally disabled — passthrough");
        return Ok(DispatchResult::Passthrough);
    }

    match gesture {
        // ---- 1-finger / 2-finger: always passthrough ----
        GestureType::Tap { fingers, .. } | GestureType::DoubleTap { fingers, .. }
            if *fingers <= 2 =>
        {
            debug!(fingers, "1/2-finger tap — passthrough");
            Ok(DispatchResult::Passthrough)
        }

        GestureType::LongPress { fingers, .. } if *fingers <= 2 => {
            debug!(fingers, "1/2-finger long press — passthrough");
            Ok(DispatchResult::Passthrough)
        }

        GestureType::Swipe { fingers, .. } if *fingers <= 2 => {
            debug!(fingers, "1/2-finger swipe — passthrough");
            Ok(DispatchResult::Passthrough)
        }

        GestureType::Pinch { fingers, .. } if *fingers <= 2 => {
            debug!(fingers, "1/2-finger pinch — passthrough");
            Ok(DispatchResult::Passthrough)
        }

        // ---- 3-finger swipe ----
        GestureType::Swipe {
            fingers: 3,
            direction,
            ..
        } => {
            if !config.gestures.swipe_enabled {
                debug!("3-finger swipe disabled — passthrough");
                return Ok(DispatchResult::Passthrough);
            }
            match direction {
                SwipeDirection::Left => {
                    debug!("3-finger swipe left → switch workspace up");
                    niri.focus_workspace_up().await?;
                }
                SwipeDirection::Right => {
                    debug!("3-finger swipe right → switch workspace down");
                    niri.focus_workspace_down().await?;
                }
                SwipeDirection::Up => {
                    debug!("3-finger swipe up → toggle_overview");
                    niri.toggle_overview().await?;
                }
                SwipeDirection::Down => {
                    debug!("3-finger swipe down → show_desktop");
                    niri.show_desktop().await?;
                }
            }
            Ok(DispatchResult::Dispatched)
        }

        // ---- 3-finger double-tap ----
        GestureType::DoubleTap { fingers: 3, .. } => {
            if !config.gestures.tap_enabled {
                debug!("3-finger double-tap disabled — passthrough");
                return Ok(DispatchResult::Passthrough);
            }
            debug!("3-finger double-tap → show_claw");
            emitter.show_claw().await?;
            Ok(DispatchResult::Dispatched)
        }

        // ---- 4-finger pinch ----
        GestureType::Pinch {
            fingers: 4,
            scale_delta,
        } => {
            if !config.gestures.pinch_enabled {
                debug!("4-finger pinch disabled — passthrough");
                return Ok(DispatchResult::Passthrough);
            }
            if *scale_delta < 1.0 {
                debug!("4-finger pinch in → show_launcher");
                emitter.show_launcher().await?;
            } else {
                debug!("4-finger pinch out → hide_launcher");
                emitter.hide_launcher().await?;
            }
            Ok(DispatchResult::Dispatched)
        }

        // ---- Edge swipes ----
        GestureType::EdgeSwipe {
            edge, direction, ..
        } => {
            if !config.gestures.edge_swipe_enabled {
                debug!("edge swipe disabled — passthrough");
                return Ok(DispatchResult::Passthrough);
            }
            match (edge, direction) {
                (Edge::Bottom, SwipeDirection::Up) => {
                    debug!("edge swipe bottom→up → show_dock");
                    emitter.show_dock().await?;
                }
                (Edge::Right, SwipeDirection::Left) => {
                    debug!("edge swipe right→left → close window (back/close)");
                    niri.close_window().await?;
                }
                (Edge::Left, SwipeDirection::Right) => {
                    debug!("edge swipe left→right → forward / undo close (noop for now)");
                    // Will integrate with undo-close or xdg-portal later.
                }
                (Edge::Top, SwipeDirection::Down) => {
                    debug!("edge swipe top→down → quick settings (noop for now)");
                    // Future: open quick-settings panel.
                }
                _ => {
                    debug!(?edge, ?direction, "unhandled edge swipe — passthrough");
                    return Ok(DispatchResult::Passthrough);
                }
            }
            Ok(DispatchResult::Dispatched)
        }

        // ---- Continuous edge gesture ----
        GestureType::ContinuousEdge(EdgeGesture {
            edge,
            phase,
            progress,
            velocity,
        }) => {
            if !config.gestures.edge_swipe_enabled {
                debug!("continuous edge gesture disabled — passthrough");
                return Ok(DispatchResult::Passthrough);
            }
            debug!(
                edge = edge.as_str(),
                phase = phase.as_str(),
                progress,
                velocity,
                "continuous edge gesture"
            );
            let _ = emitter
                .emit_edge_gesture(edge.as_str(), phase.as_str(), *progress, *velocity)
                .await;
            Ok(DispatchResult::Dispatched)
        }

        // ---- Anything else: passthrough ----
        other => {
            debug!(?other, "unhandled gesture — passthrough");
            Ok(DispatchResult::Passthrough)
        }
    }
}

#[cfg(test)]
#[path = "dispatch_tests.rs"]
mod tests;
