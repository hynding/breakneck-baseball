//! Tests for [`super`]: each big moment's shots in order, and its replay
//! slotted after the opening shot.

use super::*;

#[test]
fn a_home_run_cuts_to_the_crowd_then_hands_back() {
    assert_eq!(sequence_shot(BigMoment::HomeRun, 0.0), Some(Shot::CrowdCam));
    assert_eq!(
        sequence_shot(BigMoment::HomeRun, CROWD_SECS - 0.01),
        Some(Shot::CrowdCam)
    );
    assert_eq!(sequence_shot(BigMoment::HomeRun, CROWD_SECS), None);
    assert_eq!(replay_at(BigMoment::HomeRun), CROWD_SECS);
}

#[test]
fn a_walk_off_holds_the_pile_up() {
    for t in [0.0, 1.0, 5.0] {
        assert_eq!(sequence_shot(BigMoment::WalkOff, t), Some(Shot::PileUp));
    }
    assert!(
        replay_at(BigMoment::WalkOff) < 1.2,
        "inside the shortest pause"
    );
}

#[test]
fn a_dive_gets_its_low_angle_then_the_replay() {
    let f = Entity::from_raw_u32(7).unwrap();
    assert_eq!(
        sequence_shot(BigMoment::Dive(f), 0.5),
        Some(Shot::DiveCam(f))
    );
    assert_eq!(sequence_shot(BigMoment::Dive(f), DIVE_SECS), None);
    assert_eq!(replay_at(BigMoment::Dive(f)), DIVE_SECS);
    assert!(DIVE_SECS < 1.2, "inside the shortest pause");
}
