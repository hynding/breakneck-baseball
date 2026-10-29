//! Unit tests for [`super`] — the physics clock.

use super::*;

/// (total seconds stepped, substep count) for a frame of `delta` seconds,
/// mirroring how bevy_rapier's Variable mode consumes the mode.
fn stepped(delta: f32) -> (f32, usize) {
    let TimestepMode::Variable {
        max_dt,
        time_scale,
        substeps,
    } = timestep_for(delta)
    else {
        panic!("expected a Variable timestep");
    };
    ((delta * time_scale).min(max_dt), substeps)
}

#[test]
fn every_frame_rate_steps_the_whole_delta() {
    for fps in [240.0_f32, 144.0, 60.0, 45.0, 30.0, 20.0, 10.0] {
        let delta = 1.0 / fps;
        let (total, substeps) = stepped(delta);
        assert_eq!(
            total, delta,
            "{fps} fps: physics must cover the whole frame"
        );
        assert!(
            total / substeps as f32 <= MAX_SUBSTEP_SECS,
            "{fps} fps: substep {} s exceeds {MAX_SUBSTEP_SECS}",
            total / substeps as f32
        );
    }
}

#[test]
fn sixty_fps_and_faster_take_one_step() {
    // Includes vsync jitter just over 1/60 s.
    for delta in [1.0 / 240.0, 1.0 / 60.0, 0.0172, 0.0] {
        assert_eq!(stepped(delta).1, 1, "delta {delta}");
    }
}

#[test]
fn thirty_fps_splits_into_two_sixty_hz_steps() {
    assert_eq!(stepped(1.0 / 30.0), (1.0 / 30.0, 2));
}

#[test]
fn a_hitch_is_bounded_but_still_fully_stepped() {
    let (total, substeps) = stepped(0.5);
    assert_eq!(substeps, MAX_SUBSTEPS);
    assert_eq!(total, 0.5);
}
