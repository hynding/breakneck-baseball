//! Big-moment sequences (graphics plan task 10): a short, timed run of
//! shots through the result pause of a home run, a walk-off or a dive, with
//! the instant replay slotted in. Pure: which shot the sequence wants at a
//! moment of the pause, and when its replay starts. The moment itself is
//! read off the play by `game::replay` ([`crate::game::replay::BigMomentCue`]),
//! which also owns the skip; the broadcast rig plays the shots.
//!
//! - **Home run:** the ball-follow takes it to the seats (live), then the
//!   crowd it landed among ([`Shot::CrowdCam`]), then the replay, then the
//!   trot orbit.
//! - **Walk-off** (not a home run, which takes the home run's): the plate
//!   pile-up ([`Shot::PileUp`]) for the whole pause, the replay inside it.
//! - **Dive:** a tight, low angle on the fielder who laid out
//!   ([`Shot::DiveCam`]), then the replay.

use bevy::prelude::*;

use super::shots::Shot;

/// The play worth a sequence.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BigMoment {
    HomeRun,
    WalkOff,
    /// A fielder dove for the ball (the rig that played the dive clip).
    Dive(Entity),
}

/// How long the crowd shot holds before the home run's replay.
pub const CROWD_SECS: f32 = 1.8;
/// How long the dive shot holds before the replay.
pub const DIVE_SECS: f32 = 1.0;
/// When the walk-off's replay starts inside the pile-up shot (the shortest
/// result pause is 1.2 s, so it must start before that).
pub const PILE_UP_REPLAY_SECS: f32 = 1.0;

/// The sequence's shot `elapsed` seconds into the result pause, or `None`
/// once it has handed back to the ordinary shot list (the home run's trot
/// orbit; after a dive, the bag the throw went to, else the ball).
pub fn sequence_shot(moment: BigMoment, elapsed: f32) -> Option<Shot> {
    match moment {
        BigMoment::HomeRun => (elapsed < CROWD_SECS).then_some(Shot::CrowdCam),
        BigMoment::WalkOff => Some(Shot::PileUp),
        BigMoment::Dive(fielder) => (elapsed < DIVE_SECS).then_some(Shot::DiveCam(fielder)),
    }
}

/// Seconds into the result pause the moment's replay starts: after the
/// sequence's opening shot.
pub fn replay_at(moment: BigMoment) -> f32 {
    match moment {
        BigMoment::HomeRun => CROWD_SECS,
        BigMoment::WalkOff => PILE_UP_REPLAY_SECS,
        BigMoment::Dive(_) => DIVE_SECS,
    }
}

#[cfg(test)]
#[path = "sequence.test.rs"]
mod tests;
