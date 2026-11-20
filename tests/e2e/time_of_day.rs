//! End-to-end: the time-of-day field option (graphics plan task 12). The
//! menu's choice reaches the park at kickoff: a night game gets the dark
//! sky, a dimmed key light from the key tower and the towers' fill spots,
//! while the afternoon keeps the theme's long-standing day.

use crate::common;

use bevy::prelude::*;

use breakneck_baseball::game::look::is_night;
use breakneck_baseball::game::settings::Settings;
use breakneck_baseball::game::weather::TimeOfDay;

use common::{headless_app, start_game};

/// Starts a one-player game at `time` and returns the clear colour, the key
/// light's illuminance and how many tower fill spots the park has.
fn kickoff(time: TimeOfDay) -> (Color, f32, usize) {
    let mut app = headless_app();
    app.world_mut().resource_mut::<Settings>().time_of_day = time;
    start_game(&mut app, KeyCode::Digit1);
    app.update();
    let world = app.world_mut();
    let sky = world.resource::<ClearColor>().0;
    let key = world
        .query::<&DirectionalLight>()
        .single(world)
        .expect("one key light")
        .illuminance;
    let spots = world.query::<&SpotLight>().iter(world).count();
    (sky, key, spots)
}

#[test]
fn a_night_game_is_played_under_the_lights() {
    let (day_sky, day_key, day_spots) = kickoff(TimeOfDay::Afternoon);
    assert!(!is_night(day_sky), "the afternoon keeps the day sky");
    assert_eq!(day_spots, 0, "no tower spots by day");

    let (night_sky, night_key, night_spots) = kickoff(TimeOfDay::Night);
    assert!(is_night(night_sky), "night darkens the sky");
    assert!(
        night_key < day_key * 0.5,
        "the key tower is well under the sun: {night_key} vs {day_key}"
    );
    assert_eq!(night_spots, 3, "the three non-key towers throw fill spots");
}

/// The towers' fill spots follow the tier: hidden on Low, shown from
/// Medium up, and they track a mid-game change (Auto stepping down).
#[test]
fn the_tower_spots_follow_the_tier() {
    use breakneck_baseball::game::quality::{ActiveQuality, QualityTier};
    use breakneck_baseball::game::settings::GraphicsQuality;
    let mut app = headless_app();
    app.world_mut().resource_mut::<Settings>().time_of_day = TimeOfDay::Night;
    start_game(&mut app, KeyCode::Digit1);
    app.update();
    for (tier, shown) in [
        (QualityTier::Medium, true),
        (QualityTier::Low, false),
        (QualityTier::High, true),
    ] {
        app.insert_resource(ActiveQuality {
            setting: GraphicsQuality::Auto,
            tier,
            stepped: false,
        });
        app.update();
        let world = app.world_mut();
        let seen: Vec<bool> = world
            .query_filtered::<&Visibility, With<SpotLight>>()
            .iter(world)
            .map(|v| *v != Visibility::Hidden)
            .collect();
        assert_eq!(seen.len(), 3, "{tier:?}");
        assert!(seen.iter().all(|&s| s == shown), "{tier:?}: {seen:?}");
    }
}
