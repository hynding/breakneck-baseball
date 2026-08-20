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

use crate::game::GameState;
use crate::game::ball::{BALL_DRAG_FACTOR, Baseball, HitEvent, MAGNUS_FACTOR, WallBangEvent};
use crate::game::flow::{Phase, Play};
use crate::game::player::{CatcherRole, PlateUmpire};
use crate::game::rules;
use crate::game::variant::FieldSpec;

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

/// Home-run trot orbit: during the result pause of a home run the broadcast
/// rig sweeps around the diamond instead of holding the static wide plate, so
/// the trot is shot from a moving camera. Distance/height of the orbiting eye
/// and the radians-per-second it sweeps.
const TROT_ORBIT_DIST: f32 = 26.0;
const TROT_ORBIT_HEIGHT: f32 = 11.0;
const TROT_ORBIT_RATE: f32 = 0.7;

/// The broadcast eye for the home-run trot orbit: a point on a circle of
/// radius [`TROT_ORBIT_DIST`] at height [`TROT_ORBIT_HEIGHT`] around `focus`,
/// swept to `azimuth` radians. Same sin/cos parameterization as
/// [`orbit_transform`], so the trot shot reuses the free camera's orbit math.
fn trot_orbit_eye(focus: Vec3, azimuth: f32) -> Vec3 {
    focus
        + Vec3::new(
            TROT_ORBIT_DIST * azimuth.sin(),
            TROT_ORBIT_HEIGHT,
            TROT_ORBIT_DIST * azimuth.cos(),
        )
}

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
/// comment) — a 16:9 window. `aspect_safe_duel_vfov` below treats this as the
/// reference the batter must stay framed at; narrower windows widen the
/// vertical FOV to compensate, wider ones are left alone.
const DUEL_REFERENCE_ASPECT: f32 = 16.0 / 9.0;

/// The duel-phase vertical FOV to actually apply for a camera whose viewport
/// has the given `aspect` (width / height), so the *horizontal* field of view
/// never shrinks below what `target_vfov` gives at the 16:9 reference the
/// duel framing was tuned at. `PerspectiveProjection::fov` is vertical, so a
/// narrower-than-16:9 viewport (a portrait-ish window, or a narrow wasm
/// canvas under `fit_canvas_to_parent`) crops horizontally at a fixed
/// vertical FOV — exactly what put the batter at risk of clipping out of
/// frame in the tight catcher-POV shot. Converts `target_vfov` to the
/// horizontal FOV it gives at the 16:9 reference, then re-derives the
/// vertical FOV that reproduces *that* horizontal FOV at the real `aspect`;
/// identity at 16:9, wider (more vertical coverage) below it, and left at
/// `target_vfov` above it (ultrawide already has FOV to spare, so it's left
/// untouched rather than narrowed).
pub fn aspect_safe_duel_vfov(target_vfov: f32, aspect: f32) -> f32 {
    if aspect >= DUEL_REFERENCE_ASPECT {
        return target_vfov;
    }
    let target_hfov = 2.0 * ((target_vfov / 2.0).tan() * DUEL_REFERENCE_ASPECT).atan();
    2.0 * ((target_hfov / 2.0).tan() / aspect).atan()
}

/// Vertical FOV for the behind-pitcher view: a long, narrow "pitcher cam"
/// shot from well behind the mound, so a tighter lens than the broadcast
/// framing keeps the batter readable at that distance.
const BEHIND_PITCHER_FOV: f32 = 40.0_f32.to_radians();

/// Vertical FOV for the batting zoom: closer to the subjects than the
/// broadcast framing but not as tight as the catcher POV, which sits right
/// at the batter's shoulder.
const BATTING_ZOOM_FOV: f32 = 65.0_f32.to_radians();

// ── Framing math ──────────────────────────────────────────────────────────────

/// Signed vertical NDC coordinate (−1 = bottom edge, +1 = top edge) of world
/// point `p` as seen by a look-at camera at `eye` toward `target` with
/// vertical FOV `vfov`. Pure — the framing tests use it to prove the duel
/// shot really contains the batter, instead of eyeballing screenshots.
pub fn framed_ndc_y(eye: Vec3, target: Vec3, vfov: f32, p: Vec3) -> f32 {
    let fwd = (target - eye).normalize();
    let right = fwd.cross(Vec3::Y).normalize();
    let up = right.cross(fwd);
    let v = p - eye;
    let depth = v.dot(fwd).max(f32::EPSILON);
    (v.dot(up) / depth) / (vfov / 2.0).tan()
}

/// Fraction of the viewport height the segment `bottom`→`top` spans through
/// the same camera.
pub fn framed_height_fraction(eye: Vec3, target: Vec3, vfov: f32, bottom: Vec3, top: Vec3) -> f32 {
    ((framed_ndc_y(eye, target, vfov, top) - framed_ndc_y(eye, target, vfov, bottom)) / 2.0).abs()
}

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

/// The phases during which the broadcast rig wants (or is still holding)
/// the tight duel framing: the duel itself, the post-contact plate hold,
/// and the result pause of a pitch the catcher gloved — a called strike or
/// ball doesn't deserve a zoom-out; only balls the mitt missed (hits, dirt
/// balls, dropped thirds, HBP) release the camera. Shared between
/// [`broadcast_camera`]'s framing choice and [`hide_occluders`]'s
/// catcher-POV arm so the catcher can never pop into a lens that is still
/// parked inside his silhouette.
fn duel_framing_wanted(play: &Play, now: f32) -> bool {
    match play.phase {
        Phase::PrePitch | Phase::WindUp | Phase::Pitch => true,
        Phase::InPlay => play.since_contact(now) < BALL_FOLLOW_DELAY,
        Phase::Result => play.pitch_gloved() && !play.is_home_run(),
    }
}

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
fn hide_occluders(
    time: Res<Time>,
    view: Res<DuelView>,
    field: Res<FieldSpec>,
    play: Res<Play>,
    mode: Res<CameraMode>,
    mut subjects: Query<
        (&Transform, &mut Visibility, Has<CatcherRole>),
        Or<(With<CatcherRole>, With<PlateUmpire>)>,
    >,
) {
    let dueling = matches!(play.phase, Phase::PrePitch | Phase::WindUp | Phase::Pitch);
    let pov_inside_catcher = *mode == CameraMode::Broadcast
        && *view == DuelView::CatcherPov
        && duel_framing_wanted(&play, time.elapsed_secs());
    // The FOV this call computes is discarded (occlusion only cares about the
    // eye/target axis), so the aspect passed through doesn't matter — the
    // reference aspect keeps this a no-op correction.
    let (eye, target, _) = view.framing(&field, DUEL_REFERENCE_ASPECT);
    for (transform, mut visibility, is_catcher) in &mut subjects {
        // Occlusion only makes sense for the camera actually looking through
        // this axis: in Orbit the player is free-looking with a completely
        // different eye/target, so a rig hidden for a Broadcast duel view
        // must not stay hidden just because the view resource hasn't
        // changed — the system still runs every frame (unconditionally, not
        // gated out entirely) so switching back to Broadcast, or into
        // Orbit, both re-evaluate and settle on the right state immediately.
        let blocking = (is_catcher && pov_inside_catcher)
            || (dueling
                && *mode == CameraMode::Broadcast
                && occludes(
                    eye,
                    target,
                    transform.translation,
                    OCCLUSION_NEAR,
                    OCCLUSION_RADIUS,
                ));
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
    // The camera's actual aspect ratio (width / height), read before the
    // framing decision so the duel FOV can correct for it — see
    // `aspect_safe_duel_vfov`. Falls back to the reference aspect (a no-op
    // correction) before the camera exists.
    let aspect = match cam_q.get_single() {
        Ok((_, Projection::Perspective(persp))) => persp.aspect_ratio,
        _ => DUEL_REFERENCE_ASPECT,
    };

    // Pick the framing the current phase wants.
    let (desired_eye, desired_target, desired_fov) = match (play.phase, ball_q.get_single()) {
        // Fresh contact: hold the plate framing for a beat — the swing, the
        // crack, the batter breaking from the box — before chasing the ball.
        (Phase::InPlay, Ok(_)) if play.since_contact(time.elapsed_secs()) < BALL_FOLLOW_DELAY => (
            field.duel_eye,
            field.duel_target,
            aspect_safe_duel_vfov(DUEL_FOV, aspect),
        ),
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
        // Result pause of a home run: orbit the diamond while the batter
        // trots the bases — a sweeping victory-lap shot that lerps in from the
        // ball-follow and back out to the duel framing at phase end.
        (Phase::Result, _) if play.is_home_run() => {
            let focus = Vec3::new(field.broadcast_target.x, 1.4, field.broadcast_target.z);
            let eye = trot_orbit_eye(focus, time.elapsed_secs() * TROT_ORBIT_RATE);
            (eye, focus, BROADCAST_FOV)
        }
        // Result pause of a gloved pitch (called strike/ball, strikeout
        // into the mitt): stay in the at-bat view — the umpire's call
        // doesn't deserve a zoom-out. Everything the mitt missed (hits,
        // dirt balls, dropped thirds, HBP) falls through to the wide shot.
        (Phase::Result, _) if play.pitch_gloved() => view.framing(&field, aspect),
        // Result pause: settle on the wide home framing.
        (Phase::Result, _) => (field.broadcast_eye, field.broadcast_target, BROADCAST_FOV),
        // The duel: whichever at-bat view the player has cycled to with V.
        _ => view.framing(&field, aspect),
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

    /// At the 16:9 reference aspect the duel FOV was tuned at, the correction
    /// must be an identity (no crop was ever a problem here).
    #[test]
    fn aspect_safe_duel_vfov_is_identity_at_reference_aspect() {
        let vfov = aspect_safe_duel_vfov(DUEL_FOV, DUEL_REFERENCE_ASPECT);
        assert!(
            (vfov - DUEL_FOV).abs() < 1e-4,
            "16:9 should reproduce DUEL_FOV exactly, got {vfov}"
        );
    }

    /// A narrower-than-16:9 window (e.g. 4:3) must widen the vertical FOV so
    /// the horizontal coverage doesn't shrink and crop the batter.
    #[test]
    fn aspect_safe_duel_vfov_widens_for_a_narrower_aspect() {
        let vfov = aspect_safe_duel_vfov(DUEL_FOV, 4.0 / 3.0);
        assert!(
            vfov > DUEL_FOV,
            "4:3 should widen the vertical FOV, got {vfov} vs DUEL_FOV {DUEL_FOV}"
        );
    }

    /// A wider-than-16:9 (ultrawide) window already has FOV to spare — the
    /// duel FOV must be left untouched, not narrowed.
    #[test]
    fn aspect_safe_duel_vfov_unchanged_for_ultrawide() {
        let vfov = aspect_safe_duel_vfov(DUEL_FOV, 21.0 / 9.0);
        assert_eq!(vfov, DUEL_FOV);
    }

    /// The trot orbit eye stays on a fixed-radius, fixed-height circle around
    /// the focus for every azimuth, and actually sweeps (distinct eyes at
    /// distinct azimuths) — the "sweeping victory lap" the Result-phase branch
    /// lerps toward.
    #[test]
    fn trot_orbit_eye_rides_a_fixed_circle_and_sweeps() {
        let focus = Vec3::new(2.0, 1.4, 9.0);
        let mut prev: Option<Vec3> = None;
        for step in 0..8 {
            let azim = step as f32 * std::f32::consts::FRAC_PI_4;
            let eye = trot_orbit_eye(focus, azim);
            // Fixed height above the focus.
            assert!((eye.y - (focus.y + TROT_ORBIT_HEIGHT)).abs() < 1e-4);
            // Fixed horizontal radius from the focus.
            let horiz = Vec2::new(eye.x - focus.x, eye.z - focus.z).length();
            assert!(
                (horiz - TROT_ORBIT_DIST).abs() < 1e-3,
                "azim {azim}: radius {horiz} != {TROT_ORBIT_DIST}"
            );
            if let Some(p) = prev {
                assert!(p.distance(eye) > 1e-3, "the orbit must actually move");
            }
            prev = Some(eye);
        }
    }

    /// The catcher-POV duel framing must show the batter's entire body —
    /// spikes to helmet, on his side of the plate — filling 80–90% of the
    /// screen height at the 16:9 reference aspect, fully inside the frame,
    /// in both parks. The design ask behind the pulled-back duel eye.
    #[test]
    fn catcher_pov_frames_the_full_batter_at_80_to_90_percent() {
        use crate::game::player::{BATTER_STAND_X, RIG_HEIGHT_M};
        for id in [VariantId::Standard, VariantId::FrontYard] {
            let f = id.field();
            let (eye, target, vfov) = DuelView::CatcherPov.framing(&f, DUEL_REFERENCE_ASPECT);
            let feet = Vec3::new(BATTER_STAND_X, 0.0, 0.0);
            let head = feet + Vec3::Y * RIG_HEIGHT_M;
            let frac = framed_height_fraction(eye, target, vfov, feet, head);
            assert!(
                (0.80..=0.90).contains(&frac),
                "{id:?}: batter fills {frac:.3} of screen height, want 0.80..=0.90"
            );
            for p in [feet, head] {
                let y = framed_ndc_y(eye, target, vfov, p);
                assert!(
                    y.abs() <= 0.98,
                    "{id:?}: batter point {p} clipped at ndc y {y:.3}"
                );
            }
        }
    }

    /// The result pause holds the duel framing only for a pitch the catcher
    /// gloved (called strikes/balls, strikeouts into the mitt end tight on
    /// the plate); everything the mitt missed — hits, dirt balls, dropped
    /// thirds, HBP — releases the camera to the wide shot. The duel phases
    /// always want the tight framing; the post-contact plate hold expires
    /// with `BALL_FOLLOW_DELAY`.
    #[test]
    fn duel_framing_holds_result_only_for_gloved_pitches() {
        use crate::game::flow::Play;
        assert!(duel_framing_wanted(
            &Play::test_play(Phase::Result, true),
            10.0
        ));
        assert!(!duel_framing_wanted(
            &Play::test_play(Phase::Result, false),
            10.0
        ));
        assert!(duel_framing_wanted(
            &Play::test_play(Phase::PrePitch, false),
            10.0
        ));
        assert!(duel_framing_wanted(
            &Play::test_play(Phase::Pitch, false),
            10.0
        ));
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

            let (bz_eye, bz_target, _) = DuelView::BattingZoom.framing(&f, DUEL_REFERENCE_ASPECT);
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

            let (bp_eye, bp_target, _) = DuelView::BehindPitcher.framing(&f, DUEL_REFERENCE_ASPECT);
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
