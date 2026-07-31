//! Regression: after covering a ball in play the catcher jogs off the plate
//! (to cover home / back up a base) on the shared locomotion clip
//! [`AnimClip::RunCycle`]. If his `MoveIntent.target` is cleared while that
//! clip is still on him — an executor-ordering race between `locomote`,
//! `return_to_spots`, and `catcher_crouch` that fires deterministically on the
//! browser's single-threaded schedule — nothing ever takes the RunCycle back
//! off, so `catcher_crouch` (which only acts when the catcher has *no* clip)
//! leaves him standing bolt upright through the whole next duel, blocking the
//! catcher-POV camera.
//!
//! `locomote` owns `RunCycle` (it is the only system that adds or removes it),
//! so it must also drop it the instant there is no target to run to. This test
//! reproduces the stuck state directly — a leadoff at-bat reaches its duel,
//! then the catcher is forced into `RunCycle` with a cleared target (exactly
//! the state the race leaves behind) — and asserts the game reclaims him into
//! his crouch. It fails at HEAD (he stays in RunCycle forever) and passes once
//! `locomote` sheds a target-less RunCycle.

mod common;

use bevy::prelude::*;

use breakneck_baseball::game::animation::{AnimClip, MoveIntent, Playing};
use breakneck_baseball::game::ball::Baseball;
use breakneck_baseball::game::flow::{Phase, Play};
use breakneck_baseball::game::input::Intents;
use breakneck_baseball::game::player::CatcherRole;
use breakneck_baseball::game::variant::Ruleset;
use breakneck_baseball::game::{GameState, ScoreBoard};

use common::{headless_app, run_until, start_game, DriveGame};

#[derive(Resource, Default)]
struct Mode(u8); // 0 = drive the pitch, 1 = idle (let the duel sit)

fn start_two_player_game(app: &mut App) {
    app.init_resource::<Mode>();
    start_game(app, KeyCode::Digit2);
    let mut r = app.world_mut().resource_mut::<Ruleset>();
    r.perfect_ms = 40.0;
    r.solid_ms = 90.0;
    r.foul_ms = 140.0;
    r.exit_solid = 1.0;
    r.exit_perfect = 1.25;
}

/// Drives a leadoff at-bat toward the duel; goes quiet in idle mode so the
/// PrePitch duel sits (the window in which the catcher must be crouched).
fn drive(
    mode: Res<Mode>,
    state: Res<State<GameState>>,
    play: Option<Res<Play>>,
    score: Option<Res<ScoreBoard>>,
    mut intents: ResMut<Intents>,
    _ball: Query<&Transform, With<Baseball>>,
) {
    intents.home = default();
    intents.away = default();
    if *state.get() != GameState::Playing || mode.0 != 0 {
        return;
    }
    let (Some(play), Some(score)) = (play, score) else {
        return;
    };
    if play.phase == Phase::PrePitch {
        let intent = intents.get_mut(score.fielding_team());
        intent.aim = Vec2::ZERO;
        intent.action = true;
    }
}

fn catcher_entity(app: &mut App) -> Entity {
    let world = app.world_mut();
    let mut q = world.query_filtered::<Entity, With<CatcherRole>>();
    q.iter(world).next().expect("a catcher exists")
}

fn catcher_clip(app: &mut App, catcher: Entity) -> Option<AnimClip> {
    app.world().get::<Playing>(catcher).map(|p| p.clip)
}

#[test]
fn catcher_recrouches_after_covering_a_play() {
    let mut app = headless_app();
    app.add_systems(DriveGame, drive);
    start_two_player_game(&mut app);

    // Let the leadoff duel open and the catcher drop into his crouch.
    let reached = run_until(&mut app, 15_000, |app| {
        app.world().resource::<Play>().phase == Phase::PrePitch
    });
    assert!(reached.is_some(), "never reached the leadoff duel");
    // Stop pitching so the duel sits in PrePitch.
    app.world_mut().resource_mut::<Mode>().0 = 1;
    let catcher = catcher_entity(&mut app);
    let settled = run_until(&mut app, 600, |app| {
        catcher_clip(app, catcher) == Some(AnimClip::CatcherCrouch)
    });
    assert!(
        settled.is_some(),
        "catcher never took his crouch for the leadoff duel"
    );

    // Force the leaked state the cover-home / return race leaves behind: the
    // shared RunCycle locomotion clip still on the catcher, but with no target
    // to run to. `locomote` skips target-less movers, so at HEAD nothing sheds
    // the clip and `catcher_crouch` (acts only on a clipless catcher) can never
    // reclaim him.
    {
        let world = app.world_mut();
        world
            .entity_mut(catcher)
            .insert(Playing::new(AnimClip::RunCycle));
        if let Some(mut intent) = world.get_mut::<MoveIntent>(catcher) {
            intent.target = None;
        }
    }

    // The duel is still open (PrePitch). Give the rig systems ample frames to
    // reclaim the catcher.
    let recovered = run_until(&mut app, 600, |app| {
        catcher_clip(app, catcher) == Some(AnimClip::CatcherCrouch)
    });

    let clip = catcher_clip(&mut app, catcher);
    let phase = app.world().resource::<Play>().phase;
    assert_eq!(
        phase,
        Phase::PrePitch,
        "the duel should still be sitting in PrePitch"
    );
    assert!(
        recovered.is_some(),
        "the catcher was left standing in {clip:?} through the duel — he never \
         dropped back into his crouch after the stale RunCycle"
    );
}
