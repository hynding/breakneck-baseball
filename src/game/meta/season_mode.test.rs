//! Unit tests for [`super`] — starting, recording and abandoning season games.

use super::*;

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
        .init_state::<GameState>()
        .insert_resource(Settings::default())
        .insert_resource(ScoreBoard::default())
        .add_plugins(SeasonModePlugin);
    app.update();
    app
}

fn go(app: &mut App, state: GameState) {
    app.world_mut()
        .resource_mut::<NextState<GameState>>()
        .set(state);
    app.update();
}

fn finish_game(app: &mut App, home: u32, away: u32) {
    go(app, GameState::Playing);
    {
        let mut score = app.world_mut().resource_mut::<ScoreBoard>();
        score.home_runs = home;
        score.away_runs = away;
    }
    go(app, GameState::GameOver);
    go(app, GameState::MainMenu);
}

#[test]
fn begin_season_starts_continues_and_rolls_over() {
    let mut settings = Settings {
        season_length: 6,
        ..Default::default()
    };
    let first = begin_season(&mut settings);
    assert_eq!((first.game, first.is_final), (1, false));
    assert_eq!(settings.season.as_ref().unwrap().number, 1);

    // Continuing doesn't restart.
    settings.season.as_mut().unwrap().record(5, 0);
    assert_eq!(begin_season(&mut settings).game, 2);

    // A finished season rolls over to the next number at the menu's length.
    let season = settings.season.as_mut().unwrap();
    while season.next_game().is_some() {
        season.record(5, 0);
    }
    assert!(season.is_complete());
    settings.season_length = 12;
    assert_eq!(begin_season(&mut settings).game, 1);
    let next = settings.season.as_ref().unwrap();
    assert_eq!((next.number, next.length), (2, 12));
}

#[test]
fn a_finished_season_game_is_recorded_and_an_exhibition_is_not() {
    let mut app = app();
    begin_season(&mut app.world_mut().resource_mut::<Settings>());

    app.world_mut().resource_mut::<SeasonGame>().0 = true;
    finish_game(&mut app, 4, 2);
    let season = app.world().resource::<Settings>().season.clone().unwrap();
    assert_eq!(season.played(), 1);
    assert_eq!(season.human_record().wins, 1);

    app.world_mut().resource_mut::<SeasonGame>().0 = false;
    finish_game(&mut app, 0, 9);
    assert_eq!(
        app.world()
            .resource::<Settings>()
            .season
            .as_ref()
            .unwrap()
            .played(),
        1,
        "an exhibition game must not touch the season"
    );
}

#[test]
fn a_quit_season_game_is_not_recorded() {
    let mut app = app();
    begin_season(&mut app.world_mut().resource_mut::<Settings>());
    app.world_mut().resource_mut::<SeasonGame>().0 = true;
    go(&mut app, GameState::Playing);
    go(&mut app, GameState::Paused);
    go(&mut app, GameState::MainMenu);
    assert!(!app.world().resource::<SeasonGame>().0);
    assert_eq!(
        app.world()
            .resource::<Settings>()
            .season
            .as_ref()
            .unwrap()
            .played(),
        0
    );
}

#[test]
fn menu_status_reads_the_stored_season() {
    let mut settings = Settings::default();
    assert_eq!(menu_status(&settings), "New  (9 games)");
    begin_season(&mut settings);
    assert!(menu_status(&settings).starts_with("Game 1/9"));
}
