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

/// Nominal fastball speed (m/s) — roughly 85 mph.
pub const PITCH_SPEED: f32 = 38.0;

/// Horizontal half-width of the called strike zone (metres from plate centre).
const ZONE_HALF_WIDTH: f32 = 0.34;
/// Vertical strike-zone bounds (metres).
const ZONE_LOW: f32 = 0.5;
const ZONE_HIGH: f32 = 1.45;

/// Maximum launch angle (degrees) at which a fielder can play a ball on the
/// hop and peg the runner. Steeper balls are governed by the catch bands.
const PEG_MAX_LAUNCH_DEG: f32 = 20.0;

/// A caught fly at least this far out (scaled by [`FieldSpec::hit_scale`])
/// gives runners time to tag up and advance.
const TAG_UP_MIN_DIST: f32 = 65.0;

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

/// Flavour of an out. `Fly::deep` also drives the tag-up rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutKind {
    Ground,
    Fly {
        /// Deep enough for runners to tag up (see [`TAG_UP_MIN_DIST`]).
        deep: bool,
    },
    Pop,
    /// A pop-up caught in foul territory.
    FoulPop,
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
    /// Strike three got away from the catcher and the batter beat the play
    /// to first: no out, fresh count for the next batter.
    DroppedThird,
}

// ── Pitch & contact kinematics ────────────────────────────────────────────────

/// The pitcher's arsenal. Speeds in m/s; spin in rad/s about world axes for a
/// −Z pitch: +X is backspin (Magnus lift), −X topspin (dive), ±Y sweep.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PitchKind {
    Fastball,
    Curveball,
    Changeup,
}

impl PitchKind {
    pub fn speed(self) -> f32 {
        match self {
            PitchKind::Fastball => PITCH_SPEED,
            PitchKind::Curveball => 31.0,
            PitchKind::Changeup => 29.0,
        }
    }

    pub fn spin(self) -> Vec3 {
        match self {
            PitchKind::Fastball => Vec3::new(20.0, 0.0, 0.0),
            PitchKind::Curveball => Vec3::new(-18.0, 6.0, 0.0),
            PitchKind::Changeup => Vec3::new(6.0, 0.0, 0.0),
        }
    }

    /// Held aim at release selects the pitch (up = fastball, down = curveball,
    /// neutral = changeup). Aim keeps steering location too — aiming high
    /// *means* throwing the heater upstairs.
    pub fn from_aim(aim: Vec2) -> PitchKind {
        if aim.y > 0.35 {
            PitchKind::Fastball
        } else if aim.y < -0.35 {
            PitchKind::Curveball
        } else {
            PitchKind::Changeup
        }
    }
}

/// Solves the ballistic release velocity for a pitch of `kind` from
/// `pitch_distance` aimed at plate location `(aim.x, aim.y)` (both in
/// −1.0..=1.0, zero = middle of the zone). Deliberately gravity-only: the
/// kind's spin then bends the flight (fastballs ride, curveballs dive), so a
/// pitch's character *is* its physics.
pub fn pitch_velocity_kind(kind: PitchKind, aim: Vec2, pitch_distance: f32) -> Vec3 {
    // Wide enough that a full-inside aim reaches the batter's body — painting
    // the inside corner risks a hit-by-pitch.
    let target_x = aim.x * 0.6;
    let target_y = 1.05 + aim.y * 0.5;
    let speed = kind.speed();

    let start = mound_reset_pos(pitch_distance);
    let flight = pitch_distance / speed;
    let vx = (target_x - start.x) / flight;
    let vy = (target_y - start.y) / flight + 0.5 * GRAVITY * flight;

    Vec3::new(vx, vy, -speed)
}

/// Spin imparted by the bat: sidespin toward the spray side plus mild
/// backspin (−X lifts a +Z batted ball). Single source of truth — the live
/// ball and the landing predictor both use it.
pub fn hit_spin(vel: Vec3) -> Vec3 {
    Vec3::new(-6.0, vel.x.signum() * vel.length() * 0.25, 0.0)
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

/// Inner edge of the batter's body window; he stands at x ≈ +0.7 (see
/// `player.rs`).
const BATTER_X_MIN: f32 = 0.52;
/// Above this the pitch sails over the batter's head.
const BATTER_Y_MAX: f32 = 1.7;

/// Does a plate-crossing point plunk the batter? Only meaningful on a take —
/// swinging at the pitch negates a hit-by-pitch, as in the rulebook.
pub fn hits_batter(crossing: Vec2) -> bool {
    crossing.x >= BATTER_X_MIN && crossing.y > 0.0 && crossing.y <= BATTER_Y_MAX
}

/// Awards first base after a hit-by-pitch: dead ball, forced runners only.
/// Returns runs forced in.
pub fn hit_by_pitch(score: &mut ScoreBoard, bases: &mut Bases) -> u32 {
    let runs = advance_walk(bases);
    score.add_runs(runs);
    reset_count(score);
    runs
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

    let speed = vel.length().max(0.001);
    let launch_deg = (vel.y / speed).asin().to_degrees();
    let s = field.hit_scale;

    // Towering infield pop-ups are caught, fair or foul.
    if launch_deg > 50.0 && dist < 55.0 * s {
        return Outcome::Out(if fair { OutKind::Pop } else { OutKind::FoulPop });
    }
    if !fair {
        return Outcome::Foul;
    }

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

    // Most fly balls are run down by the defense; only the deepest drives to
    // the gaps fall in for extra bases, and the very deepest clear the fence
    // (handled above). Catching routine flies is what keeps the out-rate — and
    // therefore inning pace — in line with arcade baseball.
    if launch_deg > 20.0 && dist < 95.0 * s {
        return Outcome::Out(OutKind::Fly {
            deep: dist >= TAG_UP_MIN_DIST * s,
        });
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

/// Numerically integrates a batted ball's flight from contact height with the
/// same gravity + drag + Magnus model the live ball uses (`ball::apply_drag`,
/// `ball::apply_magnus`), returning the landing point (y = 0) and hang time.
/// This is what fielder choreography chases — the *visual* ball's touchdown,
/// not the balance-tuned range in [`classify_batted_ball`].
pub fn predict_landing(vel: Vec3, spin: Vec3, drag_factor: f32, magnus_factor: f32) -> (Vec3, f32) {
    let mut pos = Vec3::new(0.0, CONTACT_HEIGHT, 0.0);
    let mut v = vel;
    let dt = 1.0 / 120.0;
    let mut t = 0.0;
    while pos.y > 0.0 && t < 15.0 {
        let speed = v.length();
        v += -drag_factor * speed * v * dt;
        v += magnus_factor * spin.cross(v) * dt;
        v.y -= GRAVITY * dt;
        pos += v * dt;
        t += dt;
    }
    (Vec3::new(pos.x, 0.0, pos.z), t)
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

/// Records a strike (called or swinging). The final strike is an out —
/// unless `dropped_third` (the ball got away and first base was open), in
/// which case the batter reaches and no out is recorded.
pub fn call_strike(
    score: &mut ScoreBoard,
    bases: &mut Bases,
    rules: &Ruleset,
    dropped_third: bool,
) -> StrikeCall {
    score.strikes += 1;
    if score.strikes >= rules.strikes_per_out {
        if dropped_third {
            reset_count(score);
            bases.set(0, true);
            StrikeCall::DroppedThird
        } else {
            record_out(score, bases, rules);
            StrikeCall::Strikeout
        }
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

/// Charges one out *without* ending the at-bat (a runner retired on the
/// bases). Flips the half-inning once the side is retired — which also wipes
/// the count, since the interrupted batter starts over next half.
pub fn charge_out(score: &mut ScoreBoard, bases: &mut Bases, rules: &Ruleset) {
    score.outs += 1;
    if score.outs >= rules.outs_per_half {
        score.outs = 0;
        reset_count(score);
        bases.clear();
        if score.top_of_inning {
            score.top_of_inning = false;
        } else {
            score.top_of_inning = true;
            score.inning += 1;
        }
    }
}

/// Records an out that ends the at-bat, flipping the half-inning once the
/// side is retired.
pub fn record_out(score: &mut ScoreBoard, bases: &mut Bases, rules: &Ruleset) {
    reset_count(score);
    charge_out(score, bases, rules);
}

/// The base-running consequences of a batted-ball out.
pub struct OutPlay {
    /// Outs recorded on the play (1, or 2 for double plays / doubled-off).
    pub outs: u32,
    /// Runs that scored (sacrifice flies, runs crossing on a non-ending play).
    pub runs: u32,
    /// The classic force-and-relay two outs.
    pub double_play: bool,
    /// A sent runner was caught off base when the ball was caught.
    pub doubled_off: bool,
}

/// Applies a batted-ball out with its base-running consequences.
/// `runners_going` is the hit-and-run flag: runners broke with the pitch, so
/// grounders can't be turned two, but a caught ball doubles the runner off
/// and nobody tags up.
pub fn apply_batted_out(
    score: &mut ScoreBoard,
    bases: &mut Bases,
    rules: &Ruleset,
    kind: OutKind,
    runners_going: bool,
) -> OutPlay {
    let outs_left = rules.outs_per_half.saturating_sub(score.outs);
    let mut play = OutPlay {
        outs: 1,
        runs: 0,
        double_play: false,
        doubled_off: false,
    };
    match kind {
        OutKind::Ground => {
            if !runners_going && bases.is_occupied(0) && outs_left >= 2 {
                // The force at second plus the relay to first: two outs.
                bases.set(0, false);
                play.outs = 2;
                play.double_play = true;
            }
            // Unless the play ends the inning, the defense took the sure
            // out(s) and everyone else moved up a base.
            if play.outs < outs_left {
                play.runs = advance_trailing(bases);
            }
        }
        OutKind::Fly { deep } => {
            if runners_going {
                play.doubled_off = double_off_lead_runner(bases);
            } else if deep && outs_left > 1 {
                play.runs = tag_up(bases);
            }
        }
        OutKind::Pop | OutKind::FoulPop => {
            if runners_going {
                play.doubled_off = double_off_lead_runner(bases);
            }
        }
        OutKind::Pegged => {}
    }
    if play.doubled_off {
        play.outs += 1;
    }
    score.add_runs(play.runs);
    reset_count(score);
    for _ in 0..play.outs {
        charge_out(score, bases, rules);
    }
    play
}

/// After the batter is retired on the ground, every runner advances one base
/// (the defense takes the sure out at first). Returns runs forced across.
fn advance_trailing(bases: &mut Bases) -> u32 {
    let n = bases.count();
    let mut runs = 0;
    // Walk from the lead base down so nobody leapfrogs.
    for base in (0..n).rev() {
        if bases.is_occupied(base) {
            bases.set(base, false);
            if base + 1 >= n {
                runs += 1;
            } else {
                bases.set(base + 1, true);
            }
        }
    }
    runs
}

/// Tag-up on a deep fly: the runner on the last base scores and the runner
/// one behind moves up. Trailing runners hold. Returns runs scored.
fn tag_up(bases: &mut Bases) -> u32 {
    let n = bases.count();
    let mut runs = 0;
    if n >= 1 && bases.is_occupied(n - 1) {
        bases.set(n - 1, false);
        runs += 1;
    }
    if n >= 2 && bases.is_occupied(n - 2) {
        bases.set(n - 2, false);
        bases.set(n - 1, true);
    }
    runs
}

/// The runner who breaks on a steal or hit-and-run: the lead runner whose
/// next base is open (home can never be stolen here).
pub fn steal_candidate(bases: &Bases) -> Option<usize> {
    let n = bases.count();
    (0..n.saturating_sub(1))
        .rev()
        .find(|&b| bases.is_occupied(b) && !bases.is_occupied(b + 1))
}

/// Removes the runner who was sent, caught off base when the ball was
/// caught. Returns whether anyone was actually going.
fn double_off_lead_runner(bases: &mut Bases) -> bool {
    if let Some(runner) = steal_candidate(bases) {
        bases.set(runner, false);
        true
    } else {
        false
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
            call_strike(&mut score, &mut bases, &std_rules(), false),
            StrikeCall::Strikeout
        );
        assert_eq!((score.balls, score.strikes), (0, 0)); // fresh count
        assert_eq!(score.outs, 1);
    }

    #[test]
    fn dropped_third_strike_puts_the_batter_on_first() {
        let mut score = ScoreBoard {
            strikes: 2,
            ..Default::default()
        };
        let mut bases = empty();
        assert_eq!(
            call_strike(&mut score, &mut bases, &std_rules(), true),
            StrikeCall::DroppedThird
        );
        assert_eq!(score.outs, 0); // the batter reached — no out
        assert_eq!((score.balls, score.strikes), (0, 0));
        assert_eq!(bases, with(&[0]));
    }

    #[test]
    fn dropped_flag_before_strike_three_is_a_plain_strike() {
        let mut score = ScoreBoard::default();
        let mut bases = empty();
        assert_eq!(
            call_strike(&mut score, &mut bases, &std_rules(), true),
            StrikeCall::Strike
        );
        assert_eq!(score.strikes, 1);
        assert_eq!(bases, empty());
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

    // ── Batted-ball outs with base-running consequences ───────────────────────

    /// A fresh bottom-half scoreboard with the given outs.
    fn batting_home(outs: u32) -> ScoreBoard {
        ScoreBoard {
            inning: 1,
            top_of_inning: false,
            outs,
            ..Default::default()
        }
    }

    #[test]
    fn ground_ball_with_runner_on_first_turns_two() {
        let mut score = batting_home(0);
        let mut bases = with(&[0]);
        let play = apply_batted_out(&mut score, &mut bases, &std_rules(), OutKind::Ground, false);
        assert!(play.double_play);
        assert_eq!(play.outs, 2);
        assert_eq!(score.outs, 2);
        assert_eq!(bases, empty()); // both the batter and the forced runner are gone
    }

    #[test]
    fn double_play_still_scores_the_runner_from_third() {
        // 6-4-3 with runners on the corners and nobody out: the run counts
        // because the inning does not end on the play.
        let mut score = batting_home(0);
        let mut bases = with(&[0, 2]);
        let play = apply_batted_out(&mut score, &mut bases, &std_rules(), OutKind::Ground, false);
        assert!(play.double_play);
        assert_eq!(play.runs, 1);
        assert_eq!(score.home_runs, 1);
        assert_eq!(bases, empty());
    }

    #[test]
    fn inning_ending_double_play_scores_nothing() {
        // One out, runners on the corners: the DP is outs two and three — the
        // force ends the inning and the run never counts.
        let mut score = batting_home(1);
        let mut bases = with(&[0, 2]);
        let play = apply_batted_out(&mut score, &mut bases, &std_rules(), OutKind::Ground, false);
        assert!(play.double_play);
        assert_eq!(play.runs, 0);
        assert_eq!(score.home_runs, 0);
        assert!(score.top_of_inning); // half flipped
        assert_eq!(score.inning, 2);
    }

    #[test]
    fn no_double_play_with_two_outs() {
        let mut score = batting_home(2);
        let mut bases = with(&[0]);
        let play = apply_batted_out(&mut score, &mut bases, &std_rules(), OutKind::Ground, false);
        assert!(!play.double_play);
        assert_eq!(play.outs, 1);
        assert!(score.top_of_inning); // routine third out retires the side
    }

    #[test]
    fn routine_ground_out_advances_the_runners() {
        // Runner on second, nobody out: the defense takes the out at first
        // and the runner moves up to third.
        let mut score = batting_home(0);
        let mut bases = with(&[1]);
        let play = apply_batted_out(&mut score, &mut bases, &std_rules(), OutKind::Ground, false);
        assert_eq!((play.outs, play.runs), (1, 0));
        assert_eq!(bases, with(&[2]));
    }

    #[test]
    fn deep_fly_is_a_sacrifice() {
        // Runners on second and third, nobody out: both tag up — third
        // scores, second takes third.
        let mut score = batting_home(0);
        let mut bases = with(&[1, 2]);
        let play = apply_batted_out(
            &mut score,
            &mut bases,
            &std_rules(),
            OutKind::Fly { deep: true },
            false,
        );
        assert_eq!((play.outs, play.runs), (1, 1));
        assert_eq!(score.home_runs, 1);
        assert_eq!(bases, with(&[2]));
    }

    #[test]
    fn shallow_fly_holds_the_runners() {
        let mut score = batting_home(0);
        let mut bases = with(&[2]);
        let play = apply_batted_out(
            &mut score,
            &mut bases,
            &std_rules(),
            OutKind::Fly { deep: false },
            false,
        );
        assert_eq!((play.outs, play.runs), (1, 0));
        assert_eq!(bases, with(&[2]));
    }

    #[test]
    fn two_out_deep_fly_ends_the_half_scoreless() {
        let mut score = batting_home(2);
        let mut bases = with(&[2]);
        let play = apply_batted_out(
            &mut score,
            &mut bases,
            &std_rules(),
            OutKind::Fly { deep: true },
            false,
        );
        assert_eq!(play.runs, 0);
        assert_eq!(score.home_runs, 0);
        assert!(score.top_of_inning);
    }

    #[test]
    fn charge_out_keeps_the_count_mid_at_bat() {
        // A runner thrown out on the bases is not the batter's at-bat ending.
        let mut score = ScoreBoard {
            balls: 2,
            strikes: 1,
            ..batting_home(0)
        };
        let mut bases = empty();
        charge_out(&mut score, &mut bases, &std_rules());
        assert_eq!((score.balls, score.strikes, score.outs), (2, 1, 1));
    }

    #[test]
    fn runner_going_on_a_caught_fly_is_doubled_off() {
        let mut score = batting_home(0);
        let mut bases = with(&[0]);
        let play = apply_batted_out(
            &mut score,
            &mut bases,
            &std_rules(),
            OutKind::Fly { deep: false },
            true,
        );
        assert!(play.doubled_off);
        assert_eq!(play.outs, 2);
        assert_eq!(bases, empty());
    }

    #[test]
    fn runners_going_beat_the_double_play() {
        // The point of the hit-and-run: the grounder can't be turned two,
        // and the runner moves up.
        let mut score = batting_home(0);
        let mut bases = with(&[0]);
        let play = apply_batted_out(&mut score, &mut bases, &std_rules(), OutKind::Ground, true);
        assert!(!play.double_play);
        assert_eq!(play.outs, 1);
        assert_eq!(bases, with(&[1]));
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
            Outcome::Out(OutKind::Fly { .. })
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
    fn steep_foul_pop_is_caught() {
        // A towering pop sprayed well outside the wedge: caught anyway.
        assert_eq!(
            classify_batted_ball(vel_spray(60.0, 14.0, 60.0), &std_field(), &std_rules()),
            Outcome::Out(OutKind::FoulPop)
        );
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

    // ── Landing prediction ────────────────────────────────────────────────────

    #[test]
    fn dragless_landing_matches_closed_form() {
        let vel = vel_at(30.0, 30.0);
        let (land, t) = predict_landing(vel, Vec3::ZERO, 0.0, 0.0);
        let disc = vel.y * vel.y + 2.0 * GRAVITY * 0.6; // CONTACT_HEIGHT
        let t_expect = (vel.y + disc.sqrt()) / GRAVITY;
        assert!((t - t_expect).abs() < 0.05, "hang time {t} vs {t_expect}");
        let range_expect = Vec2::new(vel.x, vel.z).length() * t_expect;
        let range = Vec2::new(land.x, land.z).length();
        assert!(
            (range - range_expect).abs() < 1.5,
            "range {range} vs {range_expect}"
        );
    }

    #[test]
    fn drag_shortens_flight() {
        let vel = vel_at(30.0, 40.0);
        let (with_drag, t_drag) = predict_landing(vel, Vec3::ZERO, BALL_DRAG_FACTOR, 0.0);
        let (no_drag, _) = predict_landing(vel, Vec3::ZERO, 0.0, 0.0);
        assert!(
            Vec2::new(with_drag.x, with_drag.z).length() < Vec2::new(no_drag.x, no_drag.z).length()
        );
        assert!(t_drag > 0.5);
    }

    #[test]
    fn sidespin_bends_the_landing_point() {
        let vel = vel_at(25.0, 35.0);
        let (straight, _) = predict_landing(vel, Vec3::ZERO, BALL_DRAG_FACTOR, 0.0);
        let (bent, _) = predict_landing(
            vel,
            hit_spin(Vec3::new(10.0, 8.0, 20.0)),
            BALL_DRAG_FACTOR,
            crate::game::ball::MAGNUS_FACTOR,
        );
        assert!(
            (bent.x - straight.x).abs() > 0.5,
            "Magnus should bend the carry"
        );
    }

    // ── Pitch flight ──────────────────────────────────────────────────────────

    /// Simulates a full pitch flight with the same gravity + drag + Magnus
    /// model the live ball uses (`ball::apply_drag` / `ball::apply_magnus`),
    /// returning the plate-crossing point. Locks the balance constants to
    /// observable behaviour: if a model change makes centre pitches become
    /// balls, these fail instead of the gameplay quietly degrading.
    fn simulate_pitch(kind: PitchKind, aim: Vec2) -> Vec2 {
        let pitch_distance = std_field().pitch_distance;
        let mut pos = mound_reset_pos(pitch_distance);
        let mut vel = pitch_velocity_kind(kind, aim, pitch_distance);
        let spin = kind.spin();
        let dt = 1.0 / 240.0;

        while pos.z > 0.0 {
            let speed = vel.length();
            vel += -BALL_DRAG_FACTOR * speed * vel * dt;
            vel += crate::game::ball::MAGNUS_FACTOR * spin.cross(vel) * dt;
            vel.y -= GRAVITY * dt;
            pos += vel * dt;
            assert!(pos.y > 0.0, "pitch hit the ground before the plate");
        }
        Vec2::new(pos.x, pos.y)
    }

    #[test]
    fn every_kind_centre_aimed_is_a_strike() {
        for kind in [
            PitchKind::Fastball,
            PitchKind::Curveball,
            PitchKind::Changeup,
        ] {
            let cross = simulate_pitch(kind, Vec2::ZERO);
            assert!(
                is_in_zone(cross),
                "{kind:?} crossed at ({:.2}, {:.2}) — outside the zone",
                cross.x,
                cross.y
            );
        }
    }

    #[test]
    fn backspin_rides_and_topspin_dives() {
        let fast = simulate_pitch(PitchKind::Fastball, Vec2::ZERO);
        let curve = simulate_pitch(PitchKind::Curveball, Vec2::ZERO);
        assert!(
            fast.y > curve.y + 0.15,
            "fastball {fast:?} vs curveball {curve:?}"
        );
    }

    #[test]
    fn aim_maps_to_kinds_per_spec() {
        assert_eq!(
            PitchKind::from_aim(Vec2::new(0.0, 1.0)),
            PitchKind::Fastball
        );
        assert_eq!(
            PitchKind::from_aim(Vec2::new(0.0, -1.0)),
            PitchKind::Curveball
        );
        assert_eq!(PitchKind::from_aim(Vec2::ZERO), PitchKind::Changeup);
    }

    #[test]
    fn full_inside_fastball_plunks_the_batter() {
        // Max inside aim crosses inside the batter's body window; the batter
        // stands at x ≈ +0.7.
        let cross = simulate_pitch(PitchKind::Fastball, Vec2::new(1.0, 0.0));
        assert!(
            hits_batter(cross),
            "crossing ({:.2}, {:.2}) should hit the batter",
            cross.x,
            cross.y
        );
        assert!(!is_in_zone(cross));
    }

    #[test]
    fn batter_window_boundaries() {
        assert!(hits_batter(Vec2::new(0.6, 1.0)));
        assert!(!hits_batter(Vec2::new(0.4, 1.0))); // inside pitch, no contact
        assert!(!hits_batter(Vec2::new(-0.6, 1.0))); // away side — no batter there
        assert!(!hits_batter(Vec2::new(0.6, 2.2))); // sails over his head
    }

    #[test]
    fn hit_by_pitch_forces_like_a_walk() {
        let mut score = ScoreBoard {
            balls: 1,
            strikes: 2,
            top_of_inning: true,
            ..Default::default()
        };
        let mut bases = loaded();
        assert_eq!(hit_by_pitch(&mut score, &mut bases), 1);
        assert_eq!(score.away_runs, 1);
        assert_eq!((score.balls, score.strikes), (0, 0));
        assert_eq!(bases, loaded());
    }

    #[test]
    fn hit_spin_pulls_toward_the_spray_side() {
        let pulled = hit_spin(Vec3::new(10.0, 8.0, 20.0));
        let oppo = hit_spin(Vec3::new(-10.0, 8.0, 20.0));
        assert!(pulled.y * oppo.y < 0.0, "sidespin should flip with spray");
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
    fn one_inning_walkoff_ends_immediately() {
        let score = ScoreBoard {
            home_runs: 1,
            away_runs: 0,
            inning: 1,
            top_of_inning: false,
            ..Default::default()
        };
        assert!(is_game_over(&score, 1));
    }

    #[test]
    fn one_inning_tie_goes_to_extras() {
        // Still tied in the bottom of the 1st: play on.
        let bottom = ScoreBoard {
            inning: 1,
            top_of_inning: false,
            ..Default::default()
        };
        assert!(!is_game_over(&bottom, 1));
        // Tied after a full inning: extras.
        let extras = ScoreBoard {
            inning: 2,
            top_of_inning: true,
            ..Default::default()
        };
        assert!(!is_game_over(&extras, 1));
    }

    #[test]
    fn home_lead_entering_bottom_of_final_skips_the_half() {
        // Home led 2-0 when the top of the 6th ended: the bottom is never played.
        let score = ScoreBoard {
            home_runs: 2,
            away_runs: 0,
            inning: 6,
            top_of_inning: false,
            ..Default::default()
        };
        assert!(is_game_over(&score, 6));
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
