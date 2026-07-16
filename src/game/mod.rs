//! Top-level game module.
//!
//! `GamePlugin` registers every sub-plugin in dependency order and exposes the
//! shared [`GameState`] state machine to all systems.

pub mod ball;
pub mod camera;
pub mod field;
pub mod player;
pub mod ui;

use bevy::prelude::*;

use ball::BallPlugin;
use camera::CameraPlugin;
use field::FieldPlugin;
use player::PlayerPlugin;
use ui::UiPlugin;

/// Global game-state machine.
///
/// Systems that should only run while the game is active use
/// `.run_if(in_state(GameState::Playing))`.
#[allow(dead_code)]
#[derive(States, Debug, Clone, Eq, PartialEq, Hash, Default)]
pub enum GameState {
    /// The game has not started yet (title screen / menus).
    #[default]
    MainMenu,
    /// Active gameplay.
    Playing,
    /// The game is paused.
    Paused,
    /// Inning / game over screen.
    GameOver,
}

/// Runtime counters shared across systems.
#[derive(Resource, Default, Debug)]
pub struct ScoreBoard {
    /// Runs scored by the home team.
    pub home_runs: u32,
    /// Runs scored by the away team.
    pub away_runs: u32,
    /// Current inning (1-indexed).
    pub inning: u32,
    /// `true` = top half, `false` = bottom half.
    pub top_of_inning: bool,
    /// Balls in the current at-bat.
    pub balls: u32,
    /// Strikes in the current at-bat.
    pub strikes: u32,
    /// Outs in the current half-inning.
    pub outs: u32,
}

/// Aggregate plugin that wires every sub-system into the app.
pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app
            // State machine
            .init_state::<GameState>()
            // Shared resources
            .insert_resource(ScoreBoard {
                inning: 1,
                top_of_inning: true,
                ..default()
            })
            // Sub-plugins
            .add_plugins((
                FieldPlugin,
                BallPlugin,
                PlayerPlugin,
                CameraPlugin,
                UiPlugin,
            ))
            // Start playing immediately for now; a proper menu can gate this later.
            .add_systems(Startup, enter_playing_state);
    }
}

fn enter_playing_state(mut next_state: ResMut<NextState<GameState>>) {
    next_state.set(GameState::Playing);
}
