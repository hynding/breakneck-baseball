# Ball & Player Physics/Animation — Design

**Date:** 2026-07-17
**Status:** Approved scope, implementation pending
**Builds on:** `2026-07-16-visual-overhaul-theming-design.md`

## Goal

Make the game *move* like baseball: a pitcher who winds up, contact that
pops, fielders who chase the ball, runners who take their bases, and pitches
with real flight character. The rules engine stays untouched — outcomes are
still decided analytically at contact (`rules::classify_batted_ball`), and
everything here is presentation choreography that *performs* the known
outcome. Four phases, each independently shippable.

## Decisions (from the user)

| Question | Decision |
|----------|----------|
| Animation approach | **Option A now**: procedural transform animation on the existing primitive rigs — zero assets, wasm-size-free, matches the toy aesthetic. **Structured for Option C later**: all posing flows through one clip abstraction (`AnimClip` + a single sampler system), so swapping the sampler for Bevy's `AnimationGraph` is a driver change, not a rewrite. |
| Fielding | Cosmetic choreography now, but **player-controlled fielding is a future gameplay goal**. Fielder movement is therefore intent-driven from day one: locomotion consumes a `MoveIntent` that CPU choreography writes — exactly the `ai.rs` pattern, where the CPU writes the same `Intents` a human would. A future controllable fielder swaps the writer, not the mover. Honest constraint: fielders chase the *real* ball position, no teleporting. |
| Scope | All four layers in this one spec, built as ordered phases. |

## Architectural ground rules

1. **Rules stay sovereign.** No animation or physics change may alter the box
   score. The `Outcome` computed at contact is the script; choreography acts
   it out. Fielding never becomes physically load-bearing in this spec (the
   ball keeps ignoring player colliders via `BALL_GROUP`/`PLAYER_GROUP`).
2. **One animation pathway.** All rig posing goes through `animation.rs`
   (new module). Systems request clips; they never hand-rotate rig parts.
   `player.rs`'s existing `Swing` machine migrates into this module.
3. **Gameplay-ready seams.** Anything that will one day be player-controlled
   (fielder movement, pitch selection) reads intents, not hard-coded targets.
4. **Dual-target discipline.** Everything is meshes + transforms — no
   particle crates, no asset files. Effects reuse the `TrailGhost`
   spawn-and-fade pattern (already proven on wasm). Verify each phase with
   `cargo check` on both native and `wasm32-unknown-unknown`.

## New module: `animation.rs`

```rust
/// Every animation the game can play, by name — the Option C seam.
pub enum AnimClip {
    WindUp, ThrowRelease,          // pitcher
    RunCycle, ScoopBall, GloveUp,  // fielders & runners
    SwingBat, RecoverSwing,        // batter (migrated from player.rs Swing)
}

/// Component: what a rig is currently playing.
pub struct Playing {
    pub clip: AnimClip,
    pub timer: Timer,       // clip-defined duration
    pub next: Option<AnimClip>,  // simple chaining (WindUp → ThrowRelease)
}

/// Component on rigs that can move: written by choreography (or, later,
/// a human controller), consumed by the locomotion system.
pub struct MoveIntent {
    pub target: Option<Vec3>,   // world-space destination; None = hold
    pub speed: f32,             // m/s cap
}
```

Two systems own all motion:

- **`sample_clips`** — maps `(clip, timer.fraction())` to part transforms.
  Poses are pure functions (like today's `sweep_rotation`), keyed by the new
  `RigLimb` markers. This is the *only* code that touches rig-part
  transforms. Under Option C this system is replaced by `AnimationGraph`
  playback; callers never know.
- **`locomote`** — moves rigs toward `MoveIntent.target` (kinematic
  transform step, capped at `speed`), turns them to face travel, and plays
  `RunCycle` while moving / returns to idle at rest.

Rigs gain limbs in `spawn_rig`: two arm cylinders and two leg cylinders as
`RigPart`-tagged children (so team recoloring keeps working) with `RigLimb`
markers for the sampler. Ball-park sizes match the existing toy proportions.

## Phase 1 — Pitch rhythm & contact feedback

The anticipation → explosion loop that makes it feel like baseball.

- **`Phase::WindUp`** inserted between `PrePitch` and `Pitch`. When the
  fielding side presses action, flow stores the aim, plays `WindUp` on the
  pitcher (~0.5 s), and fires `PitchEvent` on completion (`ThrowRelease`
  chained). CPU code is untouched — it presses the same button.
  `trigger_swing`'s phase gate widens to `PrePitch | WindUp | Pitch`.
- **Hit-stop on contact:** on `HitEvent`, set `Time<Virtual>` relative speed
  to ~0.05 for ~0.06 s, then restore. One small resource + system.
- **Camera kick:** a 2–3 frame positional impulse on the broadcast camera at
  contact, decaying exponentially. Lives in `camera.rs`.
- **Contact burst:** ~8 small unlit spheres spawned at the bat/ball point
  with random-ish outward velocities (hash noise, `ai.rs` style), shrinking
  to nothing over ~0.3 s — the `TrailGhost` pattern with velocity.
- **Bounce dust:** consume the ball's `CollisionEvent`s (already enabled,
  currently unread). When the in-flight ball hits ground above a vertical
  speed threshold, spawn a dust puff at the contact point. Grass gets
  extra rolling drag so grounders die naturally instead of rolling to the
  fence (a stronger `linear_damping` while grounded, or a ground-contact
  drag term in `apply_drag`).

## Phase 2 — Fielder choreography

- **Landing prediction:** at `HitEvent`, numerically integrate the hit
  velocity with the real gravity + drag constants (small fixed-step loop,
  pure function in `rules.rs`, unit-tested) → landing point + hang time.
- **Play-derived `InPlay` duration:** replace fixed `INPLAY_SECS = 2.2` with
  `hang_time + choreography buffer`, clamped to sane bounds, so fly balls
  aren't cut off mid-air and grounders don't dawdle.
- **`FielderTask`** component (choreography state): `Hold`, `Chase`, `Camp`,
  `Scoop`, `Throw`, `Return`. At contact, the fielder nearest the landing
  point gets the job; the script matches the known `Outcome`:
  - *Fly/pop out* → run to the landing point (`MoveIntent`), `GloveUp` as
    the ball arrives; on glove contact the ball's velocity zeroes and
    `InFlight` clears — the catch *looks* real because the trajectory is.
  - *Ground out* → charge the real ball, `ScoopBall` on reach, then throw:
    the real ball gets a lobbed velocity toward the fielder nearest first
    base (`FieldSpec` has no named positions — nearest-to-base is the rule),
    who plays `GloveUp` on arrival.
  - *Hit* → chase the real ball down, scoop, lob back toward the mound.
- **`Return`:** during `Result`, every displaced fielder walks back to its
  `FieldSpec` spot; `result_phase` already resets the ball.
- Fielders are selected and driven entirely through `MoveIntent` — the
  future player-controlled-fielding feature replaces the `FielderTask`
  writer for one fielder with controller input and (then, not now) a rules
  path for player-resolved outcomes.

## Phase 3 — Base runners

`Bases` (rules) is the truth; runner rigs are its visualization.

- **`Runner { base: usize }`** rigs in batting-team colors, spawned via the
  existing `spawn_rig`. A `sync_runners` system diffs `Bases` after each
  resolved play and issues `MoveIntent`s along base paths (waypoints from
  `FieldSpec` base positions), speed tuned so arrivals land within the
  `Result` window. Runners scoring or stranded at inning end despawn (fade
  out) when `Bases` resets.
- **The batter runs:** on fair contact, the batter rig drops the bat pivot
  and becomes the new runner (sprints the first-base line — even on outs,
  like real baseball). A fresh batter rig walks in during `Result`.
- **Home run trot:** all runners + batter round every base on
  `Outcome::HomeRun`, allowed to overrun the `Result` timer (the trot is
  cosmetic; the next pitch doesn't wait for it — banner cadence rules).
- Peg-out variants need nothing special: runner rigs mirror `Bases`
  regardless of how outs happen.

## Phase 4 — Ball flight character

The only phase that touches gameplay balance, hence last.

- **Magnus force:** a system beside `apply_drag`:
  `F = MAGNUS_FACTOR · (ω × v)`, applied per tick. Backspin fastballs now
  carry; the existing hit backspin/side-spin gets flight consequences too.
  Tune `MAGNUS_FACTOR` so a max-spin pitch deflects ~0.2–0.3 m over the
  flight — readable, not silly.
- **Pitch types:** `PitchEvent` gains a `PitchKind { Fastball, Curveball,
  Changeup }` carrying speed + spin presets (defined in `rules.rs`, unit-
  tested like `pitch_velocity`). Selection is intent-driven: the fielding
  side's held aim direction at release maps to kind (up = fastball,
  down = curveball, neutral-slow = changeup) — no new input plumbing.
  CPU picks by skill-weighted noise. Batting difficulty emerges naturally
  from speed/break differences; the strike-zone call still uses the real
  plate crossing, so breaking balls genuinely steal corners.

## Testing

- Pure functions get unit tests in `rules.rs`: landing predictor (against
  closed-form no-drag cases + monotonicity with drag), pitch-kind presets,
  Magnus direction sanity (backspin ⇒ upward force for a −Z pitch).
- Choreography/animation is verified by eye per phase: native `cargo run`,
  then `/run-web` on wasm (per CLAUDE.md, both targets after physics
  changes).
- Watch-outs called out for implementation: hit-stop must not desync Rapier
  (verify physics respects `Time<Virtual>` scaling); new UI-adjacent
  effects are world-space meshes, so the wasm transparent-UI gotcha doesn't
  apply, but any new HUD element must follow `ui::hidden_tint`.

## Out of scope (explicitly)

- Player-controlled fielding gameplay (future feature; this spec only lays
  its seams — intent-driven locomotion, honest ball chasing).
- Option C (`AnimationGraph`) migration (future; enabled by the `AnimClip`
  sampler boundary).
- Skinned/glTF character models, particle-system crates, sound.
- Any change to outcome rules: catches, throws, and runners never decide
  the box score in this spec.

## Build order

1. **Phase 1** — `animation.rs` skeleton (clips, limbs, sampler), windup,
   hit-stop, camera kick, contact burst, bounce dust.
2. **Phase 2** — landing predictor, play-derived `InPlay`, `FielderTask`
   choreography, `MoveIntent` locomotion.
3. **Phase 3** — runner rigs synced to `Bases`, batter run-out, HR trot.
4. **Phase 4** — Magnus system, pitch kinds, CPU pitch selection.

Each phase ends green on both targets and playable end-to-end.
