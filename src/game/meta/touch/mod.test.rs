//! Unit tests for [`super`] — the touch module.

use super::*;

const SIZE: Vec2 = Vec2::new(1280.0, 720.0);

#[test]
fn stick_aim_maps_drag_up_to_positive_y_and_clamps() {
    // Drag straight up (screen y decreases) by the full radius.
    let up = stick_aim(Vec2::new(0.0, -STICK_RADIUS_FRAC * SIZE.y), SIZE.y);
    assert!((up - Vec2::Y).length() < 1e-5);
    // A huge drag clamps to the unit circle.
    let big = stick_aim(Vec2::new(4000.0, 4000.0), SIZE.y);
    assert!((big.length() - 1.0).abs() < 1e-5);
}

#[test]
fn tap_aim_center_is_neutral_and_right_is_positive_x() {
    assert!(tap_aim(SIZE * 0.5, SIZE).length() < 1e-5);
    let right = tap_aim(Vec2::new(SIZE.x, SIZE.y * 0.5), SIZE);
    assert!(right.x > 0.99 && right.y.abs() < 1e-5);
    let top = tap_aim(Vec2::new(SIZE.x * 0.5, 0.0), SIZE);
    assert!(top.y > 0.99);
}

#[test]
fn flick_fires_only_past_the_upward_speed_trigger() {
    let dt = 1.0 / 60.0;
    // Slow upward drift: no fire.
    let slow = Vec2::new(0.0, -0.5 * FLICK_TRIGGER_HPS * SIZE.y * dt);
    assert!(flick_fire(slow, dt, SIZE.y).is_none());
    // Fast upward flick: fires, aimed up.
    let fast = Vec2::new(0.0, -2.0 * FLICK_TRIGGER_HPS * SIZE.y * dt);
    let aim = flick_fire(fast, dt, SIZE.y).expect("flick should fire");
    assert!(aim.y > 0.99);
    // A fast *downward* swipe never fires.
    assert!(flick_fire(-fast, dt, SIZE.y).is_none());
    // Angled flick keeps its horizontal component's sign.
    let angled = Vec2::new(fast.y.abs() * 0.5, fast.y);
    let aim = flick_fire(angled, dt, SIZE.y).expect("angled flick fires");
    assert!(aim.x > 0.0 && aim.y > 0.0);
    // Hitch frames cap the bar at the displacement ceiling — pinned in
    // BOTH directions: a sharp flick batched into one stretched frame
    // still fires, while a slow resting-thumb drift across the same
    // frame (whose true speed is far under the trigger) must NOT fire
    // and burn the pitch's one flick.
    let hitch_dt = 0.5;
    let batched = Vec2::new(0.0, -1.5 * FLICK_HITCH_MIN_FRAC * SIZE.y);
    assert!(
        flick_fire(batched, hitch_dt, SIZE.y).is_some(),
        "a hitch frame must not swallow a sharp flick"
    );
    let drift = Vec2::new(0.0, -0.5 * FLICK_HITCH_MIN_FRAC * SIZE.y);
    assert!(
        flick_fire(drift, hitch_dt, SIZE.y).is_none(),
        "a slow drift across a hitch frame must not fire a swing"
    );
    // The bar may only ever LOOSEN as frames stretch: anything that
    // clears the plain speed trigger must fire at every dt. A two-arm
    // version (speed, then a floor past a cutoff) failed exactly here,
    // demanding MORE displacement just past its cutoff than the speed
    // test it replaced.
    for &mild_dt in &[0.05, 0.099, 0.101, 0.15, 0.18, 0.25, 1.0] {
        let just_over_speed = Vec2::new(0.0, -1.01 * FLICK_TRIGGER_HPS * mild_dt * SIZE.y);
        assert!(
            flick_fire(just_over_speed, mild_dt, SIZE.y).is_some(),
            "a gesture past the speed trigger must fire at dt={mild_dt}"
        );
    }
}

#[test]
fn pad_corners_map_to_zone_corners_with_screen_x_negated() {
    let pad = pad_rect(SIZE);
    // Pad top-left: screen-left = world +X (third-base side), zone top.
    let tl = pad_zone_cursor(pad.min, pad);
    assert!((tl.x - rules::ZONE_HALF_WIDTH).abs() < 1e-5);
    assert!((tl.y - rules::ZONE_HIGH).abs() < 1e-5);
    // Pad bottom-right: world −X, zone bottom.
    let br = pad_zone_cursor(pad.max, pad);
    assert!((br.x + rules::ZONE_HALF_WIDTH).abs() < 1e-5);
    assert!((br.y - rules::ZONE_LOW).abs() < 1e-5);
    // Center maps to the adapter's own resting spot — one definition.
    let c = pad_zone_cursor(pad.center(), pad);
    let resting = crate::game::batting::PciState::center();
    assert!((c - resting).length() < 1e-4);
}

#[test]
fn pad_and_swing_button_stay_disjoint_across_aspect_ratios() {
    // Landscape desktop, portrait phone (the shipped failure case at
    // height-fraction sizing), tablet-ish square, and a small landscape.
    for size in [
        SIZE,
        Vec2::new(390.0, 844.0),
        Vec2::new(800.0, 800.0),
        Vec2::new(568.0, 320.0),
    ] {
        let regions = [
            pad_rect(size),
            swing_button_rect(size),
            pause_button_rect(size),
        ];
        for (i, a) in regions.iter().enumerate() {
            for b in regions.iter().skip(i + 1) {
                assert!(
                    a.intersect(*b).is_empty(),
                    "{a:?} overlaps {b:?} at {size:?}"
                );
            }
            // Fully on screen.
            assert!(
                a.min.x >= 0.0 && a.min.y >= 0.0,
                "{a:?} off-screen at {size:?}"
            );
            assert!(
                a.max.x <= size.x && a.max.y <= size.y,
                "{a:?} off-screen at {size:?}"
            );
        }
    }
}
