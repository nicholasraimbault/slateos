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
        .filter(|g| matches!(g, GestureType::ContinuousEdge(eg) if eg.phase == GesturePhase::End))
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
        .filter(
            |g| matches!(g, GestureType::ContinuousEdge(eg) if eg.phase == GesturePhase::Cancel),
        )
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
        .filter(
            |g| matches!(g, GestureType::ContinuousEdge(eg) if eg.phase == GesturePhase::Cancel),
        )
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
    assert!(
        matches!(&start[0], GestureType::ContinuousEdge(eg) if eg.phase == GesturePhase::Start)
    );

    // Update 1.
    let u1 = r.on_event(&InputEvent::FingerMove {
        slot: 0,
        x: 640,
        y: 100,
        time: now + Duration::from_millis(50),
    });
    assert_eq!(u1.len(), 1);
    assert!(matches!(&u1[0], GestureType::ContinuousEdge(eg) if eg.phase == GesturePhase::Update));

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
        end.iter()
            .any(|g| matches!(g, GestureType::ContinuousEdge(eg) if eg.phase == GesturePhase::End)),
        "expected End in results, got {end:?}"
    );
}
