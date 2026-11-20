//! The replay's pure half: the recording (a ring of sampled poses), how a
//! moment in it is read back, which stretches of it a replay plays, and
//! where its cameras stand. No systems; `replay::mod` drives it.

use std::collections::VecDeque;

use bevy::prelude::*;

use crate::game::variant::FieldSpec;

/// How often the recorder samples (Hz, on the gameplay clock). Playback
/// interpolates between samples, so 30 Hz reads as smooth motion at half
/// the memory of sampling every 60 fps frame.
pub const SAMPLE_HZ: f32 = 30.0;

/// One entity's pose in a sample: local translation and rotation (scale
/// never animates on a rig), and its own visibility where it has one (the
/// batter is hidden once his run-out stand-in takes over, the stand-in
/// until then).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub entity: Entity,
    pub pos: Vec3,
    pub rot: Quat,
    pub vis: Option<Visibility>,
}

/// One sample of the field: when (gameplay seconds), the ball, and every
/// recorded pose, sorted by entity so a lookup is a binary search.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sample {
    pub t: f32,
    pub ball: Option<Vec3>,
    /// The ball was hidden (in a glove, after a catch).
    pub ball_hidden: bool,
    pub poses: Vec<Pose>,
}

impl Sample {
    fn pose(&self, entity: Entity) -> Option<&Pose> {
        self.poses
            .binary_search_by_key(&entity, |p| p.entity)
            .ok()
            .map(|i| &self.poses[i])
    }
}

/// The ring of samples covering the last `keep_secs` of play.
#[derive(Clone, Debug, Default)]
pub struct Recording {
    samples: VecDeque<Sample>,
    keep_secs: f32,
}

/// A moment read back from the recording.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Moment {
    pub ball: Option<Vec3>,
    /// The ball was hidden at that moment (the earlier sample's state).
    pub ball_hidden: bool,
    /// Every entity recorded at that moment and its pose.
    pub poses: Vec<Pose>,
}

impl Recording {
    pub fn new(keep_secs: f32) -> Self {
        Self {
            samples: VecDeque::new(),
            keep_secs,
        }
    }

    /// Seconds of play the ring keeps.
    pub fn keep_secs(&self) -> f32 {
        self.keep_secs
    }

    /// Changes how much the ring keeps (the quality tier changed); older
    /// samples go at the next push.
    pub fn set_keep_secs(&mut self, secs: f32) {
        self.keep_secs = secs;
    }

    /// Whether a sample is due at gameplay time `t`.
    pub fn due(&self, t: f32) -> bool {
        self.samples
            .back()
            .is_none_or(|last| t - last.t >= 1.0 / SAMPLE_HZ - 1e-4 || t < last.t)
    }

    /// Adds a sample (its poses sorted here) and drops the ones older than
    /// the window. A sample from before the newest (the clock restarted:
    /// a new game) starts the ring over.
    pub fn push(&mut self, mut sample: Sample) {
        if self.samples.back().is_some_and(|last| sample.t < last.t) {
            self.samples.clear();
        }
        sample.poses.sort_unstable_by_key(|p| p.entity);
        let newest = sample.t;
        self.samples.push_back(sample);
        while self
            .samples
            .front()
            .is_some_and(|s| newest - s.t > self.keep_secs)
        {
            self.samples.pop_front();
        }
    }

    pub fn clear(&mut self) {
        self.samples.clear();
    }

    /// The first and last sample times, if anything is recorded.
    pub fn span(&self) -> Option<(f32, f32)> {
        Some((self.samples.front()?.t, self.samples.back()?.t))
    }

    /// The field at gameplay time `t` (clamped into the span), interpolated
    /// between the two samples around it. An entity takes the pose of the
    /// earlier sample's set: one that only appears in the later sample is
    /// absent (not yet spawned), one gone from the later sample holds its
    /// last pose.
    pub fn at(&self, t: f32) -> Option<Moment> {
        let (first, last) = self.span()?;
        let t = t.clamp(first, last);
        let i = self.samples.partition_point(|s| s.t <= t).max(1) - 1;
        let a = &self.samples[i];
        let b = self.samples.get(i + 1).unwrap_or(a);
        let f = if b.t > a.t {
            ((t - a.t) / (b.t - a.t)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let poses = a
            .poses
            .iter()
            .map(|pa| match b.pose(pa.entity) {
                Some(pb) => Pose {
                    entity: pa.entity,
                    pos: pa.pos.lerp(pb.pos, f),
                    rot: pa.rot.slerp(pb.rot, f),
                    vis: pa.vis,
                },
                None => *pa,
            })
            .collect();
        let ball = match (a.ball, b.ball) {
            (Some(x), Some(y)) => Some(x.lerp(y, f)),
            (x, y) => x.or(y),
        };
        Some(Moment {
            ball,
            ball_hidden: a.ball_hidden,
            poses,
        })
    }

    /// Samples held.
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Bytes the ring holds (sample payloads, not allocator slack): the
    /// figure the per-tier memory log reports.
    pub fn bytes(&self) -> usize {
        self.samples
            .iter()
            .map(|s| std::mem::size_of::<Sample>() + s.poses.len() * std::mem::size_of::<Pose>())
            .sum()
    }
}

/// Where a replay camera stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Angle {
    /// High beside the first-base dugout, panning with the ball: the play
    /// side-on.
    Dugout,
    /// High behind the infield in centre, looking back in toward the
    /// plate: the reverse angle.
    CentreField,
}

/// One stretch of the recording a replay plays: from which angle, between
/// which gameplay times, and how fast (1.0 = as it happened).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    pub angle: Angle,
    pub from: f32,
    pub to: f32,
    pub speed: f32,
}

impl Segment {
    /// Real seconds the segment takes to play.
    pub fn secs(&self) -> f32 {
        (self.to - self.from).max(0.0) / self.speed
    }
}

/// How long before contact a replay starts: the swing comes into it.
pub const LEAD_IN: f32 = 1.0;
/// How long after the call a replay runs on: the call settles.
pub const TAIL: f32 = 0.4;
/// The slow-motion second angle's half-window around the key moment, and
/// its speed.
pub const SLOW_HALF: f32 = 1.0;
pub const SLOW_SPEED: f32 = 0.5;

/// The replay of a play, as segments: the whole play (from just before
/// `contact` to just after `call`) from the dugout, then — with two angles —
/// again from centre, at half speed around `key` when `slow` (High).
/// Everything clamps into `span`, the recording's reach.
pub fn plan(
    span: (f32, f32),
    contact: f32,
    call: f32,
    key: f32,
    angles: u8,
    slow: bool,
) -> Vec<Segment> {
    let clamp = |t: f32| t.clamp(span.0, span.1);
    let from = clamp(contact - LEAD_IN);
    let to = clamp(call + TAIL);
    if to - from < 0.2 {
        return Vec::new();
    }
    let mut segments = vec![Segment {
        angle: Angle::Dugout,
        from,
        to,
        speed: 1.0,
    }];
    if angles >= 2 {
        segments.push(if slow {
            Segment {
                angle: Angle::CentreField,
                from: clamp(key - SLOW_HALF).max(from),
                to: clamp(key + SLOW_HALF).min(to),
                speed: SLOW_SPEED,
            }
        } else {
            Segment {
                angle: Angle::CentreField,
                from,
                to,
                speed: 1.0,
            }
        });
    }
    segments.retain(|s| s.to - s.from >= 0.2);
    segments
}

/// The narrowest and widest a replay lens gets, and the ground it keeps
/// around the ball (m across the frame's height).
const MIN_FOV: f32 = 14.0_f32.to_radians();
const MAX_FOV: f32 = 60.0_f32.to_radians();
const FRAME_SPAN: f32 = 14.0;

/// The replay camera for `angle` with the ball at `ball` (or the plate
/// when the recording has none): eye, look-at, vertical FOV. The lens
/// keeps about [`FRAME_SPAN`] metres around the ball in shot.
pub fn camera(angle: Angle, field: &FieldSpec, ball: Option<Vec3>) -> (Vec3, Vec3, f32) {
    let first = field.base_positions.first().copied().unwrap_or(Vec3::X);
    let eye = match angle {
        // Out past first base in foul ground, up a storey.
        Angle::Dugout => Vec3::new(first.x * 1.5, 7.0, first.z * 0.35),
        // Behind second, inside the outfield, looking back in.
        Angle::CentreField => Vec3::new(0.0, 12.0, field.fence_line * 0.8),
    };
    let target = ball.unwrap_or(Vec3::new(0.0, 1.0, 0.0));
    let target = target.with_y(target.y.max(0.3));
    let dist = eye.distance(target).max(1.0);
    let fov = (2.0 * (FRAME_SPAN / 2.0 / dist).atan()).clamp(MIN_FOV, MAX_FOV);
    (eye, target, fov)
}

#[cfg(test)]
#[path = "clip.test.rs"]
mod tests;
