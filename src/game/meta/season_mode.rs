//! Short season mode's shell glue: which game is a season game, starting or
//! continuing the stored season when the menu launches one, and recording
//! the final score into it when the game ends. The league itself — schedule,
//! simulated CPU games, standings, final — is pure data in
//! [`crate::game::season`], persisted as [`Settings::season`].
//!
//! A season game is an ordinary one-player game (human = Home); nothing in
//! `sim/` knows the difference. A game quit from the pause board is not
//! recorded — the same matchup is still next on the schedule.

use bevy::prelude::*;

use crate::game::season::{Matchup, Season};
use crate::game::settings::Settings;
use crate::game::{GameState, ScoreBoard};

/// Whether the game being played (or just finished — the game-over card
/// reads it) is a season game. Set by the menu's launch path for every
/// launch, so a later exhibition game never inherits it.
#[derive(Resource, Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct SeasonGame(pub bool);

pub struct SeasonModePlugin;

impl Plugin for SeasonModePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SeasonGame>()
            // Before `OnEnter(GameOver)` (Bevy runs OnExit → OnTransition →
            // OnEnter), so the game-over card already shows the new standings.
            .add_systems(
                OnTransition {
                    exited: GameState::Playing,
                    entered: GameState::GameOver,
                },
                record_season_game,
            )
            .add_systems(
                OnTransition {
                    exited: GameState::Paused,
                    entered: GameState::MainMenu,
                },
                abandon_season_game,
            );
    }
}

/// Makes sure a season with a game left to play is stored, starting the next
/// season (the length the menu shows, numbered on from the last) when there
/// is none or the last one is over, and returns the human's next matchup.
pub fn begin_season(settings: &mut Settings) -> Matchup {
    if let Some(next) = settings.season.as_ref().and_then(Season::next_game) {
        return next;
    }
    let number = settings.season.as_ref().map_or(1, |s| s.number + 1);
    let season = Season::new(number, settings.season_length);
    let next = season
        .next_game()
        .expect("a fresh season always has a first game");
    settings.season = Some(season);
    next
}

/// The menu's Season-row value: progress in the stored season, or what
/// pressing it would start.
pub fn menu_status(settings: &Settings) -> String {
    match &settings.season {
        Some(season) => season.status_line(),
        None => format!("New  ({} games)", settings.season_length),
    }
}

/// Records a finished season game. Home is the human, as for every
/// one-player game.
fn record_season_game(
    season_game: Res<SeasonGame>,
    score: Res<ScoreBoard>,
    mut settings: ResMut<Settings>,
) {
    if !season_game.0 {
        return;
    }
    if let Some(season) = settings.season.as_mut() {
        season.record(score.home_runs, score.away_runs);
    }
}

/// A season game quit from the pause board is simply not counted.
fn abandon_season_game(mut season_game: ResMut<SeasonGame>) {
    season_game.0 = false;
}

#[cfg(test)]
#[path = "season_mode.test.rs"]
mod tests;
