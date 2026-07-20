//! Game camera.
//!
//! Two modes, toggled with **C** (or the controller's Select/Back button):
//!
//! - **Broadcast** (default): a high angle behind home plate that frames the
//!   whole diamond for the pitch, then gently follows the ball while it is live.
//! - **Orbit**: the free stadium camera (WASD / arrows to orbit, Q/E or wheel to
//!   zoom, R to reset) for looking around.

use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;

use crate::game::ball::{Baseball, HitEvent};
use crate::game::flow::{Phase, Play};
use crate::game::variant::FieldSpec;
use crate::game::GameState;

// ── Mode ──────────────────────────────────────────────────────────────────────

#[derive(Resource, Default, Clone, Copy, PartialEq, Eq)]
pub enum CameraMode {
    #[default]
    Broadcast,
    Orbit,
}

fn is_broadcast(mode: Res<CameraMode>) -> bool {
    *mode == CameraMode::Broadcast
}
fn is_orbit(mode: Res<CameraMode>) -> bool {
    *mode == CameraMode::Orbit
}

/// Fallback broadcast framing used before a field is chosen (initial camera
/// spawn); once a game is running the framing comes from the [`FieldSpec`].
const BROADCAST_HOME_TARGET: Vec3 = Vec3::new(0.0, 1.2, 9.0);
const BROADCAST_EYE: Vec3 = Vec3::new(0.0, 13.0, -21.0);

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

/// Smoothed eye + look-at for the broadcast camera. Both lerp toward the
/// framing the current play phase wants (tight duel framing for the pitch,
/// wide ball-following framing in play), so zooms glide instead of cutting.
#[derive(Resource)]
struct BroadcastRig {
    eye: Vec3,
    target: Vec3,
}

impl Default for BroadcastRig {
    fn default() -> Self {
        Self {
            eye: BROADCAST_EYE,
            target: BROADCAST_HOME_TARGET,
        }
    }
}

/// Impulse added to the broadcast eye on contact; decays on real time so the
/// kick rides through the hit-stop.
#[derive(Resource, Default)]
struct CameraKick(Vec3);

fn kick_on_hit(mut hits: EventReader<HitEvent>, mut kick: ResMut<CameraKick>) {
    for _ in hits.read() {
        kick.0 += Vec3::new(0.0, 0.18, -0.35);
    }
}

fn decay_kick(real: Res<Time<Real>>, mut kick: ResMut<CameraKick>) {
    kick.0 *= (-14.0 * real.delta_secs()).exp();
}

// ── Plugin ────────────────────────────────────────────────────────────────────

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OrbitState>()
            .init_resource::<CameraMode>()
            .init_resource::<BroadcastRig>()
            .init_resource::<CameraKick>()
            .add_systems(Startup, spawn_camera)
            .add_systems(
                Update,
                (toggle_camera_mode, kick_on_hit, decay_kick).run_if(in_state(GameState::Playing)),
            )
            .add_systems(
                Update,
                broadcast_camera
                    .run_if(in_state(GameState::Playing))
                    .run_if(is_broadcast),
            )
            .add_systems(
                Update,
                (orbit_camera, zoom_camera)
                    .run_if(in_state(GameState::Playing))
                    .run_if(is_orbit),
            );
    }
}

// ── Startup ───────────────────────────────────────────────────────────────────

fn spawn_camera(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_translation(BROADCAST_EYE).looking_at(BROADCAST_HOME_TARGET, Vec3::Y),
        Projection::Perspective(PerspectiveProjection {
            fov: std::f32::consts::FRAC_PI_3,
            ..default()
        }),
    ));
}

// ── Mode toggle ───────────────────────────────────────────────────────────────

fn toggle_camera_mode(
    keyboard: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    mut mode: ResMut<CameraMode>,
) {
    let toggled = keyboard.just_pressed(KeyCode::KeyC)
        || pads.iter().any(|p| p.just_pressed(GamepadButton::Select));
    if toggled {
        *mode = match *mode {
            CameraMode::Broadcast => CameraMode::Orbit,
            CameraMode::Orbit => CameraMode::Broadcast,
        };
    }
}

// ── Broadcast camera ──────────────────────────────────────────────────────────

fn broadcast_camera(
    time: Res<Time>,
    play: Res<Play>,
    field: Res<FieldSpec>,
    kick: Res<CameraKick>,
    ball_q: Query<&Transform, (With<Baseball>, Without<Camera3d>)>,
    mut rig: ResMut<BroadcastRig>,
    mut cam_q: Query<&mut Transform, With<Camera3d>>,
) {
    // Pick the framing the current phase wants.
    let (desired_eye, desired_target) = match (play.phase, ball_q.get_single()) {
        // Ball is live: the camera chases the ball — the eye slides laterally
        // with it and pulls up and back as the ball travels deep, so the
        // whole play (ball, chasing fielder, runners) stays in frame.
        (Phase::InPlay, Ok(ball)) => {
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
            (eye, target)
        }
        // Result pause: settle on the wide home framing.
        (Phase::Result, _) => (field.broadcast_eye, field.broadcast_target),
        // The duel: zoom in tight on batter vs pitcher.
        _ => (field.duel_eye, field.duel_target),
    };

    // Critically-damped-ish smoothing so framing changes glide, never cut.
    let follow = 1.0 - (-5.0 * time.delta_secs()).exp();
    rig.eye = rig.eye.lerp(desired_eye, follow);
    rig.target = rig.target.lerp(desired_target, follow);

    if let Ok(mut cam) = cam_q.get_single_mut() {
        *cam = Transform::from_translation(rig.eye + kick.0).looking_at(rig.target, Vec3::Y);
    }
}

// ── Orbit camera (free look) ──────────────────────────────────────────────────

fn orbit_camera(
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

fn zoom_camera(
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
