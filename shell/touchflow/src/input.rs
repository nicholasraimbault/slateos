/// Raw multitouch input reader.
///
/// Opens `/dev/input/eventX` via the `evdev` crate and translates MT protocol
/// B events into a stream of `InputEvent`s for the gesture recognizer. The
/// actual device access is Linux-only (`#[cfg(target_os = "linux")]`); the
/// data types and pure-logic helpers are available on all platforms so tests
/// can run on macOS.
use std::time::Instant;

// ---------------------------------------------------------------------------
// Platform-independent types
// ---------------------------------------------------------------------------

/// Maximum number of simultaneous touch slots the kernel exposes.
pub const MAX_SLOTS: usize = 10;

/// State of a single MT slot (one finger).
#[derive(Debug, Clone)]
pub struct TouchSlot {
    /// Kernel tracking ID. -1 means inactive (finger up).
    pub tracking_id: i32,
    /// Horizontal coordinate in device units.
    pub x: i32,
    /// Vertical coordinate in device units.
    pub y: i32,
    /// Pressure (0 when not reported). Gated behind the `pressure` feature
    /// because force-touch gestures are not yet implemented.
    #[cfg(feature = "pressure")]
    pub pressure: i32,
}

impl Default for TouchSlot {
    fn default() -> Self {
        Self {
            tracking_id: -1,
            x: 0,
            y: 0,
            #[cfg(feature = "pressure")]
            pressure: 0,
        }
    }
}

impl TouchSlot {
    /// Whether this slot currently tracks a finger.
    pub fn is_active(&self) -> bool {
        self.tracking_id >= 0
    }
}

/// Aggregate state for all touch slots.
#[derive(Debug, Clone)]
pub struct InputState {
    pub slots: [TouchSlot; MAX_SLOTS],
    /// The slot that subsequent `ABS_MT_*` events apply to.
    pub current_slot: usize,
}

impl Default for InputState {
    fn default() -> Self {
        Self {
            slots: std::array::from_fn(|_| TouchSlot::default()),
            current_slot: 0,
        }
    }
}

impl InputState {
    /// Count currently-active fingers.
    #[allow(dead_code)]
    pub fn active_finger_count(&self) -> usize {
        self.slots.iter().filter(|s| s.is_active()).count()
    }

    /// Positions of all active fingers.
    #[allow(dead_code)]
    pub fn active_positions(&self) -> Vec<(i32, i32)> {
        self.slots
            .iter()
            .filter(|s| s.is_active())
            .map(|s| (s.x, s.y))
            .collect()
    }
}

/// Logical events emitted by the input reader for the gesture recognizer.
/// These are platform-independent so tests can inject them freely.
#[derive(Debug, Clone)]
// The `Finger` prefix is conventional for touch input events; suppressing the
// clippy lint that would force renaming to ambiguous single-word variants.
#[allow(clippy::enum_variant_names)]
pub enum InputEvent {
    /// A new finger touched the screen.
    FingerDown {
        slot: usize,
        x: i32,
        y: i32,
        time: Instant,
    },
    /// A tracked finger moved.
    FingerMove {
        slot: usize,
        x: i32,
        y: i32,
        time: Instant,
    },
    /// A finger left the screen.
    FingerUp { slot: usize, time: Instant },
}

/// Apply an `InputEvent` to an `InputState`, mutating it in place.
#[allow(dead_code)]
pub fn apply_event(state: &mut InputState, event: &InputEvent) {
    match event {
        InputEvent::FingerDown { slot, x, y, .. } => {
            if *slot < MAX_SLOTS {
                let s = &mut state.slots[*slot];
                s.tracking_id = *slot as i32; // use slot index as a simple ID
                s.x = *x;
                s.y = *y;
                state.current_slot = *slot;
            }
        }
        InputEvent::FingerMove { slot, x, y, .. } => {
            if *slot < MAX_SLOTS {
                let s = &mut state.slots[*slot];
                s.x = *x;
                s.y = *y;
            }
        }
        InputEvent::FingerUp { slot, .. } => {
            if *slot < MAX_SLOTS {
                state.slots[*slot] = TouchSlot::default();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Linux-only: actual device access
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
pub mod device {
    use super::*;
    use anyhow::{Context, Result};
    use evdev::AbsoluteAxisType;
    use tokio::sync::mpsc;
    use tracing::info;

    /// Scan `/dev/input/event*` and return the first device that has
    /// `ABS_MT_POSITION_X` capability (i.e. a multitouch touchscreen).
    pub fn find_touchscreen() -> Result<evdev::Device> {
        let devices = evdev::enumerate()
            .map(|(_path, dev)| dev)
            .collect::<Vec<_>>();

        for dev in devices {
            let abs_axes = dev.supported_absolute_axes();
            if let Some(axes) = abs_axes {
                if axes.contains(AbsoluteAxisType::ABS_MT_POSITION_X) {
                    info!(name = ?dev.name(), "found touchscreen");
                    return Ok(dev);
                }
            }
        }

        anyhow::bail!("no touchscreen device with ABS_MT_POSITION_X found")
    }

    /// Read events from the touchscreen in a loop, translate them into
    /// `InputEvent`s, and send them over a channel.
    ///
    /// This function does not return under normal operation; cancel the
    /// tokio task to stop it.
    pub async fn run_input_reader(tx: mpsc::Sender<InputEvent>) -> Result<()> {
        let mut dev = find_touchscreen().context("scanning for touchscreen")?;
        let mut state = InputState::default();

        // Track per-slot pending state so we can detect FingerDown vs FingerMove
        // across the SYN_REPORT boundary.
        let mut pending_id_change: [Option<i32>; MAX_SLOTS] = [None; MAX_SLOTS];
        let mut pending_x: [Option<i32>; MAX_SLOTS] = [None; MAX_SLOTS];
        let mut pending_y: [Option<i32>; MAX_SLOTS] = [None; MAX_SLOTS];

        loop {
            // evdev crate provides a blocking fetch_events. Run in
            // spawn_blocking to avoid stalling the tokio runtime.
            let events: Vec<evdev::InputEvent> = {
                let evts: Vec<_> = dev.fetch_events().context("reading events")?.collect();
                evts
            };

            let now = Instant::now();

            for ev in &events {
                match ev.event_type() {
                    evdev::EventType::ABSOLUTE => {
                        let code = AbsoluteAxisType(ev.code());
                        match code {
                            AbsoluteAxisType::ABS_MT_SLOT => {
                                let slot = ev.value() as usize;
                                if slot < MAX_SLOTS {
                                    state.current_slot = slot;
                                }
                            }
                            AbsoluteAxisType::ABS_MT_TRACKING_ID => {
                                pending_id_change[state.current_slot] = Some(ev.value());
                            }
                            AbsoluteAxisType::ABS_MT_POSITION_X => {
                                pending_x[state.current_slot] = Some(ev.value());
                            }
                            AbsoluteAxisType::ABS_MT_POSITION_Y => {
                                pending_y[state.current_slot] = Some(ev.value());
                            }
                            _ => {}
                        }
                    }
                    evdev::EventType::SYNCHRONIZATION => {
                        // SYN_REPORT: flush all pending slot changes.
                        for slot_idx in 0..MAX_SLOTS {
                            if let Some(id) = pending_id_change[slot_idx].take() {
                                if id == -1 {
                                    // Finger up
                                    state.slots[slot_idx] = TouchSlot::default();
                                    let _ = tx
                                        .send(InputEvent::FingerUp {
                                            slot: slot_idx,
                                            time: now,
                                        })
                                        .await;
                                } else {
                                    // New finger down
                                    let x = pending_x[slot_idx]
                                        .take()
                                        .unwrap_or(state.slots[slot_idx].x);
                                    let y = pending_y[slot_idx]
                                        .take()
                                        .unwrap_or(state.slots[slot_idx].y);
                                    state.slots[slot_idx] = TouchSlot {
                                        tracking_id: id,
                                        x,
                                        y,
                                        #[cfg(feature = "pressure")]
                                        pressure: 0,
                                    };
                                    let _ = tx
                                        .send(InputEvent::FingerDown {
                                            slot: slot_idx,
                                            x,
                                            y,
                                            time: now,
                                        })
                                        .await;
                                }
                            } else {
                                // No tracking-ID change — check for position updates.
                                let new_x = pending_x[slot_idx].take();
                                let new_y = pending_y[slot_idx].take();
                                if new_x.is_some() || new_y.is_some() {
                                    let s = &mut state.slots[slot_idx];
                                    if let Some(x) = new_x {
                                        s.x = x;
                                    }
                                    if let Some(y) = new_y {
                                        s.y = y;
                                    }
                                    if s.is_active() {
                                        let _ = tx
                                            .send(InputEvent::FingerMove {
                                                slot: slot_idx,
                                                x: s.x,
                                                y: s.y,
                                                time: now,
                                            })
                                            .await;
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests (platform-independent)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touch_slot_default_is_inactive() {
        let slot = TouchSlot::default();
        assert_eq!(slot.tracking_id, -1);
        assert!(!slot.is_active());
    }

    #[test]
    fn input_state_default_has_no_active_fingers() {
        let state = InputState::default();
        assert_eq!(state.active_finger_count(), 0);
        assert!(state.active_positions().is_empty());
    }

    #[test]
    fn apply_finger_down() {
        let mut state = InputState::default();
        let event = InputEvent::FingerDown {
            slot: 0,
            x: 500,
            y: 300,
            time: Instant::now(),
        };
        apply_event(&mut state, &event);
        assert!(state.slots[0].is_active());
        assert_eq!(state.slots[0].x, 500);
        assert_eq!(state.slots[0].y, 300);
        assert_eq!(state.active_finger_count(), 1);
    }

    #[test]
    fn apply_finger_move() {
        let mut state = InputState::default();
        let now = Instant::now();
        apply_event(
            &mut state,
            &InputEvent::FingerDown {
                slot: 0,
                x: 100,
                y: 100,
                time: now,
            },
        );
        apply_event(
            &mut state,
            &InputEvent::FingerMove {
                slot: 0,
                x: 200,
                y: 150,
                time: now,
            },
        );
        assert_eq!(state.slots[0].x, 200);
        assert_eq!(state.slots[0].y, 150);
    }

    #[test]
    fn apply_finger_up() {
        let mut state = InputState::default();
        let now = Instant::now();
        apply_event(
            &mut state,
            &InputEvent::FingerDown {
                slot: 0,
                x: 100,
                y: 100,
                time: now,
            },
        );
        apply_event(&mut state, &InputEvent::FingerUp { slot: 0, time: now });
        assert!(!state.slots[0].is_active());
        assert_eq!(state.active_finger_count(), 0);
    }

    #[test]
    fn multiple_fingers_tracked() {
        let mut state = InputState::default();
        let now = Instant::now();
        apply_event(
            &mut state,
            &InputEvent::FingerDown {
                slot: 0,
                x: 100,
                y: 100,
                time: now,
            },
        );
        apply_event(
            &mut state,
            &InputEvent::FingerDown {
                slot: 1,
                x: 200,
                y: 200,
                time: now,
            },
        );
        apply_event(
            &mut state,
            &InputEvent::FingerDown {
                slot: 2,
                x: 300,
                y: 300,
                time: now,
            },
        );
        assert_eq!(state.active_finger_count(), 3);
        assert_eq!(
            state.active_positions(),
            vec![(100, 100), (200, 200), (300, 300)]
        );
    }

    #[test]
    fn slot_out_of_range_ignored() {
        let mut state = InputState::default();
        let now = Instant::now();
        // Slot 99 is out of range — should not panic.
        apply_event(
            &mut state,
            &InputEvent::FingerDown {
                slot: 99,
                x: 100,
                y: 100,
                time: now,
            },
        );
        assert_eq!(state.active_finger_count(), 0);
    }
}
