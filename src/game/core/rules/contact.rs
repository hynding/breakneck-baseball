//! Classifying batted-ball contact: fair/foul/home-run, the baserunning
//! read off contact, and swing-timing contact quality.

use bevy::math::{Vec2, Vec3};

use crate::game::variant::{FieldSpec, Ruleset};

use super::{INFIELD_GATHER_RADIUS, TAG_UP_MIN_DIST, fence_at, is_fair};

// ── Live-play resolution ──────────────────────────────────────────────────────

/// What contact alone settles. Everything except a ball over the fence stays
/// live: the fielders' chase and the runner races decide the rest during the
/// play, not at the crack of the bat.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ContactKind {
    HomeRun,
    Live {
        /// Whether the *predicted* landing is fair — cosmetic hint only; the
        /// actual call comes from where the ball really comes down.
        fair: bool,
    },
}

/// Classifies contact from the live-model predicted `landing` point (see
/// [`predict_landing`]).
pub fn classify_contact(landing: Vec3, field: &FieldSpec) -> ContactKind {
    let fair = is_fair(landing, field);
    let dist = Vec2::new(landing.x, landing.z).length();
    if fair && dist > fence_at(landing, field) {
        return ContactKind::HomeRun;
    }
    ContactKind::Live { fair }
}

// ── Baserunning reads after contact ───────────────────────────────────────────
// Pure, deterministic reads that drive *when the runner rigs break* off contact
// (never the call — the outcome still comes from the live-play races). Encodes
// the real-baseball conventions documented in docs/BASEBALL.md
// ("Baserunning after contact").

/// A fly ball hanging at least this long (seconds) is airborne long enough to
/// be a catch read; anything quicker is a grounder / hard liner that will be
/// on the ground before a runner has to commit. Calibrated against
/// [`predict_landing`] hang times (see the `contact_class_*` unit tests).
const GROUNDER_HANG_SECS: f32 = 1.2;

/// The shape of a fair batted ball as the runners read it off the bat — the
/// distinction that decides how each aboard runner breaks. Derived purely from
/// the predicted flight (hang time + landing distance); no RNG.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContactClass {
    /// On the ground (or a liner already down) before the runner must commit.
    Grounder,
    /// A fly that hangs long enough to be caught but is shallow enough that a
    /// tag-up would gain nothing — the "may be a fly out" read.
    CatchableFly,
    /// A deep fly: catchable, but far enough that tagging up can advance a
    /// runner (the sacrifice-fly distance, [`TAG_UP_MIN_DIST`]).
    DeepFly,
}

/// How a base runner breaks off contact. Purely a *choreography* decision —
/// the rigs move on this while the umpire's call is still being raced out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunnerBreak {
    /// Break for the next bag immediately (run on contact).
    GoNow,
    /// Advance about halfway and read the play: continue if the ball drops,
    /// retreat if it is caught.
    Halfway,
    /// Hold the bag to tag up, advancing legally after the catch.
    TagUp,
}

/// Classifies a fair batted ball for the baserunning read, from the predicted
/// `landing` point and `hang_time` (see [`predict_landing`]). Per
/// docs/BASEBALL.md "Baserunning after contact": a quick ball is a grounder,
/// a long hang deep enough for a sacrifice fly is a deep fly, everything else
/// in between is a catchable fly.
pub fn contact_class(landing: Vec3, hang_time: f32, field: &FieldSpec) -> ContactClass {
    if hang_time < GROUNDER_HANG_SECS {
        return ContactClass::Grounder;
    }
    let dist = Vec2::new(landing.x, landing.z).length();
    if dist >= TAG_UP_MIN_DIST * field.hit_scale {
        ContactClass::DeepFly
    } else {
        ContactClass::CatchableFly
    }
}

/// The break a runner takes off contact, per docs/BASEBALL.md "Baserunning
/// after contact":
/// 1. Two outs — run on contact (nothing to lose).
/// 2. Fewer than two outs, ground ball — a forced runner goes immediately; an
///    unforced runner reads whether it gets through (breaks halfway).
/// 3. Fewer than two outs, catchable fly — go halfway and read the catch.
/// 4. Fewer than two outs, deep fly — tag up.
///
/// Deterministic and RNG-free; the actual call still comes from the live-play
/// races, so this only governs *when the rig moves*, never the outcome.
pub fn runner_break(outs: u32, forced: bool, contact: ContactClass) -> RunnerBreak {
    if outs >= 2 {
        return RunnerBreak::GoNow;
    }
    match contact {
        ContactClass::Grounder if forced => RunnerBreak::GoNow,
        ContactClass::Grounder => RunnerBreak::Halfway,
        ContactClass::CatchableFly => RunnerBreak::Halfway,
        ContactClass::DeepFly => RunnerBreak::TagUp,
    }
}

/// Whether a fair ball's first-bounce landing is past the infield — the
/// "does it get through" read a `Halfway` runner (see [`runner_break`]) makes
/// off a ground ball or catchable fly that hits the dirt/grass instead of a
/// glove. Reuses [`INFIELD_GATHER_RADIUS`] (scaled by `field.hit_scale`), the
/// same infield-range radius the live-throw race already treats as "an out at
/// first is only contested on infield balls" — a landing at or beyond it is
/// through the infield for the same reason a gather out there is a lost
/// cause for the defense. Per docs/BASEBALL.md "Baserunning after contact".
pub fn landed_past_infield(landing: Vec3, field: &FieldSpec) -> bool {
    Vec2::new(landing.x, landing.z).length() >= INFIELD_GATHER_RADIUS * field.hit_scale
}

// ── Contact quality ───────────────────────────────────────────────────────────
// The batting-feel spine (docs/superpowers/specs/2026-07-30-batting-feel-design.md
// §2): a swing's outcome is graded by how far off dead-on timing it lands,
// not by contact-or-miss alone.

/// How well-timed a swing was, from a whiff to a dead-on hit. Graded by
/// [`contact_quality`] against the [`Ruleset`] timing windows.
///
/// `Weak` is never produced by [`contact_quality`] — the Classic timing
/// windows below only ever yield the other four variants. It exists so the
/// Plan-C PCI (plate-coverage-indicator) adapter, which grades contact by a
/// shrunk timing window instead, has a quality to report without widening
/// this enum later; keep matches on it exhaustive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContactQuality {
    /// No contact: the swing missed entirely.
    Whiff,
    /// Contact, but late/early enough it only ever fouls off.
    FoulTip,
    /// Weak contact (Plan-C PCI adapter only — see the enum doc comment).
    Weak,
    /// Solidly timed contact.
    Solid,
    /// Dead-on timing.
    Perfect,
}

/// Grades a swing's timing error (`dt_ms`, milliseconds, signed so early
/// swings are negative) into a [`ContactQuality`] using the active
/// [`Ruleset`]'s windows (`perfect_ms`/`solid_ms`/`foul_ms`). Symmetric
/// around zero: an early and a late swing of the same magnitude grade the
/// same. Never yields `ContactQuality::Weak` — see that variant's doc
/// comment.
pub fn contact_quality(dt_ms: f32, rules: &Ruleset) -> ContactQuality {
    let dt = dt_ms.abs();
    if dt <= rules.batting.perfect_ms {
        ContactQuality::Perfect
    } else if dt <= rules.batting.solid_ms {
        ContactQuality::Solid
    } else if dt <= rules.batting.foul_ms {
        ContactQuality::FoulTip
    } else {
        ContactQuality::Whiff
    }
}

/// Shapes a batted-ball velocity by how well the swing was timed: scales the
/// exit speed by the quality's multiplier and rotates the launch toward the
/// pull side by `pull_yaw_per_ms · dt_ms`.
///
/// `base` is the raw vector from [`hit_velocity`] (aim + contact-point launch);
/// `dt_ms` is the signed swing timing (early = negative — see
/// `flow::swing_dt_ms`). The yaw is applied about the vertical (Y) axis in the
/// same sense as [`hit_velocity`]'s `spray` angle, where a *negative* horizontal
/// component is world −X. First base is at −X and `aim.x` is negated in the
/// hit mapping (see CLAUDE.md), so −X is the right-handed batter's pull side:
/// an early (negative `dt_ms`) swing yields a negative yaw that rotates the ball
/// toward −X, i.e. pulls it. Late (positive) contact pushes the other way.
///
/// `Whiff`/`FoulTip` never reach here (they put no ball in play); they return
/// `base` unchanged so the match stays exhaustive.
pub fn apply_contact_quality(
    base: Vec3,
    quality: ContactQuality,
    dt_ms: f32,
    rules: &Ruleset,
) -> Vec3 {
    let exit_mult = match quality {
        ContactQuality::Perfect => rules.batting.exit_perfect,
        ContactQuality::Solid => rules.batting.exit_solid,
        ContactQuality::Weak => rules.batting.exit_weak,
        ContactQuality::Whiff | ContactQuality::FoulTip => return base,
    };
    let scaled = base * exit_mult;
    // Rotate the horizontal (x, z) launch about +Y by the pull yaw. Matching
    // `hit_velocity`'s spray convention (x = h·sin θ, z = h·cos θ), a positive
    // yaw increases θ (toward +X) and a negative yaw decreases it (toward −X).
    let yaw = rules.batting.pull_yaw_per_ms * dt_ms;
    let (s, c) = yaw.sin_cos();
    Vec3::new(
        scaled.x * c + scaled.z * s,
        scaled.y,
        scaled.z * c - scaled.x * s,
    )
}

/// PCI contact grading (spec §3): the timing windows shrink linearly with the
/// cursor's miss distance. `frac = miss/radius`; effective perfect =
/// `perfect_ms·(1−frac)` (0 at the radius), effective solid =
/// `solid_ms·(1−frac/2)` (halved at the radius). Timing inside the FULL solid
/// window but outside the shrunk one is clipped contact → `Weak` (the only
/// source of Weak in the game). Beyond the radius the bat's sweet spot never
/// reaches the ball: best case FoulTip on timing alone.
pub fn pci_contact_quality(dt_ms: f32, miss_m: f32, rules: &Ruleset) -> ContactQuality {
    let dt = dt_ms.abs();
    if dt > rules.batting.foul_ms {
        return ContactQuality::Whiff;
    }
    let frac = (miss_m / rules.batting.pci_radius_m).max(0.0);
    if frac > 1.0 {
        return ContactQuality::FoulTip;
    }
    let perfect_eff = rules.batting.perfect_ms * (1.0 - frac);
    let solid_eff = rules.batting.solid_ms * (1.0 - frac / 2.0);
    if dt <= perfect_eff {
        ContactQuality::Perfect
    } else if dt <= solid_eff {
        ContactQuality::Solid
    } else if dt <= rules.batting.solid_ms {
        ContactQuality::Weak
    } else {
        ContactQuality::FoulTip
    }
}

/// PCI hit direction (spec §3): derived from the contact-point offset, not
/// raw aim. Normalized against the cursor-radius scale so a half-radius miss
/// is a half-strength aim; components clamp to the aim domain. Signs: cursor
/// under the ball lofts (+y); the x component keeps raw-aim's sense (the −X
/// pull negation lives in `hit_velocity`, per CLAUDE.md).
pub fn pci_aim(offset: Vec2) -> Vec2 {
    const PCI_AIM_SCALE_M: f32 = 0.20;
    Vec2::new(
        (offset.x / PCI_AIM_SCALE_M).clamp(-1.0, 1.0),
        (-offset.y / PCI_AIM_SCALE_M).clamp(-1.0, 1.0),
    )
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
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
}
