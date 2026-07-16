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

use crate::game::ball::Baseball;
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

/// Smoothed look-at target for the broadcast camera (lerps toward the ball).
#[derive(Resource)]
struct BroadcastTarget(Vec3);

impl Default for BroadcastTarget {
    fn default() -> Self {
        Self(BROADCAST_HOME_TARGET)
    }
}

// ── Plugin ────────────────────────────────────────────────────────────────────

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OrbitState>()
            .init_resource::<CameraMode>()
            .init_resource::<BroadcastTarget>()
            .add_systems(Startup, spawn_camera)
            .add_systems(
                Update,
                toggle_camera_mode.run_if(in_state(GameState::Playing)),
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
    ball_q: Query<&Transform, (With<Baseball>, Without<Camera3d>)>,
    mut target: ResMut<BroadcastTarget>,
    mut cam_q: Query<&mut Transform, With<Camera3d>>,
) {
    // Follow the ball while it is live; otherwise settle back onto home.
    let desired = match (play.phase, ball_q.get_single()) {
        (Phase::InPlay, Ok(ball)) => {
            // Ease toward the ball but keep it a touch above ground for framing.
            Vec3::new(
                ball.translation.x,
                ball.translation.y.max(1.0),
                ball.translation.z,
            )
        }
        _ => field.broadcast_target,
    };

    // Critically-damped-ish smoothing so the camera glides rather than snaps.
    let follow = 1.0 - (-6.0 * time.delta_secs()).exp();
    target.0 = target.0.lerp(desired, follow);

    // Pull the eye back a little for deep balls so home runs stay in frame.
    let extra = ((target.0.z - field.broadcast_target.z) * 0.12).clamp(0.0, 14.0);
    let eye = field.broadcast_eye + Vec3::new(0.0, extra * 0.5, -extra);

    if let Ok(mut cam) = cam_q.get_single_mut() {
        *cam = Transform::from_translation(eye).looking_at(target.0, Vec3::Y);
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
