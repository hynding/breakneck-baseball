//! The embedded player.glb must load headless and build the shared
//! AnimationGraph — the gate every glTF rig depends on.

mod common;

use breakneck_baseball::game::model_assets::RigAnimations;

#[test]
fn embedded_model_loads_and_builds_graph() {
    let mut app = common::headless_app();
    let built = common::run_until(&mut app, 2_000, |app| {
        app.world().get_resource::<RigAnimations>().is_some()
    });
    assert!(
        built.is_some(),
        "RigAnimations never appeared — embedded player.glb failed to load or a clip is missing"
    );
}
