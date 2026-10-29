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
        assert!(behind_scatter_scale(balls) <= 1.0, "balls={balls}");
        assert!(wants_get_it_over(balls), "balls={balls}");
        assert!(ahead_timing_scale(balls) < 1.0, "balls={balls}");
        assert!(ahead_chase_scale(balls) < 1.0, "balls={balls}");
    }
}

#[test]
fn three_ball_counts_take_more_often() {
    // 3-0 is close to an automatic take, 3-1 selective, 3-2 and every
    // other count a normal offer — the walk's whole existence rides on it.
    assert!(count_take_scale(3, 0) < 0.25);
    assert!(count_take_scale(3, 1) < 1.0);
    assert!(count_take_scale(3, 1) > count_take_scale(3, 0));
    assert_eq!(count_take_scale(3, 2), 1.0);
    // The first pitch is taken more often than a mid-count strike, but it
    // is nowhere near the 3-0 take.
    assert!(count_take_scale(0, 0) < 1.0 && count_take_scale(0, 0) > 0.5);
    for balls in 0..=2 {
        for strikes in 0..=2 {
            if (balls, strikes) != (0, 0) {
                assert_eq!(count_take_scale(balls, strikes), 1.0);
            }
        }
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

#[test]
fn steal_rate_is_situational() {
    let mut score = ScoreBoard {
        inning: 3,
        ..Default::default()
    };
    let base = steal_rate(0.05, &score);
    assert!(
        (base - 0.05).abs() < 1e-6,
        "an early, tied game runs at the base rate"
    );
    score.outs = 2;
    assert!(steal_rate(0.05, &score) > base, "two outs: nothing to lose");
    score.outs = 0;
    score.inning = 8;
    assert!(
        steal_rate(0.05, &score) > base,
        "late and close: the running game is on"
    );
    score.home_runs = 6;
    assert_eq!(steal_rate(0.05, &score), 0.0, "nobody runs in a blowout");
}

#[test]
fn a_pitcher_challenges_a_hitter_who_never_offers() {
    // Only deep in the count with two called strikes taken, and only while
    // the hitter has yet to swing — the moment he offers, the honest scatter
    // is back. Taking the way to 3-0 is patience, not passivity.
    for balls in 0..=3 {
        for strikes in 0..=1 {
            assert_eq!(taker_scatter_scale(false, balls, strikes), 1.0);
        }
    }
    assert_eq!(taker_scatter_scale(false, 1, 2), 1.0);
    for balls in 2..=3 {
        assert!(taker_scatter_scale(false, balls, 2) < 1.0, "balls={balls}");
        assert_eq!(taker_scatter_scale(true, balls, 2), 1.0);
    }
}

// ── Count-aware pitch calling ─────────────────────────────────────────────

/// Share of `kind` in a count's call table.
fn share(balls: u32, strikes: u32, kind: PitchKind) -> f32 {
    let mix = pitch_mix(balls, strikes);
    let i = ARSENAL.iter().position(|k| *k == kind).unwrap();
    mix[i] / mix.iter().sum::<f32>()
}

#[test]
fn the_call_follows_the_count() {
    use PitchKind::*;
    // Behind: the heater (MLB throws a fastball 70%+ of the time at 3-0,
    // 3-1 and 2-0, per docs/BASEBALL.md).
    for (b, s) in [(3, 0), (3, 1), (2, 0)] {
        assert!(
            share(b, s, Fastball) + share(b, s, Sinker) >= 0.7,
            "{b}-{s}"
        );
    }
    // Ahead: more breaking balls, fewer heaters.
    for (b, s) in [(0, 2), (1, 2)] {
        let breaking = |b, s| share(b, s, Slider) + share(b, s, Curveball);
        assert!(breaking(b, s) > breaking(0, 0) + 0.1, "{b}-{s}");
        assert!(share(b, s, Fastball) < share(0, 0, Fastball), "{b}-{s}");
    }
    // Every count stays a real mix, never a single pitch.
    for b in 0..=3 {
        for s in 0..=2 {
            assert!(share(b, s, Fastball) < 0.85, "{b}-{s}");
            assert!(share(b, s, Changeup) <= 0.2, "{b}-{s}");
        }
    }
}

#[test]
fn call_pitch_spans_the_table() {
    // A fine sweep of rolls reproduces the weights (and the top of the
    // range never falls off the end of the table).
    for (b, s) in [(0, 0), (3, 0), (0, 2)] {
        let n = 10_000;
        for kind in ARSENAL {
            let hits = (0..n)
                .filter(|i| call_pitch(*i as f32 / n as f32, b, s) == kind)
                .count();
            let got = hits as f32 / n as f32;
            assert!((got - share(b, s, kind)).abs() < 0.002, "{b}-{s} {kind:?}");
        }
        call_pitch(0.999_999, b, s);
    }
}

#[test]
fn the_pitch_called_is_the_pitch_thrown() {
    // Whatever the scatter and the count, the aim decodes back to the
    // called kind — scatter used to turn a third of all pitches into
    // changeups.
    let steps = 12;
    for kind in ARSENAL {
        for ix in -steps..=steps {
            for iy in -steps..=steps {
                let scatter = Vec2::new(ix as f32, iy as f32 * 0.5) * (0.9 / steps as f32);
                for balls in 0..=3 {
                    for chase in [false, true] {
                        let aim = pitch_aim(kind, scatter, balls, chase);
                        assert_eq!(
                            PitchKind::from_aim(aim),
                            kind,
                            "{scatter} b={balls} {chase}"
                        );
                        assert!(aim.x >= -0.45, "never aimed at the batter: {aim}");
                    }
                }
            }
        }
    }
}

#[test]
fn chase_pitches_only_with_the_hitter_in_a_hole() {
    // ...and the hitter in the hole expands his zone to meet them.
    assert!(two_strike_chase_scale(2) > 1.0);
    assert_eq!(two_strike_chase_scale(1), 1.0);
    assert!(chase_rate(0, 2) > chase_rate(1, 2));
    assert!(chase_rate(1, 2) > 0.0);
    for (b, s) in [(0, 0), (0, 1), (2, 2), (3, 2), (3, 0)] {
        assert_eq!(chase_rate(b, s), 0.0, "{b}-{s}");
    }
    // The chase heater sits above where the regular one does.
    let fb = pitch_aim(PitchKind::Fastball, Vec2::ZERO, 0, false);
    let up = pitch_aim(PitchKind::Fastball, Vec2::ZERO, 0, true);
    assert!(up.y > fb.y);
}

#[test]
fn a_hitter_who_never_offers_gets_strikes_from_two_balls() {
    // The call goes straight at a statue from two balls — but only one who
    // has stood through two whole plate appearances already; a patient
    // hitter taking to 2-0 keeps the mix.
    assert!(grooves_for_taker(false, 2, 2));
    assert!(!grooves_for_taker(false, 1, 3));
    assert!(!grooves_for_taker(false, 2, 1));
    assert!(!grooves_for_taker(true, 5, 3));
    // The scatter pull waits for two strikes as well.
    assert!(!challenges_taker(false, 2, 1));
    assert!(challenges_taker(false, 2, 2));
}
