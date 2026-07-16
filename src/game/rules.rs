//! Pure baseball rules — no ECS systems, no physics engine, fully unit-tested.
//!
//! Everything in this module is a plain function over plain data: given the
//! current count/bases/score and an input (a batted-ball velocity, a called
//! pitch), it mutates the state and reports what happened. `flow.rs` owns the
//! real-time state machine and translates these results into banners and
//! phase transitions; this module owns the *rules of baseball* and the
//! *balance constants* that make the arcade game fair.

use bevy::math::{Vec2, Vec3};
use bevy::prelude::Resource;

use crate::game::ball::BALL_RADIUS;
use crate::game::field::PITCH_DISTANCE;
use crate::game::ScoreBoard;

// ── Tuning constants ──────────────────────────────────────────────────────────

/// Gravity magnitude used for landing-point prediction (matches Rapier default).
pub const GRAVITY: f32 = 9.81;
/// Approximate batted-ball contact height (metres).
const CONTACT_HEIGHT: f32 = 0.6;
/// Scale applied to the drag-free projectile range when predicting where a
/// batted ball lands. Aerodynamic drag accounts for only part of this (at
/// arcade exit speeds `ball::BALL_DRAG_FACTOR` costs ~5-10% of range); the rest
/// is deliberate balance tuning so the hit/out distance bands in
/// [`classify_batted_ball`] produce an arcade-appropriate out-rate. The
/// classifier tests below lock in the resulting bands — retune them together.
const DRAG_RANGE_FACTOR: f32 = 0.73;

/// Nominal pitch speed (m/s) — roughly 85 mph.
pub const PITCH_SPEED: f32 = 38.0;
/// Bias applied to the ballistic gravity-compensation term of a pitch. Values
/// above 1.0 make the pitch arrive *higher* than the drag-free solution — this
/// keeps a centre-aimed pitch comfortably inside the strike zone (verified by
/// `centre_aimed_pitch_is_a_strike` below, which simulates the full flight with
/// the same drag model the live ball uses). Pure balance tuning, not physics.
const PITCH_LOFT_BIAS: f32 = 1.3;

/// Horizontal half-width of the called strike zone (metres from plate centre).
const ZONE_HALF_WIDTH: f32 = 0.34;
/// Vertical strike-zone bounds (metres).
const ZONE_LOW: f32 = 0.5;
const ZONE_HIGH: f32 = 1.45;

/// Where the ball rests before each pitch (top of the mound).
pub fn mound_reset_pos() -> Vec3 {
    Vec3::new(0.0, BALL_RADIUS + 0.25, PITCH_DISTANCE)
}

// ── Data ──────────────────────────────────────────────────────────────────────

/// Occupancy of the three bases. All runners belong to the batting team, so a
/// boolean is enough. Used by base-running rules and the HUD diamond.
#[derive(Resource, Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bases {
    pub first: bool,
    pub second: bool,
    pub third: bool,
}

impl Bases {
    pub fn clear(&mut self) {
        *self = Bases::default();
    }
}

/// Flavour of an out, used only for the on-screen banner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutKind {
    Ground,
    Fly,
    Pop,
}

/// The result of a batted ball.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Foul,
    Out(OutKind),
    Single,
    Double,
    Triple,
    HomeRun,
}

/// What a taken ball did to the count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BallCall {
    /// The count advanced.
    Ball,
    /// Ball four — the batter walked, forcing in `runs` runs.
    Walk { runs: u32 },
}

/// What a strike did to the count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrikeCall {
    /// The count advanced.
    Strike,
    /// Strike three — the batter is out.
    Strikeout,
}

// ── Pitch & contact kinematics ────────────────────────────────────────────────

/// Solves the release velocity for a pitch aimed at plate location
/// `(aim.x, aim.y)` (both in −1.0..=1.0, zero = middle of the zone).
pub fn pitch_velocity(aim: Vec2) -> Vec3 {
    let target_x = aim.x * 0.35;
    let target_y = 1.05 + aim.y * 0.5;

    let start = mound_reset_pos();
    let flight = PITCH_DISTANCE / PITCH_SPEED;
    let vx = (target_x - start.x) / flight;
    let vy = (target_y - start.y) / flight + 0.5 * GRAVITY * (flight * PITCH_LOFT_BIAS);

    Vec3::new(vx, vy, -PITCH_SPEED)
}

/// Converts contact timing + aim into a batted-ball velocity.
///
/// Timing is everything: `contact_z ≈ 0.4` (ball on the plate) is squared-up
/// for a hard line drive, while early contact (ball still out front) skies the
/// ball for a pop-up and late contact tops it for a weak grounder. A tight
/// window means mistimed swings produce catchable balls, keeping the out-rate
/// and inning pace in line with arcade baseball.
pub fn hit_velocity(contact_z: f32, aim: Vec2) -> Vec3 {
    let ideal = 0.4_f32;
    let timing = contact_z - ideal; // >0 early, <0 late
    let quality = (1.0 - timing.abs() / 1.1).clamp(0.08, 1.0);

    let speed = 16.0 + 30.0 * quality;
    // Aim sets the intended launch; mistiming skews it toward pop-up / grounder.
    // A neutral swing (aim.y = 0) is a ~19° line drive — the base hit angle;
    // aiming up trades hittability for home-run power.
    let launch_deg = (6.0 + 26.0 * (aim.y * 0.5 + 0.5) + timing * 8.0).clamp(-8.0, 72.0);
    let launch = launch_deg.to_radians();
    let spray = (aim.x * 0.6 + timing * 0.05).clamp(-0.95, 0.95);

    let horizontal = speed * launch.cos();
    Vec3::new(
        horizontal * spray.sin(),
        speed * launch.sin(),
        horizontal * spray.cos(),
    )
}

/// Is a plate-crossing point (x = horizontal, y = height) a called strike?
pub fn is_in_zone(crossing: Vec2) -> bool {
    crossing.x.abs() <= ZONE_HALF_WIDTH && crossing.y >= ZONE_LOW && crossing.y <= ZONE_HIGH
}

// ── Batted-ball classification ────────────────────────────────────────────────

/// Classifies a batted ball from its launch velocity by predicting where it
/// lands (drag-free projectile range scaled by [`DRAG_RANGE_FACTOR`]).
pub fn classify_batted_ball(vel: Vec3) -> Outcome {
    // Time to return to ground from the contact height.
    let disc = vel.y * vel.y + 2.0 * GRAVITY * CONTACT_HEIGHT;
    let t = (vel.y + disc.sqrt()) / GRAVITY;

    let land = Vec3::new(vel.x, 0.0, vel.z) * t * DRAG_RANGE_FACTOR;
    let dist = land.length();
    let (x, z) = (land.x, land.z);

    // Fair territory is the 45° wedge opening toward +Z (centre field).
    let fair = z > 1.0 && x.abs() <= z + 0.01;
    if !fair {
        return Outcome::Foul;
    }

    let speed = vel.length().max(0.001);
    let launch_deg = (vel.y / speed).asin().to_degrees();

    // Radial fence: down the lines ≈ 100 m, straightaway centre ≈ 122 m.
    let centeredness = (((z / dist) - 0.707) / (1.0 - 0.707)).clamp(0.0, 1.0);
    let fence = 100.0 + 22.0 * centeredness;

    if dist > fence {
        return Outcome::HomeRun;
    }
    // Infield/short pop-ups are caught.
    if launch_deg > 50.0 && dist < 55.0 {
        return Outcome::Out(OutKind::Pop);
    }
    // Most fly balls are run down by the outfield; only the deepest drives to
    // the gaps fall in for extra bases, and the very deepest clear the wall
    // (handled above). Catching routine flies is what keeps the out-rate — and
    // therefore inning pace — in line with arcade baseball.
    if launch_deg > 20.0 && dist < 95.0 {
        return Outcome::Out(OutKind::Fly);
    }
    // Weakly-topped balls are fielded in the infield.
    if dist < 26.0 {
        return Outcome::Out(OutKind::Ground);
    }
    // Line drives (and deep gap flies) split the field by depth.
    if dist < 44.0 {
        Outcome::Single
    } else if dist < 68.0 {
        Outcome::Double
    } else {
        Outcome::Triple
    }
}

// ── Base running ──────────────────────────────────────────────────────────────

/// Advances runners for a clean hit where everyone moves up `hit_bases`.
/// Returns the number of runs that scored.
pub fn advance_hit(bases: &mut Bases, hit_bases: u32) -> u32 {
    let mut runs = 0;
    let mut next = Bases::default();

    let place = |base: u32, runs: &mut u32, next: &mut Bases| {
        let dest = base + hit_bases;
        match dest {
            1 => next.first = true,
            2 => next.second = true,
            3 => next.third = true,
            _ => *runs += 1, // dest >= 4 → scored
        }
    };

    if bases.third {
        place(3, &mut runs, &mut next);
    }
    if bases.second {
        place(2, &mut runs, &mut next);
    }
    if bases.first {
        place(1, &mut runs, &mut next);
    }
    place(0, &mut runs, &mut next); // the batter

    *bases = next;
    runs
}

/// Advances only forced runners for a walk. Returns runs scored (a bases-loaded
/// walk forces in one run).
pub fn advance_walk(bases: &mut Bases) -> u32 {
    if !bases.first {
        bases.first = true;
        0
    } else if !bases.second {
        bases.second = true;
        0
    } else if !bases.third {
        bases.third = true;
        0
    } else {
        1 // bases loaded: everyone forced up one, runner from third scores
    }
}

// ── Count & scoring mutations ─────────────────────────────────────────────────

/// Applies a hit worth `hit_bases` bases: advances runners, credits runs to the
/// batting team, and ends the at-bat. Returns the runs scored.
pub fn apply_hit(score: &mut ScoreBoard, bases: &mut Bases, hit_bases: u32) -> u32 {
    let runs = advance_hit(bases, hit_bases);
    score.add_runs(runs);
    reset_count(score);
    runs
}

/// Records a taken ball. Ball four walks the batter (forcing runners) and ends
/// the at-bat.
pub fn call_ball(score: &mut ScoreBoard, bases: &mut Bases) -> BallCall {
    score.balls += 1;
    if score.balls >= 4 {
        let runs = advance_walk(bases);
        score.add_runs(runs);
        reset_count(score);
        BallCall::Walk { runs }
    } else {
        BallCall::Ball
    }
}

/// Records a strike (called or swinging). Strike three is an out.
pub fn call_strike(score: &mut ScoreBoard, bases: &mut Bases) -> StrikeCall {
    score.strikes += 1;
    if score.strikes >= 3 {
        record_out(score, bases);
        StrikeCall::Strikeout
    } else {
        StrikeCall::Strike
    }
}

/// Records a foul ball: a strike, unless it would be the third.
pub fn foul(score: &mut ScoreBoard) {
    if score.strikes < 2 {
        score.strikes += 1;
    }
}

/// Records an out, ends the at-bat, and flips the half-inning after three.
pub fn record_out(score: &mut ScoreBoard, bases: &mut Bases) {
    reset_count(score);
    score.outs += 1;
    if score.outs >= 3 {
        score.outs = 0;
        bases.clear();
        if score.top_of_inning {
            score.top_of_inning = false;
        } else {
            score.top_of_inning = true;
            score.inning += 1;
        }
    }
}

/// Resets balls and strikes for a new at-bat.
pub fn reset_count(score: &mut ScoreBoard) {
    score.balls = 0;
    score.strikes = 0;
}

// ── Game end ──────────────────────────────────────────────────────────────────

/// Returns `true` if the game is over given the current score and inning count.
pub fn is_game_over(score: &ScoreBoard, innings: u32) -> bool {
    // Home has won (or walked off) once regulation is reached and it leads while
    // batting/entering the bottom half.
    if !score.top_of_inning && score.inning >= innings && score.home_runs > score.away_runs {
        return true;
    }
    // A completed bottom half (we've advanced past regulation) that is not tied.
    if score.top_of_inning && score.inning > innings && score.home_runs != score.away_runs {
        return true;
    }
    false
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::ball::BALL_DRAG_FACTOR;

    fn empty() -> Bases {
        Bases::default()
    }

    fn loaded() -> Bases {
        Bases {
            first: true,
            second: true,
            third: true,
        }
    }

    // ── Base running ──────────────────────────────────────────────────────────

    #[test]
    fn single_puts_batter_on_first() {
        let mut b = empty();
        assert_eq!(advance_hit(&mut b, 1), 0);
        assert_eq!(
            b,
            Bases {
                first: true,
                second: false,
                third: false
            }
        );
    }

    #[test]
    fn single_scores_runner_from_third() {
        let mut b = Bases {
            first: false,
            second: false,
            third: true,
        };
        // Everyone advances one: third scores, batter to first.
        assert_eq!(advance_hit(&mut b, 1), 1);
        assert_eq!(
            b,
            Bases {
                first: true,
                second: false,
                third: false
            }
        );
    }

    #[test]
    fn grand_slam_clears_bases_and_scores_four() {
        let mut b = loaded();
        assert_eq!(advance_hit(&mut b, 4), 4);
        assert_eq!(b, empty());
    }

    #[test]
    fn double_with_runner_on_first() {
        let mut b = Bases {
            first: true,
            second: false,
            third: false,
        };
        // Batter to second, runner from first to third.
        assert_eq!(advance_hit(&mut b, 2), 0);
        assert_eq!(
            b,
            Bases {
                first: false,
                second: true,
                third: true
            }
        );
    }

    #[test]
    fn walk_forces_only_when_bases_ahead_are_occupied() {
        let mut b = empty();
        assert_eq!(advance_walk(&mut b), 0);
        assert!(b.first && !b.second && !b.third);

        // Runner on first: batter forces them to second.
        let mut b = Bases {
            first: true,
            second: false,
            third: false,
        };
        assert_eq!(advance_walk(&mut b), 0);
        assert!(b.first && b.second && !b.third);

        // Bases loaded: forces in a run, still loaded.
        let mut b = loaded();
        assert_eq!(advance_walk(&mut b), 1);
        assert_eq!(b, loaded());
    }

    // ── Count mutations ───────────────────────────────────────────────────────

    #[test]
    fn third_strike_is_a_strikeout_and_an_out() {
        let mut score = ScoreBoard {
            strikes: 2,
            balls: 3,
            ..Default::default()
        };
        let mut bases = empty();
        assert_eq!(call_strike(&mut score, &mut bases), StrikeCall::Strikeout);
        assert_eq!((score.balls, score.strikes), (0, 0)); // fresh count
        assert_eq!(score.outs, 1);
    }

    #[test]
    fn foul_is_a_strike_but_never_the_third() {
        let mut score = ScoreBoard::default();
        foul(&mut score);
        assert_eq!(score.strikes, 1);
        foul(&mut score);
        assert_eq!(score.strikes, 2);
        foul(&mut score); // would be strike three — stays at two
        assert_eq!(score.strikes, 2);
    }

    #[test]
    fn fourth_ball_walks_and_forces_runners() {
        let mut score = ScoreBoard {
            balls: 3,
            strikes: 2,
            top_of_inning: true, // Away bats
            ..Default::default()
        };
        let mut bases = loaded();
        assert_eq!(
            call_ball(&mut score, &mut bases),
            BallCall::Walk { runs: 1 }
        );
        assert_eq!(score.away_runs, 1); // forced run credited to batting team
        assert_eq!((score.balls, score.strikes), (0, 0));
        assert_eq!(bases, loaded()); // still loaded after the force
    }

    #[test]
    fn three_outs_flip_the_half_inning_and_clear_bases() {
        let mut score = ScoreBoard {
            inning: 1,
            top_of_inning: true,
            outs: 2,
            ..Default::default()
        };
        let mut bases = loaded();

        // Third out of the top: flip to the bottom of the same inning.
        record_out(&mut score, &mut bases);
        assert_eq!(score.outs, 0);
        assert!(!score.top_of_inning);
        assert_eq!(score.inning, 1);
        assert_eq!(bases, empty());

        // Third out of the bottom: advance to the top of the next inning.
        score.outs = 2;
        record_out(&mut score, &mut bases);
        assert!(score.top_of_inning);
        assert_eq!(score.inning, 2);
    }

    #[test]
    fn apply_hit_credits_runs_and_resets_the_count() {
        let mut score = ScoreBoard {
            balls: 2,
            strikes: 1,
            top_of_inning: false, // Home bats
            ..Default::default()
        };
        let mut bases = Bases {
            first: false,
            second: true,
            third: false,
        };
        // Double: runner on second scores, batter to second.
        assert_eq!(apply_hit(&mut score, &mut bases, 2), 1);
        assert_eq!(score.home_runs, 1);
        assert_eq!((score.balls, score.strikes), (0, 0));
        assert!(bases.second && !bases.first && !bases.third);
    }

    // ── Classification ────────────────────────────────────────────────────────

    #[test]
    fn home_run_classifies_as_home_run() {
        // Straightaway centre, ~30° launch, high exit speed.
        let launch = 30.0_f32.to_radians();
        let speed = 46.0;
        let vel = Vec3::new(0.0, speed * launch.sin(), speed * launch.cos());
        assert_eq!(classify_batted_ball(vel), Outcome::HomeRun);
    }

    #[test]
    fn straight_up_is_a_pop_out() {
        let vel = Vec3::new(0.0, 20.0, 2.0);
        assert!(matches!(classify_batted_ball(vel), Outcome::Out(_)));
    }

    #[test]
    fn routine_fly_ball_is_caught() {
        // ~30° at a moderate exit speed: a can-of-corn fly, not deep enough to fall.
        let launch = 30.0_f32.to_radians();
        let speed = 30.0;
        let vel = Vec3::new(0.0, speed * launch.sin(), speed * launch.cos());
        assert!(matches!(
            classify_batted_ball(vel),
            Outcome::Out(OutKind::Fly)
        ));
    }

    #[test]
    fn solid_line_drive_is_a_hit() {
        // ~15° liner splits the field for a base hit.
        let launch = 15.0_f32.to_radians();
        let speed = 34.0;
        let vel = Vec3::new(0.0, speed * launch.sin(), speed * launch.cos());
        assert!(matches!(
            classify_batted_ball(vel),
            Outcome::Single | Outcome::Double | Outcome::Triple
        ));
    }

    #[test]
    fn pulled_way_foul_is_foul() {
        // Mostly sideways: |x| > z → outside the fair wedge.
        let vel = Vec3::new(30.0, 8.0, 5.0);
        assert_eq!(classify_batted_ball(vel), Outcome::Foul);
    }

    // ── Pitch flight ──────────────────────────────────────────────────────────

    /// Locks [`PITCH_LOFT_BIAS`] to observable behaviour: simulate the full
    /// pitch flight with the same gravity + quadratic-drag model the live ball
    /// uses (`ball::apply_drag`), and require a centre-aimed pitch to cross the
    /// plate inside the strike zone. If the drag model or the bias changes and
    /// centre pitches become balls, this fails instead of the gameplay quietly
    /// degrading.
    #[test]
    fn centre_aimed_pitch_is_a_strike() {
        let mut pos = mound_reset_pos();
        let mut vel = pitch_velocity(Vec2::ZERO);
        let dt = 1.0 / 240.0;

        while pos.z > 0.0 {
            let speed = vel.length();
            vel += -BALL_DRAG_FACTOR * speed * vel * dt; // same form as apply_drag
            vel.y -= GRAVITY * dt;
            pos += vel * dt;
            assert!(pos.y > 0.0, "pitch hit the ground before the plate");
        }

        assert!(
            is_in_zone(Vec2::new(pos.x, pos.y)),
            "centre-aimed pitch crossed at ({:.2}, {:.2}) — outside the zone",
            pos.x,
            pos.y,
        );
    }

    // ── Game end ──────────────────────────────────────────────────────────────

    #[test]
    fn walkoff_when_home_leads_in_bottom_of_final() {
        let score = ScoreBoard {
            home_runs: 3,
            away_runs: 2,
            inning: 9,
            top_of_inning: false,
            ..Default::default()
        };
        assert!(is_game_over(&score, 9));
    }

    #[test]
    fn tie_after_regulation_goes_to_extras() {
        let score = ScoreBoard {
            home_runs: 2,
            away_runs: 2,
            inning: 10,
            top_of_inning: true,
            ..Default::default()
        };
        assert!(!is_game_over(&score, 9));
    }

    #[test]
    fn away_leads_after_bottom_nine_ends_game() {
        let score = ScoreBoard {
            home_runs: 1,
            away_runs: 4,
            inning: 10,
            top_of_inning: true,
            ..Default::default()
        };
        assert!(is_game_over(&score, 9));
    }
}
