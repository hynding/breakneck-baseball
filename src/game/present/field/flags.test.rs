//! Unit tests for [`super`] — the wind flags.

use super::*;

fn mph(v: Vec3, mph: f32) -> Vec3 {
    v.normalize() * mph * rules::MPS_PER_MPH
}

#[test]
fn a_flag_streams_downwind() {
    for dir in [Vec3::X, Vec3::Z, Vec3::NEG_X, Vec3::new(1.0, 0.0, -1.0)] {
        let wind = mph(dir, 25.0);
        // Average out the flutter over a second.
        let fly: Vec3 = (0..60)
            .map(|i| flag_pose(wind, i as f32 / 60.0, 0.0) * Vec3::X)
            .sum::<Vec3>()
            / 60.0;
        let flat = Vec3::new(fly.x, 0.0, fly.z).normalize();
        assert!(flat.dot(dir.normalize()) > 0.97, "{dir:?}: {fly:?}");
        assert!(fly.y.abs() < 0.1, "{dir:?}: streams level, {fly:?}");
    }
}

#[test]
fn a_calm_flag_hangs_and_a_breeze_lifts_it() {
    let limp = flag_pose(Vec3::ZERO, 0.0, 0.0) * Vec3::X;
    let breeze = flag_pose(mph(Vec3::X, 8.0), 0.0, 0.0) * Vec3::X;
    let gale = flag_pose(mph(Vec3::X, 24.0), 0.0, 0.0) * Vec3::X;
    assert!(limp.y < -0.85, "{limp:?}");
    assert!(limp.y < breeze.y && breeze.y < gale.y);
    assert_eq!(stream(mph(Vec3::X, 40.0)), 1.0);
}

#[test]
fn a_stronger_wind_flutters_more() {
    let swing = |wind: Vec3| {
        let yaws: Vec<f32> = (0..120)
            .map(|i| {
                let fly = flag_pose(wind, i as f32 / 60.0, 0.0) * Vec3::X;
                fly.z.atan2(fly.x)
            })
            .collect();
        let max = yaws.iter().cloned().fold(f32::MIN, f32::max);
        let min = yaws.iter().cloned().fold(f32::MAX, f32::min);
        max - min
    };
    assert!(swing(mph(Vec3::X, 22.0)) > swing(mph(Vec3::X, 6.0)));
}
