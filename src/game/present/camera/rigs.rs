//! Camera rigs: the broadcast and orbit systems that actually move the
//! `Camera3d`, plus the occlusion pass that hides whoever is standing in
//! the broadcast lens's way. Reads the shot catalog in [`super::shots`]
//! (which shot, its framing, how to move between shots), the pure math in
//! [`super::framing`], and the mode/duel-view state owned by [`super`].

use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy_rapier3d::prelude::Velocity;

use crate::game::ball::{Baseball, HitEvent, WallBangEvent};
use crate::game::fielding::ActivePlay;
use crate::game::flow::{LiveBallEvent, Phase, Play, ResultBeat};
use crate::game::player::{CatcherRole, PlateUmpire};
use crate::game::rules;
use crate::game::variant::FieldSpec;

use super::framing::{LENS_BRUSH_RADIUS, OCCLUSION_NEAR, OCCLUSION_RADIUS, brushes_lens, occludes};
use super::shots::{
    BallState, FIELDER_CAM_MIN_HEIGHT, RigMotion, Shot, ShotCues, ShotInputs, is_grounder,
    pick_shot,
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

/// The broadcast camera's state: the shot on screen, and the lens that
/// tracks *within* it and moves *between* shots ([`RigMotion`]).
#[derive(Resource)]
pub struct BroadcastRig {
    motion: RigMotion,
    shot: Option<Shot>,
    /// The base the live ball was last thrown at, cleared once the shot
    /// returns to the duel.
    thrown_base: Option<usize>,
    /// This play's ball was hit on the ground, latched so a hop doesn't
    /// flip the shot; cleared with `thrown_base`.
    grounder: bool,
}

impl Default for BroadcastRig {
    fn default() -> Self {
        Self {
            motion: RigMotion::new(BROADCAST_EYE, BROADCAST_HOME_TARGET, BROADCAST_FOV),
            shot: None,
            thrown_base: None,
            grounder: false,
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

#[allow(clippy::too_many_arguments)]
pub(super) fn broadcast_camera(
    time: Res<Time>,
    play: Res<Play>,
    field: Res<FieldSpec>,
    view: Res<DuelView>,
    kick: Res<CameraKick>,
    active: Res<ActivePlay>,
    wind: Res<rules::Wind>,
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
    let ball_state = ball.map(|(tf, vel)| BallState {
        pos: tf.translation,
        vel: vel.linvel,
        spin: vel.angvel,
    });
    // Latch a ground ball once the plate hold is over and it is clear of
    // the bat: a hop up off the infield stays a grounder.
    if play.phase == Phase::InPlay
        && !play.is_home_run()
        && play.since_contact(now) >= BALL_FOLLOW_DELAY
        && ball_state.is_some_and(|b| is_grounder(b, wind.0))
    {
        rig.grounder = true;
    }
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
        walk_off: play.is_walk_off(),
        pitch_gloved: play.pitch_gloved(),
        view: *view,
        walkup: play.walkup_active(),
        strikeout: play.result_beat() == Some(ResultBeat::Strikeout),
        thrown_base: rig.thrown_base,
        grounder: rig.grounder,
        descending_to,
        duel_throw_base: play.duel_throw().map(|t| t.target),
    };
    let shot = pick_shot(&cues, rig.shot);

    let start = *trot_start.get_or_insert(now);
    let inputs = ShotInputs {
        field: &field,
        aspect,
        ball: ball_state,
        fielder: match shot {
            Shot::FielderCam(f) => rigs.get(f).ok().map(|tf| tf.translation),
            Shot::GroundBall => active
                .chaser()
                .and_then(|f| rigs.get(f).ok())
                .map(|tf| tf.translation),
            _ => None,
        },
        wind: wind.0,
        home_run: play.is_home_run(),
        trot_secs: now - start,
    };
    let want = shot.spec(&inputs);

    // The trot seed lives only while the trot shot does.
    if shot != Shot::TrotOrbit {
        *trot_start = None;
    }
    if matches!(shot, Shot::Duel(_)) {
        rig.thrown_base = None;
        rig.grounder = false;
    }

    if rig.shot != Some(shot) {
        // A new subject: the shot says how to get there (a cut, today).
        let kind = shot.transition_from(rig.shot);
        rig.shot = Some(shot);
        rig.motion.start(kind, want, time.delta_secs());
    } else {
        rig.motion.step(want, time.delta_secs());
    }

    if let Ok((mut cam, mut projection)) = cam_q.single_mut() {
        let m = rig.motion;
        *cam = Transform::from_translation(m.eye + kick.0).looking_at(m.target, Vec3::Y);
        if let Projection::Perspective(persp) = projection.as_mut() {
            persp.fov = m.fov;
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
