//! Every person on the field spawns as a glTF SceneRoot rig whose skeleton
//! instantiates headless (AnimationPlayer present under each root).

mod common;

use bevy::animation::AnimationPlayer;
use bevy::input::keyboard::KeyCode;
use bevy::prelude::*;
use breakneck_baseball::game::player::GltfRig;

#[test]
fn rigs_spawn_gltf_scenes_headless() {
    let mut app = common::headless_app();
    common::start_game(&mut app, KeyCode::Digit2);

    // 1 pitcher + 9 fielders + batter + umpires all use the glTF model.
    let roots = app
        .world_mut()
        .query_filtered::<Entity, With<GltfRig>>()
        .iter(app.world())
        .count();
    assert!(roots >= 11, "expected all people as glTF rigs, got {roots}");

    // Scenes instantiate async — run until every root grew a skeleton.
    let wired = common::run_until(&mut app, 4_000, |app| {
        let players = app
            .world_mut()
            .query::<&AnimationPlayer>()
            .iter(app.world())
            .count();
        players >= roots
    });
    assert!(wired.is_some(), "glTF scenes never instantiated skeletons");
}

use breakneck_baseball::game::model_assets::{RigBones, RigPlayer};

#[test]
fn rigs_wire_graph_and_bones() {
    let mut app = common::headless_app();
    common::start_game(&mut app, KeyCode::Digit2);
    let wired = common::run_until(&mut app, 4_000, |app| {
        let world = app.world_mut();
        let total = world
            .query_filtered::<(), With<GltfRig>>()
            .iter(world)
            .count();
        let done = world
            .query_filtered::<(), (With<GltfRig>, With<RigPlayer>, With<RigBones>)>()
            .iter(world)
            .count();
        total > 0 && done == total
    });
    assert!(
        wired.is_some(),
        "some glTF rigs never wired (skeleton or named bones missing)"
    );
}

use breakneck_baseball::game::animation::AnimClip;
use breakneck_baseball::game::player::CatcherRole;

#[test]
fn catcher_crouch_reaches_the_graph() {
    let mut app = common::headless_app();
    common::start_game(&mut app, KeyCode::Digit2);
    // The duel starts immediately; catcher_crouch inserts CatcherCrouch,
    // and the driver must forward it to the skeleton's AnimationPlayer.
    let animated = common::run_until(&mut app, 4_000, |app| {
        let world = app.world_mut();
        world
            .query_filtered::<&RigPlayer, With<CatcherRole>>()
            .iter(world)
            .any(|rig| rig.current == Some(AnimClip::CatcherCrouch))
    });
    assert!(
        animated.is_some(),
        "driver never started CatcherCrouch on the catcher's skeleton"
    );
}

use breakneck_baseball::game::jersey::JerseyQuad;
use breakneck_baseball::game::model_assets::GltfJerseyMesh;

#[test]
fn gltf_rigs_recolor_and_mount_jerseys() {
    let mut app = common::headless_app();
    common::start_game(&mut app, KeyCode::Digit2);
    common::run_until(&mut app, 4_000, |app| {
        let world = app.world_mut();
        let total = world
            .query_filtered::<(), With<GltfRig>>()
            .iter(world)
            .count();
        let done = world
            .query_filtered::<(), (With<GltfRig>, With<RigPlayer>)>()
            .iter(world)
            .count();
        total > 0 && done == total
    })
    .expect("rigs wired");

    // Recolour reached the skinned meshes: at least one jersey mesh exists,
    // and defense vs batter wear different material handles.
    let world = app.world_mut();
    let mut mats = std::collections::HashMap::new();
    let mut q = world.query::<(&GltfJerseyMesh, &MeshMaterial3d<StandardMaterial>)>();
    for (tag, mat) in q.iter(world) {
        // RigUnit gains a Debug derive in this task so it can key the map.
        mats.entry(format!("{:?}", tag.unit))
            .or_insert_with(Vec::new)
            .push(mat.0.clone());
    }
    assert!(
        mats.len() >= 2,
        "expected defense and batter jersey meshes, got {mats:?}"
    );

    // Jersey lettering rides bones now: every quad's parent is a named bone.
    let mut parents = world.query::<(&JerseyQuad, &Parent)>();
    let mut names = world.query::<&Name>();
    let quad_parents: Vec<Entity> = parents.iter(world).map(|(_, p)| p.get()).collect();
    assert!(!quad_parents.is_empty());
    for parent in quad_parents {
        let name = names
            .get(world, parent)
            .expect("quad parent must be a named bone");
        assert!(
            matches!(name.as_str(), "Spine" | "UpperArm.L" | "UpperArm.R"),
            "quad mounted on {name} — expected a contract bone"
        );
    }
}

use breakneck_baseball::game::model_assets::GltfPart;
use breakneck_baseball::game::player::RigUnit;
use breakneck_baseball::game::theme::ThemeId;

/// Regression for the bug where `GltfTeamMaterials` was baked once against
/// whatever theme happened to be active the instant the Gltf finished
/// loading, and never re-tinted afterwards — cycling themes on the menu
/// left glTF rigs permanently in Daylight Classic's colours. Cycle to
/// Midnight Neon on the menu *before* starting, then assert the fielding
/// team's baked jersey material actually carries Midnight Neon's colour,
/// not Daylight Classic's.
#[test]
fn gltf_team_tints_follow_a_theme_cycled_before_kickoff() {
    let mut app = common::headless_app();
    common::tap_key(&mut app, KeyCode::KeyT); // Daylight Classic -> Midnight Neon
    common::start_game(&mut app, KeyCode::Digit2);
    common::run_until(&mut app, 4_000, |app| {
        let world = app.world_mut();
        let total = world
            .query_filtered::<(), With<GltfRig>>()
            .iter(world)
            .count();
        let done = world
            .query_filtered::<(), (With<GltfRig>, With<RigPlayer>)>()
            .iter(world)
            .count();
        total > 0 && done == total
    })
    .expect("rigs wired");

    let world = app.world_mut();
    // Top of the 1st: Away bats, Home fields — the tagged Defense jersey
    // mesh wears the fielding (Home) team's colour.
    let mut q = world.query::<(&GltfJerseyMesh, &MeshMaterial3d<StandardMaterial>)>();
    let defense_handle = q
        .iter(world)
        .find(|(tag, _)| tag.unit == RigUnit::Defense && tag.part == GltfPart::Jersey)
        .map(|(_, mat)| mat.0.clone())
        .expect("a defense jersey mesh must be tagged");

    let materials = world.resource::<Assets<StandardMaterial>>();
    let actual = materials
        .get(&defense_handle)
        .expect("tagged material must be loaded")
        .base_color;

    let daylight_home_jersey = ThemeId::DaylightClassic.build().home.jersey;
    let midnight_home_jersey = ThemeId::MidnightNeon.build().home.jersey;
    assert_eq!(
        actual, midnight_home_jersey,
        "defense jersey material still shows the boot-time theme, not the active one"
    );
    assert_ne!(
        actual, daylight_home_jersey,
        "defense jersey material never left Daylight Classic's colour"
    );
}
