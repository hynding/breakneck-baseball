//! Stadium and front-yard scenery: ground layers, foul poles, the outfield
//! wall, and lighting.

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::game::GameplayEntity;
use crate::game::rules;
use crate::game::variant::FieldSpec;

use super::{FieldSurfaces, HALF_DIAGONAL};

/// Ground-plane thickness for the static field collider.
const GROUND_HALF_DEPTH: f32 = 0.1;

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
        super::GroundPlane,
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
pub(super) fn spawn_stadium_ground(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    surfaces: &FieldSurfaces,
) {
    spawn_ground_slab(commands, meshes, materials, surfaces);

    let dirt = FieldSurfaces::tiled(materials, &surfaces.dirt, 8.0);
    commands.spawn((
        GameplayEntity,
        Mesh3d(meshes.add(Cuboid::new(
            INFIELD_HALF * 2.0,
            STADIUM_LAYER_HEIGHT,
            INFIELD_HALF * 2.0,
        ))),
        MeshMaterial3d(dirt.clone()),
        Transform {
            translation: Vec3::new(0.0, STADIUM_DIRT_Y, HALF_DIAGONAL),
            rotation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_4),
            ..default()
        },
    ));

    // Rounded dirt cutouts at the corner bags (the ~13 ft sliding pits) and
    // the home-plate circle.
    let cutout = meshes.add(Cylinder::new(CUTOUT_RADIUS, STADIUM_LAYER_HEIGHT));
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
            Transform::from_translation(corner + Vec3::Y * STADIUM_CUTOUT_Y),
        ));
    }

    // The grass interior of the diamond: dirt shows only as the basepath
    // band around it (plus the mound and cutouts layered above).
    let inner_half = INFIELD_HALF - BASEPATH_WIDTH;
    commands.spawn((
        GameplayEntity,
        Mesh3d(meshes.add(Cuboid::new(
            inner_half * 2.0,
            STADIUM_LAYER_HEIGHT,
            inner_half * 2.0,
        ))),
        MeshMaterial3d(FieldSurfaces::tiled(materials, &surfaces.grass, 5.0)),
        Transform {
            translation: Vec3::new(0.0, STADIUM_GRASS_INTERIOR_Y, HALF_DIAGONAL),
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
const INFIELD_HALF: f32 = super::BASE_DISTANCE / 2.0;

/// Dirt-cutout radius at each bag and around home (~13 ft, docs/BASEBALL.md).
const CUTOUT_RADIUS: f32 = 3.96;
/// Width of the dirt basepath band framing the grass infield.
const BASEPATH_WIDTH: f32 = 4.0;

/// Full mesh height shared by every thin layered surface
/// `spawn_stadium_ground` stacks over the ground slab (dirt basepath diamond,
/// cutout circles, grass interior) — named so the chalk layer below can
/// compute each one's top face (`translation_y + height / 2`) instead of
/// re-deriving it from a duplicated literal.
const STADIUM_LAYER_HEIGHT: f32 = 0.001;
/// Dirt basepath diamond's y (lowest of the three layers).
const STADIUM_DIRT_Y: f32 = 0.001;
/// Dirt cutout circles' y — one layer above the basepath diamond so the bags
/// and home plate read as distinct dirt patches rather than fighting it.
const STADIUM_CUTOUT_Y: f32 = 0.0016;
/// Grass interior's y — the topmost of the three, so it reads over the dirt
/// diamond beneath it.
const STADIUM_GRASS_INTERIOR_Y: f32 = 0.0022;
/// Top face heights of all three stadium ground layers (`translation_y +
/// height / 2`), read directly by the chalk-clearance test below.
///
/// Test-only: production only needs the front yard's (taller) topmost decal
/// to size `CHALK_Y` (see its doc comment), but the test checks every layer
/// in both variants individually.
#[cfg(test)]
pub(super) const STADIUM_DIRT_TOP: f32 = STADIUM_DIRT_Y + STADIUM_LAYER_HEIGHT / 2.0;
#[cfg(test)]
pub(super) const STADIUM_CUTOUT_TOP: f32 = STADIUM_CUTOUT_Y + STADIUM_LAYER_HEIGHT / 2.0;
#[cfg(test)]
pub(super) const STADIUM_GRASS_INTERIOR_TOP: f32 =
    STADIUM_GRASS_INTERIOR_Y + STADIUM_LAYER_HEIGHT / 2.0;

// ── Front-yard scenery ────────────────────────────────────────────────────────
/// Full mesh height shared by every flat street/sidewalk/centre-line decal
/// `spawn_front_yard`'s `flat` closure paints — named for the same reason as
/// `STADIUM_LAYER_HEIGHT`: so the chalk layer can prove it clears these top
/// faces too, without re-deriving the height from a duplicated literal.
const STREET_DECAL_HEIGHT: f32 = 0.004;
/// Asphalt and both sidewalks sit at this y.
const STREET_DECAL_Y: f32 = 0.002;
/// The painted centre line sits one layer above the asphalt — the tallest of
/// the front yard's ground decals.
const CENTERLINE_DECAL_Y: f32 = 0.004;
/// Top face heights of the front yard's ground decals, read directly by the
/// chalk-clearance test below.
///
/// `FRONTYARD_STREET_TOP` is test-only (the asphalt/sidewalks aren't the
/// tallest decal, so `CHALK_Y` doesn't need it); `FRONTYARD_CENTERLINE_TOP`
/// *is* production-used, by `CHALK_Y` itself.
#[cfg(test)]
pub(super) const FRONTYARD_STREET_TOP: f32 = STREET_DECAL_Y + STREET_DECAL_HEIGHT / 2.0;
pub(super) const FRONTYARD_CENTERLINE_TOP: f32 = CENTERLINE_DECAL_Y + STREET_DECAL_HEIGHT / 2.0;

/// Suburban lot: the batter hits from the lawn out across the street. All
/// dressing is visual-only (no colliders) so the analytic outcomes and the
/// ball's flight are never blocked; only the ground and the pitching mat are
/// physical.
pub(super) fn spawn_front_yard(
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
        super::PitchersMound,
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
            Mesh3d(meshes.add(Cuboid::new(size.x, STREET_DECAL_HEIGHT, size.y))),
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
    flat(
        Vec2::new(300.0, 8.0),
        Vec3::new(0.0, STREET_DECAL_Y, 26.0),
        asphalt,
    );
    flat(
        Vec2::new(300.0, 0.3),
        Vec3::new(0.0, CENTERLINE_DECAL_Y, 26.0),
        Color::srgb(0.85, 0.75, 0.2), // painted centre line
    );
    flat(
        Vec2::new(300.0, 2.0),
        Vec3::new(0.0, STREET_DECAL_Y, 21.0),
        concrete,
    );
    flat(
        Vec2::new(300.0, 2.0),
        Vec3::new(0.0, STREET_DECAL_Y, 31.0),
        concrete,
    );

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

// ── Foul poles ────────────────────────────────────────────────────────────────
pub(super) fn spawn_foul_poles(
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
            super::FoulPole,
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
pub(super) fn spawn_outfield_wall(
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
            super::OutfieldWall,
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
pub(super) fn spawn_lighting(commands: &mut Commands, yaw: f32, ambient_fraction: f32) {
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
    /// call sites (0.15 Stadium, 0.20 FrontYard — retuned for contrast in
    /// the Task 20 polish sweep, previously 0.25/0.35) were passed straight
    /// through as `AmbientLight::brightness` — fine under Bevy's pre-0.14
    /// small-multiplier semantics, but in 0.15 `brightness` is an absolute
    /// lux-like value (`AmbientLight::default()` is 80.0), so a raw 0.25 was
    /// indistinguishable from "no ambient light at all" next to a 50,000 lux
    /// sun. Every scenery's resulting ambient must clear Bevy's own default
    /// fill (else it's *darker* than doing nothing) and stay a sane fraction
    /// of the sun rather than approaching or exceeding it.
    #[test]
    fn ambient_fraction_scales_with_sun_illuminance_not_raw() {
        for ambient_fraction in [0.15_f32, 0.20] {
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
}
