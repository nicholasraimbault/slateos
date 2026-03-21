/// Gesture dispatch — maps recognized gestures to system actions.
///
/// This is the integration layer between the gesture recognizer (pure logic)
/// and the Niri IPC / D-Bus emitter (system effects).  It consults the
/// configuration to honour enable/disable toggles for each gesture category.
use anyhow::Result;
use tracing::debug;

use crate::config::TouchFlowConfig;
use crate::edge::Edge;
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

        // ---- Anything else: passthrough ----
        other => {
            debug!(?other, "unhandled gesture — passthrough");
            Ok(DispatchResult::Passthrough)
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TouchFlowConfig;
    use crate::dbus_emitter::mock::{EmitterCall, MockEmitter};
    use crate::edge::Edge;
    use crate::gesture::{GestureType, SwipeDirection};
    use std::sync::{Arc, Mutex};

    // -- Mock Niri client --

    #[derive(Debug, Clone, PartialEq)]
    enum NiriCall {
        FocusColumnLeft,
        FocusColumnRight,
        ToggleOverview,
        ShowDesktop,
        MoveColumnLeft,
        MoveColumnRight,
        FocusWorkspaceUp,
        FocusWorkspaceDown,
        CloseWindow,
    }

    struct MockNiri {
        calls: Arc<Mutex<Vec<NiriCall>>>,
    }

    impl MockNiri {
        fn new() -> Self {
            Self {
                calls: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn recorded(&self) -> Vec<NiriCall> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl NiriDispatch for MockNiri {
        async fn focus_column_left(&self) -> Result<()> {
            self.calls.lock().unwrap().push(NiriCall::FocusColumnLeft);
            Ok(())
        }
        async fn focus_column_right(&self) -> Result<()> {
            self.calls.lock().unwrap().push(NiriCall::FocusColumnRight);
            Ok(())
        }
        async fn toggle_overview(&self) -> Result<()> {
            self.calls.lock().unwrap().push(NiriCall::ToggleOverview);
            Ok(())
        }
        async fn show_desktop(&self) -> Result<()> {
            self.calls.lock().unwrap().push(NiriCall::ShowDesktop);
            Ok(())
        }
        async fn move_column_left(&self) -> Result<()> {
            self.calls.lock().unwrap().push(NiriCall::MoveColumnLeft);
            Ok(())
        }
        async fn move_column_right(&self) -> Result<()> {
            self.calls.lock().unwrap().push(NiriCall::MoveColumnRight);
            Ok(())
        }
        async fn focus_workspace_up(&self) -> Result<()> {
            self.calls.lock().unwrap().push(NiriCall::FocusWorkspaceUp);
            Ok(())
        }
        async fn focus_workspace_down(&self) -> Result<()> {
            self.calls
                .lock()
                .unwrap()
                .push(NiriCall::FocusWorkspaceDown);
            Ok(())
        }
        async fn close_window(&self) -> Result<()> {
            self.calls.lock().unwrap().push(NiriCall::CloseWindow);
            Ok(())
        }
    }

    impl EmitterDispatch for MockEmitter {
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
    }

    fn default_config() -> TouchFlowConfig {
        TouchFlowConfig::default()
    }

    // -- Tests --

    #[tokio::test]
    async fn three_finger_swipe_left_dispatches_workspace_up() {
        let niri = MockNiri::new();
        let emitter = MockEmitter::new();
        let config = default_config();
        let gesture = GestureType::Swipe {
            fingers: 3,
            direction: SwipeDirection::Left,
            velocity: 500.0,
        };

        let result = dispatch_gesture(&gesture, &niri, &emitter, &config)
            .await
            .unwrap();
        assert_eq!(result, DispatchResult::Dispatched);
        assert_eq!(niri.recorded(), vec![NiriCall::FocusWorkspaceUp]);
        assert!(emitter.recorded().is_empty());
    }

    #[tokio::test]
    async fn three_finger_swipe_right_dispatches_workspace_down() {
        let niri = MockNiri::new();
        let emitter = MockEmitter::new();
        let config = default_config();
        let gesture = GestureType::Swipe {
            fingers: 3,
            direction: SwipeDirection::Right,
            velocity: 500.0,
        };

        let result = dispatch_gesture(&gesture, &niri, &emitter, &config)
            .await
            .unwrap();
        assert_eq!(result, DispatchResult::Dispatched);
        assert_eq!(niri.recorded(), vec![NiriCall::FocusWorkspaceDown]);
    }

    #[tokio::test]
    async fn three_finger_swipe_up_dispatches_toggle_overview() {
        let niri = MockNiri::new();
        let emitter = MockEmitter::new();
        let config = default_config();
        let gesture = GestureType::Swipe {
            fingers: 3,
            direction: SwipeDirection::Up,
            velocity: 500.0,
        };

        let result = dispatch_gesture(&gesture, &niri, &emitter, &config)
            .await
            .unwrap();
        assert_eq!(result, DispatchResult::Dispatched);
        assert_eq!(niri.recorded(), vec![NiriCall::ToggleOverview]);
    }

    #[tokio::test]
    async fn three_finger_swipe_down_dispatches_show_desktop() {
        let niri = MockNiri::new();
        let emitter = MockEmitter::new();
        let config = default_config();
        let gesture = GestureType::Swipe {
            fingers: 3,
            direction: SwipeDirection::Down,
            velocity: 500.0,
        };

        let result = dispatch_gesture(&gesture, &niri, &emitter, &config)
            .await
            .unwrap();
        assert_eq!(result, DispatchResult::Dispatched);
        assert_eq!(niri.recorded(), vec![NiriCall::ShowDesktop]);
    }

    #[tokio::test]
    async fn disabled_swipe_is_not_dispatched() {
        let niri = MockNiri::new();
        let emitter = MockEmitter::new();
        let mut config = default_config();
        config.gestures.swipe_enabled = false;
        let gesture = GestureType::Swipe {
            fingers: 3,
            direction: SwipeDirection::Left,
            velocity: 500.0,
        };

        let result = dispatch_gesture(&gesture, &niri, &emitter, &config)
            .await
            .unwrap();
        assert_eq!(result, DispatchResult::Passthrough);
        assert!(niri.recorded().is_empty());
    }

    #[tokio::test]
    async fn globally_disabled_gestures_passthrough() {
        let niri = MockNiri::new();
        let emitter = MockEmitter::new();
        let mut config = default_config();
        config.gestures.enabled = false;
        let gesture = GestureType::Swipe {
            fingers: 3,
            direction: SwipeDirection::Left,
            velocity: 500.0,
        };

        let result = dispatch_gesture(&gesture, &niri, &emitter, &config)
            .await
            .unwrap();
        assert_eq!(result, DispatchResult::Passthrough);
        assert!(niri.recorded().is_empty());
    }

    #[tokio::test]
    async fn one_finger_tap_passthrough() {
        let niri = MockNiri::new();
        let emitter = MockEmitter::new();
        let config = default_config();
        let gesture = GestureType::Tap {
            fingers: 1,
            position: (500, 500),
        };

        let result = dispatch_gesture(&gesture, &niri, &emitter, &config)
            .await
            .unwrap();
        assert_eq!(result, DispatchResult::Passthrough);
        assert!(niri.recorded().is_empty());
        assert!(emitter.recorded().is_empty());
    }

    #[tokio::test]
    async fn two_finger_swipe_passthrough() {
        let niri = MockNiri::new();
        let emitter = MockEmitter::new();
        let config = default_config();
        let gesture = GestureType::Swipe {
            fingers: 2,
            direction: SwipeDirection::Up,
            velocity: 300.0,
        };

        let result = dispatch_gesture(&gesture, &niri, &emitter, &config)
            .await
            .unwrap();
        assert_eq!(result, DispatchResult::Passthrough);
        assert!(niri.recorded().is_empty());
    }

    #[tokio::test]
    async fn two_finger_pinch_passthrough() {
        let niri = MockNiri::new();
        let emitter = MockEmitter::new();
        let config = default_config();
        let gesture = GestureType::Pinch {
            fingers: 2,
            scale_delta: 0.8,
        };

        let result = dispatch_gesture(&gesture, &niri, &emitter, &config)
            .await
            .unwrap();
        assert_eq!(result, DispatchResult::Passthrough);
    }

    #[tokio::test]
    async fn three_finger_double_tap_shows_claw() {
        let niri = MockNiri::new();
        let emitter = MockEmitter::new();
        let config = default_config();
        let gesture = GestureType::DoubleTap {
            fingers: 3,
            position: (500, 500),
        };

        let result = dispatch_gesture(&gesture, &niri, &emitter, &config)
            .await
            .unwrap();
        assert_eq!(result, DispatchResult::Dispatched);
        assert_eq!(emitter.recorded(), vec![EmitterCall::ShowClaw]);
    }

    #[tokio::test]
    async fn four_finger_pinch_in_shows_launcher() {
        let niri = MockNiri::new();
        let emitter = MockEmitter::new();
        let config = default_config();
        let gesture = GestureType::Pinch {
            fingers: 4,
            scale_delta: 0.5,
        };

        let result = dispatch_gesture(&gesture, &niri, &emitter, &config)
            .await
            .unwrap();
        assert_eq!(result, DispatchResult::Dispatched);
        assert_eq!(emitter.recorded(), vec![EmitterCall::ShowLauncher]);
    }

    #[tokio::test]
    async fn four_finger_pinch_out_hides_launcher() {
        let niri = MockNiri::new();
        let emitter = MockEmitter::new();
        let config = default_config();
        let gesture = GestureType::Pinch {
            fingers: 4,
            scale_delta: 1.5,
        };

        let result = dispatch_gesture(&gesture, &niri, &emitter, &config)
            .await
            .unwrap();
        assert_eq!(result, DispatchResult::Dispatched);
        assert_eq!(emitter.recorded(), vec![EmitterCall::HideLauncher]);
    }

    #[tokio::test]
    async fn disabled_pinch_is_not_dispatched() {
        let niri = MockNiri::new();
        let emitter = MockEmitter::new();
        let mut config = default_config();
        config.gestures.pinch_enabled = false;
        let gesture = GestureType::Pinch {
            fingers: 4,
            scale_delta: 0.5,
        };

        let result = dispatch_gesture(&gesture, &niri, &emitter, &config)
            .await
            .unwrap();
        assert_eq!(result, DispatchResult::Passthrough);
        assert!(emitter.recorded().is_empty());
    }

    #[tokio::test]
    async fn edge_swipe_bottom_up_shows_dock() {
        let niri = MockNiri::new();
        let emitter = MockEmitter::new();
        let config = default_config();
        let gesture = GestureType::EdgeSwipe {
            edge: Edge::Bottom,
            direction: SwipeDirection::Up,
            velocity: 800.0,
        };

        let result = dispatch_gesture(&gesture, &niri, &emitter, &config)
            .await
            .unwrap();
        assert_eq!(result, DispatchResult::Dispatched);
        assert_eq!(emitter.recorded(), vec![EmitterCall::ShowDock]);
    }

    #[tokio::test]
    async fn edge_swipe_right_to_left_closes_window() {
        let niri = MockNiri::new();
        let emitter = MockEmitter::new();
        let config = default_config();
        let gesture = GestureType::EdgeSwipe {
            edge: Edge::Right,
            direction: SwipeDirection::Left,
            velocity: 600.0,
        };

        let result = dispatch_gesture(&gesture, &niri, &emitter, &config)
            .await
            .unwrap();
        assert_eq!(result, DispatchResult::Dispatched);
        assert_eq!(niri.recorded(), vec![NiriCall::CloseWindow]);
        assert!(emitter.recorded().is_empty());
    }

    #[tokio::test]
    async fn edge_swipe_left_to_right_back_action() {
        let niri = MockNiri::new();
        let emitter = MockEmitter::new();
        let config = default_config();
        let gesture = GestureType::EdgeSwipe {
            edge: Edge::Left,
            direction: SwipeDirection::Right,
            velocity: 600.0,
        };

        let result = dispatch_gesture(&gesture, &niri, &emitter, &config)
            .await
            .unwrap();
        // Back action is dispatched (even though it's a noop internally).
        assert_eq!(result, DispatchResult::Dispatched);
    }

    #[tokio::test]
    async fn edge_swipe_top_down_quick_settings() {
        let niri = MockNiri::new();
        let emitter = MockEmitter::new();
        let config = default_config();
        let gesture = GestureType::EdgeSwipe {
            edge: Edge::Top,
            direction: SwipeDirection::Down,
            velocity: 600.0,
        };

        let result = dispatch_gesture(&gesture, &niri, &emitter, &config)
            .await
            .unwrap();
        assert_eq!(result, DispatchResult::Dispatched);
    }

    #[tokio::test]
    async fn disabled_edge_swipe_is_not_dispatched() {
        let niri = MockNiri::new();
        let emitter = MockEmitter::new();
        let mut config = default_config();
        config.gestures.edge_swipe_enabled = false;
        let gesture = GestureType::EdgeSwipe {
            edge: Edge::Bottom,
            direction: SwipeDirection::Up,
            velocity: 800.0,
        };

        let result = dispatch_gesture(&gesture, &niri, &emitter, &config)
            .await
            .unwrap();
        assert_eq!(result, DispatchResult::Passthrough);
        assert!(emitter.recorded().is_empty());
    }
}
