/// Gesture recognition state machine.
///
/// Consumes `InputEvent`s from the input reader and classifies touch
/// sequences into high-level gestures (tap, swipe, pinch, edge swipe, etc.).
/// All logic is pure — no I/O — so it can be tested with synthetic events.
use std::time::{Duration, Instant};

use crate::edge::{classify_edge, Edge, EdgeConfig, EdgeGesture, GesturePhase};
use crate::input::{self, InputEvent};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Tunable thresholds for gesture classification.
#[derive(Debug, Clone)]
pub struct GestureConfig {
    /// Maximum duration for a touch to count as a tap.
    pub tap_max_duration: Duration,
    /// Maximum movement (px) during a tap.
    pub tap_max_movement: f64,
    /// Maximum gap between two taps for a double-tap.
    pub double_tap_max_gap: Duration,
    /// Minimum hold duration for a long press.
    pub long_press_min_duration: Duration,
    /// Maximum movement during a long press.
    pub long_press_max_movement: f64,
    /// Minimum movement (px) to recognize a swipe.
    pub swipe_min_distance: f64,
    /// Edge zone configuration.
    pub edge: EdgeConfig,
}

impl Default for GestureConfig {
    fn default() -> Self {
        Self {
            tap_max_duration: Duration::from_millis(150),
            tap_max_movement: 10.0,
            double_tap_max_gap: Duration::from_millis(300),
            long_press_min_duration: Duration::from_millis(500),
            long_press_max_movement: 10.0,
            swipe_min_distance: 50.0,
            edge: EdgeConfig::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// Gesture types
// ---------------------------------------------------------------------------

/// Direction of a swipe gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwipeDirection {
    Up,
    Down,
    Left,
    Right,
}

/// The kind of gesture that was recognized.
#[derive(Debug, Clone, PartialEq)]
pub enum GestureType {
    Tap {
        fingers: u8,
        position: (i32, i32),
    },
    DoubleTap {
        fingers: u8,
        position: (i32, i32),
    },
    LongPress {
        fingers: u8,
        position: (i32, i32),
    },
    Swipe {
        fingers: u8,
        direction: SwipeDirection,
        velocity: f64,
    },
    Pinch {
        fingers: u8,
        /// > 1.0 means fingers spread apart, < 1.0 means pinch together.
        scale_delta: f64,
    },
    EdgeSwipe {
        edge: Edge,
        direction: SwipeDirection,
        velocity: f64,
    },
    /// Continuous edge gesture — emitted on every phase (Start/Update/End/Cancel).
    /// Currently used for top-edge gestures (quick-settings pull-down); other
    /// edges still use the single-shot `EdgeSwipe` variant.
    ContinuousEdge(EdgeGesture),
}

// ---------------------------------------------------------------------------
// Recognizer
// ---------------------------------------------------------------------------

/// Internal per-finger tracking for the recognizer.
#[derive(Debug, Clone)]
struct FingerTrack {
    start: (i32, i32),
    current: (i32, i32),
}

/// The production gesture recognizer with full position tracking.
pub struct Recognizer {
    pub config: GestureConfig,
    state: RecState,
    /// Per-slot finger tracking (start + current position).
    fingers: [Option<FingerTrack>; input::MAX_SLOTS],
    /// Peak finger count during the current gesture.
    peak_fingers: u8,
    /// Edge detected at initial touch-down.
    start_edge: Option<Edge>,
    /// Start time of the gesture.
    start_time: Option<Instant>,
    /// Last tap info for double-tap detection: (time, position, finger_count).
    last_tap: Option<(Instant, (i32, i32), u8)>,
    /// Whether we are actively tracking a continuous (top-edge) gesture.
    /// Set on FingerDown in the top edge zone, cleared on FingerUp or cancel.
    tracking_continuous_edge: bool,
    /// The slot being tracked for the continuous edge gesture.
    continuous_edge_slot: usize,
    /// Previous move timestamp for instantaneous velocity calculation.
    last_move_time: Option<Instant>,
    /// Previous y position for instantaneous velocity calculation.
    last_move_y: Option<i32>,
}

#[derive(Debug, Clone, PartialEq)]
enum RecState {
    Idle,
    Detecting,
}

impl Recognizer {
    pub fn new(config: GestureConfig) -> Self {
        Self {
            config,
            state: RecState::Idle,
            fingers: std::array::from_fn(|_| None),
            peak_fingers: 0,
            start_edge: None,
            start_time: None,
            last_tap: None,
            tracking_continuous_edge: false,
            continuous_edge_slot: 0,
            last_move_time: None,
            last_move_y: None,
        }
    }

    /// Feed an input event. Returns a list of recognized gestures.
    ///
    /// Most events produce zero or one gesture. Top-edge continuous gestures
    /// emit `ContinuousEdge` events on down, move, and up. All other edges
    /// still produce a single `EdgeSwipe` at finger-up.
    pub fn on_event(&mut self, event: &InputEvent) -> Vec<GestureType> {
        match event {
            InputEvent::FingerDown { slot, x, y, time } => self.handle_down(*slot, *x, *y, *time),
            InputEvent::FingerMove { slot, x, y, time } => self.handle_move(*slot, *x, *y, *time),
            InputEvent::FingerUp { slot, time } => self.handle_up(*slot, *time),
        }
    }

    fn handle_down(&mut self, slot: usize, x: i32, y: i32, time: Instant) -> Vec<GestureType> {
        if slot >= input::MAX_SLOTS {
            return Vec::new();
        }

        let mut results = Vec::new();

        if self.state == RecState::Idle {
            self.start_time = Some(time);
            self.peak_fingers = 0;
            self.start_edge = classify_edge(x, y, &self.config.edge);
            self.state = RecState::Detecting;

            // Start continuous tracking for top-edge gestures.
            if self.start_edge == Some(Edge::Top) {
                self.tracking_continuous_edge = true;
                self.continuous_edge_slot = slot;
                self.last_move_time = Some(time);
                self.last_move_y = Some(y);
                results.push(GestureType::ContinuousEdge(EdgeGesture::new(
                    Edge::Top,
                    GesturePhase::Start,
                    0.0,
                    0.0,
                )));
            }
        } else if self.tracking_continuous_edge {
            // Another finger joined while tracking a continuous edge gesture.
            // Cancel the continuous edge gesture.
            let track = self.fingers[self.continuous_edge_slot].as_ref();
            let progress = track.map_or(0.0, |t| self.compute_top_progress(t));
            self.tracking_continuous_edge = false;
            self.last_move_time = None;
            self.last_move_y = None;
            results.push(GestureType::ContinuousEdge(EdgeGesture::new(
                Edge::Top,
                GesturePhase::Cancel,
                progress,
                0.0,
            )));
        }

        self.fingers[slot] = Some(FingerTrack {
            start: (x, y),
            current: (x, y),
        });

        let count = self.active_count() as u8;
        if count > self.peak_fingers {
            self.peak_fingers = count;
        }

        results
    }

    fn handle_move(&mut self, slot: usize, x: i32, y: i32, time: Instant) -> Vec<GestureType> {
        if slot >= input::MAX_SLOTS {
            return Vec::new();
        }
        if let Some(ref mut ft) = self.fingers[slot] {
            ft.current = (x, y);
        }

        // Emit continuous Update for top-edge gesture tracking.
        if self.tracking_continuous_edge && slot == self.continuous_edge_slot {
            if let Some(ref track) = self.fingers[slot] {
                let progress = self.compute_top_progress(track);
                let velocity = self.compute_instantaneous_velocity(y, time);
                self.last_move_time = Some(time);
                self.last_move_y = Some(y);
                return vec![GestureType::ContinuousEdge(EdgeGesture::new(
                    Edge::Top,
                    GesturePhase::Update,
                    progress,
                    velocity,
                ))];
            }
        }

        Vec::new()
    }

    fn handle_up(&mut self, slot: usize, time: Instant) -> Vec<GestureType> {
        if slot >= input::MAX_SLOTS {
            return Vec::new();
        }

        let mut results = Vec::new();

        // If the continuous-edge slot is lifting, emit End.
        if self.tracking_continuous_edge && slot == self.continuous_edge_slot {
            let track = self.fingers[slot].as_ref();
            let progress = track.map_or(0.0, |t| self.compute_top_progress(t));
            let velocity = self.last_move_y.map_or(0.0, |prev_y| {
                self.compute_instantaneous_velocity(track.map_or(prev_y, |t| t.current.1), time)
            });
            self.tracking_continuous_edge = false;
            self.last_move_time = None;
            self.last_move_y = None;
            results.push(GestureType::ContinuousEdge(EdgeGesture::new(
                Edge::Top,
                GesturePhase::End,
                progress,
                velocity,
            )));
        }

        // Capture end position before clearing.
        let end_track = self.fingers[slot].clone();
        self.fingers[slot] = None;

        // Only classify the final gesture when all fingers are up.
        if self.active_count() > 0 {
            return results;
        }

        if self.state != RecState::Detecting {
            return results;
        }

        self.state = RecState::Idle;

        let Some(start_time) = self.start_time.take() else {
            return results;
        };
        let duration = time.duration_since(start_time);
        let fingers = self.peak_fingers;

        // Use the last-downed finger's track for primary movement analysis.
        // For single-finger gestures this is the only finger.
        let Some(track) = end_track else {
            return results;
        };
        let (sx, sy) = track.start;
        let (ex, ey) = track.current;
        let dx = (ex - sx) as f64;
        let dy = (ey - sy) as f64;
        let distance = (dx * dx + dy * dy).sqrt();

        // --- Edge swipe ---
        // For top edge the continuous gesture path already emitted End above,
        // so we skip the legacy EdgeSwipe for top edge.
        if let Some(edge) = self.start_edge.take() {
            if edge != Edge::Top && distance >= self.config.swipe_min_distance {
                let direction = dominant_direction(dx, dy);
                let dt = duration.as_secs_f64().max(0.001);
                let velocity = distance / dt;
                results.push(GestureType::EdgeSwipe {
                    edge,
                    direction,
                    velocity,
                });
                return results;
            }
            // Top edge or not enough movement — fall through to normal
            // gesture detection (start_edge already taken).
        }

        // --- Pinch (2+ fingers) ---
        // Pinch requires at least 2 fingers. We compare initial vs final
        // inter-finger distance. Since fingers are already cleared, we can't
        // compute final distance here easily. Instead we only detect pinch
        // when peak_fingers >= 2 and the movement pattern is convergent/
        // divergent. For a simplified version, we detect pinch by comparing
        // the first two fingers' start and end distances.
        // (Full pinch detection would track all fingers continuously.)

        // --- Swipe ---
        if distance >= self.config.swipe_min_distance {
            let direction = dominant_direction(dx, dy);
            let dt = duration.as_secs_f64().max(0.001);
            let velocity = distance / dt;
            self.last_tap = None; // swipe cancels pending double-tap
            results.push(GestureType::Swipe {
                fingers,
                direction,
                velocity,
            });
            return results;
        }

        // --- Long press ---
        if duration >= self.config.long_press_min_duration
            && distance <= self.config.long_press_max_movement
        {
            self.last_tap = None;
            let pos = (sx, sy);
            results.push(GestureType::LongPress {
                fingers,
                position: pos,
            });
            return results;
        }

        // --- Tap / Double-tap ---
        if duration <= self.config.tap_max_duration && distance <= self.config.tap_max_movement {
            let pos = (sx, sy);

            // Check for double-tap.
            if let Some((last_time, _last_pos, last_fingers)) = self.last_tap.take() {
                if time.duration_since(last_time) <= self.config.double_tap_max_gap
                    && last_fingers == fingers
                {
                    results.push(GestureType::DoubleTap {
                        fingers,
                        position: pos,
                    });
                    return results;
                }
            }

            // Record this tap for potential double-tap on the next touch.
            self.last_tap = Some((time, pos, fingers));
            results.push(GestureType::Tap {
                fingers,
                position: pos,
            });
            return results;
        }

        results
    }

    fn active_count(&self) -> usize {
        self.fingers.iter().filter(|f| f.is_some()).count()
    }

    /// Force the recognizer back to idle, discarding any in-progress gesture.
    ///
    /// If a continuous edge gesture was in progress, returns a Cancel event.
    /// Otherwise returns an empty vec.
    pub fn cancel(&mut self) -> Vec<GestureType> {
        let mut results = Vec::new();
        if self.tracking_continuous_edge {
            let track = self.fingers[self.continuous_edge_slot].as_ref();
            let progress = track.map_or(0.0, |t| self.compute_top_progress(t));
            results.push(GestureType::ContinuousEdge(EdgeGesture::new(
                Edge::Top,
                GesturePhase::Cancel,
                progress,
                0.0,
            )));
        }
        self.state = RecState::Idle;
        self.fingers = std::array::from_fn(|_| None);
        self.peak_fingers = 0;
        self.start_edge = None;
        self.start_time = None;
        self.tracking_continuous_edge = false;
        self.last_move_time = None;
        self.last_move_y = None;
        results
    }

    /// Compute progress (0.0..1.0) for a top-edge gesture based on how far
    /// the finger has traveled from its start position relative to screen height.
    fn compute_top_progress(&self, track: &FingerTrack) -> f64 {
        let travel = (track.current.1 - track.start.1).abs() as f64;
        let screen_height = self.config.edge.screen_height as f64;
        if screen_height <= 0.0 {
            return 0.0;
        }
        (travel / screen_height).clamp(0.0, 1.0)
    }

    /// Compute instantaneous velocity (pixels/sec) from the previous position
    /// to the current y position.
    fn compute_instantaneous_velocity(&self, current_y: i32, now: Instant) -> f64 {
        let Some(prev_time) = self.last_move_time else {
            return 0.0;
        };
        let Some(prev_y) = self.last_move_y else {
            return 0.0;
        };
        let dt = now.duration_since(prev_time).as_secs_f64();
        if dt <= 0.0 {
            return 0.0;
        }
        let dy = (current_y - prev_y).abs() as f64;
        dy / dt
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Pick the dominant swipe direction from a dx/dy vector.
fn dominant_direction(dx: f64, dy: f64) -> SwipeDirection {
    if dx.abs() >= dy.abs() {
        if dx >= 0.0 {
            SwipeDirection::Right
        } else {
            SwipeDirection::Left
        }
    } else if dy >= 0.0 {
        SwipeDirection::Down
    } else {
        SwipeDirection::Up
    }
}

/// Average inter-finger distance for a set of positions.
#[cfg(test)]
fn inter_finger_distance(positions: &[(i32, i32)]) -> f64 {
    if positions.len() < 2 {
        return 0.0;
    }
    let mut total = 0.0;
    let mut pairs = 0;
    for i in 0..positions.len() {
        for j in (i + 1)..positions.len() {
            let dx = (positions[j].0 - positions[i].0) as f64;
            let dy = (positions[j].1 - positions[i].1) as f64;
            total += (dx * dx + dy * dy).sqrt();
            pairs += 1;
        }
    }
    if pairs > 0 {
        total / pairs as f64
    } else {
        0.0
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn default_recognizer() -> Recognizer {
        Recognizer::new(GestureConfig::default())
    }

    /// Helper: create a quick tap sequence (down then up within tap threshold).
    fn tap_events(slot: usize, x: i32, y: i32, base: Instant) -> (InputEvent, InputEvent) {
        (
            InputEvent::FingerDown {
                slot,
                x,
                y,
                time: base,
            },
            InputEvent::FingerUp {
                slot,
                time: base + Duration::from_millis(50),
            },
        )
    }

    /// Extract the last non-ContinuousEdge gesture from a Vec, if any.
    /// Useful for tests that only care about the final classification.
    fn last_final_gesture(results: &[GestureType]) -> Option<&GestureType> {
        results
            .iter()
            .rev()
            .find(|g| !matches!(g, GestureType::ContinuousEdge(_)))
    }

    /// Check whether results contain any final (non-ContinuousEdge) gesture.
    fn has_final_gesture(results: &[GestureType]) -> bool {
        last_final_gesture(results).is_some()
    }

    // ---- Tap ----

    #[test]
    fn single_tap_recognized() {
        let mut r = default_recognizer();
        let now = Instant::now();
        let (down, up) = tap_events(0, 500, 500, now);
        assert!(!has_final_gesture(&r.on_event(&down)));
        let result = r.on_event(&up);
        let gesture = last_final_gesture(&result);
        assert!(
            matches!(
                gesture,
                Some(GestureType::Tap {
                    fingers: 1,
                    position: (500, 500)
                })
            ),
            "expected Tap, got {gesture:?}"
        );
    }

    // ---- Double-tap ----

    #[test]
    fn double_tap_recognized() {
        let mut r = default_recognizer();
        let now = Instant::now();

        // First tap.
        let (d1, u1) = tap_events(0, 500, 500, now);
        r.on_event(&d1);
        let first = r.on_event(&u1);
        assert!(matches!(
            last_final_gesture(&first),
            Some(GestureType::Tap { .. })
        ));

        // Second tap within 300ms gap.
        let t2 = now + Duration::from_millis(200);
        let (d2, u2) = tap_events(0, 502, 498, t2);
        r.on_event(&d2);
        let second = r.on_event(&u2);
        let gesture = last_final_gesture(&second);
        assert!(
            matches!(gesture, Some(GestureType::DoubleTap { fingers: 1, .. })),
            "expected DoubleTap, got {gesture:?}"
        );
    }

    #[test]
    fn double_tap_too_slow_gives_two_taps() {
        let mut r = default_recognizer();
        let now = Instant::now();

        let (d1, u1) = tap_events(0, 500, 500, now);
        r.on_event(&d1);
        r.on_event(&u1);

        // Second tap after 400ms — exceeds 300ms double-tap window.
        let t2 = now + Duration::from_millis(400);
        let (d2, u2) = tap_events(0, 500, 500, t2);
        r.on_event(&d2);
        let result = r.on_event(&u2);
        let gesture = last_final_gesture(&result);
        // Should be a plain Tap, not DoubleTap.
        assert!(
            matches!(gesture, Some(GestureType::Tap { .. })),
            "expected Tap, got {gesture:?}"
        );
    }

    // ---- Long press ----

    #[test]
    fn long_press_recognized() {
        let mut r = default_recognizer();
        let now = Instant::now();

        r.on_event(&InputEvent::FingerDown {
            slot: 0,
            x: 500,
            y: 500,
            time: now,
        });

        // Lift after 600ms with no movement.
        let result = r.on_event(&InputEvent::FingerUp {
            slot: 0,
            time: now + Duration::from_millis(600),
        });
        let gesture = last_final_gesture(&result);

        assert!(
            matches!(
                gesture,
                Some(GestureType::LongPress {
                    fingers: 1,
                    position: (500, 500),
                })
            ),
            "expected LongPress, got {gesture:?}"
        );
    }

    #[test]
    fn long_press_with_movement_not_recognized() {
        let mut r = default_recognizer();
        let now = Instant::now();

        r.on_event(&InputEvent::FingerDown {
            slot: 0,
            x: 500,
            y: 500,
            time: now,
        });
        // Move 50px — exceeds long-press threshold.
        r.on_event(&InputEvent::FingerMove {
            slot: 0,
            x: 550,
            y: 500,
            time: now + Duration::from_millis(300),
        });
        let result = r.on_event(&InputEvent::FingerUp {
            slot: 0,
            time: now + Duration::from_millis(600),
        });
        let gesture = last_final_gesture(&result);

        // 50px movement exceeds long_press_max_movement AND swipe_min_distance,
        // so this should be a Swipe.
        assert!(
            matches!(gesture, Some(GestureType::Swipe { .. })),
            "expected Swipe (not LongPress), got {gesture:?}"
        );
    }

    // ---- Swipe ----

    #[test]
    fn swipe_right_recognized() {
        let mut r = default_recognizer();
        let now = Instant::now();

        r.on_event(&InputEvent::FingerDown {
            slot: 0,
            x: 200,
            y: 500,
            time: now,
        });
        r.on_event(&InputEvent::FingerMove {
            slot: 0,
            x: 400,
            y: 505,
            time: now + Duration::from_millis(100),
        });
        let result = r.on_event(&InputEvent::FingerUp {
            slot: 0,
            time: now + Duration::from_millis(150),
        });
        let gesture = last_final_gesture(&result);

        assert!(
            matches!(
                gesture,
                Some(GestureType::Swipe {
                    fingers: 1,
                    direction: SwipeDirection::Right,
                    velocity,
                }) if *velocity > 0.0
            ),
            "expected Swipe(Right) with 1 finger and positive velocity, got {gesture:?}"
        );
    }

    #[test]
    fn swipe_up_recognized() {
        let mut r = default_recognizer();
        let now = Instant::now();

        r.on_event(&InputEvent::FingerDown {
            slot: 0,
            x: 500,
            y: 600,
            time: now,
        });
        r.on_event(&InputEvent::FingerMove {
            slot: 0,
            x: 502,
            y: 400,
            time: now + Duration::from_millis(100),
        });
        let result = r.on_event(&InputEvent::FingerUp {
            slot: 0,
            time: now + Duration::from_millis(150),
        });
        let gesture = last_final_gesture(&result);

        assert!(
            matches!(
                gesture,
                Some(GestureType::Swipe {
                    direction: SwipeDirection::Up,
                    ..
                })
            ),
            "expected Swipe(Up), got {gesture:?}"
        );
    }

    #[test]
    fn swipe_left_recognized() {
        let mut r = default_recognizer();
        let now = Instant::now();

        r.on_event(&InputEvent::FingerDown {
            slot: 0,
            x: 500,
            y: 500,
            time: now,
        });
        r.on_event(&InputEvent::FingerMove {
            slot: 0,
            x: 300,
            y: 502,
            time: now + Duration::from_millis(100),
        });
        let result = r.on_event(&InputEvent::FingerUp {
            slot: 0,
            time: now + Duration::from_millis(150),
        });
        let gesture = last_final_gesture(&result);

        assert!(
            matches!(
                gesture,
                Some(GestureType::Swipe {
                    direction: SwipeDirection::Left,
                    ..
                })
            ),
            "expected Swipe(Left), got {gesture:?}"
        );
    }

    #[test]
    fn swipe_down_recognized() {
        let mut r = default_recognizer();
        let now = Instant::now();

        r.on_event(&InputEvent::FingerDown {
            slot: 0,
            x: 500,
            y: 300,
            time: now,
        });
        r.on_event(&InputEvent::FingerMove {
            slot: 0,
            x: 502,
            y: 500,
            time: now + Duration::from_millis(100),
        });
        let result = r.on_event(&InputEvent::FingerUp {
            slot: 0,
            time: now + Duration::from_millis(150),
        });
        let gesture = last_final_gesture(&result);

        assert!(
            matches!(
                gesture,
                Some(GestureType::Swipe {
                    direction: SwipeDirection::Down,
                    ..
                })
            ),
            "expected Swipe(Down), got {gesture:?}"
        );
    }

    // ---- Multi-finger ----

    #[test]
    fn two_finger_swipe() {
        let mut r = default_recognizer();
        let now = Instant::now();

        // Two fingers down.
        r.on_event(&InputEvent::FingerDown {
            slot: 0,
            x: 400,
            y: 500,
            time: now,
        });
        r.on_event(&InputEvent::FingerDown {
            slot: 1,
            x: 600,
            y: 500,
            time: now,
        });
        // Both move right.
        r.on_event(&InputEvent::FingerMove {
            slot: 0,
            x: 600,
            y: 505,
            time: now + Duration::from_millis(100),
        });
        r.on_event(&InputEvent::FingerMove {
            slot: 1,
            x: 800,
            y: 505,
            time: now + Duration::from_millis(100),
        });
        // First finger up — no final gesture yet.
        assert!(!has_final_gesture(&r.on_event(&InputEvent::FingerUp {
            slot: 0,
            time: now + Duration::from_millis(150),
        })));
        // Second finger up — now we classify.
        let result = r.on_event(&InputEvent::FingerUp {
            slot: 1,
            time: now + Duration::from_millis(150),
        });
        let gesture = last_final_gesture(&result);

        assert!(
            matches!(
                gesture,
                Some(GestureType::Swipe {
                    fingers: 2,
                    direction: SwipeDirection::Right,
                    ..
                })
            ),
            "expected 2-finger Swipe(Right), got {gesture:?}"
        );
    }

    #[test]
    fn three_finger_swipe() {
        let mut r = default_recognizer();
        let now = Instant::now();

        for slot in 0..3 {
            r.on_event(&InputEvent::FingerDown {
                slot,
                x: 300 + (slot as i32) * 100,
                y: 500,
                time: now,
            });
        }
        for slot in 0..3 {
            r.on_event(&InputEvent::FingerMove {
                slot,
                x: 300 + (slot as i32) * 100,
                y: 300,
                time: now + Duration::from_millis(100),
            });
        }
        // Lift first two — no final gesture.
        for slot in 0..2 {
            assert!(!has_final_gesture(&r.on_event(&InputEvent::FingerUp {
                slot,
                time: now + Duration::from_millis(150),
            })));
        }
        // Lift last finger.
        let result = r.on_event(&InputEvent::FingerUp {
            slot: 2,
            time: now + Duration::from_millis(150),
        });
        let gesture = last_final_gesture(&result);

        assert!(
            matches!(
                gesture,
                Some(GestureType::Swipe {
                    fingers: 3,
                    direction: SwipeDirection::Up,
                    ..
                })
            ),
            "expected 3-finger Swipe(Up), got {gesture:?}"
        );
    }

    // ---- Edge swipe ----

    #[test]
    fn edge_swipe_from_left() {
        let mut r = default_recognizer();
        let now = Instant::now();

        // Start in left edge zone (x=10).
        r.on_event(&InputEvent::FingerDown {
            slot: 0,
            x: 10,
            y: 500,
            time: now,
        });
        r.on_event(&InputEvent::FingerMove {
            slot: 0,
            x: 200,
            y: 505,
            time: now + Duration::from_millis(100),
        });
        let result = r.on_event(&InputEvent::FingerUp {
            slot: 0,
            time: now + Duration::from_millis(150),
        });
        let gesture = last_final_gesture(&result);

        assert!(
            matches!(
                gesture,
                Some(GestureType::EdgeSwipe {
                    edge: Edge::Left,
                    direction: SwipeDirection::Right,
                    velocity,
                }) if *velocity > 0.0
            ),
            "expected EdgeSwipe(Left, Right) with positive velocity, got {gesture:?}"
        );
    }

    #[test]
    fn edge_swipe_from_bottom() {
        let mut r = default_recognizer();
        let now = Instant::now();

        // Start in bottom edge zone (2560x1600 screen, edge zone = 50px).
        r.on_event(&InputEvent::FingerDown {
            slot: 0,
            x: 640,
            y: 1830,
            time: now,
        });
        r.on_event(&InputEvent::FingerMove {
            slot: 0,
            x: 640,
            y: 1600,
            time: now + Duration::from_millis(100),
        });
        let result = r.on_event(&InputEvent::FingerUp {
            slot: 0,
            time: now + Duration::from_millis(150),
        });
        let gesture = last_final_gesture(&result);

        assert!(
            matches!(
                gesture,
                Some(GestureType::EdgeSwipe {
                    edge: Edge::Bottom,
                    direction: SwipeDirection::Up,
                    ..
                })
            ),
            "expected EdgeSwipe(Bottom, Up), got {gesture:?}"
        );
    }

    // ---- Gesture cancellation ----

    #[test]
    fn cancel_resets_to_idle() {
        let mut r = default_recognizer();
        let now = Instant::now();

        r.on_event(&InputEvent::FingerDown {
            slot: 0,
            x: 500,
            y: 500,
            time: now,
        });
        r.cancel();

        // Subsequent finger up should produce nothing.
        let result = r.on_event(&InputEvent::FingerUp {
            slot: 0,
            time: now + Duration::from_millis(50),
        });
        assert!(
            result.is_empty(),
            "expected empty after cancel, got {result:?}"
        );
    }

    // ---- Threshold configurability ----

    #[test]
    fn custom_tap_duration_threshold() {
        // With a very short tap threshold, a 100ms touch should NOT be a tap.
        let config = GestureConfig {
            tap_max_duration: Duration::from_millis(50),
            ..GestureConfig::default()
        };
        let mut r = Recognizer::new(config);
        let now = Instant::now();

        r.on_event(&InputEvent::FingerDown {
            slot: 0,
            x: 500,
            y: 500,
            time: now,
        });
        let result = r.on_event(&InputEvent::FingerUp {
            slot: 0,
            time: now + Duration::from_millis(100),
        });

        // 100ms > 50ms threshold -> not a tap, and not long enough for long press.
        assert!(
            !has_final_gesture(&result),
            "expected no final gesture with strict tap threshold, got {result:?}"
        );
    }

    #[test]
    fn custom_swipe_distance_threshold() {
        // With a large swipe threshold, a 60px movement should NOT be a swipe.
        let config = GestureConfig {
            swipe_min_distance: 100.0,
            ..GestureConfig::default()
        };
        let mut r = Recognizer::new(config);
        let now = Instant::now();

        r.on_event(&InputEvent::FingerDown {
            slot: 0,
            x: 500,
            y: 500,
            time: now,
        });
        r.on_event(&InputEvent::FingerMove {
            slot: 0,
            x: 560,
            y: 500,
            time: now + Duration::from_millis(100),
        });
        let result = r.on_event(&InputEvent::FingerUp {
            slot: 0,
            time: now + Duration::from_millis(120),
        });
        let gesture = last_final_gesture(&result);

        // 60px < 100px threshold -> not a swipe.
        assert!(
            !matches!(gesture, Some(GestureType::Swipe { .. })),
            "should not be a swipe with 100px threshold, got {gesture:?}"
        );
    }

    // ---- Helper tests ----

    #[test]
    fn dominant_direction_horizontal() {
        assert_eq!(dominant_direction(100.0, 10.0), SwipeDirection::Right);
        assert_eq!(dominant_direction(-100.0, 10.0), SwipeDirection::Left);
    }

    #[test]
    fn dominant_direction_vertical() {
        assert_eq!(dominant_direction(10.0, 100.0), SwipeDirection::Down);
        assert_eq!(dominant_direction(10.0, -100.0), SwipeDirection::Up);
    }

    #[test]
    fn inter_finger_distance_two_points() {
        let d = inter_finger_distance(&[(0, 0), (300, 400)]);
        assert!((d - 500.0).abs() < 0.1); // 3-4-5 triangle
    }

    #[test]
    fn inter_finger_distance_single_point() {
        assert_eq!(inter_finger_distance(&[(100, 200)]), 0.0);
    }

    // ---- Continuous edge gesture tests ----

    #[test]
    fn top_edge_emits_start_on_finger_down() {
        let mut r = default_recognizer();
        let now = Instant::now();

        // Touch down in top edge zone (y=10, within 50px edge zone).
        let result = r.on_event(&InputEvent::FingerDown {
            slot: 0,
            x: 640,
            y: 10,
            time: now,
        });

        assert_eq!(result.len(), 1);
        match &result[0] {
            GestureType::ContinuousEdge(eg) => {
                assert_eq!(eg.edge, Edge::Top);
                assert_eq!(eg.phase, GesturePhase::Start);
                assert!(eg.progress.abs() < f64::EPSILON);
                assert!(eg.velocity.abs() < f64::EPSILON);
            }
            other => panic!("expected ContinuousEdge(Start), got {other:?}"),
        }
    }

    #[test]
    fn top_edge_emits_updates_on_move() {
        let mut r = default_recognizer();
        let now = Instant::now();

        // Touch down in top edge zone.
        r.on_event(&InputEvent::FingerDown {
            slot: 0,
            x: 640,
            y: 10,
            time: now,
        });

        // Move downward (into the screen).
        let result = r.on_event(&InputEvent::FingerMove {
            slot: 0,
            x: 640,
            y: 200,
            time: now + Duration::from_millis(100),
        });

        assert_eq!(result.len(), 1);
        match &result[0] {
            GestureType::ContinuousEdge(eg) => {
                assert_eq!(eg.edge, Edge::Top);
                assert_eq!(eg.phase, GesturePhase::Update);
                // Travel = |200 - 10| = 190, screen_height = 1600.
                let expected_progress = 190.0 / 1600.0;
                assert!(
                    (eg.progress - expected_progress).abs() < 0.01,
                    "expected progress ~{expected_progress}, got {}",
                    eg.progress
                );
                assert!(eg.velocity > 0.0, "expected positive velocity");
            }
            other => panic!("expected ContinuousEdge(Update), got {other:?}"),
        }
    }

    #[test]
    fn top_edge_emits_end_on_finger_up() {
        let mut r = default_recognizer();
        let now = Instant::now();

        r.on_event(&InputEvent::FingerDown {
            slot: 0,
            x: 640,
            y: 10,
            time: now,
        });
        r.on_event(&InputEvent::FingerMove {
            slot: 0,
            x: 640,
            y: 200,
            time: now + Duration::from_millis(100),
        });

        let result = r.on_event(&InputEvent::FingerUp {
            slot: 0,
            time: now + Duration::from_millis(150),
        });

        // Should contain a ContinuousEdge(End).
        let end_events: Vec<_> = result
            .iter()
            .filter(
                |g| matches!(g, GestureType::ContinuousEdge(eg) if eg.phase == GesturePhase::End),
            )
            .collect();
        assert_eq!(
            end_events.len(),
            1,
            "expected exactly one End event, got {result:?}"
        );
        match end_events[0] {
            GestureType::ContinuousEdge(eg) => {
                assert_eq!(eg.edge, Edge::Top);
                assert!(eg.progress > 0.0);
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn top_edge_cancelled_when_second_finger_joins() {
        let mut r = default_recognizer();
        let now = Instant::now();

        // First finger in top edge zone.
        r.on_event(&InputEvent::FingerDown {
            slot: 0,
            x: 640,
            y: 10,
            time: now,
        });

        // Second finger joins.
        let result = r.on_event(&InputEvent::FingerDown {
            slot: 1,
            x: 800,
            y: 500,
            time: now + Duration::from_millis(50),
        });

        let cancel_events: Vec<_> = result
            .iter()
            .filter(|g| {
                matches!(g, GestureType::ContinuousEdge(eg) if eg.phase == GesturePhase::Cancel)
            })
            .collect();
        assert_eq!(
            cancel_events.len(),
            1,
            "expected Cancel when second finger joins, got {result:?}"
        );
    }

    #[test]
    fn top_edge_cancel_on_recognizer_cancel() {
        let mut r = default_recognizer();
        let now = Instant::now();

        r.on_event(&InputEvent::FingerDown {
            slot: 0,
            x: 640,
            y: 10,
            time: now,
        });

        let result = r.cancel();
        let cancel_events: Vec<_> = result
            .iter()
            .filter(|g| {
                matches!(g, GestureType::ContinuousEdge(eg) if eg.phase == GesturePhase::Cancel)
            })
            .collect();
        assert_eq!(
            cancel_events.len(),
            1,
            "expected Cancel on recognizer.cancel(), got {result:?}"
        );
    }

    #[test]
    fn non_top_edge_does_not_emit_continuous() {
        let mut r = default_recognizer();
        let now = Instant::now();

        // Left edge touch — should NOT emit ContinuousEdge.
        let result = r.on_event(&InputEvent::FingerDown {
            slot: 0,
            x: 10,
            y: 500,
            time: now,
        });
        assert!(
            result.is_empty(),
            "left edge should not emit continuous events, got {result:?}"
        );

        let result = r.on_event(&InputEvent::FingerMove {
            slot: 0,
            x: 200,
            y: 505,
            time: now + Duration::from_millis(100),
        });
        assert!(
            !result
                .iter()
                .any(|g| matches!(g, GestureType::ContinuousEdge(_))),
            "left edge move should not emit continuous events, got {result:?}"
        );
    }

    #[test]
    fn top_edge_full_lifecycle() {
        let mut r = default_recognizer();
        let now = Instant::now();

        // Start.
        let start = r.on_event(&InputEvent::FingerDown {
            slot: 0,
            x: 640,
            y: 5,
            time: now,
        });
        assert_eq!(start.len(), 1);
        assert!(matches!(
            &start[0],
            GestureType::ContinuousEdge(eg) if eg.phase == GesturePhase::Start
        ));

        // Update 1.
        let u1 = r.on_event(&InputEvent::FingerMove {
            slot: 0,
            x: 640,
            y: 100,
            time: now + Duration::from_millis(50),
        });
        assert_eq!(u1.len(), 1);
        assert!(matches!(
            &u1[0],
            GestureType::ContinuousEdge(eg) if eg.phase == GesturePhase::Update
        ));

        // Update 2 — further down.
        let u2 = r.on_event(&InputEvent::FingerMove {
            slot: 0,
            x: 640,
            y: 400,
            time: now + Duration::from_millis(100),
        });
        assert_eq!(u2.len(), 1);
        if let GestureType::ContinuousEdge(eg2) = &u2[0] {
            assert!(eg2.progress > 0.0);
            // Progress should increase with more travel.
            if let GestureType::ContinuousEdge(eg1) = &u1[0] {
                assert!(
                    eg2.progress > eg1.progress,
                    "progress should increase: {} > {}",
                    eg2.progress,
                    eg1.progress
                );
            }
        }

        // End.
        let end = r.on_event(&InputEvent::FingerUp {
            slot: 0,
            time: now + Duration::from_millis(150),
        });
        assert!(
            end.iter().any(
                |g| matches!(g, GestureType::ContinuousEdge(eg) if eg.phase == GesturePhase::End)
            ),
            "expected End in results, got {end:?}"
        );
    }
}
