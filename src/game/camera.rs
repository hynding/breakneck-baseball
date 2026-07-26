//! Game camera.
//!
//! Two modes, toggled with **C** (or the controller's Select/Back button):
//!
//! - **Broadcast** (default): a high angle behind home plate that frames the
//!   whole diamond for the pitch, then gently follows the ball while it is live.
//! - **Orbit**: the free stadium camera (WASD / arrows to orbit, Q/E or wheel to
//!   zoom, R to reset) for looking around.
//!
//! While broadcast holds the duel (pitch/swing) framing, **V** cycles through
//! four [`DuelView`]s (catcher POV, behind-the-pitcher, a tight batting zoom,
//! and the elevated broadcast plate shot). Whichever of the catcher/plate
//! umpire sits right in front of the lens for the active view is hidden for
//! the duration ([`hide_occluders`]) — a body brushing the glass, not a full
//! occlusion raycast, so the far-off behind-pitcher and broadcast-plate eyes
//! never trigger it even though those two stand technically "between" eye
//! and target.

use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy_rapier3d::prelude::Velocity;

use crate::game::ball::{Baseball, HitEvent, WallBangEvent, BALL_DRAG_FACTOR, MAGNUS_FACTOR};
use crate::game::flow::{Phase, Play};
use crate::game::player::{CatcherRole, PlateUmpire};
use crate::game::rules;
use crate::game::variant::FieldSpec;
use crate::game::GameState;

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
/// unaffected — this only picks the framing `broadcast_camera` reads while
/// the duel phases (`PrePitch`/`WindUp`/`Pitch`) are in effect.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum DuelView {
    /// The catcher's own point of view (Task 12): the lens sits just past
    /// his crouched head, so he — and the plate umpire behind him — never
    /// render, no occlusion check needed.
    #[default]
    CatcherPov,
    /// Behind and above the mound, looking out at the batter — the
    /// reference "pitcher cam": the catcher and plate umpire are meant to
    /// stay in frame here.
    BehindPitcher,
    /// A tight zoom from behind and beside the batter's box, looking across
    /// the zone toward the pitcher — close enough behind the plate that the
    /// catcher (and the umpire behind him) sit right in the sightline.
    BattingZoom,
    /// The elevated behind-home broadcast shot (reuses the wide framing).
    BroadcastPlate,
}

impl DuelView {
    /// The next view in the cycle (wraps).
    fn next(self) -> DuelView {
        match self {
            DuelView::CatcherPov => DuelView::BehindPitcher,
            DuelView::BehindPitcher => DuelView::BattingZoom,
            DuelView::BattingZoom => DuelView::BroadcastPlate,
            DuelView::BroadcastPlate => DuelView::CatcherPov,
        }
    }

    /// This view's eye, look-at target, and vertical FOV for the given park.
    fn framing(self, field: &FieldSpec) -> (Vec3, Vec3, f32) {
        match self {
            DuelView::CatcherPov => (field.duel_eye, field.duel_target, DUEL_FOV),
            DuelView::BehindPitcher => (
                field.behind_pitcher_eye,
                field.behind_pitcher_target,
                BEHIND_PITCHER_FOV,
            ),
            DuelView::BattingZoom => (
                field.batting_zoom_eye,
                field.batting_zoom_target,
                BATTING_ZOOM_FOV,
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

/// Seconds after contact before the camera leaves the plate to chase the
/// ball — long enough to watch the swing land and the batter break.
const BALL_FOLLOW_DELAY: f32 = 1.0;

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

/// Vertical FOV for the behind-pitcher view: a long, narrow "pitcher cam"
/// shot from well behind the mound, so a tighter lens than the broadcast
/// framing keeps the batter readable at that distance.
const BEHIND_PITCHER_FOV: f32 = 40.0_f32.to_radians();

/// Vertical FOV for the batting zoom: closer to the subjects than the
/// broadcast framing but not as tight as the catcher POV, which sits right
/// at the batter's shoulder.
const BATTING_ZOOM_FOV: f32 = 65.0_f32.to_radians();

// ── Occlusion ─────────────────────────────────────────────────────────────────

/// How close to the eye (metres, measured along the eye→target axis) a
/// subject must be to count as blocking the shot. Small on purpose: this is
/// a body brushing the lens, not a general raycast, which is why views whose
/// eye sits far from the catcher/umpire (behind-pitcher, broadcast plate)
/// never trigger it even though those two are technically "in between" eye
/// and target in the literal geometric sense.
const OCCLUSION_NEAR: f32 = 4.0;

/// How far off the eye→target axis (metres) a subject may sit and still
/// count as blocking the shot.
const OCCLUSION_RADIUS: f32 = 1.6;

/// Pure predicate: does `subject` sit close enough to `eye`, and close
/// enough to the `eye`→`target` sightline, to block the shot? `near` caps
/// how far down the axis (from the eye) counts as "in the way"; `radius`
/// caps how far off the axis. A subject behind the eye (negative distance
/// along the axis) never occludes.
pub fn occludes(eye: Vec3, target: Vec3, subject: Vec3, near: f32, radius: f32) -> bool {
    let axis = target - eye;
    let axis_len = axis.length();
    if axis_len < f32::EPSILON {
        return false;
    }
    let axis_dir = axis / axis_len;
    let to_subject = subject - eye;
    let along = to_subject.dot(axis_dir);
    if along <= 0.0 || along > near.min(axis_len) {
        return false;
    }
    let perp = to_subject - axis_dir * along;
    perp.length() <= radius
}

/// Hides the catcher/plate umpire root(s) that sit in the way of the active
/// duel view for as long as they do, and restores them the rest of the
/// time — outside the duel phases (ball in play, result pause) or on a view
/// change that clears the block. Root-level `Visibility` is the same
/// mechanism run-out rigs use to swap the batter for his stand-in
/// (`runner.rs`), so this never fights that: it only ever touches
/// `CatcherRole`/`PlateUmpire` roots, which never run bases.
#[allow(clippy::type_complexity)]
fn hide_occluders(
    view: Res<DuelView>,
    field: Res<FieldSpec>,
    play: Res<Play>,
    mode: Res<CameraMode>,
    mut subjects: Query<(&Transform, &mut Visibility), Or<(With<CatcherRole>, With<PlateUmpire>)>>,
) {
    let dueling = matches!(play.phase, Phase::PrePitch | Phase::WindUp | Phase::Pitch);
    let (eye, target, _) = view.framing(&field);
    for (transform, mut visibility) in &mut subjects {
        // Occlusion only makes sense for the camera actually looking through
        // this axis: in Orbit the player is free-looking with a completely
        // different eye/target, so a rig hidden for a Broadcast duel view
        // must not stay hidden just because the view resource hasn't
        // changed — the system still runs every frame (unconditionally, not
        // gated out entirely) so switching back to Broadcast, or into
        // Orbit, both re-evaluate and settle on the right state immediately.
        let blocking = dueling
            && *mode == CameraMode::Broadcast
            && occludes(
                eye,
                target,
                transform.translation,
                OCCLUSION_NEAR,
                OCCLUSION_RADIUS,
            );
        *visibility = if blocking {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
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

/// Smoothed eye + look-at + FOV for the broadcast camera. All three lerp
/// toward what the current play phase wants (tight duel framing for the
/// pitch, wide ball-following framing in play), so zooms glide instead of
/// cutting.
#[derive(Resource)]
struct BroadcastRig {
    eye: Vec3,
    target: Vec3,
    fov: f32,
}

impl Default for BroadcastRig {
    fn default() -> Self {
        Self {
            eye: BROADCAST_EYE,
            target: BROADCAST_HOME_TARGET,
            fov: BROADCAST_FOV,
        }
    }
}

/// Impulse added to the broadcast eye on contact; decays on real time so the
/// kick rides through the hit-stop.
#[derive(Resource, Default)]
struct CameraKick(Vec3);

/// The live ball as the broadcast camera reads it.
type BallQuery<'w, 's> =
    Query<'w, 's, (&'static Transform, &'static Velocity), (With<Baseball>, Without<Camera3d>)>;

fn kick_on_hit(mut hits: EventReader<HitEvent>, mut kick: ResMut<CameraKick>) {
    for _ in hits.read() {
        kick.0 += Vec3::new(0.0, 0.18, -0.35);
    }
}

/// A smaller thump when the ball bangs off the outfield wall.
fn kick_on_wall_bang(mut bangs: EventReader<WallBangEvent>, mut kick: ResMut<CameraKick>) {
    for _ in bangs.read() {
        kick.0 += Vec3::new(0.0, 0.10, 0.20);
    }
}

fn decay_kick(real: Res<Time<Real>>, mut kick: ResMut<CameraKick>) {
    kick.0 *= (-14.0 * real.delta_secs()).exp();
}

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
                    kick_on_hit,
                    kick_on_wall_bang,
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
    commands.spawn((
        Camera3d::default(),
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

// ── Broadcast camera ──────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn broadcast_camera(
    time: Res<Time>,
    play: Res<Play>,
    field: Res<FieldSpec>,
    view: Res<DuelView>,
    kick: Res<CameraKick>,
    ball_q: BallQuery,
    mut rig: ResMut<BroadcastRig>,
    mut cam_q: Query<(&mut Transform, &mut Projection), With<Camera3d>>,
) {
    // Pick the framing the current phase wants.
    let (desired_eye, desired_target, desired_fov) = match (play.phase, ball_q.get_single()) {
        // Fresh contact: hold the plate framing for a beat — the swing, the
        // crack, the batter breaking from the box — before chasing the ball.
        (Phase::InPlay, Ok(_)) if play.since_contact(time.elapsed_secs()) < BALL_FOLLOW_DELAY => {
            (field.duel_eye, field.duel_target, DUEL_FOV)
        }
        // A live, uncalled play: cut to where the ball is coming down. The
        // eye stations itself between home and the predicted landing spot —
        // a medium shot of the drop zone, so the chasing fielder and the
        // play about to happen are what's framed, not just the ball.
        (Phase::InPlay, Ok((ball, vel))) if !play.is_resolved() => {
            // Re-predict from the live ball; as the ball settles this
            // converges to the ball itself, so the shot lands with the play.
            let (landing, _) = rules::predict_landing_from(
                ball.translation,
                vel.linvel,
                vel.angvel,
                BALL_DRAG_FACTOR,
                MAGNUS_FACTOR,
            );
            let focus = Vec3::new(landing.x, 1.0, landing.z);
            // Keep the ball's flight in the corner of the eye.
            let target = focus.lerp(ball.translation, 0.3);

            let flat = Vec2::new(focus.x, focus.z);
            let depth = flat.length();
            // Not too close: back off along the home→landing line, higher
            // and further for deeper plays.
            let back = (depth * 0.45).clamp(12.0, 30.0);
            let height = (depth * 0.30).clamp(8.0, 18.0);
            let toward_home = -flat.normalize_or_zero();
            let eye = focus + Vec3::new(toward_home.x * back, height, toward_home.y * back);
            (eye, target, BROADCAST_FOV)
        }
        // Called plays (home-run trots): sweep with the ball — the eye
        // slides laterally and pulls up and back as it travels deep.
        (Phase::InPlay, Ok((ball, _))) => {
            let target = Vec3::new(
                ball.translation.x,
                ball.translation.y.max(1.0),
                ball.translation.z,
            );
            let depth = (ball.translation.z * 0.18).clamp(0.0, 22.0);
            let eye = field.broadcast_eye
                + Vec3::new(
                    ball.translation.x * 0.4,
                    depth * 0.6 + ball.translation.y * 0.15,
                    -depth,
                );
            (eye, target, BROADCAST_FOV)
        }
        // Result pause: settle on the wide home framing.
        (Phase::Result, _) => (field.broadcast_eye, field.broadcast_target, BROADCAST_FOV),
        // The duel: whichever at-bat view the player has cycled to with V.
        _ => view.framing(&field),
    };

    // Critically-damped-ish smoothing so framing changes glide, never cut.
    let follow = 1.0 - (-5.0 * time.delta_secs()).exp();
    rig.eye = rig.eye.lerp(desired_eye, follow);
    rig.target = rig.target.lerp(desired_target, follow);
    rig.fov = rig.fov + (desired_fov - rig.fov) * follow;

    if let Ok((mut cam, mut projection)) = cam_q.get_single_mut() {
        *cam = Transform::from_translation(rig.eye + kick.0).looking_at(rig.target, Vec3::Y);
        if let Projection::Perspective(persp) = projection.as_mut() {
            persp.fov = rig.fov;
        }
    }
}

// ── Orbit camera (free look) ──────────────────────────────────────────────────

fn orbit_camera(
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

    orbit.yaw += yaw_delta;
    orbit.pitch = (orbit.pitch + pitch_delta).clamp(0.1, std::f32::consts::FRAC_PI_2 - 0.05);

    if keyboard.just_pressed(KeyCode::KeyR) {
        *orbit = OrbitState::default();
    }

    let transform = orbit_transform(&orbit);
    for mut cam_transform in &mut camera_query {
        *cam_transform = transform;
    }
}

fn zoom_camera(
    mut scroll: EventReader<MouseWheel>,
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
    if keyboard.pressed(KeyCode::KeyQ) {
        zoom_delta -= 15.0 * dt;
    }
    if keyboard.pressed(KeyCode::KeyE) {
        zoom_delta += 15.0 * dt;
    }

    orbit.distance = (orbit.distance + zoom_delta).clamp(10.0, 200.0);

    let transform = orbit_transform(&orbit);
    for mut cam_transform in &mut camera_query {
        *cam_transform = transform;
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::variant::VariantId;

    #[test]
    fn duel_view_cycles_through_all_four_and_wraps() {
        let v = DuelView::CatcherPov;
        let v = v.next();
        assert_eq!(v, DuelView::BehindPitcher);
        let v = v.next();
        assert_eq!(v, DuelView::BattingZoom);
        let v = v.next();
        assert_eq!(v, DuelView::BroadcastPlate);
        let v = v.next();
        assert_eq!(v, DuelView::CatcherPov);
    }

    #[test]
    fn subject_behind_the_eye_never_occludes() {
        // Same axis as in front, but placed behind the eye (negative along).
        let eye = Vec3::new(0.0, 1.4, -0.9);
        let target = Vec3::new(0.0, 0.85, 15.0);
        let behind = Vec3::new(0.0, 0.6, -3.0);
        assert!(!occludes(
            eye,
            target,
            behind,
            OCCLUSION_NEAR,
            OCCLUSION_RADIUS
        ));
    }

    #[test]
    fn subject_on_axis_within_near_and_radius_occludes() {
        let eye = Vec3::ZERO;
        let target = Vec3::new(0.0, 0.0, 10.0);
        // 2 m down the axis, dead centre: well inside both thresholds.
        let subject = Vec3::new(0.0, 0.0, 2.0);
        assert!(occludes(eye, target, subject, 4.0, 1.6));
    }

    #[test]
    fn subject_beyond_the_near_threshold_does_not_occlude() {
        let eye = Vec3::ZERO;
        let target = Vec3::new(0.0, 0.0, 10.0);
        // On axis, but far past the near cutoff — this is the mechanism
        // that keeps the behind-pitcher/broadcast-plate eyes from ever
        // hiding the catcher, even though he's technically "between" eye
        // and target for those views too.
        let subject = Vec3::new(0.0, 0.0, 8.0);
        assert!(!occludes(eye, target, subject, 4.0, 1.6));
    }

    #[test]
    fn subject_off_axis_beyond_radius_does_not_occlude() {
        let eye = Vec3::ZERO;
        let target = Vec3::new(0.0, 0.0, 10.0);
        // 2 m down the axis (within `near`) but 3 m off to the side.
        let subject = Vec3::new(3.0, 0.0, 2.0);
        assert!(!occludes(eye, target, subject, 4.0, 1.6));
    }

    #[test]
    fn degenerate_axis_never_occludes() {
        let eye = Vec3::new(1.0, 1.0, 1.0);
        assert!(!occludes(eye, eye, eye, 4.0, 1.6));
    }

    /// The catcher/umpire spawn spots (`FieldSpec::fielder_positions` /
    /// `umpire_positions`, offset by the same `Vec3::Y * 0.6` `player.rs`
    /// adds at spawn) really do sit inside the occlusion cone for
    /// `BattingZoom` and really do sit outside it for `BehindPitcher`, for
    /// every variant — the concrete regression the e2e test also drives
    /// through the real ECS.
    #[test]
    fn per_variant_occlusion_matches_the_reference_shots() {
        for id in [VariantId::Standard, VariantId::FrontYard] {
            let f = id.field();
            let catcher = f
                .fielder_positions
                .iter()
                .find(|p| p.z < 0.0)
                .map(|p| *p + Vec3::Y * 0.6);
            let umpire = f.umpire_positions.first().map(|p| *p + Vec3::Y * 0.6);

            let (bz_eye, bz_target, _) = DuelView::BattingZoom.framing(&f);
            if let Some(catcher) = catcher {
                assert!(
                    occludes(bz_eye, bz_target, catcher, OCCLUSION_NEAR, OCCLUSION_RADIUS),
                    "{id:?}: batting zoom should be blocked by the catcher"
                );
            }
            if let Some(umpire) = umpire {
                assert!(
                    occludes(bz_eye, bz_target, umpire, OCCLUSION_NEAR, OCCLUSION_RADIUS),
                    "{id:?}: batting zoom should be blocked by the plate umpire"
                );
            }

            let (bp_eye, bp_target, _) = DuelView::BehindPitcher.framing(&f);
            if let Some(catcher) = catcher {
                assert!(
                    !occludes(bp_eye, bp_target, catcher, OCCLUSION_NEAR, OCCLUSION_RADIUS),
                    "{id:?}: behind-pitcher must keep the catcher visible"
                );
            }
            if let Some(umpire) = umpire {
                assert!(
                    !occludes(bp_eye, bp_target, umpire, OCCLUSION_NEAR, OCCLUSION_RADIUS),
                    "{id:?}: behind-pitcher must keep the plate umpire visible"
                );
            }
        }
    }
}
