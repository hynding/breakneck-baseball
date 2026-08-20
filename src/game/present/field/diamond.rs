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
mod tests {
    use super::*;
    use crate::game::variant::VariantId;

    /// Basic sanity check on `distance_point_to_segment`'s geometry,
    /// independent of any field const: zero exactly on the segment, the
    /// expected perpendicular distance off it (while still between the
    /// endpoints), and clamped-to-endpoint distance for a point beyond it —
    /// this is a *segment* distance, not an infinite line's.
    #[test]
    fn distance_point_to_segment_matches_geometry() {
        let a = Vec2::ZERO;
        let b = Vec2::new(10.0, 10.0);
        assert!(distance_point_to_segment(Vec2::new(5.0, 5.0), a, b) < 1e-5);
        let want = 5.0 / std::f32::consts::SQRT_2;
        assert!((distance_point_to_segment(Vec2::new(0.0, 5.0), a, b) - want).abs() < 1e-4);
        // Beyond `b`: the infinite line's perpendicular distance would still
        // be small, but the segment must clamp to `b` itself.
        let beyond = Vec2::new(20.0, 20.0);
        let want_clamped = beyond.distance(b);
        assert!((distance_point_to_segment(beyond, a, b) - want_clamped).abs() < 1e-4);
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

    /// The foul lines must run along the real bases' *outer* edge on their
    /// way from home to the fence — offset from `FieldSpec::base_positions`
    /// by exactly the bag's half-width, not through the centre (MLB Rule
    /// 2.03 / groundskeeperu.com, see `foul_line_span`'s doc comment and
    /// docs/BASEBALL.md) — derived from the base positions themselves, not a
    /// hardcoded 45°, so this must hold for both variants (Standard's
    /// diamond and FrontYard's lawn, which uses a different `fair_half_angle`
    /// and base layout entirely).
    ///
    /// Exercises `foul_line_span` — the exact function `spawn_foul_line`
    /// calls to place the chalk — rather than re-deriving the direction/fence
    /// formula inline, so a regression in the real spawn path actually fails
    /// this test. Verified: temporarily changing `foul_line_span` to always
    /// read `base_positions[0]` (ignoring `base_index`) made the third-base
    /// assertions fail for both variants with a many-metres-off distance, as
    /// expected; reverted after confirming the failure.
    #[test]
    fn foul_lines_pass_through_first_and_third_base() {
        for variant in [VariantId::Standard, VariantId::FrontYard] {
            let field = variant.field();
            for &base_index in &[0, field.base_count() - 1] {
                let base = field.base_positions[base_index];
                let (start, end) = foul_line_span(&field, base_index)
                    .unwrap_or_else(|| panic!("{variant:?} base {base_index} at origin"));
                let dist = distance_point_to_segment(
                    Vec2::new(base.x, base.z),
                    Vec2::new(start.x, start.z),
                    Vec2::new(end.x, end.z),
                );
                assert!(
                    (dist - BASE_HALF_WIDTH).abs() < 1e-4,
                    "{variant:?} base {base_index} at {base:?} is {dist} m off its foul line, \
                     want exactly the bag half-width ({BASE_HALF_WIDTH} m) — not zero (through \
                     the centre) and not the old chalk-half-width tolerance",
                );
            }
        }
    }

    /// Regression guard for the FrontYard z-fighting bug: `CHALK_Y` was
    /// originally derived only against the stadium's own ground layers
    /// (topmost `STADIUM_GRASS_INTERIOR_TOP` ≈ 0.0027), but `spawn_chalk_lines`
    /// runs for *both* sceneries and the front yard's street/sidewalk/
    /// centre-line decals sit higher (topmost `FRONTYARD_CENTERLINE_TOP` =
    /// 0.006) — so a front-yard foul line crossing the street (z ≈ 20–32)
    /// shared a z-plane with the centre line and z-fought. Checks the chalk
    /// quads' actual *bottom* face (`CHALK_Y - CHALK_MESH_HEIGHT / 2`) clears
    /// every named ground-decal top in both variants, reading the same consts
    /// the spawn functions build their meshes from.
    #[test]
    fn chalk_clears_every_ground_decal_in_both_variants() {
        let chalk_bottom = CHALK_Y - CHALK_MESH_HEIGHT / 2.0;
        for (label, top) in [
            (
                "stadium dirt basepath",
                super::super::stadium::STADIUM_DIRT_TOP,
            ),
            ("stadium cutouts", super::super::stadium::STADIUM_CUTOUT_TOP),
            (
                "stadium grass interior",
                super::super::stadium::STADIUM_GRASS_INTERIOR_TOP,
            ),
            (
                "front yard street/sidewalks",
                super::super::stadium::FRONTYARD_STREET_TOP,
            ),
            (
                "front yard centre line",
                super::super::stadium::FRONTYARD_CENTERLINE_TOP,
            ),
        ] {
            assert!(
                chalk_bottom > top,
                "chalk bottom {chalk_bottom} does not clear {label}'s top face {top}"
            );
        }
    }
}
