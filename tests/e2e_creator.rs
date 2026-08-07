//! Creator stage e2e (debug builds): C enters, the preview rig dresses
//! through the SAME pipeline gameplay uses, Esc leaves.
#![cfg(feature = "debug")]

mod common;

use bevy::prelude::*;
use breakneck_baseball::game::creator::{CreatorState, PreviewRig};
use breakneck_baseball::game::gear::DressedAs;
use breakneck_baseball::game::GameState;
use common::{headless_app, run_until, tap_key};

#[test]
fn creator_stage_dresses_the_preview_rig() {
    let mut app = headless_app();
    tap_key(&mut app, KeyCode::KeyC);
    let entered = run_until(&mut app, 2_000, |app| {
        *app.world().resource::<State<GameState>>().get() == GameState::Creator
    });
    assert!(entered.is_some(), "C on the menu must open the creator");
    let dressed = run_until(&mut app, 5_000, |app| {
        let world = app.world_mut();
        world
            .query_filtered::<&DressedAs, With<PreviewRig>>()
            .iter(world)
            .next()
            .is_some()
    });
    assert!(
        dressed.is_some(),
        "the preview rig must dress via the shared pipeline"
    );
    // Selection change re-dresses: pick the away team's slot 2.
    {
        let mut cs = app.world_mut().resource_mut::<CreatorState>();
        cs.team = breakneck_baseball::game::Team::Away;
        cs.index = 2;
    }
    let redressed = run_until(&mut app, 1_000, |app| {
        let world = app.world_mut();
        world
            .query_filtered::<&DressedAs, With<PreviewRig>>()
            .iter(world)
            .next()
            .map(|d| d.team() == breakneck_baseball::game::Team::Away)
            .unwrap_or(false)
    });
    assert!(
        redressed.is_some(),
        "selection change must re-dress the preview"
    );
    tap_key(&mut app, KeyCode::Escape);
    let left = run_until(&mut app, 2_000, |app| {
        *app.world().resource::<State<GameState>>().get() == GameState::MainMenu
    });
    assert!(left.is_some(), "Esc must return to the menu");
}
