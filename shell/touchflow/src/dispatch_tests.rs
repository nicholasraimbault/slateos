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

// ---- ContinuousEdge dispatch ----

#[tokio::test]
async fn continuous_edge_update_emits_edge_gesture_signal() {
    use crate::edge::{EdgeGesture, GesturePhase};

    let niri = MockNiri::new();
    let emitter = MockEmitter::new();
    let config = default_config();
    let gesture = GestureType::ContinuousEdge(EdgeGesture::new(
        Edge::Top,
        GesturePhase::Update,
        0.4,
        300.0,
    ));

    let result = dispatch_gesture(&gesture, &niri, &emitter, &config)
        .await
        .unwrap();
    assert_eq!(result, DispatchResult::Dispatched);

    let recorded = emitter.recorded();
    assert_eq!(recorded.len(), 1);
    assert!(matches!(
        &recorded[0],
        EmitterCall::EdgeGesture { edge, phase, .. }
            if edge == "top" && phase == "update"
    ));
    assert!(niri.recorded().is_empty());
}

#[tokio::test]
async fn continuous_edge_start_emits_edge_gesture_signal() {
    use crate::edge::{EdgeGesture, GesturePhase};

    let niri = MockNiri::new();
    let emitter = MockEmitter::new();
    let config = default_config();
    let gesture =
        GestureType::ContinuousEdge(EdgeGesture::new(Edge::Top, GesturePhase::Start, 0.0, 0.0));

    let result = dispatch_gesture(&gesture, &niri, &emitter, &config)
        .await
        .unwrap();
    assert_eq!(result, DispatchResult::Dispatched);

    let recorded = emitter.recorded();
    assert_eq!(recorded.len(), 1);
    assert!(matches!(
        &recorded[0],
        EmitterCall::EdgeGesture { edge, phase, .. }
            if edge == "top" && phase == "start"
    ));
}

#[tokio::test]
async fn continuous_edge_end_emits_edge_gesture_signal() {
    use crate::edge::{EdgeGesture, GesturePhase};

    let niri = MockNiri::new();
    let emitter = MockEmitter::new();
    let config = default_config();
    let gesture =
        GestureType::ContinuousEdge(EdgeGesture::new(Edge::Top, GesturePhase::End, 0.8, 150.0));

    let result = dispatch_gesture(&gesture, &niri, &emitter, &config)
        .await
        .unwrap();
    assert_eq!(result, DispatchResult::Dispatched);

    let recorded = emitter.recorded();
    assert_eq!(recorded.len(), 1);
    assert!(matches!(
        &recorded[0],
        EmitterCall::EdgeGesture { edge, phase, .. }
            if edge == "top" && phase == "end"
    ));
}

#[tokio::test]
async fn continuous_edge_disabled_when_edge_swipe_off() {
    use crate::edge::{EdgeGesture, GesturePhase};

    let niri = MockNiri::new();
    let emitter = MockEmitter::new();
    let mut config = default_config();
    config.gestures.edge_swipe_enabled = false;
    let gesture = GestureType::ContinuousEdge(EdgeGesture::new(
        Edge::Top,
        GesturePhase::Update,
        0.5,
        200.0,
    ));

    let result = dispatch_gesture(&gesture, &niri, &emitter, &config)
        .await
        .unwrap();
    assert_eq!(result, DispatchResult::Passthrough);
    assert!(emitter.recorded().is_empty());
}
