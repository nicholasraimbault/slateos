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
    #[allow(dead_code)]
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
    #[allow(dead_code)]
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

#[cfg(test)]
#[path = "gesture_tests.rs"]
mod tests;
