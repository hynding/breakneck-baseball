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
use crate::game::{ScoreBoard, Team};

// ── Tuning constants ──────────────────────────────────────────────────────────

/// Gravity magnitude used for landing-point prediction (matches Rapier default).
pub const GRAVITY: f32 = 9.81;
/// Approximate batted-ball contact height (metres).
const CONTACT_HEIGHT: f32 = 0.6;

/// Nominal fastball speed (m/s) — roughly 85 mph.
pub const PITCH_SPEED: f32 = 38.0;

/// Horizontal half-width of the called strike zone (metres from plate
/// centre). Public so the field can draw the zone the umpire actually calls.
pub const ZONE_HALF_WIDTH: f32 = 0.34;
/// Vertical strike-zone bounds (metres).
pub const ZONE_LOW: f32 = 0.5;
pub const ZONE_HIGH: f32 = 1.45;

/// A caught fly at least this far out (scaled by [`FieldSpec::hit_scale`])
/// gives runners time to tag up and advance.
const TAG_UP_MIN_DIST: f32 = 65.0;

// ── Live-play race constants ──────────────────────────────────────────────────
// The outcome of a ball in play is decided *during* the play by kinematic
// races between the live simulation and these speeds — never at contact.

/// Base-runner sprint speed (m/s) — shared with the runner rigs so the
/// animation and the umpire agree.
pub const RUNNER_SPEED: f32 = 7.5;
/// Fielder sprint speed — matches the fielding choreography's chase speed.
pub const FIELDER_SPEED: f32 = 7.0;
/// First-step reaction delay for fielders and runners alike.
const REACTION: f32 = 0.35;
/// Throw flight speed and glove-to-hand transfer time.
const THROW_FLIGHT_SPEED: f32 = 27.0;
const THROW_TRANSFER: f32 = 0.5;
/// A relay (catch-and-rethrow at a bag) turns faster than a gather.
const RELAY_TRANSFER: f32 = 0.3;
/// Head start a hit-and-run jump gives every forced runner (they broke with
/// the windup, not at contact).
const HIT_AND_RUN_JUMP: f32 = 1.6;
/// Extra grace a sent batter gets stretching for the next base — the throw
/// is usually going somewhere else, so the race is softer than the walk.
const STRETCH_GRACE: f32 = 0.9;
/// Bang-bang margin: ties and near-ties go to the runner.
const RUNNER_MARGIN: f32 = 0.35;
/// Gathers beyond this radius (scaled by hit_scale) concede first base — the
/// out at first is only contested on infield balls.
const INFIELD_GATHER_RADIUS: f32 = 30.0;
/// A catch closer to home than this (scaled) is an infield pop.
const POP_RADIUS: f32 = 30.0;

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

/// Batters per lineup (regulation nine).
pub const LINEUP_SIZE: u32 = 9;

/// Each team's place in its batting order. The order itself is implicit
/// (slots 1..=9 rotate); what the rules require is that it always rotates —
/// every completed plate appearance brings up the next batter.
#[derive(Resource, Default, Debug, Clone, PartialEq, Eq)]
pub struct BattingOrder {
    home: u32,
    away: u32,
}

impl BattingOrder {
    /// 1-based lineup slot of the batter currently due up for `team`.
    pub fn current(&self, team: Team) -> u32 {
        let slot = match team {
            Team::Home => self.home,
            Team::Away => self.away,
        };
        slot + 1
    }

    /// The plate appearance ended; the next batter steps in.
    pub fn advance(&mut self, team: Team) {
        let slot = match team {
            Team::Home => &mut self.home,
            Team::Away => &mut self.away,
        };
        *slot = (*slot + 1) % LINEUP_SIZE;
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
    /// The batter was cut down trying for one base too many; the other
    /// runners keep the `advanced` bases they'd earned.
    Stretching {
        advanced: u32,
    },
}

/// The result of a batted ball.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Foul,
    Out(OutKind),
    /// The force-and-relay two outs: the forced runner and the batter.
    DoublePlay,
    /// The force got the runner at `out_base` but the batter beat the relay:
    /// one out, batter on first.
    FieldersChoice {
        out_base: usize,
    },
    /// A clean hit worth this many bases (1 = single … up to the base count).
    Hit(u32),
    HomeRun,
}

/// The batting side's call on a live ball, read at resolution: send the
/// batter for the extra base, hold him a base early, or let the analytic
/// walk decide.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RunnerCall {
    #[default]
    Neutral,
    Send,
    Hold,
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
    /// Hard breaking ball that sweeps toward the batter's side (+X).
    Slider,
    /// Two-seamer that dives and runs away from the batter (−X).
    Sinker,
}

impl PitchKind {
    pub fn speed(self) -> f32 {
        match self {
            PitchKind::Fastball => PITCH_SPEED,
            PitchKind::Curveball => 31.0,
            PitchKind::Changeup => 29.0,
            PitchKind::Slider => 33.0,
            PitchKind::Sinker => 35.0,
        }
    }

    pub fn spin(self) -> Vec3 {
        match self {
            PitchKind::Fastball => Vec3::new(20.0, 0.0, 0.0),
            PitchKind::Curveball => Vec3::new(-18.0, 6.0, 0.0),
            PitchKind::Changeup => Vec3::new(6.0, 0.0, 0.0),
            // −Y spin accelerates a −Z pitch toward +X (the batter's side);
            // +Y sweeps it away toward −X. Mild ±X components add ride/dive.
            PitchKind::Slider => Vec3::new(-4.0, -14.0, 0.0),
            PitchKind::Sinker => Vec3::new(-10.0, 10.0, 0.0),
        }
    }

    /// Held aim at release selects the pitch by its dominant axis: up =
    /// fastball, down = curveball, left = slider, right = sinker, neutral =
    /// changeup. Aim keeps steering location too — aiming high *means*
    /// throwing the heater upstairs, and aiming inside means the sweeper in.
    pub fn from_aim(aim: Vec2) -> PitchKind {
        if aim.x.abs() > 0.35 && aim.x.abs() >= aim.y.abs() {
            if aim.x < 0.0 {
                PitchKind::Slider
            } else {
                PitchKind::Sinker
            }
        } else if aim.y > 0.35 {
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
    // the inside corner risks a hit-by-pitch. Negated: stick-right means
    // screen-right, which the behind-home camera renders as world −X.
    let target_x = -aim.x * 0.6;
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
    // aiming up trades hittability for home-run power. Spray is negated so
    // stick-right pulls toward screen-right (world −X).
    let launch_deg = (6.0 + 26.0 * (aim.y * 0.5 + 0.5) + timing * 8.0).clamp(-8.0, 72.0);
    let launch = launch_deg.to_radians();
    let spray = (-aim.x * 0.6 + timing * 0.05).clamp(-0.95, 0.95);

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

// ── Live-play resolution ──────────────────────────────────────────────────────

/// Is a ground position in fair territory (the wedge opening toward +Z)?
pub fn is_fair(pos: Vec3, field: &FieldSpec) -> bool {
    pos.z > 1.0 && pos.x.abs() <= pos.z * field.fair_half_angle.tan() + 0.01
}

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

/// Radial fence distance in the direction of `pos`, interpolated from the
/// foul lines to straightaway centre. The single source of truth for where
/// the wall stands: home-run classification, the spawned wall geometry, and
/// the fielders' don't-run-through-the-wall caps all read it.
pub fn fence_at(pos: Vec3, field: &FieldSpec) -> f32 {
    let dist = Vec2::new(pos.x, pos.z).length();
    let cos_half = field.fair_half_angle.cos();
    let centeredness = (((pos.z / dist.max(0.001)) - cos_half) / (1.0 - cos_half)).clamp(0.0, 1.0);
    field.fence_line + (field.fence_center - field.fence_line) * centeredness
}

/// Time for a fielder at `from` to reach `landing`, first step included.
pub fn catch_time(from: Vec3, landing: Vec3) -> f32 {
    REACTION + Vec2::new(landing.x - from.x, landing.z - from.z).length() / FIELDER_SPEED
}

/// The fielder (index into `fielders`) best placed to catch a ball landing at
/// `landing` after `hang` seconds — `None` if nobody can make it.
pub fn best_catcher(fielders: &[Vec3], landing: Vec3, hang: f32) -> Option<usize> {
    fielders
        .iter()
        .enumerate()
        .map(|(i, f)| (i, catch_time(*f, landing)))
        .filter(|(_, t)| *t <= hang)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// The out recorded when the ball is caught on the fly at `pos`.
pub fn resolve_catch(pos: Vec3, field: &FieldSpec) -> OutKind {
    if !is_fair(pos, field) {
        return OutKind::FoulPop;
    }
    let dist = Vec2::new(pos.x, pos.z).length();
    let s = field.hit_scale;
    if dist < POP_RADIUS * s {
        OutKind::Pop
    } else {
        OutKind::Fly {
            deep: dist >= TAG_UP_MIN_DIST * s,
        }
    }
}

/// The batter-vs-throw race once a fair ball is gathered at `pos`,
/// `gather_time` seconds after contact. Infield gathers contest the out at
/// first; deeper gathers concede it (nobody throws a clean outfield single to
/// first) and the batter stretches for every extra base the throw can't beat.
pub fn resolve_gathered(
    pos: Vec3,
    gather_time: f32,
    field: &FieldSpec,
    rules: &Ruleset,
) -> Outcome {
    let leg = field.base_positions.first().map_or(27.43, |b| b.length());
    let runner_at = |base: usize| REACTION + leg * base as f32 / RUNNER_SPEED;
    let throw_at = |target: Vec3| {
        gather_time
            + THROW_TRANSFER
            + Vec2::new(target.x - pos.x, target.z - pos.z).length() / THROW_FLIGHT_SPEED
    };
    let safe = |base: usize| {
        field
            .base_positions
            .get(base - 1)
            .is_some_and(|bp| runner_at(base) <= throw_at(*bp) + RUNNER_MARGIN)
    };

    let from_home = Vec2::new(pos.x, pos.z).length();
    if from_home < INFIELD_GATHER_RADIUS * field.hit_scale && !safe(1) {
        // Beaten to the bag (or, on the front lawn, beaned on the way).
        return Outcome::Out(if rules.peg_outs {
            OutKind::Pegged
        } else {
            OutKind::Ground
        });
    }
    let mut bases = 1;
    while bases < field.base_count() && safe(bases + 1) {
        bases += 1;
    }
    Outcome::Hit(bases as u32)
}

/// The base the defense throws to for the most reasonable out once a fair
/// ball is gathered at `pos`, `gather_time` seconds after contact: the lead
/// *force* out the throw can still beat, falling back to first base when no
/// better play is on. `runners_going` marks a hit-and-run jump, which takes
/// the runner forces off the table. 0-indexed into `base_positions`;
/// `base_count()` means home plate (bases loaded, force at the plate). Pure
/// choreography guidance — the call itself comes from [`resolve_thrown`].
pub fn throw_target(
    pos: Vec3,
    gather_time: f32,
    bases: &Bases,
    runners_going: bool,
    field: &FieldSpec,
) -> usize {
    let leg = field.base_positions.first().map_or(27.43, |b| b.length());
    // Every forced runner (batter included) sprints exactly one base from a
    // standing start at contact, so one clock covers them all — minus the
    // jump when the runners broke with the windup.
    let runner_at = forced_runner_at(leg, runners_going);
    let throw_at = |target: Vec3| {
        gather_time
            + THROW_TRANSFER
            + Vec2::new(target.x - pos.x, target.z - pos.z).length() / THROW_FLIGHT_SPEED
    };
    let base_pos = |b: usize| home_or_base(b, field);

    // Take the biggest force out the throw still beats; else the sure-ish
    // play at first. The batter never has the jump, so first base races on
    // the standing-start clock.
    let batter_at = forced_runner_at(leg, false);
    let mut b = lead_force(bases, field);
    loop {
        let clock = if b == 0 { batter_at } else { runner_at };
        if clock > throw_at(base_pos(b)) + RUNNER_MARGIN {
            return b;
        }
        if b == 0 {
            return 0;
        }
        b -= 1;
    }
}

/// One forced runner's time to the next bag: a standing start at contact,
/// minus the hit-and-run head start when the runners broke with the windup.
fn forced_runner_at(leg: f32, going: bool) -> f32 {
    REACTION + leg / RUNNER_SPEED - if going { HIT_AND_RUN_JUMP } else { 0.0 }
}

/// World position of base `b`, where `b == base_count()` means home plate.
fn home_or_base(b: usize, field: &FieldSpec) -> Vec3 {
    if b == field.base_count() {
        Vec3::ZERO
    } else {
        field.base_positions[b]
    }
}

/// The lead base of the force chain: the batter forces first, and each
/// consecutively occupied base extends the chain one further (bases loaded
/// forces the runner at the plate, expressed as `base_count()`).
fn lead_force(bases: &Bases, field: &FieldSpec) -> usize {
    let mut lead = 0;
    for b in 0..field.base_count() {
        if lead == b && bases.is_occupied(b) {
            lead = b + 1;
        } else {
            break;
        }
    }
    lead
}

/// The race once the ball-holder throws to `target` (0-indexed;
/// `base_count()` = home plate), `throw_time` seconds after contact.
///
/// An out needs a live force at the target, an infield-range gather, and the
/// throw beating the forced runner's one-base sprint (minus the jump on a
/// hit-and-run). A force out at first retires the batter plainly; at any
/// later bag the relay to first races the batter — beat him and it's the
/// classic [`Outcome::DoublePlay`], lose and it's a
/// [`Outcome::FieldersChoice`]. No out at all concedes, and the batter takes
/// every base the throw can't beat — the same walk as [`resolve_gathered`],
/// whose behaviour this reproduces exactly for a prompt neutral throw to
/// first with empty bases. `runner_call` is the batting side's say: `Send`
/// stretches for one extra base against a softer race (cut down trying if it
/// fails), `Hold` pulls up a base early.
#[allow(clippy::too_many_arguments)]
pub fn resolve_thrown(
    pos: Vec3,
    throw_time: f32,
    target: usize,
    bases: &Bases,
    runners_going: bool,
    runner_call: RunnerCall,
    field: &FieldSpec,
    rules: &Ruleset,
) -> Outcome {
    let leg = field.base_positions.first().map_or(27.43, |b| b.length());
    let throw_at = |p: Vec3| {
        throw_time
            + THROW_TRANSFER
            + Vec2::new(p.x - pos.x, p.z - pos.z).length() / THROW_FLIGHT_SPEED
    };
    let base_pos = |b: usize| home_or_base(b, field);
    let flat_dist = |a: Vec3, b: Vec3| Vec2::new(a.x - b.x, a.z - b.z).length();

    let from_home = Vec2::new(pos.x, pos.z).length();
    let infield = from_home < INFIELD_GATHER_RADIUS * field.hit_scale;
    let runner_clock = if target == 0 {
        // The batter is the forced runner at first and never has the jump.
        forced_runner_at(leg, false)
    } else {
        forced_runner_at(leg, runners_going)
    };
    if target <= lead_force(bases, field)
        && infield
        && runner_clock > throw_at(base_pos(target)) + RUNNER_MARGIN
    {
        if target == 0 {
            // The sure out at first: just the batter.
            return Outcome::Out(if rules.peg_outs {
                OutKind::Pegged
            } else {
                OutKind::Ground
            });
        }
        // Forced runner retired; the relay to first races the batter.
        let relay_arrival = throw_at(base_pos(target))
            + RELAY_TRANSFER
            + flat_dist(base_pos(target), base_pos(0)) / THROW_FLIGHT_SPEED;
        if forced_runner_at(leg, false) > relay_arrival + RUNNER_MARGIN {
            return Outcome::DoublePlay;
        }
        return Outcome::FieldersChoice { out_base: target };
    }

    // No out on the throw: the batter takes every base it can't beat.
    let batter_at = |base: usize| REACTION + leg * base as f32 / RUNNER_SPEED;
    let safe = |base: usize| {
        field
            .base_positions
            .get(base - 1)
            .is_some_and(|bp| batter_at(base) <= throw_at(*bp) + RUNNER_MARGIN)
    };
    let mut n = 1;
    while n < field.base_count() && safe(n + 1) {
        n += 1;
    }
    match runner_call {
        // Sent for one more: a softer race (the ball is usually elsewhere),
        // but getting it wrong is an out on the bases.
        RunnerCall::Send if n < field.base_count() => {
            let stretch_to = field.base_positions[n];
            if batter_at(n + 1) <= throw_at(stretch_to) + RUNNER_MARGIN + STRETCH_GRACE {
                Outcome::Hit(n as u32 + 1)
            } else {
                Outcome::Out(OutKind::Stretching { advanced: n as u32 })
            }
        }
        // Held up: bank the sure bases (never less than the single).
        RunnerCall::Hold => Outcome::Hit((n as u32 - 1).max(1)),
        _ => Outcome::Hit(n as u32),
    }
}

/// The batting side's runner call from its held aim during a live ball:
/// stick down sends the batter for the extra base (matching the send-the-
/// runner steal convention), stick up holds him a base early.
pub fn runner_call_from_aim(aim: Vec2) -> RunnerCall {
    if aim.y < -0.7 {
        RunnerCall::Send
    } else if aim.y > 0.7 {
        RunnerCall::Hold
    } else {
        RunnerCall::Neutral
    }
}

/// The base a held defensive aim selects for a manual throw — the base
/// (home = `base_count()`) whose on-screen direction from the plate best
/// matches the stick. Screen right is world −X and screen up is +Z (the
/// behind-home camera), so the diamond reads naturally: right = first,
/// up = second on a three-base diamond, left = third, down = home. `None`
/// when the stick is too centred or points nowhere near a base.
pub fn aimed_base(aim: Vec2, field: &FieldSpec) -> Option<usize> {
    if aim.length() < 0.5 {
        return None;
    }
    let dir = Vec2::new(-aim.x, aim.y).normalize(); // screen aim → world (x, z)
    let mut best: Option<(usize, f32)> = None;
    let mut consider = |b: usize, world: Vec2| {
        let Some(bd) = world.try_normalize() else {
            return;
        };
        let dot = dir.dot(bd);
        if dot > best.map_or(0.3, |(_, d)| d) {
            best = Some((b, dot));
        }
    };
    for (b, p) in field.base_positions.iter().enumerate() {
        consider(b, Vec2::new(p.x, p.z));
    }
    consider(field.base_count(), Vec2::new(0.0, -1.0)); // home reads as "down"
    best.map(|(b, _)| b)
}

/// Numerically integrates a batted ball's flight from contact height with the
/// same gravity + drag + Magnus model the live ball uses (`ball::apply_drag`,
/// `ball::apply_magnus`), returning the landing point (y = 0) and hang time.
/// This is what fielder choreography chases — the *visual* ball's touchdown,
/// not the balance-tuned range in [`classify_batted_ball`].
pub fn predict_landing(vel: Vec3, spin: Vec3, drag_factor: f32, magnus_factor: f32) -> (Vec3, f32) {
    predict_landing_from(
        Vec3::new(0.0, CONTACT_HEIGHT, 0.0),
        vel,
        spin,
        drag_factor,
        magnus_factor,
    )
}

/// [`predict_landing`] from an arbitrary mid-flight state — what a chasing
/// fielder re-plans against every frame as the live ball bends.
pub fn predict_landing_from(
    start: Vec3,
    vel: Vec3,
    spin: Vec3,
    drag_factor: f32,
    magnus_factor: f32,
) -> (Vec3, f32) {
    let mut pos = start;
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
    advance_hit_with_jump(bases, hit_bases, false)
}

/// [`advance_hit`], but `jump` gives every *existing* runner one extra base —
/// the hit-and-run reward for breaking with the pitch (first-to-third on a
/// single). The batter still takes exactly `hit_bases`.
pub fn advance_hit_with_jump(bases: &mut Bases, hit_bases: u32, jump: bool) -> u32 {
    debug_assert!(hit_bases >= 1, "a hit is worth at least one base");
    let n = bases.count();
    let runner_step = hit_bases as usize + jump as usize;
    let batter_step = hit_bases as usize;
    let mut runs = 0;
    let mut next = vec![false; n];

    for base in 0..n {
        if bases.is_occupied(base) {
            let dest = base + runner_step;
            if dest >= n {
                runs += 1; // past the last base → scored
            } else {
                next[dest] = true;
            }
        }
    }
    // The batter reaches base `hit_bases` (1-indexed); one past the last base
    // means they came all the way around.
    if batter_step > n {
        runs += 1;
    } else {
        next[batter_step - 1] = true;
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

/// Applies a hit worth `hit_bases` bases: advances runners (with the
/// hit-and-run `jump` when runners were going), credits runs to the batting
/// team, and ends the at-bat. Returns the runs scored.
pub fn apply_hit(score: &mut ScoreBoard, bases: &mut Bases, hit_bases: u32, jump: bool) -> u32 {
    let runs = advance_hit_with_jump(bases, hit_bases, jump);
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
/// a caught ball doubles the runner off and nobody tags up. Whether a
/// grounder turns two is no longer decided here — [`resolve_thrown`] races
/// the actual relay and reports [`Outcome::DoublePlay`] /
/// [`Outcome::FieldersChoice`] outright (see [`apply_double_play`] and
/// [`apply_fielders_choice`]).
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
            // The defense took the sure out at first; unless the play ends
            // the inning, everyone else moved up a base.
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
        // Cut down stretching: the other runners keep the bases they earned
        // (a timing play — any run that crossed counts).
        OutKind::Stretching { advanced } => {
            play.runs = advance_runners_only(bases, advanced);
        }
    }
    if play.doubled_off {
        play.outs += 1;
    }
    // Never charge past the end of the half — a second out on the play can't
    // leak into the next half-inning.
    play.outs = play.outs.min(outs_left);
    score.add_runs(play.runs);
    reset_count(score);
    for _ in 0..play.outs {
        charge_out(score, bases, rules);
    }
    play
}

/// Applies [`Outcome::DoublePlay`]: the forced runner at second and the
/// batter at first, with the trailing advance only when the play doesn't end
/// the inning — identical base math to the old fiat double play. With one
/// out remaining only the force counts (the inning ends on it).
pub fn apply_double_play(score: &mut ScoreBoard, bases: &mut Bases, rules: &Ruleset) -> OutPlay {
    let outs_left = rules.outs_per_half.saturating_sub(score.outs);
    let mut play = OutPlay {
        outs: 2.min(outs_left),
        runs: 0,
        double_play: true,
        doubled_off: false,
    };
    bases.set(0, false); // the forced runner dies at the middle bag
    if play.outs < outs_left {
        play.runs = advance_trailing(bases);
    }
    score.add_runs(play.runs);
    reset_count(score);
    for _ in 0..play.outs {
        charge_out(score, bases, rules);
    }
    play
}

/// Applies [`Outcome::FieldersChoice`]: the forced runner is retired at
/// `out_base` while the batter reaches first; the forced runners behind the
/// out move up with him and everyone ahead holds. Never scores a run.
pub fn apply_fielders_choice(
    score: &mut ScoreBoard,
    bases: &mut Bases,
    rules: &Ruleset,
    out_base: usize,
) -> OutPlay {
    let outs_left = rules.outs_per_half.saturating_sub(score.outs);
    let play = OutPlay {
        outs: 1,
        runs: 0,
        double_play: false,
        doubled_off: false,
    };
    if out_base > 0 {
        bases.set(out_base - 1, false);
    }
    if play.outs < outs_left {
        // Everyone behind the out was forced by the batter: each moves up,
        // and the batter takes first.
        for base in (0..out_base.saturating_sub(1)).rev() {
            if bases.is_occupied(base) {
                bases.set(base, false);
                bases.set(base + 1, true);
            }
        }
        bases.set(0, true);
    }
    reset_count(score);
    charge_out(score, bases, rules);
    play
}

/// Advances every *existing* runner `n` bases without placing the batter —
/// the base state after the batter is cut down stretching. Returns runs.
fn advance_runners_only(bases: &mut Bases, n: u32) -> u32 {
    let count = bases.count();
    let mut runs = 0;
    for base in (0..count).rev() {
        if bases.is_occupied(base) {
            bases.set(base, false);
            let dest = base + n as usize;
            if dest >= count {
                runs += 1;
            } else {
                bases.set(dest, true);
            }
        }
    }
    runs
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

/// What sending the runner produced once the pitch reached the catcher.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StealResult {
    /// Safe — the runner now stands on `base` (0-indexed).
    Stolen { base: usize },
    /// Thrown out; the out is charged but the batter's count stands.
    Caught,
    /// Nobody was in a position to steal.
    NoRunner,
}

/// Resolves a straight steal on a pitch the batter didn't put in play: the
/// jump beats the throw on off-speed stuff, but a fastball gets there in
/// time — unless the runner broke from an extended lead (`big_jump`), which
/// beats any pitch. The extended lead was the gamble: it exposed the runner
/// to a pickoff during the pre-pitch window (see [`attempt_pickoff`]). One
/// runner (the lead eligible one) goes per pitch.
pub fn attempt_steal(
    score: &mut ScoreBoard,
    bases: &mut Bases,
    rules: &Ruleset,
    off_speed: bool,
    big_jump: bool,
) -> StealResult {
    let Some(runner) = steal_candidate(bases) else {
        return StealResult::NoRunner;
    };
    if off_speed || big_jump {
        bases.set(runner, false);
        bases.set(runner + 1, true);
        StealResult::Stolen { base: runner + 1 }
    } else {
        bases.set(runner, false);
        charge_out(score, bases, rules);
        StealResult::Caught
    }
}

/// What a pickoff throw during the pre-pitch window produced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickoffResult {
    /// Caught off the bag mid-extension — the runner is out.
    PickedOff { base: usize },
    /// The runner dove back in time (a normal lead is always safe).
    SafeBack,
    /// Nobody was leading off anywhere.
    NoRunner,
}

/// Resolves a pickoff throw at the lead eligible runner. The analytic model
/// keeps runners glued to the bag on a normal lead — only an *extended* lead
/// (the offense arming an early steal) strays far enough to be caught. This
/// is the deterministic counter to the guaranteed [`attempt_steal`] big jump.
pub fn attempt_pickoff(
    score: &mut ScoreBoard,
    bases: &mut Bases,
    rules: &Ruleset,
    extended: bool,
) -> PickoffResult {
    let Some(runner) = steal_candidate(bases) else {
        return PickoffResult::NoRunner;
    };
    if extended {
        bases.set(runner, false);
        charge_out(score, bases, rules);
        PickoffResult::PickedOff { base: runner }
    } else {
        PickoffResult::SafeBack
    }
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

    // ── Steals ────────────────────────────────────────────────────────────────

    #[test]
    fn steal_succeeds_against_offspeed() {
        let mut score = ScoreBoard::default();
        let mut bases = with(&[0]);
        assert_eq!(
            attempt_steal(&mut score, &mut bases, &std_rules(), true, false),
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
            attempt_steal(&mut score, &mut bases, &std_rules(), false, false),
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
        // (his target is now... still second — one steal per pitch).
        let mut score = ScoreBoard::default();
        let mut bases = with(&[0, 1]);
        assert_eq!(
            attempt_steal(&mut score, &mut bases, &std_rules(), true, false),
            StealResult::Stolen { base: 2 }
        );
        assert_eq!(bases, with(&[0, 2]));
    }

    #[test]
    fn home_cannot_be_stolen() {
        let mut score = ScoreBoard::default();
        let mut bases = with(&[2]);
        assert_eq!(
            attempt_steal(&mut score, &mut bases, &std_rules(), true, false),
            StealResult::NoRunner
        );
        assert_eq!(bases, with(&[2]));
    }

    #[test]
    fn empty_bases_cannot_steal() {
        let mut score = ScoreBoard::default();
        let mut bases = empty();
        assert_eq!(
            attempt_steal(&mut score, &mut bases, &std_rules(), true, false),
            StealResult::NoRunner
        );
    }

    #[test]
    fn big_jump_beats_even_a_fastball() {
        let mut score = ScoreBoard::default();
        let mut bases = with(&[0]);
        assert_eq!(
            attempt_steal(&mut score, &mut bases, &std_rules(), false, true),
            StealResult::Stolen { base: 1 }
        );
        assert_eq!(bases, with(&[1]));
        assert_eq!(score.outs, 0);
    }

    // ── Pickoffs ──────────────────────────────────────────────────────────────

    #[test]
    fn pickoff_catches_an_extended_lead() {
        let mut score = ScoreBoard {
            balls: 1,
            strikes: 2,
            ..Default::default()
        };
        let mut bases = with(&[0]);
        assert_eq!(
            attempt_pickoff(&mut score, &mut bases, &std_rules(), true),
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
            attempt_pickoff(&mut score, &mut bases, &std_rules(), false),
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
            attempt_pickoff(&mut score, &mut bases, &std_rules(), true),
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
            attempt_pickoff(&mut score, &mut bases, &std_rules(), true),
            PickoffResult::PickedOff { base: 0 }
        );
        assert_eq!(score.outs, 0, "side retired: outs reset");
        assert!(!score.top_of_inning, "half-inning flips on the third out");
        assert_eq!(bases, empty());
    }

    #[test]
    fn hit_and_run_sends_first_to_third_on_a_single() {
        let mut b = with(&[0]);
        // Runner takes two (the jump), batter takes one.
        assert_eq!(advance_hit_with_jump(&mut b, 1, true), 0);
        assert_eq!(b, with(&[0, 2]));
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
    fn double_play_retires_the_batter_and_the_forced_runner() {
        let mut score = batting_home(0);
        let mut bases = with(&[0]);
        let play = apply_double_play(&mut score, &mut bases, &std_rules());
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
        let play = apply_double_play(&mut score, &mut bases, &std_rules());
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
        let play = apply_double_play(&mut score, &mut bases, &std_rules());
        assert!(play.double_play);
        assert_eq!(play.runs, 0);
        assert_eq!(score.home_runs, 0);
        assert!(score.top_of_inning); // half flipped
        assert_eq!(score.inning, 2);
    }

    #[test]
    fn double_play_with_two_outs_only_counts_the_force() {
        // The inning ends on the force at second; the relay out can't leak
        // into the next half.
        let mut score = batting_home(2);
        let mut bases = with(&[0]);
        let play = apply_double_play(&mut score, &mut bases, &std_rules());
        assert_eq!(play.outs, 1);
        assert_eq!(score.outs, 0); // half flipped cleanly
        assert!(score.top_of_inning);
    }

    #[test]
    fn fielders_choice_trades_the_runner_for_the_batter() {
        let mut score = batting_home(0);
        let mut bases = with(&[0]);
        let play = apply_fielders_choice(&mut score, &mut bases, &std_rules(), 1);
        assert_eq!(play.outs, 1);
        assert_eq!(score.outs, 1);
        assert_eq!(bases, with(&[0])); // batter standing where the runner was
    }

    #[test]
    fn fielders_choice_at_home_keeps_the_bases_loaded() {
        // Force at the plate with the bases full: the lead runner dies, the
        // rest move up behind the batter — still loaded, nobody scored.
        let mut score = batting_home(0);
        let mut bases = loaded();
        let count = bases.count();
        let play = apply_fielders_choice(&mut score, &mut bases, &std_rules(), count);
        assert_eq!(play.outs, 1);
        assert_eq!(play.runs, 0);
        assert_eq!(score.home_runs, 0);
        assert_eq!(bases, loaded());
    }

    #[test]
    fn cut_down_stretching_keeps_the_runners_advance() {
        // Batter out trying to stretch a single with a runner on second: the
        // runner still moves up (and in from third would score).
        let mut score = batting_home(0);
        let mut bases = with(&[1]);
        let play = apply_batted_out(
            &mut score,
            &mut bases,
            &std_rules(),
            OutKind::Stretching { advanced: 1 },
            false,
        );
        assert_eq!(play.outs, 1);
        assert_eq!(bases, with(&[2]));
        assert_eq!(play.runs, 0);
    }

    #[test]
    fn honest_ground_out_takes_one_and_advances_the_runner() {
        // The out at first is just the batter now — the runner moves up
        // behind the play instead of being doubled off by fiat.
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
        assert_eq!(apply_hit(&mut score, &mut bases, 2, false), 1);
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

    // ── Live-play races ───────────────────────────────────────────────────────

    #[test]
    fn routine_fly_gets_run_down() {
        // A can-of-corn to shallow centre hangs ~3 s; the middle infield
        // reaches it with time to spare.
        let f = std_field();
        assert!(best_catcher(&f.fielder_positions, Vec3::new(0.0, 0.0, 44.0), 3.0).is_some());
    }

    #[test]
    fn sinking_liner_falls_in() {
        // A liner dying at 55 m hangs ~1.5 s: nobody can get there.
        let f = std_field();
        assert!(best_catcher(&f.fielder_positions, Vec3::new(0.0, 0.0, 55.0), 1.5).is_none());
    }

    #[test]
    fn catches_map_to_pop_fly_and_foul_pop() {
        let f = std_field();
        assert_eq!(resolve_catch(Vec3::new(0.0, 0.0, 12.0), &f), OutKind::Pop);
        assert_eq!(
            resolve_catch(Vec3::new(0.0, 0.0, 50.0), &f),
            OutKind::Fly { deep: false }
        );
        assert_eq!(
            resolve_catch(Vec3::new(0.0, 0.0, 80.0), &f),
            OutKind::Fly { deep: true }
        );
        assert_eq!(
            resolve_catch(Vec3::new(-30.0, 0.0, 10.0), &f),
            OutKind::FoulPop
        );
    }

    #[test]
    fn quick_infield_gather_beats_the_batter() {
        assert_eq!(
            resolve_gathered(Vec3::new(0.0, 0.0, 7.0), 1.2, &std_field(), &std_rules()),
            Outcome::Out(OutKind::Ground)
        );
    }

    #[test]
    fn slow_infield_gather_is_an_infield_single() {
        assert_eq!(
            resolve_gathered(Vec3::new(0.0, 0.0, 26.0), 3.0, &std_field(), &std_rules()),
            Outcome::Hit(1)
        );
    }

    #[test]
    fn shallow_outfield_gather_concedes_a_single() {
        assert_eq!(
            resolve_gathered(Vec3::new(0.0, 0.0, 35.0), 2.6, &std_field(), &std_rules()),
            Outcome::Hit(1)
        );
    }

    #[test]
    fn deep_gap_gather_is_a_double() {
        assert_eq!(
            resolve_gathered(Vec3::new(50.0, 0.0, 95.0), 5.8, &std_field(), &std_rules()),
            Outcome::Hit(2)
        );
    }

    #[test]
    fn ball_to_the_wall_is_a_triple() {
        assert_eq!(
            resolve_gathered(Vec3::new(0.0, 0.0, 120.0), 7.5, &std_field(), &std_rules()),
            Outcome::Hit(3)
        );
    }

    // ── Throw-target selection ────────────────────────────────────────────────

    #[test]
    fn bases_empty_throws_to_first() {
        assert_eq!(
            throw_target(Vec3::new(0.0, 0.0, 7.0), 1.2, &empty(), false, &std_field()),
            0
        );
    }

    #[test]
    fn runner_on_first_takes_the_force_at_second() {
        // Gathered near second with a runner on first: the lead force is on
        // and the short throw beats the runner.
        assert_eq!(
            throw_target(
                Vec3::new(0.0, 0.0, 30.0),
                1.2,
                &with(&[0]),
                false,
                &std_field()
            ),
            1
        );
    }

    #[test]
    fn runner_on_second_is_not_forced() {
        // No runner on first, so second base is not a force — take first.
        assert_eq!(
            throw_target(
                Vec3::new(0.0, 0.0, 30.0),
                1.2,
                &with(&[1]),
                false,
                &std_field()
            ),
            0
        );
    }

    #[test]
    fn bases_loaded_forces_the_play_at_home() {
        let field = std_field();
        assert_eq!(
            throw_target(Vec3::new(-5.0, 0.0, 10.0), 0.8, &loaded(), false, &field),
            field.base_count()
        );
    }

    #[test]
    fn late_gather_falls_back_to_first() {
        // Gathered so late that no throw beats any runner: still play to
        // first — the conventional, "most reasonable" attempt.
        assert_eq!(
            throw_target(
                Vec3::new(0.0, 0.0, 60.0),
                6.0,
                &with(&[0]),
                false,
                &std_field()
            ),
            0
        );
    }

    #[test]
    fn hit_and_run_jump_takes_the_force_off_the_table() {
        // A mid-infield gather that forces the standing-start runner at
        // second — but with the windup jump the throw can't win there, so
        // the smart throw goes to first instead.
        let pos = Vec3::new(0.0, 0.0, 20.0);
        assert_eq!(throw_target(pos, 1.2, &with(&[0]), false, &std_field()), 1);
        assert_eq!(throw_target(pos, 1.2, &with(&[0]), true, &std_field()), 0);
    }

    // ── Thrown-ball resolution ────────────────────────────────────────────────

    fn neutral(
        pos: Vec3,
        t: f32,
        target: usize,
        bases: &Bases,
        f: &FieldSpec,
        r: &Ruleset,
    ) -> Outcome {
        resolve_thrown(pos, t, target, bases, false, RunnerCall::Neutral, f, r)
    }

    #[test]
    fn prompt_throw_to_first_matches_resolve_gathered() {
        let (f, r) = (std_field(), std_rules());
        for (pos, t) in [
            (Vec3::new(0.0, 0.0, 7.0), 1.2),
            (Vec3::new(0.0, 0.0, 26.0), 3.0),
            (Vec3::new(0.0, 0.0, 35.0), 2.6),
            (Vec3::new(50.0, 0.0, 95.0), 5.8),
        ] {
            assert_eq!(
                neutral(pos, t, 0, &empty(), &f, &r),
                resolve_gathered(pos, t, &f, &r),
                "at {pos:?} t={t}"
            );
        }
    }

    #[test]
    fn quick_force_at_second_turns_two() {
        // Sharp play near the bag: the force arrives early and the relay to
        // first still beats the batter — the classic double play.
        assert_eq!(
            neutral(
                Vec3::new(0.0, 0.0, 28.0),
                1.2,
                1,
                &with(&[0]),
                &std_field(),
                &std_rules()
            ),
            Outcome::DoublePlay
        );
    }

    #[test]
    fn slow_force_at_second_is_a_fielders_choice() {
        // A weak roller near the plate: the force barely beats the runner,
        // and the long relay cannot double the batter.
        assert_eq!(
            neutral(
                Vec3::new(0.0, 0.0, 5.0),
                1.8,
                1,
                &with(&[0]),
                &std_field(),
                &std_rules()
            ),
            Outcome::FieldersChoice { out_base: 1 }
        );
    }

    #[test]
    fn throw_behind_the_play_concedes_the_single() {
        // Third base is not a force with only a runner on first: the throw
        // there gets nobody, and the batter has the single.
        assert_eq!(
            neutral(
                Vec3::new(0.0, 0.0, 28.0),
                1.2,
                2,
                &with(&[0]),
                &std_field(),
                &std_rules()
            ),
            Outcome::Hit(1)
        );
    }

    #[test]
    fn bases_loaded_quick_throw_home_turns_two() {
        // The 2-3 special: force at the plate, relay to first in time.
        let field = std_field();
        assert_eq!(
            neutral(
                Vec3::new(-5.0, 0.0, 10.0),
                0.8,
                field.base_count(),
                &loaded(),
                &field,
                &std_rules()
            ),
            Outcome::DoublePlay
        );
    }

    #[test]
    fn outfield_gather_cannot_force_anyone() {
        // Even aimed at a live force, a deep gather concedes: the out at any
        // bag is only contested from infield range.
        assert_eq!(
            neutral(
                Vec3::new(0.0, 0.0, 60.0),
                3.5,
                1,
                &with(&[0]),
                &std_field(),
                &std_rules()
            ),
            Outcome::Hit(1)
        );
    }

    #[test]
    fn hit_and_run_beats_the_force_at_second() {
        // The jump the runner got at the windup makes the force unwinnable;
        // the play falls through to a plain single.
        assert_eq!(
            resolve_thrown(
                Vec3::new(0.0, 0.0, 28.0),
                1.2,
                1,
                &with(&[0]),
                true,
                RunnerCall::Neutral,
                &std_field(),
                &std_rules()
            ),
            Outcome::Hit(1)
        );
    }

    #[test]
    fn sent_batter_is_cut_down_stretching() {
        // A shallow-outfield single with the batter sent: the extra base is
        // not there, and the batter is out on the bases with the single's
        // advancement preserved for the other runners.
        assert_eq!(
            resolve_thrown(
                Vec3::new(0.0, 0.0, 60.0),
                3.5,
                0,
                &empty(),
                false,
                RunnerCall::Send,
                &std_field(),
                &std_rules()
            ),
            Outcome::Out(OutKind::Stretching { advanced: 1 })
        );
    }

    #[test]
    fn sent_batter_stretches_a_double_into_a_triple() {
        // Deep in the gap the softer stretch race is winnable.
        assert_eq!(
            resolve_thrown(
                Vec3::new(0.0, 0.0, 110.0),
                6.5,
                0,
                &empty(),
                false,
                RunnerCall::Send,
                &std_field(),
                &std_rules()
            ),
            Outcome::Hit(3)
        );
    }

    #[test]
    fn held_batter_banks_the_single() {
        // The same deep ball played safe stops a base short of the walk.
        let neutral_bases = match resolve_thrown(
            Vec3::new(0.0, 0.0, 110.0),
            6.5,
            0,
            &empty(),
            false,
            RunnerCall::Neutral,
            &std_field(),
            &std_rules(),
        ) {
            Outcome::Hit(n) => n,
            other => panic!("expected a hit, got {other:?}"),
        };
        assert_eq!(
            resolve_thrown(
                Vec3::new(0.0, 0.0, 110.0),
                6.5,
                0,
                &empty(),
                false,
                RunnerCall::Hold,
                &std_field(),
                &std_rules()
            ),
            Outcome::Hit((neutral_bases - 1).max(1))
        );
    }

    // ── Aimed-base selection ──────────────────────────────────────────────────

    #[test]
    fn aim_maps_the_diamond_to_the_stick() {
        let f = std_field();
        // Screen right = first, up = second, left = third, down = home.
        assert_eq!(aimed_base(Vec2::new(1.0, 0.0), &f), Some(0));
        assert_eq!(aimed_base(Vec2::new(0.0, 1.0), &f), Some(1));
        assert_eq!(aimed_base(Vec2::new(-1.0, 0.0), &f), Some(2));
        assert_eq!(aimed_base(Vec2::new(0.0, -1.0), &f), Some(f.base_count()));
    }

    #[test]
    fn centred_stick_selects_nothing() {
        assert_eq!(aimed_base(Vec2::new(0.2, 0.1), &std_field()), None);
    }

    #[test]
    fn fence_interpolates_line_to_center() {
        let f = std_field();
        // Straightaway centre field.
        assert!((fence_at(Vec3::new(0.0, 0.0, 100.0), &f) - f.fence_center).abs() < 0.01);
        // Down the line the fence sits at the line distance.
        let line = Vec3::new(100.0, 0.0, 100.0); // 45° = the foul line
        assert!((fence_at(line, &f) - f.fence_line).abs() < 0.01);
    }

    // ── Front-yard live play ──────────────────────────────────────────────────

    fn yard() -> (FieldSpec, Ruleset) {
        (VariantId::FrontYard.field(), VariantId::FrontYard.rules())
    }

    #[test]
    fn front_yard_infield_out_is_a_peg() {
        let (f, r) = yard();
        assert_eq!(
            resolve_gathered(Vec3::new(0.0, 0.0, 4.0), 0.4, &f, &r),
            Outcome::Out(OutKind::Pegged)
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
            PitchKind::Slider,
            PitchKind::Sinker,
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
        assert_eq!(PitchKind::from_aim(Vec2::new(-1.0, 0.0)), PitchKind::Slider);
        assert_eq!(PitchKind::from_aim(Vec2::new(1.0, 0.0)), PitchKind::Sinker);
        // The dominant axis wins a diagonal.
        assert_eq!(
            PitchKind::from_aim(Vec2::new(0.4, 0.9)),
            PitchKind::Fastball
        );
        assert_eq!(PitchKind::from_aim(Vec2::new(-0.9, 0.4)), PitchKind::Slider);
    }

    #[test]
    fn slider_sweeps_in_and_sinker_runs_away() {
        let neutral = simulate_pitch(PitchKind::Changeup, Vec2::ZERO);
        let slider = simulate_pitch(PitchKind::Slider, Vec2::ZERO);
        let sinker = simulate_pitch(PitchKind::Sinker, Vec2::ZERO);
        // The batter stands at +X: the slider breaks toward him, the sinker
        // runs away, and the sinker also finishes below the slider.
        assert!(
            slider.x > neutral.x + 0.08,
            "slider {slider:?} vs {neutral:?}"
        );
        assert!(
            sinker.x < neutral.x - 0.08,
            "sinker {sinker:?} vs {neutral:?}"
        );
    }

    #[test]
    fn full_inside_fastball_plunks_the_batter() {
        // Max inside aim (stick-left: the batter's box is on the +X /
        // screen-left side) crosses inside the batter's body window.
        let cross = simulate_pitch(PitchKind::Fastball, Vec2::new(-1.0, 0.0));
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

    // ── Batting order ─────────────────────────────────────────────────────────

    #[test]
    fn batting_order_rotates_nine_and_wraps() {
        let mut order = BattingOrder::default();
        assert_eq!(order.current(Team::Home), 1);
        for _ in 0..8 {
            order.advance(Team::Home);
        }
        assert_eq!(order.current(Team::Home), 9);
        order.advance(Team::Home);
        assert_eq!(order.current(Team::Home), 1);
        // Teams rotate independently.
        assert_eq!(order.current(Team::Away), 1);
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
