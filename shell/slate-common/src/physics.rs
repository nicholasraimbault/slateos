/// Physics simulation for gesture and UI animations.
///
/// Provides momentum tracking (for flick/fling velocity calculation) and
/// spring dynamics (for snap-back and commit animations). These are pure
/// math — no async, no I/O — so they're fully testable on any platform.
///
/// Originally lived in touchflow; moved here so that iced apps (shoal,
/// slate-launcher, etc.) can reuse the same spring and momentum primitives
/// without depending on touchflow.
use std::collections::VecDeque;
use std::time::Instant;

// ---------------------------------------------------------------------------
// Momentum tracker
// ---------------------------------------------------------------------------

/// Tracks recent touch positions to compute release velocity.
///
/// Feed it `(time, x, y)` samples during a drag. When the finger lifts,
/// call `velocity_at_release()` to get a velocity vector in pixels/second.
#[derive(Debug)]
pub struct MomentumTracker {
    history: VecDeque<(Instant, f64, f64)>,
    /// Maximum number of samples retained. Older samples are discarded.
    max_samples: usize,
}

impl MomentumTracker {
    pub fn new(max_samples: usize) -> Self {
        Self {
            history: VecDeque::with_capacity(max_samples),
            max_samples,
        }
    }

    /// Record a position sample.
    pub fn push(&mut self, time: Instant, x: f64, y: f64) {
        if self.history.len() == self.max_samples {
            self.history.pop_front();
        }
        self.history.push_back((time, x, y));
    }

    /// Clear all recorded samples.
    pub fn reset(&mut self) {
        self.history.clear();
    }

    /// Compute the velocity (px/sec) at the moment the finger was released.
    ///
    /// Uses the first and last samples in the window. Returns `(0.0, 0.0)` if
    /// there are fewer than two samples or the time delta is zero.
    pub fn velocity_at_release(&self) -> (f64, f64) {
        // Need at least two samples to compute a finite difference.
        let (Some(first), Some(last)) = (self.history.front(), self.history.back()) else {
            return (0.0, 0.0);
        };
        let (t0, x0, y0) = first;
        let (t1, x1, y1) = last;
        let dt = t1.duration_since(*t0).as_secs_f64();
        if dt <= 0.0 {
            return (0.0, 0.0);
        }
        ((x1 - x0) / dt, (y1 - y0) / dt)
    }
}

/// Apply one frame of friction-based deceleration.
///
/// Each call multiplies the velocity by `friction` (e.g. 0.95). At 60 fps
/// this converges smoothly to zero.
pub fn decelerate(vx: f64, vy: f64, friction: f64) -> (f64, f64) {
    (vx * friction, vy * friction)
}

// ---------------------------------------------------------------------------
// Spring physics
// ---------------------------------------------------------------------------

/// A damped spring following Hooke's law: F = -k*x - d*v.
///
/// Used for snap-back animations (e.g. an incomplete swipe returning to its
/// starting position) and commit animations (overshooting then settling).
#[derive(Debug, Clone)]
pub struct Spring {
    /// Spring stiffness (k). Higher = snappier.
    pub stiffness: f64,
    /// Damping ratio (d). 1.0 = critically damped, <1.0 = under-damped
    /// (oscillates), >1.0 = over-damped (sluggish).
    pub damping: f64,
}

impl Default for Spring {
    fn default() -> Self {
        Self {
            stiffness: 600.0,
            damping: 0.8,
        }
    }
}

impl Spring {
    pub fn new(stiffness: f64, damping: f64) -> Self {
        Self { stiffness, damping }
    }

    /// Balanced preset for general-purpose UI transitions.
    pub const RESPONSIVE: Self = Self {
        stiffness: 300.0,
        damping: 25.0,
    };

    /// Slow, soft preset for background or low-priority animations.
    pub const GENTLE: Self = Self {
        stiffness: 150.0,
        damping: 20.0,
    };

    /// Fast, tight preset for quick feedback animations.
    pub const SNAPPY: Self = Self {
        stiffness: 400.0,
        damping: 30.0,
    };

    /// Compute the spring force given current displacement and velocity.
    /// `displacement` is the distance from the rest position (positive = stretched).
    pub fn force(&self, displacement: f64, velocity: f64) -> f64 {
        -self.stiffness * displacement - self.damping * velocity
    }

    /// Advance the simulation by `dt` seconds using semi-implicit Euler.
    ///
    /// Returns `(new_position, new_velocity)`.
    pub fn step(&self, position: f64, velocity: f64, dt: f64) -> (f64, f64) {
        let accel = self.force(position, velocity);
        let new_vel = velocity + accel * dt;
        let new_pos = position + new_vel * dt;
        (new_pos, new_vel)
    }

    /// Check whether the spring has effectively come to rest.
    ///
    /// Both position and velocity must be within `threshold` of zero.
    pub fn is_settled(&self, position: f64, velocity: f64, threshold: f64) -> bool {
        position.abs() < threshold && velocity.abs() < threshold
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    // ---- Spring tests ----

    #[test]
    fn spring_force_basic() {
        let s = Spring::new(600.0, 0.8);
        let f = s.force(1.0, 0.0);
        assert!((f - -600.0).abs() < 1e-9);
    }

    #[test]
    fn spring_force_with_velocity() {
        let s = Spring::new(600.0, 0.8);
        let f = s.force(2.0, 100.0);
        assert!((f - -1280.0).abs() < 1e-9);
    }

    #[test]
    fn spring_zero_displacement_zero_velocity() {
        let s = Spring::default();
        assert!((s.force(0.0, 0.0)).abs() < 1e-12);
    }

    #[test]
    fn spring_convergence() {
        let s = Spring::new(300.0, 15.0);
        let dt = 1.0 / 60.0;
        let mut pos = 100.0;
        let mut vel = 0.0;

        for _ in 0..600 {
            let (p, v) = s.step(pos, vel, dt);
            pos = p;
            vel = v;
        }

        assert!(
            s.is_settled(pos, vel, 0.1),
            "pos={pos}, vel={vel} — spring did not settle"
        );
    }

    #[test]
    fn spring_settled_check() {
        let s = Spring::default();
        assert!(s.is_settled(0.0, 0.0, 0.5));
        assert!(s.is_settled(0.1, -0.1, 0.5));
        assert!(!s.is_settled(1.0, 0.0, 0.5));
        assert!(!s.is_settled(0.0, 1.0, 0.5));
    }

    #[test]
    fn spring_negative_displacement() {
        let s = Spring::new(600.0, 0.8);
        let f = s.force(-1.0, 0.0);
        assert!(f > 0.0, "expected positive force, got {f}");
        assert!((f - 600.0).abs() < 1e-9);
    }

    #[test]
    fn spring_responsive_preset() {
        let s = Spring::RESPONSIVE;
        assert!((s.stiffness - 300.0).abs() < 1e-9);
        assert!((s.damping - 25.0).abs() < 1e-9);
    }

    #[test]
    fn spring_gentle_preset() {
        let s = Spring::GENTLE;
        assert!((s.stiffness - 150.0).abs() < 1e-9);
        assert!((s.damping - 20.0).abs() < 1e-9);
    }

    #[test]
    fn spring_snappy_preset() {
        let s = Spring::SNAPPY;
        assert!((s.stiffness - 400.0).abs() < 1e-9);
        assert!((s.damping - 30.0).abs() < 1e-9);
    }

    #[test]
    fn responsive_preset_settles() {
        let s = Spring::RESPONSIVE;
        let dt = 1.0 / 60.0;
        let mut pos = 50.0;
        let mut vel = 0.0;
        for _ in 0..600 {
            let (p, v) = s.step(pos, vel, dt);
            pos = p;
            vel = v;
        }
        assert!(
            s.is_settled(pos, vel, 0.1),
            "RESPONSIVE preset did not settle: pos={pos}, vel={vel}"
        );
    }

    #[test]
    fn gentle_preset_settles() {
        let s = Spring::GENTLE;
        let dt = 1.0 / 60.0;
        let mut pos = 50.0;
        let mut vel = 0.0;
        for _ in 0..600 {
            let (p, v) = s.step(pos, vel, dt);
            pos = p;
            vel = v;
        }
        assert!(
            s.is_settled(pos, vel, 0.1),
            "GENTLE preset did not settle: pos={pos}, vel={vel}"
        );
    }

    #[test]
    fn snappy_preset_settles() {
        let s = Spring::SNAPPY;
        let dt = 1.0 / 60.0;
        let mut pos = 50.0;
        let mut vel = 0.0;
        for _ in 0..600 {
            let (p, v) = s.step(pos, vel, dt);
            pos = p;
            vel = v;
        }
        assert!(
            s.is_settled(pos, vel, 0.1),
            "SNAPPY preset did not settle: pos={pos}, vel={vel}"
        );
    }

    // ---- Momentum tracker tests ----

    #[test]
    fn momentum_velocity_basic() {
        let mut m = MomentumTracker::new(10);
        let t0 = Instant::now();
        let t1 = t0 + Duration::from_millis(100);
        m.push(t0, 0.0, 0.0);
        m.push(t1, 100.0, 0.0);
        let (vx, vy) = m.velocity_at_release();
        assert!((vx - 1000.0).abs() < 1.0);
        assert!(vy.abs() < 1.0);
    }

    #[test]
    fn momentum_velocity_diagonal() {
        let mut m = MomentumTracker::new(10);
        let t0 = Instant::now();
        let t1 = t0 + Duration::from_millis(200);
        m.push(t0, 0.0, 0.0);
        m.push(t1, 200.0, 400.0);
        let (vx, vy) = m.velocity_at_release();
        assert!((vx - 1000.0).abs() < 1.0);
        assert!((vy - 2000.0).abs() < 1.0);
    }

    #[test]
    fn momentum_empty_returns_zero() {
        let m = MomentumTracker::new(10);
        assert_eq!(m.velocity_at_release(), (0.0, 0.0));
    }

    #[test]
    fn momentum_single_sample_returns_zero() {
        let mut m = MomentumTracker::new(10);
        m.push(Instant::now(), 50.0, 50.0);
        assert_eq!(m.velocity_at_release(), (0.0, 0.0));
    }

    #[test]
    fn momentum_reset_clears_history() {
        let mut m = MomentumTracker::new(10);
        let t0 = Instant::now();
        m.push(t0, 0.0, 0.0);
        m.push(t0 + Duration::from_millis(100), 100.0, 0.0);
        m.reset();
        assert_eq!(m.velocity_at_release(), (0.0, 0.0));
    }

    #[test]
    fn momentum_max_samples_evicts_oldest() {
        let mut m = MomentumTracker::new(3);
        let t0 = Instant::now();
        m.push(t0, 0.0, 0.0);
        m.push(t0 + Duration::from_millis(100), 100.0, 0.0);
        m.push(t0 + Duration::from_millis(200), 200.0, 0.0);
        m.push(t0 + Duration::from_millis(300), 300.0, 0.0);
        let (vx, _) = m.velocity_at_release();
        assert!((vx - 1000.0).abs() < 1.0);
    }

    // ---- Deceleration tests ----

    #[test]
    fn deceleration_reduces_velocity() {
        let (vx, vy) = decelerate(1000.0, 500.0, 0.95);
        assert!((vx - 950.0).abs() < 1e-9);
        assert!((vy - 475.0).abs() < 1e-9);
    }

    #[test]
    fn deceleration_converges_to_zero() {
        let mut vx = 1000.0;
        let mut vy = 1000.0;
        for _ in 0..300 {
            let (nvx, nvy) = decelerate(vx, vy, 0.95);
            vx = nvx;
            vy = nvy;
        }
        assert!(vx.abs() < 0.01, "vx should be near zero, got {vx}");
        assert!(vy.abs() < 0.01, "vy should be near zero, got {vy}");
    }

    #[test]
    fn deceleration_zero_friction_preserves_velocity() {
        let (vx, vy) = decelerate(100.0, 200.0, 1.0);
        assert!((vx - 100.0).abs() < 1e-9);
        assert!((vy - 200.0).abs() < 1e-9);
    }
}
