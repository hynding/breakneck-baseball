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

use crate::game::theme::Theme;
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

/// Shared mesh/material for trail ghosts, built once per game from the theme.
#[derive(Resource)]
struct TrailAssets {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

/// One fading ghost of the ball's path; shrinks to nothing and despawns.
#[derive(Component)]
struct TrailGhost(Timer);

/// Ghost lifetime (seconds) and spawn cadence while the ball flies.
const TRAIL_LIFETIME: f32 = 0.35;
const TRAIL_INTERVAL: f32 = 0.025;
/// Below this speed (m/s) the ball is rolling, not flying — no trail.
const TRAIL_MIN_SPEED: f32 = 8.0;

/// Query alias for "the flying ball" (keeps clippy's type-complexity happy).
type FlyingBall<'w, 's> =
    Query<'w, 's, (&'static Transform, &'static Velocity), (With<Baseball>, With<InFlight>)>;

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
                    spawn_trail,
                    fade_trail,
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
    theme: Res<Theme>,
) {
    // The rendered ball is deliberately larger than the physics ball (a real
    // baseball is a 7 cm dot at broadcast distance) and self-lit so it reads
    // against grass, sky, and asphalt alike. Collider stays regulation.
    let visual_radius = BALL_RADIUS * theme.ball.visual_scale;

    commands.insert_resource(TrailAssets {
        mesh: meshes.add(Sphere::new(visual_radius * 0.8)),
        material: materials.add(StandardMaterial {
            base_color: theme.ball.trail,
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        }),
    });

    commands.spawn((
        Baseball,
        GameplayEntity,
        Mesh3d(meshes.add(Sphere::new(visual_radius))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: theme.ball.color,
            emissive: theme.ball.emissive,
            perceptual_roughness: 0.4,
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
fn apply_drag(
    mut query: Query<(&Transform, &mut Velocity), (With<Baseball>, With<InFlight>)>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();
    for (transform, mut vel) in &mut query {
        let speed = vel.linvel.length();
        if speed > 0.0 {
            let drag = -BALL_DRAG_FACTOR * speed * speed * vel.linvel / speed;
            vel.linvel += drag * dt;
        }
        // Rolling resistance: once the ball is down and barely bouncing,
        // bleed horizontal speed so grounders die instead of rolling forever.
        if transform.translation.y < BALL_RADIUS * 3.0 && vel.linvel.y.abs() < 1.0 {
            let f = (1.0 - 1.6 * dt).max(0.0);
            vel.linvel.x *= f;
            vel.linvel.z *= f;
        }
    }
}

/// Drops a fading ghost sphere behind the flying ball every
/// [`TRAIL_INTERVAL`] seconds so its path reads at a glance.
fn spawn_trail(
    time: Res<Time>,
    mut since_last: Local<f32>,
    assets: Option<Res<TrailAssets>>,
    ball_q: FlyingBall,
    mut commands: Commands,
) {
    let Some(assets) = assets else {
        return;
    };
    let Ok((transform, vel)) = ball_q.get_single() else {
        return;
    };
    if vel.linvel.length() < TRAIL_MIN_SPEED {
        return;
    }

    *since_last += time.delta_secs();
    if *since_last < TRAIL_INTERVAL {
        return;
    }
    *since_last = 0.0;

    commands.spawn((
        TrailGhost(Timer::from_seconds(TRAIL_LIFETIME, TimerMode::Once)),
        GameplayEntity,
        Mesh3d(assets.mesh.clone()),
        MeshMaterial3d(assets.material.clone()),
        Transform::from_translation(transform.translation),
    ));
}

/// Shrinks each ghost to nothing over its lifetime, then despawns it.
fn fade_trail(
    time: Res<Time>,
    mut ghosts: Query<(Entity, &mut TrailGhost, &mut Transform)>,
    mut commands: Commands,
) {
    for (entity, mut ghost, mut transform) in &mut ghosts {
        if ghost.0.tick(time.delta()).finished() {
            commands.entity(entity).despawn();
        } else {
            transform.scale = Vec3::splat(1.0 - ghost.0.fraction());
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
