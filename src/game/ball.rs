//! Baseball physics — the ball entity and all systems that act on it.
//!
//! The ball is a dynamic Rapier rigid body with realistic restitution and
//! friction coefficients, and an aerodynamic drag force applied every physics
//! tick to simulate air resistance.
//!
//! **Key constants** (official MLB baseball):
//! - Diameter: 2.9 – 3.0 in  ≈ **0.074 m** (radius ≈ 0.037 m)
//! - Mass: 5 – 5.25 oz       ≈ **0.148 kg**
//! - Coefficient of restitution: ≈ **0.55**

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::game::variant::FieldSpec;
use crate::game::{GameState, GameplayEntity};

/// Official ball radius in metres.
pub const BALL_RADIUS: f32 = 0.037;
/// Official ball mass in kilograms.
pub const BALL_MASS: f32 = 0.148;
/// Aerodynamic drag coefficient × reference area (simplified).
pub const BALL_DRAG_FACTOR: f32 = 0.0003;

/// Collision group for the ball; [`PLAYER_GROUP`] capsules are excluded from
/// its filter. Outcomes are analytic, so a ball–player contact adds nothing —
/// and a pitched ball glancing off the batter's collider would turn a strike
/// into a wild deflection.
pub const BALL_GROUP: Group = Group::GROUP_1;
/// Collision group for player capsules.
pub const PLAYER_GROUP: Group = Group::GROUP_2;

// ── Marker components ─────────────────────────────────────────────────────────
/// Marks the active baseball in play.
#[derive(Component)]
pub struct Baseball;

/// Indicates the ball is currently in flight (has been pitched or batted).
#[derive(Component)]
pub struct InFlight;

// ── Events ────────────────────────────────────────────────────────────────────
/// Fired when a pitch is thrown. Carries the initial world-space velocity.
#[derive(Event)]
pub struct PitchEvent {
    /// Velocity vector in world space (m/s).
    pub velocity: Vec3,
}

/// Fired when the ball is hit by the batter.
#[derive(Event)]
pub struct HitEvent {
    /// Velocity imparted to the ball (m/s).
    pub velocity: Vec3,
}

// ── Plugin ────────────────────────────────────────────────────────────────────
pub struct BallPlugin;

impl Plugin for BallPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<PitchEvent>()
            .add_event::<HitEvent>()
            .add_systems(OnEnter(GameState::Playing), spawn_ball)
            .add_systems(
                Update,
                (
                    apply_pitch,
                    apply_hit,
                    apply_drag,
                    reset_ball_if_out_of_bounds,
                )
                    .run_if(in_state(GameState::Playing)),
            );
    }
}

// ── Startup: spawn the ball at rest on the pitcher's mound ───────────────────
fn spawn_ball(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    field: Res<FieldSpec>,
) {
    commands.spawn((
        Baseball,
        GameplayEntity,
        Mesh3d(meshes.add(Sphere::new(BALL_RADIUS))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.6,
            ..default()
        })),
        Transform::from_xyz(0.0, BALL_RADIUS + 0.25, field.pitch_distance),
        RigidBody::Dynamic,
        Collider::ball(BALL_RADIUS),
        Velocity::zero(),
        // Accurate coefficient of restitution for a baseball.
        Restitution {
            coefficient: 0.55,
            combine_rule: CoefficientCombineRule::Min,
        },
        Friction {
            coefficient: 0.5,
            combine_rule: CoefficientCombineRule::Average,
        },
        ColliderMassProperties::Mass(BALL_MASS),
        // Continuous collision detection prevents tunnelling at high speeds.
        Ccd::enabled(),
        // Allow rotation (spin) but dampen it slightly.
        Damping {
            linear_damping: 0.0,
            angular_damping: 0.1,
        },
        CollisionGroups::new(BALL_GROUP, !PLAYER_GROUP),
        ActiveEvents::COLLISION_EVENTS,
    ));
}

// ── Systems ───────────────────────────────────────────────────────────────────

/// Responds to a [`PitchEvent`] by giving the ball the specified velocity and
/// marking it as in-flight.
fn apply_pitch(
    mut events: EventReader<PitchEvent>,
    mut query: Query<(Entity, &mut Velocity), With<Baseball>>,
    mut commands: Commands,
) {
    for event in events.read() {
        for (entity, mut vel) in &mut query {
            vel.linvel = event.velocity;
            // Backspin (about the axis perpendicular to a -Z pitch). Spin
            // about the travel axis would grind against the mound while still
            // in contact and kick the release sideways.
            vel.angvel = Vec3::new(-event.velocity.length() * 0.5, 0.0, 0.0);
            commands.entity(entity).insert(InFlight);
        }
    }
}

/// Responds to a [`HitEvent`] by setting the ball's velocity and keeping the
/// in-flight marker active.
fn apply_hit(
    mut events: EventReader<HitEvent>,
    mut query: Query<(Entity, &mut Velocity), With<Baseball>>,
    mut commands: Commands,
) {
    for event in events.read() {
        for (entity, mut vel) in &mut query {
            vel.linvel = event.velocity;
            vel.angvel = Vec3::new(0.0, event.velocity.length() * 0.3, 0.0);
            commands.entity(entity).insert(InFlight);
        }
    }
}

/// Applies a simplified quadratic aerodynamic drag every physics frame.
///
/// `F_drag = -drag_factor × |v|² × v̂`
fn apply_drag(mut query: Query<&mut Velocity, (With<Baseball>, With<InFlight>)>, time: Res<Time>) {
    let dt = time.delta_secs();
    for mut vel in &mut query {
        let speed = vel.linvel.length();
        if speed > 0.0 {
            let drag = -BALL_DRAG_FACTOR * speed * speed * vel.linvel / speed;
            vel.linvel += drag * dt;
        }
    }
}

/// Resets the ball to the pitcher's mound if it falls below the world or flies
/// beyond the field's playable radius.
fn reset_ball_if_out_of_bounds(
    mut query: Query<(&mut Transform, &mut Velocity), With<Baseball>>,
    mut commands: Commands,
    entity_query: Query<Entity, (With<Baseball>, With<InFlight>)>,
    field: Res<FieldSpec>,
) {
    for (mut transform, mut vel) in &mut query {
        let pos = transform.translation;
        let out = pos.y < -10.0 || Vec2::new(pos.x, pos.z).length() > field.bounds;
        if out {
            transform.translation = Vec3::new(0.0, BALL_RADIUS + 0.25, field.pitch_distance);
            vel.linvel = Vec3::ZERO;
            vel.angvel = Vec3::ZERO;

            for entity in &entity_query {
                commands.entity(entity).remove::<InFlight>();
            }
        }
    }
}
