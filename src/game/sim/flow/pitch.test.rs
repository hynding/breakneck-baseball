//! Unit tests for [`super`] — the pitch module.

use super::*;

#[test]
fn swing_dt_ms_is_signed_early_negative() {
    // Ball travels toward the plate at −Z (vel_z < 0).
    let vel_z = -30.0;
    // Out in front of the plate (z > PLATE_Z): the swing is early → negative.
    assert!(swing_dt_ms(2.0, vel_z) < 0.0);
    // Already past the plate (z < PLATE_Z): late → positive.
    assert!(swing_dt_ms(-1.0, vel_z) > 0.0);
    // Dead on the plate: zero.
    assert!(swing_dt_ms(PLATE_Z, vel_z).abs() < f32::EPSILON);
}

#[test]
fn swing_dt_ms_never_divides_by_zero() {
    // A stalled or forward-drifting ball is clamped, not a NaN/inf.
    assert!(swing_dt_ms(1.0, 0.0).is_finite());
    assert!(swing_dt_ms(1.0, 5.0).is_finite());
}

#[test]
fn late_swing_z_round_trips_through_swing_dt_ms() {
    // The whole point: the Z it hands back reads back out at exactly
    // `foul_ms` through the same swing_dt_ms the live check uses.
    for vel_z in [-29.0_f32, -31.0, -33.0, -35.0, -38.0] {
        for foul_ms in [90.0_f32, 140.0, 200.0] {
            let z = late_swing_z(vel_z, foul_ms);
            let dt = swing_dt_ms(z, vel_z);
            assert!(
                (dt - foul_ms).abs() < 1e-3,
                "vel_z={vel_z} foul_ms={foul_ms}: late_swing_z={z} -> dt={dt}"
            );
        }
    }
}

#[test]
fn late_swing_z_reaches_far_past_the_old_fixed_cutoff() {
    // The bug this replaces: a fixed −1.2 m cutoff only ever reaches
    // ~40 ms of lateness at game pitch speeds, so no `foul_ms` (140 by
    // default) worth of window was ever geometrically reachable. The
    // derived Z must sit well past that fixed point for every pitch
    // speed in the arsenal (29–38 m/s).
    for vel_z in [-29.0_f32, -31.0, -33.0, -35.0, -38.0] {
        let z = late_swing_z(vel_z, 140.0);
        assert!(
            z < -1.2,
            "vel_z={vel_z}: late_swing_z={z} should reach past the old fixed −1.2 m cutoff"
        );
    }
}

#[test]
fn late_swing_z_never_divides_by_zero() {
    assert!(late_swing_z(0.0, 140.0).is_finite());
    assert!(late_swing_z(5.0, 140.0).is_finite());
}

#[test]
fn bat_arrival_z_looks_the_swing_startup_ahead() {
    // A press now meets the ball where it will be `SWING_CONTACT_SECS`
    // later — closer to the plate by that much flight.
    let vel_z = -30.0;
    let z = bat_arrival_z(6.0, vel_z);
    assert!(
        (z - (6.0 - 30.0 * SWING_CONTACT_SECS)).abs() < 1e-5,
        "arrival z {z}"
    );
    // Pressing exactly one startup ahead of the plate arrives dead-on.
    assert!(swing_dt_at_contact_ms(30.0 * SWING_CONTACT_SECS, vel_z).abs() < 1e-3);
}

#[test]
fn swing_dt_at_contact_is_the_press_error_plus_the_startup() {
    // The two timing reads differ by exactly the startup, whatever the
    // speed: the CPU/Director/Meter press on the contact read so their
    // realized timing at the bat is unchanged by the startup.
    for vel_z in [-29.0_f32, -33.0, -38.0] {
        for z in [8.0_f32, 4.0, 0.0, -2.0] {
            let press = swing_dt_ms(z, vel_z);
            let contact = swing_dt_at_contact_ms(z, vel_z);
            assert!(
                (contact - press - SWING_CONTACT_SECS * 1000.0).abs() < 1e-2,
                "vel_z={vel_z} z={z}: press {press} contact {contact}"
            );
        }
    }
}
