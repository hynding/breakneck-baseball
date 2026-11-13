use super::*;

#[test]
fn the_radar_gun_reads_broadcast_speeds() {
    // Every heater reads in the 90s, a curve in the high 70s / low 80s,
    // whatever the pitch's true (slower) flight speed is.
    for i in 0..200 {
        let seed = i as f64 * 0.37;
        let fb = broadcast_reading(PitchKind::Fastball, 1.0, seed);
        assert!((92..=96).contains(&fb), "fastball {fb}");
        let cb = broadcast_reading(PitchKind::Curveball, 1.0, seed);
        assert!((78..=82).contains(&cb), "curveball {cb}");
    }
    // The reading wanders pitch to pitch instead of repeating one number.
    let seen: std::collections::HashSet<i32> = (0..50)
        .map(|i| broadcast_reading(PitchKind::Fastball, 1.0, i as f64 * 1.3))
        .collect();
    assert!(seen.len() >= 3, "{seen:?}");
    // The pace dial scales it: a faster game reads faster.
    assert!(
        broadcast_reading(PitchKind::Fastball, 1.2, 0.0)
            > broadcast_reading(PitchKind::Fastball, 1.0, 0.0) + 10
    );
}
