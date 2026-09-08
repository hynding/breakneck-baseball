//! Unit tests for [`super`] — the coach module.

use super::*;

fn snap(time: f32, phase: CoachPhase) -> CoachSnapshot {
    CoachSnapshot {
        time,
        phase,
        in_steal_window: false,
        pending_call: false,
        runners_settled: true,
        untouched_pitch_result: false,
        result_secs: 1.2,
        auto_throw_delay_secs: 0.6,
        catcher_pos: Some(Vec3::new(0.0, 0.0, -2.0)),
        ball: None,
        contact: None,
        chaser: None,
        covers: Vec::new(),
        holding_since: None,
        fielders: Vec::new(),
        fielder_spots: Vec::new(),
        base_positions: vec![
            Vec3::new(-19.4, 0.0, 19.4),
            Vec3::new(0.0, 0.0, 38.8),
            Vec3::new(19.4, 0.0, 19.4),
        ],
        runners: Vec::new(),
    }
}

fn contact(at: f32, class: ContactClass, bases: &[usize]) -> ContactFacts {
    let mut occ = vec![false; 3];
    for &b in bases {
        occ[b] = true;
    }
    ContactFacts {
        at,
        class,
        outs_at_contact: 0,
        bases_at_contact: occ,
        steal_armed: false,
    }
}

fn runner(base: usize, pos: Vec3, moving: bool) -> RunnerFacts {
    RunnerFacts { base, pos, moving }
}

fn count(findings: &[CoachFinding], check: CheckId, severity: Severity) -> usize {
    findings
        .iter()
        .filter(|f| f.check == check && f.severity == severity)
        .count()
}

// ── Steal window ─────────────────────────────────────────────────────────

#[test]
fn pitch_during_the_steal_window_is_a_violation() {
    let mut coach = Coach::default();
    let mut s = snap(1.0, CoachPhase::WindUp);
    s.in_steal_window = true;
    let f = coach.observe(&s);
    assert_eq!(count(&f, CheckId::StealWindow, Severity::Violation), 1);
    // Fires once, not every sample.
    let f = coach.observe(&s);
    assert!(f.is_empty());
}

#[test]
fn pitch_after_the_window_closes_is_clean() {
    let mut coach = Coach::default();
    let mut s = snap(1.0, CoachPhase::PrePitch);
    s.in_steal_window = true;
    assert!(coach.observe(&s).is_empty());
    let s = snap(4.0, CoachPhase::WindUp);
    assert!(coach.observe(&s).is_empty());
}

// ── Runner breaks ────────────────────────────────────────────────────────

fn go_now_play(runner_moving: bool) -> (Coach, Vec<CoachFinding>) {
    let mut coach = Coach::default();
    let bag = Vec3::new(-19.4, 0.0, 19.4);
    let mut all = Vec::new();
    // Forced grounder with a runner on first: GoNow expected.
    for i in 0..30 {
        let t = 10.0 + i as f32 * 0.1;
        let mut s = snap(t, CoachPhase::InPlay);
        s.contact = Some(contact(10.0, ContactClass::Grounder, &[0]));
        s.chaser = Some(0);
        s.covers = vec![(0, 1), (1, 2), (2, 3), (3, 4)];
        s.runners = vec![runner(0, bag, runner_moving)];
        all.extend(coach.observe(&s));
    }
    // Play ends.
    let s = snap(13.5, CoachPhase::Result);
    all.extend(coach.observe(&s));
    (coach, all)
}

#[test]
fn forced_grounder_runner_who_never_breaks_is_a_violation() {
    let (_, f) = go_now_play(false);
    assert_eq!(count(&f, CheckId::RunnerBreaks, Severity::Violation), 1);
}

#[test]
fn forced_grounder_runner_who_breaks_is_clean() {
    let (_, f) = go_now_play(true);
    assert_eq!(count(&f, CheckId::RunnerBreaks, Severity::Violation), 0);
    assert_eq!(count(&f, CheckId::RunnerBreaks, Severity::Late), 0);
}

#[test]
fn late_break_is_reported_late_not_violation() {
    let mut coach = Coach::default();
    let bag = Vec3::new(-19.4, 0.0, 19.4);
    let mut all = Vec::new();
    for i in 0..30 {
        let t = 10.0 + i as f32 * 0.1;
        let mut s = snap(t, CoachPhase::InPlay);
        s.contact = Some(contact(10.0, ContactClass::Grounder, &[0]));
        s.chaser = Some(0);
        s.covers = vec![(0, 1), (1, 2), (2, 3), (3, 4)];
        // Breaks only 1.5 s after contact — past the 0.4 s tolerance.
        s.runners = vec![runner(0, bag, t >= 11.5)];
        all.extend(coach.observe(&s));
    }
    assert_eq!(count(&all, CheckId::RunnerBreaks, Severity::Late), 1);
    assert_eq!(count(&all, CheckId::RunnerBreaks, Severity::Violation), 0);
}

#[test]
fn tag_up_runner_leaving_early_is_a_violation() {
    let mut coach = Coach::default();
    let bag = Vec3::new(19.4, 0.0, 19.4); // third base
    let mut s = snap(10.2, CoachPhase::InPlay);
    s.contact = Some(contact(10.0, ContactClass::DeepFly, &[2]));
    s.ball = Some(BallFacts {
        pos: Vec3::new(0.0, 20.0, 60.0),
        vel: Vec3::new(0.0, 5.0, 20.0),
        predicted_landing: Some(Vec3::new(0.0, 0.0, 80.0)),
    });
    s.chaser = Some(0);
    // 8 m off the bag with the ball still up: left early.
    s.runners = vec![runner(2, bag + Vec3::new(-6.0, 0.0, -5.3), true)];
    let f = coach.observe(&s);
    assert_eq!(count(&f, CheckId::RunnerBreaks, Severity::Violation), 1);
}

#[test]
fn tag_up_runner_holding_is_clean() {
    let mut coach = Coach::default();
    let bag = Vec3::new(19.4, 0.0, 19.4);
    let mut s = snap(10.2, CoachPhase::InPlay);
    s.contact = Some(contact(10.0, ContactClass::DeepFly, &[2]));
    s.ball = Some(BallFacts {
        pos: Vec3::new(0.0, 20.0, 60.0),
        vel: Vec3::new(0.0, 5.0, 20.0),
        predicted_landing: Some(Vec3::new(0.0, 0.0, 80.0)),
    });
    s.chaser = Some(0);
    s.runners = vec![runner(2, bag + Vec3::new(-2.0, 0.0, 0.0), false)];
    let f = coach.observe(&s);
    assert_eq!(count(&f, CheckId::RunnerBreaks, Severity::Violation), 0);
}

#[test]
fn steal_armed_contact_suspends_break_expectations() {
    let mut coach = Coach::default();
    let bag = Vec3::new(-19.4, 0.0, 19.4);
    let mut all = Vec::new();
    for i in 0..30 {
        let t = 10.0 + i as f32 * 0.1;
        let mut s = snap(t, CoachPhase::InPlay);
        let mut c = contact(10.0, ContactClass::Grounder, &[0]);
        c.steal_armed = true;
        s.contact = Some(c);
        s.chaser = Some(0);
        s.runners = vec![runner(0, bag, false)];
        all.extend(coach.observe(&s));
    }
    let s = snap(13.5, CoachPhase::Result);
    all.extend(coach.observe(&s));
    assert_eq!(count(&all, CheckId::RunnerBreaks, Severity::Violation), 0);
}

// ── Chaser convergence ───────────────────────────────────────────────────

#[test]
fn live_ball_with_no_chaser_is_a_violation_at_play_end() {
    let mut coach = Coach::default();
    let mut all = Vec::new();
    for i in 0..20 {
        let t = 10.0 + i as f32 * 0.1;
        let mut s = snap(t, CoachPhase::InPlay);
        s.contact = Some(contact(10.0, ContactClass::CatchableFly, &[]));
        all.extend(coach.observe(&s));
    }
    let s = snap(12.5, CoachPhase::Result);
    all.extend(coach.observe(&s));
    assert_eq!(
        count(&all, CheckId::ChaserConvergence, Severity::Violation),
        1
    );
}

#[test]
fn promptly_assigned_chaser_tracking_the_landing_is_clean() {
    let mut coach = Coach::default();
    let landing = Vec3::new(5.0, 0.0, 60.0);
    let mut all = Vec::new();
    for i in 0..20 {
        let t = 10.0 + i as f32 * 0.1;
        let mut s = snap(t, CoachPhase::InPlay);
        s.contact = Some(contact(10.0, ContactClass::CatchableFly, &[]));
        s.ball = Some(BallFacts {
            pos: Vec3::new(2.0, 15.0, 40.0),
            vel: Vec3::new(1.0, -2.0, 15.0),
            predicted_landing: Some(landing),
        });
        s.chaser = Some(2);
        s.fielders = vec![FielderFacts {
            index: 2,
            pos: Vec3::new(0.0, 0.0, 50.0),
            move_target: Some(landing),
        }];
        all.extend(coach.observe(&s));
    }
    assert_eq!(
        count(&all, CheckId::ChaserConvergence, Severity::Violation),
        0
    );
    assert_eq!(count(&all, CheckId::ChaserConvergence, Severity::Late), 0);
}

#[test]
fn chaser_with_a_stale_intercept_is_flagged() {
    let mut coach = Coach::default();
    let mut all = Vec::new();
    for i in 0..20 {
        let t = 10.0 + i as f32 * 0.1;
        let mut s = snap(t, CoachPhase::InPlay);
        s.contact = Some(contact(10.0, ContactClass::CatchableFly, &[]));
        s.ball = Some(BallFacts {
            pos: Vec3::new(2.0, 10.0, 40.0),
            vel: Vec3::new(1.0, -2.0, 15.0),
            predicted_landing: Some(Vec3::new(5.0, 0.0, 60.0)),
        });
        s.chaser = Some(2);
        s.fielders = vec![FielderFacts {
            index: 2,
            pos: Vec3::new(-20.0, 0.0, 30.0),
            // Parked on a target 20+ m from the live landing.
            move_target: Some(Vec3::new(-25.0, 0.0, 30.0)),
        }];
        all.extend(coach.observe(&s));
    }
    assert_eq!(count(&all, CheckId::ChaserConvergence, Severity::Late), 1);
}

// ── Base coverage ────────────────────────────────────────────────────────

#[test]
fn uncovered_force_bag_is_a_violation() {
    let mut coach = Coach::default();
    let mut all = Vec::new();
    for i in 0..10 {
        let t = 10.0 + i as f32 * 0.1;
        let mut s = snap(t, CoachPhase::InPlay);
        // Runner on first: bags 0 (batter) and 1 (force) are relevant.
        s.contact = Some(contact(10.0, ContactClass::Grounder, &[0]));
        s.chaser = Some(0);
        s.covers = vec![(0, 1)]; // second base left uncovered
        s.runners = vec![runner(0, Vec3::new(-19.4, 0.0, 19.4), true)];
        all.extend(coach.observe(&s));
    }
    assert_eq!(count(&all, CheckId::BaseCoverage, Severity::Violation), 1);
}

#[test]
fn covered_force_bags_are_clean() {
    let mut coach = Coach::default();
    let mut all = Vec::new();
    for i in 0..10 {
        let t = 10.0 + i as f32 * 0.1;
        let mut s = snap(t, CoachPhase::InPlay);
        s.contact = Some(contact(10.0, ContactClass::Grounder, &[0]));
        s.chaser = Some(0);
        s.covers = vec![(0, 1), (1, 2)];
        s.runners = vec![runner(0, Vec3::new(-19.4, 0.0, 19.4), true)];
        all.extend(coach.observe(&s));
    }
    assert_eq!(count(&all, CheckId::BaseCoverage, Severity::Violation), 0);
}

// ── Catcher receives ─────────────────────────────────────────────────────

#[test]
fn untouched_pitch_resting_in_the_mitt_is_clean() {
    let mut coach = Coach::default();
    let mut all = Vec::new();
    for i in 0..12 {
        let t = 20.0 + i as f32 * 0.1;
        let mut s = snap(t, CoachPhase::Result);
        s.untouched_pitch_result = true;
        s.ball = Some(BallFacts {
            pos: Vec3::new(0.0, 0.5, -1.6),
            vel: Vec3::ZERO,
            predicted_landing: None,
        });
        all.extend(coach.observe(&s));
    }
    assert_eq!(
        count(&all, CheckId::CatcherReceives, Severity::Violation),
        0
    );
}

#[test]
fn untouched_pitch_sailing_past_the_mitt_is_a_violation() {
    let mut coach = Coach::default();
    let mut all = Vec::new();
    for i in 0..12 {
        let t = 20.0 + i as f32 * 0.1;
        let mut s = snap(t, CoachPhase::Result);
        s.untouched_pitch_result = true;
        s.ball = Some(BallFacts {
            pos: Vec3::new(0.0, 0.8, -6.0 - i as f32),
            vel: Vec3::new(0.0, 0.0, -20.0),
            predicted_landing: None,
        });
        all.extend(coach.observe(&s));
    }
    assert_eq!(
        count(&all, CheckId::CatcherReceives, Severity::Violation),
        1
    );
}

// ── Throw discipline ─────────────────────────────────────────────────────

#[test]
fn ball_held_past_the_auto_throw_deadline_is_a_violation() {
    let mut coach = Coach::default();
    let mut s = snap(11.5, CoachPhase::InPlay);
    s.contact = Some(contact(10.0, ContactClass::Grounder, &[]));
    s.chaser = None;
    s.holding_since = Some(10.4); // 1.1 s held > 0.6 + 0.3 grace
    let f = coach.observe(&s);
    assert_eq!(count(&f, CheckId::ThrowDiscipline, Severity::Violation), 1);
}

#[test]
fn ball_thrown_inside_the_deadline_is_clean() {
    let mut coach = Coach::default();
    let mut s = snap(10.8, CoachPhase::InPlay);
    s.contact = Some(contact(10.0, ContactClass::Grounder, &[]));
    s.holding_since = Some(10.4); // 0.4 s held < 0.9 deadline
    let f = coach.observe(&s);
    assert_eq!(count(&f, CheckId::ThrowDiscipline, Severity::Violation), 0);
}

#[test]
fn pending_call_never_announced_is_a_violation() {
    let mut coach = Coach::default();
    let mut all = Vec::new();
    for i in 0..60 {
        let t = 10.0 + i as f32 * 0.1;
        let mut s = snap(t, CoachPhase::InPlay);
        s.contact = Some(contact(10.0, ContactClass::Grounder, &[]));
        s.pending_call = true;
        all.extend(coach.observe(&s));
    }
    assert_eq!(
        count(&all, CheckId::ThrowDiscipline, Severity::Violation),
        1
    );
}

#[test]
fn pending_call_announced_inside_the_cap_is_clean() {
    let mut coach = Coach::default();
    let mut all = Vec::new();
    for i in 0..20 {
        let t = 10.0 + i as f32 * 0.1;
        let mut s = snap(t, CoachPhase::InPlay);
        s.contact = Some(contact(10.0, ContactClass::Grounder, &[]));
        s.pending_call = i < 15; // announced at 1.5 s
        all.extend(coach.observe(&s));
    }
    assert_eq!(
        count(&all, CheckId::ThrowDiscipline, Severity::Violation),
        0
    );
}

// ── Settlement ───────────────────────────────────────────────────────────

#[test]
fn runners_never_settling_is_a_violation() {
    let mut coach = Coach::default();
    let mut all = Vec::new();
    for i in 0..200 {
        let t = 30.0 + i as f32 * 0.1;
        let mut s = snap(t, CoachPhase::Result);
        s.runners_settled = false;
        all.extend(coach.observe(&s));
    }
    assert_eq!(count(&all, CheckId::Settlement, Severity::Violation), 1);
}

#[test]
fn a_normal_result_pause_is_clean() {
    let mut coach = Coach::default();
    let mut all = Vec::new();
    for i in 0..15 {
        let t = 30.0 + i as f32 * 0.1;
        let mut s = snap(t, CoachPhase::Result);
        s.runners_settled = i > 5;
        all.extend(coach.observe(&s));
    }
    assert_eq!(count(&all, CheckId::Settlement, Severity::Violation), 0);
}

// ── Idle discipline ──────────────────────────────────────────────────────

#[test]
fn fielder_parked_off_his_spot_between_plays_is_a_violation() {
    let mut coach = Coach::default();
    let spot = Vec3::new(0.0, 0.0, 50.0);
    let mut all = Vec::new();
    for i in 0..40 {
        let t = 40.0 + i as f32 * 0.1;
        let mut s = snap(t, CoachPhase::PrePitch);
        s.fielder_spots = vec![spot];
        s.fielders = vec![FielderFacts {
            index: 0,
            // Parked 8 m off the spot, no movement order.
            pos: spot + Vec3::new(8.0, 0.0, 0.0),
            move_target: None,
        }];
        all.extend(coach.observe(&s));
    }
    assert_eq!(count(&all, CheckId::IdleDiscipline, Severity::Violation), 1);
}

#[test]
fn fielder_jogging_back_to_his_spot_is_clean() {
    let mut coach = Coach::default();
    let spot = Vec3::new(0.0, 0.0, 50.0);
    let mut all = Vec::new();
    for i in 0..40 {
        let t = 40.0 + i as f32 * 0.1;
        let mut s = snap(t, CoachPhase::PrePitch);
        s.fielder_spots = vec![spot];
        // Jogs in at the return speed; heading straight for the spot.
        let remaining = (8.0 - (t - 40.0) * 4.0).max(0.0);
        s.fielders = vec![FielderFacts {
            index: 0,
            pos: spot + Vec3::new(remaining, 0.0, 0.0),
            move_target: (remaining > 0.0).then_some(spot),
        }];
        all.extend(coach.observe(&s));
    }
    assert_eq!(count(&all, CheckId::IdleDiscipline, Severity::Violation), 0);
}

#[test]
fn set_fielders_are_clean_forever() {
    let mut coach = Coach::default();
    let spot = Vec3::new(0.0, 0.0, 50.0);
    for i in 0..100 {
        let t = 40.0 + i as f32 * 0.1;
        let mut s = snap(t, CoachPhase::PrePitch);
        s.fielder_spots = vec![spot];
        s.fielders = vec![FielderFacts {
            index: 0,
            pos: spot,
            move_target: None,
        }];
        assert!(coach.observe(&s).is_empty());
    }
}
