//! The game intro (graphics plan task 15): on PLAY BALL the camera flies
//! over the park for [`INTRO_SECS`], from high over centre field round the
//! third-base side to behind the plate, and the lineup card
//! (`ui::lineup`) comes up for its second half.
//!
//! Presentation only, the same way a replay is: while it runs the gameplay
//! clock (`Time<Virtual>`) is paused, so nothing in the game moves, and the
//! flyover writes the camera after the broadcast rig and before transform
//! propagation; the clock resumes exactly where it stopped. Any action press
//! after a short grace skips it, and a [`Director`] (scripts, autoplay, the
//! balance sim) skips it outright. The headless test harness inserts
//! [`IntroDisabled`] so no e2e has to wait it out. Reduce motion holds one
//! still frame instead of flying.

use std::f32::consts::PI;

use bevy::prelude::*;
use bevy::transform::TransformSystems;

use crate::game::GameState;
use crate::game::director::Director;
use crate::game::field::stands_height_under;
use crate::game::input::Intents;
use crate::game::settings::Settings;
use crate::game::variant::FieldSpec;

/// How long the flyover runs (real seconds).
pub const INTRO_SECS: f32 = 5.0;
/// Real seconds in before the lineup card comes up (it then stays up for
/// the last 3.2 s, long enough to read).
pub const LINEUP_FROM: f32 = 1.8;
const _: () = assert!(INTRO_SECS - LINEUP_FROM >= 3.0);
/// Real seconds before a press can skip it, so the press that started the
/// game doesn't skip it unseen.
pub const SKIP_GRACE: f32 = 0.35;

/// The most one frame advances the intro (real seconds).
const MAX_STEP: f32 = 0.1;

/// How far over the stands the flyover keeps (m), and how finely it
/// looks ahead along its path for them.
const STANDS_CLEARANCE: f32 = 6.0;
const CLEAR_SAMPLES: usize = 48;

/// The flyover's lens (vertical, radians).
const FLY_FOV: f32 = 50.0_f32.to_radians();

/// Insert to turn the intro off (the headless test harness does, as it does
/// `JuiceDisabled`).
#[derive(Resource)]
pub struct IntroDisabled;

/// The intro on screen, if any. Public so the lineup card and tests can
/// read it.
#[derive(Resource, Default, Debug)]
pub struct Intro {
    active: Option<Run>,
    /// Intros started this session (tests read it).
    pub played: u32,
    /// Intros cut short by a press or a Director.
    pub skipped: u32,
}

#[derive(Debug)]
struct Run {
    /// Real seconds since it started.
    shown: f32,
    /// The camera as it was, put back when the intro ends.
    camera: Option<(Transform, Projection)>,
}

impl Intro {
    /// Whether the intro is playing.
    pub fn is_playing(&self) -> bool {
        self.active.is_some()
    }

    /// Real seconds into the intro, while it plays.
    pub fn shown(&self) -> Option<f32> {
        self.active.as_ref().map(|r| r.shown)
    }

    /// The lineup card is up.
    pub fn lineup_up(&self) -> bool {
        self.shown().is_some_and(|t| t >= LINEUP_FROM)
    }
}

/// The flyover at `t` real seconds in: eye, look-at and lens. From high
/// beyond centre field, looking in at the plate, it swings round the
/// third-base side (+X; first base sits at -X) and settles low behind
/// home looking out at the mound, eased in and out. `still` (reduce
/// motion) holds the closing frame the whole time.
pub fn flyover(t: f32, field: &FieldSpec, still: bool) -> (Vec3, Vec3, f32) {
    let f = if still {
        1.0
    } else {
        (t / INTRO_SECS).clamp(0.0, 1.0)
    };
    let s = f * f * (3.0 - 2.0 * f);
    // Never lower than the stands still ahead on the path ask for, so the
    // swing passes over the first-base seats rather than through them and
    // the descent never climbs back up.
    let floor = (0..=CLEAR_SAMPLES)
        .map(|i| {
            let ahead = s + (1.0 - s) * i as f32 / CLEAR_SAMPLES as f32;
            stands_height_under(path(ahead, field), field) + STANDS_CLEARANCE
        })
        .fold(0.0, f32::max);
    let eye = path(s, field);
    let eye = eye.with_y(eye.y.max(floor));
    let home = Vec3::new(0.0, 1.0, 0.0);
    let mound = Vec3::new(0.0, 1.0, field.pitch_distance * 0.6);
    (eye, home.lerp(mound, s), FLY_FOV)
}

/// The flyover's eye at eased progress `s` before it clears the stands.
/// Azimuth from +Z (centre field) toward +X, ending at -Z (behind home).
fn path(s: f32, field: &FieldSpec) -> Vec3 {
    let azimuth = s * PI;
    let radius = field.fence_line * 1.25 + (22.0 - field.fence_line * 1.25) * s;
    let height = 42.0 + (7.0 - 42.0) * s;
    Vec3::new(azimuth.sin() * radius, height, azimuth.cos() * radius)
}

pub struct IntroPlugin;

impl Plugin for IntroPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Intro>()
            .add_systems(crate::game::game_start(), start_intro)
            .add_systems(OnEnter(GameState::GameOver), stop_intro)
            .add_systems(OnEnter(GameState::MainMenu), stop_intro)
            .add_systems(
                Update,
                advance_intro
                    // After every intent writer that isn't gameplay (devices,
                    // the Director's seam) and before flow reads them, so the
                    // presses the intro swallows never reach the game; and
                    // before the broadcast rig, so the frame it ends on is
                    // already the rig's.
                    .after(crate::game::GameplayOrder::Input)
                    .before(crate::game::GameplayOrder::Flow)
                    .run_if(in_state(GameState::Playing)),
            )
            .add_systems(
                PostUpdate,
                fly_camera
                    .after(crate::game::replay::ReplaySystems)
                    .before(TransformSystems::Propagate)
                    .run_if(in_state(GameState::Playing).or(in_state(GameState::Paused))),
            );
    }
}

/// Starts the intro as the game starts, holding the gameplay clock.
fn start_intro(
    disabled: Option<Res<IntroDisabled>>,
    mut intro: ResMut<Intro>,
    mut virt: ResMut<Time<Virtual>>,
    cam: Query<(&Transform, &Projection), With<Camera3d>>,
) {
    if disabled.is_some() {
        return;
    }
    if intro.active.is_none() {
        intro.active = Some(Run {
            shown: 0.0,
            camera: cam.single().ok().map(|(tf, p)| (*tf, p.clone())),
        });
        intro.played += 1;
        virt.pause();
    }
}

/// Runs the intro on the real clock; a press past the grace, a Director,
/// or the end gives the game its clock back. Every press made while it
/// plays, the skip included, is swallowed: flow reads `Intents` with the
/// clock held, and a press would otherwise start the first pitch under the
/// flyover or throw it the instant the intro ends. (That covers devices and
/// the Director's seam; the CPU writes its intents later, inside flow, and
/// is held only because its timers run on the paused clock.)
fn advance_intro(
    real: Res<Time<Real>>,
    mut intents: ResMut<Intents>,
    director: Option<Res<Director>>,
    disabled: Option<Res<IntroDisabled>>,
    mut intro: ResMut<Intro>,
    mut virt: ResMut<Time<Virtual>>,
    mut cam: Query<(&mut Transform, &mut Projection), With<Camera3d>>,
) {
    // Read without marking: `Intro` changes only while it plays (the lineup
    // card repaints on its change).
    if intro.active.is_none() {
        return;
    }
    let Some(run) = intro.active.as_mut() else {
        return;
    };
    // A hitch (the scene spawning on PLAY BALL, a slow phone) must not eat
    // the flyover in one frame.
    run.shown += real.delta_secs().min(MAX_STEP);
    let pressed = run.shown >= SKIP_GRACE && (intents.home.action || intents.away.action);
    let skip = pressed || director.is_some() || disabled.is_some();
    *intents = Intents::default();
    if (skip || run.shown >= INTRO_SECS)
        && let Some(run) = intro.active.take()
    {
        if skip {
            intro.skipped += 1;
        }
        put_camera_back(run.camera, &mut cam);
        virt.unpause();
    }
}

/// Ends an intro the game left mid-way (quit to the menu).
fn stop_intro(
    mut intro: ResMut<Intro>,
    mut virt: ResMut<Time<Virtual>>,
    mut cam: Query<(&mut Transform, &mut Projection), With<Camera3d>>,
) {
    if let Some(run) = intro.active.take() {
        put_camera_back(run.camera, &mut cam);
        virt.unpause();
    }
}

fn put_camera_back(
    saved: Option<(Transform, Projection)>,
    cam: &mut Query<(&mut Transform, &mut Projection), With<Camera3d>>,
) {
    if let (Some((tf, projection)), Ok((mut live_tf, mut live_projection))) =
        (saved, cam.single_mut())
    {
        *live_tf = tf;
        *live_projection = projection;
    }
}

/// Points the camera along the flyover (after the broadcast rig has had its
/// say this frame, so the rig never fights it).
fn fly_camera(
    intro: Res<Intro>,
    field: Res<FieldSpec>,
    settings: Option<Res<Settings>>,
    mut cam: Query<(&mut Transform, &mut Projection), With<Camera3d>>,
) {
    let Some(t) = intro.shown() else {
        return;
    };
    let still = settings.as_ref().is_some_and(|s| s.reduce_motion);
    let (eye, target, fov) = flyover(t, &field, still);
    if let Ok((mut tf, mut projection)) = cam.single_mut() {
        *tf = Transform::from_translation(eye).looking_at(target, Vec3::Y);
        if let Projection::Perspective(persp) = projection.as_mut() {
            persp.fov = fov;
        }
    }
}

#[cfg(test)]
#[path = "intro.test.rs"]
mod tests;
