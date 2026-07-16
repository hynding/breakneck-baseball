//! Top-level game module.
//!
//! `GamePlugin` registers every sub-plugin in dependency order and exposes the
//! shared [`GameState`] state machine to all systems.

pub mod ball;
pub mod camera;
pub mod field;
pub mod flow;
pub mod input;
pub mod menu;
pub mod player;
pub mod ui;

use bevy::prelude::*;

use ball::BallPlugin;
use camera::CameraPlugin;
use field::FieldPlugin;
use flow::FlowPlugin;
use input::InputPlugin;
use menu::MenuPlugin;
use player::PlayerPlugin;
use ui::UiPlugin;

/// The two teams. In 1-player mode the human is always [`Team::Home`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Team {
    Home,
    Away,
}

impl Team {
    /// The opposing team.
    pub fn other(self) -> Team {
        match self {
            Team::Home => Team::Away,
            Team::Away => Team::Home,
        }
    }
}

/// How many humans are playing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GameMode {
    /// Human = Home, CPU = Away.
    #[default]
    OnePlayer,
    /// Human P1 = Home, human P2 = Away.
    TwoPlayers,
}

/// Number of regulation innings. Adjust here to shorten a game for testing.
pub const REGULATION_INNINGS: u32 = 9;

/// Chosen game options, set by the menu before entering [`GameState::Playing`].
#[derive(Resource, Debug)]
pub struct GameConfig {
    pub mode: GameMode,
    pub innings: u32,
}

impl Default for GameConfig {
    fn default() -> Self {
        Self {
            mode: GameMode::OnePlayer,
            innings: REGULATION_INNINGS,
        }
    }
}

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

impl ScoreBoard {
    /// The team currently at bat. Away hits in the top half, Home in the bottom.
    pub fn batting_team(&self) -> Team {
        if self.top_of_inning {
            Team::Away
        } else {
            Team::Home
        }
    }

    /// The team currently in the field.
    pub fn fielding_team(&self) -> Team {
        self.batting_team().other()
    }

    /// Adds `runs` to the batting team's total.
    pub fn add_runs(&mut self, runs: u32) {
        match self.batting_team() {
            Team::Home => self.home_runs += runs,
            Team::Away => self.away_runs += runs,
        }
    }
}

/// Aggregate plugin that wires every sub-system into the app.
pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app
            // State machine
            .init_state::<GameState>()
            // Shared resources
            .init_resource::<GameConfig>()
            .insert_resource(ScoreBoard {
                inning: 1,
                top_of_inning: true,
                ..default()
            })
            // Sub-plugins (input/menu first so their resources exist for the rest)
            .add_plugins((
                InputPlugin,
                MenuPlugin,
                FieldPlugin,
                BallPlugin,
                PlayerPlugin,
                FlowPlugin,
                CameraPlugin,
                UiPlugin,
            ));
        // The game now boots to `GameState::MainMenu` (the default) and the menu
        // transitions into `Playing` once a mode is chosen.
    }
}
