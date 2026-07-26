//! Playing-field geometry, spawned from the chosen [`FieldSpec`].
//!
//! Shared pieces (ground, bases, mound, lighting) are placed wherever the spec
//! says; the surroundings are dressed by the spec's [`Scenery`] routine —
//! a classic ballpark or a suburban front yard.
//!
//! **Standard field dimensions** (metric, matching real MLB proportions scaled
//! to Bevy world units where 1 unit ≈ 1 metre):
//!
//! | Feature                     | Real feet | Metres (≈) |
//! |-----------------------------|-----------|------------|
//! | Base-to-base                | 90 ft     | 27.43 m    |
//! | Home plate → pitcher's mound| 60.5 ft   | 18.44 m    |
//! | Home plate → centre-field   | 400 ft    | 121.9 m    |
//! | Foul lines (1B / 3B)        | 330 ft    | 100.6 m    |

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::math::Affine2;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy_rapier3d::prelude::*;

use crate::game::ai::hash01;
use crate::game::flow::{Phase, Play};
use crate::game::rules;
use crate::game::variant::{FieldSpec, Scenery};
use crate::game::{GameState, GameplayEntity};

// ── Distances in metres ───────────────────────────────────────────────────────
/// Distance between consecutive bases (90 ft).
pub const BASE_DISTANCE: f32 = 27.43;
/// Home plate → pitching rubber (60.5 ft).
pub const PITCH_DISTANCE: f32 = 18.44;
/// Half the base-path diagonal, used to place second base along the Z axis.
pub const HALF_DIAGONAL: f32 = BASE_DISTANCE * std::f32::consts::SQRT_2 / 2.0;
/// Ground-plane thickness for the static field collider.
const GROUND_HALF_DEPTH: f32 = 0.1;

// ── Field-object marker components ───────────────────────────────────────────
/// Marks the entire playing-surface ground plane.
#[derive(Component)]
pub struct GroundPlane;

/// Marks a base object: `Some(i)` is the (0-indexed) i-th base in running
/// order, `None` is home plate.
#[allow(dead_code)]
#[derive(Component)]
pub struct Base {
    pub index: Option<usize>,
}

/// Marks the pitcher's mound.
#[derive(Component)]
pub struct PitchersMound;

/// Marks one of the four foul-line poles.
#[derive(Component)]
pub struct FoulPole;

/// Marks an outfield-wall panel — collision partners for wall-carom effects.
#[derive(Component)]
pub struct OutfieldWall;

/// Marks a piece of the floating strike-zone box shown during the duel.
#[derive(Component)]
struct StrikeZoneOverlay;

// ── Procedural surfaces ───────────────────────────────────────────────────────
// Runtime-generated textures, no asset files (the same philosophy as the
// procedural audio and jerseys): mowing-striped grass and speckled infield
// dirt, per the groundskeeping notes in docs/BASEBALL.md.

/// Grass with alternating mow stripes and per-blade jitter.
fn grass_image() -> Image {
    const SIZE: usize = 64;
    const STRIPE: usize = 8;
    let mut data = vec![0u8; SIZE * SIZE * 4];
    for y in 0..SIZE {
        let light = (y / STRIPE).is_multiple_of(2);
        for x in 0..SIZE {
            let n = hash01(x as f32 * 12.9 + y as f32 * 78.2) * 14.0 - 7.0;
            let (r, g, b) = if light {
                (52.0, 142.0, 52.0)
            } else {
                (42.0, 122.0, 44.0)
            };
            let at = (y * SIZE + x) * 4;
            data[at] = (r + n) as u8;
            data[at + 1] = (g + n) as u8;
            data[at + 2] = (b + n) as u8;
            data[at + 3] = 255;
        }
    }
    tiling_image(SIZE as u32, data)
}

/// Infield dirt: warm clay with darker speckles.
fn dirt_image() -> Image {
    const SIZE: usize = 64;
    let mut data = vec![0u8; SIZE * SIZE * 4];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let seed = x as f32 * 31.7 + y as f32 * 57.3;
            let n = hash01(seed) * 24.0 - 12.0;
            let (r, g, b) = if hash01(seed * 1.7) > 0.93 {
                (150.0, 115.0, 82.0) // a pebble
            } else {
                (194.0, 153.0, 108.0)
            };
            let at = (y * SIZE + x) * 4;
            data[at] = (r + n) as u8;
            data[at + 1] = (g + n) as u8;
            data[at + 2] = (b + n) as u8;
            data[at + 3] = 255;
        }
    }
    tiling_image(SIZE as u32, data)
}

/// Wraps raw RGBA pixels in a repeat-sampled square texture.
fn tiling_image(size: u32, data: Vec<u8>) -> Image {
    let mut image = Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..default()
    });
    image
}

/// The pair of tiled surface materials every park is dressed with.
struct FieldSurfaces {
    grass: Handle<Image>,
    dirt: Handle<Image>,
}

impl FieldSurfaces {
    fn build(images: &mut Assets<Image>) -> Self {
        Self {
            grass: images.add(grass_image()),
            dirt: images.add(dirt_image()),
        }
    }

    /// A material tiling `texture` `repeats` times across a unit UV face.
    fn tiled(
        materials: &mut Assets<StandardMaterial>,
        texture: &Handle<Image>,
        repeats: f32,
    ) -> Handle<StandardMaterial> {
        materials.add(StandardMaterial {
            base_color_texture: Some(texture.clone()),
            uv_transform: Affine2::from_scale(Vec2::splat(repeats)),
            perceptual_roughness: 0.95,
            ..default()
        })
    }
}

// ── Plugin ────────────────────────────────────────────────────────────────────
pub struct FieldPlugin;

impl Plugin for FieldPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(crate::game::game_start(), spawn_field)
            .add_systems(
                Update,
                strike_zone_visibility.run_if(in_state(GameState::Playing)),
            );
    }
}

// ── Systems ───────────────────────────────────────────────────────────────────
#[allow(clippy::too_many_arguments)]
fn spawn_field(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    field: Res<FieldSpec>,
    theme: Res<crate::game::theme::Theme>,
) {
    // The sky is theme data like everything else — a bright day or a night
    // game. Matters most from the catcher's-eye duel camera, which looks up
    // past the wall into nothing but clear colour.
    commands.insert_resource(ClearColor(theme.sky));
    let surfaces = FieldSurfaces::build(&mut images);
    match field.scenery {
        Scenery::Stadium => {
            spawn_stadium_ground(&mut commands, &mut meshes, &mut materials, &surfaces);
            spawn_stadium_mound(
                &mut commands,
                &mut meshes,
                &mut materials,
                &surfaces,
                &field,
            );
            spawn_foul_poles(&mut commands, &mut meshes, &mut materials);
            spawn_outfield_wall(&mut commands, &mut meshes, &mut materials, &field);
        }
        Scenery::FrontYard => {
            spawn_front_yard(
                &mut commands,
                &mut meshes,
                &mut materials,
                &surfaces,
                &field,
            );
        }
    }
    spawn_bases(&mut commands, &mut meshes, &mut materials, &field);
    spawn_chalk_lines(&mut commands, &mut meshes, &mut materials, &field);
    spawn_strike_zone(&mut commands, &mut meshes, &mut materials);
    // The sun sits behind home plate in both parks so everything the
    // broadcast and duel cameras look at — players' backs, house fronts, the
    // outfield — is lit rather than silhouetted; ambient keeps shadow sides
    // readable up close.
    match field.scenery {
        Scenery::Stadium => spawn_lighting(
            &mut commands,
            std::f32::consts::PI - std::f32::consts::FRAC_PI_6,
            0.25,
        ),
        Scenery::FrontYard => spawn_lighting(
            &mut commands,
            std::f32::consts::PI + std::f32::consts::FRAC_PI_6,
            0.35,
        ),
    }
}

/// The flat ground slab every scenery stands on (static collider for the
/// ball), dressed in the tiled mown-grass texture.
fn spawn_ground_slab(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    surfaces: &FieldSurfaces,
) {
    let half_size = 150.0_f32;
    commands.spawn((
        GroundPlane,
        GameplayEntity,
        Mesh3d(meshes.add(Cuboid::new(
            half_size * 2.0,
            GROUND_HALF_DEPTH * 2.0,
            half_size * 2.0,
        ))),
        // ~48 tiles across 300 m puts the mow stripes at ~0.8 m each.
        MeshMaterial3d(FieldSurfaces::tiled(materials, &surfaces.grass, 48.0)),
        Transform::from_xyz(0.0, -GROUND_HALF_DEPTH, 0.0),
        RigidBody::Fixed,
        Collider::cuboid(half_size, GROUND_HALF_DEPTH, half_size),
    ));
}

// ── Stadium scenery ───────────────────────────────────────────────────────────
/// The playing surface, layered per docs/BASEBALL.md's groundskeeping notes:
/// striped outfield grass, a dirt basepath diamond with a grass infield
/// inside it, and the 13 ft dirt cutouts at the bags and around home plate.
fn spawn_stadium_ground(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    surfaces: &FieldSurfaces,
) {
    spawn_ground_slab(commands, meshes, materials, surfaces);

    let dirt = FieldSurfaces::tiled(materials, &surfaces.dirt, 8.0);
    commands.spawn((
        GameplayEntity,
        Mesh3d(meshes.add(Cuboid::new(INFIELD_HALF * 2.0, 0.001, INFIELD_HALF * 2.0))),
        MeshMaterial3d(dirt.clone()),
        Transform {
            translation: Vec3::new(0.0, 0.001, HALF_DIAGONAL),
            rotation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_4),
            ..default()
        },
    ));

    // Rounded dirt cutouts at the corner bags (the ~13 ft sliding pits) and
    // the home-plate circle.
    let cutout = meshes.add(Cylinder::new(CUTOUT_RADIUS, 0.001));
    let corners = [
        Vec3::ZERO,
        Vec3::new(-HALF_DIAGONAL, 0.0, HALF_DIAGONAL),
        Vec3::new(0.0, 0.0, HALF_DIAGONAL * 2.0),
        Vec3::new(HALF_DIAGONAL, 0.0, HALF_DIAGONAL),
    ];
    for corner in corners {
        commands.spawn((
            GameplayEntity,
            Mesh3d(cutout.clone()),
            MeshMaterial3d(dirt.clone()),
            Transform::from_translation(corner + Vec3::Y * 0.0016),
        ));
    }

    // The grass interior of the diamond: dirt shows only as the basepath
    // band around it (plus the mound and cutouts layered above).
    let inner_half = INFIELD_HALF - BASEPATH_WIDTH;
    commands.spawn((
        GameplayEntity,
        Mesh3d(meshes.add(Cuboid::new(inner_half * 2.0, 0.001, inner_half * 2.0))),
        MeshMaterial3d(FieldSurfaces::tiled(materials, &surfaces.grass, 5.0)),
        Transform {
            translation: Vec3::new(0.0, 0.0022, HALF_DIAGONAL),
            rotation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_4),
            ..default()
        },
    ));
}

/// Half-size of the infield-dirt square *before* the 45° rotation
/// `spawn_stadium_ground` applies to turn it into the basepath diamond.
///
/// A square of half-size `H` has its corners at distance `H * √2` from its
/// centre; for the rotated diamond's corners to land exactly on home plate
/// and the three bases (each `HALF_DIAGONAL` from the diamond's centre — see
/// `spawn_bases`/`variant.rs`'s `base_positions`, per docs/BASEBALL.md) we
/// need `H * √2 == HALF_DIAGONAL`, i.e. `H == BASE_DISTANCE / 2`. This used
/// to be `BASE_DISTANCE / √2` (√2 too big), which overshot the bases by
/// ~40% and stranded the bags inside the "grass interior" square instead of
/// on the dirt basepath band — regression-tested by
/// `tests::infield_diamond_corners_align_with_bases`, which reads this
/// const directly.
const INFIELD_HALF: f32 = BASE_DISTANCE / 2.0;

/// Dirt-cutout radius at each bag and around home (~13 ft, docs/BASEBALL.md).
const CUTOUT_RADIUS: f32 = 3.96;
/// Width of the dirt basepath band framing the grass infield.
const BASEPATH_WIDTH: f32 = 4.0;

// ── Bases ─────────────────────────────────────────────────────────────────────
/// Home plate at the origin plus one bag per spec base position.
fn spawn_bases(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    field: &FieldSpec,
) {
    // Regulation bags (docs/BASEBALL.md): 18 in square since the 2023 rule
    // change, a real raised bag with a touch of glow so it pops against the
    // dirt; home plate is the flat 17 in slab.
    let base_material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        emissive: LinearRgba::rgb(0.12, 0.12, 0.12),
        perceptual_roughness: 0.6,
        ..default()
    });
    let home_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.94, 0.94, 0.92),
        emissive: LinearRgba::rgb(0.08, 0.08, 0.08),
        perceptual_roughness: 0.7,
        ..default()
    });
    let base_mesh = meshes.add(Cuboid::new(0.457, 0.09, 0.457));
    let home_mesh = meshes.add(Cuboid::new(PLATE_WIDTH, 0.02, PLATE_WIDTH));

    let mut spawn = |index: Option<usize>, pos: Vec3, y: f32, mesh: Handle<Mesh>, mat| {
        commands.spawn((
            Base { index },
            GameplayEntity,
            Mesh3d(mesh),
            MeshMaterial3d(mat),
            Transform::from_translation(pos + Vec3::Y * y),
            RigidBody::Fixed,
            Collider::cuboid(0.23, 0.045, 0.23),
        ));
    };

    spawn(None, Vec3::ZERO, 0.01, home_mesh, home_material);
    for (i, pos) in field.base_positions.iter().enumerate() {
        spawn(
            Some(i),
            *pos,
            0.045,
            base_mesh.clone(),
            base_material.clone(),
        );
    }
}

// ── Chalk lines ───────────────────────────────────────────────────────────────
/// Home plate's width (17 in, docs/BASEBALL.md) — shared by `spawn_bases`'s
/// plate slab and the batter's-box math below so the 6 in gap is measured
/// from the plate's *actual* modeled edge rather than a second, independently
/// duplicated literal.
const PLATE_WIDTH: f32 = 0.432;
const PLATE_HALF_WIDTH: f32 = PLATE_WIDTH / 2.0;

/// Chalk line width: rulebooks call for lines "not less than 2 in nor more
/// than 4 in" of lime/chalk/paint (Official Baseball Rules 2.01; batflipsports.com's
/// groundskeeping guide gives the same 2–4 in range, and foxvalleypaint.com/
/// baseballstandard.com note MLB crews commonly stripe foul lines at the wider
/// end, ~4 in). We use 3 in (0.076 m), the middle of that range — see
/// docs/BASEBALL.md.
const CHALK_WIDTH: f32 = 0.076;
/// Height chalk sits at: above every dirt/grass layer `spawn_stadium_ground`
/// paints (dirt basepath 0.001, cutouts 0.0016, grass interior 0.0022), so
/// lines never z-fight with the surfaces they're drawn on.
const CHALK_Y: f32 = 0.003;

/// Batter's box: 4 ft × 6 ft, long side toward the pitcher, drawn 6 in off
/// each side of the plate (docs/BASEBALL.md).
const BOX_HALF_WIDTH: f32 = 0.61; // 4 ft / 2, side-to-side of the plate
const BOX_HALF_LENGTH: f32 = 0.915; // 6 ft / 2, toward the pitcher/catcher
const BOX_PLATE_GAP: f32 = 0.152; // 6 in
/// Centre-line x-offset of each batter's box from home plate: the plate's
/// own half-width, plus the regulation gap, plus half the box (so its inner
/// edge sits exactly `BOX_PLATE_GAP` off the plate).
const BOX_CENTER_X: f32 = PLATE_HALF_WIDTH + BOX_PLATE_GAP + BOX_HALF_WIDTH;

/// Perpendicular distance from `p` to the infinite line through `a` and `b`
/// (both points in the ground's XZ plane, passed here as `Vec2(x, z)`).
///
/// Test-only: a geometry check on what `spawn_chalk_segment` paints, not
/// something the spawn code itself needs at runtime (it places quads by
/// direct translation/rotation math, not by testing points against a line).
#[cfg(test)]
fn distance_to_line(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let dir = (b - a).normalize_or_zero();
    if dir == Vec2::ZERO {
        return p.distance(a);
    }
    let offset = p - a;
    let along = dir * offset.dot(dir);
    (offset - along).length()
}

/// Whether ground point `p` sits on the *hollow* rectangular chalk outline
/// centred at `center` with half-extents `half` (x = side-to-side, y = toward
/// the pitcher) and line width `width` — i.e. within `width / 2` of one of
/// the four edges. A point deep in the box's interior, or outside it
/// entirely, is not "on" the outline: `spawn_batters_box` paints a box, not a
/// filled rectangle.
///
/// Test-only, same reasoning as `distance_to_line` above.
#[cfg(test)]
fn on_box_outline(p: Vec2, center: Vec2, half: Vec2, width: f32) -> bool {
    let local = (p - center).abs();
    let margin = width / 2.0;
    if local.x > half.x + margin || local.y > half.y + margin {
        return false;
    }
    let near_side_edge = (local.x - half.x).abs() <= margin;
    let near_end_edge = (local.y - half.y).abs() <= margin;
    near_side_edge || near_end_edge
}

/// One flat chalk quad lying on the ground: `size.x` along world X, `size.y`
/// along world Z, centred at `(translation.x, CHALK_Y, translation.z)`. Used
/// for the batter's-box edges, which are already axis-aligned and need no
/// rotation.
fn spawn_flat_chalk(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    chalk: &Handle<StandardMaterial>,
    size: Vec2,
    translation: Vec3,
) {
    commands.spawn((
        GameplayEntity,
        Mesh3d(meshes.add(Cuboid::new(size.x, 0.002, size.y))),
        MeshMaterial3d(chalk.clone()),
        Transform::from_xyz(translation.x, CHALK_Y, translation.z),
    ));
}

/// A straight chalk line from `from` to `to` (both y = 0, ground plane),
/// `width` metres wide — used for the foul lines, which run at whatever
/// angle the base positions dictate rather than along a world axis.
fn spawn_chalk_segment(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    chalk: &Handle<StandardMaterial>,
    from: Vec3,
    to: Vec3,
    width: f32,
) {
    let delta = to - from;
    let length = Vec2::new(delta.x, delta.z).length();
    if length < f32::EPSILON {
        return;
    }
    // Same yaw convention as `spawn_outfield_wall`'s chord panels: maps local
    // +X onto the world-space direction of `delta`.
    let yaw = (-delta.z).atan2(delta.x);
    commands.spawn((
        GameplayEntity,
        Mesh3d(meshes.add(Cuboid::new(length, 0.002, width))),
        MeshMaterial3d(chalk.clone()),
        Transform {
            translation: Vec3::new((from.x + to.x) / 2.0, CHALK_Y, (from.z + to.z) / 2.0),
            rotation: Quat::from_rotation_y(yaw),
            ..default()
        },
    ));
}

/// One batter's-box outline, `side_sign` +1 for the box at +X, -1 for the
/// mirrored box at -X — four separate edges (not a filled rectangle) per
/// `on_box_outline`'s membership test above.
fn spawn_batters_box(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    chalk: &Handle<StandardMaterial>,
    side_sign: f32,
) {
    let inner_x = side_sign * (BOX_CENTER_X - BOX_HALF_WIDTH);
    let outer_x = side_sign * (BOX_CENTER_X + BOX_HALF_WIDTH);
    let long_span = BOX_HALF_LENGTH * 2.0 + CHALK_WIDTH; // overlap the corners
    for x in [inner_x, outer_x] {
        spawn_flat_chalk(
            commands,
            meshes,
            chalk,
            Vec2::new(CHALK_WIDTH, long_span),
            Vec3::new(x, 0.0, 0.0),
        );
    }
    let short_span = (outer_x - inner_x).abs() + CHALK_WIDTH;
    for z in [-BOX_HALF_LENGTH, BOX_HALF_LENGTH] {
        spawn_flat_chalk(
            commands,
            meshes,
            chalk,
            Vec2::new(short_span, CHALK_WIDTH),
            Vec3::new(side_sign * BOX_CENTER_X, 0.0, z),
        );
    }
}

/// The foul line running from home plate through the base at `base_index`
/// and out to the fence in that exact direction (`rules::fence_at`) — the
/// same function that places the outfield wall, so the chalk, the wall, and
/// the home-run ruling all agree on where fair territory ends.
fn spawn_foul_line(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    chalk: &Handle<StandardMaterial>,
    field: &FieldSpec,
    base_index: usize,
) {
    let base = field.base_positions[base_index];
    let dir = Vec3::new(base.x, 0.0, base.z).normalize_or_zero();
    if dir == Vec3::ZERO {
        return;
    }
    let end = dir * rules::fence_at(dir, field);
    spawn_chalk_segment(commands, meshes, chalk, Vec3::ZERO, end, CHALK_WIDTH);
}

/// Two batter's-box outlines flanking the plate and the foul lines from home
/// through first and third base out to the fence, per docs/BASEBALL.md's
/// groundskeeping notes.
///
/// These are geometry — thin flat quads lying on the ground — not a texture
/// layer, unlike the mow-striped grass and speckled dirt above. Those
/// textures tile every few metres (`FieldSurfaces::tiled`, e.g. 48 repeats
/// across the 300 m ground slab), which is right for a repeating pattern but
/// wrong for a single straight line that must land at one exact world
/// position over 100+ m of fence distance: painting that into a small tiling
/// texture would repeat the line every tile instead of drawing it once. Flat
/// quads are the same technique `spawn_front_yard` already uses for its
/// street markings, just applied to the plate area and reused across
/// sceneries.
fn spawn_chalk_lines(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    field: &FieldSpec,
) {
    let chalk = materials.add(StandardMaterial {
        base_color: Color::srgb(0.96, 0.96, 0.94),
        emissive: LinearRgba::rgb(0.05, 0.05, 0.05),
        perceptual_roughness: 0.85,
        ..default()
    });
    for side_sign in [1.0_f32, -1.0] {
        spawn_batters_box(commands, meshes, &chalk, side_sign);
    }
    for base_index in [0, field.base_count() - 1] {
        spawn_foul_line(commands, meshes, &chalk, field, base_index);
    }
}

// ── Pitcher's mound ───────────────────────────────────────────────────────────
/// Regulation mound per docs/BASEBALL.md: 18 ft diameter, 10 in high, with a
/// wider low skirt approximating the 1-in-per-foot slope, and the white
/// 24 in × 6 in pitching rubber on the table.
fn spawn_stadium_mound(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    surfaces: &FieldSurfaces,
    field: &FieldSpec,
) {
    let dirt = FieldSurfaces::tiled(materials, &surfaces.dirt, 3.0);
    // The sloped skirt: a broad, shallow ring under the mound proper.
    commands.spawn((
        GameplayEntity,
        Mesh3d(meshes.add(Cylinder::new(3.6, 0.1))),
        MeshMaterial3d(dirt.clone()),
        Transform::from_xyz(0.0, 0.05, field.pitch_distance),
    ));
    commands.spawn((
        PitchersMound,
        GameplayEntity,
        Mesh3d(meshes.add(Cylinder::new(2.74, 0.25))), // 9 ft radius, 10 in high
        MeshMaterial3d(dirt),
        Transform::from_xyz(0.0, 0.125, field.pitch_distance),
        RigidBody::Fixed,
        Collider::cylinder(0.125, 2.74),
    ));
    // The rubber, proud of the table.
    commands.spawn((
        GameplayEntity,
        Mesh3d(meshes.add(Cuboid::new(0.61, 0.02, 0.152))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.95, 0.95, 0.93),
            perceptual_roughness: 0.7,
            ..default()
        })),
        Transform::from_xyz(0.0, 0.26, field.pitch_distance),
    ));
}

// ── Front-yard scenery ────────────────────────────────────────────────────────
/// Suburban lot: the batter hits from the lawn out across the street. All
/// dressing is visual-only (no colliders) so the analytic outcomes and the
/// ball's flight are never blocked; only the ground and the pitching mat are
/// physical.
fn spawn_front_yard(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    surfaces: &FieldSurfaces,
    field: &FieldSpec,
) {
    // A well-kept suburban lawn gets the same mow stripes as the stadium.
    spawn_ground_slab(commands, meshes, materials, surfaces);

    // A rubber pitching mat instead of a mound.
    commands.spawn((
        PitchersMound,
        GameplayEntity,
        Mesh3d(meshes.add(Cuboid::new(0.8, 0.04, 0.8))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.25, 0.25, 0.28),
            perceptual_roughness: 1.0,
            ..default()
        })),
        Transform::from_xyz(0.0, 0.02, field.pitch_distance),
        RigidBody::Fixed,
        Collider::cuboid(0.4, 0.02, 0.4),
    ));

    let mut flat = |size: Vec2, pos: Vec3, color: Color| {
        commands.spawn((
            GameplayEntity,
            Mesh3d(meshes.add(Cuboid::new(size.x, 0.004, size.y))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: color,
                perceptual_roughness: 1.0,
                ..default()
            })),
            Transform::from_translation(pos),
        ));
    };

    // The street runs the whole block, flanked by sidewalks.
    let asphalt = Color::srgb(0.32, 0.32, 0.34);
    let concrete = Color::srgb(0.62, 0.62, 0.60);
    flat(Vec2::new(300.0, 8.0), Vec3::new(0.0, 0.002, 26.0), asphalt);
    flat(
        Vec2::new(300.0, 0.3),
        Vec3::new(0.0, 0.004, 26.0),
        Color::srgb(0.85, 0.75, 0.2), // painted centre line
    );
    flat(Vec2::new(300.0, 2.0), Vec3::new(0.0, 0.002, 21.0), concrete);
    flat(Vec2::new(300.0, 2.0), Vec3::new(0.0, 0.002, 31.0), concrete);

    let mut block = |size: Vec3, pos: Vec3, color: Color| {
        commands.spawn((
            GameplayEntity,
            Mesh3d(meshes.add(Cuboid::new(size.x, size.y, size.z))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: color,
                perceptual_roughness: 0.9,
                ..default()
            })),
            Transform::from_translation(pos),
        ));
    };

    // Our house behind home plate, with a door and windows facing the yard.
    // Kept behind z = -12.5 so the broadcast camera (eye z = -12) never looks
    // through it.
    let siding = Color::srgb(0.78, 0.72, 0.58);
    let roof = Color::srgb(0.35, 0.22, 0.18);
    let trim = Color::srgb(0.30, 0.34, 0.42);
    block(
        Vec3::new(14.0, 5.0, 7.0),
        Vec3::new(0.0, 2.5, -16.5),
        siding,
    );
    block(Vec3::new(15.0, 1.2, 8.0), Vec3::new(0.0, 5.6, -16.5), roof);
    block(Vec3::new(1.4, 2.4, 0.2), Vec3::new(0.0, 1.2, -12.9), trim); // door
    block(Vec3::new(2.0, 1.4, 0.2), Vec3::new(-4.0, 2.6, -12.9), trim); // window
    block(Vec3::new(2.0, 1.4, 0.2), Vec3::new(4.0, 2.6, -12.9), trim); // window

    // The neighbours' houses across the street — clear those for a home run.
    let neighbour = Color::srgb(0.62, 0.68, 0.74);
    for x in [-22.0, 0.0, 22.0] {
        block(
            Vec3::new(14.0, 5.5, 7.0),
            Vec3::new(x, 2.75, 44.0),
            neighbour,
        );
        block(Vec3::new(15.0, 1.2, 8.0), Vec3::new(x, 6.2, 44.0), roof);
    }

    // Hedges along the lot lines.
    let hedge = Color::srgb(0.13, 0.35, 0.13);
    block(Vec3::new(0.8, 1.0, 16.0), Vec3::new(16.0, 0.5, 10.0), hedge);
    block(
        Vec3::new(0.8, 1.0, 16.0),
        Vec3::new(-16.0, 0.5, 10.0),
        hedge,
    );
}

// ── Strike-zone overlay ───────────────────────────────────────────────────────
/// A floating box over the plate showing exactly the zone the umpire calls
/// ([`rules::ZONE_HALF_WIDTH`] / `ZONE_LOW..ZONE_HIGH`) — the catcher's-eye
/// duel view's aiming aid. Visible only during the duel (see
/// [`strike_zone_visibility`]); no colliders, the ball flies through it.
fn spawn_strike_zone(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
) {
    let width = rules::ZONE_HALF_WIDTH * 2.0;
    let height = rules::ZONE_HIGH - rules::ZONE_LOW;
    let mid_y = (rules::ZONE_HIGH + rules::ZONE_LOW) / 2.0;
    let mut translucent = |color: Color| {
        materials.add(StandardMaterial {
            base_color: color,
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            cull_mode: None,
            ..default()
        })
    };
    let fill = translucent(Color::srgba(1.0, 1.0, 1.0, 0.07));
    let frame = translucent(Color::srgba(1.0, 1.0, 1.0, 0.4));

    let mut part = |size: Vec3, pos: Vec3, mat: &Handle<StandardMaterial>| {
        commands.spawn((
            StrikeZoneOverlay,
            GameplayEntity,
            Mesh3d(meshes.add(Cuboid::new(size.x, size.y, size.z))),
            MeshMaterial3d(mat.clone()),
            Transform::from_translation(pos),
        ));
    };

    part(
        Vec3::new(width, height, 0.006),
        Vec3::new(0.0, mid_y, 0.0),
        &fill,
    );
    let bar = 0.02;
    for y in [rules::ZONE_LOW, rules::ZONE_HIGH] {
        part(
            Vec3::new(width + bar, bar, 0.008),
            Vec3::new(0.0, y, 0.0),
            &frame,
        );
    }
    for x in [-rules::ZONE_HALF_WIDTH, rules::ZONE_HALF_WIDTH] {
        part(
            Vec3::new(bar, height + bar, 0.008),
            Vec3::new(x, mid_y, 0.0),
            &frame,
        );
    }
}

/// The zone box belongs to the duel: shown while a pitch is coming, hidden
/// the moment the ball is in play.
fn strike_zone_visibility(
    play: Res<Play>,
    mut overlay: Query<&mut Visibility, With<StrikeZoneOverlay>>,
) {
    let visible = matches!(play.phase, Phase::PrePitch | Phase::WindUp | Phase::Pitch);
    for mut visibility in &mut overlay {
        let desired = if visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != desired {
            *visibility = desired;
        }
    }
}

// ── Foul poles ────────────────────────────────────────────────────────────────
fn spawn_foul_poles(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
) {
    let pole_material = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.84, 0.0), // gold
        metallic: 0.8,
        perceptual_roughness: 0.2,
        ..default()
    });

    let foul_line_distance = 100.6_f32; // ≈ 330 ft

    for sign in [-1.0_f32, 1.0_f32] {
        commands.spawn((
            FoulPole,
            GameplayEntity,
            Mesh3d(meshes.add(Cylinder::new(0.05, 15.0))),
            MeshMaterial3d(pole_material.clone()),
            Transform::from_xyz(sign * foul_line_distance, 7.5, foul_line_distance),
            RigidBody::Fixed,
            Collider::cylinder(7.5, 0.05),
        ));
    }
}

// ── Outfield wall ─────────────────────────────────────────────────────────────
/// Height of the outfield wall (m).
pub const WALL_HEIGHT: f32 = 3.0;

/// A curved wall of flat panels spanning the fair-territory arc, standing
/// exactly on the spec's home-run fence ([`rules::fence_at`] interpolates the
/// foul-line distance out to straightaway centre). Each panel is a fixed
/// collider, so live balls carom off it — a ball ruled a home run at contact
/// clears it in the air, and everything else plays off the wall.
fn spawn_outfield_wall(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    field: &FieldSpec,
) {
    let wall_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.13, 0.30, 0.55), // padded outfield-wall blue
        perceptual_roughness: 0.9,
        // A little self-illumination so the shadowed side still reads as a wall
        // rather than a black void.
        emissive: LinearRgba::rgb(0.02, 0.05, 0.10),
        ..default()
    });

    let thickness = 0.4_f32;
    let panels = 24;
    let span = field.fair_half_angle * 2.0;
    // A point on the rules fence at angle `theta` off the centre-field axis.
    let fence_point = |theta: f32| {
        let dir = Vec3::new(theta.sin(), 0.0, theta.cos());
        dir * rules::fence_at(dir, field)
    };

    // The wall is the polyline through the fence points: each panel is the
    // chord between neighbours, so visual, physical, and home-run fences are
    // one and the same surface.
    for i in 0..panels {
        let t0 = -span / 2.0 + span * i as f32 / panels as f32;
        let t1 = -span / 2.0 + span * (i + 1) as f32 / panels as f32;
        let (p0, p1) = (fence_point(t0), fence_point(t1));
        let mid = (p0 + p1) / 2.0 + Vec3::Y * (WALL_HEIGHT / 2.0);
        let chord = p1 - p0;
        // Slightly over the chord so neighbouring panels overlap (no gaps).
        let width = chord.length() * 1.02;
        // Yaw that maps the panel's local +X onto the chord direction.
        let yaw = (-chord.z).atan2(chord.x);

        commands.spawn((
            OutfieldWall,
            GameplayEntity,
            Mesh3d(meshes.add(Cuboid::new(width, WALL_HEIGHT, thickness))),
            MeshMaterial3d(wall_material.clone()),
            Transform {
                translation: mid,
                rotation: Quat::from_rotation_y(yaw),
                ..default()
            },
            RigidBody::Fixed,
            Collider::cuboid(width / 2.0, WALL_HEIGHT / 2.0, thickness / 2.0),
            // Matches the ball's own restitution so its Min combine rule
            // keeps a lively carom instead of a dead drop.
            Restitution {
                coefficient: 0.55,
                combine_rule: CoefficientCombineRule::Min,
            },
        ));
    }
}

// ── Lighting ──────────────────────────────────────────────────────────────────
/// Sun illuminance, matched to `AmbientLight::brightness`'s physical lux-like
/// units (Bevy 0.15; `AmbientLight::default()` is 80.0, nowhere near a
/// plausible "sun") — `ambient_fraction` below is a fraction of *this*, not
/// an absolute brightness.
const SUN_ILLUMINANCE: f32 = 50_000.0;

/// Sunlight angled to cast shadows, with the azimuth (`yaw`) chosen per
/// scenery, plus an ambient fill so shadows aren't pitch-black. `ambient_fraction`
/// is relative to [`SUN_ILLUMINANCE`] (e.g. 0.25 = a quarter of the sun's
/// illuminance) rather than an absolute brightness — passing a raw lux value
/// here (as opposed to a 0..1-ish fraction) would make every unlit surface
/// render essentially black next to a 50,000 lux sun, which is exactly the
/// regression this constant documents: `AmbientLight::brightness` changed
/// from a small relative multiplier to absolute lux in Bevy 0.14, and a
/// leftover pre-migration value (~0.25) is indistinguishable from "no
/// ambient at all" at this scale — invisible from a distance, but glaring
/// once the duel camera sits close enough to see a shadowed cube face.
fn spawn_lighting(commands: &mut Commands, yaw: f32, ambient_fraction: f32) {
    commands.spawn((
        GameplayEntity,
        DirectionalLight {
            illuminance: SUN_ILLUMINANCE,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(
            EulerRot::YXZ,
            yaw,
            -std::f32::consts::FRAC_PI_4,
            0.0,
        )),
    ));

    commands.insert_resource(AmbientLight {
        color: Color::WHITE,
        brightness: ambient_fraction * SUN_ILLUMINANCE,
    });
}

// ── Tests ─────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::variant::VariantId;

    /// Mirrors the rotation + translation `spawn_stadium_ground` applies to
    /// the infield-dirt cuboid (`Transform::from_rotation_y(FRAC_PI_4)`
    /// around a centre at `(0, HALF_DIAGONAL)`), so the diamond's *actual*
    /// world-space corners — not just its nominal half-size — can be checked
    /// against the true base positions.
    fn diamond_corner(sign_x: f32, sign_z: f32, half_size: f32) -> Vec3 {
        let center = Vec3::new(0.0, 0.0, HALF_DIAGONAL);
        let rotation = Quat::from_rotation_y(std::f32::consts::FRAC_PI_4);
        center + rotation * Vec3::new(sign_x * half_size, 0.0, sign_z * half_size)
    }

    /// The infield dirt diamond's four corners must land exactly on home
    /// plate and the three bases, so the dirt basepath band runs along the
    /// real baselines and every bag sits centered on it (docs/BASEBALL.md's
    /// groundskeeping notes). Regression test for a bug where `INFIELD_HALF`
    /// was `BASE_DISTANCE / √2` — √2 too large — which overshot the bases by
    /// ~40% and left the bags (and home plate's own dirt cutout, painted
    /// underneath the grass-interior layer) stranded inside the diamond's
    /// grass interior instead of on its dirt corners. Reads `INFIELD_HALF`
    /// directly — the same const `spawn_stadium_ground` builds the mesh
    /// from — so a regression to the old formula fails this test.
    #[test]
    fn infield_diamond_corners_align_with_bases() {
        let eps = 0.01;

        let home = diamond_corner(1.0, -1.0, INFIELD_HALF);
        assert!(home.distance(Vec3::ZERO) < eps, "home at {home:?}");

        let first = diamond_corner(-1.0, -1.0, INFIELD_HALF);
        let want_first = Vec3::new(-HALF_DIAGONAL, 0.0, HALF_DIAGONAL);
        assert!(
            first.distance(want_first) < eps,
            "first at {first:?}, want {want_first:?}"
        );

        let second = diamond_corner(-1.0, 1.0, INFIELD_HALF);
        let want_second = Vec3::new(0.0, 0.0, HALF_DIAGONAL * 2.0);
        assert!(
            second.distance(want_second) < eps,
            "second at {second:?}, want {want_second:?}"
        );

        let third = diamond_corner(1.0, 1.0, INFIELD_HALF);
        let want_third = Vec3::new(HALF_DIAGONAL, 0.0, HALF_DIAGONAL);
        assert!(
            third.distance(want_third) < eps,
            "third at {third:?}, want {want_third:?}"
        );
    }

    /// Regression for a units bug: `spawn_lighting`'s `ambient_fraction`
    /// call sites (0.25 Stadium, 0.35 FrontYard) were passed straight
    /// through as `AmbientLight::brightness` — fine under Bevy's pre-0.14
    /// small-multiplier semantics, but in 0.15 `brightness` is an absolute
    /// lux-like value (`AmbientLight::default()` is 80.0), so 0.25 was
    /// indistinguishable from "no ambient light at all" next to a 50,000 lux
    /// sun. Every scenery's resulting ambient must clear Bevy's own default
    /// fill (else it's *darker* than doing nothing) and stay a sane fraction
    /// of the sun rather than approaching or exceeding it.
    #[test]
    fn ambient_fraction_scales_with_sun_illuminance_not_raw() {
        for ambient_fraction in [0.25_f32, 0.35] {
            let brightness = ambient_fraction * SUN_ILLUMINANCE;
            assert!(
                brightness > AmbientLight::default().brightness,
                "ambient {brightness} lux is dimmer than Bevy's own default ({}) — \
                 shadowed surfaces will render darker than out-of-the-box Bevy",
                AmbientLight::default().brightness
            );
            assert!(
                brightness < SUN_ILLUMINANCE,
                "ambient {brightness} lux should stay a fraction of the sun ({SUN_ILLUMINANCE}), \
                 not wash shadows out entirely"
            );
        }
    }

    /// Basic sanity check on `distance_to_line`'s geometry, independent of
    /// any field const: zero exactly on the line, and the expected
    /// perpendicular distance off it.
    #[test]
    fn distance_to_line_matches_geometry() {
        let a = Vec2::ZERO;
        let b = Vec2::new(10.0, 10.0);
        assert!(distance_to_line(Vec2::new(5.0, 5.0), a, b) < 1e-5);
        let want = 5.0 / std::f32::consts::SQRT_2;
        assert!((distance_to_line(Vec2::new(0.0, 5.0), a, b) - want).abs() < 1e-4);
    }

    /// `on_box_outline` must flag a point on either the inner (plate-side) or
    /// outer edge as "on" the outline, but reject both the box's own deep
    /// interior and home plate itself — the chalk paints a hollow rectangle,
    /// not a filled one, per `spawn_batters_box`.
    #[test]
    fn box_outline_flags_edges_not_interior_or_plate() {
        let center = Vec2::new(BOX_CENTER_X, 0.0);
        let half = Vec2::new(BOX_HALF_WIDTH, BOX_HALF_LENGTH);

        let inner_edge = Vec2::new(BOX_CENTER_X - BOX_HALF_WIDTH, 0.0);
        assert!(on_box_outline(inner_edge, center, half, CHALK_WIDTH));
        let outer_edge = Vec2::new(BOX_CENTER_X + BOX_HALF_WIDTH, 0.0);
        assert!(on_box_outline(outer_edge, center, half, CHALK_WIDTH));
        let front_edge = Vec2::new(BOX_CENTER_X, BOX_HALF_LENGTH);
        assert!(on_box_outline(front_edge, center, half, CHALK_WIDTH));

        assert!(
            !on_box_outline(center, center, half, CHALK_WIDTH),
            "the box's own centre should read as interior, not outline"
        );
        assert!(
            !on_box_outline(Vec2::ZERO, center, half, CHALK_WIDTH),
            "home plate itself must sit clear of the box outline"
        );
    }

    /// Regression guard: the batter's box must actually clear the plate by
    /// the regulation 6 in gap (docs/BASEBALL.md), reading `PLATE_HALF_WIDTH`
    /// and `BOX_PLATE_GAP` directly so a future edit that shrinks either
    /// can't silently overlap the box onto the plate.
    #[test]
    fn batters_box_clears_the_plate() {
        let inner_edge = BOX_CENTER_X - BOX_HALF_WIDTH;
        assert!((inner_edge - (PLATE_HALF_WIDTH + BOX_PLATE_GAP)).abs() < 1e-6);
        assert!(inner_edge > PLATE_HALF_WIDTH);
    }

    /// The foul lines `spawn_foul_line` paints must run exactly through the
    /// real base positions on their way from home to the fence — derived
    /// from `FieldSpec::base_positions`, not a hardcoded 45°, so this must
    /// hold for both variants (Standard's diamond and FrontYard's lawn, which
    /// uses a different `fair_half_angle` and base layout entirely).
    #[test]
    fn foul_lines_pass_through_first_and_third_base() {
        for variant in [VariantId::Standard, VariantId::FrontYard] {
            let field = variant.field();
            for &base_index in &[0, field.base_count() - 1] {
                let base = field.base_positions[base_index];
                let base_xz = Vec2::new(base.x, base.z);
                let dir = Vec3::new(base.x, 0.0, base.z).normalize_or_zero();
                assert!(dir != Vec3::ZERO, "{variant:?} base {base_index} at origin");
                let end = dir * rules::fence_at(dir, &field);
                let dist = distance_to_line(base_xz, Vec2::ZERO, Vec2::new(end.x, end.z));
                assert!(
                    dist < 0.01,
                    "{variant:?} base {base_index} at {base:?} is {dist} m off its foul line"
                );
            }
        }
    }
}
