//! Camera rigs: the broadcast and orbit systems that actually move the
//! `Camera3d`, plus the occlusion pass that hides whoever is standing in
//! the broadcast lens's way. Reads the pure math in [`super::framing`] and
//! the mode/duel-view state owned by [`super`].

use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy_rapier3d::prelude::Velocity;

use crate::game::ball::{BALL_DRAG_FACTOR, Baseball, HitEvent, MAGNUS_FACTOR, WallBangEvent};
use crate::game::fielding::ActivePlay;
use crate::game::flow::{LiveBallEvent, Phase, Play, ResultBeat};
use crate::game::player::{CatcherRole, PlateUmpire};
use crate::game::rules;
use crate::game::variant::FieldSpec;

use super::framing::{
    LENS_BRUSH_RADIUS, OCCLUSION_NEAR, OCCLUSION_RADIUS, TROT_ORBIT_RATE, brushes_lens, occludes,
    trot_orbit_eye,
};
use super::{
    BALL_FOLLOW_DELAY, BROADCAST_EYE, BROADCAST_FOV, BROADCAST_HOME_TARGET, CameraMode,
    DUEL_REFERENCE_ASPECT, DuelView,
};

// ── Occlusion ─────────────────────────────────────────────────────────────────

/// Hides the catcher/plate umpire root(s) that sit in the way of the active
/// duel view for as long as they do, and restores them the rest of the
/// time — outside the duel phases (ball in play, result pause) or on a view
/// change that clears the block. Root-level `Visibility` is the same
/// mechanism run-out rigs use to swap the batter for his stand-in
/// (`runner.rs`), so this never fights that: it only ever touches
/// `CatcherRole`/`PlateUmpire` roots, which never run bases.
///
/// The catcher-POV view gets its own arm: its eye sits fractionally
/// *inside* the catcher's silhouette (see `FieldSpec::duel_eye`), behind
/// his forward surface, where the `occludes` cone (which only looks ahead
/// of the eye) can't see him — so in that view the catcher is hidden
/// outright whenever the duel framing is wanted or still held.
#[allow(clippy::type_complexity)]
pub(super) fn hide_occluders(
    view: Res<DuelView>,
    field: Res<FieldSpec>,
    rig: Res<BroadcastRig>,
    mode: Res<CameraMode>,
    mut subjects: Query<
        (&Transform, &mut Visibility, Has<CatcherRole>),
        Or<(With<CatcherRole>, With<PlateUmpire>)>,
    >,
) {
    // While the lens is parked at the plate — the duel itself, the
    // post-contact hold, and a gloved pitch's result pause — nothing may
    // pop into it. Gating on the duel phases alone let the plate umpire
    // stand up out of his crouch straight into the parked catcher-POV lens
    // during the result pause (playtest 2026-08-20).
    // The rig's shot is the source of truth: the duel framing is "held"
    // exactly while a `Shot::Duel` is on screen (the duel itself, the
    // post-contact hold, a gloved pitch's result pause).
    let framing_held = *mode == CameraMode::Broadcast && matches!(rig.shot(), Some(Shot::Duel(_)));
    let pov_at_plate = framing_held && *view == DuelView::CatcherPov;
    // The FOV this call computes is discarded (occlusion only cares about the
    // eye/target axis), so the aspect passed through doesn't matter — the
    // reference aspect keeps this a no-op correction.
    let (eye, target, _) = view.framing(&field, DUEL_REFERENCE_ASPECT);
    for (transform, mut visibility, _is_catcher) in &mut subjects {
        // Occlusion only makes sense for the camera actually looking through
        // this axis: in Orbit the player is free-looking with a completely
        // different eye/target, so a rig hidden for a Broadcast duel view
        // must not stay hidden just because the view resource hasn't
        // changed — the system still runs every frame (unconditionally, not
        // gated out entirely) so switching back to Broadcast, or into
        // Orbit, both re-evaluate and settle on the right state immediately.
        //
        // Catcher-POV hides both plate rigs outright: the eye sits inside
        // the catcher's silhouette with the umpire crouched *behind* it,
        // where a look-ahead cone can never flag him, yet his geometry
        // pokes through the near plane.
        // Any tight view also hides a body the lens is parked against —
        // beside or behind the eye, where the look-ahead cone is blind
        // (the front-yard umpire under the default batting view).
        let blocking = pov_at_plate
            || (framing_held
                && (occludes(
                    eye,
                    target,
                    transform.translation,
                    OCCLUSION_NEAR,
                    OCCLUSION_RADIUS,
                ) || brushes_lens(eye, transform.translation, LENS_BRUSH_RADIUS)));
        let desired = if blocking {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        if *visibility != desired {
            *visibility = desired;
        }
    }
}

// ── Orbit state ───────────────────────────────────────────────────────────────

#[derive(Resource)]
pub struct OrbitState {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub target: Vec3,
}

impl Default for OrbitState {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: 0.6,
            distance: 60.0,
            target: Vec3::new(0.0, 0.0, 30.0),
        }
    }
}

/// One subject per shot (TODO 102; the reference grammar in
/// docs/agent/SMB3-REFERENCE-NOTES.md §2.5–2.6). A change of `Shot` is a
/// **hard cut** — the rig snaps to the new framing — and smoothing only ever
/// runs *within* a shot (the ball-follow and the fielder cam tracking their
/// subjects). The old single rig glided 20–30 m across the park between
/// framings; a cut announces the new subject instead.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Shot {
    /// The duel framing of the active [`DuelView`] (a V press is a cut too).
    Duel(DuelView),
    /// Chasing the live ball toward where it comes down.
    BallFollow,
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
    pub pitch_gloved: bool,
    pub view: DuelView,
    /// The walk-up hold is on (`Play::walkup_active`).
    pub walkup: bool,
    /// The result pause is a strikeout's.
    pub strikeout: bool,
    /// The base the live ball was last thrown at, if any.
    pub thrown_base: Option<usize>,
    /// The chasing fielder, when the ball is in the air and coming down —
    /// the fielder cam's subject.
    pub descending_to: Option<Entity>,
}

/// Which shot the play wants right now. `current` is the shot on screen: a
/// result pause that isn't a gloved pitch or a home run *keeps* it, so the
/// call is announced in the shot the play ended on (SAFE! at the bag, OUT
/// over the fielder) instead of cutting away to a wide hold.
pub(super) fn pick_shot(cues: &ShotCues, current: Option<Shot>) -> Shot {
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
            (None, None) => Shot::BallFollow,
        },
        Phase::Result if cues.home_run => Shot::TrotOrbit,
        // Strike three gets its reaction shot (before the gloved arm: a K
        // into the mitt is gloved too).
        Phase::Result if cues.strikeout => Shot::ReactionCam,
        // A gloved pitch's call doesn't deserve a zoom-out: stay at the plate.
        Phase::Result if cues.pitch_gloved => Shot::Duel(cues.view),
        Phase::Result => current.unwrap_or(Shot::Duel(cues.view)),
    }
}

/// The broadcast camera's state: the shot on screen, and the smoothed
/// eye + look-at + FOV that track *within* it.
#[derive(Resource)]
pub struct BroadcastRig {
    eye: Vec3,
    target: Vec3,
    fov: f32,
    shot: Option<Shot>,
    /// The base the live ball was last thrown at, cleared once the shot
    /// returns to the duel.
    thrown_base: Option<usize>,
}

impl Default for BroadcastRig {
    fn default() -> Self {
        Self {
            eye: BROADCAST_EYE,
            target: BROADCAST_HOME_TARGET,
            fov: BROADCAST_FOV,
            shot: None,
            thrown_base: None,
        }
    }
}

impl BroadcastRig {
    /// The shot currently on screen (`None` before the first frame).
    pub fn shot(&self) -> Option<Shot> {
        self.shot
    }
}

/// Impulse added to the broadcast eye on contact; decays on real time so the
/// kick rides through the hit-stop.
#[derive(Resource, Default)]
pub(super) struct CameraKick(Vec3);

/// The live ball as the broadcast camera reads it.
type BallQuery<'w, 's> =
    Query<'w, 's, (&'static Transform, &'static Velocity), (With<Baseball>, Without<Camera3d>)>;

pub(super) fn kick_on_hit(mut hits: MessageReader<HitEvent>, mut kick: ResMut<CameraKick>) {
    for _ in hits.read() {
        kick.0 += Vec3::new(0.0, 0.18, -0.35);
    }
}

/// A smaller thump when the ball bangs off the outfield wall.
pub(super) fn kick_on_wall_bang(
    mut bangs: MessageReader<WallBangEvent>,
    mut kick: ResMut<CameraKick>,
) {
    for _ in bangs.read() {
        kick.0 += Vec3::new(0.0, 0.10, 0.20);
    }
}

pub(super) fn decay_kick(real: Res<Time<Real>>, mut kick: ResMut<CameraKick>) {
    kick.0 *= (-14.0 * real.delta_secs()).exp();
}

// ── Broadcast camera ──────────────────────────────────────────────────────────

/// Vertical FOV of the fielder and base cams: a medium lens, tight enough
/// that the ball and the bag read large, wide enough for the runner and the
/// throw to share the frame.
const PLAY_CAM_FOV: f32 = 45.0_f32.to_radians();
/// A fly ball must still be this high (metres) for the fielder cam to take
/// over — below it the catch is a moment away and the ball-follow already
/// has it.
const FIELDER_CAM_MIN_HEIGHT: f32 = 2.5;
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

#[allow(clippy::too_many_arguments)]
pub(super) fn broadcast_camera(
    time: Res<Time>,
    play: Res<Play>,
    field: Res<FieldSpec>,
    view: Res<DuelView>,
    kick: Res<CameraKick>,
    active: Res<ActivePlay>,
    mut live: MessageReader<LiveBallEvent>,
    ball_q: BallQuery,
    rigs: Query<&Transform, (Without<Baseball>, Without<Camera3d>)>,
    mut rig: ResMut<BroadcastRig>,
    mut trot_start: Local<Option<f32>>,
    mut cam_q: Query<(&mut Transform, &mut Projection), With<Camera3d>>,
) {
    // The camera's actual aspect ratio (width / height), read before the
    // framing decision so the duel FOV can correct for it — see
    // `aspect_safe_duel_vfov`. Falls back to the reference aspect (a no-op
    // correction) before the camera exists.
    let aspect = match cam_q.single() {
        Ok((_, Projection::Perspective(persp))) => persp.aspect_ratio,
        _ => DUEL_REFERENCE_ASPECT,
    };

    // A throw is the cue for the base cam: the race is decided at the
    // throw and announced at the arrival (`flow::resolve_live_play`), so
    // this is exactly the shot the call lands in.
    for ev in live.read() {
        if let LiveBallEvent::Thrown { base, .. } = *ev {
            rig.thrown_base = Some(base);
        }
    }

    let now = time.elapsed_secs();
    let ball = ball_q.single().ok();
    let descending_to = ball.and_then(|(tf, vel)| {
        let coming_down = tf.translation.y > FIELDER_CAM_MIN_HEIGHT && vel.linvel.y < 0.0;
        active
            .chaser()
            .filter(|f| coming_down && rigs.get(*f).is_ok())
    });
    let cues = ShotCues {
        phase: play.phase,
        since_contact: play.since_contact(now),
        home_run: play.is_home_run(),
        pitch_gloved: play.pitch_gloved(),
        view: *view,
        walkup: play.walkup_active(),
        strikeout: play.result_beat() == Some(ResultBeat::Strikeout),
        thrown_base: rig.thrown_base,
        descending_to,
    };
    let shot = pick_shot(&cues, rig.shot);

    // Frame the shot.
    let (desired_eye, desired_target, desired_fov) = match (shot, ball) {
        (Shot::Duel(v), _) => v.framing(&field, aspect),
        // A live, uncalled play: where the ball is coming down. The eye
        // stations itself between home and the predicted landing spot —
        // a medium shot of the drop zone, so the chasing fielder and the
        // play about to happen are what's framed, not just the ball.
        (Shot::BallFollow, Some((ball, vel))) if !play.is_home_run() => {
            // Re-predict from the live ball; as the ball settles this
            // converges to the ball itself, so the shot lands with the play.
            let (landing, _) = rules::predict_landing_from(
                ball.translation,
                vel.linvel,
                vel.angvel,
                BALL_DRAG_FACTOR,
                MAGNUS_FACTOR,
            );
            let focus = Vec3::new(landing.x, 1.0, landing.z);
            // Keep the ball's flight in frame: a high fly pulls the look-at
            // up toward it, a grounder barely moves it off the landing spot.
            let target = focus.lerp(ball.translation, 0.5);

            let flat = Vec2::new(focus.x, focus.z);
            let depth = flat.length();
            // Not too close: back off along the home→landing line, higher
            // and further for deeper plays.
            let back = (depth * 0.45).clamp(12.0, 30.0);
            let height = (depth * 0.30).clamp(8.0, 18.0);
            let toward_home = -flat.normalize_or_zero();
            let eye = focus + Vec3::new(toward_home.x * back, height, toward_home.y * back);
            (eye, target, BROADCAST_FOV)
        }
        // Home-run flights: sweep with the ball — the eye slides laterally
        // and pulls up and back as it travels deep. (Any other called play
        // keeps the landing-zone framing above through its result pause —
        // the shot the call landed in, not a glide out to the wide plate.)
        (Shot::BallFollow, Some((ball, _))) => {
            let target = Vec3::new(
                ball.translation.x,
                ball.translation.y.max(1.0),
                ball.translation.z,
            );
            let depth = (ball.translation.z * 0.18).clamp(0.0, 22.0);
            let eye = field.broadcast_eye
                + Vec3::new(
                    ball.translation.x * 0.4,
                    depth * 0.6 + ball.translation.y * 0.15,
                    -depth,
                );
            (eye, target, BROADCAST_FOV)
        }
        (Shot::BallFollow, None) => (field.broadcast_eye, field.broadcast_target, BROADCAST_FOV),
        (Shot::FielderCam(fielder), Some((ball, _))) => {
            let fielder_pos = rigs
                .get(fielder)
                .map(|tf| tf.translation)
                .unwrap_or(ball.translation);
            let (eye, target) = fielder_cam(fielder_pos, ball.translation);
            (eye, target, PLAY_CAM_FOV)
        }
        (Shot::FielderCam(_), None) => (field.broadcast_eye, field.broadcast_target, BROADCAST_FOV),
        (Shot::BaseCam(base), _) => {
            let (eye, target) = base_cam(&field, base);
            (eye, target, PLAY_CAM_FOV)
        }
        (Shot::WalkUp, _) => (WALKUP_EYE, WALKUP_TARGET, WALKUP_FOV),
        (Shot::ReactionCam, _) => (REACTION_EYE, REACTION_TARGET, REACTION_FOV),
        // Result pause of a home run: orbit the diamond while the batter
        // trots the bases — a sweeping victory-lap shot. The azimuth is
        // seeded per trot and swept only through the behind-home arc, so
        // the outfield sky — where the fireworks burst — stays in frame for
        // the whole show (the old wall-clock phase started the orbit
        // anywhere and faced away half the time — TODO 70).
        (Shot::TrotOrbit, _) => {
            let focus = Vec3::new(field.broadcast_target.x, 1.4, field.broadcast_target.z);
            let start = *trot_start.get_or_insert(now);
            let sweep = ((now - start) * TROT_ORBIT_RATE - 0.9).clamp(-0.9, 0.9);
            let eye = trot_orbit_eye(focus, std::f32::consts::PI + sweep);
            (eye, focus, BROADCAST_FOV)
        }
    };

    // The trot seed lives only while the trot shot does.
    if shot != Shot::TrotOrbit {
        *trot_start = None;
    }
    if matches!(shot, Shot::Duel(_)) {
        rig.thrown_base = None;
    }

    if rig.shot != Some(shot) {
        // A new subject: cut. Snapping is what makes the framing change
        // read as a shot and not a fly-through.
        rig.shot = Some(shot);
        rig.eye = desired_eye;
        rig.target = desired_target;
        rig.fov = desired_fov;
    } else {
        // Within a shot: critically-damped-ish smoothing so the ball-follow
        // and fielder cam track their subjects without jitter.
        let follow = 1.0 - (-5.0 * time.delta_secs()).exp();
        rig.eye = rig.eye.lerp(desired_eye, follow);
        rig.target = rig.target.lerp(desired_target, follow);
        rig.fov += (desired_fov - rig.fov) * follow;
    }

    if let Ok((mut cam, mut projection)) = cam_q.single_mut() {
        *cam = Transform::from_translation(rig.eye + kick.0).looking_at(rig.target, Vec3::Y);
        if let Projection::Perspective(persp) = projection.as_mut() {
            persp.fov = rig.fov;
        }
    }
}

// ── Orbit camera (free look) ──────────────────────────────────────────────────

/// The orbit's key controls only engage while a Shift is held: WASD/arrows
/// are the *gameplay* aim keys for both players, and the free-look camera
/// stealing them mid-play meant spinning the camera with every pitch aim
/// (TODO 64). The mouse wheel (see [`zoom_camera`]) stays modifier-free —
/// it has no gameplay meaning.
fn orbit_modifier_held(keyboard: &ButtonInput<KeyCode>) -> bool {
    keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight)
}

pub(super) fn orbit_camera(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut orbit: ResMut<OrbitState>,
    mut camera_query: Query<&mut Transform, With<Camera3d>>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();
    let yaw_speed = 1.2_f32;
    let pitch_speed = 0.8_f32;

    let mut yaw_delta = 0.0_f32;
    let mut pitch_delta = 0.0_f32;

    if orbit_modifier_held(&keyboard) {
        if keyboard.pressed(KeyCode::ArrowLeft) || keyboard.pressed(KeyCode::KeyA) {
            yaw_delta -= yaw_speed * dt;
        }
        if keyboard.pressed(KeyCode::ArrowRight) || keyboard.pressed(KeyCode::KeyD) {
            yaw_delta += yaw_speed * dt;
        }
        if keyboard.pressed(KeyCode::ArrowUp) || keyboard.pressed(KeyCode::KeyW) {
            pitch_delta += pitch_speed * dt;
        }
        if keyboard.pressed(KeyCode::ArrowDown) || keyboard.pressed(KeyCode::KeyS) {
            pitch_delta -= pitch_speed * dt;
        }
    }

    orbit.yaw += yaw_delta;
    orbit.pitch = (orbit.pitch + pitch_delta).clamp(0.1, std::f32::consts::FRAC_PI_2 - 0.05);

    if orbit_modifier_held(&keyboard) && keyboard.just_pressed(KeyCode::KeyR) {
        *orbit = OrbitState::default();
    }

    let transform = orbit_transform(&orbit);
    for mut cam_transform in &mut camera_query {
        ease_toward(&mut cam_transform, &transform, dt);
    }
}

/// Exponential ease used by both orbit writers: the broadcast rig glides
/// between framings, but C used to hard-cut into (and out of) orbit because
/// these systems assigned the transform directly (TODO 69). Fast enough to
/// feel 1:1 under held keys, soft enough that the mode switch reads as a
/// move.
fn ease_toward(current: &mut Transform, target: &Transform, dt: f32) {
    let s = 1.0 - (-8.0 * dt).exp();
    current.translation = current.translation.lerp(target.translation, s);
    current.rotation = current.rotation.slerp(target.rotation, s);
}

pub(super) fn zoom_camera(
    mut scroll: MessageReader<MouseWheel>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut orbit: ResMut<OrbitState>,
    mut camera_query: Query<&mut Transform, With<Camera3d>>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();

    let mut zoom_delta = 0.0_f32;
    for ev in scroll.read() {
        zoom_delta -= ev.y * 3.0;
    }
    // Q/E ride the same Shift modifier as the orbit keys (TODO 64); the
    // wheel needs none.
    if orbit_modifier_held(&keyboard) {
        if keyboard.pressed(KeyCode::KeyQ) {
            zoom_delta -= 15.0 * dt;
        }
        if keyboard.pressed(KeyCode::KeyE) {
            zoom_delta += 15.0 * dt;
        }
    }

    orbit.distance = (orbit.distance + zoom_delta).clamp(10.0, 200.0);

    let transform = orbit_transform(&orbit);
    for mut cam_transform in &mut camera_query {
        ease_toward(&mut cam_transform, &transform, dt);
    }
}

fn orbit_transform(orbit: &OrbitState) -> Transform {
    let offset = Vec3::new(
        orbit.distance * orbit.yaw.sin() * orbit.pitch.cos(),
        orbit.distance * orbit.pitch.sin(),
        orbit.distance * orbit.yaw.cos() * orbit.pitch.cos(),
    );
    Transform::from_translation(orbit.target + offset).looking_at(orbit.target, Vec3::Y)
}

#[cfg(test)]
#[path = "rigs.test.rs"]
mod tests;
