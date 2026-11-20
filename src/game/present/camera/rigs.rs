//! Camera rigs: the broadcast and orbit systems that actually move the
//! `Camera3d`, plus the occlusion pass that hides whoever is standing in
//! the broadcast lens's way. Reads the shot catalog in [`super::shots`]
//! (which shot, its framing, how to move between shots), the pure math in
//! [`super::framing`], and the mode/duel-view state owned by [`super`].

use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy_rapier3d::prelude::Velocity;

use crate::game::ScoreBoard;
use crate::game::ball::{Baseball, HitEvent, WallBangEvent};
use crate::game::fielding::ActivePlay;
use crate::game::flow::{LiveBallEvent, Phase, Play, ResultBeat};
use crate::game::player::{CatcherRole, PlateUmpire};
use crate::game::rules;
use crate::game::runner::{BasePath, Departed};
use crate::game::settings::Settings;
use crate::game::variant::FieldSpec;

use super::framing::{LENS_BRUSH_RADIUS, OCCLUSION_NEAR, OCCLUSION_RADIUS, brushes_lens, occludes};
use super::sequence::sequence_shot;
use super::shots::{
    BallState, FIELDER_CAM_MIN_HEIGHT, PlayRead, RUNNER_CAM_MIN_START_SECS, RigMotion, RunnerFrame,
    Shot, ShotCues, ShotInputs, pick_runner_subject, pick_shot, runner_cam_wanted, throw_secs_left,
};
use super::{
    BALL_FOLLOW_DELAY, BROADCAST_EYE, BROADCAST_FOV, BROADCAST_HOME_TARGET, CameraMode,
    DUEL_REFERENCE_ASPECT, DuelView,
};
use crate::game::replay::BigMomentCue;

// ── Occlusion ─────────────────────────────────────────────────────────────────

/// Hides the catcher/plate umpire root(s) that sit in the way of the active
/// duel view for as long as they do, and restores them the rest of the
/// time — outside the duel phases (ball in play, result pause) or on a view
/// change that clears the block. Root-level `Visibility` is the same
/// mechanism run-out rigs use to swap the batter for his stand-in
/// (`runner.rs`), so this never fights that: it only ever touches
/// `CatcherRole`/`PlateUmpire` roots, which never run bases.
///
/// The catcher-POV view gets its own arm: its eye sits fractionally
/// *inside* the catcher's silhouette (see `FieldSpec::duel_eye`), behind
/// his forward surface, where the `occludes` cone (which only looks ahead
/// of the eye) can't see him — so in that view the catcher is hidden
/// outright whenever the duel framing is wanted or still held.
#[allow(clippy::type_complexity)]
pub(super) fn hide_occluders(
    view: Res<DuelView>,
    field: Res<FieldSpec>,
    rig: Res<BroadcastRig>,
    mode: Res<CameraMode>,
    mut subjects: Query<
        (&Transform, &mut Visibility, Has<CatcherRole>),
        Or<(With<CatcherRole>, With<PlateUmpire>)>,
    >,
) {
    // While the lens is parked at the plate — the duel itself, the
    // post-contact hold, and a gloved pitch's result pause — nothing may
    // pop into it. Gating on the duel phases alone let the plate umpire
    // stand up out of his crouch straight into the parked catcher-POV lens
    // during the result pause (playtest 2026-08-20).
    // The rig's shot is the source of truth: the duel framing is "held"
    // exactly while a `Shot::Duel` is on screen (the duel itself, the
    // post-contact hold, a gloved pitch's result pause).
    let framing_held = *mode == CameraMode::Broadcast && matches!(rig.shot(), Some(Shot::Duel(_)));
    let pov_at_plate = framing_held && *view == DuelView::CatcherPov;
    // The FOV this call computes is discarded (occlusion only cares about the
    // eye/target axis), so the aspect passed through doesn't matter — the
    // reference aspect keeps this a no-op correction.
    let (eye, target, _) = view.framing(&field, DUEL_REFERENCE_ASPECT);
    for (transform, mut visibility, _is_catcher) in &mut subjects {
        // Occlusion only makes sense for the camera actually looking through
        // this axis: in Orbit the player is free-looking with a completely
        // different eye/target, so a rig hidden for a Broadcast duel view
        // must not stay hidden just because the view resource hasn't
        // changed — the system still runs every frame (unconditionally, not
        // gated out entirely) so switching back to Broadcast, or into
        // Orbit, both re-evaluate and settle on the right state immediately.
        //
        // Catcher-POV hides both plate rigs outright: the eye sits inside
        // the catcher's silhouette with the umpire crouched *behind* it,
        // where a look-ahead cone can never flag him, yet his geometry
        // pokes through the near plane.
        // Any tight view also hides a body the lens is parked against —
        // beside or behind the eye, where the look-ahead cone is blind
        // (the front-yard umpire under the default batting view).
        let blocking = pov_at_plate
            || (framing_held
                && (occludes(
                    eye,
                    target,
                    transform.translation,
                    OCCLUSION_NEAR,
                    OCCLUSION_RADIUS,
                ) || brushes_lens(eye, transform.translation, LENS_BRUSH_RADIUS)));
        let desired = if blocking {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        if *visibility != desired {
            *visibility = desired;
        }
    }
}

// ── Orbit state ───────────────────────────────────────────────────────────────

#[derive(Resource)]
pub struct OrbitState {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub target: Vec3,
}

impl Default for OrbitState {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: 0.6,
            distance: 60.0,
            target: Vec3::new(0.0, 0.0, 30.0),
        }
    }
}

/// The broadcast camera's state: the shot on screen, and the lens that
/// tracks *within* it and moves *between* shots ([`RigMotion`]).
#[derive(Resource)]
pub struct BroadcastRig {
    motion: RigMotion,
    shot: Option<Shot>,
    /// The base the live ball was last thrown at, cleared once the shot
    /// returns to the duel.
    thrown_base: Option<usize>,
    /// Where that throw left the fielder's hand.
    throw_from: Option<Vec3>,
    /// The runner the runner cam settled on for this throw and the bag he
    /// is bound for, kept so the shot never hops between runners (and
    /// still frames him once his run ends).
    runner_subject: Option<(Entity, Vec3)>,
    /// The runner cam has been on screen for this throw: once it has, the
    /// play never waits on a runner again (it goes to the bag).
    runner_shown: bool,
    /// What this play's batted ball has done: the grounder call (made once
    /// just after the plate hold) and whether it has come down
    /// ([`PlayRead`]); cleared with `thrown_base`.
    read: PlayRead,
    /// Where the ground-ball shot last saw the chasing fielder, held once
    /// the chase ends so the framing doesn't jump as he gathers the ball.
    ground_fielder: Option<Vec3>,
    /// The contact (on the virtual clock) the per-play state above belongs
    /// to; a new contact clears it, even one this camera never saw the
    /// lead-up to (the orbit view was on between plays).
    play_contact: Option<f32>,
    /// The half-inning whose opening cutaway has started, and when (on the
    /// virtual clock; [`super::shots::half_inning_cutaway`]).
    cut_half: super::shots::CutHalf,
}

impl Default for BroadcastRig {
    fn default() -> Self {
        Self {
            motion: RigMotion::new(BROADCAST_EYE, BROADCAST_HOME_TARGET, BROADCAST_FOV),
            shot: None,
            thrown_base: None,
            throw_from: None,
            runner_subject: None,
            runner_shown: false,
            read: PlayRead::default(),
            ground_fielder: None,
            play_contact: None,
            cut_half: None,
        }
    }
}

impl BroadcastRig {
    /// The shot currently on screen (`None` before the first frame).
    pub fn shot(&self) -> Option<Shot> {
        self.shot
    }
}

/// Impulse added to the broadcast eye on contact; decays on real time so the
/// kick rides through the hit-stop.
#[derive(Resource, Default)]
pub(super) struct CameraKick(pub(super) Vec3);

/// A base path ending this close to the thrown-to bag (m, flat) is bound
/// for it.
const RUNNER_ARRIVED: f32 = 1.0;

/// The live ball as the broadcast camera reads it.
type BallQuery<'w, 's> =
    Query<'w, 's, (&'static Transform, &'static Velocity), (With<Baseball>, Without<Camera3d>)>;

/// The kick on contact and off the wall at full shake.
pub(super) const HIT_KICK: Vec3 = Vec3::new(0.0, 0.18, -0.35);
pub(super) const WALL_KICK: Vec3 = Vec3::new(0.0, 0.10, 0.20);

/// The share of the authored kick the CAMERA SHAKE setting lets through:
/// none under reduce motion, full with no settings (as the harnesses
/// always had).
pub(super) fn shake(settings: Option<&Settings>) -> f32 {
    settings.map_or(1.0, |s| {
        if s.reduce_motion {
            0.0
        } else {
            s.camera_shake.scale()
        }
    })
}

pub(super) fn kick_on_hit(
    mut hits: MessageReader<HitEvent>,
    settings: Option<Res<Settings>>,
    mut kick: ResMut<CameraKick>,
) {
    let k = shake(settings.as_deref());
    for _ in hits.read() {
        kick.0 += HIT_KICK * k;
    }
}

/// A smaller thump when the ball bangs off the outfield wall.
pub(super) fn kick_on_wall_bang(
    mut bangs: MessageReader<WallBangEvent>,
    settings: Option<Res<Settings>>,
    mut kick: ResMut<CameraKick>,
) {
    let k = shake(settings.as_deref());
    for _ in bangs.read() {
        kick.0 += WALL_KICK * k;
    }
}

pub(super) fn decay_kick(real: Res<Time<Real>>, mut kick: ResMut<CameraKick>) {
    kick.0 *= (-14.0 * real.delta_secs()).exp();
}

/// Runners on a base path, and rigs that have left the field.
type RunnerPaths<'w, 's> = (
    Query<'w, 's, (Entity, &'static BasePath)>,
    Query<'w, 's, (), With<Departed>>,
);

#[allow(clippy::too_many_arguments)]
pub(super) fn broadcast_camera(
    time: Res<Time>,
    play: Res<Play>,
    field: Res<FieldSpec>,
    view: Res<DuelView>,
    // (Paired to stay inside Bevy's 16-parameter system limit.)
    (kick, settings, score): (Res<CameraKick>, Option<Res<Settings>>, Res<ScoreBoard>),
    active: Res<ActivePlay>,
    wind: Res<rules::Wind>,
    mut live: MessageReader<LiveBallEvent>,
    mut walls: MessageReader<WallBangEvent>,
    cue: Option<Res<BigMomentCue>>,
    ball_q: BallQuery,
    rigs: Query<&Transform, (Without<Baseball>, Without<Camera3d>)>,
    // (Paired to stay inside Bevy's 16-parameter system limit.)
    (paths, departed): RunnerPaths,
    mut rig: ResMut<BroadcastRig>,
    mut trot_start: Local<Option<f32>>,
    mut cam_q: Query<(&mut Transform, &mut Projection), With<Camera3d>>,
) {
    let cue = cue.as_deref().copied().unwrap_or_default();
    // The camera's actual aspect ratio (width / height), read before the
    // framing decision so the duel FOV can correct for it — see
    // `aspect_safe_duel_vfov`. Falls back to the reference aspect (a no-op
    // correction) before the camera exists.
    let aspect = match cam_q.single() {
        Ok((_, Projection::Perspective(persp))) => persp.aspect_ratio,
        _ => DUEL_REFERENCE_ASPECT,
    };

    // A throw is the cue for the base cam: the race is decided at the
    // throw and announced at the arrival (`flow::resolve_live_play`), so
    // this is exactly the shot the call lands in.
    for ev in live.read() {
        if let LiveBallEvent::Thrown { pos, base, .. } = *ev {
            rig.thrown_base = Some(base);
            rig.throw_from = Some(pos);
            rig.runner_subject = None;
            rig.runner_shown = false;
        }
    }

    let now = time.elapsed_secs();
    let ball = ball_q.single().ok();
    let ball_state = ball.map(|(tf, vel)| BallState {
        pos: tf.translation,
        vel: vel.linvel,
        spin: vel.angvel,
    });
    // Between plays nothing carries over — also when this camera was off
    // (the orbit view) between plays and never saw the pitch.
    let contact = now - play.since_contact(now);
    let new_play =
        play.phase == Phase::InPlay && rig.play_contact.is_none_or(|c| (c - contact).abs() > 0.01);
    if new_play {
        rig.play_contact = Some(contact);
    }
    if new_play || !matches!(play.phase, Phase::InPlay | Phase::Result) {
        rig.thrown_base = None;
        rig.throw_from = None;
        rig.runner_subject = None;
        rig.runner_shown = false;
        rig.read = PlayRead::default();
        rig.ground_fielder = None;
    }
    // Watch the batted ball from contact; just after the plate hold it is
    // called a grounder or not, and the call holds for the play.
    let off_wall = walls.read().count() > 0;
    if play.phase == Phase::InPlay {
        let ready = !play.is_home_run() && play.since_contact(now) >= BALL_FOLLOW_DELAY;
        rig.read
            .observe(ready, ball_state, active.landed_at(), off_wall, wind.0);
    }
    let grounder = rig.read.grounder() == Some(true);
    let landed = rig.read.landed();
    let descending_to = ball.and_then(|(tf, vel)| {
        // A chopper's high hop is still a grounder's play, and a fly that
        // has already come down is not caught on the way down: no fielder
        // cam for either.
        let coming_down =
            !grounder && !landed && tf.translation.y > FIELDER_CAM_MIN_HEIGHT && vel.linvel.y < 0.0;
        active
            .chaser()
            .filter(|f| coming_down && rigs.get(*f).is_ok())
    });
    // A long throw in from the outfield first trails the runner racing it
    // (the one bound for the thrown-to bag), then hands over to the base
    // cam for the last stretch and the call.
    let bag_at = |base: usize| {
        field
            .base_positions
            .get(base)
            .copied()
            .unwrap_or(Vec3::ZERO)
    };
    // (A throw already in the receiver's glove is no long throw: the base
    // cam takes it, whatever the clock says.)
    let long_throw = match (rig.thrown_base, rig.throw_from, ball_state) {
        (Some(base), Some(from), Some(b))
            if play.phase == Phase::InPlay
                && !play.is_home_run()
                && active.possessor().is_none() =>
        {
            runner_cam_wanted(
                from,
                bag_at(base),
                b.pos,
                b.vel,
                field.fence_line,
                matches!(rig.shot, Some(Shot::BaseCam(_))),
            )
        }
        _ => false,
    };
    // Pick the subject only once the call is decided: that is when the
    // runners are re-pathed for it. Flow runs before Fielding, so it reads
    // the throw and decides the call the frame after it; the re-path's
    // commands are flushed before this system (it runs after
    // `GameplayOrder::Rigs`), so the pick lands on that second frame. The
    // one frame between holds the shot already up (ball-follow or the
    // ground-ball shot), so nothing flashes.
    let runner_cam = if long_throw && play.pending_call().is_some() {
        let base = rig.thrown_base.expect("long_throw needs a throw");
        let thrown_bag = bag_at(base);
        // Picked once per throw, then kept (a runner on a triple races on
        // past the bag the throw went to). A subject that is gone — a
        // runner who scored leaves the field at the plate — ends the runner
        // cam rather than cutting to someone else.
        let subject = match rig.runner_subject {
            Some((e, dest)) => (rigs.get(e).is_ok() && !departed.contains(e)).then_some((e, dest)),
            None if rig.runner_shown => None,
            // Too little of the throw left to be worth a shot of its own:
            // release the hold and let the base cam have it.
            None if ball_state.is_some_and(|b| {
                throw_secs_left(thrown_bag, b.pos, b.vel) < RUNNER_CAM_MIN_START_SECS
            }) =>
            {
                rig.runner_shown = true;
                None
            }
            // Every rig still running a base path — runners aboard, the
            // batter, a runner scoring or about to be cut down.
            None => pick_runner_subject(
                paths.iter().filter_map(|(e, path)| {
                    Some((e, rigs.get(e).ok()?.translation, path.destination()?))
                }),
                thrown_bag,
                RUNNER_ARRIVED,
            ),
        };
        if subject.is_some() {
            rig.runner_subject = subject;
        }
        subject.map(|(e, _)| e)
    } else {
        None
    };
    // Until the call is decided, or while its runner hasn't surfaced, keep
    // the base cam back (the shot stays on the ball: the chase is over, so
    // no fielder cam) rather than cut to the bag and straight back.
    let thrown_base = if long_throw && runner_cam.is_none() && !rig.runner_shown {
        None
    } else {
        rig.thrown_base
    };
    let (cut_half, cutaway) = super::shots::half_inning_cutaway(
        play.walkup_active() && play.phase == Phase::PrePitch,
        (score.inning, score.top_of_inning),
        now,
        rig.cut_half,
    );
    rig.cut_half = cut_half;
    let cues = ShotCues {
        phase: play.phase,
        cutaway,
        since_contact: play.since_contact(now),
        home_run: play.is_home_run(),
        walk_off: play.is_walk_off(),
        pitch_gloved: play.pitch_gloved(),
        view: *view,
        walkup: play.walkup_active(),
        strikeout: play.result_beat() == Some(ResultBeat::Strikeout),
        thrown_base,
        grounder,
        runner_cam,
        descending_to,
        duel_throw_base: play.duel_throw().map(|t| t.target),
        sequence: cue
            .playing()
            .filter(|_| play.phase == Phase::Result)
            .and_then(|m| sequence_shot(m, cue.since_call(now))),
    };
    let shot = pick_shot(&cues, rig.shot);

    let start = *trot_start.get_or_insert(now);
    let inputs = ShotInputs {
        field: &field,
        aspect,
        ball: ball_state,
        fielder: match shot {
            Shot::FielderCam(f) | Shot::DiveCam(f) => rigs.get(f).ok().map(|tf| tf.translation),
            Shot::GroundBall => {
                let chaser = active
                    .chaser()
                    .and_then(|f| rigs.get(f).ok())
                    .map(|tf| tf.translation);
                rig.ground_fielder = chaser.or(rig.ground_fielder);
                rig.ground_fielder
            }
            _ => None,
        },
        runner: match (shot, rig.runner_subject, rig.thrown_base) {
            (Shot::RunnerCam(r), Some((_, dest)), Some(base)) => {
                rigs.get(r).ok().map(|tf| RunnerFrame {
                    runner: tf.translation,
                    bound_for: dest,
                    call_bag: bag_at(base),
                })
            }
            _ => None,
        },
        wind: wind.0,
        home_run: play.is_home_run(),
        trot_secs: now - start,
        duel_fov_nudge: f32::from(settings.as_ref().map_or(0, |s| s.duel_fov)).to_radians(),
        still: settings.as_ref().is_some_and(|s| s.reduce_motion),
        exit: cue.exit,
    };
    let want = shot.spec(&inputs);

    // The trot seed lives only while the trot shot does.
    if shot != Shot::TrotOrbit {
        *trot_start = None;
    }
    if matches!(shot, Shot::Duel(_)) {
        // (Not the play read: the plate hold is a duel frame too, and the
        // read watches the ball through it.)
        rig.thrown_base = None;
    }

    if matches!(shot, Shot::RunnerCam(_)) {
        rig.runner_shown = true;
    }
    if rig.shot != Some(shot) {
        // A new subject: the shot says how to get there (a cut, today).
        let reduce = settings.as_ref().is_some_and(|s| s.reduce_motion);
        let kind = super::shots::motion_safe(shot.transition_from(rig.shot), reduce);
        rig.shot = Some(shot);
        rig.motion.start(kind, want, time.delta_secs());
    } else {
        rig.motion.step(want, time.delta_secs());
    }

    if let Ok((mut cam, mut projection)) = cam_q.single_mut() {
        let m = rig.motion;
        *cam = Transform::from_translation(m.eye + kick.0).looking_at(m.target, Vec3::Y);
        if let Projection::Perspective(persp) = projection.as_mut() {
            persp.fov = m.fov;
        }
    }
}

// ── Orbit camera (free look) ──────────────────────────────────────────────────

/// The orbit's key controls only engage while a Shift is held: WASD/arrows
/// are the *gameplay* aim keys for both players, and the free-look camera
/// stealing them mid-play meant spinning the camera with every pitch aim
/// (TODO 64). The mouse wheel (see [`zoom_camera`]) stays modifier-free —
/// it has no gameplay meaning.
fn orbit_modifier_held(keyboard: &ButtonInput<KeyCode>) -> bool {
    keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight)
}

pub(super) fn orbit_camera(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut orbit: ResMut<OrbitState>,
    mut camera_query: Query<&mut Transform, With<Camera3d>>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();
    let yaw_speed = 1.2_f32;
    let pitch_speed = 0.8_f32;

    let mut yaw_delta = 0.0_f32;
    let mut pitch_delta = 0.0_f32;

    if orbit_modifier_held(&keyboard) {
        if keyboard.pressed(KeyCode::ArrowLeft) || keyboard.pressed(KeyCode::KeyA) {
            yaw_delta -= yaw_speed * dt;
        }
        if keyboard.pressed(KeyCode::ArrowRight) || keyboard.pressed(KeyCode::KeyD) {
            yaw_delta += yaw_speed * dt;
        }
        if keyboard.pressed(KeyCode::ArrowUp) || keyboard.pressed(KeyCode::KeyW) {
            pitch_delta += pitch_speed * dt;
        }
        if keyboard.pressed(KeyCode::ArrowDown) || keyboard.pressed(KeyCode::KeyS) {
            pitch_delta -= pitch_speed * dt;
        }
    }

    orbit.yaw += yaw_delta;
    orbit.pitch = (orbit.pitch + pitch_delta).clamp(0.1, std::f32::consts::FRAC_PI_2 - 0.05);

    if orbit_modifier_held(&keyboard) && keyboard.just_pressed(KeyCode::KeyR) {
        *orbit = OrbitState::default();
    }

    let transform = orbit_transform(&orbit);
    for mut cam_transform in &mut camera_query {
        ease_toward(&mut cam_transform, &transform, dt);
    }
}

/// Exponential ease used by both orbit writers: the broadcast rig glides
/// between framings, but C used to hard-cut into (and out of) orbit because
/// these systems assigned the transform directly (TODO 69). Fast enough to
/// feel 1:1 under held keys, soft enough that the mode switch reads as a
/// move.
fn ease_toward(current: &mut Transform, target: &Transform, dt: f32) {
    let s = 1.0 - (-8.0 * dt).exp();
    current.translation = current.translation.lerp(target.translation, s);
    current.rotation = current.rotation.slerp(target.rotation, s);
}

pub(super) fn zoom_camera(
    mut scroll: MessageReader<MouseWheel>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut orbit: ResMut<OrbitState>,
    mut camera_query: Query<&mut Transform, With<Camera3d>>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();

    let mut zoom_delta = 0.0_f32;
    for ev in scroll.read() {
        zoom_delta -= ev.y * 3.0;
    }
    // Q/E ride the same Shift modifier as the orbit keys (TODO 64); the
    // wheel needs none.
    if orbit_modifier_held(&keyboard) {
        if keyboard.pressed(KeyCode::KeyQ) {
            zoom_delta -= 15.0 * dt;
        }
        if keyboard.pressed(KeyCode::KeyE) {
            zoom_delta += 15.0 * dt;
        }
    }

    orbit.distance = (orbit.distance + zoom_delta).clamp(10.0, 200.0);

    let transform = orbit_transform(&orbit);
    for mut cam_transform in &mut camera_query {
        ease_toward(&mut cam_transform, &transform, dt);
    }
}

fn orbit_transform(orbit: &OrbitState) -> Transform {
    let offset = Vec3::new(
        orbit.distance * orbit.yaw.sin() * orbit.pitch.cos(),
        orbit.distance * orbit.pitch.sin(),
        orbit.distance * orbit.yaw.cos() * orbit.pitch.cos(),
    );
    Transform::from_translation(orbit.target + offset).looking_at(orbit.target, Vec3::Y)
}
