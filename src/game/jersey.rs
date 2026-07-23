//! Procedural jersey lettering — names and numbers, no font assets.
//!
//! In the same spirit as the synthesized audio, every jersey texture is drawn
//! at runtime from a built-in 5×7 bitmap font into an in-memory RGBA image:
//! the back carries the surname over a big number, the chest and both
//! shoulders carry the number alone. Quads hang off the rig roots (they are
//! not [`RigPart`]s, so team recolouring ignores them) and a single system
//! re-dresses everyone whenever the half-inning flips, the batting order
//! advances, or a substitution changes the roster. Textures are cached per
//! (team, player, face) so a full game allocates a few dozen small images.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::game::roster::{PlayerCard, Rosters};
use crate::game::rules::BattingOrder;
use crate::game::theme::Theme;
use crate::game::{GameState, ScoreBoard, Team};

// ── The 5×7 font ──────────────────────────────────────────────────────────────

/// Glyph rows, top to bottom; bit 4 is the leftmost column.
type Glyph = [u8; 7];

const fn letter(c: char) -> Glyph {
    match c {
        'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        'C' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        'D' => [0x1E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1E],
        'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        'G' => [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0E],
        'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'I' => [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        'J' => [0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0C],
        'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        'N' => [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11],
        'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        'Q' => [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
        'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x1B, 0x11],
        'X' => [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
        'Y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        'Z' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
        '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        '2' => [0x0E, 0x11, 0x01, 0x06, 0x08, 0x10, 0x1F],
        '3' => [0x0E, 0x11, 0x01, 0x06, 0x01, 0x11, 0x0E],
        '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        '5' => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        '6' => [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
        _ => [0; 7],
    }
}

/// Advance per glyph in font pixels (5 columns + 1 gap).
const ADVANCE: usize = 6;

/// Stamps `text` into an RGBA byte canvas at `(x, y)`, each font pixel drawn
/// as a `scale`×`scale` block.
fn draw_text(
    canvas: &mut [u8],
    width: usize,
    x: usize,
    y: usize,
    scale: usize,
    text: &str,
    color: [u8; 4],
) {
    let height = canvas.len() / (4 * width);
    for (i, c) in text.chars().enumerate() {
        let glyph = letter(c);
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..5 {
                if bits & (0x10 >> col) == 0 {
                    continue;
                }
                for dy in 0..scale {
                    for dx in 0..scale {
                        let px = x + (i * ADVANCE + col) * scale + dx;
                        let py = y + row * scale + dy;
                        if px < width && py < height {
                            let at = (py * width + px) * 4;
                            canvas[at..at + 4].copy_from_slice(&color);
                        }
                    }
                }
            }
        }
    }
}

/// Pixel width of `text` at `scale` (without the trailing gap).
fn text_width(text: &str, scale: usize) -> usize {
    (text.chars().count() * ADVANCE).saturating_sub(1) * scale
}

/// The largest scale at which `text` fits `max_width` pixels (at least 1).
fn fit_scale(text: &str, max_width: usize) -> usize {
    (max_width / text_width(text, 1).max(1)).max(1)
}

// ── Texture building ──────────────────────────────────────────────────────────

/// Which lettering a quad shows.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum JerseyFace {
    /// Name arched over the big number.
    Back,
    /// Number only (chest and shoulders share it).
    Number,
}

fn build_texture(card: &PlayerCard, face: JerseyFace, color: [u8; 4]) -> Image {
    let (w, h) = match face {
        JerseyFace::Back => (96, 96),
        JerseyFace::Number => (48, 48),
    };
    let mut canvas = vec![0u8; w * h * 4];
    let number = card.number.to_string();
    match face {
        JerseyFace::Back => {
            let name_scale = fit_scale(card.name, w - 8).min(2);
            let nx = (w - text_width(card.name, name_scale)) / 2;
            draw_text(&mut canvas, w, nx, 6, name_scale, card.name, color);
            let num_scale = fit_scale(&number, w - 12).min(6);
            let x = (w - text_width(&number, num_scale)) / 2;
            let y = 24 + (h - 24 - 7 * num_scale) / 2;
            draw_text(&mut canvas, w, x, y, num_scale, &number, color);
        }
        JerseyFace::Number => {
            let scale = fit_scale(&number, w - 8).min(4);
            let x = (w - text_width(&number, scale)) / 2;
            let y = (h - 7 * scale) / 2;
            draw_text(&mut canvas, w, x, y, scale, &number, color);
        }
    }
    Image::new(
        Extent3d {
            width: w as u32,
            height: h as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        canvas,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// Lettering colour that reads against a jersey colour.
fn contrast_color(jersey: Color) -> [u8; 4] {
    let c = jersey.to_srgba();
    let luminance = 0.299 * c.red + 0.587 * c.green + 0.114 * c.blue;
    if luminance > 0.55 {
        [16, 16, 24, 255]
    } else {
        [245, 245, 245, 255]
    }
}

// ── Components & assets ───────────────────────────────────────────────────────

/// Whose lettering a rig wears: resolved against the live scoreboard so the
/// defense always shows the fielding team's roster and the batter the man
/// actually due up.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum JerseyRole {
    Pitcher,
    Fielder(usize),
    Batter,
}

/// One lettered quad on a rig.
#[derive(Component)]
pub struct JerseyQuad {
    role: JerseyRole,
    face: JerseyFace,
}

/// Shared quad meshes plus the transparent placeholder material quads spawn
/// with (never fully invisible on wasm — the update system paints them on
/// the first frame anyway).
#[derive(Resource)]
pub struct JerseyAssets {
    back: Handle<Mesh>,
    chest: Handle<Mesh>,
    shoulder: Handle<Mesh>,
    placeholder: Handle<StandardMaterial>,
}

/// Cache of generated lettering materials, keyed by (team, name, number,
/// face) — bounded by roster size.
#[derive(Resource, Default)]
struct JerseyCache(HashMap<(Team, &'static str, u32, JerseyFace), Handle<StandardMaterial>>);

/// Builds the shared quad meshes/placeholder. Called by the player spawner
/// (which inserts the resource) so rigs and their jerseys appear together —
/// no cross-plugin ordering to get wrong.
pub fn make_assets(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> JerseyAssets {
    JerseyAssets {
        back: meshes.add(Rectangle::new(0.46, 0.46)),
        chest: meshes.add(Rectangle::new(0.20, 0.20)),
        shoulder: meshes.add(Rectangle::new(0.14, 0.14)),
        placeholder: materials.add(StandardMaterial {
            base_color: Color::srgba(1.0, 1.0, 1.0, 0.01),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        }),
    }
}

/// Hangs the four lettered quads (back, chest, both shoulders) off a rig
/// root. Called by the player spawner for every dressed rig.
pub fn attach_jerseys(
    commands: &mut Commands,
    rig: Entity,
    role: JerseyRole,
    assets: &JerseyAssets,
) {
    let pi = std::f32::consts::PI;
    let half = std::f32::consts::FRAC_PI_2;
    let quads: [(JerseyFace, &Handle<Mesh>, Vec3, f32); 4] = [
        (
            JerseyFace::Back,
            &assets.back,
            Vec3::new(0.0, 0.32, -0.315),
            pi,
        ),
        (
            JerseyFace::Number,
            &assets.chest,
            Vec3::new(0.0, 0.42, 0.315),
            0.0,
        ),
        (
            JerseyFace::Number,
            &assets.shoulder,
            Vec3::new(0.315, 0.52, 0.0),
            half,
        ),
        (
            JerseyFace::Number,
            &assets.shoulder,
            Vec3::new(-0.315, 0.52, 0.0),
            -half,
        ),
    ];
    for (face, mesh, pos, yaw) in quads {
        let quad = commands
            .spawn((
                JerseyQuad { role, face },
                Mesh3d((*mesh).clone()),
                MeshMaterial3d(assets.placeholder.clone()),
                Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(yaw)),
            ))
            .id();
        commands.entity(quad).set_parent(rig);
    }
}

// ── The dressing system ───────────────────────────────────────────────────────

/// Re-letters every jersey quad whenever the scoreboard flips sides, the
/// batting order advances, a substitution rewrites a roster, or fresh quads
/// appear.
#[allow(clippy::too_many_arguments)]
fn dress_jerseys(
    score: Res<ScoreBoard>,
    order: Res<BattingOrder>,
    rosters: Res<Rosters>,
    theme: Res<Theme>,
    assets: Option<Res<JerseyAssets>>,
    mut cache: ResMut<JerseyCache>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    added: Query<(), Added<JerseyQuad>>,
    mut quads: Query<(&JerseyQuad, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    if assets.is_none() {
        return;
    }
    let refresh =
        score.is_changed() || order.is_changed() || rosters.is_changed() || !added.is_empty();
    if !refresh {
        return;
    }

    for (quad, mut material) in &mut quads {
        let team = match quad.role {
            JerseyRole::Batter => score.batting_team(),
            _ => score.fielding_team(),
        };
        let roster = rosters.team(team);
        let card = match quad.role {
            JerseyRole::Pitcher => roster.fielding(None),
            JerseyRole::Fielder(i) => roster.fielding(Some(i)),
            JerseyRole::Batter => roster.batting(order.current(team)),
        };
        let key = (team, card.name, card.number, quad.face);
        let handle = cache.0.entry(key).or_insert_with(|| {
            let template = match team {
                Team::Home => &theme.home,
                Team::Away => &theme.away,
            };
            let image = build_texture(card, quad.face, contrast_color(template.jersey));
            materials.add(StandardMaterial {
                base_color_texture: Some(images.add(image)),
                alpha_mode: AlphaMode::Blend,
                perceptual_roughness: 0.9,
                ..default()
            })
        });
        if material.0 != *handle {
            material.0 = handle.clone();
        }
    }
}

pub struct JerseyPlugin;

impl Plugin for JerseyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<JerseyCache>()
            .add_systems(crate::game::game_start(), reset_cache)
            .add_systems(Update, dress_jerseys.run_if(in_state(GameState::Playing)));
    }
}

/// A new game may bring a new theme (and with it new lettering contrast):
/// drop every cached material so textures are rebuilt against the current
/// jerseys — this also frees the previous game's images.
fn reset_cache(mut cache: ResMut<JerseyCache>) {
    cache.0.clear();
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyphs_exist_for_the_full_roster_alphabet() {
        for c in ('A'..='Z').chain('0'..='9') {
            assert_ne!(letter(c), [0; 7], "glyph for {c:?} is blank");
        }
        assert_eq!(letter(' '), [0; 7]);
    }

    #[test]
    fn back_texture_draws_name_and_number_pixels() {
        let card = PlayerCard {
            name: "OKAFOR",
            number: 23,
        };
        let image = build_texture(&card, JerseyFace::Back, [255, 255, 255, 255]);
        let data = image.data;
        let lit = data.chunks(4).filter(|px| px[3] == 255).count();
        // A six-letter name plus two big digits lights up plenty of pixels.
        assert!(lit > 200, "only {lit} opaque pixels drawn");
        // Everything else stays transparent (the jersey shows through).
        let clear = data.chunks(4).filter(|px| px[3] == 0).count();
        assert!(clear > lit);
    }

    #[test]
    fn number_texture_scales_single_digits_up() {
        let card = PlayerCard {
            name: "PYE",
            number: 8,
        };
        let one = build_texture(&card, JerseyFace::Number, [255, 255, 255, 255]);
        let lit = one.data.chunks(4).filter(|px| px[3] == 255).count();
        assert!(lit > 100, "a lone digit should be drawn large ({lit} px)");
    }

    #[test]
    fn lettering_contrasts_with_the_jersey() {
        let on_dark = contrast_color(Color::srgb(0.1, 0.1, 0.3));
        let on_light = contrast_color(Color::srgb(0.9, 0.9, 0.85));
        assert!(on_dark[0] > 200);
        assert!(on_light[0] < 60);
    }
}
