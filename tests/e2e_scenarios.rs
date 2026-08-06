//! Scenario presets applied to the live headless game: the jump-cut template
//! for rule regressions — no inning-scripting to reach a situation.

mod common;

use bevy::prelude::*;
use breakneck_baseball::game::flow::Play;
use breakneck_baseball::game::runner::Runner;
use breakneck_baseball::game::scenario::{apply_to_world, presets, PRESET_LOADED};
use breakneck_baseball::game::ScoreBoard;
use common::{headless_app, run_until, start_game};

#[test]
fn bases_loaded_preset_manifests_runners_and_count() {
    let mut app = headless_app();
    start_game(&mut app, KeyCode::Digit1);
    let s = presets()
        .into_iter()
        .find(|s| s.name == PRESET_LOADED)
        .unwrap();
    apply_to_world(app.world_mut(), &s).expect("ball is dead at PrePitch");

    let score = app.world().resource::<ScoreBoard>();
    assert_eq!((score.balls, score.strikes, score.outs), (3, 2, 2));

    // The runner mirror walks rigs onto every occupied bag.
    let settled = run_until(&mut app, 5_000, |app| {
        let mut q = app.world_mut().query::<&Runner>();
        q.iter(app.world()).count() == 3
    });
    assert!(
        settled.is_some(),
        "three runner rigs must appear for bases loaded"
    );
}

#[test]
fn steal_preset_opens_the_window() {
    let mut app = headless_app();
    start_game(&mut app, KeyCode::Digit1);
    let s = presets()
        .into_iter()
        .find(|s| s.name == "Steal duel: R1")
        .unwrap();
    apply_to_world(app.world_mut(), &s).unwrap();
    app.update();
    assert!(
        app.world().resource::<Play>().in_steal_window(),
        "a runner on first must reopen the steal window"
    );
}
