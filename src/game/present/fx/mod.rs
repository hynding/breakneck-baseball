//! Impact particles, trails, and fireworks. Purely cosmetic: nothing here
//! may touch the scoreboard, the bases, or the rules — and nothing here may
//! touch `Time<Virtual>` either: hit-stop/slow-mo live in `game::juice`,
//! the single owner of `relative_speed` (a second writer here once raced it
//! and could cancel Perfect's slow-mo tail — TODO 61).

use bevy::prelude::*;

use crate::game::GameState;
use crate::game::settings::PitchTrailStyle;

mod ball_read;
mod particles;
mod rain;
mod throw;
mod trail;

pub use particles::FireworkSpark;
pub use rain::RainDrop;
pub use throw::{DuelThrowBall, throw_arc};
pub use trail::TrailMote;

use particles::{
    Fireworks, bounce_dust, build_fx_assets, contact_burst, home_run_fireworks, spawn_ball_halo,
    spawn_landing_ring, tick_particles, update_ball_halo, update_landing_ring, wall_bang_burst,
};
use trail::{pitch_trail, tick_trail};

/// Shared meshes/materials for effects, built once per game from the theme.
#[derive(Resource)]
struct FxAssets {
    spark_mesh: Handle<Mesh>,
    dust_mesh: Handle<Mesh>,
    /// A fatter mote for fireworks, so the show reads from the outfield.
    firework_mesh: Handle<Mesh>,
    spark: Handle<StandardMaterial>,
    dust: Handle<StandardMaterial>,
    /// A small bright palette the fireworks pick from, burst by burst.
    firework: Vec<Handle<StandardMaterial>>,
}

/// Meshes plus the fade ladder for the chosen colour — built per game start
/// from [`Settings`] (the settings screen only exists on the menu, so the
/// choice can't change mid-game).
#[derive(Resource)]
struct TrailAssets {
    style: PitchTrailStyle,
    mesh: Handle<Mesh>,
    /// `fade[k]` = the chosen colour at alpha rung `k` (0 = brightest).
    fade: Vec<Handle<StandardMaterial>>,
}

pub struct FxPlugin;

impl Plugin for FxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Fireworks>()
            .add_systems(
                crate::game::game_start(),
                (
                    build_fx_assets,
                    ball_read::spawn_ball_readability,
                    spawn_landing_ring,
                    spawn_ball_halo,
                    rain::spawn_rain,
                ),
            )
            .add_systems(
                Update,
                (
                    contact_burst,
                    wall_bang_burst,
                    home_run_fireworks,
                    bounce_dust,
                    update_landing_ring,
                    update_ball_halo,
                    ball_read::spawn_flight_ghosts,
                    ball_read::fade_flight_ghosts,
                    tick_particles,
                    pitch_trail,
                    tick_trail,
                    throw::stage_duel_throw,
                    rain::fall_rain,
                )
                    .run_if(in_state(GameState::Playing)),
            )
            // After Rapier writes the ball's pose for this frame and before
            // it propagates, so the proxy and shadow sit on the ball being
            // drawn rather than one physics step behind it.
            .add_systems(
                PostUpdate,
                (ball_read::place_ball_shadow, ball_read::place_ball_proxy)
                    .after(bevy_rapier3d::plugin::PhysicsSet::Writeback)
                    .before(bevy::transform::TransformSystems::Propagate)
                    .run_if(in_state(GameState::Playing)),
            );
    }
}
