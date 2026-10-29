//! Unit tests for [`super`] — the steal module.

use super::super::test_support::*;
use super::*;

// ── Steals ────────────────────────────────────────────────────────────────

fn on(kind: PitchKind) -> StealAttempt {
    StealAttempt::on_first_move(kind)
}

#[test]
fn steal_succeeds_against_offspeed() {
    let mut score = ScoreBoard::default();
    let mut bases = with(&[0]);
    assert_eq!(
        attempt_steal(
            &mut score,
            &mut bases,
            &std_rules(),
            &on(PitchKind::Changeup)
        ),
        StealResult::Stolen { base: 1 }
    );
    assert_eq!(bases, with(&[1]));
    assert_eq!(score.outs, 0);
}

#[test]
fn steal_is_caught_against_a_fastball() {
    let mut score = ScoreBoard {
        balls: 2,
        strikes: 1,
        ..Default::default()
    };
    let mut bases = with(&[0]);
    assert_eq!(
        attempt_steal(
            &mut score,
            &mut bases,
            &std_rules(),
            &on(PitchKind::Fastball)
        ),
        StealResult::Caught
    );
    assert_eq!(bases, empty());
    assert_eq!(score.outs, 1);
    // The at-bat continues with the count intact.
    assert_eq!((score.balls, score.strikes), (2, 1));
}

#[test]
fn only_the_lead_eligible_runner_steals() {
    // Runners on first and second: second steals third; first stays put
    // (one steal per pitch). Third needs a lead stretched in the duel —
    // the throw is short.
    let mut score = ScoreBoard::default();
    let mut bases = with(&[0, 1]);
    let attempt = StealAttempt {
        lead_m: LEAD_MAX_M,
        ..on(PitchKind::Changeup)
    };
    assert_eq!(
        attempt_steal(&mut score, &mut bases, &std_rules(), &attempt),
        StealResult::Stolen { base: 2 }
    );
    assert_eq!(bases, with(&[0, 2]));
}

#[test]
fn home_cannot_be_stolen() {
    let mut score = ScoreBoard::default();
    let mut bases = with(&[2]);
    assert_eq!(
        attempt_steal(
            &mut score,
            &mut bases,
            &std_rules(),
            &on(PitchKind::Changeup)
        ),
        StealResult::NoRunner
    );
    assert_eq!(bases, with(&[2]));
}

#[test]
fn empty_bases_cannot_steal() {
    let mut score = ScoreBoard::default();
    let mut bases = empty();
    assert_eq!(
        attempt_steal(
            &mut score,
            &mut bases,
            &std_rules(),
            &on(PitchKind::Changeup)
        ),
        StealResult::NoRunner
    );
}

#[test]
fn a_full_stretched_lead_beats_even_a_fastball() {
    let mut score = ScoreBoard::default();
    let mut bases = with(&[0]);
    let attempt = StealAttempt {
        lead_m: LEAD_MAX_M,
        ..on(PitchKind::Fastball)
    };
    assert_eq!(
        attempt_steal(&mut score, &mut bases, &std_rules(), &attempt),
        StealResult::Stolen { base: 1 }
    );
    assert_eq!(bases, with(&[1]));
    assert_eq!(score.outs, 0);
}

// ── The race ──────────────────────────────────────────────────────────────

#[test]
fn race_times_match_the_mlb_stopwatch() {
    // Per docs/BASEBALL.md: ~1.3-1.5 s to the plate, ~2.0 s pop to second,
    // ~3.3-3.5 s for an average runner from a normal lead.
    let fb = delivery_secs(PitchKind::Fastball);
    assert!((1.3..=1.4).contains(&fb), "fastball to the plate {fb}");
    let ch = delivery_secs(PitchKind::Changeup);
    assert!((1.4..=1.55).contains(&ch), "changeup to the plate {ch}");
    let race = steal_race(0, &on(PitchKind::Fastball));
    let pop = race.defense_secs - fb;
    assert!((1.95..=2.05).contains(&pop), "pop to second {pop}");
    assert!(
        (3.3..=3.45).contains(&race.runner_secs),
        "runner {}",
        race.runner_secs
    );
    // Third is the short throw.
    let pop3 = steal_race(1, &on(PitchKind::Fastball)).defense_secs - fb;
    assert!((1.55..=1.7).contains(&pop3), "pop to third {pop3}");
}

#[test]
fn every_pitch_kind_slows_the_battery_by_its_speed() {
    use PitchKind::*;
    let def = |k| steal_race(0, &on(k)).defense_secs;
    assert!(def(Fastball) < def(Sinker));
    assert!(def(Sinker) < def(Slider));
    assert!(def(Slider) < def(Curveball));
    assert!(def(Curveball) < def(Changeup));
}

#[test]
fn a_late_break_turns_a_steal_into_an_out() {
    let clean = on(PitchKind::Changeup);
    assert!(steal_race(0, &clean).safe());
    let late = StealAttempt {
        break_fraction: 0.6,
        ..clean
    };
    assert!(
        !steal_race(0, &late).safe(),
        "broke as the ball was nearly out"
    );
    let race = steal_race(0, &late);
    assert!(
        (race.runner_secs - steal_race(0, &clean).runner_secs - 0.6 * LATE_BREAK_SECS).abs() < 1e-5
    );
}

#[test]
fn a_pitch_in_the_dirt_gives_the_runner_the_transfer() {
    let high = StealAttempt {
        crossing: Some(Vec2::new(0.0, ZONE_LOW + 0.3)),
        ..on(PitchKind::Fastball)
    };
    assert!(!steal_race(0, &high).safe());
    assert_eq!(receive_penalty_secs(high.crossing), 0.0);
    let dirt = StealAttempt {
        crossing: Some(Vec2::new(0.0, 0.0)),
        ..high
    };
    assert!(
        steal_race(0, &dirt).safe(),
        "the catcher dug it out too late"
    );
    assert!(
        receive_penalty_secs(dirt.crossing)
            > receive_penalty_secs(Some(Vec2::new(0.0, ZONE_LOW - 0.1)))
    );
    // Reaching across for a wide one costs a little too.
    assert!(receive_penalty_secs(Some(Vec2::new(ZONE_HALF_WIDTH + 0.5, 0.8))) > 0.0);
}

#[test]
fn a_longer_lead_is_a_faster_steal() {
    let short = steal_race(0, &on(PitchKind::Fastball)).runner_secs;
    let long = steal_race(
        0,
        &StealAttempt {
            lead_m: LEAD_NORMAL_M + 1.0,
            ..on(PitchKind::Fastball)
        },
    )
    .runner_secs;
    assert!((short - long - 1.0 / RUNNER_SPRINT_SPEED).abs() < 1e-5);
}

#[test]
fn shuffle_lead_grows_and_retreats_within_bounds() {
    let mut lead = LEAD_NORMAL_M;
    for _ in 0..200 {
        lead = shuffle_lead(lead, true, 1.0 / 60.0);
    }
    assert_eq!(lead, LEAD_MAX_M);
    let back = shuffle_lead(lead, false, 0.5);
    assert!((lead - back - 0.5 * LEAD_SHUFFLE_SPEED).abs() < 1e-5);
    assert_eq!(shuffle_lead(back, false, 10.0), LEAD_NORMAL_M);
}

// ── Pickoffs ──────────────────────────────────────────────────────────────

#[test]
fn pickoff_is_a_race_against_the_dive_back() {
    // A fresh stretch still gets back; one hung out there is caught.
    assert!(dive_back_secs(LEAD_NORMAL_M) < PICKOFF_THROW_SECS);
    assert!(dive_back_secs(LEAD_NORMAL_M + 1.0) < PICKOFF_THROW_SECS);
    assert!(dive_back_secs(LEAD_MAX_M) > PICKOFF_THROW_SECS);
    let mut score = ScoreBoard::default();
    let mut bases = with(&[0]);
    assert_eq!(
        attempt_pickoff(&mut score, &mut bases, &std_rules(), LEAD_NORMAL_M + 1.0),
        PickoffResult::SafeBack
    );
}

#[test]
fn pickoff_catches_an_extended_lead() {
    let mut score = ScoreBoard {
        balls: 1,
        strikes: 2,
        ..Default::default()
    };
    let mut bases = with(&[0]);
    assert_eq!(
        attempt_pickoff(&mut score, &mut bases, &std_rules(), LEAD_MAX_M),
        PickoffResult::PickedOff { base: 0 }
    );
    assert_eq!(bases, empty());
    assert_eq!(score.outs, 1);
    // The batter's count survives — no pitch was thrown.
    assert_eq!((score.balls, score.strikes), (1, 2));
}

#[test]
fn pickoff_on_a_normal_lead_is_safe() {
    let mut score = ScoreBoard::default();
    let mut bases = with(&[0]);
    assert_eq!(
        attempt_pickoff(&mut score, &mut bases, &std_rules(), LEAD_NORMAL_M),
        PickoffResult::SafeBack
    );
    assert_eq!(bases, with(&[0]));
    assert_eq!(score.outs, 0);
}

#[test]
fn pickoff_with_nobody_leading_is_no_play() {
    let mut score = ScoreBoard::default();
    let mut bases = empty();
    assert_eq!(
        attempt_pickoff(&mut score, &mut bases, &std_rules(), LEAD_MAX_M),
        PickoffResult::NoRunner
    );
}

#[test]
fn pickoff_third_out_retires_the_side() {
    let mut score = ScoreBoard {
        outs: 2,
        top_of_inning: true,
        inning: 1,
        ..Default::default()
    };
    let mut bases = with(&[0]);
    assert_eq!(
        attempt_pickoff(&mut score, &mut bases, &std_rules(), LEAD_MAX_M),
        PickoffResult::PickedOff { base: 0 }
    );
    assert_eq!(score.outs, 0, "side retired: outs reset");
    assert!(!score.top_of_inning, "half-inning flips on the third out");
    assert_eq!(bases, empty());
}
