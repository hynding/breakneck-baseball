# Game Variants & Front Yard Ball — Design

**Date:** 2026-07-16
**Status:** Approved scope, implementation pending
**Builds on:** `2026-07-16-playable-baseball-game-design.md` (the standard game)

## Goal

Restructure the game so that *the rules of the sport* and *the shape of the
field* are *data*, not code — then prove the flexibility twice:

1. **Standard baseball keeps playing exactly as it does today** (same counts,
   same outcome bands, same field, all existing tests green).
2. **"Front Yard Ball"** ships as a second selectable variant: a suburban front
   lawn with **four bases** spread across its corners, **4 fielders per team**
   spread over the street/sidewalk/neighbor yards, and outs recorded by
   **pegging** (hitting the runner with the ball) as well as catches and tags.

The deeper goal is that a *third* variant (rooftop ball, kickball rules,
5-out innings, whatever) should require only a new data definition plus
scenery — no changes to the rules engine or flow machine.

## Requirements & assumptions (locked)

| Topic | Decision |
|-------|----------|
| What varies | Base count & positions, count thresholds (balls/strikes/outs), innings, team size (fielder count & placement), field geometry (fair wedge, fence, scale), out mechanics (add peg-outs), scenery, camera framing. |
| What does NOT vary (yet) | Two teams; one batter at a time; arcade analytic outcome resolution at contact; pitch duel controls; the `MainMenu → Playing → GameOver` state machine. |
| "4+ players per team" | Refers to the on-field team size (pitcher + fielders spawned/simulated), not the number of humans. Humans remain 1–2, controlling pitch/swing as today. |
| Variant selection | On the main menu, before choosing 1P/2P: **F** (or d-pad left/right) cycles the field variant; the menu shows the current one. |
| Peg-outs | Deterministic, no RNG and no fielder simulation: derived from the predicted landing point's proximity to a fielder position (details below), consistent with the analytic arcade model. |
| Coordinate convention | Home plate stays at the world origin with +Z toward the field, for *every* variant. Pitch/swing/flow logic is plate-local and unchanged. |

## Approaches considered

- **A. Data-driven config structs (chosen).** A `Ruleset` (thresholds &
  mechanics) and a `FieldSpec` (geometry, players, scenery, camera) defined in
  plain Rust data; the pure functions in `rules.rs` take them as parameters.
  Bevy-friendly (plain resources), unit-testable, serde-ready for future file
  loading.
- **B. Trait-object rulesets.** `Box<dyn Variant>` overriding behavior per
  variant. Rejected: dynamic dispatch buys nothing — every requested variation
  is expressible as data; traits complicate resource storage and testing.
- **C. Asset-file variants (RON).** Max flexibility, but adds asset-loading
  machinery and error handling now for no current need. Deferred; approach A's
  structs are shaped so C can be layered on later without rework.

## Architecture

One new module, `game/variant.rs`, owns the variant data model and the two
built-in definitions. Everything else *consumes* it.

### New data model (`game/variant.rs`)

```rust
/// Countable-rule knobs. All thresholds the flow/rules engine reads.
pub struct Ruleset {
    pub balls_per_walk: u32,      // standard 4, front yard 4
    pub strikes_per_out: u32,     // standard 3, front yard 3
    pub outs_per_half: u32,       // standard 3, front yard 3
    pub innings: u32,             // standard 9, front yard 3 (quick games)
    pub peg_outs: bool,           // front yard: landing near a fielder = pegged
}

/// Field geometry + personnel. Home plate is implicitly at the origin.
pub struct FieldSpec {
    pub name: &'static str,            // menu / HUD label
    pub base_positions: Vec<Vec3>,     // running order; last base leads home
    pub pitch_distance: f32,           // rubber at (0, h, pitch_distance)
    pub fair_half_angle: f32,          // radians each side of +Z
    pub fence_line: f32,               // fence distance down the lines
    pub fence_center: f32,             // fence distance to straightaway center
    pub hit_scale: f32,                // scales the outcome distance bands
    pub peg_radius: f32,               // peg-out proximity (ignored if !peg_outs)
    pub fielder_positions: Vec<Vec3>,  // defensive team incl. anyone extra
    pub bounds: f32,                   // out-of-bounds ball-reset radius
    pub broadcast_eye: Vec3,           // camera framing per field size
    pub broadcast_target: Vec3,
    pub scenery: Scenery,              // which spawn routine dresses the set
}

pub enum Scenery { Stadium, FrontYard }

/// A complete variant and the two built-ins.
pub struct Variant { pub rules: Ruleset, pub field: FieldSpec }
impl Variant {
    pub fn standard() -> Self { /* today's exact values */ }
    pub fn front_yard() -> Self { /* see Front Yard section */ }
}
```

`Variant` is stored inside the existing `GameConfig` resource (menu writes it
before entering `Playing`); `Ruleset` and `FieldSpec` are also inserted as
standalone resources at game start so systems take only what they read.

### Generalized rules engine (`game/rules.rs`)

Stays pure and unit-tested; hardcoded baseball facts become parameters:

- `Bases` becomes `struct Bases { occupied: Vec<bool> }` (index 0 = first
  base), sized from `FieldSpec::base_positions.len()`. Helpers:
  `Bases::new(count)`, `clear()` (preserves size), `runner_count()`.
- `Outcome` generalizes `Single/Double/Triple` into `Hit(u32)` (bases earned,
  `1..=base_count`); `HomeRun`, `Foul`, `Out(OutKind)` remain. `OutKind` gains
  `Pegged`.
- `advance_hit(bases, hit_bases)` / `advance_walk(bases)` work over the vector
  (walk = force chain from first; hit = everyone up `hit_bases`; reaching
  index ≥ len scores).
- `call_ball`, `call_strike`, `record_out`, `is_game_over` take `&Ruleset`
  thresholds instead of literals 4/3/3.
- `classify_batted_ball(vel, &FieldSpec)` uses the spec's fair wedge, fence
  interpolation, and distance bands scaled by `hit_scale`. Band boundaries
  are today's constants × `hit_scale`; the hit tier ladder is derived from
  `base_count` (3 bases → 1/2/3-base hits exactly as today; 4 bases adds a
  4-base tier between the triple band and the fence).
- **Peg-out rule:** if `peg_outs` and the predicted landing point is within
  `peg_radius` of any `fielder_position` **and** the ball's flight time is
  short (line drives/grounders — launch < 20°), the would-be hit converts to
  `Out(Pegged)` ("PEGGED!"). Fly balls stay governed by the catch bands.
  Deterministic, testable, and creates the front-yard texture: hitting it
  straight at the kid on the sidewalk gets you beaned.
- `pitch_velocity(aim, pitch_distance)` and `mound_reset_pos(pitch_distance)`
  take the distance instead of importing the field constant.

### Consumers

| Module | Change |
|--------|--------|
| `game/mod.rs` | `GameConfig { mode, variant }`; insert `Ruleset`/`FieldSpec` resources on entering `Playing`; register `variant.rs`. `ScoreBoard` unchanged. |
| `game/field.rs` | `spawn_field` reads `FieldSpec`: ground, bases at `base_positions`, rubber at `pitch_distance`, fence/scenery via `Scenery` dispatch (`spawn_stadium` = today's code; `spawn_front_yard` = house, driveway, street, sidewalks, neighbor lawns, hedges). |
| `game/player.rs` | Fielders spawn from `fielder_positions` (any count). Pitcher at `pitch_distance`. Batter unchanged. |
| `game/flow.rs` | Passes `&Ruleset`/`&FieldSpec` into rules calls; banner text for `Hit(n)`: SINGLE/DOUBLE/TRIPLE for 1–3, "n BASES!" beyond; "PEGGED!" for `OutKind::Pegged`. Swing-window constants stay plate-local and shared. |
| `game/ball.rs` | Mound reset + out-of-bounds check use `FieldSpec` (`pitch_distance`, `bounds`) instead of constants. |
| `game/ai.rs` | Unchanged logic; it already works in plate-local space. |
| `game/camera.rs` | Broadcast eye/target come from `FieldSpec` (resource read; falls back to standard framing outside `Playing`). Orbit default distance scales with `fence_center`. |
| `game/ui.rs` | Base-pip HUD spawns `base_count` pips arranged around the diamond circle instead of a fixed 3. Scoreboard text unchanged. |
| `game/menu.rs` | Field selector line ("Field: Classic Stadium ◂ F ▸ Front Yard"); chosen `Variant` written into `GameConfig` at start. Game-over screen unchanged. |

### Front Yard Ball definition

A kid's-rules sandlot compressed onto a suburban lot. All distances in metres.

- **Rules:** 4 balls / 3 strikes / 3 outs, **3 innings**, `peg_outs: true`.
- **Bases (4):** lawn corners and the curb — e.g. first (8, 0, 6), second
  (10, 0, 14), third (−10, 0, 14), fourth (−8, 0, 6)… final values tuned in
  play-testing so the polygon reads as "corners of the lawn".
- **Pitching:** from the middle of the yard, `pitch_distance ≈ 10`. Pitch
  speed is unchanged (arcade feel > realism; the shorter flight tightens the
  reaction window slightly, which suits "breakneck").
- **Geometry:** `fair_half_angle ≈ 55°` (the street splays wider than a
  stadium wedge), `fence_line ≈ 38`, `fence_center ≈ 48` (over the street
  into the neighbor's picture window = home run), `hit_scale ≈ 0.4`,
  `peg_radius ≈ 4.5`, `bounds ≈ 90`.
- **Team (4):** pitcher mid-yard + 3 fielders: left sidewalk, right sidewalk,
  deep in the across-the-street neighbor's yard.
- **Scenery:** flat lawn-green ground; the batter hits *away* from the house
  toward the street: gray asphalt band with a yellow center line, lighter
  sidewalk strips both sides, house block + door + windows behind home plate,
  hedges along the lot lines, a couple of neighbor-house blocks across the
  street. Simple cuboid/cylinder primitives in the existing material style —
  no assets.
- **Camera:** `broadcast_eye ≈ (0, 7, −12)`, `broadcast_target ≈ (0, 1, 5)`.

### Error handling & edge cases

- `Bases` indexing is length-safe (`occupied.get(i)`); the HUD and rules never
  assume 3.
- A `Variant` with 0 bases or 0 fielders is prevented by construction (the
  built-ins are the only source today); `debug_assert!`s document invariants.
- Ball out-of-bounds reset radius comes from the spec, so small fields don't
  let the ball wander the void for seconds.
- Game restart / return-to-menu rebuilds everything from `GameConfig`, so
  switching variants between games needs no special teardown beyond the
  existing `GameplayEntity` cleanup.

## Testing / verification

1. **Unit tests (rules.rs)** — all existing tests updated to call with the
   standard `Ruleset`/`FieldSpec` and must keep passing *with identical
   expectations* (proves standard baseball is untouched). New tests:
   - 4-base advancement: bases-loaded walk with 4 bases does *not* score;
     5-base "home run" clears; `Hit(4)` from third-band distance on the small
     field.
   - Custom thresholds: e.g. `outs_per_half: 3` vs a 4-out ruleset flips the
     half-inning at the right count.
   - Peg-out: low liner landing within `peg_radius` of a front-yard fielder →
     `Out(Pegged)`; same ball with `peg_outs: false` → hit; high fly over the
     same spot → governed by catch bands.
   - Classification parity: a table of standard-field velocities asserting
     the same outcomes as before the refactor.
2. **Dual-target:** `cargo test`, `cargo fmt --check`, clippy `-D warnings`,
   `cargo check` native + wasm (the CI gate, run locally).
3. **Interactive:** `/run-web` — play a standard half-inning (unchanged feel),
   then a Front Yard game: verify menu selection, 4 pips on the HUD, peg-out
   banner occurs, scenery/camera read correctly, game ends after 3 innings.

## Build sequence (milestones)

1. **Variant data model** — `variant.rs` with `Ruleset`/`FieldSpec`/`Variant`
   + built-ins; wired into `GameConfig` and inserted as resources (standard
   only; game identical).
2. **Rules generalization (TDD)** — N-base `Bases`, `Hit(u32)`, threshold
   params, spec-driven classification, peg rule; all tests green.
3. **World from data** — field/players/ball/camera/ui consume the spec;
   standard game verified visually identical; dual-target check.
4. **Front Yard variant** — definition, scenery, menu selector, banners;
   play-test both variants in the browser; tune lawn geometry.
