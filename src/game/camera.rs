//! Game camera — a stadium-style orbital camera that can be repositioned
//! using keyboard/mouse input.
//!
//! Controls (default):
//! - **Scroll wheel / Q/E** — zoom in / out
//! - **WASD / arrow keys** — orbit horizontally and vertically
//! - **R** — reset to default position

use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;

use crate::game::GameState;

// ── Camera state resource ─────────────────────────────────────────────────────
/// Stores the spherical-coordinate orbit state.
#[derive(Resource)]
pub struct OrbitState {
    /// Horizontal angle around the Y axis (radians).
    pub yaw: f32,
    /// Vertical angle above the XZ plane (radians).
    pub pitch: f32,
    /// Distance from the look-at target.
    pub distance: f32,
    /// Point the camera orbits around.
    pub target: Vec3,
}

impl Default for OrbitState {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: 0.6,                        // ≈ 35° above ground
            distance: 60.0,                    // metres from home plate
            target: Vec3::new(0.0, 0.0, 30.0), // look toward centre field
        }
    }
}

// ── Plugin ────────────────────────────────────────────────────────────────────
pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OrbitState>()
            .add_systems(Startup, spawn_camera)
            .add_systems(
                Update,
                (orbit_camera, zoom_camera).run_if(in_state(GameState::Playing)),
            );
    }
}

// ── Startup ───────────────────────────────────────────────────────────────────
fn spawn_camera(mut commands: Commands, orbit: Res<OrbitState>) {
    let transform = orbit_transform(&orbit);

    commands.spawn((
        Camera3d::default(),
        transform,
        // Projection can be tuned for a cinematic feel.
        Projection::Perspective(PerspectiveProjection {
            fov: std::f32::consts::FRAC_PI_3, // 60°
            ..default()
        }),
    ));
}

// ── Systems ───────────────────────────────────────────────────────────────────
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

    // Reset
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

// ── Helpers ───────────────────────────────────────────────────────────────────
fn orbit_transform(orbit: &OrbitState) -> Transform {
    let offset = Vec3::new(
        orbit.distance * orbit.yaw.sin() * orbit.pitch.cos(),
        orbit.distance * orbit.pitch.sin(),
        orbit.distance * orbit.yaw.cos() * orbit.pitch.cos(),
    );
    Transform::from_translation(orbit.target + offset).looking_at(orbit.target, Vec3::Y)
}
