//! The diamond itself: the top-level `spawn_field` system, home plate and
//! the bases, the batter's boxes and foul-line chalk, and the pitcher's
//! mound over stadium dirt.

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::game::GameplayEntity;
use crate::game::rules;
use crate::game::variant::{FieldSpec, Scenery};

use super::FieldSurfaces;

// ── Systems ───────────────────────────────────────────────────────────────────
#[allow(clippy::too_many_arguments)]
pub(super) fn spawn_field(
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
            super::stadium::spawn_stadium_ground(
                &mut commands,
                &mut meshes,
                &mut materials,
                &surfaces,
            );
            spawn_stadium_mound(
                &mut commands,
                &mut meshes,
                &mut materials,
                &surfaces,
                &field,
            );
            super::stadium::spawn_foul_poles(&mut commands, &mut meshes, &mut materials);
            super::stadium::spawn_outfield_wall(&mut commands, &mut meshes, &mut materials, &field);
        }
        Scenery::FrontYard => {
            super::stadium::spawn_front_yard(
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
    super::zone::spawn_strike_zone(&mut commands, &mut meshes, &mut materials, &theme);
    // The sun sits behind home plate in both parks so everything the
    // broadcast and duel cameras look at — players' backs, house fronts, the
    // outfield — is lit rather than silhouetted; ambient keeps shadow sides
    // readable up close.
    // `ambient_fraction` retuned for contrast (Task 20 polish sweep): the
    // physically-derived 0.25/0.35 read washed-out and pastel against the
    // 50,000 lux sun — deeper shadows read better than a technically
    // accurate flat fill. A taste call, not a units fix (see
    // `spawn_lighting`'s doc comment for the units scheme itself).
    match field.scenery {
        Scenery::Stadium => super::stadium::spawn_lighting(
            &mut commands,
            std::f32::consts::PI - std::f32::consts::FRAC_PI_6,
            0.15,
        ),
        Scenery::FrontYard => super::stadium::spawn_lighting(
            &mut commands,
            std::f32::consts::PI + std::f32::consts::FRAC_PI_6,
            0.20,
        ),
    }
}

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
    let base_mesh = meshes.add(Cuboid::new(BASE_SIZE, 0.09, BASE_SIZE));
    let home_mesh = meshes.add(Cuboid::new(PLATE_WIDTH, 0.02, PLATE_WIDTH));

    let mut spawn = |index: Option<usize>, pos: Vec3, y: f32, mesh: Handle<Mesh>, mat| {
        commands.spawn((
            super::Base { index },
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
///
/// `pub(super)`: also the depth of `zone.rs`'s strike-zone prism (the
/// rulebook zone is a prism *over the plate*).
pub(super) const PLATE_WIDTH: f32 = rules::PLATE_HALF_WIDTH_M * 2.0;
const PLATE_HALF_WIDTH: f32 = PLATE_WIDTH / 2.0;

/// Regulation base size since the 2023 rule change: 18 in square (0.457 m,
/// docs/BASEBALL.md) — shared by `spawn_bases`'s bag mesh above and the foul
/// line's outward offset below (`foul_line_span`).
const BASE_SIZE: f32 = 0.457;
const BASE_HALF_WIDTH: f32 = BASE_SIZE / 2.0;

/// Chalk line width: rulebooks call for lines "not less than 2 in nor more
/// than 4 in" of lime/chalk/paint (Official Baseball Rules 2.01; batflipsports.com's
/// groundskeeping guide gives the same 2–4 in range, and foxvalleypaint.com/
/// baseballstandard.com note MLB crews commonly stripe foul lines at the wider
/// end, ~4 in). We use 3 in (0.076 m), the middle of that range — see
/// docs/BASEBALL.md.
const CHALK_WIDTH: f32 = 0.076;
/// Full mesh height of every chalk quad (`spawn_flat_chalk`,
/// `spawn_chalk_segment`) — named so `CHALK_Y` below can derive the quads'
/// actual *bottom* face and prove it clears every ground decal in both
/// variants, not just its own translation.
const CHALK_MESH_HEIGHT: f32 = 0.002;
/// Gap left between the chalk quads' bottom face and the tallest ground
/// decal's top face, in whichever variant that decal belongs to.
const CHALK_CLEARANCE: f32 = 0.003;
/// Height chalk sits at. Chalk is shared by both sceneries (`spawn_chalk_lines`
/// runs regardless of `Scenery`), so it must clear every layer either one
/// paints: the stadium's dirt basepath/cutouts/grass-interior (topmost
/// `STADIUM_GRASS_INTERIOR_TOP`) *and* the front yard's street/sidewalk/
/// centre-line decals (topmost `FRONTYARD_CENTERLINE_TOP`, which is taller —
/// `spawn_front_yard`'s decals sit at y up to 0.004 vs. the stadium's
/// 0.0022). A regression here (chalk derived only against the stadium's
/// layers) let the front yard's foul lines z-fight the street and sidewalks
/// where they cross them (z ≈ 20–32); `tests::chalk_clears_every_ground_decal_in_both_variants`
/// guards it.
const CHALK_Y: f32 =
    super::stadium::FRONTYARD_CENTERLINE_TOP + CHALK_CLEARANCE + CHALK_MESH_HEIGHT / 2.0;

/// Batter's box: 4 ft × 6 ft, long side toward the pitcher, drawn 6 in off
/// each side of the plate (docs/BASEBALL.md). Lengthwise the box is spawned
/// *centred* on home plate (z = 0 below), not offset toward the pitcher:
/// researched for this task (groundskeeperu.com's field-layout guide and
/// corroborating groundskeeping references) puts the box's back line 3 ft
/// from the plate's centre and the front line 3 ft ahead of it — exactly
/// half the 6 ft box length each way, i.e. symmetric. See docs/BASEBALL.md.
const BOX_HALF_WIDTH: f32 = 0.61; // 4 ft / 2, side-to-side of the plate
const BOX_HALF_LENGTH: f32 = 0.915; // 6 ft / 2, toward the pitcher/catcher
const BOX_PLATE_GAP: f32 = 0.152; // 6 in
/// Centre-line x-offset of each batter's box from home plate: the plate's
/// own half-width, plus the regulation gap, plus half the box (so its inner
/// edge sits exactly `BOX_PLATE_GAP` off the plate).
const BOX_CENTER_X: f32 = PLATE_HALF_WIDTH + BOX_PLATE_GAP + BOX_HALF_WIDTH;

/// Perpendicular distance from `p` to the *segment* `a`→`b` — clamped to the
/// segment rather than the infinite line through it, so a point beyond
/// either endpoint is measured to that endpoint (both points in the ground's
/// XZ plane, passed here as `Vec2(x, z)`).
///
/// Test-only: a geometry check on what `spawn_chalk_segment` paints, not
/// something the spawn code itself needs at runtime (it places quads by
/// direct translation/rotation math, not by testing points against a line).
#[cfg(test)]
fn distance_point_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let dir = b - a;
    let len_sq = dir.length_squared();
    if len_sq < f32::EPSILON {
        return p.distance(a);
    }
    let t = ((p - a).dot(dir) / len_sq).clamp(0.0, 1.0);
    p.distance(a + dir * t)
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
        Mesh3d(meshes.add(Cuboid::new(size.x, CHALK_MESH_HEIGHT, size.y))),
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
        Mesh3d(meshes.add(Cuboid::new(length, CHALK_MESH_HEIGHT, width))),
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

/// The (start, end) points of the foul line running from home plate through
/// the base at `base_index`, out to the fence in that exact direction
/// (`rules::fence_at`) — the same function that places the outfield wall, so
/// the chalk, the wall, and the home-run ruling all agree on where fair
/// territory ends. `None` if the base sits exactly at the origin (nothing to
/// aim through — never true for a real `FieldSpec`, but keeps this total).
///
/// Per MLB Rule 2.03 ("The first and third base bags shall be entirely
/// within the infield" — i.e. fair territory) and groundskeeperu.com's
/// field-layout guide ("the foul edge of the foul line will line up exactly
/// with the foul edge of the base"): the line's fair-side edge runs along the
/// bag's *outer* (foul-side) edge, not through its centre. Modelled as a
/// parallel offset of the home→fence ray, perpendicular to `dir` and away
/// from the fair wedge's axis of symmetry (x = 0, see `rules::is_fair`), by
/// exactly the bag's half-width (`BASE_HALF_WIDTH`) — see docs/BASEBALL.md.
///
/// Pure geometry, deliberately factored out of `spawn_foul_line` so the test
/// module can exercise the *exact* span the spawn path paints, rather than
/// re-deriving the same formula in a test (which would pass even if this
/// function regressed).
fn foul_line_span(field: &FieldSpec, base_index: usize) -> Option<(Vec3, Vec3)> {
    let base = field.base_positions[base_index];
    let dir = Vec3::new(base.x, 0.0, base.z).normalize_or_zero();
    if dir == Vec3::ZERO {
        return None;
    }
    // The perpendicular to `dir` whose x-component shares `dir.x`'s sign —
    // i.e. points away from x = 0, out toward the foul side, for either the
    // first- or third-base line alike.
    let perp = Vec3::new(-dir.z, 0.0, dir.x);
    let outward = if perp.x * dir.x > 0.0 { perp } else { -perp };
    let offset = outward * BASE_HALF_WIDTH;
    Some((offset, dir * rules::fence_at(dir, field) + offset))
}

/// Spawns the chalk quad for `foul_line_span(field, base_index)`.
fn spawn_foul_line(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    chalk: &Handle<StandardMaterial>,
    field: &FieldSpec,
    base_index: usize,
) {
    if let Some((start, end)) = foul_line_span(field, base_index) {
        spawn_chalk_segment(commands, meshes, chalk, start, end, CHALK_WIDTH);
    }
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
        super::PitchersMound,
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "diamond.test.rs"]
mod tests;
