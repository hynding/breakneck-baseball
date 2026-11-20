//! Instant replay (graphics plan task 9): a ring buffer of the ball and
//! every rig's pose, played back from one or two broadcast angles after a
//! big play.
//!
//! Presentation only, and it never re-simulates. While a replay runs the
//! gameplay clock (`Time<Virtual>`) is paused, so physics, flow timers,
//! animation and locomotion all hold still; each frame the recorded poses
//! are written over the live transforms after animation and physics have
//! run and before transform propagation, so only the rendered frame sees
//! them, and the live transforms are put back in `Last`. When it ends the
//! clock resumes exactly where it stopped.
//!
//! Which plays get one is the [`ReplayMode`] setting: big plays (a home
//! run, a walk-off, a throw home, a diving catch) by default. Any confirm
//! press skips (the Director's presses too, so scripts and autoplay never
//! stall on one). Buffer length and angle count come from the quality
//! tier's [`RenderBudget`](crate::game::quality::RenderBudget).

pub mod clip;

use bevy::animation::AnimationTargetId;
use bevy::prelude::*;
use bevy::transform::TransformSystems;
use bevy_rapier3d::plugin::PhysicsSet;

use crate::game::GameState;
use crate::game::animation::{AnimClip, Playing, RigLimb};
use crate::game::ball::Baseball;
use crate::game::flow::{LiveBallEvent, Phase, Play};
use crate::game::input::Intents;
use crate::game::player::RigUnitTag;
use crate::game::quality::{ActiveQuality, QualityTier};
use crate::game::settings::{ReplayMode, Settings};
use crate::game::variant::FieldSpec;

use clip::{Moment, Recording, Sample, Segment};

/// How long after the call a replay starts (gameplay seconds): the call
/// banner lands first.
pub const AFTER_CALL: f32 = 0.6;
/// Real seconds into a replay before a press can skip it, so the press that
/// ended the play (or a held button) doesn't skip it unseen.
pub const SKIP_GRACE: f32 = 0.35;

/// The replay on screen, if any. Public so the HUD's REPLAY tag and tests
/// can read it.
#[derive(Resource, Default, Debug)]
pub struct Replay {
    active: Option<Active>,
    /// Replays played this game (tests and the memory log read it).
    pub played: u32,
}

#[derive(Debug)]
struct Active {
    segments: Vec<Segment>,
    seg: usize,
    /// Where playback is in the recording (gameplay seconds).
    clock: f32,
    /// Real seconds since it started.
    shown: f32,
}

impl Replay {
    /// Whether a replay is playing.
    pub fn is_playing(&self) -> bool {
        self.active.is_some()
    }

    /// The segment on screen plays slower than it happened.
    pub fn slow_motion(&self) -> bool {
        self.active
            .as_ref()
            .and_then(|a| a.segments.get(a.seg))
            .is_some_and(|s| s.speed < 1.0)
    }

    /// A press skips the replay now (past the grace that keeps the press
    /// that ended the play from skipping it).
    pub fn skippable(&self) -> bool {
        self.active.as_ref().is_some_and(|a| a.shown >= SKIP_GRACE)
    }
}

/// The recorder's ring.
#[derive(Resource, Default)]
pub struct ReplayRecorder(pub Recording);

/// What the play in progress has done that makes it worth a replay.
#[derive(Resource, Default, Debug)]
struct PlayWatch {
    prev: Option<Phase>,
    /// Contact, on the gameplay clock.
    contact: Option<f32>,
    throw_home: bool,
    dove: bool,
    /// When the play's result pause began, and whether it gets a replay.
    called_at: Option<f32>,
    wanted: bool,
    offered: bool,
}

/// Live transforms and visibilities a replay frame wrote over, put back
/// in `Last`.
#[derive(Resource, Default)]
struct LiveCopy {
    transforms: Vec<(Entity, Transform)>,
    visibility: Vec<(Entity, Visibility)>,
}

type Recorded<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static Transform),
    (
        Or<(With<RigUnitTag>, With<RigLimb>, With<AnimationTargetId>)>,
        Without<Baseball>,
        Without<Camera3d>,
    ),
>;

pub struct ReplayPlugin;

impl Plugin for ReplayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Replay>()
            .init_resource::<ReplayRecorder>()
            .init_resource::<PlayWatch>()
            .init_resource::<LiveCopy>()
            .add_systems(crate::game::game_start(), reset)
            .add_systems(OnEnter(GameState::GameOver), stop)
            .add_systems(OnEnter(GameState::MainMenu), stop)
            .add_systems(
                Update,
                (watch_play, start_replay, advance_replay)
                    .chain()
                    .after(crate::game::GameplayOrder::Runners)
                    .run_if(in_state(GameState::Playing)),
            )
            .add_systems(
                PostUpdate,
                (record, apply_replay)
                    .chain()
                    .after(bevy::app::AnimationSystems)
                    .after(PhysicsSet::Writeback)
                    .before(TransformSystems::Propagate)
                    .run_if(in_state(GameState::Playing)),
            )
            .add_systems(Last, restore_live);
    }
}

fn tier(active: Option<&ActiveQuality>) -> QualityTier {
    active.map_or(QualityTier::Medium, |a| a.tier)
}

fn reset(
    mut replay: ResMut<Replay>,
    mut recorder: ResMut<ReplayRecorder>,
    mut watch: ResMut<PlayWatch>,
) {
    *replay = Replay::default();
    recorder.0.clear();
    *watch = PlayWatch::default();
}

/// Ends any replay and gives the gameplay clock back (the game ended or
/// was quit mid-replay).
fn stop(mut replay: ResMut<Replay>, mut virt: ResMut<Time<Virtual>>) {
    if replay.active.take().is_some() {
        virt.unpause();
    }
}

/// Follows the play: contact, a throw home, a dive, and the call.
#[allow(clippy::too_many_arguments)]
fn watch_play(
    time: Res<Time>,
    play: Res<Play>,
    settings: Res<Settings>,
    field: Res<FieldSpec>,
    replay: Res<Replay>,
    mut watch: ResMut<PlayWatch>,
    mut live: MessageReader<LiveBallEvent>,
    clips: Query<&Playing, Changed<Playing>>,
) {
    if replay.is_playing() {
        live.clear();
        return;
    }
    let now = time.elapsed_secs();
    let entered = watch.prev != Some(play.phase);
    watch.prev = Some(play.phase);
    match play.phase {
        Phase::InPlay => {
            if entered {
                *watch = PlayWatch {
                    prev: Some(Phase::InPlay),
                    contact: Some(now - play.since_contact(now)),
                    ..default()
                };
            }
            for ev in live.read() {
                if let LiveBallEvent::Thrown { base, .. } = *ev
                    && base >= field.base_count()
                {
                    watch.throw_home = true;
                }
            }
            if clips.iter().any(|p| p.clip == AnimClip::Dive) {
                watch.dove = true;
            }
        }
        Phase::Result => {
            live.clear();
            if entered && watch.contact.is_some() {
                watch.called_at = Some(now);
                let big =
                    play.is_home_run() || play.is_walk_off() || watch.throw_home || watch.dove;
                watch.wanted = match settings.replays {
                    ReplayMode::Off => false,
                    ReplayMode::BigPlays => big,
                    ReplayMode::Always => true,
                };
            }
        }
        _ => {
            live.clear();
            if entered {
                watch.contact = None;
                watch.called_at = None;
                watch.wanted = false;
                watch.offered = false;
            }
        }
    }
}

/// Starts the replay a short beat into the result pause, freezing play.
#[allow(clippy::too_many_arguments)]
fn start_replay(
    time: Res<Time>,
    play: Res<Play>,
    quality: Option<Res<ActiveQuality>>,
    recorder: Res<ReplayRecorder>,
    mut watch: ResMut<PlayWatch>,
    mut replay: ResMut<Replay>,
    mut virt: ResMut<Time<Virtual>>,
) {
    let now = time.elapsed_secs();
    let (Some(contact), Some(called)) = (watch.contact, watch.called_at) else {
        return;
    };
    if play.phase != Phase::Result
        || !watch.wanted
        || watch.offered
        || replay.is_playing()
        || now < called + AFTER_CALL
    {
        return;
    }
    watch.offered = true;
    let Some(span) = recorder.0.span() else {
        return;
    };
    let tier = tier(quality.as_deref());
    let budget = tier.budget();
    // The moment worth slowing down: the swing on a home run, else the
    // catch or tag just before the call.
    let key = if play.is_home_run() {
        contact
    } else {
        called - 0.3
    };
    let segments = clip::plan(
        span,
        contact,
        called,
        key,
        budget.replay_angles,
        tier == QualityTier::High,
    );
    let Some(first) = segments.first().copied() else {
        return;
    };
    info!(
        "replay: {:?} tier, {} segment(s), {:.1} s; buffer {} samples, {:.0} KiB",
        tier,
        segments.len(),
        segments.iter().map(Segment::secs).sum::<f32>(),
        recorder.0.len(),
        recorder.0.bytes() as f32 / 1024.0,
    );
    replay.active = Some(Active {
        segments,
        seg: 0,
        clock: first.from,
        shown: 0.0,
    });
    replay.played += 1;
    virt.pause();
}

/// Runs the replay on the real clock; a confirm press (after a short
/// grace) or the last segment's end gives play back.
fn advance_replay(
    real: Res<Time<Real>>,
    intents: Res<Intents>,
    mut replay: ResMut<Replay>,
    mut virt: ResMut<Time<Virtual>>,
) {
    let Some(active) = replay.active.as_mut() else {
        return;
    };
    let dt = real.delta_secs();
    active.shown += dt;
    let skip = active.shown >= SKIP_GRACE && (intents.home.action || intents.away.action);
    let mut done = skip;
    if !done {
        let seg = active.segments[active.seg];
        active.clock += dt * seg.speed;
        if active.clock >= seg.to {
            active.seg += 1;
            match active.segments.get(active.seg) {
                Some(next) => active.clock = next.from,
                None => done = true,
            }
        }
    }
    if done {
        replay.active = None;
        virt.unpause();
    }
}

/// Samples the field into the ring (not while a replay plays: the clock
/// is stopped and the transforms are the replay's).
fn record(
    time: Res<Time>,
    quality: Option<Res<ActiveQuality>>,
    replay: Res<Replay>,
    mut recorder: ResMut<ReplayRecorder>,
    ball: Query<&Transform, With<Baseball>>,
    recorded: Recorded,
) {
    if replay.is_playing() {
        return;
    }
    let keep = tier(quality.as_deref()).budget().replay_secs;
    if recorder.0.keep_secs() != keep {
        recorder.0.set_keep_secs(keep);
    }
    let t = time.elapsed_secs();
    if !recorder.0.due(t) {
        return;
    }
    let sample = Sample {
        t,
        ball: ball.single().ok().map(|tf| tf.translation),
        poses: recorded
            .iter()
            .map(|(entity, tf)| clip::Pose {
                entity,
                pos: tf.translation,
                rot: tf.rotation,
            })
            .collect(),
    };
    recorder.0.push(sample);
}

/// Writes the replay's frame over the live transforms (kept for
/// [`restore_live`]) and points the camera.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn apply_replay(
    replay: Res<Replay>,
    recorder: Res<ReplayRecorder>,
    field: Res<FieldSpec>,
    mut copy: ResMut<LiveCopy>,
    mut transforms: Query<&mut Transform, (Without<Camera3d>, Without<Baseball>)>,
    mut ball: Query<&mut Transform, (With<Baseball>, Without<Camera3d>)>,
    mut roots: Query<(Entity, &mut Visibility), With<RigUnitTag>>,
    mut cam: Query<(&mut Transform, &mut Projection), With<Camera3d>>,
) {
    let Some(active) = replay.active.as_ref() else {
        return;
    };
    let Some(Moment {
        ball: ball_at,
        poses,
    }) = recorder.0.at(active.clock)
    else {
        return;
    };
    for pose in &poses {
        if let Ok(mut tf) = transforms.get_mut(pose.entity) {
            copy.transforms.push((pose.entity, *tf));
            tf.translation = pose.pos;
            tf.rotation = pose.rot;
        }
    }
    // A rig that didn't exist yet at this moment (spawned later in the
    // play) stays out of the shot.
    for (entity, mut vis) in &mut roots {
        if poses.binary_search_by_key(&entity, |p| p.entity).is_err() {
            copy.visibility.push((entity, *vis));
            *vis = Visibility::Hidden;
        }
    }
    if let (Some(at), Ok(mut tf)) = (ball_at, ball.single_mut()) {
        copy.transforms.push((Entity::PLACEHOLDER, *tf));
        tf.translation = at;
    }
    let angle = active.segments[active.seg].angle;
    let (eye, target, fov) = clip::camera(angle, &field, ball_at);
    if let Ok((mut tf, mut projection)) = cam.single_mut() {
        let target = if target.distance(eye) < 0.5 {
            eye + Vec3::Z
        } else {
            target
        };
        *tf = Transform::from_translation(eye).looking_at(target, Vec3::Y);
        if let Projection::Perspective(p) = projection.as_mut() {
            p.fov = fov;
        }
    }
}

/// Puts back what a replay frame wrote over, so gameplay never sees it.
fn restore_live(
    mut copy: ResMut<LiveCopy>,
    mut transforms: Query<&mut Transform, (Without<Camera3d>, Without<Baseball>)>,
    mut ball: Query<&mut Transform, (With<Baseball>, Without<Camera3d>)>,
    mut vis: Query<&mut Visibility>,
) {
    for (entity, live) in copy.transforms.drain(..) {
        if entity == Entity::PLACEHOLDER {
            if let Ok(mut tf) = ball.single_mut() {
                *tf = live;
            }
        } else if let Ok(mut tf) = transforms.get_mut(entity) {
            *tf = live;
        }
    }
    for (entity, live) in copy.visibility.drain(..) {
        if let Ok(mut v) = vis.get_mut(entity) {
            *v = live;
        }
    }
}
