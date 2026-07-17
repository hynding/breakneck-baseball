# Ball & Player Physics/Animation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the game move like baseball — pitcher windup, contact feedback, fielders chasing the ball, base runners, and pitch types with Magnus flight — without the rules engine ever noticing.

**Architecture:** All rig posing flows through a new `animation.rs` (clip enum + one sampler system — the future `AnimationGraph` seam). All rig movement flows through a `MoveIntent` component (the future player-controlled-fielding seam). Choreography in new `fx.rs` / `fielding.rs` / `runner.rs` modules performs outcomes that `rules.rs` already decided at contact.

**Tech Stack:** Bevy 0.15 ECS, bevy_rapier3d, native + wasm32-unknown-unknown.

## Global Constraints

- Prefix all cargo commands: `export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"`
- Outcomes stay analytic: no animation/physics change may alter what `rules.rs` decides. Ball keeps ignoring player capsules (`CollisionGroups::new(BALL_GROUP, !PLAYER_GROUP)`).
- Systems never rotate rig parts directly — they insert `Playing` clips (animation.rs is the only module that poses limbs/pivots).
- No new crates, no asset files. Effects are meshes + transforms (the `TrailGhost` pattern).
- After physics/rendering changes: `cargo check` AND `cargo check --target wasm32-unknown-unknown` must both pass.
- Home plate at origin, +Z toward the field, everywhere.
- Rig roots are `KinematicPositionBased`; move them by mutating `Transform` only.
- Commit after every task with a conventional-commit message ending in the Claude co-author trailer.

---

## Phase 1 — Pitch rhythm & contact feedback

### Task 1: `animation.rs` — clips, sampler, locomotion

**Files:**
- Create: `src/game/animation.rs`
- Modify: `src/game/mod.rs` (declare module, register plugin)

**Interfaces:**
- Consumes: `GameState` from `crate::game`.
- Produces (later tasks rely on these exact names):
  - `pub enum AnimClip { WindUp, ThrowRelease, RunCycle, ScoopBall, GloveUp, SwingBat, RecoverSwing }` with `pub fn duration(self) -> f32`, `pub fn looping(self) -> bool`
  - `pub struct Playing { pub clip: AnimClip, pub timer: Timer, pub next: Option<AnimClip> }` with `Playing::new(clip)` and `Playing::then(clip, next)`
  - `pub enum LimbKind { ArmL, ArmR, LegL, LegR }`, `pub struct RigLimb { pub kind: LimbKind }`
  - `#[derive(Default)] pub struct MoveIntent { pub target: Option<Vec3>, pub speed: f32 }`
  - `pub fn bat_idle_rotation() -> Quat`
  - `pub struct AnimationPlugin`

- [ ] **Step 1: Write the module**

```rust
//! Procedural rig animation — the single pathway through which rigs move.
//!
//! Systems never rotate rig parts directly; they insert a [`Playing`] clip on
//! a rig root (or the bat pivot) and [`sample_clips`] poses it every frame.
//! This boundary is deliberate: a future `AnimationGraph` backend replaces the
//! sampler without touching any caller. Likewise all locomotion goes through
//! [`MoveIntent`], so a human controller can later drive a fielder by writing
//! the same component the CPU choreography writes.

use bevy::prelude::*;

use crate::game::GameState;

/// Every animation the game can play, by name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimClip {
    /// Pitcher rocks back, throwing arm winds up, leg kicks.
    WindUp,
    /// Pitcher's arm whips through release.
    ThrowRelease,
    /// Looping run for any moving rig.
    RunCycle,
    /// Crouch-and-scoop for gathering a grounder.
    ScoopBall,
    /// Glove arm goes straight up to receive a ball.
    GloveUp,
    /// Bat pivot sweeps through the zone (poses the entity itself).
    SwingBat,
    /// Bat pivot returns to the shoulder (poses the entity itself).
    RecoverSwing,
}

impl AnimClip {
    /// Seconds one play-through lasts.
    pub fn duration(self) -> f32 {
        match self {
            AnimClip::WindUp => 0.5,
            AnimClip::ThrowRelease => 0.22,
            AnimClip::RunCycle => 0.45,
            AnimClip::ScoopBall => 0.32,
            AnimClip::GloveUp => 0.28,
            AnimClip::SwingBat => 0.16,
            AnimClip::RecoverSwing => 0.25,
        }
    }

    /// Clips that repeat until the component is removed.
    pub fn looping(self) -> bool {
        matches!(self, AnimClip::RunCycle)
    }
}

/// What a rig is currently playing. Insert to start, remove to stop; the
/// sampler chains to `next` when a one-shot clip finishes.
#[derive(Component)]
pub struct Playing {
    pub clip: AnimClip,
    pub timer: Timer,
    pub next: Option<AnimClip>,
}

impl Playing {
    pub fn new(clip: AnimClip) -> Self {
        let mode = if clip.looping() {
            TimerMode::Repeating
        } else {
            TimerMode::Once
        };
        Self {
            clip,
            timer: Timer::from_seconds(clip.duration(), mode),
            next: None,
        }
    }

    pub fn then(clip: AnimClip, next: AnimClip) -> Self {
        Self {
            next: Some(next),
            ..Self::new(clip)
        }
    }
}

/// Which limb a joint entity poses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimbKind {
    ArmL,
    ArmR,
    LegL,
    LegR,
}

/// Marks a poseable joint (direct child of a rig root). The joint sits at the
/// shoulder/hip and the limb mesh hangs below it, so rotating the joint
/// swings the limb.
#[derive(Component)]
pub struct RigLimb {
    pub kind: LimbKind,
}

/// Movement order for a rig: written by choreography (or, later, a human
/// controller), consumed by [`locomote`]. Cleared on arrival.
#[derive(Component, Default)]
pub struct MoveIntent {
    pub target: Option<Vec3>,
    /// Metres per second.
    pub speed: f32,
}

// ── Poses ─────────────────────────────────────────────────────────────────────

fn ease_out(f: f32) -> f32 {
    1.0 - (1.0 - f).powi(3)
}

/// Bat resting over the shoulder (the pivot's spawn rotation).
pub fn bat_idle_rotation() -> Quat {
    Quat::from_euler(EulerRot::ZXY, -0.5, 0.35, 0.0)
}

/// Bat laid horizontal, swept `angle` around Y.
fn bat_sweep_rotation(angle: f32) -> Quat {
    Quat::from_rotation_y(angle) * Quat::from_rotation_z(1.45)
}

/// Horizontal sweep range (radians about Y): cocked toward the catcher,
/// through the plate, to a follow-through toward the pitcher.
const SWEEP_FROM: f32 = -1.7;
const SWEEP_TO: f32 = 1.7;

/// Rotation for clips that pose the playing entity itself (the bat pivot).
fn self_pose(clip: AnimClip, f: f32) -> Option<Quat> {
    match clip {
        AnimClip::SwingBat => Some(bat_sweep_rotation(
            SWEEP_FROM + (SWEEP_TO - SWEEP_FROM) * ease_out(f),
        )),
        AnimClip::RecoverSwing => Some(bat_sweep_rotation(SWEEP_TO).slerp(bat_idle_rotation(), f)),
        _ => None,
    }
}

/// Joint rotation for a limb-posing clip at progress `f` (0..=1). Rotations
/// are in rig-local space; the rig root's yaw supplies world facing.
fn limb_pose(clip: AnimClip, kind: LimbKind, f: f32) -> Quat {
    use AnimClip::*;
    use LimbKind::*;
    match clip {
        WindUp => {
            let lift = ease_out(f);
            match kind {
                ArmR => Quat::from_rotation_x(-2.6 * lift),
                ArmL => Quat::from_rotation_x(-1.2 * lift),
                LegL => Quat::from_rotation_x(1.0 * lift),
                LegR => Quat::IDENTITY,
            }
        }
        ThrowRelease => {
            let s = ease_out(f);
            match kind {
                ArmR => Quat::from_rotation_x(-2.6 + 3.4 * s),
                ArmL => Quat::from_rotation_x(-1.2 + 1.2 * s),
                LegL => Quat::from_rotation_x(1.0 - 1.0 * s),
                LegR => Quat::IDENTITY,
            }
        }
        RunCycle => {
            let swing = (f * std::f32::consts::TAU).sin() * 0.9;
            match kind {
                ArmL | LegR => Quat::from_rotation_x(swing),
                ArmR | LegL => Quat::from_rotation_x(-swing),
            }
        }
        ScoopBall => {
            let dip = (f * std::f32::consts::PI).sin();
            match kind {
                ArmL | ArmR => Quat::from_rotation_x(1.6 * dip),
                LegL | LegR => Quat::from_rotation_x(0.4 * dip),
            }
        }
        GloveUp => {
            let lift = ease_out(f);
            match kind {
                ArmL => Quat::from_rotation_x(-2.9 * lift),
                _ => Quat::IDENTITY,
            }
        }
        SwingBat | RecoverSwing => Quat::IDENTITY,
    }
}

// ── Systems ───────────────────────────────────────────────────────────────────

/// Poses every playing rig from `(clip, progress)`, chains `next`, and removes
/// finished one-shots. The only code in the game that rotates rig parts.
fn sample_clips(
    time: Res<Time>,
    mut commands: Commands,
    mut playing_q: Query<(Entity, &mut Playing, &mut Transform, Option<&Children>)>,
    mut limb_q: Query<(&RigLimb, &mut Transform), Without<Playing>>,
) {
    for (entity, mut playing, mut transform, children) in &mut playing_q {
        playing.timer.tick(time.delta());
        let f = playing.timer.fraction();

        if let Some(rot) = self_pose(playing.clip, f) {
            transform.rotation = rot;
        } else if let Some(children) = children {
            for &child in children {
                if let Ok((limb, mut limb_tf)) = limb_q.get_mut(child) {
                    limb_tf.rotation = limb_pose(playing.clip, limb.kind, f);
                }
            }
        }

        if playing.timer.finished() && !playing.clip.looping() {
            if let Some(next) = playing.next.take() {
                playing.clip = next;
                playing.timer = Timer::from_seconds(next.duration(), TimerMode::Once);
            } else {
                if self_pose(playing.clip, 1.0).is_some() {
                    transform.rotation = bat_idle_rotation();
                }
                commands.entity(entity).remove::<Playing>();
            }
        }
    }
}

/// Returns limbs to neutral whenever a clip stops (covers both the sampler's
/// own removal and choreography removing `RunCycle` mid-loop).
fn settle_removed(
    mut removed: RemovedComponents<Playing>,
    children_q: Query<&Children>,
    mut limb_q: Query<(&RigLimb, &mut Transform)>,
) {
    for entity in removed.read() {
        if let Ok(children) = children_q.get(entity) {
            for &child in children {
                if let Ok((_, mut limb_tf)) = limb_q.get_mut(child) {
                    limb_tf.rotation = Quat::IDENTITY;
                }
            }
        }
    }
}

/// Distance that counts as "arrived".
const ARRIVE_EPS: f32 = 0.2;

/// Steps every rig with a `MoveIntent` toward its target, faces it along the
/// travel direction, and keeps `RunCycle` playing while it moves.
fn locomote(
    time: Res<Time>,
    mut commands: Commands,
    mut movers: Query<(Entity, &mut Transform, &mut MoveIntent, Option<&Playing>)>,
) {
    let dt = time.delta_secs();
    for (entity, mut transform, mut intent, playing) in &mut movers {
        let Some(target) = intent.target else { continue };
        let mut to = target - transform.translation;
        to.y = 0.0; // rigs stay at their spawn height
        let dist = to.length();
        if dist <= ARRIVE_EPS {
            intent.target = None;
            if playing.is_some_and(|p| p.clip == AnimClip::RunCycle) {
                commands.entity(entity).remove::<Playing>();
            }
            continue;
        }
        let dir = to / dist;
        transform.translation += dir * (intent.speed * dt).min(dist);
        transform.rotation = Quat::from_rotation_y(dir.x.atan2(dir.z));
        if playing.is_none() {
            commands.entity(entity).insert(Playing::new(AnimClip::RunCycle));
        }
    }
}

// ── Plugin ────────────────────────────────────────────────────────────────────

pub struct AnimationPlugin;

impl Plugin for AnimationPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (locomote, sample_clips, settle_removed)
                .chain()
                .run_if(in_state(GameState::Playing)),
        );
    }
}
```

- [ ] **Step 2: Register in `mod.rs`** — add `pub mod animation;`, `use animation::AnimationPlugin;`, and `AnimationPlugin` in the `add_plugins` tuple (after `PlayerPlugin`).

- [ ] **Step 3: Verify** — `cargo check` passes (expect `dead_code` warnings for not-yet-used items; silence with `#[allow(dead_code)]` on `Playing::then`, `MoveIntent`, `bat_idle_rotation` only if clippy is run in CI — otherwise leave, they're consumed by Task 2/3).

- [ ] **Step 4: Commit** — `git commit -m "feat: procedural animation module (clips, sampler, locomotion)"`

### Task 2: Limbs on every rig + bat-swing migration

**Files:**
- Modify: `src/game/player.rs`

**Interfaces:**
- Consumes: `AnimClip`, `Playing`, `RigLimb`, `LimbKind`, `MoveIntent`, `bat_idle_rotation` from `animation.rs`.
- Produces: every rig root has `MoveIntent::default()`; batter's pivot entity has marker `pub(crate) struct BatPivot`; `RigMeshes` gains `limb: Handle<Mesh>` field. `Swing` enum, `animate_swing`, `idle_rotation`, `sweep_rotation`, `SWING_SECS`, `RECOVER_SECS`, `SWEEP_FROM`, `SWEEP_TO` are DELETED from player.rs.

- [ ] **Step 1: Add limbs and facing rotation to `spawn_rig`.** Bake facing into the root rotation instead of the brim offset (locomotion rotates roots, so offsets must be facing-agnostic):

```rust
// in spawn_rig: root spawn gains a rotation, brim moves to local +Z
.spawn((
    GameplayEntity,
    FacingDirection(Vec3::Z * facing),
    Transform::from_translation(position).with_rotation(Quat::from_rotation_y(
        if facing < 0.0 { std::f32::consts::PI } else { 0.0 },
    )),
    Visibility::default(),
    RigidBody::KinematicPositionBased,
    Collider::capsule_y(0.6, 0.4),
    CollisionGroups::new(PLAYER_GROUP, Group::ALL),
    MoveIntent::default(),
))
// brim child: Transform::from_xyz(0.0, 0.9, 0.19)  ← no more `* facing`
```

and after the brim, the four limb joints:

```rust
for (kind, x, y) in [
    (LimbKind::ArmL, 0.36, 0.30),
    (LimbKind::ArmR, -0.36, 0.30),
    (LimbKind::LegL, 0.14, -0.40),
    (LimbKind::LegR, -0.14, -0.40),
] {
    rig.spawn((
        RigLimb { kind },
        Transform::from_xyz(x, y, 0.0),
        Visibility::default(),
    ))
    .with_children(|joint| {
        joint.spawn((
            RigPart { unit, part: PartKind::Jersey },
            Mesh3d(meshes.limb.clone()),
            MeshMaterial3d(mats.jersey.clone()),
            Transform::from_xyz(0.0, -0.26, 0.0),
        ));
    });
}
```

`RigMeshes` gains `limb: meshes.add(Cylinder::new(0.055, 0.5))`.

- [ ] **Step 2: Migrate the swing.** Delete the `Swing` enum, `animate_swing`, `idle_rotation`, `sweep_rotation`, and the four swing constants. Add `#[derive(Component)] pub(crate) struct BatPivot;`. Batter spawn: pivot gets `(BatPivot, Transform::from_translation(Vec3::new(-0.25, 0.45, 0.0)).with_rotation(bat_idle_rotation()), Visibility::default())`. Rewrite `trigger_swing`:

```rust
fn trigger_swing(
    intents: Res<Intents>,
    score: Res<ScoreBoard>,
    play: Res<Play>,
    pivots: Query<(Entity, Option<&Playing>), With<BatPivot>>,
    mut commands: Commands,
) {
    if !matches!(play.phase, Phase::PrePitch | Phase::Pitch) {
        return;
    }
    if !intents.get(score.batting_team()).action {
        return;
    }
    for (entity, playing) in &pivots {
        if playing.is_none() {
            commands
                .entity(entity)
                .insert(Playing::then(AnimClip::SwingBat, AnimClip::RecoverSwing));
        }
    }
}
```

Remove `animate_swing` from the plugin's system tuple (now just `(recolor_teams, trigger_swing)`).

- [ ] **Step 3: Verify** — `cargo check`, then `cargo run --features dev`: swing still animates on Space; players have arms/legs; pitcher/fielder caps face home.

- [ ] **Step 4: Commit** — `git commit -m "feat: limbed rigs and clip-driven bat swing"`

### Task 3: `Phase::WindUp` and the pitcher's delivery

**Files:**
- Modify: `src/game/flow.rs`, `src/game/ai.rs`, `src/game/player.rs`

**Interfaces:**
- Produces: `Phase` gains `WindUp` variant (between `PrePitch` and `Pitch`); `Play` gains `pending_pitch: Option<Vec2>` field. Tasks 7/12 modify these further.

- [ ] **Step 1: flow.rs.** Add `WindUp` to `Phase`. Add `pending_pitch: Option<Vec2>` to `Play` (default `None`). `pre_pitch` no longer sends `PitchEvent`; instead:

```rust
// in pre_pitch, replacing the pitch_ev.send block:
if intent.action {
    play.pending_pitch = Some(intent.aim);
    play.phase = Phase::WindUp;
    play.timer = Timer::from_seconds(AnimClip::WindUp.duration(), TimerMode::Once);
    play.crossing = None;
    play.resolved = false;
    for pitcher in &pitcher_q {
        commands
            .entity(pitcher)
            .insert(Playing::then(AnimClip::WindUp, AnimClip::ThrowRelease));
    }
}
```

(`pre_pitch` gains `pitcher_q: Query<Entity, With<Pitcher>>` and `mut commands: Commands`, loses `pitch_ev`.) New system between `pre_pitch` and `pitch_live` in the chain:

```rust
/// WindUp: the pitcher's delivery plays out, then the ball leaves the hand.
fn wind_up(
    time: Res<Time>,
    mut play: ResMut<Play>,
    field: Res<FieldSpec>,
    mut pitch_ev: EventWriter<PitchEvent>,
) {
    if play.phase != Phase::WindUp {
        return;
    }
    if play.timer.tick(time.delta()).finished() {
        let aim = play.pending_pitch.take().unwrap_or(Vec2::ZERO);
        pitch_ev.send(PitchEvent {
            velocity: rules::pitch_velocity(aim, field.pitch_distance),
        });
        play.phase = Phase::Pitch;
    }
}
```

- [ ] **Step 2: ai.rs.** `cpu_offense`'s reset gate becomes `matches!(play.phase, Phase::PrePitch | Phase::WindUp)`. (`cpu_defense` needs no change: its `!= PrePitch` branch already releases the button during `WindUp`.)

- [ ] **Step 3: player.rs.** `trigger_swing` gate becomes `matches!(play.phase, Phase::PrePitch | Phase::WindUp | Phase::Pitch)`.

- [ ] **Step 4: Verify** — `cargo test` (flow has no tests but rules must stay green), `cargo run --features dev`: pitcher visibly winds up ~0.5 s before each pitch, CPU still pitches, batter can start a swing during the windup. Camera stays in duel framing through `WindUp` (the `_ =>` arm in `broadcast_camera` covers it — confirm visually).

- [ ] **Step 5: Commit** — `git commit -m "feat: pitcher windup phase before every pitch"`

### Task 4: `fx.rs` — hit-stop + camera kick

**Files:**
- Create: `src/game/fx.rs`
- Modify: `src/game/mod.rs` (register `FxPlugin`), `src/game/camera.rs`

**Interfaces:**
- Produces: `pub struct FxPlugin`; camera.rs owns `CameraKick(Vec3)` resource applied to the broadcast eye. Task 5 adds particle systems to fx.rs.

- [ ] **Step 1: fx.rs with hit-stop**

```rust
//! Game feel — hit-stop and impact particles. Purely cosmetic.

use bevy::prelude::*;

use crate::game::ball::HitEvent;
use crate::game::GameState;

/// How hard time slows on contact, and for how long (real seconds).
const HIT_STOP_SCALE: f32 = 0.05;
const HIT_STOP_SECS: f32 = 0.06;

#[derive(Resource, Default)]
struct HitStop(Option<Timer>);

fn start_hit_stop(
    mut hits: EventReader<HitEvent>,
    mut virt: ResMut<Time<Virtual>>,
    mut stop: ResMut<HitStop>,
) {
    if hits.read().next().is_some() {
        virt.set_relative_speed(HIT_STOP_SCALE);
        stop.0 = Some(Timer::from_seconds(HIT_STOP_SECS, TimerMode::Once));
    }
}

fn end_hit_stop(
    real: Res<Time<Real>>,
    mut virt: ResMut<Time<Virtual>>,
    mut stop: ResMut<HitStop>,
) {
    let finished = stop
        .0
        .as_mut()
        .is_some_and(|t| t.tick(real.delta()).finished());
    if finished {
        virt.set_relative_speed(1.0);
        stop.0 = None;
    }
}

pub struct FxPlugin;

impl Plugin for FxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HitStop>().add_systems(
            Update,
            (start_hit_stop, end_hit_stop).run_if(in_state(GameState::Playing)),
        );
    }
}
```

Register `FxPlugin` in mod.rs after `FlowPlugin`.

- [ ] **Step 2: camera kick.** In camera.rs add:

```rust
/// Impulse added to the broadcast eye on contact; decays on real time so the
/// kick rides through the hit-stop.
#[derive(Resource, Default)]
struct CameraKick(Vec3);

fn kick_on_hit(mut hits: EventReader<HitEvent>, mut kick: ResMut<CameraKick>) {
    for _ in hits.read() {
        kick.0 += Vec3::new(0.0, 0.18, -0.35);
    }
}

fn decay_kick(real: Res<Time<Real>>, mut kick: ResMut<CameraKick>) {
    kick.0 *= (-14.0 * real.delta_secs()).exp();
}
```

`broadcast_camera` gains `kick: Res<CameraKick>` and its final line becomes `*cam = Transform::from_translation(rig.eye + kick.0).looking_at(rig.target, Vec3::Y);`. Register `(kick_on_hit, decay_kick)` in `Update` under `in_state(GameState::Playing)`, and `init_resource::<CameraKick>()`. Import `crate::game::ball::HitEvent`.

- [ ] **Step 3: Verify** — `cargo run --features dev`: contact visibly "pops" (brief freeze + camera flinch). Confirm the ball's flight afterwards looks unchanged (Rapier follows the virtual clock; if the ball keeps full speed during the freeze, drop `HIT_STOP_SCALE` handling to also set `RapierConfiguration` time-scale — investigate before shipping the task).

- [ ] **Step 4: Commit** — `git commit -m "feat: hit-stop and camera kick on contact"`

### Task 5: Contact burst, bounce dust, grass roll drag

**Files:**
- Modify: `src/game/fx.rs`, `src/game/ai.rs` (make `hash01`/`noise` `pub(crate)`), `src/game/ball.rs`

**Interfaces:**
- Consumes: `hash01(seed: f32) -> f32`, `noise(seed: f32) -> f32` from ai.rs (change `fn` → `pub(crate) fn`).

- [ ] **Step 1: particles in fx.rs**

```rust
use bevy_rapier3d::prelude::{CollisionEvent, Velocity};

use crate::game::ai::{hash01, noise};
use crate::game::ball::Baseball;
use crate::game::theme::Theme;
use crate::game::GameplayEntity;

/// One transient effect mote: moves, scales, dies.
#[derive(Component)]
struct Particle {
    vel: Vec3,
    timer: Timer,
    gravity: f32,
    /// Positive = expands to (1 + grow); negative = shrinks to nothing.
    grow: f32,
}

#[derive(Resource)]
struct FxAssets {
    spark_mesh: Handle<Mesh>,
    dust_mesh: Handle<Mesh>,
    spark: Handle<StandardMaterial>,
    dust: Handle<StandardMaterial>,
}

fn build_fx_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    theme: Res<Theme>,
) {
    commands.insert_resource(FxAssets {
        spark_mesh: meshes.add(Sphere::new(0.07)),
        dust_mesh: meshes.add(Sphere::new(0.14)),
        spark: materials.add(StandardMaterial {
            base_color: theme.ball.trail,
            unlit: true,
            ..default()
        }),
        dust: materials.add(StandardMaterial {
            base_color: Color::srgba(0.75, 0.7, 0.6, 1.0),
            unlit: true,
            ..default()
        }),
    });
}

fn contact_burst(
    mut hits: EventReader<HitEvent>,
    ball_q: Query<&Transform, With<Baseball>>,
    assets: Option<Res<FxAssets>>,
    time: Res<Time>,
    mut commands: Commands,
) {
    let Some(assets) = assets else { return };
    for _ in hits.read() {
        let Ok(ball) = ball_q.get_single() else { continue };
        for i in 0..10 {
            let seed = time.elapsed_secs() * 13.7 + i as f32 * 1.618;
            let dir = Vec3::new(
                noise(seed),
                hash01(seed * 1.3) * 0.8 + 0.2,
                noise(seed * 1.7),
            )
            .normalize_or_zero();
            commands.spawn((
                Particle {
                    vel: dir * (4.0 + hash01(seed * 2.1) * 5.0),
                    timer: Timer::from_seconds(0.35, TimerMode::Once),
                    gravity: 4.0,
                    grow: -1.0,
                },
                GameplayEntity,
                Mesh3d(assets.spark_mesh.clone()),
                MeshMaterial3d(assets.spark.clone()),
                Transform::from_translation(ball.translation),
            ));
        }
    }
}

/// Threshold impact speed for a dust puff (m/s).
const DUST_MIN_SPEED: f32 = 4.0;

fn bounce_dust(
    mut collisions: EventReader<CollisionEvent>,
    ball_q: Query<(Entity, &Transform, &Velocity), With<Baseball>>,
    assets: Option<Res<FxAssets>>,
    time: Res<Time>,
    mut commands: Commands,
) {
    let Some(assets) = assets else { return };
    let Ok((ball_entity, ball_tf, vel)) = ball_q.get_single() else { return };
    for event in collisions.read() {
        let CollisionEvent::Started(a, b, _) = event else { continue };
        if *a != ball_entity && *b != ball_entity {
            continue;
        }
        if vel.linvel.length() < DUST_MIN_SPEED {
            continue;
        }
        for i in 0..6 {
            let seed = time.elapsed_secs() * 9.1 + i as f32 * 2.399;
            commands.spawn((
                Particle {
                    vel: Vec3::new(noise(seed) * 1.6, 0.6 + hash01(seed * 1.9), noise(seed * 2.3) * 1.6),
                    timer: Timer::from_seconds(0.4, TimerMode::Once),
                    gravity: 0.8,
                    grow: 1.6,
                },
                GameplayEntity,
                Mesh3d(assets.dust_mesh.clone()),
                MeshMaterial3d(assets.dust.clone()),
                Transform::from_translation(Vec3::new(ball_tf.translation.x, 0.08, ball_tf.translation.z)),
            ));
        }
    }
}

fn tick_particles(
    time: Res<Time>,
    mut particles: Query<(Entity, &mut Particle, &mut Transform)>,
    mut commands: Commands,
) {
    let dt = time.delta_secs();
    for (entity, mut particle, mut transform) in &mut particles {
        particle.vel.y -= particle.gravity * dt;
        let step = particle.vel * dt;
        transform.translation += step;
        let f = particle.timer.tick(time.delta()).fraction();
        transform.scale = Vec3::splat(if particle.grow >= 0.0 {
            1.0 + particle.grow * f
        } else {
            (1.0 - f).max(0.01)
        });
        if particle.timer.finished() {
            commands.entity(entity).despawn();
        }
    }
}
```

Add `build_fx_assets` to `OnEnter(GameState::Playing)` and `(contact_burst, bounce_dust, tick_particles)` to the plugin's `Update` set.

- [ ] **Step 2: grass roll drag in ball.rs.** `apply_drag` query gains `&Transform`:

```rust
fn apply_drag(
    mut query: Query<(&Transform, &mut Velocity), (With<Baseball>, With<InFlight>)>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();
    for (transform, mut vel) in &mut query {
        let speed = vel.linvel.length();
        if speed > 0.0 {
            let drag = -BALL_DRAG_FACTOR * speed * speed * vel.linvel / speed;
            vel.linvel += drag * dt;
        }
        // Rolling resistance: once the ball is down and barely bouncing,
        // bleed horizontal speed so grounders die instead of rolling forever.
        if transform.translation.y < BALL_RADIUS * 3.0 && vel.linvel.y.abs() < 1.0 {
            let f = (1.0 - 1.6 * dt).max(0.0);
            vel.linvel.x *= f;
            vel.linvel.z *= f;
        }
    }
}
```

- [ ] **Step 3: Verify** — `cargo check && cargo check --target wasm32-unknown-unknown`, then `cargo run --features dev`: sparks at contact, dust on bounces, grounders come to rest in the infield/outfield.

- [ ] **Step 4: Commit** — `git commit -m "feat: contact burst, bounce dust, grass roll drag"`

---

## Phase 2 — Fielder choreography

### Task 6: `rules::predict_landing` (TDD)

**Files:**
- Modify: `src/game/rules.rs`

**Interfaces:**
- Produces: `pub fn predict_landing(vel: Vec3, drag_factor: f32) -> (Vec3, f32)` — landing point (y=0) and hang time for a ball leaving contact height. (Task 12 later extends the signature with spin.)

- [ ] **Step 1: Write failing tests** (in rules.rs `tests` module):

```rust
// ── Landing prediction ────────────────────────────────────────────────────

#[test]
fn dragless_landing_matches_closed_form() {
    let vel = vel_at(30.0, 30.0);
    let (land, t) = predict_landing(vel, 0.0);
    let disc = vel.y * vel.y + 2.0 * GRAVITY * 0.6; // CONTACT_HEIGHT
    let t_expect = (vel.y + disc.sqrt()) / GRAVITY;
    assert!((t - t_expect).abs() < 0.05, "hang time {t} vs {t_expect}");
    let range_expect = Vec2::new(vel.x, vel.z).length() * t_expect;
    let range = Vec2::new(land.x, land.z).length();
    assert!((range - range_expect).abs() < 1.5, "range {range} vs {range_expect}");
}

#[test]
fn drag_shortens_flight() {
    let vel = vel_at(30.0, 40.0);
    let (with_drag, t_drag) = predict_landing(vel, BALL_DRAG_FACTOR);
    let (no_drag, _) = predict_landing(vel, 0.0);
    assert!(Vec2::new(with_drag.x, with_drag.z).length() < Vec2::new(no_drag.x, no_drag.z).length());
    assert!(t_drag > 0.5);
}
```

- [ ] **Step 2: Run** `cargo test predict` — expect compile failure (`predict_landing` not found).

- [ ] **Step 3: Implement** (near `classify_batted_ball`):

```rust
/// Numerically integrates a batted ball's flight from contact height with the
/// same gravity + quadratic-drag model the live ball uses (`ball::apply_drag`),
/// returning the landing point (y = 0) and hang time. This is what fielder
/// choreography chases — the *visual* ball's touchdown, not the balance-tuned
/// range in [`classify_batted_ball`].
pub fn predict_landing(vel: Vec3, drag_factor: f32) -> (Vec3, f32) {
    let mut pos = Vec3::new(0.0, CONTACT_HEIGHT, 0.0);
    let mut v = vel;
    let dt = 1.0 / 120.0;
    let mut t = 0.0;
    while pos.y > 0.0 && t < 15.0 {
        let speed = v.length();
        v += -drag_factor * speed * v * dt;
        v.y -= GRAVITY * dt;
        pos += v * dt;
        t += dt;
    }
    (Vec3::new(pos.x, 0.0, pos.z), t)
}
```

- [ ] **Step 4: Run** `cargo test` — all green.

- [ ] **Step 5: Commit** — `git commit -m "feat: drag-aware landing predictor in rules"`

### Task 7: `BallInPlayEvent` + play-derived InPlay window

**Files:**
- Modify: `src/game/flow.rs`

**Interfaces:**
- Produces: `#[derive(Event, Clone, Copy)] pub struct BallInPlayEvent { pub outcome: Outcome, pub landing: Vec3, pub hang_time: f32 }` — fired once per fair contact. Registered via `add_event` in `FlowPlugin`.

- [ ] **Step 1:** Define the event; add `.add_event::<BallInPlayEvent>()`. In `pitch_live`'s contact branch (non-foul path only), before setting `Phase::InPlay`:

```rust
let (landing, hang_time) =
    rules::predict_landing(velocity, crate::game::ball::BALL_DRAG_FACTOR);
in_play_ev.send(BallInPlayEvent { outcome, landing, hang_time });
play.phase = Phase::InPlay;
play.timer = Timer::from_seconds(
    (hang_time + INPLAY_BUFFER).clamp(INPLAY_MIN, INPLAY_MAX),
    TimerMode::Once,
);
play.resolved = true;
```

with constants replacing `INPLAY_SECS`:

```rust
/// The live-ball window: hang time plus room for the fielding choreography,
/// clamped so grounders don't dawdle and moonshots don't stall the game.
const INPLAY_BUFFER: f32 = 1.2;
const INPLAY_MIN: f32 = 2.2;
const INPLAY_MAX: f32 = 6.5;
```

`pitch_live` gains `mut in_play_ev: EventWriter<BallInPlayEvent>`.

- [ ] **Step 2: Verify** — `cargo test && cargo run --features dev`: deep flies stay live until they land; weak grounders wrap up on the old cadence.

- [ ] **Step 3: Commit** — `git commit -m "feat: ball-in-play event with play-derived live window"`

### Task 8: `fielding.rs` — chase, catch, scoop, throw, return

**Files:**
- Create: `src/game/fielding.rs`
- Modify: `src/game/mod.rs` (register), `src/game/player.rs` (remove `#[allow(dead_code)]` from `Fielder`)

**Interfaces:**
- Consumes: `BallInPlayEvent`, `Phase`, `Play` (flow.rs); `Fielder` (player.rs); `AnimClip`, `MoveIntent`, `Playing` (animation.rs); `Baseball`, `InFlight` (ball.rs); `GRAVITY` (rules.rs).
- Produces: `pub struct FieldingPlugin`. Nothing else public.

- [ ] **Step 1: Write the module**

```rust
//! Fielder choreography — a cosmetic performance of the outcome that
//! `rules::classify_batted_ball` already decided at contact. Fielders chase
//! the real ball (no teleporting) and move only through `MoveIntent`, the
//! same seam a future player-controlled fielder will write.

use bevy::prelude::*;
use bevy_rapier3d::prelude::Velocity;

use crate::game::animation::{AnimClip, MoveIntent, Playing};
use crate::game::ball::{Baseball, InFlight};
use crate::game::flow::{BallInPlayEvent, Phase, Play};
use crate::game::player::Fielder;
use crate::game::rules::{self, OutKind, Outcome};
use crate::game::variant::FieldSpec;
use crate::game::GameState;

/// Sprint speed while chasing (m/s) and jog speed returning to position.
const CHASE_SPEED: f32 = 7.0;
const RETURN_SPEED: f32 = 4.0;
/// Horizontal distance that counts as "on the ball".
const REACH: f32 = 0.9;
/// Horizontal speed of a cosmetic lob throw.
const THROW_SPEED: f32 = 16.0;

/// Choreography state for the current live ball.
#[derive(Resource, Default)]
struct ActivePlay(PlayState);

#[derive(Default, Clone, Copy, PartialEq)]
enum PlayState {
    #[default]
    Idle,
    Chasing {
        chaser: Entity,
        outcome: Outcome,
        airborne: bool,
    },
    Thrown {
        catcher: Entity,
    },
    Done,
}

/// A lobbed velocity from `from` to `to` under gravity.
fn lob_velocity(from: Vec3, to: Vec3) -> Vec3 {
    let flat = Vec3::new(to.x - from.x, 0.0, to.z - from.z);
    let d = flat.length().max(0.1);
    let t = (d / THROW_SPEED).clamp(0.4, 1.2);
    let vy = 0.5 * rules::GRAVITY * t + (to.y - from.y) / t;
    flat / t + Vec3::Y * vy
}

fn horizontal_distance(a: Vec3, b: Vec3) -> f32 {
    Vec2::new(a.x - b.x, a.z - b.z).length()
}

/// On contact, the fielder nearest the predicted landing point takes the job.
fn assign_on_contact(
    mut events: EventReader<BallInPlayEvent>,
    field: Res<FieldSpec>,
    mut fielders: Query<(Entity, &Transform, &mut MoveIntent), With<Fielder>>,
    mut active: ResMut<ActivePlay>,
) {
    for ev in events.read() {
        // Don't chase over the fence: cap the run at the warning track.
        let mut target = ev.landing;
        let flat = Vec2::new(target.x, target.z);
        let max = field.fence_line.min(field.fence_center) - 3.0;
        if flat.length() > max {
            let capped = flat.normalize_or_zero() * max;
            target = Vec3::new(capped.x, 0.0, capped.y);
        }

        let Some((chaser, _, _)) = fielders
            .iter()
            .min_by(|a, b| {
                horizontal_distance(a.1.translation, target)
                    .total_cmp(&horizontal_distance(b.1.translation, target))
            })
        else {
            continue;
        };
        if let Ok((_, _, mut intent)) = fielders.get_mut(chaser) {
            intent.target = Some(target);
            intent.speed = CHASE_SPEED;
        }
        active.0 = PlayState::Chasing {
            chaser,
            outcome: ev.outcome,
            airborne: matches!(ev.outcome, Outcome::Out(OutKind::Fly | OutKind::Pop)),
        };
    }
}

/// Runs the chase to its finish: a catch (fly/pop), or a scoop and throw.
#[allow(clippy::too_many_arguments)]
fn chase_and_gather(
    mut active: ResMut<ActivePlay>,
    field: Res<FieldSpec>,
    mut ball_q: Query<(Entity, &Transform, &mut Velocity), With<Baseball>>,
    mut fielders: Query<(Entity, &Transform, &mut MoveIntent), With<Fielder>>,
    mut commands: Commands,
) {
    let PlayState::Chasing { chaser, outcome, airborne } = active.0 else {
        return;
    };
    let Ok((ball_entity, ball_tf, mut ball_vel)) = ball_q.get_single_mut() else {
        return;
    };
    let Ok((_, chaser_tf, mut intent)) = fielders.get_mut(chaser) else {
        active.0 = PlayState::Done;
        return;
    };
    let ball_pos = ball_tf.translation;
    let chaser_pos = chaser_tf.translation;
    let d = horizontal_distance(ball_pos, chaser_pos);

    if airborne {
        // Camp under the predicted landing point; glove the ball as it drops in.
        if d < 1.2 && ball_pos.y < 2.4 && ball_vel.linvel.y < 0.0 {
            ball_vel.linvel = Vec3::ZERO;
            ball_vel.angvel = Vec3::ZERO;
            commands.entity(ball_entity).remove::<InFlight>();
            commands.entity(chaser).insert(Playing::new(AnimClip::GloveUp));
            intent.target = None;
            active.0 = PlayState::Done;
        }
        return;
    }

    // Grounders and hits: run at the real ball until it's in reach.
    intent.target = Some(ball_pos);
    intent.speed = CHASE_SPEED;
    if d < REACH && ball_pos.y < 1.2 {
        commands.entity(chaser).insert(Playing::new(AnimClip::ScoopBall));
        intent.target = None;

        match outcome {
            Outcome::Out(OutKind::Ground) | Outcome::Out(OutKind::Pegged) => {
                // Fire it to the fielder nearest first base for the (already
                // recorded) out. FieldSpec has no named positions —
                // nearest-to-base is the rule.
                let first = field
                    .base_positions
                    .first()
                    .copied()
                    .unwrap_or(Vec3::ZERO);
                let receiver = fielders
                    .iter()
                    .filter(|(e, _, _)| *e != chaser)
                    .min_by(|a, b| {
                        horizontal_distance(a.1.translation, first)
                            .total_cmp(&horizontal_distance(b.1.translation, first))
                    })
                    .map(|(e, tf, _)| (e, tf.translation));
                if let Some((catcher, catcher_pos)) = receiver {
                    ball_vel.linvel =
                        lob_velocity(ball_pos, catcher_pos + Vec3::Y * 0.6);
                    ball_vel.angvel = Vec3::ZERO;
                    commands.entity(catcher).insert(Playing::new(AnimClip::GloveUp));
                    active.0 = PlayState::Thrown { catcher };
                } else {
                    ball_vel.linvel = Vec3::ZERO;
                    active.0 = PlayState::Done;
                }
            }
            _ => {
                // A hit: lob it back in toward the mound and let it land.
                ball_vel.linvel = lob_velocity(
                    ball_pos,
                    Vec3::new(0.0, 0.6, field.pitch_distance),
                );
                ball_vel.angvel = Vec3::ZERO;
                active.0 = PlayState::Done;
            }
        }
    }
}

/// The throw arrives: the receiver stops the ball dead.
fn receive_throw(
    mut active: ResMut<ActivePlay>,
    mut ball_q: Query<(Entity, &Transform, &mut Velocity), With<Baseball>>,
    fielders: Query<&Transform, With<Fielder>>,
    mut commands: Commands,
) {
    let PlayState::Thrown { catcher } = active.0 else {
        return;
    };
    let Ok((ball_entity, ball_tf, mut ball_vel)) = ball_q.get_single_mut() else {
        return;
    };
    let Ok(catcher_tf) = fielders.get(catcher) else {
        active.0 = PlayState::Done;
        return;
    };
    let arrived = ball_tf.translation.distance(catcher_tf.translation) < 1.0
        || (ball_tf.translation.y < 0.15 && ball_vel.linvel.length() < 2.0);
    if arrived {
        ball_vel.linvel = Vec3::ZERO;
        ball_vel.angvel = Vec3::ZERO;
        commands.entity(ball_entity).remove::<InFlight>();
        active.0 = PlayState::Done;
    }
}

/// During the result pause, everyone jogs back to their spot.
fn return_to_spots(
    play: Res<Play>,
    field: Res<FieldSpec>,
    mut active: ResMut<ActivePlay>,
    mut fielders: Query<(&Fielder, &Transform, &mut MoveIntent)>,
) {
    if play.phase != Phase::Result || active.0 == PlayState::Idle {
        return;
    }
    active.0 = PlayState::Idle;
    for (fielder, tf, mut intent) in &mut fielders {
        let Some(spot) = field.fielder_positions.get(fielder.index) else {
            continue;
        };
        if Vec2::new(tf.translation.x - spot.x, tf.translation.z - spot.z).length() > 0.3 {
            intent.target = Some(*spot);
            intent.speed = RETURN_SPEED;
        }
    }
}

fn reset_active(mut active: ResMut<ActivePlay>) {
    active.0 = PlayState::Idle;
}

pub struct FieldingPlugin;

impl Plugin for FieldingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ActivePlay>()
            .add_systems(OnEnter(GameState::Playing), reset_active)
            .add_systems(
                Update,
                (assign_on_contact, chase_and_gather, receive_throw, return_to_spots)
                    .chain()
                    .run_if(in_state(GameState::Playing)),
            );
    }
}
```

Note: `Outcome` needs `Copy` on `PlayState` — it already derives `Clone, Copy` in rules.rs. `assign_on_contact` borrows: the `min_by` iterates immutably then `get_mut` re-borrows — fine because the first borrow ends.

- [ ] **Step 2:** Register `FieldingPlugin` in mod.rs (after `FlowPlugin`). Remove the now-stale `#[allow(dead_code)]` on `Fielder` in player.rs.

- [ ] **Step 3: Verify** — `cargo check` both targets, `cargo test`, then `cargo run --features dev` and watch several at-bats: fly outs get camped under and gloved; grounders get charged, scooped, thrown to the first-base side; hits get chased down and lobbed in; everyone jogs home during the result pause.

- [ ] **Step 4: Commit** — `git commit -m "feat: fielder choreography (chase, catch, scoop, throw, return)"`

---

## Phase 3 — Base runners

### Task 9: Rig sharing + runner infrastructure

**Files:**
- Modify: `src/game/player.rs`
- Create: `src/game/runner.rs`
- Modify: `src/game/mod.rs` (register `RunnerPlugin`)

**Interfaces:**
- player.rs produces (all `pub(crate)`): `RigMeshes` becomes a `#[derive(Resource)]` inserted at spawn (fields unchanged plus `limb`), `spawn_rig`, `RigUnit`, `RigMaterials`, `TeamPalette` (+ its `for_team`).
- runner.rs produces: `pub struct RunnerPlugin`; components `Runner { base: usize }`, `BasePath { waypoints: Vec<Vec3>, next: usize }`, `DespawnAtPathEnd` (all private).

- [ ] **Step 1: player.rs exports.** Add `#[derive(Resource)]` to `RigMeshes`; `spawn_players` ends with `commands.insert_resource(rig_meshes);` (build it, use it, then move it into the resource — reorder so all `spawn_rig` calls happen first). Mark `pub(crate)`: `RigMeshes` + fields, `RigUnit`, `RigMaterials`, `TeamPalette` (+ `for_team`), `spawn_rig`.

- [ ] **Step 2: runner.rs**

```rust
//! Base-runner rigs — pure visualization of the `Bases` truth in rules.rs.
//! Runners never decide anything; they mirror occupancy after each play.

use bevy::prelude::*;

use crate::game::animation::MoveIntent;
use crate::game::player::{spawn_rig, RigMeshes, RigUnit, TeamPalette};
use crate::game::rules::Bases;
use crate::game::variant::FieldSpec;
use crate::game::{GameState, ScoreBoard};

const RUN_SPEED: f32 = 6.0;
/// Rig-root height above the base pad.
const RIG_Y: f32 = 0.6;
/// Where a new runner starts (the batter's box).
const PLATE_START: Vec3 = Vec3::new(0.7, RIG_Y, 0.0);

/// A rig standing on (or running to) 0-indexed base `base`.
#[derive(Component)]
struct Runner {
    base: usize,
}

/// Waypoints the rig visits in order (fed one at a time into `MoveIntent`).
#[derive(Component)]
struct BasePath {
    waypoints: Vec<Vec3>,
    next: usize,
}

/// Despawn the rig when its path is exhausted (scored / cleared / ghost run).
#[derive(Component)]
struct DespawnAtPathEnd;

fn base_pos(field: &FieldSpec, base: usize) -> Vec3 {
    field.base_positions[base] + Vec3::Y * RIG_Y
}

/// Waypoints for advancing from `from` (None = home plate) to `to` inclusive.
fn path_between(field: &FieldSpec, from: Option<usize>, to: usize) -> Vec<Vec3> {
    let start = from.map_or(0, |f| f + 1);
    (start..=to).map(|b| base_pos(field, b)).collect()
}

/// Waypoints from base `from` around the remaining bases and home.
fn path_home(field: &FieldSpec, from: usize) -> Vec<Vec3> {
    let mut wp: Vec<Vec3> = ((from + 1)..field.base_count())
        .map(|b| base_pos(field, b))
        .collect();
    wp.push(Vec3::new(0.0, RIG_Y, 0.0));
    wp
}

/// Feeds the next waypoint whenever the rig has arrived at the previous one.
fn advance_paths(
    mut movers: Query<(
        Entity,
        &mut BasePath,
        &mut MoveIntent,
        Option<&DespawnAtPathEnd>,
    )>,
    mut commands: Commands,
) {
    for (entity, mut path, mut intent, despawn) in &mut movers {
        if intent.target.is_some() {
            continue;
        }
        if path.next < path.waypoints.len() {
            intent.target = Some(path.waypoints[path.next]);
            intent.speed = RUN_SPEED;
            path.next += 1;
        } else if despawn.is_some() {
            commands.entity(entity).despawn_recursive();
        }
    }
}

/// Mirrors `Bases` after every change: existing runners advance (greedy,
/// most-advanced first), a new runner appears for the batter's base, and
/// leftovers (scored, or wiped by a half-inning flip) run home and leave.
fn sync_runners(
    bases: Res<Bases>,
    field: Res<FieldSpec>,
    score: Res<ScoreBoard>,
    rig_meshes: Option<Res<RigMeshes>>,
    palette: Option<Res<TeamPalette>>,
    mut runners: Query<(Entity, &mut Runner)>,
    mut commands: Commands,
) {
    if !bases.is_changed() {
        return;
    }
    let (Some(rig_meshes), Some(palette)) = (rig_meshes, palette) else {
        return;
    };

    // Existing runners, most advanced first.
    let mut pool: Vec<(Entity, usize)> = runners.iter().map(|(e, r)| (e, r.base)).collect();
    pool.sort_by(|a, b| b.1.cmp(&a.1));

    let mut unmatched: Vec<usize> = Vec::new();
    let occupied: Vec<usize> = (0..bases.count()).filter(|&b| bases.is_occupied(b)).collect();

    for &target in occupied.iter().rev() {
        if let Some(i) = pool.iter().position(|&(_, from)| from <= target) {
            let (entity, from) = pool.remove(i);
            if from != target {
                commands.entity(entity).insert(BasePath {
                    waypoints: path_between(&field, Some(from), target),
                    next: 0,
                });
                if let Ok((_, mut runner)) = runners.get_mut(entity) {
                    runner.base = target;
                }
            }
        } else {
            unmatched.push(target);
        }
    }

    // The batter reaching base: spawn a fresh runner at the plate.
    for target in unmatched {
        let mats = palette.for_team(score.batting_team());
        let entity = spawn_rig(
            &mut commands,
            &rig_meshes,
            RigUnit::Batter,
            mats,
            PLATE_START,
            1.0,
        );
        commands.entity(entity).insert((
            Runner { base: target },
            BasePath {
                waypoints: path_between(&field, None, target),
                next: 0,
            },
        ));
    }

    // Leftovers scored or were cleared: run home and leave the field.
    for (entity, from) in pool {
        commands.entity(entity).insert((
            BasePath {
                waypoints: path_home(&field, from),
                next: 0,
            },
            DespawnAtPathEnd,
        ));
        commands.entity(entity).remove::<Runner>();
    }
}

pub struct RunnerPlugin;

impl Plugin for RunnerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (sync_runners, advance_paths)
                .chain()
                .run_if(in_state(GameState::Playing)),
        );
    }
}
```

- [ ] **Step 3:** Register `RunnerPlugin` in mod.rs after `FieldingPlugin`.

- [ ] **Step 4: Verify** — `cargo check` both targets; `cargo run --features dev`: after a single, a runner rig stands on first; after a follow-up double, they run first→second→third while a new runner takes second… wait — the batter takes second on a double, the runner from first reaches third. Confirm exactly that on screen. Walks push the chain. After the third out, everyone jogs off.

- [ ] **Step 5: Commit** — `git commit -m "feat: base-runner rigs mirroring the bases state"`

### Task 10: Batter run-out, home-run trot, batter hand-off

**Files:**
- Modify: `src/game/runner.rs`, `src/game/player.rs` (make `Batter` query usable — it's already `pub`)

**Interfaces:**
- Consumes: `BallInPlayEvent` (flow.rs), `Batter` (player.rs), `Outcome`/`OutKind` (rules.rs).

- [ ] **Step 1: run-outs and trots in runner.rs**

```rust
use crate::game::flow::{BallInPlayEvent, Phase, Play};
use crate::game::player::Batter;
use crate::game::rules::Outcome;

/// On fair contact the batter always runs: to first on an out (ghost run,
/// despawns there), around every base on a home run. Hits/walks are covered
/// by `sync_runners`. The real batter rig hides for the duration and
/// "steps back in" at the next PrePitch.
fn batter_runs(
    mut events: EventReader<BallInPlayEvent>,
    field: Res<FieldSpec>,
    score: Res<ScoreBoard>,
    rig_meshes: Option<Res<RigMeshes>>,
    palette: Option<Res<TeamPalette>>,
    mut batter_q: Query<&mut Visibility, With<Batter>>,
    mut commands: Commands,
) {
    for ev in events.read() {
        let (Some(rig_meshes), Some(palette)) = (&rig_meshes, &palette) else {
            return;
        };
        let (path, keep) = match ev.outcome {
            Outcome::Out(_) => (path_between(&field, None, 0), false),
            Outcome::HomeRun => (path_home(&field, usize::MAX /* see below */), false),
            _ => continue, // hits: sync_runners spawns the real runner
        };
        // For the trot, path_home can't take "no base yet": build it directly.
        let waypoints = if matches!(ev.outcome, Outcome::HomeRun) {
            let mut wp: Vec<Vec3> =
                (0..field.base_count()).map(|b| base_pos(&field, b)).collect();
            wp.push(Vec3::new(0.0, RIG_Y, 0.0));
            wp
        } else {
            path
        };
        let _ = keep;

        let mats = palette.for_team(score.batting_team());
        let entity = spawn_rig(
            &mut commands,
            &rig_meshes,
            RigUnit::Batter,
            mats,
            PLATE_START,
            1.0,
        );
        commands.entity(entity).insert((
            BasePath { waypoints, next: 0 },
            DespawnAtPathEnd,
        ));

        for mut vis in &mut batter_q {
            *vis = Visibility::Hidden;
        }
    }
}
```

Simplify while implementing: compute `waypoints` in the match directly (`Out(_)` → `path_between(&field, None, 0)`, `HomeRun` → full circuit) instead of the `path`/`keep` dance shown above — the shown code documents intent; write the clean version.

Note the hit case: `sync_runners` spawns the batter's runner from `PLATE_START`, which *is* the batter visually replacing themselves — so `batter_runs` must also hide the batter on `Outcome::Hit(_)`. Move the visibility-hiding before the `match` and make the match only decide whether to spawn a ghost.

```rust
/// The next at-bat begins: the batter steps back into the box.
fn batter_returns(play: Res<Play>, mut batter_q: Query<&mut Visibility, With<Batter>>) {
    if play.phase != Phase::PrePitch {
        return;
    }
    for mut vis in &mut batter_q {
        if *vis != Visibility::Inherited {
            *vis = Visibility::Inherited;
        }
    }
}
```

Add both to the plugin: `(batter_runs, sync_runners, advance_paths, batter_returns).chain()`.

- [ ] **Step 2: Verify** — `cargo run --features dev`: ground out → batter sprints toward first while the play happens, vanishes there; home run → full trot; single → batter appears to run to first (the spawned runner), and a "new batter" appears for the next pitch. Strikeouts/walks leave the batter in the box (correct for K; for walks the runner spawn covers the trot to first).

- [ ] **Step 3: Commit** — `git commit -m "feat: batter run-outs and home-run trot"`

---

## Phase 4 — Ball flight character

### Task 11: Magnus force + pitch-kind presets (TDD)

**Files:**
- Modify: `src/game/rules.rs`, `src/game/ball.rs`

**Interfaces:**
- rules.rs produces: `pub enum PitchKind { Fastball, Curveball, Changeup }` with `pub fn speed(self) -> f32`, `pub fn spin(self) -> Vec3`, `pub fn from_aim(aim: Vec2) -> PitchKind`; `pub fn pitch_velocity_kind(kind: PitchKind, aim: Vec2, pitch_distance: f32) -> Vec3` (same solve as `pitch_velocity` with the kind's speed; `pitch_velocity` becomes a thin `Fastball` wrapper or is inlined away — update callers).
- ball.rs produces: `pub const MAGNUS_FACTOR: f32 = 0.0028;`, `PitchEvent` gains `pub spin: Vec3`; new `apply_magnus` system; `rules::hit_spin` used by `apply_hit`.
- rules.rs also produces: `pub fn hit_spin(vel: Vec3) -> Vec3` — sidespin toward the pull side + mild backspin, the single source both the live ball and (Task 12) the predictor use.

- [ ] **Step 1: Failing tests** in rules.rs:

```rust
// ── Pitch kinds & Magnus ──────────────────────────────────────────────────

/// Simulates a full pitch flight with drag + Magnus (the live-ball model).
fn simulate_pitch(kind: PitchKind, aim: Vec2) -> Vec2 {
    let pitch_distance = std_field().pitch_distance;
    let mut pos = mound_reset_pos(pitch_distance);
    let mut vel = pitch_velocity_kind(kind, aim, pitch_distance);
    let spin = kind.spin();
    let dt = 1.0 / 240.0;
    while pos.z > 0.0 {
        let speed = vel.length();
        vel += -BALL_DRAG_FACTOR * speed * vel * dt;
        vel += crate::game::ball::MAGNUS_FACTOR * spin.cross(vel) * dt;
        vel.y -= GRAVITY * dt;
        pos += vel * dt;
        assert!(pos.y > 0.0, "pitch hit the ground before the plate");
    }
    Vec2::new(pos.x, pos.y)
}

#[test]
fn every_kind_centre_aimed_is_a_strike() {
    for kind in [PitchKind::Fastball, PitchKind::Curveball, PitchKind::Changeup] {
        let cross = simulate_pitch(kind, Vec2::ZERO);
        assert!(
            is_in_zone(cross),
            "{kind:?} crossed at ({:.2}, {:.2}) — outside the zone",
            cross.x,
            cross.y
        );
    }
}

#[test]
fn backspin_rides_and_topspin_dives() {
    let fast = simulate_pitch(PitchKind::Fastball, Vec2::ZERO);
    let curve = simulate_pitch(PitchKind::Curveball, Vec2::ZERO);
    assert!(fast.y > curve.y + 0.15, "fastball {fast:?} vs curveball {curve:?}");
}

#[test]
fn aim_maps_to_kinds_per_spec() {
    assert_eq!(PitchKind::from_aim(Vec2::new(0.0, 1.0)), PitchKind::Fastball);
    assert_eq!(PitchKind::from_aim(Vec2::new(0.0, -1.0)), PitchKind::Curveball);
    assert_eq!(PitchKind::from_aim(Vec2::ZERO), PitchKind::Changeup);
}

#[test]
fn hit_spin_pulls_toward_the_spray_side() {
    let pulled = hit_spin(Vec3::new(10.0, 8.0, 20.0));
    let oppo = hit_spin(Vec3::new(-10.0, 8.0, 20.0));
    assert!(pulled.y * oppo.y < 0.0, "sidespin should flip with spray");
}
```

The old `centre_aimed_pitch_is_a_strike` test is superseded — delete it (its drag-model lock lives on inside `simulate_pitch`).

- [ ] **Step 2:** `cargo test pitch` — compile failure (missing types).

- [ ] **Step 3: Implement in rules.rs**

```rust
/// The pitcher's arsenal. Speeds in m/s; spin in rad/s about world axes for a
/// −Z pitch: +X is backspin (Magnus lift), −X topspin (dive), ±Y sweep.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PitchKind {
    Fastball,
    Curveball,
    Changeup,
}

impl PitchKind {
    pub fn speed(self) -> f32 {
        match self {
            PitchKind::Fastball => 38.0,
            PitchKind::Curveball => 31.0,
            PitchKind::Changeup => 29.0,
        }
    }

    pub fn spin(self) -> Vec3 {
        match self {
            PitchKind::Fastball => Vec3::new(24.0, 0.0, 0.0),
            PitchKind::Curveball => Vec3::new(-18.0, 6.0, 0.0),
            PitchKind::Changeup => Vec3::new(6.0, 0.0, 0.0),
        }
    }

    /// Held aim at release selects the pitch (up = fastball, down = curveball,
    /// neutral = changeup). Aim keeps steering location too — aiming high
    /// *means* throwing the heater upstairs.
    pub fn from_aim(aim: Vec2) -> PitchKind {
        if aim.y > 0.35 {
            PitchKind::Fastball
        } else if aim.y < -0.35 {
            PitchKind::Curveball
        } else {
            PitchKind::Changeup
        }
    }
}

pub fn pitch_velocity_kind(kind: PitchKind, aim: Vec2, pitch_distance: f32) -> Vec3 {
    // identical body to pitch_velocity with PITCH_SPEED → kind.speed()
}

/// Spin imparted by the bat: sidespin toward the pull side plus mild backspin.
/// Single source of truth — the live ball and the landing predictor both use it.
pub fn hit_spin(vel: Vec3) -> Vec3 {
    Vec3::new(-6.0, vel.x.signum() * vel.length() * 0.25, 0.0)
}
```

Rewrite `pitch_velocity` as `pitch_velocity_kind(PitchKind::Fastball, aim, pitch_distance)` delegation (flow still calls it until Task 12).

- [ ] **Step 4: ball.rs.** Add `pub const MAGNUS_FACTOR: f32 = 0.0028;` and the system (registered in the Update tuple):

```rust
/// Magnus effect: spin bends flight. `F/m = MAGNUS_FACTOR · (ω × v)`.
fn apply_magnus(
    mut query: Query<&mut Velocity, (With<Baseball>, With<InFlight>)>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();
    for mut vel in &mut query {
        let angvel = vel.angvel;
        vel.linvel += MAGNUS_FACTOR * angvel.cross(vel.linvel) * dt;
    }
}
```

`PitchEvent` gains `pub spin: Vec3`; `apply_pitch` sets `vel.angvel = event.spin` (delete the hardcoded −X backspin — its sign would Magnus the ball *downward*). `apply_hit` sets `vel.angvel = crate::game::rules::hit_spin(event.velocity)`. flow.rs `wind_up` compiles by passing `spin: rules::PitchKind::Fastball.spin()` for now (Task 12 wires selection).

- [ ] **Step 5:** `cargo test` — all green (tune `MAGNUS_FACTOR`/spins if the zone tests fail; the curveball dropping below `ZONE_LOW` means less topspin or more `speed`).

- [ ] **Step 6: Commit** — `git commit -m "feat: Magnus flight and pitch-kind presets"`

### Task 12: Wire pitch selection into flow/AI; predictor learns spin

**Files:**
- Modify: `src/game/flow.rs`, `src/game/ai.rs`, `src/game/rules.rs` (predictor signature + test), `src/game/fielding.rs` (call-site if needed)

**Interfaces:**
- `Play.pending_pitch` becomes `Option<(Vec2, PitchKind)>`.
- `predict_landing(vel: Vec3, spin: Vec3, drag_factor: f32, magnus_factor: f32) -> (Vec3, f32)` — updated signature; Task 6's tests pass `Vec3::ZERO, …, 0.0` where they meant "no aerodynamics".

- [ ] **Step 1: flow.rs.** `pre_pitch` stores `play.pending_pitch = Some((intent.aim, rules::PitchKind::from_aim(intent.aim)))`. `wind_up` sends:

```rust
let (aim, kind) = play.pending_pitch.take().unwrap_or((Vec2::ZERO, rules::PitchKind::Changeup));
pitch_ev.send(PitchEvent {
    velocity: rules::pitch_velocity_kind(kind, aim, field.pitch_distance),
    spin: kind.spin(),
});
```

Delete the now-unused `rules::pitch_velocity` wrapper (keep `pitch_velocity_kind` only) and fix any test callers. The contact branch's predictor call becomes:

```rust
let (landing, hang_time) = rules::predict_landing(
    velocity,
    rules::hit_spin(velocity),
    crate::game::ball::BALL_DRAG_FACTOR,
    crate::game::ball::MAGNUS_FACTOR,
);
```

- [ ] **Step 2: rules.rs.** Extend `predict_landing`:

```rust
pub fn predict_landing(vel: Vec3, spin: Vec3, drag_factor: f32, magnus_factor: f32) -> (Vec3, f32) {
    // integration loop gains: v += magnus_factor * spin.cross(v) * dt;
}
```

Update Task 6's two tests to pass `Vec3::ZERO, drag, 0.0`, and add:

```rust
#[test]
fn sidespin_bends_the_landing_point() {
    let vel = vel_at(25.0, 35.0);
    let (straight, _) = predict_landing(vel, Vec3::ZERO, BALL_DRAG_FACTOR, 0.0);
    let (bent, _) = predict_landing(
        vel,
        hit_spin(Vec3::new(10.0, 8.0, 20.0)),
        BALL_DRAG_FACTOR,
        crate::game::ball::MAGNUS_FACTOR,
    );
    assert!((bent.x - straight.x).abs() > 0.5, "Magnus should bend the carry");
}
```

- [ ] **Step 3: ai.rs.** CPU picks a kind, then shapes its aim to match the selection thresholds:

```rust
// in cpu_defense, replacing the aim computation:
let spread = 0.55 * (1.0 - cfg.skill) + 0.12;
let mut aim = Vec2::new(noise(t * 1.7) * spread, noise(t * 2.3) * spread * 0.5);
let roll = hash01(t * 4.3);
aim.y += if roll < 0.45 {
    0.55 // fastball
} else if roll < 0.75 {
    0.0 // changeup
} else {
    -0.55 // curveball
};
aim = aim.clamp(Vec2::splat(-1.0), Vec2::splat(1.0));
```

- [ ] **Step 4: Verify** — `cargo test`; `cargo check --target wasm32-unknown-unknown`; `cargo run --features dev`: pitches visibly differ (heater rides high and arrives fast; curve is slower with late drop; changeup hangs); batted balls hook/slice toward the pull side; fielders still camp where flies actually land (the predictor now matches Magnus flight).

- [ ] **Step 5: Commit** — `git commit -m "feat: pitch selection via aim, spin-aware landing prediction"`

---

### Task 13: Docs + dual-target sweep

**Files:**
- Modify: `CLAUDE.md` (architecture section)

- [ ] **Step 1:** Add to CLAUDE.md's Architecture section (after the theme paragraph), in its voice, ~4 lines: `animation.rs` is the single posing/locomotion pathway (`AnimClip` sampler + `MoveIntent` — the seams for a future `AnimationGraph` backend and player-controlled fielding); `fx.rs`/`fielding.rs`/`runner.rs` are cosmetic choreography of already-decided outcomes and must never mutate `ScoreBoard`/`Bases`; the plugin list in `mod.rs` gains those four.

- [ ] **Step 2: Full sweep** — `cargo test && cargo clippy --all-targets && cargo check --target wasm32-unknown-unknown`, then `/run-web` for a browser smoke test of one full half-inning.

- [ ] **Step 3: Commit** — `git commit -m "docs: animation/choreography architecture in CLAUDE.md"`

---

## Self-review notes

- **Spec coverage:** windup ✓(T3), hit-stop/kick ✓(T4), burst/dust/roll ✓(T5), predictor+window ✓(T6/T7), fielder tasks ✓(T8), runners/trot/run-out ✓(T9/T10), Magnus/kinds ✓(T11/T12), Option-C seam ✓(T1 sampler), fielding-gameplay seam ✓(MoveIntent everywhere), docs ✓(T13).
- **Known intentional simplifications** (all inside the spec's "cosmetic" envelope): fouls aren't chased; a chaser whose ball resets out-of-bounds briefly runs toward the mound; departing runners recolor at the half-inning flip (reads as sides swapping); walk trots reuse the runner spawn rather than a bespoke jog.
- **Type consistency check:** `Playing::then(clip, next)` (T1) used in T2/T3; `MoveIntent { target, speed }` used in T8/T9; `predict_landing` signature change is explicit in T12 including test updates; `PitchEvent { velocity, spin }` introduced in T11 with flow compiling via a fastball default until T12.
