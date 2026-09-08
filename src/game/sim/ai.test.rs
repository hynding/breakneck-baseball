//! Unit tests for [`super`] — the ai module.

use super::*;

#[test]
fn draw_target_dt_is_deterministic_and_bounded() {
    // Same seed always draws the same target (the CPU's decision has to
    // be reproducible across frames via `get_or_insert_with`).
    let a = draw_target_dt(12.34, 70.0);
    let b = draw_target_dt(12.34, 70.0);
    assert_eq!(a, b);
    // noise() is in −1.0..1.0, so the draw never exceeds the spread.
    assert!(a.abs() <= 70.0 + f32::EPSILON);

    // A different seed generally draws a different target — spot-check
    // a handful so this isn't a degenerate constant function.
    let distinct = (0..8)
        .map(|i| draw_target_dt(i as f32 * 11.9, 70.0))
        .collect::<Vec<_>>();
    assert!(
        distinct.windows(2).any(|w| (w[0] - w[1]).abs() > 1.0),
        "expected varying draws across seeds, got {distinct:?}"
    );
}

#[test]
fn ready_to_press_fires_on_the_first_frame_dt_reaches_target() {
    // A synthetic dt ramp standing in for `swing_dt_ms` sampled once per
    // frame as a pitch approaches: monotonically increasing, early
    // (negative) to late (positive), like the live ball's timing error.
    let ramp: Vec<f32> = (0..50).map(|i| -100.0 + i as f32 * 4.0).collect();
    let target = 37.0;

    let fired = ramp.iter().position(|&dt| ready_to_press(dt, target));

    // First dt >= 37.0 in the ramp (-100, -96, ..., step 4) is 40.0 at
    // index 35 — pin the exact frame, not just "eventually fires".
    assert_eq!(fired, Some(35));
    assert!(ramp[35] >= target);
    assert!(ramp[34] < target);
}

#[test]
fn ready_to_press_fires_immediately_for_an_already_past_target() {
    // A target earlier than the current dt (the commit landed later
    // than the drawn target) presses on the very same frame — a swing
    // can't be un-pressed to wait for a target already behind it.
    assert!(ready_to_press(-10.0, -50.0));
    assert!(ready_to_press(0.0, 0.0));
}

#[test]
fn ready_to_press_holds_for_a_target_not_yet_reached() {
    assert!(!ready_to_press(-20.0, 10.0));
}

// ── The behind-in-the-count package (TODO 10) ─────────────────────────────

#[test]
fn the_package_arms_engage_together_at_two_balls() {
    // Both sides of the package key on the same count threshold — the
    // pairing is the whole point (a pitcher-only pull converts walks
    // into strikeouts; see the TODO 10 history).
    for balls in 0..=1 {
        assert_eq!(behind_scatter_scale(balls), 1.0);
        assert!(!wants_get_it_over(balls));
        assert_eq!(ahead_timing_scale(balls), 1.0);
        assert_eq!(ahead_chase_scale(balls), 1.0);
    }
    for balls in 2..=3 {
        assert!(behind_scatter_scale(balls) < 1.0, "balls={balls}");
        assert!(wants_get_it_over(balls), "balls={balls}");
        assert!(ahead_timing_scale(balls) < 1.0, "balls={balls}");
        assert!(ahead_chase_scale(balls) < 1.0, "balls={balls}");
    }
}

#[test]
fn ahead_count_timing_draw_is_strictly_tighter() {
    // Same seed, scaled spread: the drawn |target| shrinks by exactly
    // the scale, so the ahead-count hitter's misses are proportionally
    // smaller — whiffs become contact, not the other way around.
    let base = draw_target_dt(12.34, 70.0);
    let ahead = draw_target_dt(12.34, 70.0 * ahead_timing_scale(2));
    assert!(ahead.abs() < base.abs());
    assert_eq!(ahead, base * ahead_timing_scale(2));
}
