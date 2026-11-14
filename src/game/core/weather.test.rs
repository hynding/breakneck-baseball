//! Unit tests for [`super`] — weather and fence presets.

use super::*;
use crate::game::variant::VariantId;

#[test]
fn the_defaults_are_the_game_before_conditions() {
    let fair = Weather::default().spec();
    assert_eq!(fair.temp_f, NEUTRAL_TEMP_F);
    assert_eq!(fair.wind_mph, (2.0, 14.0));
    assert!(!fair.rain);
    assert_eq!(Conditions::default().carry(), 1.0);
    let standard = VariantId::Standard.field();
    let applied = Fences::default().apply(standard.clone());
    assert_eq!(applied.fence_line, standard.fence_line);
    assert_eq!(applied.fence_center, standard.fence_center);
    assert_eq!(applied.bounds, standard.bounds);
}

#[test]
fn warm_air_carries_and_cold_air_holds_the_ball_up() {
    let hot = Conditions::for_weather(Weather::Hot).carry();
    let cold = Conditions::for_weather(Weather::Cold).carry();
    assert!(hot > 1.0 && cold < 1.0);
    // ~1% of carry per 10°F, half of it on the exit speed: a 95°F day is
    // about +1.25% exit speed, a 42°F one about -1.4%.
    assert!((hot - 1.0125).abs() < 1e-4, "{hot}");
    assert!((cold - 0.986).abs() < 1e-4, "{cold}");
    assert_eq!(Conditions::for_weather(Weather::Hot).temp_report(), "95F");
}

#[test]
fn every_weather_cycles_and_has_a_sane_spec() {
    let mut w = Weather::Fair;
    for _ in 0..Weather::ALL.len() {
        let s = w.spec();
        assert!((30.0..=105.0).contains(&s.temp_f), "{w:?}");
        assert!(s.wind_mph.0 <= s.wind_mph.1, "{w:?}");
        assert!((0.0..=1.0).contains(&s.overcast), "{w:?}");
        assert!(s.sun > 0.0, "{w:?}");
        w = w.next();
    }
    assert_eq!(w, Weather::Fair);
    assert_eq!(Weather::Calm.spec().wind_mph, (0.0, 0.0));
    assert!(Weather::Rain.spec().rain);
}

#[test]
fn fences_move_the_wall_to_real_park_depths() {
    let ft = |m: f32| m / 0.3048;
    let standard = VariantId::Standard.field();
    let cozy = Fences::Cozy.apply(standard.clone());
    let deep = Fences::Deep.apply(standard.clone());
    // Per docs/BASEBALL.md ("Field dimensions"): cozy parks ~310/380 ft,
    // spacious ones ~345/420 ft.
    assert!((300.0..=320.0).contains(&ft(cozy.fence_line)));
    assert!((370.0..=390.0).contains(&ft(cozy.fence_center)));
    assert!((335.0..=355.0).contains(&ft(deep.fence_line)));
    assert!((410.0..=430.0).contains(&ft(deep.fence_center)));
    // A deep park keeps the ball-loss radius beyond its wall.
    assert!(deep.bounds > standard.bounds);
    assert_eq!(cozy.bounds, standard.bounds);
    let mut f = Fences::Standard;
    for _ in 0..3 {
        f = f.next();
    }
    assert_eq!(f, Fences::Standard);
}
