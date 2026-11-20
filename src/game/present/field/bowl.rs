//! The stadium past the wall (task 5 of the graphics and camera plan): a
//! seating bowl that wraps the park from the batter's eye round behind home,
//! a crowd in it, light towers, and a scoreboard with the score.
//!
//! All of it is scenery: no colliders (a ball that leaves the park is
//! already called and reset by `sim::ball`), no shadows from the bowl, and
//! nothing reads gameplay state except the scoreboard, which mirrors
//! [`ScoreBoard`].
//!
//! The crowd is sized by the tier's [`RenderBudget`]: seated fans are
//! picked from every seat in the bowl, lower rows first, and merged into
//! one mesh per sector of the bowl (a dozen draws for the whole crowd, and
//! sectors behind the camera are culled). Cards face home plate and are
//! one-sided, as is the bowl, so the free camera looking in from outside
//! sees through to the field. On High the crowd casts shadows and its
//! sectors bounce after a home run or a walk-off.

use std::f32::consts::{PI, TAU};

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::game::flow::{BallInPlayEvent, Play};
use crate::game::quality::{ActiveQuality, RenderBudget};
use crate::game::rules::{self, ContactKind};
use crate::game::theme::Theme;
use crate::game::variant::{FieldSpec, Scenery};
use crate::game::{GameplayEntity, ScoreBoard};

use super::stadium::WALL_HEIGHT;

/// Gap between the outfield wall and the first row (m).
const WALL_GAP: f32 = 2.0;
/// Distance from each foul line to the foul-territory stands (m).
const FOUL_GAP: f32 = 18.0;
/// Horizontal depth of the seating, front row to back (m).
pub const BOWL_DEPTH: f32 = 34.0;
/// Rise per metre of depth: about 31°, a typical lower-deck rake.
const BOWL_SLOPE: f32 = 0.6;
/// Seat rows front to back (one metre of depth each).
pub const ROWS: usize = 34;
/// Seat pitch along a row (m), with an aisle every [`AISLE_EVERY`] seats.
const SEAT_W: f32 = 0.6;
const AISLE_EVERY: usize = 16;
/// Front-row height over the outfield wall, and over the foul stands' rail.
const FRONT_FAIR: f32 = WALL_HEIGHT + 0.6;
const FRONT_FOUL: f32 = 1.4;
/// Half-width of the batter's eye (no seats) either side of dead centre.
const EYE_HALF: f32 = 8.0 * PI / 180.0;
/// Angular samples round the bowl.
const SAMPLES: usize = 240;
/// Crowd sectors (one mesh each).
const SECTORS: usize = 12;
/// Parapet along the top of the bowl (m).
const PARAPET: f32 = 2.0;

/// The direction `theta` radians off dead centre (+Z), toward +X.
fn dir(theta: f32) -> Vec3 {
    Vec3::new(theta.sin(), 0.0, theta.cos())
}

/// `theta` folded into -π..=π.
fn wrap(theta: f32) -> f32 {
    (theta + PI).rem_euclid(TAU) - PI
}

/// Distance from home plate to the front row along `theta`: just behind
/// the home-run fence in fair territory, then a band parallel to each foul
/// line, which meets itself in a corner behind the plate.
pub fn inner_radius(theta: f32, field: &FieldSpec) -> f32 {
    let fence = rules::fence_at(dir(theta) * 10.0, field) + WALL_GAP;
    let past_line = wrap(theta).abs() - field.fair_half_angle;
    if past_line <= 0.0 {
        fence
    } else {
        fence.min(FOUL_GAP / past_line.sin().max(1e-3))
    }
}

/// Front-row height along `theta`: over the wall in fair territory, a low
/// rail along the foul lines, blended across the foul poles.
fn front_height(theta: f32, field: &FieldSpec) -> f32 {
    let edge = 2.0_f32.to_radians();
    let x = (field.fair_half_angle + edge - wrap(theta).abs()) / (2.0 * edge);
    let t = x.clamp(0.0, 1.0);
    let t = t * t * (3.0 - 2.0 * t);
    FRONT_FOUL + (FRONT_FAIR - FRONT_FOUL) * t
}

/// The bowl's angular span: from just past the batter's eye, round behind
/// home, back to the other side of it.
fn span() -> (f32, f32) {
    (EYE_HALF, TAU - EYE_HALF)
}

/// A point on the seating at `theta`, `depth` metres back from the front
/// row (0..=[`BOWL_DEPTH`]).
pub fn seat_point(theta: f32, depth: f32, field: &FieldSpec) -> Vec3 {
    let r = inner_radius(theta, field) + depth;
    dir(theta) * r + Vec3::Y * (front_height(theta, field) + depth * BOWL_SLOPE)
}

/// The bowl's top edge height at `theta`.
pub fn top_height(theta: f32, field: &FieldSpec) -> f32 {
    front_height(theta, field) + BOWL_DEPTH * BOWL_SLOPE
}

/// One seat: where, which way it faces, its row and a stable hash.
#[derive(Clone, Copy, Debug)]
pub struct Seat {
    pub pos: Vec3,
    pub theta: f32,
    pub row: usize,
    pub hash: f32,
}

/// Integer hash to 0..1 (stable across platforms, no RNG).
fn hash(a: u32, b: u32) -> f32 {
    let mut h = a.wrapping_mul(0x9E37_79B9) ^ b.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    (h & 0x00FF_FFFF) as f32 / 0x0100_0000 as f32
}

/// Every seat in the bowl, row by row, with aisles left empty.
pub fn seats(field: &FieldSpec) -> Vec<Seat> {
    let (a, b) = span();
    let mut out = Vec::new();
    for row in 0..ROWS {
        let depth = (row as f32 + 0.5) * (BOWL_DEPTH / ROWS as f32);
        let fine = SAMPLES * 4;
        let mut prev = seat_point(a, depth, field);
        let mut walked = 0.0;
        let mut index = 0usize;
        for k in 1..=fine {
            let theta = a + (b - a) * k as f32 / fine as f32;
            let p = seat_point(theta, depth, field);
            walked += p.distance(prev);
            prev = p;
            while walked >= SEAT_W {
                walked -= SEAT_W;
                index += 1;
                if index.is_multiple_of(AISLE_EVERY) {
                    continue;
                }
                out.push(Seat {
                    pos: p,
                    theta,
                    row,
                    hash: hash(row as u32, index as u32),
                });
            }
        }
    }
    out
}

/// The `count` seats a crowd of that size fills: lower rows first, with
/// enough scatter that a thin crowd still reads as people, not a stripe.
pub fn fill(seats: &[Seat], count: usize) -> Vec<Seat> {
    let mut ranked: Vec<(f32, Seat)> = seats
        .iter()
        .map(|s| (s.hash * 0.65 + (s.row as f32 / ROWS as f32) * 0.35, *s))
        .collect();
    ranked.sort_by(|x, y| x.0.total_cmp(&y.0));
    ranked.into_iter().take(count).map(|(_, s)| s).collect()
}

/// Which crowd sector a seat at `theta` belongs to.
fn sector_of(theta: f32) -> usize {
    let (a, b) = span();
    (((theta - a) / (b - a) * SECTORS as f32) as usize).min(SECTORS - 1)
}

/// Accumulates flat-shaded quads into one mesh, winding each to face
/// `facing` so back-face culling hides it from behind.
#[derive(Default)]
struct QuadMesh {
    pos: Vec<[f32; 3]>,
    normal: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    color: Vec<[f32; 4]>,
    index: Vec<u32>,
}

impl QuadMesh {
    /// `a b c d` go round the quad; `uv` matches them.
    fn quad(&mut self, corners: [Vec3; 4], uv: [[f32; 2]; 4], facing: Vec3, color: [f32; 4]) {
        let [a, b, _, d] = corners;
        let n = (b - a).cross(d - a);
        let flip = n.dot(facing) < 0.0;
        let n = if flip { -n } else { n }.normalize_or(facing);
        let base = self.pos.len() as u32;
        for (p, t) in corners.iter().zip(uv) {
            self.pos.push(p.to_array());
            self.normal.push(n.to_array());
            self.uv.push(t);
            self.color.push(color);
        }
        let tri: [u32; 6] = if flip {
            [0, 2, 1, 0, 3, 2]
        } else {
            [0, 1, 2, 0, 2, 3]
        };
        self.index.extend(tri.iter().map(|i| base + i));
    }

    fn build(self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normal)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uv)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.color)
        .with_inserted_indices(Indices::U32(self.index))
    }
}

const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

/// The seating ramp (tiled with rows of seats) and the concrete around it:
/// the front fascia and the parapet along the top.
fn bowl_meshes(field: &FieldSpec) -> (Mesh, Mesh) {
    let (a, b) = span();
    let mut seats = QuadMesh::default();
    let mut concrete = QuadMesh::default();
    let mut along = 0.0;
    for k in 0..SAMPLES {
        let t0 = a + (b - a) * k as f32 / SAMPLES as f32;
        let t1 = a + (b - a) * (k + 1) as f32 / SAMPLES as f32;
        let (f0, f1) = (seat_point(t0, 0.0, field), seat_point(t1, 0.0, field));
        let (r0, r1) = (
            seat_point(t0, BOWL_DEPTH, field),
            seat_point(t1, BOWL_DEPTH, field),
        );
        let step = f0.distance(f1) / 4.0;
        let (u0, u1) = (along, along + step);
        along = u1;
        let inward = -dir((t0 + t1) / 2.0);
        seats.quad(
            [f0, f1, r1, r0],
            [[u0, 0.0], [u1, 0.0], [u1, ROWS as f32], [u0, ROWS as f32]],
            Vec3::Y + inward,
            WHITE,
        );
        // Fascia from the turf up to the front row.
        concrete.quad(
            [f0.with_y(0.0), f1.with_y(0.0), f1, f0],
            [[0.0; 2]; 4],
            inward,
            WHITE,
        );
        // Parapet along the top edge.
        concrete.quad(
            [r0, r1, r1 + Vec3::Y * PARAPET, r0 + Vec3::Y * PARAPET],
            [[0.0; 2]; 4],
            inward,
            WHITE,
        );
    }
    (seats.build(), concrete.build())
}

/// The seat-row texture: a seat band and a darker riser, tiled once per
/// row down the ramp and every few metres along it.
fn seat_rows_image(seat: Color, riser: Color) -> Image {
    const N: u32 = 16;
    let (s, r) = (
        seat.to_srgba().to_u8_array(),
        riser.to_srgba().to_u8_array(),
    );
    let mut data = Vec::with_capacity((N * N * 4) as usize);
    for y in 0..N {
        for x in 0..N {
            // Seat backs every quarter of the tile, risers on the bottom
            // rows of each row-tile.
            let px = if y >= N - 4 || x % 4 == 0 { r } else { s };
            data.extend_from_slice(&px);
        }
    }
    super::textures::tiling_image(N, data)
}

/// The crowd in each sector, as one mesh per sector. `bob` sectors are
/// returned with their seats relative to the sector's own origin so the
/// whole sector can be lifted.
fn crowd_meshes(fans: &[Seat], theme: &Theme) -> Vec<(Vec3, Mesh)> {
    let mut sectors: Vec<(Vec3, usize, QuadMesh)> = (0..SECTORS)
        .map(|_| (Vec3::ZERO, 0, QuadMesh::default()))
        .collect();
    for fan in fans {
        let s = &mut sectors[sector_of(fan.theta)];
        s.0 += fan.pos;
        s.1 += 1;
    }
    for s in &mut sectors {
        s.0 /= s.1.max(1) as f32;
    }
    let shirts = [
        theme.home.jersey,
        theme.home.jersey,
        theme.home.jersey,
        theme.home.cap,
        theme.away.jersey,
        Color::srgb(0.92, 0.92, 0.90),
        Color::srgb(0.30, 0.32, 0.36),
        Color::srgb(0.95, 0.78, 0.20),
        Color::srgb(0.18, 0.45, 0.30),
    ];
    let skins = [
        Color::srgb(0.87, 0.67, 0.50),
        Color::srgb(0.62, 0.42, 0.28),
        Color::srgb(0.40, 0.26, 0.17),
    ];
    let lin = |c: Color| c.to_linear().to_f32_array();
    for (i, fan) in fans.iter().enumerate() {
        let (origin, _, mesh) = &mut sectors[sector_of(fan.theta)];
        let facing = -dir(fan.theta);
        let side = Vec3::Y.cross(facing).normalize();
        let h2 = hash(i as u32, 7);
        let standing = fan.hash > 0.9;
        let body = if standing { 0.85 } else { 0.55 };
        let shirt = lin(shirts[(h2 * shirts.len() as f32) as usize % shirts.len()]);
        let skin = lin(skins[(fan.hash * 97.0) as usize % skins.len()]);
        let at = fan.pos - *origin + Vec3::Y * 0.15;
        let card = |w: f32, y0: f32, y1: f32| {
            let half = side * (w / 2.0);
            [
                at - half + Vec3::Y * y0,
                at + half + Vec3::Y * y0,
                at + half + Vec3::Y * y1,
                at - half + Vec3::Y * y1,
            ]
        };
        mesh.quad(card(0.46, 0.0, body), [[0.0; 2]; 4], facing, shirt);
        mesh.quad(
            card(0.24, body + 0.02, body + 0.26),
            [[0.0; 2]; 4],
            facing,
            skin,
        );
    }
    sectors
        .into_iter()
        .filter(|s| s.1 > 0)
        .map(|(origin, _, mesh)| (origin, mesh.build()))
        .collect()
}

/// Marks a crowd sector; `phase` staggers its bounce.
#[derive(Component)]
pub struct CrowdSector {
    rest: Vec3,
    phase: f32,
}

/// The bowl itself (spawned once per stadium game).
#[derive(Component)]
pub struct Bowl;

/// The crowd's material and the tier it was last built for.
#[derive(Resource)]
pub(super) struct CrowdState {
    material: Handle<StandardMaterial>,
    built_for: Option<(usize, bool)>,
    /// Seconds of cheering left (home run, walk-off).
    cheer: f32,
    walk_off_seen: bool,
}

/// The scoreboard's face, redrawn when the score changes.
#[derive(Resource)]
pub(super) struct ScoreboardFace(Handle<Image>);

const BOARD_PX: (usize, usize) = (96, 48);

fn scoreboard_image(score: &ScoreBoard) -> Image {
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let (w, h) = BOARD_PX;
    let mut canvas = vec![0u8; w * h * 4];
    for px in canvas.as_chunks_mut::<4>().0 {
        px.copy_from_slice(&[10, 14, 12, 255]);
    }
    let amber = [255, 196, 64, 255];
    let white = [235, 235, 225, 255];
    let draw = crate::game::jersey::draw_text;
    draw(&mut canvas, w, 4, 4, 2, "AWAY", white);
    draw(&mut canvas, w, 4, 20, 2, "HOME", white);
    let runs = |n: u32| format!("{n:>2}");
    draw(&mut canvas, w, 64, 4, 2, &runs(score.away_runs), amber);
    draw(&mut canvas, w, 64, 20, 2, &runs(score.home_runs), amber);
    let half = if score.top_of_inning { "T" } else { "B" };
    let line = format!("{half}{} OUT {}", score.inning, score.outs);
    draw(&mut canvas, w, 4, 37, 1, &line, amber);
    let mut image = Image::new(
        Extent3d {
            width: w as u32,
            height: h as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        canvas,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    // Crisp bulbs: the 5×7 font goes soft under linear filtering.
    image.sampler = bevy::image::ImageSampler::nearest();
    image
}

/// A box facing home plate from `at`, `size` = (width, height, depth).
fn facing_home(at: Vec3) -> Quat {
    let flat = (-at).with_y(0.0);
    Quat::from_rotation_y(flat.x.atan2(flat.z))
}

/// A light tower's unshadowed fill spot at night. Shown on tiers whose
/// budget has `tower_lights` ([`tower_lights_follow_budget`]).
#[derive(Component)]
pub struct TowerLight;

/// Where the fill spots aim: the middle of the infield.
const TOWER_AIM: Vec3 = Vec3::new(0.0, 0.0, 25.0);

/// Each fill spot's output (lm). Bevy spreads a spot's lumens like a point
/// light's (over 4π sr), so at the ~170 m throw from a tower this lands
/// about 2,000 lux on the infield: three of them roughly double the night
/// key tower's light on the field, lifting it above the dim stands.
const TOWER_FILL_LUMENS: f32 = 8.0e8;

/// Shows the towers' fill spots on tiers that budget them and hides them
/// on the rest, following Auto's step-downs mid-game.
pub(super) fn tower_lights_follow_budget(
    active: Option<Res<ActiveQuality>>,
    mut lights: Query<(&mut Visibility, Ref<TowerLight>)>,
) {
    let Some(active) = active else {
        return;
    };
    let want = if active.budget().tower_lights {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for (mut visibility, marker) in &mut lights {
        if (active.is_changed() || marker.is_added()) && *visibility != want {
            *visibility = want;
        }
    }
}

pub(super) fn spawn_bowl(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    field: Res<FieldSpec>,
    theme: Res<Theme>,
    settings: Option<Res<crate::game::settings::Settings>>,
) {
    commands.remove_resource::<CrowdState>();
    commands.remove_resource::<ScoreboardFace>();
    if field.scenery != Scenery::Stadium {
        return;
    }
    let (ramp, concrete) = bowl_meshes(&field);
    let seat_tex = images.add(seat_rows_image(
        Color::srgb(0.16, 0.30, 0.52),
        Color::srgb(0.30, 0.31, 0.33),
    ));
    let matte = |m: StandardMaterial| StandardMaterial {
        perceptual_roughness: 0.95,
        ..m
    };
    commands.spawn((
        Bowl,
        GameplayEntity,
        Mesh3d(meshes.add(ramp)),
        MeshMaterial3d(materials.add(matte(StandardMaterial {
            base_color_texture: Some(seat_tex),
            ..default()
        }))),
        NotShadowCaster,
    ));
    commands.spawn((
        Bowl,
        GameplayEntity,
        Mesh3d(meshes.add(concrete)),
        MeshMaterial3d(materials.add(matte(StandardMaterial {
            base_color: Color::srgb(0.46, 0.46, 0.47),
            ..default()
        }))),
        NotShadowCaster,
    ));

    // The batter's eye: a dark, plain screen behind dead centre, as tall
    // as the bowl, so the pitch is read against nothing.
    let eye_r = field.fence_center + WALL_GAP + 4.0;
    // As wide as the gap in the seats at the screen's own distance, so it
    // never cuts into the ramp beside it.
    let eye_w = 2.0 * eye_r * EYE_HALF.tan();
    let eye_h = top_height(0.0, &field) + PARAPET;
    commands.spawn((
        GameplayEntity,
        Mesh3d(meshes.add(Cuboid::new(eye_w, eye_h, 1.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.04, 0.08, 0.06),
            perceptual_roughness: 1.0,
            ..default()
        })),
        Transform::from_xyz(0.0, eye_h / 2.0, eye_r),
        NotShadowCaster,
    ));

    // Light towers over the bowl's back edge, lamp banks facing the field.
    let pole_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.55, 0.56, 0.58),
        metallic: 0.6,
        perceptual_roughness: 0.4,
        ..default()
    });
    let lamp_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.98, 0.92),
        emissive: LinearRgba::rgb(6.0, 5.8, 5.2),
        ..default()
    });
    // Night: the towers light the park. The key tower's light is the
    // field's shadowed directional key (`diamond::day_look`); the others
    // each throw an unshadowed fill spot at the infield, on the tiers whose
    // budget allows (`TowerLight`).
    let time = settings.map_or_else(Default::default, |s| s.time_of_day);
    let night = super::diamond::day_look(theme.sky, &theme.field, time).night;
    for deg in [-118.0_f32, -42.0, 42.0, 118.0] {
        let theta = deg.to_radians();
        let base = seat_point(theta, BOWL_DEPTH, &field) + dir(theta) * 3.0;
        let height = base.y + 22.0;
        let foot = base.with_y(0.0);
        if night && deg != super::diamond::KEY_TOWER_DEG {
            let lamp = foot + Vec3::Y * (height + 1.5);
            commands.spawn((
                GameplayEntity,
                TowerLight,
                SpotLight {
                    color: Color::srgb(0.86, 0.92, 1.0),
                    intensity: TOWER_FILL_LUMENS,
                    range: 400.0,
                    radius: 2.0,
                    shadows_enabled: false,
                    outer_angle: 0.5,
                    inner_angle: 0.3,
                    ..default()
                },
                Transform::from_translation(lamp).looking_at(TOWER_AIM, Vec3::Y),
                Visibility::Hidden,
            ));
        }
        commands.spawn((
            GameplayEntity,
            Mesh3d(meshes.add(Cylinder::new(0.6, height))),
            MeshMaterial3d(pole_mat.clone()),
            Transform::from_translation(foot + Vec3::Y * (height / 2.0)),
            NotShadowCaster,
        ));
        commands.spawn((
            GameplayEntity,
            Mesh3d(meshes.add(Cuboid::new(8.0, 4.0, 0.8))),
            MeshMaterial3d(lamp_mat.clone()),
            Transform::from_translation(foot + Vec3::Y * (height + 1.5))
                .with_rotation(facing_home(foot) * Quat::from_rotation_x(-0.35)),
            NotShadowCaster,
        ));
    }

    // Scoreboard over the left-centre seats.
    let theta = 26.0_f32.to_radians();
    let at = seat_point(theta, BOWL_DEPTH, &field) + dir(theta) * 4.0;
    let (bw, bh) = (24.0, 12.0);
    let centre = at.with_y(at.y + PARAPET + bh / 2.0 + 1.0);
    // A fresh game's board; `update_scoreboard` repaints it on the first
    // score change. Painted from scratch rather than read from `ScoreBoard`,
    // whose game_start reset may not have run yet.
    let fresh = ScoreBoard {
        inning: 1,
        top_of_inning: true,
        ..default()
    };
    let face = images.add(scoreboard_image(&fresh));
    commands.insert_resource(ScoreboardFace(face.clone()));
    let turn = facing_home(at);
    commands.spawn((
        GameplayEntity,
        Mesh3d(meshes.add(Cuboid::new(bw + 1.0, bh + 1.0, 1.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.08, 0.09, 0.10),
            ..default()
        })),
        Transform::from_translation(centre).with_rotation(turn),
        NotShadowCaster,
    ));
    commands.spawn((
        GameplayEntity,
        Mesh3d(meshes.add(Rectangle::new(bw, bh))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color_texture: Some(face),
            unlit: true,
            ..default()
        })),
        // Just in front of the board, on the side facing home.
        Transform::from_translation(centre + turn * Vec3::Z * 0.55).with_rotation(turn),
        NotShadowCaster,
    ));
    // A flag over the board, streaming in the day's wind (`flags`).
    super::flags::FlagKit::new(&mut meshes, &mut materials).spawn_flagpole(
        &mut commands,
        &mut meshes,
        centre + Vec3::Y * (bh / 2.0 + 0.5),
        7.0,
        4.4,
    );

    commands.insert_resource(CrowdState {
        material: materials.add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 1.0,
            ..default()
        }),
        built_for: None,
        cheer: 0.0,
        walk_off_seen: false,
    });
}

/// How many fans and whether they cast shadows, for a budget.
fn crowd_plan(budget: &RenderBudget) -> (usize, bool) {
    (budget.crowd_instances as usize, budget.crowd_shadows)
}

/// (Re)builds the crowd whenever the tier's allowance differs from the
/// crowd on the field: at game start, and when Auto steps a tier down.
#[allow(clippy::too_many_arguments)]
pub(super) fn build_crowd(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    state: Option<ResMut<CrowdState>>,
    active: Option<Res<ActiveQuality>>,
    field: Res<FieldSpec>,
    theme: Res<Theme>,
    sectors: Query<Entity, With<CrowdSector>>,
) {
    let (Some(mut state), Some(active)) = (state, active) else {
        return;
    };
    let plan = crowd_plan(&active.budget());
    if state.built_for == Some(plan) {
        return;
    }
    state.built_for = Some(plan);
    for e in &sectors {
        commands.entity(e).despawn();
    }
    let fans = fill(&seats(&field), plan.0);
    for (i, (origin, mesh)) in crowd_meshes(&fans, &theme).into_iter().enumerate() {
        let mut e = commands.spawn((
            CrowdSector {
                rest: origin,
                phase: i as f32 * 1.3,
            },
            GameplayEntity,
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(state.material.clone()),
            Transform::from_translation(origin),
        ));
        if !plan.1 {
            e.insert(NotShadowCaster);
        }
    }
}

/// On High, the crowd's sectors bounce for a few seconds after a home run
/// or a walk-off.
pub(super) fn cheer_crowd(
    time: Res<Time>,
    play: Res<Play>,
    active: Option<Res<ActiveQuality>>,
    mut in_play: MessageReader<BallInPlayEvent>,
    state: Option<ResMut<CrowdState>>,
    mut sectors: Query<(&CrowdSector, &mut Transform)>,
) {
    let Some(mut state) = state else {
        in_play.clear();
        return;
    };
    let homer = in_play
        .read()
        .any(|ev| matches!(ev.kind, ContactKind::HomeRun));
    let walk_off = play.is_walk_off() && !state.walk_off_seen;
    if state.walk_off_seen != play.is_walk_off() {
        state.walk_off_seen = play.is_walk_off();
    }
    let motion = active.is_some_and(|a| a.budget().crowd_motion);
    if motion && (homer || walk_off) {
        state.cheer = 4.0;
    }
    if state.cheer <= 0.0 {
        return;
    }
    state.cheer = (state.cheer - time.delta_secs()).max(0.0);
    let t = time.elapsed_secs();
    let fade = (state.cheer / 1.0).min(1.0);
    for (sector, mut tf) in &mut sectors {
        let lift = 0.18 * fade * (t * 9.0 + sector.phase).sin().abs();
        tf.translation = sector.rest + Vec3::Y * lift;
    }
}

/// Repaints the scoreboard's face when the score, inning or outs change.
pub(super) fn update_scoreboard(
    score: Res<ScoreBoard>,
    face: Option<Res<ScoreboardFace>>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(face) = face else { return };
    if !score.is_changed() {
        return;
    }
    if let Some(image) = images.get_mut(&face.0) {
        *image = scoreboard_image(&score);
    }
}

#[cfg(test)]
#[path = "bowl.test.rs"]
mod tests;
