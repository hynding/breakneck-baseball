//! Presentation themes — every colour, template, and styling knob in one
//! swappable bundle.
//!
//! A [`Theme`] is plain data, exactly like [`crate::game::variant::Variant`]:
//! the UI palette, the per-team player templates, and the ball styling. UI
//! and world-spawn systems read the [`Theme`] resource instead of hardcoding
//! colours, so a whole new look is a new [`ThemeId`] arm — not new systems.
//! **T** on the main menu cycles themes.

use bevy::color::LinearRgba;
use bevy::prelude::{Color, Resource};

/// Which rig construction builds the player bodies. The animation seam
/// ([`crate::game::animation::AnimClip`] + `MoveIntent` + the root
/// drop/pitch channels) is model-agnostic, so a richer humanoid model plugs
/// in as a new arm here plus its own mesh/pose builders — no system changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PlayerModelId {
    /// The built-in capsule-and-cylinder rig.
    #[default]
    Blocky,
}

/// The full presentation bundle, inserted as a resource.
#[derive(Resource, Clone, Debug)]
pub struct Theme {
    pub ui: UiTheme,
    pub home: PlayerTemplate,
    pub away: PlayerTemplate,
    pub ball: BallTheme,
    /// World clear colour — the sky above the park (bright day or night).
    pub sky: Color,
    /// Which player-model construction dresses the rigs.
    pub player_model: PlayerModelId,
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
                },
                home: PlayerTemplate {
                    jersey: Color::srgb(0.22, 0.42, 0.9),
                    cap: Color::srgb(0.08, 0.14, 0.38),
                    skin: Color::srgb(0.87, 0.67, 0.5),
                    bat: Color::srgb(0.72, 0.5, 0.28),
                },
                away: PlayerTemplate {
                    jersey: Color::srgb(0.88, 0.22, 0.2),
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
                sky: Color::srgb(0.48, 0.67, 0.88),
                player_model: PlayerModelId::Blocky,
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
                sky: Color::srgb(0.02, 0.03, 0.08),
                player_model: PlayerModelId::Blocky,
            },
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle_visits_both_and_wraps() {
        assert_eq!(ThemeId::DaylightClassic.next(), ThemeId::MidnightNeon);
        assert_eq!(ThemeId::MidnightNeon.next(), ThemeId::DaylightClassic);
    }

    #[test]
    fn themes_are_distinct_designs() {
        let (day, night) = (
            ThemeId::DaylightClassic.build(),
            ThemeId::MidnightNeon.build(),
        );
        assert_ne!(
            ThemeId::DaylightClassic.label(),
            ThemeId::MidnightNeon.label()
        );
        assert_ne!(day.ui.accent, night.ui.accent);
        assert_ne!(day.home.jersey, night.home.jersey);
        assert_ne!(day.ball.color, night.ball.color);
        // The ball must actually be enlarged for visibility in every theme.
        assert!(day.ball.visual_scale > 1.5 && night.ball.visual_scale > 1.5);
    }
}
