//! The shot catalog: which shot the play wants ([`pick_shot`]), how each
//! shot frames its subject ([`Shot::spec`] → [`ShotSpec`]), and how the rig
//! moves from one shot to the next ([`Transition`], [`RigMotion`]). All
//! pure, so the framing tests drive it without an `App`; the broadcast rig
//! in [`super::rigs`] only gathers the inputs and writes the camera.
//!
//! Adding a shot is one [`Shot`] variant, its arm in [`Shot::spec`], and a
//! [`pick_shot`] arm. Every shot cuts in today (task 6 of the graphics and
//! camera plan changes no framing); later shots pick an ease or a whip pan
//! in [`Shot::transition_from`].

use bevy::prelude::*;

use crate::game::ball::{BALL_DRAG_FACTOR, MAGNUS_FACTOR};
use crate::game::flow::Phase;
use crate::game::rules;
use crate::game::variant::FieldSpec;

use super::framing::{TROT_ORBIT_RATE, fit_vfov, trot_orbit_eye};
use super::{BALL_FOLLOW_DELAY, BROADCAST_FOV, DuelView};

// ── The shot list ─────────────────────────────────────────────────────────────

/// One subject per shot (TODO 102; the reference grammar in
/// docs/agent/SMB3-REFERENCE-NOTES.md §2.5–2.6). How a change of `Shot`
/// moves the lens is the new shot's [`Shot::transition_from`]: every shot
/// today is a **hard cut** (the rig snaps to the new framing), and
/// smoothing only runs *within* a shot (the ball-follow and the fielder cam
/// tracking their subjects). The old single rig glided 20–30 m across the
/// park between framings; a cut announces the new subject instead.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Shot {
    /// The duel framing of the active [`DuelView`] (a V press is a cut too).
    Duel(DuelView),
    /// Chasing the live ball toward where it comes down, the lens leading
    /// it to the predicted landing spot.
    BallFollow,
    /// A ball on the ground: a high, wide infield shot that holds the ball,
    /// the fielder going for it, and first base.
    GroundBall,
    /// A throw in from the outfield: trailing the runner it races as he
    /// heads for the bag, the throw coming into frame. Hands over to
    /// the base cam as the ball nears the bag.
    RunnerCam(Entity),
    /// Behind and above the fielder about to make the catch, ball in frame.
    FielderCam(Entity),
    /// Low beside the bag a throw is headed for (`base_count()` = home):
    /// runner, bag, and the incoming throw — the call lands in this shot.
    BaseCam(usize),
    /// The home-run trot orbit.
    TrotOrbit,
    /// The walk-up between plate appearances: a close plate shot from the
    /// first-base side, the incoming batter with the catcher and umpire
    /// behind him.
    WalkUp,
    /// A strikeout's result pause: side-on at the plate from the first-base
    /// side — the batter, the catcher with the ball, the umpire behind.
    ReactionCam,
    /// A home run's crowd reaction: from the outfield grass, up at the
    /// stands where the ball went out ([`super::sequence`]).
    CrowdCam,
    /// A dive: low and tight beside the fielder who laid out.
    DiveCam(Entity),
    /// A walk-off: low at the plate from the third-base side, the winning
    /// run scoring into the waiting team.
    PileUp,
    /// Between half-innings, the first walk-up of the new half opens on
    /// the park: the scoreboard and its line score in a stadium, a high
    /// wide look over the yard elsewhere ([`half_inning_cam`]). Behind the
    /// curtain's wipe, inside the walk-up hold, so it costs no game time.
    HalfInning,
}

/// Everything the shot picker reads, gathered once per frame so the choice
/// itself is a pure function ([`pick_shot`]) the unit tests can drive.
pub(super) struct ShotCues {
    pub phase: Phase,
    /// Seconds since contact (meaningful in `InPlay`).
    pub since_contact: f32,
    pub home_run: bool,
    /// The home side just walked it off (`Play::is_walk_off`): the result
    /// pause orbits the celebration like a home-run trot.
    pub walk_off: bool,
    pub pitch_gloved: bool,
    pub view: DuelView,
    /// The walk-up hold is on (`Play::walkup_active`).
    pub walkup: bool,
    /// The walk-up opens a new half-inning and its cutaway is still up
    /// (the rig decides; [`half_inning_cutaway`]).
    pub cutaway: bool,
    /// The result pause is a strikeout's.
    pub strikeout: bool,
    /// The base the live ball was last thrown at, if any.
    pub thrown_base: Option<usize>,
    /// The ball was hit on the ground ([`is_grounder`], latched by the rig
    /// for the rest of the play).
    pub grounder: bool,
    /// The runner racing a long throw that is still far from its bag (the
    /// rig decides; [`runner_cam_wanted`]).
    pub runner_cam: Option<Entity>,
    /// The chasing fielder, when the ball is in the air and coming down —
    /// the fielder cam's subject.
    pub descending_to: Option<Entity>,
    /// The bag a running-game throw (a pickoff, the catcher's throw on a
    /// steal) is staged at, while it is (`Play::duel_throw`).
    pub duel_throw_base: Option<usize>,
    /// A big moment's sequence shot for this moment of the result pause
    /// ([`super::sequence::sequence_shot`]), unless it was skipped.
    pub sequence: Option<Shot>,
}

/// Which shot the play wants right now. `current` is the shot on screen: a
/// result pause that isn't a gloved pitch or a home run *keeps* it, so the
/// call is announced in the shot the play ended on (SAFE! at the bag, OUT
/// over the fielder) instead of cutting away to a wide hold.
pub(super) fn pick_shot(cues: &ShotCues, current: Option<Shot>) -> Shot {
    // A running-game throw is the play: the call lands at the bag, so
    // that's where the camera is — the runner diving back or sliding in, the
    // cover man, the incoming ball.
    if let Some(base) = cues.duel_throw_base {
        return Shot::BaseCam(base);
    }
    match cues.phase {
        Phase::PrePitch if cues.walkup && cues.cutaway => Shot::HalfInning,
        Phase::PrePitch if cues.walkup => Shot::WalkUp,
        Phase::PrePitch | Phase::WindUp | Phase::Pitch => Shot::Duel(cues.view),
        // Fresh contact: hold the plate framing for a beat — the crack and
        // the bat coming through — then cut to the ball.
        Phase::InPlay if cues.since_contact < BALL_FOLLOW_DELAY => Shot::Duel(cues.view),
        Phase::InPlay if cues.home_run => Shot::BallFollow,
        Phase::InPlay => match (cues.runner_cam, cues.thrown_base, cues.descending_to) {
            (Some(runner), _, _) => Shot::RunnerCam(runner),
            (None, Some(base), _) => Shot::BaseCam(base),
            (None, None, Some(fielder)) => Shot::FielderCam(fielder),
            (None, None, None) if cues.grounder => Shot::GroundBall,
            (None, None, None) => Shot::BallFollow,
        },
        // A big moment's sequence owns the pause while it runs.
        Phase::Result if let Some(shot) = cues.sequence => shot,
        Phase::Result if cues.home_run || cues.walk_off => Shot::TrotOrbit,
        // Strike three gets its reaction shot (before the gloved arm: a K
        // into the mitt is gloved too).
        Phase::Result if cues.strikeout => Shot::ReactionCam,
        // A gloved pitch's call doesn't deserve a zoom-out: stay at the plate.
        Phase::Result if cues.pitch_gloved => Shot::Duel(cues.view),
        // The call always lands in the bag's shot, never on the runner cam
        // (a throw caught early, or the play called before the hand-over).
        Phase::Result if matches!(current, Some(Shot::RunnerCam(_))) => {
            cues.thrown_base.map_or(Shot::BallFollow, Shot::BaseCam)
        }
        // A sequence shot that ended (or was skipped) hands the rest of the
        // pause to the play: the bag the throw went to, else the ball.
        Phase::Result
            if matches!(
                current,
                Some(Shot::DiveCam(_) | Shot::CrowdCam | Shot::PileUp)
            ) =>
        {
            cues.thrown_base.map_or(Shot::BallFollow, Shot::BaseCam)
        }
        Phase::Result => current.unwrap_or(Shot::Duel(cues.view)),
    }
}

// ── Shot framings ─────────────────────────────────────────────────────────────

/// Vertical FOV of the fielder and base cams: a medium lens, tight enough
/// that the ball and the bag read large, wide enough for the runner and the
/// throw to share the frame.
const PLAY_CAM_FOV: f32 = 45.0_f32.to_radians();
/// A fly ball must still be this high (metres) for the fielder cam to take
/// over — below it the catch is a moment away and the ball-follow already
/// has it.
pub(super) const FIELDER_CAM_MIN_HEIGHT: f32 = 2.5;
/// Fielder cam: standoff behind the fielder (along the ball→fielder line)
/// and its height.
const FIELDER_CAM_BACK: f32 = 6.0;
const FIELDER_CAM_HEIGHT: f32 = 3.2;
/// Base cam: standoff from the bag and its height.
const BASE_CAM_BACK: f32 = 7.0;
const BASE_CAM_HEIGHT: f32 = 2.4;
/// Walk-up shot: from the first-base side (−x) in front of the plate, low,
/// looking back at the batter's box with the catcher and umpire behind it.
pub(super) const WALKUP_EYE: Vec3 = Vec3::new(-2.8, 1.25, 2.4);
pub(super) const WALKUP_TARGET: Vec3 = Vec3::new(0.55, 0.95, -0.6);
const WALKUP_FOV: f32 = 42.0_f32.to_radians();
/// Reaction cam: side-on at the plate from the first-base side (−x), a
/// touch behind it, so the batter in his box, the catcher's spot, and the
/// umpire behind him line up across the frame.
pub(super) const REACTION_EYE: Vec3 = Vec3::new(-4.6, 1.35, -0.6);
pub(super) const REACTION_TARGET: Vec3 = Vec3::new(0.35, 1.0, -1.0);
const REACTION_FOV: f32 = 40.0_f32.to_radians();

/// The crowd shot's lens, the dive cam's, and the pile-up's.
const CROWD_FOV: f32 = 32.0_f32.to_radians();
const DIVE_FOV: f32 = 38.0_f32.to_radians();
const PILE_UP_FOV: f32 = 48.0_f32.to_radians();
/// The pile-up: low on the third-base side (+x) in front of the plate,
/// looking across it toward the first-base dugout.
pub(super) const PILE_UP_EYE: Vec3 = Vec3::new(3.4, 1.3, 3.4);
pub(super) const PILE_UP_TARGET: Vec3 = Vec3::new(0.0, 0.9, -0.3);
/// The crowd shot aims this far into the stands past the fence, and keeps
/// at least this far off dead centre, where the batter's eye stands with no
/// seats (`field::bowl`, ±8°).
const CROWD_INTO_STANDS: f32 = 10.0;
const CROWD_OFF_CENTRE: f32 = 12.0_f32.to_radians();

/// How long the half-inning cutaway holds before the walk-up shot takes
/// over (gameplay seconds, inside the walk-up hold).
pub const CUTAWAY_SECS: f32 = 1.6;

/// A half-inning, `(inning, top)`, and when its cutaway started.
pub(super) type CutHalf = Option<((u32, bool), f32)>;

/// Whether the walk-up on screen opens with the half-inning cutaway. A
/// half is marked seen on its first pre-pitch frame (`pre_pitch`; `half` is
/// `(inning, top)`), and the cutaway plays only if that frame opens on a
/// walk-up: so never in the game's first half (the intro flyover has just
/// shown the park), never later in a half, and not at all in a half whose
/// leadoff has no walk-up (the last half ended on a caught stealing). It
/// holds for [`CUTAWAY_SECS`] of the hold. `cut` is the half last seen and
/// when its cutaway started on the gameplay clock. Returns the new `cut`
/// and whether the cutaway is up.
pub(super) fn half_inning_cutaway(
    pre_pitch: bool,
    walkup: bool,
    half: (u32, bool),
    now: f32,
    cut: CutHalf,
) -> (CutHalf, bool) {
    let walkup = pre_pitch && walkup;
    let cut = match cut {
        Some((seen, _)) if seen == half => cut,
        _ if pre_pitch => {
            let opens = walkup && half != (1, true);
            Some((half, if opens { now } else { f32::NEG_INFINITY }))
        }
        _ => cut,
    };
    let up = walkup && cut.is_some_and(|(seen, at)| seen == half && now - at < CUTAWAY_SECS);
    (cut, up)
}

/// Eye, look-at and lens for the half-inning cutaway. In a stadium, the
/// scoreboard square on from the outfield grass, the board filling most of
/// the frame so the line score reads; with no scoreboard (the front yard),
/// a high wide look over the whole yard from behind the plate.
pub(super) fn half_inning_cam(field: &FieldSpec, aspect: f32) -> (Vec3, Vec3, f32) {
    if field.scenery == crate::game::variant::Scenery::Stadium {
        let board = crate::game::field::scoreboard_centre(field);
        let toward_home = (-board).with_y(0.0).normalize_or(Vec3::NEG_Z);
        let eye = board + toward_home * 46.0 - Vec3::Y * 8.0;
        let half = crate::game::field::SCOREBOARD_SIZE / 2.0;
        let across = Vec3::Y.cross(toward_home).normalize_or(Vec3::X);
        let corners = [
            board + across * half.x + Vec3::Y * half.y,
            board - across * half.x - Vec3::Y * half.y,
            board + across * half.x - Vec3::Y * half.y,
            board - across * half.x + Vec3::Y * half.y,
        ];
        let fov = fit_vfov(eye, board, &corners, aspect, 0.8);
        (eye, board, fov)
    } else {
        let eye = Vec3::new(0.0, 22.0, -26.0);
        let target = Vec3::new(0.0, 0.0, field.fence_line * 0.45);
        (eye, target, 55.0_f32.to_radians())
    }
}

/// Eye and look-at for the crowd shot: from the outfield grass on the line
/// the ball left by, up at the seats beyond the fence there (shifted off the
/// batter's eye for a ball out to dead centre).
pub(super) fn crowd_cam(field: &FieldSpec, exit: Vec3) -> (Vec3, Vec3) {
    let mut angle = exit.x.atan2(exit.z);
    if !angle.is_finite() {
        angle = 0.0;
    }
    if angle.abs() < CROWD_OFF_CENTRE {
        angle = CROWD_OFF_CENTRE.copysign(if angle == 0.0 { 1.0 } else { angle });
    }
    let dir = Vec3::new(angle.sin(), 0.0, angle.cos());
    let fence = rules::fence_at(dir * 100.0, field);
    let seats = fence + CROWD_INTO_STANDS;
    // The stands rise about 0.6 m per metre back from a front row a little
    // over the wall's height (`field::bowl`).
    let target = dir * seats + Vec3::Y * (4.0 + CROWD_INTO_STANDS * 0.6);
    let eye = dir * (fence * 0.55) + Vec3::Y * 3.0;
    (eye, target)
}

/// Eye and look-at for the dive cam: low beside the fielder, a quarter
/// round toward home, the lens at his level so the layout reads.
pub(super) fn dive_cam(fielder: Vec3) -> (Vec3, Vec3) {
    let flat = Vec3::new(fielder.x, 0.0, fielder.z);
    let home = (-flat).normalize_or(-Vec3::Z);
    let side = Vec3::new(-home.z, 0.0, home.x);
    let eye = flat + side * 3.5 + home * 1.8 + Vec3::Y * 0.7;
    let target = flat + Vec3::Y * 0.45;
    (eye, target)
}

/// Eye and look-at for the base cam at `base` (`base_count()` = home): low
/// and just outside the diamond, three-quarters behind the runner's line of
/// approach, so the bag, the runner arriving, and the throw coming in from
/// the infield share the frame. Pure — pinned by the framing tests.
pub(super) fn base_cam(field: &FieldSpec, base: usize) -> (Vec3, Vec3) {
    let count = field.base_count();
    let bag_at = |i: usize| {
        if i >= count {
            Vec3::ZERO
        } else {
            field.base_positions[i]
        }
    };
    let bag = bag_at(base);
    let prev = if base == 0 {
        Vec3::ZERO
    } else {
        bag_at(base - 1)
    };
    let centre =
        (field.base_positions.iter().copied().sum::<Vec3>() + Vec3::ZERO) / (count as f32 + 1.0);
    let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z);
    let out = flat(bag - centre).normalize_or_zero();
    let along = flat(bag - prev).normalize_or_zero();
    let dir = (out * 0.6 + along * 0.4).normalize_or(-Vec3::Z);
    let eye = bag + dir * BASE_CAM_BACK + Vec3::Y * BASE_CAM_HEIGHT;
    let target = bag + Vec3::Y * 0.9;
    (eye, target)
}

/// Eye and look-at for the fielder cam: behind and above the fielder on
/// the far side from the ball, looking across him at the ball coming down.
pub(super) fn fielder_cam(fielder: Vec3, ball: Vec3) -> (Vec3, Vec3) {
    let away = Vec3::new(fielder.x - ball.x, 0.0, fielder.z - ball.z).normalize_or(Vec3::Z);
    let eye = fielder + away * FIELDER_CAM_BACK + Vec3::Y * FIELDER_CAM_HEIGHT;
    let target = ball.lerp(fielder + Vec3::Y, 0.5);
    (eye, target)
}

// ── Framing a shot ────────────────────────────────────────────────────────────

/// How fast the rig tracks a moving subject *within* a shot: the
/// exponential follow rate (1/s) every shot uses today.
pub(super) const FOLLOW_RATE: f32 = 5.0;

/// Where a shot puts the lens this frame: eye, look-at and vertical FOV,
/// plus how hard the rig chases them while the shot holds.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ShotSpec {
    pub eye: Vec3,
    pub target: Vec3,
    pub fov: f32,
    /// Exponential follow rate (1/s) toward this spec within the shot.
    pub follow: f32,
}

impl ShotSpec {
    fn new(eye: Vec3, target: Vec3, fov: f32) -> Self {
        Self {
            eye,
            target,
            fov,
            follow: FOLLOW_RATE,
        }
    }
}

/// How the rig gets from one shot to the next.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Transition {
    /// Snap to the new framing: a new subject announced by the edit (the
    /// grammar every shot uses today, TODO 102).
    Cut,
    /// Glide eye, look-at and lens to the new framing over `secs`, eased in
    /// and out.
    Ease { secs: f32 },
    /// Hold the eye and swing the look-at to the new subject over `secs`
    /// (fast in the middle), then cut the eye to the new framing.
    WhipPan { secs: f32 },
}

/// The live ball as a shot reads it.
#[derive(Clone, Copy, Debug)]
pub(super) struct BallState {
    pub pos: Vec3,
    pub vel: Vec3,
    pub spin: Vec3,
}

/// Everything a shot's framing reads, gathered once per frame.
pub(super) struct ShotInputs<'a> {
    pub field: &'a FieldSpec,
    /// The camera's width / height.
    pub aspect: f32,
    pub ball: Option<BallState>,
    /// The fielder the shot is about (the fielder cam's subject, the
    /// grounder's chaser), where it stands.
    pub fielder: Option<Vec3>,
    /// The runner cam's subject.
    pub runner: Option<RunnerFrame>,
    pub wind: Vec3,
    pub home_run: bool,
    /// Seconds since the trot orbit began.
    pub trot_secs: f32,
    /// Added to the duel views' field of view (radians; the DUEL VIEW FOV
    /// setting).
    pub duel_fov_nudge: f32,
    /// Reduce motion: the trot orbit holds its opening angle instead of
    /// sweeping.
    pub still: bool,
    /// Where a home run left the park (the crowd shot looks there).
    pub exit: Option<Vec3>,
}

impl Shot {
    /// The framing this shot wants this frame.
    pub(super) fn spec(self, inp: &ShotInputs) -> ShotSpec {
        let field = inp.field;
        let wide = || ShotSpec::new(field.broadcast_eye, field.broadcast_target, BROADCAST_FOV);
        match (self, inp.ball) {
            (Shot::Duel(v), _) => {
                let (eye, target, fov) = v.nudged_framing(field, inp.aspect, inp.duel_fov_nudge);
                ShotSpec::new(eye, target, fov)
            }
            (Shot::BallFollow, Some(ball)) if !inp.home_run => {
                let (eye, target, focus) = landing_zone_cam(ball, inp.wind, -field.broadcast_eye.z);
                // Fit where the ball is about to be too: the lens follows at a
                // finite rate, so it must already have room for the next
                // quarter-second of flight.
                let ahead = ball.pos + ball.vel * LEAD_SECS;
                let fov = fit_vfov(
                    eye,
                    target,
                    &[ball.pos, ahead, focus],
                    inp.aspect,
                    LEAD_MARGIN,
                )
                .clamp(BROADCAST_FOV, MAX_LEAD_FOV);
                ShotSpec {
                    follow: BALL_FOLLOW_RATE,
                    ..ShotSpec::new(eye, target, fov)
                }
            }
            (Shot::GroundBall, Some(ball)) => {
                let first = field.base_positions.first().copied().unwrap_or(Vec3::ZERO);
                let (eye, target, fov) = ground_ball_cam(ball.pos, inp.fielder, first, inp.aspect);
                ShotSpec::new(eye, target, fov)
            }
            (Shot::BallFollow, Some(ball)) => {
                let (eye, target) = home_run_follow(field, ball.pos);
                ShotSpec::new(eye, target, BROADCAST_FOV)
            }
            (Shot::BallFollow | Shot::FielderCam(_) | Shot::GroundBall, None) => wide(),
            (Shot::FielderCam(_), Some(ball)) => {
                let (eye, target) = fielder_cam(inp.fielder.unwrap_or(ball.pos), ball.pos);
                ShotSpec::new(eye, target, PLAY_CAM_FOV)
            }
            (Shot::RunnerCam(_), Some(ball)) => match inp.runner {
                Some(r) => {
                    let (eye, target, fov) = runner_cam(r, ball.pos, inp.aspect);
                    // Quick to follow: the throw is in frame at up to 40 m/s.
                    ShotSpec {
                        follow: BALL_FOLLOW_RATE,
                        ..ShotSpec::new(eye, target, fov)
                    }
                }
                None => wide(),
            },
            (Shot::RunnerCam(_), None) => wide(),
            (Shot::BaseCam(base), _) => {
                let (eye, target) = base_cam(field, base);
                ShotSpec::new(eye, target, PLAY_CAM_FOV)
            }
            (Shot::WalkUp, _) => ShotSpec::new(WALKUP_EYE, WALKUP_TARGET, WALKUP_FOV),
            (Shot::ReactionCam, _) => ShotSpec::new(REACTION_EYE, REACTION_TARGET, REACTION_FOV),
            (Shot::CrowdCam, _) => {
                let exit = inp.exit.unwrap_or(Vec3::new(0.0, 0.0, field.fence_center));
                let (eye, target) = crowd_cam(field, exit);
                ShotSpec::new(eye, target, CROWD_FOV)
            }
            (Shot::DiveCam(_), _) => match inp.fielder {
                Some(fielder) => {
                    let (eye, target) = dive_cam(fielder);
                    ShotSpec::new(eye, target, DIVE_FOV)
                }
                None => wide(),
            },
            (Shot::PileUp, _) => ShotSpec::new(PILE_UP_EYE, PILE_UP_TARGET, PILE_UP_FOV),
            (Shot::HalfInning, _) => {
                let (eye, target, fov) = half_inning_cam(field, inp.aspect);
                ShotSpec::new(eye, target, fov)
            }
            // Result pause of a home run: orbit the diamond while the batter
            // trots the bases. The azimuth sweeps only through the
            // behind-home arc, so the outfield sky, where the fireworks
            // burst, stays in frame for the whole show (TODO 70).
            (Shot::TrotOrbit, _) => {
                let focus = Vec3::new(field.broadcast_target.x, 1.4, field.broadcast_target.z);
                let secs = if inp.still { 0.0 } else { inp.trot_secs };
                let sweep = (secs * TROT_ORBIT_RATE - 0.9).clamp(-0.9, 0.9);
                let eye = trot_orbit_eye(focus, std::f32::consts::PI + sweep);
                ShotSpec::new(eye, focus, BROADCAST_FOV)
            }
        }
    }

    /// How the rig moves into this shot from `from`. Every shot cuts today.
    pub(super) fn transition_from(self, _from: Option<Shot>) -> Transition {
        Transition::Cut
    }
}

/// The transition to play: under reduce motion every ease and whip pan
/// becomes a cut, so the lens never glides or swings on its own.
pub(super) fn motion_safe(kind: Transition, reduce_motion: bool) -> Transition {
    if reduce_motion { Transition::Cut } else { kind }
}

/// The narrowest and widest a nudged duel lens may get (radians).
const DUEL_FOV_MIN: f32 = 20.0_f32.to_radians();
const DUEL_FOV_MAX: f32 = 90.0_f32.to_radians();

/// A duel view's lens with the player's nudge added, kept to a sane range.
pub(super) fn nudged_fov(fov: f32, nudge: f32) -> f32 {
    if nudge == 0.0 {
        return fov;
    }
    (fov + nudge).clamp(DUEL_FOV_MIN, DUEL_FOV_MAX)
}

/// A live, uncalled play: where the ball is coming down. The eye stations
/// itself between home and the predicted landing spot, a medium shot of the
/// drop zone, so the chasing fielder and the play about to happen are
/// framed, not just the ball. Returns the eye, the look-at and the
/// drop zone's focus (1 m over the predicted landing spot); the ball-follow then opens the lens until both
/// the ball and the landing spot sit in the middle of the frame, wider as
/// the ball climbs and tighter as it drops. `max_behind` caps how far
/// behind the plate the eye may go (the park's broadcast eye distance).
pub(super) fn landing_zone_cam(ball: BallState, wind: Vec3, max_behind: f32) -> (Vec3, Vec3, Vec3) {
    // Re-predict from the live ball; as the ball settles this converges to
    // the ball itself, so the shot lands with the play.
    let (landing, _) = rules::predict_landing_from(
        ball.pos,
        ball.vel,
        ball.spin,
        BALL_DRAG_FACTOR,
        MAGNUS_FACTOR,
        wind,
    );
    let focus = Vec3::new(landing.x, 1.0, landing.z);
    let flat = Vec2::new(focus.x, focus.z);
    let depth = flat.length();
    let line = flat.normalize_or(Vec2::Y);
    // Not too close: the eye parks back along the home→landing line from
    // the drop zone, higher and further for deeper plays, and for a ball
    // climbing high (a pop-up would otherwise go straight over the lens).
    // While the ball is still short of that spot the eye trails behind it
    // instead, so the ball is always ahead of the lens.
    let lift = ball.pos.y.max(0.0);
    let back = (depth * 0.45)
        .clamp(12.0, 30.0)
        .max((lift * 1.6).min(MAX_BACK));
    let height = (depth * 0.30).clamp(8.0, 18.0).max(lift * 0.4);
    let ball_along = Vec2::new(ball.pos.x, ball.pos.z).dot(line);
    // Never further behind the plate than the park's own broadcast eye,
    // which is known to be clear of the backstop and the scenery.
    let along = (depth - back).min(ball_along - TRAIL_BACK).max(-max_behind);
    // (And never behind it in world z either: for a ball landing behind
    // the plate the line itself points back toward the backstop.)
    let eye = Vec3::new(line.x * along, height, (line.y * along).max(-max_behind));
    // Look down the bisector of the ball and the drop zone as the eye sees
    // them, so a lens fitted to both opens the least: a high fly lifts the
    // look-at toward the ball, a grounder barely moves it off the spot.
    let to = |p: Vec3| (p - eye).normalize_or(Vec3::Z);
    let axis = (to(ball.pos) + to(focus)).normalize_or(to(focus));
    let target = eye + axis * (focus - eye).length().max(1.0);
    (eye, target, focus)
}

/// How far behind the ball (m, along the home→landing line) the
/// ball-follow's eye trails while the ball is still short of where the eye
/// parks.
const TRAIL_BACK: f32 = 14.0;
/// How far ahead along its flight (s) the ball-follow leaves room for the
/// ball, so the following lens never lets it slip toward the edge.
const LEAD_SECS: f32 = 0.25;
/// The furthest the ball-follow's eye backs off a high ball (m).
const MAX_BACK: f32 = 60.0;
/// The ball-follow tracks faster than the other shots: its eye trails a
/// ball flying at up to 45 m/s.
const BALL_FOLLOW_RATE: f32 = 10.0;

/// The runner cam: standoff from the runner's race (beyond half its
/// length) and height.
const RUNNER_CAM_BACK: f32 = 9.0;
const RUNNER_CAM_HEIGHT: f32 = 4.0;
const RUNNER_CAM_MIN_FOV: f32 = 40.0_f32.to_radians();
/// A throw is in from deep (worth the runner cam) when it leaves the
/// fielder's hand this far from home, as a fraction of the park's
/// foul-line fence distance: 75 m in the stadium, well past the middle
/// infielders' 45 m depth — a ball run down in the gap or the corner, not
/// a single gathered in shallow — and 28.5 m on the front lawn, so only
/// the across-the-street fielder (34 m) takes it, never the sidewalks
/// (23 m).
pub(super) const OUTFIELD_FRACTION: f32 = 0.75;
/// The runner cam hands over to the base cam once the throw is this many
/// seconds from the bag: a deep throw flies about 1.2 s, so the bag's shot
/// gets as long as the runner's — the slide and the tag read before the
/// call.
pub(super) const RUNNER_CAM_HANDOFF_SECS: f32 = 0.6;
/// A ball slower than this (m/s, flat) is no longer a throw in flight
/// (it bounced short or is rolling): the bag's shot takes over.
const THROW_MIN_SPEED: f32 = 5.0;

/// The runner cam only *starts* when the throw is at least this many
/// seconds from the bag: a shorter run would flash past before the hand-over.
pub(super) const RUNNER_CAM_MIN_START_SECS: f32 = 0.9;

/// Seconds until a throw at `ball` moving at `ball_vel` reaches `bag`, on
/// its flat speed (infinite for a ball not moving).
pub(super) fn throw_secs_left(bag: Vec3, ball: Vec3, ball_vel: Vec3) -> f32 {
    let flat = |v: Vec3| Vec2::new(v.x, v.z);
    let speed = flat(ball_vel).length();
    if speed <= f32::EPSILON {
        return f32::INFINITY;
    }
    flat(ball).distance(flat(bag)) / speed
}

/// The runner a long throw races: among `(rig, at, bound_for)` for every rig
/// still on a base path, the one bound for `thrown_bag` first (within
/// `arrived` metres), else the one nearest it — on a double the throw can go
/// in behind the batter, to first. Returns the rig and the bag it is bound
/// for.
pub(super) fn pick_runner_subject(
    candidates: impl IntoIterator<Item = (Entity, Vec3, Vec3)>,
    thrown_bag: Vec3,
    arrived: f32,
) -> Option<(Entity, Vec3)> {
    let flat = |v: Vec3| v.with_y(0.0);
    candidates
        .into_iter()
        .map(|(e, at, dest)| {
            let bound_elsewhere = flat(dest).distance(flat(thrown_bag)) > arrived;
            (e, dest, bound_elsewhere, at.distance(thrown_bag))
        })
        .min_by(|a, b| a.2.cmp(&b.2).then(a.3.total_cmp(&b.3)))
        .map(|(e, dest, _, _)| (e, dest))
}

/// Whether the rig should be on the runner cam: a throw in from the
/// outfield ([`OUTFIELD_FRACTION`]) from `throw_from` to `bag`, still in
/// flight and more than [`RUNNER_CAM_HANDOFF_SECS`] from the bag, while the
/// base cam hasn't taken the play yet (a cut back from the bag would read
/// as a stutter).
pub(super) fn runner_cam_wanted(
    throw_from: Vec3,
    bag: Vec3,
    ball: Vec3,
    ball_vel: Vec3,
    fence_line: f32,
    base_cam_shown: bool,
) -> bool {
    let flat = |v: Vec3| Vec2::new(v.x, v.z);
    !base_cam_shown
        && flat(throw_from).length() > fence_line * OUTFIELD_FRACTION
        && flat(ball_vel).length() > THROW_MIN_SPEED
        && throw_secs_left(bag, ball, ball_vel) > RUNNER_CAM_HANDOFF_SECS
}

/// The runner cam: standing off the runner's race to the bag he is bound
/// for, swung round to the side away from the throw so the ball comes in
/// from beyond the runner (a throw from behind him — right field on a run to
/// third — would otherwise come from behind the lens), the lens fitted to
/// the runner, his bag, the bag the call lands on and the ball.
pub(super) fn runner_cam(r: RunnerFrame, ball: Vec3, aspect: f32) -> (Vec3, Vec3, f32) {
    let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z);
    let (runner, bag) = (r.runner, r.bound_for);
    let run = flat(bag - runner).normalize_or(flat(bag).normalize_or(Vec3::Z));
    // The ground the shot must hold: the race and the bag the call lands on.
    let ground = [runner, bag, r.call_bag];
    let centre = ground.iter().copied().sum::<Vec3>() / 3.0;
    let from_ball = flat(centre - ball).normalize_or(-run);
    // Outward from the diamond, for a ball straight behind the runner
    // (the two directions cancel).
    let side = Vec3::new(-run.z, 0.0, run.x);
    let outward = if side.dot(flat(centre)) >= 0.0 {
        side
    } else {
        -side
    };
    // Mostly straight away from the ball (so it comes in from beyond the
    // race), drawn a little behind the runner.
    let back = (from_ball * 2.0 - run).normalize_or(outward);
    let spread = ground
        .iter()
        .map(|p| flat(*p - centre).length())
        .fold(0.0, f32::max);
    let reach = RUNNER_CAM_BACK + spread;
    let eye = centre.with_y(0.0) + back * reach + Vec3::Y * RUNNER_CAM_HEIGHT;
    let subjects = [
        runner + Vec3::Y,
        bag + Vec3::Y * 0.5,
        r.call_bag + Vec3::Y * 0.5,
        ball,
    ];
    // Look between the runner's race and the ball, leaning to the race.
    let target = (centre + Vec3::Y * 0.75).lerp(ball, 0.25);
    let fov = fit_vfov(eye, target, &subjects, aspect, LEAD_MARGIN)
        .clamp(RUNNER_CAM_MIN_FOV, MAX_LEAD_FOV);
    (eye, target, fov)
}

/// How far from the frame's centre (in NDC) the lead shots keep their
/// subjects: the middle 80%.
pub(super) const LEAD_MARGIN: f32 = 0.8;
/// The widest the lens opens to keep a climbing ball and its landing spot
/// both in frame.
pub(super) const MAX_LEAD_FOV: f32 = 80.0_f32.to_radians();
/// The ground-ball shot: eye height and distance behind home, and its
/// narrowest lens.
const GROUND_CAM_HEIGHT: f32 = 11.0;
const GROUND_CAM_BACK: f32 = 9.0;
const GROUND_CAM_MIN_FOV: f32 = 40.0_f32.to_radians();
/// A ball counts as hit on the ground when it is this low and would be
/// down again within [`GROUNDER_HANG`] seconds.
const GROUNDER_HEIGHT: f32 = 1.5;
const GROUNDER_HANG: f32 = 1.0;

/// The runner cam's subject: where the runner is, the bag he is bound for,
/// and the bag the throw is going to (the call lands there; it can differ,
/// e.g. a throw in behind the batter on a double).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct RunnerFrame {
    pub runner: Vec3,
    pub bound_for: Vec3,
    pub call_bag: Vec3,
}

/// What the rig has learned about this play's batted ball, fed once a frame
/// while it is live ([`PlayRead::observe`]) and cleared between plays.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct PlayRead {
    grounder: Option<bool>,
    landed: bool,
}

impl PlayRead {
    /// One frame of the live play. `ready` is the first frame after the
    /// plate hold: the grounder call is made **once**, at that frame, and
    /// kept for the whole play — a ball that already touched down during
    /// the hold (`landed_at`, on the contact clock: fielding's first-bounce
    /// mark) is a grounder, else the ball's state then decides
    /// ([`is_grounder`]). Deciding later would read every fly ball as a
    /// grounder in its last half-metre, and a held ball as one too.
    /// `off_wall` is the ball having banged off the fence.
    pub fn observe(
        &mut self,
        ready: bool,
        ball: Option<BallState>,
        landed_at: Option<f32>,
        off_wall: bool,
        wind: Vec3,
    ) {
        if self.grounder.is_none()
            && ready
            && let Some(b) = ball
        {
            // A touchdown after the hold is a fly or a liner coming down:
            // the call can be that late only when the broadcast camera was
            // switched on mid-play.
            self.grounder = Some(match landed_at {
                Some(t) => t <= BALL_FOLLOW_DELAY,
                None => is_grounder(b, wind),
            });
        }
        // Only after the call: on the contact frame `landed_at` can still
        // hold the last play's mark.
        if self.grounder.is_some() && (landed_at.is_some() || off_wall) {
            self.landed = true;
        }
    }

    /// The grounder call, once made.
    pub fn grounder(&self) -> Option<bool> {
        self.grounder
    }

    /// The ball has come down (or off the wall) since the call: it is not
    /// caught on the way down any more, so the fielder cam is done for the
    /// play.
    pub fn landed(&self) -> bool {
        self.landed
    }
}

/// True for a ball on (or skimming) the ground: low, and coming down at
/// once. Only meaningful off the bat; see [`PlayRead::observe`].
pub(super) fn is_grounder(ball: BallState, wind: Vec3) -> bool {
    if ball.pos.y > GROUNDER_HEIGHT {
        return false;
    }
    let (_, hang) = rules::predict_landing_from(
        ball.pos,
        ball.vel,
        ball.spin,
        BALL_DRAG_FACTOR,
        MAGNUS_FACTOR,
        wind,
    );
    hang < GROUNDER_HANG
}

/// The ground-ball shot: high behind home, looking at the middle of the
/// ball, the fielder going for it and first base, the lens opened just
/// enough to keep all three in the middle of the frame.
pub(super) fn ground_ball_cam(
    ball: Vec3,
    fielder: Option<Vec3>,
    first: Vec3,
    aspect: f32,
) -> (Vec3, Vec3, f32) {
    let ball = ball.with_y(ball.y.max(0.3));
    let mut subjects = vec![ball, first + Vec3::Y * 0.5];
    subjects.extend(fielder.map(|f| f + Vec3::Y));
    let centre = subjects.iter().copied().sum::<Vec3>() / subjects.len() as f32;
    // Behind home on the line through the play's centre, so the infield
    // opens out in front of the lens.
    let flat = Vec2::new(centre.x, centre.z);
    let away = -flat.normalize_or(Vec2::Y);
    let eye = Vec3::new(
        away.x * GROUND_CAM_BACK,
        GROUND_CAM_HEIGHT,
        away.y * GROUND_CAM_BACK,
    );
    // Lean the look-at toward the ball, the subject that must never leave.
    let target = centre.lerp(ball, 0.35);
    let fov = fit_vfov(eye, target, &subjects, aspect, LEAD_MARGIN)
        .clamp(GROUND_CAM_MIN_FOV, MAX_LEAD_FOV);
    (eye, target, fov)
}

/// Home-run flights: sweep with the ball. The eye slides laterally and
/// pulls up and back as it travels deep.
pub(super) fn home_run_follow(field: &FieldSpec, ball: Vec3) -> (Vec3, Vec3) {
    let target = Vec3::new(ball.x, ball.y.max(1.0), ball.z);
    let depth = (ball.z * 0.18).clamp(0.0, 22.0);
    let eye = field.broadcast_eye + Vec3::new(ball.x * 0.4, depth * 0.6 + ball.y * 0.15, -depth);
    (eye, target)
}

// ── Moving between shots ──────────────────────────────────────────────────────

/// A transition in progress: the framing it started from and when.
#[derive(Clone, Copy, Debug)]
struct Blend {
    from: ShotSpec,
    kind: Transition,
    elapsed: f32,
}

/// The rig's lens state: where it is now, and any transition under way.
#[derive(Clone, Copy, Debug)]
pub(super) struct RigMotion {
    pub eye: Vec3,
    pub target: Vec3,
    pub fov: f32,
    blend: Option<Blend>,
}

impl RigMotion {
    pub(super) fn new(eye: Vec3, target: Vec3, fov: f32) -> Self {
        Self {
            eye,
            target,
            fov,
            blend: None,
        }
    }

    fn current(&self) -> ShotSpec {
        ShotSpec::new(self.eye, self.target, self.fov)
    }

    /// True while a transition is still moving the lens.
    #[cfg(test)]
    pub(super) fn blending(&self) -> bool {
        self.blend.is_some()
    }

    /// Start moving to a new shot. A cut lands this frame; an ease or a
    /// whip pan starts from wherever the lens is now and takes this frame's
    /// first step. A transition with no usable length (zero, negative or
    /// not finite) is a cut.
    pub(super) fn start(&mut self, kind: Transition, to: ShotSpec, dt: f32) {
        match kind {
            Transition::Ease { secs } | Transition::WhipPan { secs }
                if secs.is_finite() && secs > 0.0 =>
            {
                self.blend = Some(Blend {
                    from: self.current(),
                    kind,
                    elapsed: 0.0,
                });
                self.step(to, dt);
            }
            _ => {
                self.blend = None;
                self.set(to);
            }
        }
    }

    fn set(&mut self, s: ShotSpec) {
        self.eye = s.eye;
        self.target = s.target;
        self.fov = s.fov;
    }

    /// Advance one frame toward `want`: along the transition while one is
    /// under way, otherwise the shot's own follow.
    pub(super) fn step(&mut self, want: ShotSpec, dt: f32) {
        if let Some(mut b) = self.blend {
            b.elapsed += dt;
            let secs = match b.kind {
                Transition::Ease { secs } | Transition::WhipPan { secs } => secs,
                Transition::Cut => 0.0,
            };
            let t = (b.elapsed / secs).clamp(0.0, 1.0);
            if t >= 1.0 {
                self.blend = None;
                self.set(want);
                return;
            }
            self.blend = Some(b);
            match b.kind {
                Transition::WhipPan { .. } => {
                    let s = whip(t);
                    self.eye = b.from.eye;
                    self.target = b.from.target.lerp(want.target, s);
                    self.fov = b.from.fov;
                }
                _ => {
                    let s = smoothstep(t);
                    self.eye = b.from.eye.lerp(want.eye, s);
                    self.target = b.from.target.lerp(want.target, s);
                    self.fov = b.from.fov + (want.fov - b.from.fov) * s;
                }
            }
            return;
        }
        // Within a shot: critically-damped-ish smoothing so the ball-follow
        // and fielder cam track their subjects without jitter.
        let k = 1.0 - (-want.follow * dt).exp();
        self.eye = self.eye.lerp(want.eye, k);
        self.target = self.target.lerp(want.target, k);
        self.fov += (want.fov - self.fov) * k;
    }
}

fn smoothstep(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// A whip pan's curve: slow off the mark, a blur through the middle, a
/// quick settle (smootherstep).
fn whip(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

#[cfg(test)]
#[path = "shots.test.rs"]
mod tests;
