//! End-to-end: the player-polish reactions (graphics plan task 11). Strike
//! three gets the pitcher's cap tip and the batter's dejected walk to the
//! dugout, which ends cleanly when the next at-bat starts; and only fielders
//! wear the mitt.

use crate::common;

use bevy::prelude::*;
use bevy_rapier3d::prelude::Velocity;

use breakneck_baseball::game::GameState;
use breakneck_baseball::game::ScoreBoard;
use breakneck_baseball::game::animation::{AnimClip, Playing};
use breakneck_baseball::game::ball::Baseball;
use breakneck_baseball::game::flow::{Phase, Play, ResultBeat, bat_arrival_z};
use breakneck_baseball::game::input::Intents;
use breakneck_baseball::game::model_assets::{RigAnimations, RigPlayer};
use breakneck_baseball::game::player::{Batter, Pitcher, RigUnit, RigUnitTag};

use common::{DriveGame, headless_app, run_until, start_game};

const MAX_FRAMES: u64 = 30_000;

/// Pitches whenever the ball is held and swings far too early at every
/// pitch — three whiffs, a strikeout (the `walkup.rs` script).
fn drive(
    state: Res<State<GameState>>,
    play: Option<Res<Play>>,
    score: Option<Res<ScoreBoard>>,
    mut intents: ResMut<Intents>,
    ball: Query<(&Transform, &Velocity), With<Baseball>>,
) {
    if *state.get() != GameState::Playing {
        return;
    }
    let (Some(play), Some(score)) = (play, score) else {
        return;
    };
    intents.home = default();
    intents.away = default();
    match play.phase {
        Phase::PrePitch => intents.get_mut(score.fielding_team()).action = true,
        Phase::Pitch => {
            if let Ok((t, v)) = ball.single()
                && (5.0..=6.0).contains(&bat_arrival_z(t.translation.z, v.linvel.z))
            {
                intents.get_mut(score.batting_team()).action = true;
            }
        }
        _ => {}
    }
}

fn clip_of<M: Component>(app: &mut App) -> Option<AnimClip> {
    app.world_mut()
        .query_filtered::<Option<&Playing>, With<M>>()
        .single(app.world())
        .unwrap()
        .map(|p| p.clip)
}

#[test]
fn strike_three_gets_a_cap_tip_and_a_dejected_walk_off() {
    let mut app = headless_app();
    app.add_systems(DriveGame, drive);
    start_game(&mut app, KeyCode::Digit2);

    let k = run_until(&mut app, MAX_FRAMES, |app| {
        let play = app.world().resource::<Play>();
        play.phase == Phase::Result && play.result_beat() == Some(ResultBeat::Strikeout)
    });
    assert!(k.is_some(), "three early whiffs never produced a strikeout");

    let tipped = run_until(&mut app, 3, |app| {
        clip_of::<Pitcher>(app) == Some(AnimClip::CapTip)
    });
    assert!(
        tipped.is_some(),
        "the pitcher must tip his cap on strike three"
    );

    // He turns for the dugout a beat into the pause (`runner.rs`
    // WALK_OFF_DELAY, on the pause's own clock).
    let trudging = run_until(&mut app, MAX_FRAMES, |app| {
        app.world().resource::<Play>().phase != Phase::Result
            || clip_of::<Batter>(app) == Some(AnimClip::DejectedWalk)
    });
    assert!(
        trudging.is_some() && clip_of::<Batter>(&mut app) == Some(AnimClip::DejectedWalk),
        "the struck-out batter must walk off on the dejected walk"
    );

    // The next at-bat ends the trudge: the batter is back in his stance, not
    // dragging the walk into the duel.
    let next_duel = run_until(&mut app, MAX_FRAMES, |app| {
        app.world().resource::<Play>().phase == Phase::Pitch
    });
    assert!(next_duel.is_some(), "the next pitch never came");
    let clip = clip_of::<Batter>(&mut app);
    assert!(
        clip != Some(AnimClip::DejectedWalk),
        "the dejected walk outlived the strikeout: {clip:?}"
    );
}

#[test]
fn only_fielders_wear_the_mitt() {
    let mut app = headless_app();
    start_game(&mut app, KeyCode::Digit2);
    let wired = run_until(&mut app, 4_000, |app| {
        let world = app.world_mut();
        let roots = world
            .query_filtered::<(), With<RigUnitTag>>()
            .iter(world)
            .count();
        let done = world
            .query_filtered::<(), (With<RigUnitTag>, With<RigPlayer>)>()
            .iter(world)
            .count();
        roots > 0 && done == roots
    });
    assert!(wired.is_some(), "rigs never wired");
    app.update(); // the wiring's visibility inserts land

    let glove = app
        .world()
        .resource::<RigAnimations>()
        .glove_material
        .clone();
    let world = app.world_mut();
    let gloves: Vec<(Entity, Visibility)> = world
        .query::<(Entity, &MeshMaterial3d<StandardMaterial>, &Visibility)>()
        .iter(world)
        .filter(|(_, m, _)| m.0 == glove)
        .map(|(e, _, v)| (e, *v))
        .collect();
    assert!(!gloves.is_empty(), "the model has no mitt submesh");
    let (mut shown, mut hidden) = (0, 0);
    for (mesh, visibility) in gloves {
        // Climb to the rig root to read whose glove this is.
        let mut e = mesh;
        let unit = loop {
            if let Some(tag) = world.get::<RigUnitTag>(e) {
                break tag.0;
            }
            e = world
                .get::<ChildOf>(e)
                .expect("mitt outside a rig")
                .parent();
        };
        if unit == RigUnit::Defense {
            assert_eq!(visibility, Visibility::Inherited, "a fielder lost his mitt");
            shown += 1;
        } else {
            assert_eq!(visibility, Visibility::Hidden, "{unit:?} rig wears a mitt");
            hidden += 1;
        }
    }
    assert!(shown >= 9, "every fielder wears one, saw {shown}");
    assert!(hidden >= 2, "the batter and umpires must not, saw {hidden}");
}
