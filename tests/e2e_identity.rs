//! Identity plumbing e2e: rigs know who they are; runners wear jerseys.

mod common;

use bevy::prelude::*;
use breakneck_baseball::game::jersey::JerseyQuad;
use breakneck_baseball::game::player::{Batter, Pitcher};
use breakneck_baseball::game::roster::PlayerIdentity;
use breakneck_baseball::game::runner::Runner;
use breakneck_baseball::game::scenario::{apply_to_world, presets, PRESET_LOADED};
use breakneck_baseball::game::Team;
use common::{headless_app, run_until, start_game};

/// JerseyQuads start as rig-root children and re-parent onto bones once the
/// async glTF wiring lands — either way they stay descendants of the root.
fn count_quads(world: &mut World, root: Entity) -> usize {
    let mut count = 0;
    let mut stack = vec![root];
    while let Some(e) = stack.pop() {
        if world.get::<JerseyQuad>(e).is_some() {
            count += 1;
        }
        if let Some(children) = world.get::<Children>(e) {
            stack.extend(children.iter().copied());
        }
    }
    count
}

#[test]
fn seated_rigs_are_identified_at_kickoff() {
    let mut app = headless_app();
    start_game(&mut app, KeyCode::Digit1);
    // Top 1st: Away bats slot 1, Home pitches.
    let world = app.world_mut();
    let batter_id = *world
        .query_filtered::<&PlayerIdentity, With<Batter>>()
        .single(world);
    assert_eq!(
        batter_id,
        PlayerIdentity {
            team: Team::Away,
            index: 0
        }
    );
    let pitcher_id = *world
        .query_filtered::<&PlayerIdentity, With<Pitcher>>()
        .single(world);
    assert_eq!(
        pitcher_id,
        PlayerIdentity {
            team: Team::Home,
            index: 0
        }
    );
}

#[test]
fn runner_rigs_are_identified_and_wear_jerseys() {
    let mut app = headless_app();
    start_game(&mut app, KeyCode::Digit1);
    let s = presets()
        .into_iter()
        .find(|s| s.name == PRESET_LOADED)
        .unwrap();
    apply_to_world(app.world_mut(), &s).expect("ball is dead at PrePitch");
    let settled = run_until(&mut app, 5_000, |app| {
        let mut q = app.world_mut().query::<&Runner>();
        q.iter(app.world()).count() == 3
    });
    assert!(
        settled.is_some(),
        "three runner rigs must appear for bases loaded"
    );

    // Every runner knows who it is (scenario-manifested runners take the
    // batter-side fallback identity) and carries the four lettered quads.
    let world = app.world_mut();
    let runners: Vec<Entity> = world
        .query_filtered::<Entity, With<Runner>>()
        .iter(world)
        .collect();
    for rig in runners {
        let id = world
            .get::<PlayerIdentity>(rig)
            .expect("runner rig must carry PlayerIdentity");
        assert_eq!(id.team, Team::Away, "runners belong to the batting team");
        assert_eq!(count_quads(world, rig), 4, "runner must wear its jerseys");
    }
}

#[test]
fn skin_tones_dress_the_wired_rigs() {
    let mut app = headless_app();
    start_game(&mut app, KeyCode::Digit1);
    // Wait for glTF wiring + dressing (async asset load).
    let dressed = run_until(&mut app, 5_000, |app| {
        let world = app.world_mut();
        let mut q = world.query::<&breakneck_baseball::game::gear::DressedAs>();
        q.iter(world).count() > 0
    });
    assert!(
        dressed.is_some(),
        "at least one rig must dress after wiring"
    );
    // A dressed rig's skin meshes must not wear the shared base material.
    let world = app.world_mut();
    let base = world
        .resource::<breakneck_baseball::game::model_assets::RigAnimations>()
        .skin_material
        .clone();
    let mut rigs = world.query_filtered::<
        &breakneck_baseball::game::model_assets::RigSkinMeshes,
        With<breakneck_baseball::game::gear::DressedAs>,
    >();
    let skin_meshes: Vec<Entity> = rigs.iter(world).flat_map(|m| m.0.clone()).collect();
    assert!(!skin_meshes.is_empty());
    for mesh in skin_meshes {
        let mat = world
            .get::<MeshMaterial3d<StandardMaterial>>(mesh)
            .expect("skin mesh keeps its material component");
        assert_ne!(
            mat.0, base,
            "dressed skin must be a swatch clone, not the base"
        );
    }
}

#[test]
fn headwear_hides_the_baked_cap_and_mounts_gear() {
    let mut app = headless_app();
    start_game(&mut app, KeyCode::Digit1);
    // VEGA (home slot 0 → the pitcher in the top 1st) wears a Helmet in
    // data/players.ron: his baked cap must hide and a helmet prop appear.
    // Gate on the PITCHER RIG specifically being dressed — rigs wire
    // asynchronously per-entity, so "any gear exists" would race.
    let done = run_until(&mut app, 5_000, |app| {
        let world = app.world_mut();
        let mut q = world.query_filtered::<
            &breakneck_baseball::game::gear::RigGear,
            With<breakneck_baseball::game::player::Pitcher>,
        >();
        q.iter(world)
            .next()
            .map(|g| !g.0.is_empty())
            .unwrap_or(false)
    });
    assert!(
        done.is_some(),
        "the helmeted pitcher must dress with gear props"
    );

    let world = app.world_mut();
    // Find the pitcher rig (identity Home/0 = VEGA per data/players.ron).
    let mut pitchers = world.query_filtered::<(
        &breakneck_baseball::game::model_assets::RigCapMeshes,
        &breakneck_baseball::game::gear::RigGear,
    ), With<breakneck_baseball::game::player::Pitcher>>();
    let (caps, gear) = pitchers.single(world);
    let cap_entities = caps.0.clone();
    let gear_entities = gear.0.clone();
    assert!(
        !gear_entities.is_empty(),
        "helmet wearer must own gear props"
    );
    for cap in cap_entities {
        assert_eq!(
            world.get::<Visibility>(cap).copied(),
            Some(Visibility::Hidden),
            "baked cap must hide under a helmet"
        );
    }
    // Spec §7: props are parented to the right bone entities — the helmet
    // must be a child of the pitcher rig's Head bone.
    let mut pitcher_bones = world.query_filtered::<
        &breakneck_baseball::game::model_assets::RigBones,
        With<breakneck_baseball::game::player::Pitcher>,
    >();
    let head = pitcher_bones.single(world).head;
    let on_head = gear_entities
        .iter()
        .any(|&p| world.get::<Parent>(p).map(|par| par.get()) == Some(head));
    assert!(on_head, "the helmet prop must hang off the Head bone");
}
