//! Unit tests for [`super`] — the contact module.

use super::super::test_support::*;
use super::super::{hit_spin, hit_velocity, predict_landing};
use super::*;
use crate::game::ball::BALL_DRAG_FACTOR;
use crate::game::variant::BattingTuning;

// ── Classification ────────────────────────────────────────────────────────

#[test]
fn deep_drive_over_the_fence_is_a_home_run() {
    let vel = vel_at(32.0, 50.0);
    let (landing, _) = predict_landing(
        vel,
        hit_spin(vel),
        BALL_DRAG_FACTOR,
        crate::game::ball::MAGNUS_FACTOR,
    );
    assert_eq!(
        classify_contact(landing, &std_field()),
        ContactKind::HomeRun
    );
}

#[test]
fn balls_short_of_the_fence_stay_live() {
    let vel = vel_at(30.0, 30.0);
    let (landing, _) = predict_landing(
        vel,
        hit_spin(vel),
        BALL_DRAG_FACTOR,
        crate::game::ball::MAGNUS_FACTOR,
    );
    assert_eq!(
        classify_contact(landing, &std_field()),
        ContactKind::Live { fair: true }
    );
}

#[test]
fn pulled_way_foul_projects_foul() {
    // Mostly sideways: |x| > z → outside the standard 45° fair wedge.
    let (landing, _) = predict_landing(Vec3::new(30.0, 8.0, 5.0), Vec3::ZERO, 0.0, 0.0);
    assert_eq!(
        classify_contact(landing, &std_field()),
        ContactKind::Live { fair: false }
    );
}

// ── Baserunning reads after contact ───────────────────────────────────────

/// Predicted landing + hang time for a launch angle/speed, run through the
/// same flight model the live ball uses (so the thresholds are calibrated
/// against real trajectories, not hand-picked numbers).
fn flight(launch_deg: f32, speed: f32, spray_deg: f32) -> (Vec3, f32) {
    let vel = vel_spray(launch_deg, speed, spray_deg);
    predict_landing(
        vel,
        hit_spin(vel),
        BALL_DRAG_FACTOR,
        crate::game::ball::MAGNUS_FACTOR,
    )
}

#[test]
fn contact_class_topped_ball_is_a_grounder() {
    // A ball hit nearly flat is on the ground almost at once.
    let (landing, hang) = flight(2.0, 20.0, 0.0);
    assert!(hang < GROUNDER_HANG_SECS, "hang {hang}");
    assert_eq!(
        contact_class(landing, hang, &std_field()),
        ContactClass::Grounder
    );
}

#[test]
fn contact_class_can_of_corn_is_a_catchable_fly() {
    // A high, shallow pop hangs a long time but lands in the infield/short
    // outfield — catchable, but no tag-up value.
    let (landing, hang) = flight(60.0, 24.0, 0.0);
    assert!(hang >= GROUNDER_HANG_SECS, "hang {hang}");
    assert_eq!(
        contact_class(landing, hang, &std_field()),
        ContactClass::CatchableFly
    );
}

#[test]
fn contact_class_deep_drive_is_a_deep_fly() {
    // A long carry to the warning track: catchable, and deep enough that a
    // runner tags up.
    let (landing, hang) = flight(30.0, 40.0, 0.0);
    assert!(hang >= GROUNDER_HANG_SECS, "hang {hang}");
    assert!(
        Vec2::new(landing.x, landing.z).length() >= TAG_UP_MIN_DIST,
        "landing {landing:?}"
    );
    assert_eq!(
        contact_class(landing, hang, &std_field()),
        ContactClass::DeepFly
    );
}

#[test]
fn landed_past_infield_reads_the_infield_gather_radius() {
    let field = std_field();
    // Well short of the infield-gather radius: an infield chopper, not
    // through.
    let shallow = Vec3::new(0.0, 0.0, 10.0);
    assert!(!landed_past_infield(shallow, &field), "shallow {shallow:?}");
    // Comfortably beyond it: a ball through to the outfield grass.
    let deep = Vec3::new(0.0, 0.0, INFIELD_GATHER_RADIUS * field.hit_scale + 5.0);
    assert!(landed_past_infield(deep, &field), "deep {deep:?}");
    // Exactly at the boundary counts as past (>=, matching the gather
    // race's own "infield" cutoff).
    let boundary = Vec3::new(0.0, 0.0, INFIELD_GATHER_RADIUS * field.hit_scale);
    assert!(
        landed_past_infield(boundary, &field),
        "boundary {boundary:?}"
    );
}

#[test]
fn two_outs_everyone_runs_on_contact() {
    for class in [
        ContactClass::Grounder,
        ContactClass::CatchableFly,
        ContactClass::DeepFly,
    ] {
        for forced in [false, true] {
            assert_eq!(runner_break(2, forced, class), RunnerBreak::GoNow);
        }
    }
}

#[test]
fn forced_grounder_goes_and_unforced_grounder_reads() {
    assert_eq!(
        runner_break(0, true, ContactClass::Grounder),
        RunnerBreak::GoNow
    );
    assert_eq!(
        runner_break(1, false, ContactClass::Grounder),
        RunnerBreak::Halfway
    );
}

#[test]
fn catchable_fly_goes_halfway_regardless_of_force() {
    assert_eq!(
        runner_break(0, false, ContactClass::CatchableFly),
        RunnerBreak::Halfway
    );
    assert_eq!(
        runner_break(1, true, ContactClass::CatchableFly),
        RunnerBreak::Halfway
    );
}

#[test]
fn deep_fly_tags_up_with_fewer_than_two_outs() {
    assert_eq!(
        runner_break(0, false, ContactClass::DeepFly),
        RunnerBreak::TagUp
    );
    assert_eq!(
        runner_break(1, true, ContactClass::DeepFly),
        RunnerBreak::TagUp
    );
}

// ── Contact quality ─────────────────────────────────────────────────────

#[test]
fn pci_dead_center_keeps_full_windows() {
    let r = Ruleset {
        batting: BattingTuning {
            perfect_ms: 40.0,
            solid_ms: 90.0,
            foul_ms: 130.0,
            pci_radius_m: 0.20,
            ..std_rules().batting
        },
        ..std_rules()
    };
    assert_eq!(pci_contact_quality(30.0, 0.0, &r), ContactQuality::Perfect);
    assert_eq!(pci_contact_quality(80.0, 0.0, &r), ContactQuality::Solid);
}

#[test]
fn pci_at_radius_perfect_vanishes_and_solid_halves() {
    let r = Ruleset {
        batting: BattingTuning {
            perfect_ms: 40.0,
            solid_ms: 90.0,
            foul_ms: 130.0,
            pci_radius_m: 0.20,
            ..std_rules().batting
        },
        ..std_rules()
    };
    assert_eq!(pci_contact_quality(10.0, 0.20, &r), ContactQuality::Solid); // no Perfect left
    assert_eq!(pci_contact_quality(80.0, 0.20, &r), ContactQuality::Weak); // outside solid/2=45 → clipped
    assert_eq!(pci_contact_quality(40.0, 0.20, &r), ContactQuality::Solid); // inside 45
}

#[test]
fn pci_beyond_radius_caps_at_foul_tip() {
    let r = Ruleset {
        batting: BattingTuning {
            perfect_ms: 40.0,
            solid_ms: 90.0,
            foul_ms: 130.0,
            pci_radius_m: 0.20,
            ..std_rules().batting
        },
        ..std_rules()
    };
    assert_eq!(pci_contact_quality(10.0, 0.35, &r), ContactQuality::FoulTip);
    assert_eq!(pci_contact_quality(200.0, 0.35, &r), ContactQuality::Whiff);
    // timing still whiffs
}

#[test]
fn pci_aim_signs_loft_and_pull() {
    // Cursor UNDER the ball (offset.y negative) undercuts → lofts (aim.y +).
    assert!(pci_aim(Vec2::new(0.0, -0.1)).y > 0.0);
    // Cursor toward +x of the ball: same sense as raw aim.x (the −X pull
    // negation happens inside hit_velocity, exactly as for raw aim).
    assert!(pci_aim(Vec2::new(0.1, 0.0)).x > 0.0);
    // Saturates to the aim domain.
    assert!(pci_aim(Vec2::new(9.0, -9.0)).length() <= std::f32::consts::SQRT_2 + 1e-5);
}

#[test]
fn contact_quality_windows_are_data_driven() {
    // Explicit windows pin the dt→quality *mapping* independent of the
    // shipped Standard tuning: those numbers live in `variant.rs` and are
    // the B7 balance harness's to move (tests/balance_sim.rs), so this test
    // must not double as a snapshot of them.
    let r = Ruleset {
        batting: BattingTuning {
            perfect_ms: 40.0,
            solid_ms: 90.0,
            foul_ms: 140.0,
            ..std_rules().batting
        },
        ..std_rules()
    };
    use ContactQuality::*;
    assert_eq!(contact_quality(0.0, &r), Perfect);
    assert_eq!(contact_quality(-39.9, &r), Perfect);
    assert_eq!(contact_quality(40.1, &r), Solid);
    assert_eq!(contact_quality(-90.0, &r), Solid);
    assert_eq!(contact_quality(90.1, &r), FoulTip);
    assert_eq!(contact_quality(-140.0, &r), FoulTip);
    assert_eq!(contact_quality(140.1, &r), Whiff);
    assert_eq!(contact_quality(999.0, &r), Whiff);
}

#[test]
fn perfect_contact_is_faster_than_solid() {
    let r = std_rules();
    // Identical base vector, different quality: Perfect's exit multiplier
    // must beat Solid's (whatever the shipped tuning sets them to, Perfect
    // is always the harder-hit ball — see `variant.rs`/tests/balance_sim.rs).
    let base = hit_velocity(0.4, Vec2::ZERO);
    let perfect = apply_contact_quality(base, ContactQuality::Perfect, 0.0, &r);
    let solid = apply_contact_quality(base, ContactQuality::Solid, 0.0, &r);
    assert!(perfect.length() > solid.length());
    // Dead-on (dt = 0) leaves the launch *direction* untouched — the
    // quality only scales exit speed, it doesn't add pull yaw at zero
    // timing error. (Magnitude is scaled by the exit multiplier, which the
    // balance tuning may set to anything, so compare directions.)
    assert!((solid.normalize() - base.normalize()).length() < 1e-4);
}

#[test]
fn early_contact_pulls_toward_minus_x() {
    let r = std_rules();
    // A straightaway base vector (aim.x = 0, contact on the plate): purely
    // +Z, no side component.
    let base = hit_velocity(0.4, Vec2::ZERO);
    assert!(base.x.abs() < 1e-4, "base swing must start straightaway");
    // Early (negative dt) is the right-handed batter's pull: −X.
    let early = apply_contact_quality(base, ContactQuality::Solid, -80.0, &r);
    assert!(
        early.x < 0.0,
        "an early swing must pull toward −X (got x = {})",
        early.x
    );
    // Late (positive dt) pushes the opposite way: +X.
    let late = apply_contact_quality(base, ContactQuality::Solid, 80.0, &r);
    assert!(
        late.x > 0.0,
        "a late swing must push toward +X (got x = {})",
        late.x
    );
}
