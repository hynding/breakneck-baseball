//! Presentation themes — every colour, template, and styling knob in one
//! swappable bundle.
//!
//! A [`Theme`] is plain data, exactly like [`crate::game::variant::Variant`]:
//! the UI palette, the per-team player templates, and the ball styling. UI
//! and world-spawn systems read the [`Theme`] resource instead of hardcoding
//! colours, so a whole new look is a new [`ThemeId`] arm — not new systems.
//! **T** on the main menu cycles themes.
//!
//! Colour-vision check (2026-08-21, deuteranopia + protanopia emulation over
//! the web build, screenshots in `docs/agent/playtest/2026-08-21/`): both
//! built-in themes keep the team split legible — the warm team collapses
//! toward yellow-olive and the cool team toward lavender, distinct hues in
//! all three conditions — and jersey names/numbers back-stop any remaining
//! ambiguity. HUD count dots are position-labelled (B/S/O), so their colours
//! are redundant. No palette change needed.

use bevy::color::LinearRgba;
use bevy::prelude::{Color, Resource};

/// Which glTF model asset a rig uses; the path is resolved by
/// `model_assets::player_model_path` and friends. New models = new arms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelId {
    Player,
}

/// Which rig construction builds the player bodies. The animation seam
/// ([`crate::game::animation::AnimClip`] + `MoveIntent` + the root
/// drop/pitch channels) is model-agnostic, so a richer humanoid model plugs
/// in as a new arm here — the graph driver behind the shared `AnimClip`
/// names is the actual seam, not the mesh construction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PlayerModelId {
    /// The built-in capsule-and-cylinder rig — kept as the fallback arm and
    /// the escape hatch if a model asset regresses.
    #[default]
    Blocky,
    /// A skinned, clip-animated glTF humanoid.
    Gltf(ModelId),
}

/// The full presentation bundle, inserted as a resource.
#[derive(Resource, Clone, Debug)]
pub struct Theme {
    pub ui: UiTheme,
    pub home: PlayerTemplate,
    pub away: PlayerTemplate,
    pub ball: BallTheme,
    pub fx: FxTheme,
    /// World clear colour — the sky above the park (bright day or night).
    pub sky: Color,
    /// Which player-model construction dresses the rigs.
    pub player_model: PlayerModelId,
}

/// How many shell colours a fireworks show cycles through.
pub const FIREWORK_COLORS: usize = 5;

/// Effect colours — the landing ring, contact sparks, infield dust, and the
/// home-run firework palette.
///
/// These live here rather than at each spawn site so a theme swap repaints
/// the *whole* show. The dust and firework colours used to be hardcoded in
/// `present/fx/particles.rs`, so a night game kicked up warm daylight dust
/// and burst warm daylight shells; the ring and spark meanwhile re-derived
/// themselves from `ui.accent` and `ball.trail` at their own sites, which
/// meant the effect palette had no single place to read (TODO 77).
///
/// Only *hue* belongs here. Per-effect opacity stays at the spawn site,
/// where it expresses how that effect reads (the home-run halo is fainter
/// than the sparks it surrounds) rather than anything about the theme.
///
/// The pitch trail is deliberately NOT here: `Settings::trail_color` is the
/// player's own choice, and a theme swap must not silently overwrite it.
#[derive(Clone, Debug)]
pub struct FxTheme {
    /// The touchdown indicator ring under a live fly ball.
    pub ring: Color,
    /// Contact sparks, and (faded) the home-run halo.
    pub spark: Color,
    /// Infield dust kicked up on a hard grounder or a slide.
    pub dust: Color,
    /// Home-run firework shells, one material per entry.
    pub fireworks: [Color; FIREWORK_COLORS],
}

/// Palette for every HUD/menu element.
#[derive(Clone, Debug)]
pub struct UiTheme {
    /// Translucent card background.
    pub panel_bg: Color,
    /// Hairline card border.
    pub panel_border: Color,
    /// Titles, selected values, occupied base pips.
    pub accent: Color,
    pub text_primary: Color,
    pub text_dim: Color,
    /// Empty base pips / unlit count dots.
    pub pip_off: Color,
    /// B/S/O indicator-dot colours.
    pub count_ball: Color,
    pub count_strike: Color,
    pub count_out: Color,
    /// Banner tone palette (see `flow::BannerTone`).
    pub tone_good: Color,
    pub tone_bad: Color,
    pub tone_info: Color,
    pub tone_epic: Color,
    /// Strike-zone overlay wireframe — a near-invisible "ghost" tuned per
    /// theme so it reads against *that theme's* sky (a fixed near-black
    /// frame disappeared entirely at night — TODO 63).
    pub zone_frame: Color,
    /// The zone's near-face tint (the pane the PCI cursor reads against).
    pub zone_fill: Color,
}

/// One team's player look. Swappable per theme.
#[derive(Clone, Debug)]
pub struct PlayerTemplate {
    pub jersey: Color,
    pub cap: Color,
    pub skin: Color,
    pub bat: Color,
}

/// Ball styling. `visual_scale` multiplies the *mesh* radius only — the
/// physics collider stays at the regulation [`crate::game::ball::BALL_RADIUS`].
#[derive(Clone, Debug)]
pub struct BallTheme {
    pub color: Color,
    pub emissive: LinearRgba,
    pub visual_scale: f32,
    /// Translucent motion-trail colour.
    pub trail: Color,
}

/// The selectable themes, cycled on the main menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ThemeId {
    #[default]
    DaylightClassic,
    MidnightNeon,
}

impl ThemeId {
    /// The next theme in the menu cycle (wraps).
    pub fn next(self) -> ThemeId {
        match self {
            ThemeId::DaylightClassic => ThemeId::MidnightNeon,
            ThemeId::MidnightNeon => ThemeId::DaylightClassic,
        }
    }

    /// Menu label.
    pub fn label(self) -> &'static str {
        match self {
            ThemeId::DaylightClassic => "Daylight Classic",
            ThemeId::MidnightNeon => "Midnight Neon",
        }
    }

    /// Materialises the full theme definition.
    pub fn build(self) -> Theme {
        match self {
            // Warm broadcast look: navy glass panels, gold accents, classic
            // red-vs-blue teams, a bright white ball.
            ThemeId::DaylightClassic => Theme {
                ui: UiTheme {
                    panel_bg: Color::srgba(0.04, 0.07, 0.14, 0.85),
                    panel_border: Color::srgba(1.0, 1.0, 1.0, 0.14),
                    accent: Color::srgb(1.0, 0.84, 0.25),
                    text_primary: Color::WHITE,
                    text_dim: Color::srgba(1.0, 1.0, 1.0, 0.65),
                    pip_off: Color::srgba(1.0, 1.0, 1.0, 0.25),
                    count_ball: Color::srgb(0.45, 0.85, 0.45),
                    count_strike: Color::srgb(1.0, 0.72, 0.25),
                    count_out: Color::srgb(1.0, 0.42, 0.35),
                    tone_good: Color::srgb(0.55, 1.0, 0.65),
                    tone_bad: Color::srgb(1.0, 0.5, 0.4),
                    tone_info: Color::srgb(0.95, 0.9, 0.7),
                    tone_epic: Color::srgb(1.0, 0.84, 0.25),
                    // Dark steel ghost against the bright day sky.
                    zone_frame: Color::srgba(0.10, 0.11, 0.14, 0.20),
                    zone_fill: Color::srgba(0.05, 0.06, 0.08, 0.10),
                },
                home: PlayerTemplate {
                    jersey: Color::srgb(0.22, 0.42, 0.9),
                    cap: Color::srgb(0.08, 0.14, 0.38),
                    skin: Color::srgb(0.87, 0.67, 0.5),
                    bat: Color::srgb(0.72, 0.5, 0.28),
                },
                away: PlayerTemplate {
                    // Deep red, not the old brighter 0.88/0.22/0.2: under
                    // the daylight sun that salmon read skin-toned at
                    // broadcast distance (TODO 82).
                    jersey: Color::srgb(0.74, 0.13, 0.12),
                    cap: Color::srgb(0.4, 0.06, 0.06),
                    skin: Color::srgb(0.87, 0.67, 0.5),
                    bat: Color::srgb(0.72, 0.5, 0.28),
                },
                ball: BallTheme {
                    color: Color::WHITE,
                    emissive: LinearRgba::rgb(1.5, 1.4, 1.1),
                    visual_scale: 2.7,
                    trail: Color::srgba(1.0, 1.0, 0.9, 0.35),
                },
                fx: FxTheme {
                    // Ring and spark match this theme's `ui.accent` and
                    // `ball.trail` — the values they used to re-derive from
                    // those fields at their spawn sites.
                    ring: Color::srgb(1.0, 0.84, 0.25),
                    spark: Color::srgba(1.0, 1.0, 0.9, 0.35),
                    // Warm infield tan, lit by daylight.
                    dust: Color::srgba(0.75, 0.7, 0.6, 1.0),
                    fireworks: [
                        Color::srgb(1.0, 0.85, 0.30),
                        Color::srgb(1.0, 0.35, 0.35),
                        Color::srgb(0.45, 0.70, 1.0),
                        Color::srgb(0.60, 1.0, 0.55),
                        Color::srgb(1.0, 0.55, 0.90),
                    ],
                },
                sky: Color::srgb(0.48, 0.67, 0.88),
                player_model: PlayerModelId::Gltf(ModelId::Player),
            },
            // Night-game arcade look: black glass, cyan accents, cyan-vs-
            // magenta teams, a neon ball that reads at any distance.
            ThemeId::MidnightNeon => Theme {
                ui: UiTheme {
                    panel_bg: Color::srgba(0.01, 0.02, 0.05, 0.88),
                    panel_border: Color::srgba(0.2, 0.95, 1.0, 0.35),
                    accent: Color::srgb(0.25, 0.95, 1.0),
                    text_primary: Color::srgb(0.92, 0.98, 1.0),
                    text_dim: Color::srgba(0.8, 0.9, 1.0, 0.55),
                    pip_off: Color::srgba(0.5, 0.8, 1.0, 0.2),
                    count_ball: Color::srgb(0.3, 1.0, 0.7),
                    count_strike: Color::srgb(1.0, 0.85, 0.2),
                    count_out: Color::srgb(1.0, 0.3, 0.5),
                    tone_good: Color::srgb(0.4, 1.0, 0.8),
                    tone_bad: Color::srgb(1.0, 0.45, 0.3),
                    tone_info: Color::srgb(0.8, 0.9, 1.0),
                    tone_epic: Color::srgb(1.0, 0.25, 0.75),
                    // Pale neon ghost against the near-black night sky.
                    zone_frame: Color::srgba(0.75, 0.9, 1.0, 0.22),
                    zone_fill: Color::srgba(0.6, 0.8, 1.0, 0.08),
                },
                home: PlayerTemplate {
                    jersey: Color::srgb(0.1, 0.85, 0.95),
                    cap: Color::srgb(0.02, 0.2, 0.3),
                    skin: Color::srgb(0.8, 0.7, 0.62),
                    bat: Color::srgb(0.15, 0.15, 0.2),
                },
                away: PlayerTemplate {
                    jersey: Color::srgb(1.0, 0.2, 0.72),
                    cap: Color::srgb(0.3, 0.02, 0.2),
                    skin: Color::srgb(0.8, 0.7, 0.62),
                    bat: Color::srgb(0.15, 0.15, 0.2),
                },
                ball: BallTheme {
                    color: Color::srgb(1.0, 0.95, 0.35),
                    emissive: LinearRgba::rgb(2.2, 2.0, 0.6),
                    visual_scale: 2.7,
                    trail: Color::srgba(1.0, 0.95, 0.4, 0.4),
                },
                fx: FxTheme {
                    // Same relationship as Daylight's: the ring takes this
                    // theme's cyan accent, the spark its neon ball trail.
                    ring: Color::srgb(0.25, 0.95, 1.0),
                    spark: Color::srgba(1.0, 0.95, 0.4, 0.4),
                    // Cool and dim: warm tan dust under the lights read as
                    // daylight puffs on a night field (TODO 77).
                    dust: Color::srgba(0.42, 0.48, 0.60, 1.0),
                    // Neon shells, tuned to this theme's accents rather than
                    // the broadcast palette.
                    fireworks: [
                        Color::srgb(0.25, 0.95, 1.0),
                        Color::srgb(1.0, 0.25, 0.75),
                        Color::srgb(0.55, 0.35, 1.0),
                        Color::srgb(0.30, 1.0, 0.70),
                        Color::srgb(1.0, 0.95, 0.35),
                    ],
                },
                sky: Color::srgb(0.02, 0.03, 0.08),
                player_model: PlayerModelId::Gltf(ModelId::Player),
            },
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "theme.test.rs"]
mod tests;
