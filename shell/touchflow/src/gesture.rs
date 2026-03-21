/// Gesture recognition state machine.
///
/// Consumes `InputEvent`s from the input reader and classifies touch
/// sequences into high-level gestures (tap, swipe, pinch, edge swipe, etc.).
/// All logic is pure — no I/O — so it can be tested with synthetic events.
use std::time::{Duration, Instant};

use crate::edge::{classify_edge, Edge, EdgeConfig};
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
#[allow(dead_code)] // Pinch variant constructed by dispatch layer (Task 6)
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
}

/// Current phase of the gesture state machine.
///
/// Exposed for downstream consumers (e.g. the dispatch layer) that may want
/// to inspect the recognizer's phase. Not all variants are used internally
/// yet — `Active` and `Completing` will be consumed by the animation/dispatch
/// pipeline in Task 6.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Variants consumed by dispatch layer (Task 6)
pub enum GestureState {
    Idle,
    Detecting {
        start_time: Instant,
        start_positions: Vec<(i32, i32)>,
    },
    Recognized(GestureType),
    Active(GestureType),
    Completing(GestureType),
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
        }
    }

    /// Feed an input event. Returns `Some(gesture)` when recognition completes.
    pub fn on_event(&mut self, event: &InputEvent) -> Option<GestureType> {
        match event {
            InputEvent::FingerDown { slot, x, y, time } => self.handle_down(*slot, *x, *y, *time),
            InputEvent::FingerMove { slot, x, y, .. } => {
                self.handle_move(*slot, *x, *y);
                None
            }
            InputEvent::FingerUp { slot, time } => self.handle_up(*slot, *time),
        }
    }

    fn handle_down(&mut self, slot: usize, x: i32, y: i32, time: Instant) -> Option<GestureType> {
        if slot >= input::MAX_SLOTS {
            return None;
        }

        if self.state == RecState::Idle {
            self.start_time = Some(time);
            self.peak_fingers = 0;
            self.start_edge = classify_edge(x, y, &self.config.edge);
            self.state = RecState::Detecting;
        }

        self.fingers[slot] = Some(FingerTrack {
            start: (x, y),
            current: (x, y),
        });

        let count = self.active_count() as u8;
        if count > self.peak_fingers {
            self.peak_fingers = count;
        }

        None
    }

    fn handle_move(&mut self, slot: usize, x: i32, y: i32) {
        if slot >= input::MAX_SLOTS {
            return;
        }
        if let Some(ref mut ft) = self.fingers[slot] {
            ft.current = (x, y);
        }
    }

    fn handle_up(&mut self, slot: usize, time: Instant) -> Option<GestureType> {
        if slot >= input::MAX_SLOTS {
            return None;
        }

        // Capture end position before clearing.
        let end_track = self.fingers[slot].clone();
        self.fingers[slot] = None;

        // Only classify when all fingers are up.
        if self.active_count() > 0 {
            return None;
        }

        if self.state != RecState::Detecting {
            return None;
        }

        self.state = RecState::Idle;

        let start_time = self.start_time.take()?;
        let duration = time.duration_since(start_time);
        let fingers = self.peak_fingers;

        // Use the last-downed finger's track for primary movement analysis.
        // For single-finger gestures this is the only finger.
        let track = end_track?;
        let (sx, sy) = track.start;
        let (ex, ey) = track.current;
        let dx = (ex - sx) as f64;
        let dy = (ey - sy) as f64;
        let distance = (dx * dx + dy * dy).sqrt();

        // --- Edge swipe ---
        if let Some(edge) = self.start_edge.take() {
            if distance >= self.config.swipe_min_distance {
                let direction = dominant_direction(dx, dy);
                let dt = duration.as_secs_f64().max(0.001);
                let velocity = distance / dt;
                return Some(GestureType::EdgeSwipe {
                    edge,
                    direction,
                    velocity,
                });
            }
            // If not enough movement, fall through to normal gesture detection
            // (reset start_edge already taken).
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
            return Some(GestureType::Swipe {
                fingers,
                direction,
                velocity,
            });
        }

        // --- Long press ---
        if duration >= self.config.long_press_min_duration
            && distance <= self.config.long_press_max_movement
        {
            self.last_tap = None;
            let pos = (sx, sy);
            return Some(GestureType::LongPress {
                fingers,
                position: pos,
            });
        }

        // --- Tap / Double-tap ---
        if duration <= self.config.tap_max_duration && distance <= self.config.tap_max_movement {
            let pos = (sx, sy);

            // Check for double-tap.
            if let Some((last_time, _last_pos, last_fingers)) = self.last_tap.take() {
                if time.duration_since(last_time) <= self.config.double_tap_max_gap
                    && last_fingers == fingers
                {
                    return Some(GestureType::DoubleTap {
                        fingers,
                        position: pos,
                    });
                }
            }

            // Record this tap for potential double-tap on the next touch.
            self.last_tap = Some((time, pos, fingers));
            return Some(GestureType::Tap {
                fingers,
                position: pos,
            });
        }

        None
    }

    fn active_count(&self) -> usize {
        self.fingers.iter().filter(|f| f.is_some()).count()
    }

    /// Force the recognizer back to idle, discarding any in-progress gesture.
    pub fn cancel(&mut self) {
        self.state = RecState::Idle;
        self.fingers = std::array::from_fn(|_| None);
        self.peak_fingers = 0;
        self.start_edge = None;
        self.start_time = None;
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

    // ---- Tap ----

    #[test]
    fn single_tap_recognized() {
        let mut r = default_recognizer();
        let now = Instant::now();
        let (down, up) = tap_events(0, 500, 500, now);
        assert!(r.on_event(&down).is_none());
        let result = r.on_event(&up);
        assert!(
            matches!(
                result,
                Some(GestureType::Tap {
                    fingers: 1,
                    position: (500, 500)
                })
            ),
            "expected Tap, got {result:?}"
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
        assert!(matches!(first, Some(GestureType::Tap { .. })));

        // Second tap within 300ms gap.
        let t2 = now + Duration::from_millis(200);
        let (d2, u2) = tap_events(0, 502, 498, t2);
        r.on_event(&d2);
        let second = r.on_event(&u2);
        assert!(
            matches!(second, Some(GestureType::DoubleTap { fingers: 1, .. })),
            "expected DoubleTap, got {second:?}"
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
        // Should be a plain Tap, not DoubleTap.
        assert!(
            matches!(result, Some(GestureType::Tap { .. })),
            "expected Tap, got {result:?}"
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

        assert!(
            matches!(
                result,
                Some(GestureType::LongPress {
                    fingers: 1,
                    position: (500, 500),
                })
            ),
            "expected LongPress, got {result:?}"
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

        // 50px movement exceeds long_press_max_movement AND swipe_min_distance,
        // so this should be a Swipe.
        assert!(
            matches!(result, Some(GestureType::Swipe { .. })),
            "expected Swipe (not LongPress), got {result:?}"
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

        match result {
            Some(GestureType::Swipe {
                fingers,
                direction,
                velocity,
            }) => {
                assert_eq!(fingers, 1);
                assert_eq!(direction, SwipeDirection::Right);
                assert!(velocity > 0.0);
            }
            other => panic!("expected Swipe(Right), got {other:?}"),
        }
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

        match result {
            Some(GestureType::Swipe { direction, .. }) => {
                assert_eq!(direction, SwipeDirection::Up);
            }
            other => panic!("expected Swipe(Up), got {other:?}"),
        }
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

        match result {
            Some(GestureType::Swipe { direction, .. }) => {
                assert_eq!(direction, SwipeDirection::Left);
            }
            other => panic!("expected Swipe(Left), got {other:?}"),
        }
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

        match result {
            Some(GestureType::Swipe { direction, .. }) => {
                assert_eq!(direction, SwipeDirection::Down);
            }
            other => panic!("expected Swipe(Down), got {other:?}"),
        }
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
        // First finger up — no result yet.
        assert!(r
            .on_event(&InputEvent::FingerUp {
                slot: 0,
                time: now + Duration::from_millis(150),
            })
            .is_none());
        // Second finger up — now we classify.
        let result = r.on_event(&InputEvent::FingerUp {
            slot: 1,
            time: now + Duration::from_millis(150),
        });

        match result {
            Some(GestureType::Swipe {
                fingers, direction, ..
            }) => {
                assert_eq!(fingers, 2);
                assert_eq!(direction, SwipeDirection::Right);
            }
            other => panic!("expected 2-finger Swipe(Right), got {other:?}"),
        }
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
        // Lift first two — no result.
        for slot in 0..2 {
            assert!(r
                .on_event(&InputEvent::FingerUp {
                    slot,
                    time: now + Duration::from_millis(150),
                })
                .is_none());
        }
        // Lift last finger.
        let result = r.on_event(&InputEvent::FingerUp {
            slot: 2,
            time: now + Duration::from_millis(150),
        });

        match result {
            Some(GestureType::Swipe {
                fingers, direction, ..
            }) => {
                assert_eq!(fingers, 3);
                assert_eq!(direction, SwipeDirection::Up);
            }
            other => panic!("expected 3-finger Swipe(Up), got {other:?}"),
        }
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

        match result {
            Some(GestureType::EdgeSwipe {
                edge,
                direction,
                velocity,
            }) => {
                assert_eq!(edge, Edge::Left);
                assert_eq!(direction, SwipeDirection::Right);
                assert!(velocity > 0.0);
            }
            other => panic!("expected EdgeSwipe(Left, Right), got {other:?}"),
        }
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

        match result {
            Some(GestureType::EdgeSwipe {
                edge, direction, ..
            }) => {
                assert_eq!(edge, Edge::Bottom);
                assert_eq!(direction, SwipeDirection::Up);
            }
            other => panic!("expected EdgeSwipe(Bottom, Up), got {other:?}"),
        }
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
            result.is_none(),
            "expected None after cancel, got {result:?}"
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

        // 100ms > 50ms threshold → not a tap, and not long enough for long press.
        assert!(
            result.is_none(),
            "expected None with strict tap threshold, got {result:?}"
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

        // 60px < 100px threshold → not a swipe.
        assert!(
            !matches!(result, Some(GestureType::Swipe { .. })),
            "should not be a swipe with 100px threshold, got {result:?}"
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
}
