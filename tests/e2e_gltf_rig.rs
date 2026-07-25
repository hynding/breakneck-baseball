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

use breakneck_baseball::game::model_assets::RigAnimations;
use breakneck_baseball::game::player::Batter;

/// The bat is skinned into the shared mesh, so every glTF rig instantiates a
/// bat-material submesh — catchers, fielders, umpires, and run-out rigs
/// (`RigUnit::Batter` but no `Batter` marker) all carry one. Only the plate
/// batter (the `Batter`-marked rig) should ever show it.
#[test]
fn bat_shows_only_on_the_batter() {
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

    let world = app.world_mut();
    let bat_material = world.resource::<RigAnimations>().bat_material.clone();

    let mut roots = world.query_filtered::<(Entity, Option<&Batter>), With<GltfRig>>();
    let roots: Vec<(Entity, bool)> = roots
        .iter(world)
        .map(|(e, batter)| (e, batter.is_some()))
        .collect();
    assert!(!roots.is_empty(), "expected wired glTF rigs to exist");

    let mut children_q = world.query::<&Children>();
    let mut mats_q = world.query::<&MeshMaterial3d<StandardMaterial>>();

    for (root, is_batter) in roots {
        let mut bat_meshes = Vec::new();
        let mut stack = vec![root];
        while let Some(e) = stack.pop() {
            if let Ok(mat) = mats_q.get(world, e) {
                if mat.0 == bat_material {
                    bat_meshes.push(e);
                }
            }
            if let Ok(children) = children_q.get(world, e) {
                stack.extend(children.iter().copied());
            }
        }
        assert_eq!(
            bat_meshes.len(),
            1,
            "rig {root:?} (batter={is_batter}) expected exactly one bat submesh, got {bat_meshes:?}"
        );
        let bat_mesh = bat_meshes[0];
        let visibility = world
            .get::<Visibility>(bat_mesh)
            .expect("bat submesh must have a Visibility component");
        if is_batter {
            assert_eq!(
                *visibility,
                Visibility::Inherited,
                "batter's bat submesh must be visible"
            );
        } else {
            assert_eq!(
                *visibility,
                Visibility::Hidden,
                "non-batter rig {root:?}'s bat submesh must be hidden"
            );
        }
    }
}

/// Regression for the mirrored-lettering bug: `UpperArm.L` and `UpperArm.R`
/// share the *same* rest rotation in the model contract (only their
/// translation's X sign differs — confirmed against the exported glTF's
/// node rotations), so `mount_jerseys_on_bones` must give both shoulder
/// quads the *same* yaw. Mirroring the sign (as a naive left/right symmetry
/// assumption did) turned `ShoulderR`'s face 180° from where it belonged;
/// back-face culling then showed its reverse (mirrored) side to the close
/// duel camera instead of honestly hiding it — confirmed visually on both
/// wgpu/Metal and wasm/WebGL2 by isolating each mounted quad in turn. This
/// test can't render, so it encodes the fix at the level that matters: every
/// quad mounted on an `UpperArm.L` bone and every quad mounted on an
/// `UpperArm.R` bone must carry an identical local rotation.
#[test]
fn shoulder_jersey_quads_share_the_same_mount_yaw() {
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

    let world = app.world_mut();
    let mut quads = world.query::<(&JerseyQuad, &Transform, &Parent)>();
    let mut names = world.query::<&Name>();
    let entries: Vec<(Transform, Entity)> = quads
        .iter(world)
        .map(|(_, transform, parent)| (*transform, parent.get()))
        .collect();

    let mut left_rotations = Vec::new();
    let mut right_rotations = Vec::new();
    for (transform, parent) in entries {
        let Ok(name) = names.get(world, parent) else {
            continue;
        };
        match name.as_str() {
            "UpperArm.L" => left_rotations.push(transform.rotation),
            "UpperArm.R" => right_rotations.push(transform.rotation),
            _ => {}
        }
    }

    assert!(
        !left_rotations.is_empty() && !right_rotations.is_empty(),
        "expected shoulder quads mounted on both arms, got {} left, {} right",
        left_rotations.len(),
        right_rotations.len()
    );
    for &l in &left_rotations {
        for &r in &right_rotations {
            assert!(
                l.abs_diff_eq(r, 1e-5),
                "ShoulderL yaw {l:?} != ShoulderR yaw {r:?} — UpperArm.L/.R share a rest \
                 rotation, so mirroring the mount yaw turns one shoulder's face 180° from \
                 where it belongs"
            );
        }
    }
}
