//! Unit tests for [`super`] — the debug module.

use super::*;
use bevy_inspector_egui::inspector_egui_impls::InspectorEguiImpl;
use std::any::TypeId;

/// The Tune tab is only usable if every primitive leaf type has an
/// `InspectorEguiImpl` in the type registry — without it, every field
/// renders as an error label instead of an editable widget.
#[test]
fn debug_plugin_registers_inspector_widgets_for_primitives() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(bevy::state::app::StatesPlugin)
        // EguiPlugin's build wants shader/image assets that normally come
        // from the render stack; provide bare asset storage instead.
        .add_plugins(bevy::asset::AssetPlugin::default());
    app.init_asset::<bevy::render::render_resource::Shader>();
    app.init_asset::<bevy::image::Image>();
    // DefaultPlugins registers these in the real app; the inspector's
    // config plugin asserts on them, so the bare harness must too.
    app.register_type::<Entity>();
    app.register_type::<bevy::asset::Handle<bevy::render::mesh::Mesh>>();
    app.register_type::<bevy::asset::Handle<bevy::image::Image>>();
    app.register_type::<bevy::render::view::RenderLayers>();
    app.init_state::<crate::game::GameState>();
    app.add_plugins(DebugPlugin);

    let registry = app.world().resource::<AppTypeRegistry>().0.clone();
    let registry = registry.read();
    for (name, id) in [
        ("f32", TypeId::of::<f32>()),
        ("u32", TypeId::of::<u32>()),
        ("bool", TypeId::of::<bool>()),
    ] {
        assert!(
            registry.get_type_data::<InspectorEguiImpl>(id).is_some(),
            "{name} has no InspectorEguiImpl — the Tune tab would render \
             error labels instead of editable widgets"
        );
    }
}
