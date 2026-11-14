//! Unit tests for [`super`] — rain.

use super::*;
use crate::game::weather::Weather;

fn app_with(weather: Weather) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .insert_resource(Settings {
            weather,
            ..Default::default()
        })
        .add_systems(Startup, spawn_rain)
        .add_systems(Update, fall_rain);
    app
}

fn drops(app: &mut App) -> Vec<f32> {
    let mut q = app.world_mut().query::<&RainDrop>();
    q.iter(app.world()).map(|d| d.offset.y).collect()
}

#[test]
fn only_a_rainy_day_rains() {
    let mut dry = app_with(Weather::Fair);
    dry.update();
    assert!(drops(&mut dry).is_empty());
    let mut wet = app_with(Weather::Rain);
    wet.update();
    assert_eq!(drops(&mut wet).len(), DROPS);
}

#[test]
fn drops_fall_and_wrap_inside_the_box() {
    let mut app = app_with(Weather::Rain);
    app.world_mut()
        .spawn((MainCamera, GlobalTransform::default()));
    app.update();
    for _ in 0..200 {
        app.update();
    }
    for y in drops(&mut app) {
        assert!((0.0..=HEIGHT).contains(&y), "{y}");
    }
}

#[test]
fn the_spread_fills_the_unit_interval() {
    let xs: Vec<f32> = (0..DROPS).map(|i| spread(i, 0)).collect();
    assert!(xs.iter().all(|x| (0.0..=1.0).contains(x)));
    assert!(xs.iter().any(|x| *x < 0.2) && xs.iter().any(|x| *x > 0.8));
}
