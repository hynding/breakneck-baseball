//! Game feel — hit-stop and impact particles. Purely cosmetic: nothing here
//! may touch the scoreboard, the bases, or the rules.

use bevy::prelude::*;

use crate::game::GameState;
use crate::game::ball::HitEvent;
use crate::game::settings::PitchTrailStyle;

mod particles;
mod trail;

pub use particles::FireworkSpark;
pub use trail::TrailMote;

use particles::{
    Fireworks, bounce_dust, build_fx_assets, contact_burst, home_run_fireworks, spawn_ball_halo,
    spawn_landing_ring, tick_particles, update_ball_halo, update_landing_ring, wall_bang_burst,
};
use trail::{pitch_trail, tick_trail};

/// How hard time slows on contact, and for how long (real seconds).
const HIT_STOP_SCALE: f32 = 0.05;
const HIT_STOP_SECS: f32 = 0.06;

#[derive(Resource, Default)]
struct HitStop(Option<Timer>);

/// Freezes the world for a beat when bat meets ball.
fn start_hit_stop(
    mut hits: EventReader<HitEvent>,
    mut virt: ResMut<Time<Virtual>>,
    mut stop: ResMut<HitStop>,
    base: Res<crate::game::juice::BaseSpeed>,
) {
    if hits.read().next().is_some() {
        virt.set_relative_speed(HIT_STOP_SCALE * base.0);
        stop.0 = Some(Timer::from_seconds(HIT_STOP_SECS, TimerMode::Once));
    }
}

/// Restores full speed once the (real-time) freeze window elapses.
fn end_hit_stop(
    real: Res<Time<Real>>,
    mut virt: ResMut<Time<Virtual>>,
    mut stop: ResMut<HitStop>,
    base: Res<crate::game::juice::BaseSpeed>,
) {
    let finished = stop
        .0
        .as_mut()
        .is_some_and(|t| t.tick(real.delta()).finished());
    if finished {
        virt.set_relative_speed(base.0);
        stop.0 = None;
    }
}

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
        app.init_resource::<HitStop>()
            .init_resource::<Fireworks>()
            .add_systems(
                crate::game::game_start(),
                (build_fx_assets, spawn_landing_ring, spawn_ball_halo),
            )
            .add_systems(
                Update,
                (
                    start_hit_stop,
                    end_hit_stop,
                    contact_burst,
                    wall_bang_burst,
                    home_run_fireworks,
                    bounce_dust,
                    update_landing_ring,
                    update_ball_halo,
                    tick_particles,
                    pitch_trail,
                    tick_trail,
                )
                    .run_if(in_state(GameState::Playing)),
            );
    }
}
