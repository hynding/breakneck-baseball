//! Top-level game module.
//!
//! `GamePlugin` registers every sub-plugin in dependency order and exposes the
//! shared [`GameState`] state machine to all systems.

pub mod ai;
pub mod animation;
pub mod audio;
pub mod ball;
pub mod camera;
pub mod field;
pub mod fielding;
pub mod flow;
pub mod fx;
pub mod input;
pub mod jersey;
pub mod menu;
pub mod player;
pub mod roster;
pub mod rules;
pub mod runner;
pub mod subs;
pub mod theme;
pub mod ui;
pub mod variant;

use bevy::prelude::*;

use animation::AnimationPlugin;
use audio::SoundPlugin;
use ball::BallPlugin;
use camera::CameraPlugin;
use field::FieldPlugin;
use fielding::FieldingPlugin;
use flow::FlowPlugin;
use fx::FxPlugin;
use input::InputPlugin;
use jersey::JerseyPlugin;
use menu::MenuPlugin;
use player::PlayerPlugin;
use roster::Rosters;
use runner::RunnerPlugin;
use subs::SubsPlugin;
use theme::ThemeId;
use ui::UiPlugin;
use variant::VariantId;

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

    /// HUD/menu label.
    pub fn label(self) -> &'static str {
        match self {
            Team::Home => "HOME",
            Team::Away => "AWAY",
        }
    }
}

/// The schedule that fires once when a game starts from the menu — the slot
/// for every scene spawn/reset system. Deliberately *not* `OnEnter(Playing)`:
/// resuming from `Paused` re-enters `Playing` and must not respawn anything.
pub(crate) fn game_start() -> OnTransition<GameState> {
    OnTransition {
        exited: GameState::MainMenu,
        entered: GameState::Playing,
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

/// Chosen game options, set by the menu before entering [`GameState::Playing`].
/// The variant's [`variant::Ruleset`] and [`variant::FieldSpec`] resources are
/// (re)written from `variant` when a game starts; the [`theme::Theme`]
/// resource is rewritten whenever `theme` is cycled on the menu.
#[derive(Resource, Debug)]
pub struct GameConfig {
    pub mode: GameMode,
    pub variant: VariantId,
    pub theme: ThemeId,
    /// Regulation innings for the next game; menu-cycled through
    /// [`variant::INNINGS_OPTIONS`], seeded from the variant's default
    /// whenever the variant changes.
    pub innings: u32,
}

impl Default for GameConfig {
    fn default() -> Self {
        let variant = VariantId::default();
        Self {
            mode: GameMode::default(),
            innings: variant.rules().innings,
            variant,
            theme: ThemeId::default(),
        }
    }
}

/// Global game-state machine.
///
/// Systems that should only run while the game is active use
/// `.run_if(in_state(GameState::Playing))`. Scene spawn/reset systems key on
/// the `MainMenu → Playing` *transition* (not `OnEnter(Playing)`), and
/// teardown on `Playing → GameOver`, so that pausing (`Playing ⇄ Paused`)
/// leaves the whole scene intact.
#[derive(States, Debug, Clone, Eq, PartialEq, Hash, Default)]
pub enum GameState {
    /// The game has not started yet (title screen / menus).
    #[default]
    MainMenu,
    /// Active gameplay.
    Playing,
    /// Stopped between plays — the substitution board (see `subs.rs`).
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

    /// Resets to the start of a brand-new game.
    pub fn reset(&mut self) {
        *self = ScoreBoard {
            inning: 1,
            top_of_inning: true,
            ..default()
        };
    }
}

/// Marks every entity spawned for active gameplay (field, ball, players, HUD)
/// so the whole scene can be torn down when a game ends and rebuilt on restart.
#[derive(Component)]
pub struct GameplayEntity;

/// Aggregate plugin that wires every sub-system into the app.
pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app
            // State machine
            .init_state::<GameState>()
            // Shared resources
            .init_resource::<GameConfig>()
            // Variant data defaults to standard baseball; the menu overwrites
            // both resources with the chosen variant before a game starts.
            .insert_resource(VariantId::Standard.rules())
            .insert_resource(VariantId::Standard.field())
            .insert_resource(ThemeId::DaylightClassic.build())
            .insert_resource(ScoreBoard {
                inning: 1,
                top_of_inning: true,
                ..default()
            })
            .init_resource::<Rosters>()
            // Sub-plugins (input/menu first so their resources exist for the rest)
            .add_plugins((
                InputPlugin,
                MenuPlugin,
                FieldPlugin,
                BallPlugin,
                PlayerPlugin,
                AnimationPlugin,
                FlowPlugin,
                FxPlugin,
                SoundPlugin,
                FieldingPlugin,
                RunnerPlugin,
                CameraPlugin,
                UiPlugin,
                JerseyPlugin,
                SubsPlugin,
            ))
            // Fresh scoreboard/rosters each time a game starts from the menu;
            // tear the scene down once the game is over. Pausing stays inside
            // Playing ⇄ Paused and touches neither.
            .add_systems(game_start(), reset_scoreboard)
            .add_systems(
                OnTransition {
                    exited: GameState::Playing,
                    entered: GameState::GameOver,
                },
                cleanup_gameplay,
            );
        // The game now boots to `GameState::MainMenu` (the default) and the menu
        // transitions into `Playing` once a mode is chosen.
    }
}

/// Resets the scoreboard (and both rosters) whenever a new game begins.
fn reset_scoreboard(mut score: ResMut<ScoreBoard>, mut rosters: ResMut<Rosters>) {
    score.reset();
    *rosters = Rosters::default();
}

/// Despawns all gameplay entities when leaving `Playing` so a restart rebuilds
/// the scene cleanly (each sub-plugin re-spawns on the next `OnEnter`).
fn cleanup_gameplay(mut commands: Commands, query: Query<Entity, With<GameplayEntity>>) {
    for entity in &query {
        commands.entity(entity).despawn_recursive();
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_innings_follow_the_default_variant() {
        assert_eq!(GameConfig::default().innings, 9);
    }
}
