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
    /// The result pause is a strikeout's.
    pub strikeout: bool,
    /// The base the live ball was last thrown at, if any.
    pub thrown_base: Option<usize>,
    /// The ball was hit on the ground ([`is_grounder`], latched by the rig
    /// for the rest of the play).
    pub grounder: bool,
    /// The chasing fielder, when the ball is in the air and coming down —
    /// the fielder cam's subject.
    pub descending_to: Option<Entity>,
    /// The bag a running-game throw (a pickoff, the catcher's throw on a
    /// steal) is staged at, while it is (`Play::duel_throw`).
    pub duel_throw_base: Option<usize>,
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
        Phase::PrePitch if cues.walkup => Shot::WalkUp,
        Phase::PrePitch | Phase::WindUp | Phase::Pitch => Shot::Duel(cues.view),
        // Fresh contact: hold the plate framing for a beat — the crack and
        // the bat coming through — then cut to the ball.
        Phase::InPlay if cues.since_contact < BALL_FOLLOW_DELAY => Shot::Duel(cues.view),
        Phase::InPlay if cues.home_run => Shot::BallFollow,
        Phase::InPlay => match (cues.thrown_base, cues.descending_to) {
            (Some(base), _) => Shot::BaseCam(base),
            (None, Some(fielder)) => Shot::FielderCam(fielder),
            (None, None) if cues.grounder => Shot::GroundBall,
            (None, None) => Shot::BallFollow,
        },
        Phase::Result if cues.home_run || cues.walk_off => Shot::TrotOrbit,
        // Strike three gets its reaction shot (before the gloved arm: a K
        // into the mitt is gloved too).
        Phase::Result if cues.strikeout => Shot::ReactionCam,
        // A gloved pitch's call doesn't deserve a zoom-out: stay at the plate.
        Phase::Result if cues.pitch_gloved => Shot::Duel(cues.view),
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
    pub wind: Vec3,
    pub home_run: bool,
    /// Seconds since the trot orbit began.
    pub trot_secs: f32,
}

impl Shot {
    /// The framing this shot wants this frame.
    pub(super) fn spec(self, inp: &ShotInputs) -> ShotSpec {
        let field = inp.field;
        let wide = || ShotSpec::new(field.broadcast_eye, field.broadcast_target, BROADCAST_FOV);
        match (self, inp.ball) {
            (Shot::Duel(v), _) => {
                let (eye, target, fov) = v.framing(field, inp.aspect);
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
            (Shot::BaseCam(base), _) => {
                let (eye, target) = base_cam(field, base);
                ShotSpec::new(eye, target, PLAY_CAM_FOV)
            }
            (Shot::WalkUp, _) => ShotSpec::new(WALKUP_EYE, WALKUP_TARGET, WALKUP_FOV),
            (Shot::ReactionCam, _) => ShotSpec::new(REACTION_EYE, REACTION_TARGET, REACTION_FOV),
            // Result pause of a home run: orbit the diamond while the batter
            // trots the bases. The azimuth sweeps only through the
            // behind-home arc, so the outfield sky, where the fireworks
            // burst, stays in frame for the whole show (TODO 70).
            (Shot::TrotOrbit, _) => {
                let focus = Vec3::new(field.broadcast_target.x, 1.4, field.broadcast_target.z);
                let sweep = (inp.trot_secs * TROT_ORBIT_RATE - 0.9).clamp(-0.9, 0.9);
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
            let down_in_hold = landed_at.is_some_and(|t| t < BALL_FOLLOW_DELAY);
            self.grounder = Some(down_in_hold || is_grounder(b, wind));
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
