//! Short season e2e: **3** on the menu launches a one-player season game,
//! its final score lands in the persisted standings on the way to the
//! game-over card, the card shows the table, and the menu's Season line
//! names the next opponent. An exhibition game in between leaves the season
//! alone.

use crate::common;

use bevy::prelude::*;
use breakneck_baseball::game::season_mode::SeasonGame;
use breakneck_baseball::game::settings::Settings;
use breakneck_baseball::game::{GameConfig, GameMode, GameState, ScoreBoard};
use common::{headless_app, start_game};

fn all_text(app: &mut App) -> String {
    let world = app.world_mut();
    let mut all = String::new();
    for text in world.query::<&Text>().iter(world) {
        all.push_str(&text.0);
        all.push('\n');
    }
    for span in world.query::<&TextSpan>().iter(world) {
        all.push_str(&span.0);
        all.push('\n');
    }
    all
}

fn go(app: &mut App, state: GameState) {
    app.world_mut()
        .resource_mut::<NextState<GameState>>()
        .set(state);
    for _ in 0..4 {
        app.update();
    }
}

fn played(app: &App) -> u32 {
    app.world()
        .resource::<Settings>()
        .season
        .as_ref()
        .map_or(0, |s| s.played())
}

#[test]
fn a_season_game_records_into_the_standings() {
    let mut app = headless_app();
    for _ in 0..4 {
        app.update();
    }
    // The e2e processes share one settings file: start from a clean slate.
    {
        let mut settings = app.world_mut().resource_mut::<Settings>();
        settings.season = None;
        settings.season_length = 6;
    }
    app.update();
    assert!(
        all_text(&mut app).contains("New  (6 games)"),
        "menu should offer a new 6-game season:\n{}",
        all_text(&mut app)
    );

    start_game(&mut app, KeyCode::Digit3);
    assert!(app.world().resource::<SeasonGame>().0);
    assert_eq!(
        app.world().resource::<GameConfig>().mode,
        GameMode::OnePlayer
    );
    assert_eq!(played(&app), 0);

    {
        let mut score = app.world_mut().resource_mut::<ScoreBoard>();
        score.home_runs = 5;
        score.away_runs = 2;
    }
    go(&mut app, GameState::GameOver);
    assert_eq!(played(&app), 1);
    let card = all_text(&mut app);
    assert!(card.contains("Next: game 2/6 vs Gators"), "{card}");
    assert!(
        card.contains("You*"),
        "a 1-0 human is a finalist so far:\n{card}"
    );

    go(&mut app, GameState::MainMenu);
    let menu = all_text(&mut app);
    assert!(menu.contains("Game 2/6 vs Gators  (1-0)"), "{menu}");

    // An exhibition game is not a season game.
    start_game(&mut app, KeyCode::Digit1);
    assert!(!app.world().resource::<SeasonGame>().0);
    go(&mut app, GameState::GameOver);
    assert_eq!(played(&app), 1, "the exhibition must not be recorded");
    assert!(!all_text(&mut app).contains("Next: game"));
    go(&mut app, GameState::MainMenu);

    app.world_mut().resource_mut::<Settings>().season = None;
    app.update();
}
