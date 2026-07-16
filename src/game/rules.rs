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
use crate::game::variant::{FieldSpec, Ruleset};
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

/// Maximum launch angle (degrees) at which a fielder can play a ball on the
/// hop and peg the runner. Steeper balls are governed by the catch bands.
const PEG_MAX_LAUNCH_DEG: f32 = 20.0;

/// Where the ball rests before each pitch (top of the mound / rubber).
pub fn mound_reset_pos(pitch_distance: f32) -> Vec3 {
    Vec3::new(0.0, BALL_RADIUS + 0.25, pitch_distance)
}

// ── Data ──────────────────────────────────────────────────────────────────────

/// Occupancy of the bases, in running order (index 0 = first base). All
/// runners belong to the batting team, so a boolean per base is enough. The
/// base count comes from the field variant. Used by base-running rules and
/// the HUD diamond.
#[derive(Resource, Clone, Debug, PartialEq, Eq)]
pub struct Bases {
    occupied: Vec<bool>,
}

impl Default for Bases {
    /// A standard three-base diamond.
    fn default() -> Self {
        Bases::new(3)
    }
}

impl Bases {
    /// Empty bases for a field with `count` bases (excluding home).
    pub fn new(count: usize) -> Self {
        Self {
            occupied: vec![false; count],
        }
    }

    /// Number of bases on this field.
    pub fn count(&self) -> usize {
        self.occupied.len()
    }

    /// Is the (0-indexed) base occupied? Out-of-range reads are just empty.
    pub fn is_occupied(&self, base: usize) -> bool {
        self.occupied.get(base).copied().unwrap_or(false)
    }

    /// Sets one base's occupancy. Out-of-range writes are ignored.
    pub fn set(&mut self, base: usize, occupied: bool) {
        if let Some(slot) = self.occupied.get_mut(base) {
            *slot = occupied;
        }
    }

    /// Empties every base, keeping the base count.
    pub fn clear(&mut self) {
        self.occupied.fill(false);
    }

    /// Empties the bases *and* adopts a (possibly different) base count.
    pub fn reset_for(&mut self, count: usize) {
        self.occupied.clear();
        self.occupied.resize(count, false);
    }
}

/// Flavour of an out, used only for the on-screen banner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutKind {
    Ground,
    Fly,
    Pop,
    /// The runner was hit with the thrown ball (front-yard rules).
    Pegged,
}

/// The result of a batted ball.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Foul,
    Out(OutKind),
    /// A clean hit worth this many bases (1 = single … up to the base count).
    Hit(u32),
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

/// Solves the release velocity for a pitch from `pitch_distance` aimed at
/// plate location `(aim.x, aim.y)` (both in −1.0..=1.0, zero = middle of the
/// zone).
pub fn pitch_velocity(aim: Vec2, pitch_distance: f32) -> Vec3 {
    let target_x = aim.x * 0.35;
    let target_y = 1.05 + aim.y * 0.5;

    let start = mound_reset_pos(pitch_distance);
    let flight = pitch_distance / PITCH_SPEED;
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
/// lands (drag-free projectile range scaled by [`DRAG_RANGE_FACTOR`]) on the
/// given field. The distance bands that decide out/hit tiers scale with
/// [`FieldSpec::hit_scale`] so small parks keep the same arcade balance.
pub fn classify_batted_ball(vel: Vec3, field: &FieldSpec, rules: &Ruleset) -> Outcome {
    // Time to return to ground from the contact height.
    let disc = vel.y * vel.y + 2.0 * GRAVITY * CONTACT_HEIGHT;
    let t = (vel.y + disc.sqrt()) / GRAVITY;

    let land = Vec3::new(vel.x, 0.0, vel.z) * t * DRAG_RANGE_FACTOR;
    let dist = land.length();

    // Fair territory is the wedge opening toward +Z with the field's half-angle.
    let fair = land.z > 1.0 && land.x.abs() <= land.z * field.fair_half_angle.tan() + 0.01;
    if !fair {
        return Outcome::Foul;
    }

    let speed = vel.length().max(0.001);
    let launch_deg = (vel.y / speed).asin().to_degrees();

    // Radial fence, interpolated from the lines to straightaway centre.
    let cos_half = field.fair_half_angle.cos();
    let centeredness = (((land.z / dist) - cos_half) / (1.0 - cos_half)).clamp(0.0, 1.0);
    let fence = field.fence_line + (field.fence_center - field.fence_line) * centeredness;

    if dist > fence {
        return Outcome::HomeRun;
    }

    // Front-yard rules: a low ball landing next to a defender (the pitcher
    // counts too) is played on the hop and thrown at the runner — pegged out.
    // Hitting it right at someone is the cardinal sin of street ball.
    if rules.peg_outs && launch_deg < PEG_MAX_LAUNCH_DEG {
        let pitcher = Vec3::new(0.0, 0.0, field.pitch_distance);
        let pegged = std::iter::once(&pitcher)
            .chain(field.fielder_positions.iter())
            .any(|p| Vec2::new(land.x - p.x, land.z - p.z).length() < field.peg_radius);
        if pegged {
            return Outcome::Out(OutKind::Pegged);
        }
    }

    let s = field.hit_scale;
    // Infield/short pop-ups are caught.
    if launch_deg > 50.0 && dist < 55.0 * s {
        return Outcome::Out(OutKind::Pop);
    }
    // Most fly balls are run down by the defense; only the deepest drives to
    // the gaps fall in for extra bases, and the very deepest clear the fence
    // (handled above). Catching routine flies is what keeps the out-rate — and
    // therefore inning pace — in line with arcade baseball.
    if launch_deg > 20.0 && dist < 95.0 * s {
        return Outcome::Out(OutKind::Fly);
    }
    // Weakly-topped balls are fielded in the infield.
    if dist < 26.0 * s {
        return Outcome::Out(OutKind::Ground);
    }
    // Line drives (and deep gap flies) split the field by depth; fields with
    // more bases add a deeper tier per extra base.
    let mut hit = 1u32;
    let mut boundary = 44.0 * s;
    while (hit as usize) < field.base_count() && dist >= boundary {
        hit += 1;
        boundary += 24.0 * s;
    }
    Outcome::Hit(hit)
}

// ── Base running ──────────────────────────────────────────────────────────────

/// Advances runners for a clean hit where everyone moves up `hit_bases`.
/// `hit_bases` may exceed the base count by one (a home run clears the field
/// and scores the batter). Returns the number of runs that scored.
pub fn advance_hit(bases: &mut Bases, hit_bases: u32) -> u32 {
    debug_assert!(hit_bases >= 1, "a hit is worth at least one base");
    let n = bases.count();
    let step = hit_bases as usize;
    let mut runs = 0;
    let mut next = vec![false; n];

    for base in 0..n {
        if bases.is_occupied(base) {
            let dest = base + step;
            if dest >= n {
                runs += 1; // past the last base → scored
            } else {
                next[dest] = true;
            }
        }
    }
    // The batter reaches base `hit_bases` (1-indexed); one past the last base
    // means they came all the way around.
    if step > n {
        runs += 1;
    } else {
        next[step - 1] = true;
    }

    bases.occupied = next;
    runs
}

/// Advances only forced runners for a walk: the batter takes first and pushes
/// the chain ahead of them. Returns runs scored (a fully-loaded walk forces in
/// one run).
pub fn advance_walk(bases: &mut Bases) -> u32 {
    for base in 0..bases.count() {
        if !bases.is_occupied(base) {
            bases.set(base, true);
            return 0;
        }
    }
    1 // every base occupied: the lead runner is forced home
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

/// Records a taken ball. The final ball walks the batter (forcing runners) and
/// ends the at-bat.
pub fn call_ball(score: &mut ScoreBoard, bases: &mut Bases, rules: &Ruleset) -> BallCall {
    score.balls += 1;
    if score.balls >= rules.balls_per_walk {
        let runs = advance_walk(bases);
        score.add_runs(runs);
        reset_count(score);
        BallCall::Walk { runs }
    } else {
        BallCall::Ball
    }
}

/// Records a strike (called or swinging). The final strike is an out.
pub fn call_strike(score: &mut ScoreBoard, bases: &mut Bases, rules: &Ruleset) -> StrikeCall {
    score.strikes += 1;
    if score.strikes >= rules.strikes_per_out {
        record_out(score, bases, rules);
        StrikeCall::Strikeout
    } else {
        StrikeCall::Strike
    }
}

/// Records a foul ball: a strike, unless it would be the last one.
pub fn foul(score: &mut ScoreBoard, rules: &Ruleset) {
    if score.strikes + 1 < rules.strikes_per_out {
        score.strikes += 1;
    }
}

/// Records an out, ends the at-bat, and flips the half-inning once the side
/// is retired.
pub fn record_out(score: &mut ScoreBoard, bases: &mut Bases, rules: &Ruleset) {
    reset_count(score);
    score.outs += 1;
    if score.outs >= rules.outs_per_half {
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

    use crate::game::variant::VariantId;

    fn std_rules() -> Ruleset {
        VariantId::Standard.rules()
    }

    fn empty() -> Bases {
        Bases::default()
    }

    /// A standard diamond with the given (0-indexed) bases occupied.
    fn with(occupied: &[usize]) -> Bases {
        let mut b = Bases::default();
        for &base in occupied {
            b.set(base, true);
        }
        b
    }

    fn loaded() -> Bases {
        with(&[0, 1, 2])
    }

    // ── Base running ──────────────────────────────────────────────────────────

    #[test]
    fn single_puts_batter_on_first() {
        let mut b = empty();
        assert_eq!(advance_hit(&mut b, 1), 0);
        assert_eq!(b, with(&[0]));
    }

    #[test]
    fn single_scores_runner_from_third() {
        let mut b = with(&[2]);
        // Everyone advances one: third scores, batter to first.
        assert_eq!(advance_hit(&mut b, 1), 1);
        assert_eq!(b, with(&[0]));
    }

    #[test]
    fn grand_slam_clears_bases_and_scores_four() {
        let mut b = loaded();
        assert_eq!(advance_hit(&mut b, 4), 4);
        assert_eq!(b, empty());
    }

    #[test]
    fn double_with_runner_on_first() {
        let mut b = with(&[0]);
        // Batter to second, runner from first to third.
        assert_eq!(advance_hit(&mut b, 2), 0);
        assert_eq!(b, with(&[1, 2]));
    }

    #[test]
    fn walk_forces_only_when_bases_ahead_are_occupied() {
        let mut b = empty();
        assert_eq!(advance_walk(&mut b), 0);
        assert_eq!(b, with(&[0]));

        // Runner on first: batter forces them to second.
        let mut b = with(&[0]);
        assert_eq!(advance_walk(&mut b), 0);
        assert_eq!(b, with(&[0, 1]));

        // Bases loaded: forces in a run, still loaded.
        let mut b = loaded();
        assert_eq!(advance_walk(&mut b), 1);
        assert_eq!(b, loaded());
    }

    #[test]
    fn four_base_walk_chain_only_scores_when_all_full() {
        let mut b = Bases::new(4);
        for expected in [0, 0, 0, 0, 1] {
            assert_eq!(advance_walk(&mut b), expected);
        }
    }

    #[test]
    fn four_base_hit_advancement() {
        let mut b = Bases::new(4);
        // Batter reaches the fourth base without scoring.
        assert_eq!(advance_hit(&mut b, 4), 0);
        assert!(b.is_occupied(3));
        // A five-base homer scores that runner and the batter.
        assert_eq!(advance_hit(&mut b, 5), 2);
        assert_eq!(b, Bases::new(4));
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
        assert_eq!(
            call_strike(&mut score, &mut bases, &std_rules()),
            StrikeCall::Strikeout
        );
        assert_eq!((score.balls, score.strikes), (0, 0)); // fresh count
        assert_eq!(score.outs, 1);
    }

    #[test]
    fn foul_is_a_strike_but_never_the_third() {
        let mut score = ScoreBoard::default();
        foul(&mut score, &std_rules());
        assert_eq!(score.strikes, 1);
        foul(&mut score, &std_rules());
        assert_eq!(score.strikes, 2);
        foul(&mut score, &std_rules()); // would be strike three — stays at two
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
            call_ball(&mut score, &mut bases, &std_rules()),
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
        record_out(&mut score, &mut bases, &std_rules());
        assert_eq!(score.outs, 0);
        assert!(!score.top_of_inning);
        assert_eq!(score.inning, 1);
        assert_eq!(bases, empty());

        // Third out of the bottom: advance to the top of the next inning.
        score.outs = 2;
        record_out(&mut score, &mut bases, &std_rules());
        assert!(score.top_of_inning);
        assert_eq!(score.inning, 2);
    }

    #[test]
    fn custom_out_threshold_flips_half_inning() {
        let rules = Ruleset {
            outs_per_half: 4,
            ..std_rules()
        };
        let mut score = ScoreBoard {
            inning: 1,
            top_of_inning: true,
            outs: 2,
            ..Default::default()
        };
        let mut bases = empty();
        // Out three of four: the half-inning continues.
        record_out(&mut score, &mut bases, &rules);
        assert_eq!((score.outs, score.top_of_inning), (3, true));
        // Out four retires the side.
        record_out(&mut score, &mut bases, &rules);
        assert_eq!((score.outs, score.top_of_inning), (0, false));
    }

    #[test]
    fn apply_hit_credits_runs_and_resets_the_count() {
        let mut score = ScoreBoard {
            balls: 2,
            strikes: 1,
            top_of_inning: false, // Home bats
            ..Default::default()
        };
        let mut bases = with(&[1]);
        // Double: runner on second scores, batter to second.
        assert_eq!(apply_hit(&mut score, &mut bases, 2), 1);
        assert_eq!(score.home_runs, 1);
        assert_eq!((score.balls, score.strikes), (0, 0));
        assert_eq!(bases, with(&[1]));
    }

    // ── Classification ────────────────────────────────────────────────────────

    fn std_field() -> FieldSpec {
        VariantId::Standard.field()
    }

    /// Straightaway-centre launch velocity from angle (degrees) and speed.
    fn vel_at(launch_deg: f32, speed: f32) -> Vec3 {
        vel_spray(launch_deg, speed, 0.0)
    }

    /// Launch velocity sprayed `spray_deg` degrees off the centre-field axis.
    fn vel_spray(launch_deg: f32, speed: f32, spray_deg: f32) -> Vec3 {
        let launch = launch_deg.to_radians();
        let spray = spray_deg.to_radians();
        let horizontal = speed * launch.cos();
        Vec3::new(
            horizontal * spray.sin(),
            speed * launch.sin(),
            horizontal * spray.cos(),
        )
    }

    #[test]
    fn home_run_classifies_as_home_run() {
        // Straightaway centre, ~30° launch, high exit speed.
        assert_eq!(
            classify_batted_ball(vel_at(30.0, 46.0), &std_field(), &std_rules()),
            Outcome::HomeRun
        );
    }

    #[test]
    fn straight_up_is_a_pop_out() {
        let vel = Vec3::new(0.0, 20.0, 2.0);
        assert!(matches!(
            classify_batted_ball(vel, &std_field(), &std_rules()),
            Outcome::Out(_)
        ));
    }

    #[test]
    fn routine_fly_ball_is_caught() {
        // ~30° at a moderate exit speed: a can-of-corn fly, not deep enough to fall.
        assert!(matches!(
            classify_batted_ball(vel_at(30.0, 30.0), &std_field(), &std_rules()),
            Outcome::Out(OutKind::Fly)
        ));
    }

    #[test]
    fn solid_line_drive_is_a_hit() {
        // ~15° liner splits the field for a base hit.
        assert!(matches!(
            classify_batted_ball(vel_at(15.0, 34.0), &std_field(), &std_rules()),
            Outcome::Hit(1..=3)
        ));
    }

    #[test]
    fn pulled_way_foul_is_foul() {
        // Mostly sideways: |x| > z → outside the standard 45° fair wedge.
        let vel = Vec3::new(30.0, 8.0, 5.0);
        assert_eq!(
            classify_batted_ball(vel, &std_field(), &std_rules()),
            Outcome::Foul
        );
    }

    // ── Front-yard classification ─────────────────────────────────────────────

    fn yard() -> (FieldSpec, Ruleset) {
        (VariantId::FrontYard.field(), VariantId::FrontYard.rules())
    }

    #[test]
    fn peg_out_low_liner_lands_near_fielder() {
        let (f, r) = yard();
        // A flat ~10° liner up the middle lands a couple of metres from the
        // mid-yard pitcher — beaned.
        assert_eq!(
            classify_batted_ball(vel_at(10.0, 20.0), &f, &r),
            Outcome::Out(OutKind::Pegged)
        );
    }

    #[test]
    fn same_ball_without_peg_rule_is_a_hit() {
        let (f, mut r) = yard();
        r.peg_outs = false;
        assert!(matches!(
            classify_batted_ball(vel_at(10.0, 20.0), &f, &r),
            Outcome::Hit(_)
        ));
    }

    #[test]
    fn high_fly_over_fielder_is_not_pegged() {
        let (f, r) = yard();
        // Steep flies are governed by the catch bands, never the peg rule.
        let out = classify_batted_ball(vel_at(45.0, 16.0), &f, &r);
        assert!(!matches!(out, Outcome::Out(OutKind::Pegged)));
    }

    #[test]
    fn four_base_field_can_yield_hit_four() {
        let (f, r) = yard();
        // A hard low liner into the right-field gap: deep enough for the
        // fourth tier, far from every fielder, under the fence.
        assert_eq!(
            classify_batted_ball(vel_spray(15.0, 33.0, 30.0), &f, &r),
            Outcome::Hit(4)
        );
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
        let pitch_distance = std_field().pitch_distance;
        let mut pos = mound_reset_pos(pitch_distance);
        let mut vel = pitch_velocity(Vec2::ZERO, pitch_distance);
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
