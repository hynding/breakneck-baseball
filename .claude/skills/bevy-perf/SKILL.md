---
name: bevy-perf
description: Use when the game is "slow", "stuttering", "dropping frames", "laggy on web", or when asked to profile or optimize runtime performance. Bevy-0.15-specific practice - ECS scheduling, change detection, asset caching, Rapier stepping, diagnostics setup, and the wasm/WebGL2 constraints. For generic Rust performance, defer to the rust-skills rules named at the bottom.
---

# Bevy 0.15 Performance

Profile before optimizing (`rust-skills/rules/perf-profile-first.md`). In Bevy that means:
turn on diagnostics, find *which system* is slow, then apply the matching pattern below.

## Measure first

- Debug builds here already add `FrameTimeDiagnosticsPlugin` (F1 panel shows FPS —
  `src/game/meta/debug.rs`). For per-system numbers add
  `bevy::diagnostic::{SystemInformationDiagnosticsPlugin, LogDiagnosticsPlugin}` temporarily:
  `LogDiagnosticsPlugin::default()` prints every diagnostic on a 1 s cadence.
- Native release ≠ wasm: WebGL2 is the slow path (no compute, fewer threads, driver overhead).
  Measure on the wasm build for web-facing complaints (`/run-web`).
- The debug Time tab and `juice.rs` both scale `Time<Virtual>` — a "slow" game may just be a
  stuck `relative_speed`; check it before profiling (must compose with `juice::BaseSpeed`).

## ECS scheduling

- Systems run in parallel when their queries are disjoint. A `ResMut<T>` or `&mut` query
  serializes against every other user of `T` — take the narrowest access that works
  (`Res<` over `ResMut<`, `&` over `&mut`, `Option<Res<>>` only when truly optional).
- Explicit `.chain()`/`.before()/.after()` ordering also serializes; order only where a real
  data dependency exists (this repo's `PhaseSet` and the `adapt_swings` chain are deliberate).
- Note for this repo: wasm Bevy runs effectively single-threaded anyway — scheduling wins
  matter natively, allocation/query wins matter everywhere.

## Change detection & queries

- Put `Changed<T>`/`Added<T>` filters on mirror/sync systems (recolour, jersey re-letter,
  HUD updates) so they cost nothing on quiet frames. Change detection is per-`DerefMut` —
  writing an identical value still flags it; use `set_if_neq` where that matters.
- Don't call `query.get(entity)` inside a per-entity inner loop (O(n·m) archetype lookups) —
  restructure to iterate once, or collect the lookup side into a `HashMap` first
  (`rust-skills/rules/perf-collect-once.md`).
- Use `Local<Vec<_>>`/`Local<HashMap<_>>` for per-system scratch buffers and `.clear()` them,
  instead of allocating per frame (`rust-skills/rules/mem-reuse-collections.md`,
  `mem-with-capacity.md`).
- `par_iter` helps big homogeneous loops natively; useless on wasm (single thread).

## Assets & materials

- Every `materials.add(...)` / `images.add(...)` per frame is an allocation *and* a GPU
  upload. Cache handles in a resource and reuse (`jersey.rs` caches per player; `fx`
  particles should share materials/meshes, not mint new ones per burst).
- `Handle<T>` clones are cheap (ref-counted id); cloning the *asset* is not.
- Runtime-generated textures (`field/textures.rs`, jerseys): generate once at spawn, never
  in `Update`.

## Time & Rapier

- `Time<Virtual>` is game time (pausable, scalable — juice/slow-mo lives here);
  `Time<Fixed>` accumulates virtual time into fixed ticks. Rapier 0.28's default
  `TimestepMode::Variable` steps once per frame from the time delta — physics cost scales
  with collider count every frame, and steps regardless of `GameState` (why pausing is
  refused while the ball flies).
- Keep collider counts flat: fielders are kinematic capsules, the wall is fixed — verify FX
  never spawn colliders and that transient entities despawn (leaks show as a slowly growing
  step time; count via the debug State tab).

## Wasm / WebGL2 constraints

- Single-threaded: no compute shaders, no async task pools worth leaning on; heavy per-frame
  CPU work (procedural texture regen, big allocations) hits harder than natively.
- WebGL2 limits: no storage buffers (Bevy falls back to uniform batching — fewer lights and
  smaller batches), guaranteed `MAX_TEXTURE_SIZE` can be as low as 2048–4096 — keep runtime
  textures (jerseys, field) comfortably under it, and MSAA/shadow-map defaults (4x / 2048)
  are worth revisiting explicitly for WebGL2.
- Binary size is a perf metric on web (download = startup time): `--profile wasm-release`
  already sets `opt-level = "z"` + LTO (`rust-skills/rules/perf-release-profile.md`).

## Generic Rust perf — don't restate, read these

`rust-skills/rules/`: `perf-profile-first.md`, `perf-iter-over-index.md`,
`perf-collect-once.md`, `perf-entry-api.md`, `mem-reuse-collections.md`,
`mem-with-capacity.md`, `mem-avoid-format.md`, `anti-format-hot-path.md`,
`anti-collect-intermediate.md`, `anti-clone-excessive.md`.
