//! Unit tests for [`super`] — the theme module.

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

/// A theme swap must repaint the *whole* effect show, not half of it.
///
/// Before [`FxTheme`] the dust and firework colours were hardcoded in
/// `present/fx/particles.rs`, so switching to the night theme left warm
/// daylight puffs and warm daylight shells on the field (TODO 77). Every
/// channel is compared, so re-hardcoding any one of them fails here.
#[test]
fn every_fx_channel_is_repainted_by_a_theme_swap() {
    let (day, night) = (
        ThemeId::DaylightClassic.build(),
        ThemeId::MidnightNeon.build(),
    );
    assert_ne!(day.fx.ring, night.fx.ring, "landing ring");
    assert_ne!(day.fx.spark, night.fx.spark, "contact sparks / HR halo");
    assert_ne!(day.fx.dust, night.fx.dust, "infield dust");
    assert_ne!(day.fx.fireworks, night.fx.fireworks, "firework shells");
    // Every shell in a show should be a different colour, or the burst
    // reads as one flat blob.
    for theme in [&day, &night] {
        for (i, a) in theme.fx.fireworks.iter().enumerate() {
            for b in &theme.fx.fireworks[i + 1..] {
                assert_ne!(a, b, "duplicate firework shell colour");
            }
        }
    }
}

/// The strike-zone ghost must stay a ghost (nearly transparent, never
/// alpha 0 per the wasm rule) *and* actually contrast its own theme's
/// sky — the original fixed near-black frame vanished at night (TODO 63).
#[test]
fn zone_ghost_reads_against_every_sky() {
    let luminance = |c: Color| {
        let s = c.to_srgba();
        0.2126 * s.red + 0.7152 * s.green + 0.0722 * s.blue
    };
    for id in [ThemeId::DaylightClassic, ThemeId::MidnightNeon] {
        let theme = id.build();
        let frame = theme.ui.zone_frame.to_srgba();
        assert!(
            frame.alpha > 0.0 && frame.alpha <= 0.25,
            "{id:?}: frame should be nearly transparent, got alpha {}",
            frame.alpha
        );
        let fill = theme.ui.zone_fill.to_srgba();
        assert!(
            fill.alpha > 0.0 && fill.alpha < 0.2,
            "{id:?}: fill stays a whisper, got alpha {}",
            fill.alpha
        );
        let contrast = (luminance(theme.ui.zone_frame) - luminance(theme.sky)).abs();
        assert!(
            contrast >= 0.05,
            "{id:?}: zone frame luminance must clear its sky by 0.05, got {contrast}"
        );
    }
}

/// A night game must darken the *ground and the light*, not just the sky —
/// with only the sky, UI, and jerseys swapped, Midnight Neon read as a black
/// void over a sunny field (TODO 96). Daylight stays the textures as
/// painted (a white multiplier), so the day look is untouched by the
/// dressing's existence.
#[test]
fn night_dresses_the_field_darker_and_cooler_than_day() {
    let luminance = |c: Color| {
        let s = c.to_srgba();
        0.2126 * s.red + 0.7152 * s.green + 0.0722 * s.blue
    };
    let (day, night) = (
        ThemeId::DaylightClassic.build(),
        ThemeId::MidnightNeon.build(),
    );
    assert_eq!(day.field.grass_tint, Color::WHITE, "daylight grass as painted");
    assert_eq!(day.field.dirt_tint, Color::WHITE, "daylight dirt as painted");
    assert_eq!(day.field.sun, Color::WHITE);
    assert_eq!(day.field.sun_scale, 1.0);
    for (name, d, n) in [
        ("grass", day.field.grass_tint, night.field.grass_tint),
        ("dirt", day.field.dirt_tint, night.field.dirt_tint),
        ("ambient", day.field.ambient, night.field.ambient),
    ] {
        assert!(
            luminance(n) < luminance(d),
            "{name}: night should be darker than day"
        );
        let s = n.to_srgba();
        assert!(s.blue >= s.red, "{name}: night should be cooler (blue >= red)");
        assert!(luminance(n) > 0.3, "{name}: still a lit park, not a void");
    }
    assert!(
        night.field.sun_scale < day.field.sun_scale && night.field.sun_scale >= 0.4,
        "under the lights, not in the dark: got {}",
        night.field.sun_scale
    );
}
