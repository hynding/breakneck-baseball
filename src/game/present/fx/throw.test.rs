//! Unit tests for [`super`] — the running-game throw's flight.

use super::*;

#[test]
fn the_arc_starts_in_the_hand_and_ends_in_the_glove() {
    let from = Vec3::new(0.0, 1.2, 18.0);
    let to = Vec3::new(-19.4, 1.0, 19.4);
    assert!(throw_arc(from, to, 0.0).distance(from) < 1e-5);
    assert!(throw_arc(from, to, 1.0).distance(to) < 1e-5);
    // Out-of-range fractions pin to the ends rather than overshooting.
    assert!(throw_arc(from, to, 1.5).distance(to) < 1e-5);
}

#[test]
fn the_arc_is_a_firm_throw_not_a_lob() {
    let from = Vec3::new(0.0, 1.2, 0.0);
    let to = Vec3::new(0.0, 1.0, 38.8);
    let mid = throw_arc(from, to, 0.5);
    let line = from.lerp(to, 0.5);
    let lift = mid.y - line.y;
    assert!(lift > 0.5 && lift < 4.0, "peak lift {lift}");
}
