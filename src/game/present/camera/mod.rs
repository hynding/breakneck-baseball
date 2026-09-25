//! Game camera.
//!
//! Two modes, toggled with **C** (or the controller's Select/Back button):
//!
//! - **Broadcast** (default): the duel framing for the pitch, then a shot
//!   list that *cuts* between one-subject shots while the ball is live —
//!   ball-follow, fielder cam, base cam ([`Shot`]).
//! - **Orbit**: the free stadium camera (WASD / arrows to orbit, Q/E or wheel to
//!   zoom, R to reset) for looking around.
//!
//! While broadcast holds the duel (pitch/swing) framing, **V** cycles through
//! four [`DuelView`]s (the default batting view from behind the box, the
//! catcher POV, behind-the-pitcher, and the elevated broadcast plate shot).
//! Whichever of the catcher/plate
//! umpire sits right in front of the lens for the active view is hidden for
//! the duration ([`rigs::hide_occluders`]) — a body brushing the glass, not a
//! full occlusion raycast, so the far-off behind-pitcher and broadcast-plate
//! eyes never trigger it even though those two stand technically "between"
//! eye and target.

use bevy::prelude::*;

use crate::game::GameState;
use crate::game::variant::FieldSpec;

mod framing;
mod rigs;

pub use framing::{
    aspect_safe_duel_vfov, brushes_lens, framed_height_fraction, framed_ndc_x, framed_ndc_y,
    occludes,
};
pub use rigs::{BroadcastRig, OrbitState, Shot};

use rigs::{
    CameraKick, broadcast_camera, decay_kick, hide_occluders, kick_on_hit, kick_on_wall_bang,
    orbit_camera, zoom_camera,
};

// ── Mode ──────────────────────────────────────────────────────────────────────

#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum CameraMode {
    #[default]
    Broadcast,
    Orbit,
}

fn is_broadcast(mode: Res<CameraMode>) -> bool {
    *mode == CameraMode::Broadcast
}
fn is_orbit(mode: Res<CameraMode>) -> bool {
    *mode == CameraMode::Orbit
}

// ── Duel view ─────────────────────────────────────────────────────────────────

/// The active at-bat framing during the pitch/swing duel, cycled with **V**.
/// Post-contact camera behaviour (the plate hold, then the ball chase) is
/// unaffected — this only picks the framing `rigs::broadcast_camera` reads
/// while the duel phases (`PrePitch`/`WindUp`/`Pitch`) are in effect.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum DuelView {
    /// The catcher's own point of view (Task 12): the lens sits just past
    /// his crouched head, so he — and the plate umpire behind him — never
    /// render, no occlusion check needed. Was the default until TODO 100:
    /// under a metre from the batter the swing crosses the frame in a few
    /// frames and the ball flies *into* the lens, which made a hit or miss
    /// unreadable.
    CatcherPov,
    /// Behind and above the mound, looking out at the batter — the
    /// reference "pitcher cam": the catcher and plate umpire are meant to
    /// stay in frame here.
    BehindPitcher,
    /// The default (TODO 100): the reference batting shot from 2 m behind
    /// the plate at the batter's chest height — his whole body on the
    /// screen-left third, the zone at centre, the pitcher above it — so the
    /// ball grows toward the lens and the full bat arc is visible beside
    /// its path. The catcher sits in the sightline and is auto-hidden; the
    /// umpire is behind the eye. Framing in `FieldSpec::batting_zoom_eye`.
    #[default]
    BattingZoom,
    /// The elevated behind-home broadcast shot (reuses the wide framing).
    BroadcastPlate,
}

impl DuelView {
    /// The next view in the cycle (wraps).
    fn next(self) -> DuelView {
        match self {
            DuelView::BattingZoom => DuelView::CatcherPov,
            DuelView::CatcherPov => DuelView::BehindPitcher,
            DuelView::BehindPitcher => DuelView::BroadcastPlate,
            DuelView::BroadcastPlate => DuelView::BattingZoom,
        }
    }

    /// This view's eye, look-at target, and vertical FOV for the given park
    /// and camera `aspect` ratio (width / height).
    fn framing(self, field: &FieldSpec, aspect: f32) -> (Vec3, Vec3, f32) {
        match self {
            DuelView::CatcherPov => (
                field.duel_eye,
                field.duel_target,
                aspect_safe_duel_vfov(DUEL_FOV, aspect),
            ),
            DuelView::BehindPitcher => (
                field.behind_pitcher_eye,
                field.behind_pitcher_target,
                BEHIND_PITCHER_FOV,
            ),
            // The same aspect correction as the catcher POV: this is the
            // tight default shot now, and a narrow viewport must widen
            // rather than crop the batter off the left edge.
            DuelView::BattingZoom => (
                field.batting_zoom_eye,
                field.batting_zoom_target,
                aspect_safe_duel_vfov(BATTING_ZOOM_FOV, aspect),
            ),
            DuelView::BroadcastPlate => {
                (field.broadcast_eye, field.broadcast_target, BROADCAST_FOV)
            }
        }
    }
}

fn toggle_duel_view(keyboard: Res<ButtonInput<KeyCode>>, mut view: ResMut<DuelView>) {
    if keyboard.just_pressed(KeyCode::KeyV) {
        *view = view.next();
    }
}

/// Fallback broadcast framing used before a field is chosen (initial camera
/// spawn); once a game is running the framing comes from the [`FieldSpec`].
const BROADCAST_HOME_TARGET: Vec3 = Vec3::new(0.0, 1.2, 9.0);
const BROADCAST_EYE: Vec3 = Vec3::new(0.0, 13.0, -21.0);

/// Seconds after contact before the camera cuts from the plate to the ball
/// — just the crack and the bat coming through, per the reference footage
/// (a cut within a quarter second; docs/agent/SMB3-REFERENCE-NOTES.md
/// §2.5). `runner::RUN_OUT_DELAY` matches it so the batter is never seen
/// swapped for his run-out stand-in in this frame.
const BALL_FOLLOW_DELAY: f32 = 0.25;

/// Vertical FOV used everywhere except the duel (unchanged from the single
/// FOV this camera used to run at everywhere).
const BROADCAST_FOV: f32 = std::f32::consts::FRAC_PI_3;

/// Vertical FOV during the duel — *wider* than the broadcast framing, not
/// narrower. Sitting the eye just past the catcher's crouched head (so no
/// part of him renders) puts the batter, off to the side at x≈0.7-1.1,
/// less than a metre from the lens; at that range `BROADCAST_FOV` clips his
/// far edge. Opening the lens up keeps him fully in frame while the zone
/// box — close enough now to fill a quarter of the screen width — still
/// reads far more prominent than the old far-back framing ever did, since
/// its screen size comes from eye proximity, not FOV.
const DUEL_FOV: f32 = 80.0_f32.to_radians();

/// The aspect ratio `DUEL_FOV` was tuned and validated at (see its doc
/// comment) — a 16:9 window. `framing::aspect_safe_duel_vfov` below treats
/// this as the reference the batter must stay framed at; narrower windows
/// widen the vertical FOV to compensate, wider ones are left alone.
const DUEL_REFERENCE_ASPECT: f32 = 16.0 / 9.0;

/// Vertical FOV for the behind-pitcher view: a long, narrow "pitcher cam"
/// shot from well behind the mound, so a tighter lens than the broadcast
/// framing keeps the batter readable at that distance.
const BEHIND_PITCHER_FOV: f32 = 40.0_f32.to_radians();

/// Vertical FOV for the default batting view: 2 m behind the plate a 60°
/// lens puts the whole batter in frame at ~80% of the screen height while
/// the zone box at centre stays large (the composition the framing test
/// pins). Aspect-corrected like `DUEL_FOV` (see `DuelView::framing`).
const BATTING_ZOOM_FOV: f32 = 60.0_f32.to_radians();

// ── Plugin ────────────────────────────────────────────────────────────────────

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OrbitState>()
            .init_resource::<CameraMode>()
            .init_resource::<DuelView>()
            .init_resource::<BroadcastRig>()
            .init_resource::<CameraKick>()
            .add_systems(Startup, spawn_camera)
            .add_systems(
                Update,
                (
                    toggle_camera_mode,
                    toggle_duel_view,
                    // Kick impulses honour reduce-motion; decay still runs so
                    // any residue drains if the setting flips mid-kick.
                    (kick_on_hit, kick_on_wall_bang).run_if(crate::game::juice::motion_enabled),
                    decay_kick,
                    hide_occluders,
                )
                    .run_if(in_state(GameState::Playing)),
            )
            .add_systems(
                Update,
                broadcast_camera
                    .run_if(in_state(GameState::Playing))
                    .run_if(is_broadcast),
            )
            .add_systems(
                Update,
                (orbit_camera, zoom_camera)
                    .run_if(in_state(GameState::Playing))
                    .run_if(is_orbit),
            );
    }
}

// ── Startup ───────────────────────────────────────────────────────────────────

fn spawn_camera(mut commands: Commands) {
    // Explicit anti-aliasing choice per target (was the implicit Bevy
    // default, Sample4 everywhere): 4x holds on native GPUs; on
    // wasm/WebGL2 the MSAA resolve is a measurable per-frame cost at
    // stadium resolution, so the web build takes 2x — edges on the blocky
    // art style read nearly the same and the resolve is half the work.
    #[cfg(not(target_arch = "wasm32"))]
    let msaa = Msaa::Sample4;
    #[cfg(target_arch = "wasm32")]
    let msaa = Msaa::Sample2;
    commands.spawn((
        Camera3d::default(),
        msaa,
        Transform::from_translation(BROADCAST_EYE).looking_at(BROADCAST_HOME_TARGET, Vec3::Y),
        Projection::Perspective(PerspectiveProjection {
            fov: BROADCAST_FOV,
            ..default()
        }),
    ));
}

// ── Mode toggle ───────────────────────────────────────────────────────────────

fn toggle_camera_mode(
    keyboard: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    mut mode: ResMut<CameraMode>,
) {
    let toggled = keyboard.just_pressed(KeyCode::KeyC)
        || pads.iter().any(|p| p.just_pressed(GamepadButton::Select));
    if toggled {
        *mode = match *mode {
            CameraMode::Broadcast => CameraMode::Orbit,
            CameraMode::Orbit => CameraMode::Broadcast,
        };
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "mod.test.rs"]
mod tests;
