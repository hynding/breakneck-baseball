//! End-to-end: ground surface detail (graphics plan task 16). On Medium and
//! High the turf varies in shade and the batter's boxes pick up cleat marks
//! as the pitches mount; High also normal-maps the turf and clay. Low keeps
//! the plain park, and the cleat marks follow a mid-game tier change.

use crate::common;

use bevy::prelude::*;

use breakneck_baseball::game::ball::PitchEvent;
use breakneck_baseball::game::field::{
    CLEAT_STAGES, CleatMarks, CleatWear, GroundPlane, SurfaceDecal,
};
use breakneck_baseball::game::quality::{ActiveQuality, QualityTier};
use breakneck_baseball::game::settings::{GraphicsQuality, Settings};

use common::{headless_app, start_game};

fn set_tier(app: &mut App, tier: QualityTier) {
    app.insert_resource(ActiveQuality {
        setting: GraphicsQuality::Auto,
        tier,
        stepped: false,
    });
}

/// A stadium game started on `tier`.
fn game_on(tier: QualityTier) -> App {
    let mut app = headless_app();
    app.world_mut().resource_mut::<Settings>().graphics = match tier {
        QualityTier::Low => GraphicsQuality::Low,
        QualityTier::Medium => GraphicsQuality::Medium,
        QualityTier::High => GraphicsQuality::High,
    };
    set_tier(&mut app, tier);
    start_game(&mut app, KeyCode::Digit1);
    app.update();
    app
}

/// How many decals are shown, and the cleat marks' texture handles.
fn decals(app: &mut App) -> (usize, usize, Vec<Option<Handle<Image>>>) {
    let world = app.world_mut();
    let mut shown = 0;
    let mut total = 0;
    let mut cleats = Vec::new();
    let rows: Vec<(Visibility, Handle<StandardMaterial>, bool)> = world
        .query_filtered::<(
            &Visibility,
            &MeshMaterial3d<StandardMaterial>,
            Has<CleatMarks>,
        ), With<SurfaceDecal>>()
        .iter(world)
        .map(|(v, m, cleat)| (*v, m.0.clone(), cleat))
        .collect();
    let materials = world.resource::<Assets<StandardMaterial>>();
    for (visibility, material, cleat) in rows {
        total += 1;
        if visibility != Visibility::Hidden {
            shown += 1;
        }
        if cleat {
            let m = materials.get(&material).expect("a decal material");
            cleats.push(m.base_color_texture.clone());
        }
    }
    (total, shown, cleats)
}

/// The ground slab's mesh: whether it has vertex colours and tangents.
fn ground(app: &mut App) -> (bool, bool) {
    let world = app.world_mut();
    let handle = world
        .query_filtered::<&Mesh3d, With<GroundPlane>>()
        .single(world)
        .expect("one ground slab")
        .0
        .clone();
    let mesh = world
        .resource::<Assets<Mesh>>()
        .get(&handle)
        .expect("its mesh");
    (
        mesh.attribute(Mesh::ATTRIBUTE_COLOR).is_some(),
        mesh.attribute(Mesh::ATTRIBUTE_TANGENT).is_some(),
    )
}

/// How many meshes in the scene carry both vertex colours and tangents:
/// on High, the varied ground plus the worn clay frame and its four pits.
fn worn_and_normal_mapped(app: &mut App) -> usize {
    let world = app.world_mut();
    let handles: Vec<Handle<Mesh>> = world
        .query::<&Mesh3d>()
        .iter(world)
        .map(|m| m.0.clone())
        .collect();
    let meshes = world.resource::<Assets<Mesh>>();
    handles
        .iter()
        .filter_map(|h| meshes.get(h))
        .filter(|m| {
            m.attribute(Mesh::ATTRIBUTE_COLOR).is_some()
                && m.attribute(Mesh::ATTRIBUTE_TANGENT).is_some()
        })
        .count()
}

#[test]
fn low_keeps_the_plain_park() {
    let mut app = game_on(QualityTier::Low);
    assert_eq!(
        app.world().resource::<ActiveQuality>().tier,
        QualityTier::Low
    );
    let (total, shown, _) = decals(&mut app);
    assert_eq!(total, 2, "one per batter's box");
    assert_eq!(shown, 0);
    assert_eq!(ground(&mut app), (false, false));
}

#[test]
fn medium_varies_the_ground_without_normal_maps() {
    let mut app = game_on(QualityTier::Medium);
    let (_, shown, _) = decals(&mut app);
    assert_eq!(shown, 0, "the boxes wait for a pitch");
    assert_eq!(ground(&mut app), (true, false));
    assert_eq!(worn_and_normal_mapped(&mut app), 0);
}

/// High: the normal-mapped ground, and cleat marks that arrive with the
/// first pitch and deepen as the count mounts; then a step down to Low
/// hides every decal.
#[test]
fn high_wears_the_boxes_in_as_the_game_goes() {
    let mut app = game_on(QualityTier::High);
    assert_eq!(ground(&mut app), (true, true));
    assert!(
        worn_and_normal_mapped(&mut app) >= 6,
        "the ground, the worn clay band and its four pits"
    );
    assert_eq!(app.world().resource::<CleatWear>().pitches, 0);

    app.world_mut().write_message(PitchEvent {
        velocity: Vec3::new(0.0, 0.0, -38.0),
        spin: Vec3::ZERO,
    });
    app.update();
    app.update();
    assert_eq!(app.world().resource::<CleatWear>().pitches, 1);
    let (_, shown, first) = decals(&mut app);
    assert_eq!(shown, 2, "the boxes show their first marks");
    assert_eq!(first.len(), 2);

    app.world_mut().resource_mut::<CleatWear>().pitches = CLEAT_STAGES[2];
    app.update();
    let (_, _, later) = decals(&mut app);
    assert_ne!(first, later, "heavier wear later in the game");

    set_tier(&mut app, QualityTier::Low);
    app.update();
    let (_, shown, _) = decals(&mut app);
    assert_eq!(shown, 0, "a step down to Low hides them");
}
