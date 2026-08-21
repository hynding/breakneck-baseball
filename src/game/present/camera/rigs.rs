//! Camera rigs: the broadcast and orbit systems that actually move the
//! `Camera3d`, plus the occlusion pass that hides whoever is standing in
//! the broadcast lens's way. Reads the pure math in [`super::framing`] and
//! the mode/duel-view state owned by [`super`].

use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy_rapier3d::prelude::Velocity;

use crate::game::ball::{BALL_DRAG_FACTOR, Baseball, HitEvent, MAGNUS_FACTOR, WallBangEvent};
use crate::game::flow::{Phase, Play};
use crate::game::player::{CatcherRole, PlateUmpire};
use crate::game::rules;
use crate::game::variant::FieldSpec;

use super::framing::{
    OCCLUSION_NEAR, OCCLUSION_RADIUS, TROT_ORBIT_RATE, duel_framing_wanted, occludes,
    trot_orbit_eye,
};
use super::{
    BALL_FOLLOW_DELAY, BROADCAST_EYE, BROADCAST_FOV, BROADCAST_HOME_TARGET, CameraMode, DUEL_FOV,
    DUEL_REFERENCE_ASPECT, DuelView, aspect_safe_duel_vfov,
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
    time: Res<Time>,
    view: Res<DuelView>,
    field: Res<FieldSpec>,
    play: Res<Play>,
    mode: Res<CameraMode>,
    mut subjects: Query<
        (&Transform, &mut Visibility, Has<CatcherRole>),
        Or<(With<CatcherRole>, With<PlateUmpire>)>,
    >,
) {
    // The same predicate the broadcast rig uses to *hold* the duel framing
    // gates the hiding: while the lens is parked at the plate — the duel
    // itself, the post-contact hold, and a gloved pitch's result pause —
    // nothing may pop into it. Gating on the duel phases alone let the
    // plate umpire stand up out of his crouch straight into the parked
    // catcher-POV lens during the result pause (playtest 2026-08-20).
    let framing_held =
        *mode == CameraMode::Broadcast && duel_framing_wanted(&play, time.elapsed_secs());
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
        let blocking = pov_at_plate
            || (framing_held
                && occludes(
                    eye,
                    target,
                    transform.translation,
                    OCCLUSION_NEAR,
                    OCCLUSION_RADIUS,
                ));
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

/// Smoothed eye + look-at + FOV for the broadcast camera. All three lerp
/// toward what the current play phase wants (tight duel framing for the
/// pitch, wide ball-following framing in play), so zooms glide instead of
/// cutting.
#[derive(Resource)]
pub(super) struct BroadcastRig {
    eye: Vec3,
    target: Vec3,
    fov: f32,
}

impl Default for BroadcastRig {
    fn default() -> Self {
        Self {
            eye: BROADCAST_EYE,
            target: BROADCAST_HOME_TARGET,
            fov: BROADCAST_FOV,
        }
    }
}

/// Impulse added to the broadcast eye on contact; decays on real time so the
/// kick rides through the hit-stop.
#[derive(Resource, Default)]
pub(super) struct CameraKick(Vec3);

/// The live ball as the broadcast camera reads it.
type BallQuery<'w, 's> =
    Query<'w, 's, (&'static Transform, &'static Velocity), (With<Baseball>, Without<Camera3d>)>;

pub(super) fn kick_on_hit(mut hits: EventReader<HitEvent>, mut kick: ResMut<CameraKick>) {
    for _ in hits.read() {
        kick.0 += Vec3::new(0.0, 0.18, -0.35);
    }
}

/// A smaller thump when the ball bangs off the outfield wall.
pub(super) fn kick_on_wall_bang(
    mut bangs: EventReader<WallBangEvent>,
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

#[allow(clippy::too_many_arguments)]
pub(super) fn broadcast_camera(
    time: Res<Time>,
    play: Res<Play>,
    field: Res<FieldSpec>,
    view: Res<DuelView>,
    kick: Res<CameraKick>,
    ball_q: BallQuery,
    mut rig: ResMut<BroadcastRig>,
    mut cam_q: Query<(&mut Transform, &mut Projection), With<Camera3d>>,
) {
    // The camera's actual aspect ratio (width / height), read before the
    // framing decision so the duel FOV can correct for it — see
    // `aspect_safe_duel_vfov`. Falls back to the reference aspect (a no-op
    // correction) before the camera exists.
    let aspect = match cam_q.get_single() {
        Ok((_, Projection::Perspective(persp))) => persp.aspect_ratio,
        _ => DUEL_REFERENCE_ASPECT,
    };

    // Pick the framing the current phase wants.
    let (desired_eye, desired_target, desired_fov) = match (play.phase, ball_q.get_single()) {
        // Fresh contact: hold the plate framing for a beat — the swing, the
        // crack, the batter breaking from the box — before chasing the ball.
        (Phase::InPlay, Ok(_)) if play.since_contact(time.elapsed_secs()) < BALL_FOLLOW_DELAY => (
            field.duel_eye,
            field.duel_target,
            aspect_safe_duel_vfov(DUEL_FOV, aspect),
        ),
        // A live, uncalled play: cut to where the ball is coming down. The
        // eye stations itself between home and the predicted landing spot —
        // a medium shot of the drop zone, so the chasing fielder and the
        // play about to happen are what's framed, not just the ball.
        (Phase::InPlay, Ok((ball, vel))) if !play.is_resolved() => {
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
            // Keep the ball's flight in the corner of the eye.
            let target = focus.lerp(ball.translation, 0.3);

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
        // Called plays (home-run trots): sweep with the ball — the eye
        // slides laterally and pulls up and back as it travels deep.
        (Phase::InPlay, Ok((ball, _))) => {
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
        // Result pause of a home run: orbit the diamond while the batter
        // trots the bases — a sweeping victory-lap shot that lerps in from the
        // ball-follow and back out to the duel framing at phase end.
        (Phase::Result, _) if play.is_home_run() => {
            let focus = Vec3::new(field.broadcast_target.x, 1.4, field.broadcast_target.z);
            let eye = trot_orbit_eye(focus, time.elapsed_secs() * TROT_ORBIT_RATE);
            (eye, focus, BROADCAST_FOV)
        }
        // Result pause of a gloved pitch (called strike/ball, strikeout
        // into the mitt): stay in the at-bat view — the umpire's call
        // doesn't deserve a zoom-out. Everything the mitt missed (hits,
        // dirt balls, dropped thirds, HBP) falls through to the wide shot.
        (Phase::Result, _) if play.pitch_gloved() => view.framing(&field, aspect),
        // Result pause: settle on the wide home framing.
        (Phase::Result, _) => (field.broadcast_eye, field.broadcast_target, BROADCAST_FOV),
        // The duel: whichever at-bat view the player has cycled to with V.
        _ => view.framing(&field, aspect),
    };

    // Critically-damped-ish smoothing so framing changes glide, never cut.
    let follow = 1.0 - (-5.0 * time.delta_secs()).exp();
    rig.eye = rig.eye.lerp(desired_eye, follow);
    rig.target = rig.target.lerp(desired_target, follow);
    rig.fov = rig.fov + (desired_fov - rig.fov) * follow;

    if let Ok((mut cam, mut projection)) = cam_q.get_single_mut() {
        *cam = Transform::from_translation(rig.eye + kick.0).looking_at(rig.target, Vec3::Y);
        if let Projection::Perspective(persp) = projection.as_mut() {
            persp.fov = rig.fov;
        }
    }
}

// ── Orbit camera (free look) ──────────────────────────────────────────────────

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

    orbit.yaw += yaw_delta;
    orbit.pitch = (orbit.pitch + pitch_delta).clamp(0.1, std::f32::consts::FRAC_PI_2 - 0.05);

    if keyboard.just_pressed(KeyCode::KeyR) {
        *orbit = OrbitState::default();
    }

    let transform = orbit_transform(&orbit);
    for mut cam_transform in &mut camera_query {
        *cam_transform = transform;
    }
}

pub(super) fn zoom_camera(
    mut scroll: EventReader<MouseWheel>,
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
    if keyboard.pressed(KeyCode::KeyQ) {
        zoom_delta -= 15.0 * dt;
    }
    if keyboard.pressed(KeyCode::KeyE) {
        zoom_delta += 15.0 * dt;
    }

    orbit.distance = (orbit.distance + zoom_delta).clamp(10.0, 200.0);

    let transform = orbit_transform(&orbit);
    for mut cam_transform in &mut camera_query {
        *cam_transform = transform;
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
