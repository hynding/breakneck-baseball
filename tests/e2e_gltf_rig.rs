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
