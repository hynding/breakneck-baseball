//! Dressing e2e: looks follow identity across flips, without per-pitch churn.

mod common;

use bevy::prelude::*;
use breakneck_baseball::game::gear::GearProp;
use breakneck_baseball::game::player::Batter;
use breakneck_baseball::game::roster::PlayerIdentity;
use breakneck_baseball::game::{ScoreBoard, Team};
use common::{headless_app, run_until, start_game};

/// Per-pitch identity re-stamps must not rebuild gear: prop entity ids for
/// an unchanged look stay stable across scoreboard changes.
#[test]
fn gear_survives_count_changes_without_respawning() {
    let mut app = headless_app();
    start_game(&mut app, KeyCode::Digit1);
    // Readiness = the dressed-rig count has been STABLE for 60 frames
    // (async per-rig wiring means "some gear exists" races a late rig).
    let mut stable_frames = 0u32;
    let mut last_count = 0usize;
    let ready = run_until(&mut app, 10_000, |app| {
        let world = app.world_mut();
        let count = world
            .query::<&breakneck_baseball::game::gear::DressedAs>()
            .iter(world)
            .count();
        if count > 0 && count == last_count {
            stable_frames += 1;
        } else {
            stable_frames = 0;
            last_count = count;
        }
        stable_frames >= 60
    });
    assert!(ready.is_some(), "dressed-rig count never stabilized");
    let world = app.world_mut();
    let before: Vec<Entity> = world
        .query_filtered::<Entity, With<GearProp>>()
        .iter(world)
        .collect();
    // Force a scoreboard change (a ball on the count) — identities re-stamp.
    world.resource_mut::<ScoreBoard>().balls += 1;
    for _ in 0..8 {
        app.update();
    }
    let world = app.world_mut();
    let after: Vec<Entity> = world
        .query_filtered::<Entity, With<GearProp>>()
        .iter(world)
        .collect();
    assert_eq!(
        before, after,
        "unchanged looks must not respawn props on count changes"
    );
}

/// After a half-inning flip the batter rig is a different team's player —
/// its DressedAs must follow (the old batter look would be a wrong-team leak).
#[test]
fn batter_redresses_on_half_inning_flip() {
    let mut app = headless_app();
    start_game(&mut app, KeyCode::Digit1);
    let ready = run_until(&mut app, 5_000, |app| {
        let world = app.world_mut();
        world
            .query_filtered::<&PlayerIdentity, With<Batter>>()
            .iter(world)
            .next()
            .is_some()
    });
    assert!(ready.is_some());
    // Flip the half-inning wholesale (outs reset, sides swap).
    {
        let mut score = app.world_mut().resource_mut::<ScoreBoard>();
        score.top_of_inning = false;
    }
    // The claim under test is the DRESSING following the flip, not just
    // identity (Phase 1 already pins identity) — read DressedAs::team().
    let flipped = run_until(&mut app, 1_000, |app| {
        let world = app.world_mut();
        world
            .query_filtered::<&breakneck_baseball::game::gear::DressedAs, With<Batter>>()
            .iter(world)
            .next()
            .map(|d| d.team() == Team::Home)
            .unwrap_or(false)
    });
    assert!(flipped.is_some(), "batter dressing must follow the flip");
}

/// Spec §7: runner rigs are dressed too. Manifest bases-loaded runners via
/// the scenario seam (the e2e_identity pattern) and assert each carries
/// DressedAs once wired.
#[test]
fn runner_rigs_are_dressed() {
    use breakneck_baseball::game::gear::DressedAs;
    use breakneck_baseball::game::runner::Runner;
    use breakneck_baseball::game::scenario::{apply_to_world, presets, PRESET_LOADED};
    let mut app = headless_app();
    start_game(&mut app, KeyCode::Digit1);
    let s = presets()
        .into_iter()
        .find(|s| s.name == PRESET_LOADED)
        .unwrap();
    apply_to_world(app.world_mut(), &s).expect("ball is dead at PrePitch");
    let dressed = run_until(&mut app, 5_000, |app| {
        let world = app.world_mut();
        let runners: Vec<Entity> = world
            .query_filtered::<Entity, With<Runner>>()
            .iter(world)
            .collect();
        runners.len() == 3 && runners.iter().all(|&r| world.get::<DressedAs>(r).is_some())
    });
    assert!(dressed.is_some(), "all three scenario runners must dress");
}
