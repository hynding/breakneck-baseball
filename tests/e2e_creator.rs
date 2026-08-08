//! Creator stage e2e (debug builds): C enters, the preview rig dresses
//! through the SAME pipeline gameplay uses, Esc leaves.
#![cfg(feature = "debug")]

mod common;

use bevy::prelude::*;
use breakneck_baseball::game::appearance::{Headwear, RosterDefs};
use breakneck_baseball::game::creator::{save_working_to, selected_def, CreatorState, PreviewRig};
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

/// End-to-end version of `save_working_to`'s unit-level validation test: a
/// working copy edited (through the Creator, no panel) to an invalid name
/// must be rejected on save and must not touch disk at all.
#[test]
fn save_rejects_an_invalid_name_and_writes_nothing() {
    let mut app = headless_app();
    tap_key(&mut app, KeyCode::KeyC);
    let entered = run_until(&mut app, 2_000, |app| {
        *app.world().resource::<State<GameState>>().get() == GameState::Creator
    });
    assert!(entered.is_some(), "C on the menu must open the creator");

    {
        let mut cs = app.world_mut().resource_mut::<CreatorState>();
        let (team, index) = (cs.team, cs.index);
        selected_def(&mut cs.working, team, index).name = "bad!".to_string();
    }

    let dir = std::env::temp_dir().join(format!("bb-creator-e2e-save-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("players.ron");
    std::fs::remove_file(&path).ok();
    let path_str = path.to_str().unwrap();

    let cs = app.world().resource::<CreatorState>();
    let result = save_working_to(path_str, &cs.working);

    assert!(result.is_err(), "an invalid name must be rejected");
    assert!(!path.exists(), "a rejected save must not write anything");

    std::fs::remove_file(&path).ok();
}

fn cap_visibility(app: &mut App) -> Option<Visibility> {
    let world = app.world_mut();
    let rig = world
        .query_filtered::<Entity, With<PreviewRig>>()
        .iter(world)
        .next()?;
    let caps = world.get::<RigCapMeshes>(rig)?;
    if caps.0.is_empty() {
        return None;
    }
    let visibilities: Vec<Visibility> = caps
        .0
        .iter()
        .filter_map(|&mesh| world.get::<Visibility>(mesh).copied())
        .collect();
    if visibilities.len() != caps.0.len() {
        return None;
    }
    visibilities
        .windows(2)
        .all(|w| w[0] == w[1])
        .then(|| visibilities[0])
}

/// Finding 1 (creator-open watcher clobber): while the Creator is open, an
/// external `RosterDefs` reload (the dev watcher hot-swapping
/// `data/players.ron` after an AI/editor edit — simulated here by writing
/// straight to the `RosterDefs` resource, exactly like
/// `dev_watch::watch_players_file` would) must (a) flow into
/// `CreatorState::working`/`snapshot` and re-dress the preview, and (b)
/// survive leaving the Creator — the unconditional `OnExit` revert must not
/// stomp it back out just because the user made no panel edits of their own.
#[test]
fn external_reload_syncs_creator_state_and_survives_exit() {
    let mut app = headless_app();
    tap_key(&mut app, KeyCode::KeyC);
    let entered = run_until(&mut app, 2_000, |app| {
        *app.world().resource::<State<GameState>>().get() == GameState::Creator
    });
    assert!(entered.is_some(), "C on the menu must open the creator");

    // Select HOLT (home index 5), same as the panel-less-apply test above:
    // default appearance, so `Headwear::Cap` starts the cap mesh visible —
    // a real transition to prove once the simulated reload flips it.
    {
        let mut cs = app.world_mut().resource_mut::<CreatorState>();
        cs.team = Team::Home;
        cs.index = 5;
    }
    let selected = run_until(&mut app, 2_000, |app| {
        cap_visibility(app) == Some(Visibility::Inherited)
    });
    assert!(
        selected.is_some(),
        "HOLT's default Cap headwear must start the cap mesh visible"
    );

    // Simulate the dev watcher: build fresh content that diverges from both
    // `cs.working` and `cs.snapshot` (the external-reload invariant) and
    // write it straight into `RosterDefs`, exactly as
    // `dev_watch::watch_players_file` does on a real disk change — no panel,
    // no `apply_creator_edits`, in the loop at all.
    let reloaded = {
        let defs = app.world().resource::<RosterDefs>();
        let mut file = defs.0.clone();
        file.home[5].appearance.headwear = Headwear::Bare;
        file
    };
    *app.world_mut().resource_mut::<RosterDefs>() = RosterDefs(reloaded.clone());

    // (part a) The reload must fold into both copies and re-dress the
    // preview — no panel edit involved anywhere in this test.
    let synced = run_until(&mut app, 2_000, |app| {
        let cs = app.world().resource::<CreatorState>();
        cs.working == reloaded && cs.snapshot == reloaded
    });
    assert!(
        synced.is_some(),
        "an external RosterDefs reload must sync into cs.working and cs.snapshot"
    );
    let redressed = run_until(&mut app, 2_000, |app| {
        cap_visibility(app) == Some(Visibility::Hidden)
    });
    assert!(
        redressed.is_some(),
        "the synced reload must re-dress the preview (cap hidden for Headwear::Bare)"
    );

    // (part b) Leaving the Creator now, with zero panel edits, must not
    // revert the live RosterDefs back to the pre-reload content.
    tap_key(&mut app, KeyCode::Escape);
    let left = run_until(&mut app, 2_000, |app| {
        *app.world().resource::<State<GameState>>().get() == GameState::MainMenu
    });
    assert!(left.is_some(), "Esc must return to the menu");
    assert_eq!(
        app.world().resource::<RosterDefs>().0,
        reloaded,
        "exiting the Creator with no user edits must not clobber an external reload"
    );
}
