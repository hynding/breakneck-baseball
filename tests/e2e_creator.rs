//! Creator stage e2e (debug builds): C enters, the preview rig dresses
//! through the SAME pipeline gameplay uses, Esc leaves.
#![cfg(feature = "debug")]

mod common;

use bevy::prelude::*;
use breakneck_baseball::game::appearance::Headwear;
use breakneck_baseball::game::creator::{selected_def, CreatorState, PreviewRig};
use breakneck_baseball::game::gear::DressedAs;
use breakneck_baseball::game::model_assets::RigCapMeshes;
use breakneck_baseball::game::{GameState, Team};
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

/// The apply path is deliberately a system separate from the egui panel —
/// the panel only ever mutates `CreatorState.working` (+ `.status`), and
/// this test proves the pipeline reacts with no panel (and so no egui
/// context at all) anywhere in the loop: mutate `cs.working` directly, the
/// same way a headless driver or a future scripted tool would, and expect
/// the preview rig to re-dress exactly as if the panel had done it.
#[test]
fn creator_apply_path_updates_preview_without_the_panel() {
    let mut app = headless_app();
    tap_key(&mut app, KeyCode::KeyC);
    let entered = run_until(&mut app, 2_000, |app| {
        *app.world().resource::<State<GameState>>().get() == GameState::Creator
    });
    assert!(entered.is_some(), "C on the menu must open the creator");

    // Select HOLT (home index 5): an authored player with no appearance
    // overrides, so `Headwear::Cap` (the default) starts the cap *visible* —
    // a real transition to prove, unlike VEGA (index 0, already `Helmet`).
    {
        let mut cs = app.world_mut().resource_mut::<CreatorState>();
        cs.team = Team::Home;
        cs.index = 5;
    }
    let selected = run_until(&mut app, 2_000, |app| {
        let world = app.world_mut();
        let Some(rig) = world
            .query_filtered::<Entity, With<PreviewRig>>()
            .iter(world)
            .next()
        else {
            return false;
        };
        world
            .get::<RigCapMeshes>(rig)
            .map(|caps| {
                !caps.0.is_empty()
                    && caps.0.iter().all(|&mesh| {
                        world
                            .get::<Visibility>(mesh)
                            .is_some_and(|v| *v == Visibility::Inherited)
                    })
            })
            .unwrap_or(false)
    });
    assert!(
        selected.is_some(),
        "HOLT's default Cap headwear must start the cap mesh visible"
    );

    // Mutate the working copy directly — no panel involved — and trigger
    // the panel-independent apply path.
    {
        let mut cs = app.world_mut().resource_mut::<CreatorState>();
        let (team, index) = (cs.team, cs.index);
        selected_def(&mut cs.working, team, index)
            .appearance
            .headwear = Headwear::Bare;
    }

    let hidden = run_until(&mut app, 2_000, |app| {
        let world = app.world_mut();
        let Some(rig) = world
            .query_filtered::<Entity, With<PreviewRig>>()
            .iter(world)
            .next()
        else {
            return false;
        };
        world
            .get::<RigCapMeshes>(rig)
            .map(|caps| {
                !caps.0.is_empty()
                    && caps.0.iter().all(|&mesh| {
                        world
                            .get::<Visibility>(mesh)
                            .is_some_and(|v| *v == Visibility::Hidden)
                    })
            })
            .unwrap_or(false)
    });
    assert!(
        hidden.is_some(),
        "headwear -> Bare via the working copy alone (no panel) must hide the cap mesh"
    );
}
