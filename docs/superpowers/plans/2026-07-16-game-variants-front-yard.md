# Game Variants & Front Yard Ball Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make rules and field geometry data-driven (`Ruleset` + `FieldSpec`), keep standard baseball byte-identical in behavior, and ship a second playable variant: Front Yard Ball (4 bases on a suburban lawn, 4 fielders, peg-outs).

**Architecture:** New `game/variant.rs` module owns plain-data `Ruleset`/`FieldSpec`/`VariantId` (Bevy resources). The pure functions in `game/rules.rs` take them as parameters instead of hardcoding 3 bases / 4-3-3 counts / MLB geometry. `field.rs`, `player.rs`, `ball.rs`, `camera.rs`, `ui.rs`, `flow.rs`, `menu.rs` consume the resources. Home plate stays at the origin (+Z toward the field) for every variant.

**Tech Stack:** Rust, Bevy 0.15, bevy_rapier3d. Dual-target (native + wasm32-unknown-unknown).

## Global Constraints

- Toolchain PATH prefix required for every cargo command: `export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"`
- CI gate must stay green: `cargo fmt --check`, `cargo test`, `cargo clippy --all-targets -- -D warnings` (native and wasm), `cargo check --target wasm32-unknown-unknown`.
- Standard baseball behavior must be unchanged: existing rules tests keep their exact expectations (only call signatures adapt).
- No `rand` dependency; determinism everywhere (wasm-safe).
- Comment style: rustdoc on public items, comments state constraints not narration.

---

### Task 1: Variant data model (`variant.rs`) wired into config

**Files:**
- Create: `src/game/variant.rs`
- Modify: `src/game/mod.rs` (register module, `GameConfig`, insert resources)
- Modify: `src/game/flow.rs` (read innings from `Ruleset` instead of `GameConfig`)
- Modify: `src/game/menu.rs` (write chosen variant's resources at start; still standard-only)

**Interfaces:**
- Produces: `Ruleset { balls_per_walk, strikes_per_out, outs_per_half, innings, peg_outs }` (all `u32` except `peg_outs: bool`), `FieldSpec { name: &'static str, base_positions: Vec<Vec3>, pitch_distance: f32, fair_half_angle: f32, fence_line: f32, fence_center: f32, hit_scale: f32, peg_radius: f32, fielder_positions: Vec<Vec3>, bounds: f32, broadcast_eye: Vec3, broadcast_target: Vec3, scenery: Scenery }`, `enum Scenery { Stadium, FrontYard }`, `FieldSpec::base_count() -> usize`, `enum VariantId { Standard, FrontYard }` with `next()`, `label()`, `rules() -> Ruleset`, `field() -> FieldSpec`. `GameConfig` becomes `{ mode: GameMode, variant: VariantId }` (innings removed; `REGULATION_INNINGS` moves into `variant.rs`).
- Both `Ruleset` and `FieldSpec` are `#[derive(Resource, Clone, Debug)]` and app-init'd to the Standard values, so every system can take `Res<Ruleset>` / `Res<FieldSpec>` unconditionally.

- [ ] **Step 1: Write failing tests** (in `variant.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_matches_regulation_baseball() {
        let (r, f) = (VariantId::Standard.rules(), VariantId::Standard.field());
        assert_eq!((r.balls_per_walk, r.strikes_per_out, r.outs_per_half, r.innings), (4, 3, 3, 9));
        assert!(!r.peg_outs);
        assert_eq!(f.base_count(), 3);
        assert_eq!(f.pitch_distance, 18.44);
        assert_eq!(f.scenery, Scenery::Stadium);
        // Second base straight out along +Z at the diamond diagonal.
        assert!((f.base_positions[1] - Vec3::new(0.0, 0.0, 54.86)).length() < 0.01);
    }

    #[test]
    fn front_yard_is_four_bases_with_pegging() {
        let (r, f) = (VariantId::FrontYard.rules(), VariantId::FrontYard.field());
        assert!(r.peg_outs);
        assert_eq!(r.innings, 3);
        assert_eq!(f.base_count(), 4);
        assert_eq!(f.fielder_positions.len(), 4); // pitcher + 3 = 4-player team
        assert!(f.peg_radius > 0.0);
        assert_eq!(f.scenery, Scenery::FrontYard);
    }

    #[test]
    fn variant_cycle_visits_all_and_wraps() {
        assert_eq!(VariantId::Standard.next(), VariantId::FrontYard);
        assert_eq!(VariantId::FrontYard.next(), VariantId::Standard);
    }
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test variant` → FAIL (module missing).
- [ ] **Step 3: Implement `variant.rs`** — structs above plus:

```rust
impl VariantId {
    pub fn rules(self) -> Ruleset {
        match self {
            VariantId::Standard => Ruleset { balls_per_walk: 4, strikes_per_out: 3, outs_per_half: 3, innings: 9, peg_outs: false },
            VariantId::FrontYard => Ruleset { balls_per_walk: 4, strikes_per_out: 3, outs_per_half: 3, innings: 3, peg_outs: true },
        }
    }
    pub fn field(self) -> FieldSpec {
        match self {
            VariantId::Standard => FieldSpec {
                name: "Classic Stadium",
                base_positions: vec![
                    Vec3::new(BASE_DISTANCE, 0.0, BASE_DISTANCE),
                    Vec3::new(0.0, 0.0, BASE_DISTANCE * 2.0),
                    Vec3::new(-BASE_DISTANCE, 0.0, BASE_DISTANCE),
                ],
                pitch_distance: 18.44,
                fair_half_angle: std::f32::consts::FRAC_PI_4,
                fence_line: 100.0, fence_center: 122.0,
                hit_scale: 1.0, peg_radius: 0.0, bounds: 220.0,
                fielder_positions: vec![ /* today's 8 spots from player.rs verbatim */ ],
                broadcast_eye: Vec3::new(0.0, 13.0, -21.0),
                broadcast_target: Vec3::new(0.0, 1.2, 9.0),
                scenery: Scenery::Stadium,
            },
            VariantId::FrontYard => FieldSpec {
                name: "Front Yard",
                base_positions: vec![
                    Vec3::new(8.0, 0.0, 6.0), Vec3::new(10.0, 0.0, 14.0),
                    Vec3::new(-10.0, 0.0, 14.0), Vec3::new(-8.0, 0.0, 6.0),
                ],
                pitch_distance: 10.0,
                fair_half_angle: 55.0_f32.to_radians(),
                fence_line: 38.0, fence_center: 48.0,
                hit_scale: 0.4, peg_radius: 4.5, bounds: 90.0,
                fielder_positions: vec![
                    Vec3::new(0.0, 0.0, 10.0),   // pitcher, mid-yard
                    Vec3::new(12.0, 0.0, 20.0),  // right sidewalk
                    Vec3::new(-12.0, 0.0, 20.0), // left sidewalk
                    Vec3::new(0.0, 0.0, 34.0),   // across-the-street yard
                ],
                broadcast_eye: Vec3::new(0.0, 7.0, -12.0),
                broadcast_target: Vec3::new(0.0, 1.0, 5.0),
                scenery: Scenery::FrontYard,
            },
        }
    }
}
```

`BASE_DISTANCE` stays exported from `field.rs`. In `mod.rs`: `GameConfig { mode, variant: VariantId }`, `init_resource` unchanged plus `.insert_resource(VariantId::Standard.rules())` / `.insert_resource(VariantId::Standard.field())`. In `menu.rs::menu_select`, add `ResMut<Ruleset>` + `ResMut<FieldSpec>` and set both from `config.variant` (still always `Standard` this task). In `flow.rs::maybe_end_game` take `&Ruleset` and use `rules.innings`.

- [ ] **Step 4: Verify** — `cargo test` (all green), `cargo clippy --all-targets -- -D warnings`, `cargo fmt`.
- [ ] **Step 5: Commit** — `feat: variant data model (Ruleset + FieldSpec) with standard and front-yard definitions`

---

### Task 2: N-base `Bases` and threshold-parameterized counts (TDD)

**Files:**
- Modify: `src/game/rules.rs` (Bases, advance_*, call_*, record_out + tests)
- Modify: `src/game/flow.rs` (pass `&Ruleset`; resize bases on reset)
- Modify: `src/game/ui.rs` (HUD pips from `base_count`)

**Interfaces:**
- Produces: `Bases::new(count)`, `Bases::default()` = 3 bases, `is_occupied(i)`, `set(i, bool)`, `count()`, `clear()`, `reset_for(count)` (clear + resize); `advance_hit(&mut Bases, u32) -> u32`, `advance_walk(&mut Bases) -> u32` (unchanged names); `call_ball(&mut ScoreBoard, &mut Bases, &Ruleset) -> BallCall`, `call_strike(.., &Ruleset) -> StrikeCall`, `record_out(.., &Ruleset)`.
- Consumes: `Ruleset` from Task 1.

- [ ] **Step 1: Adapt existing tests + add failing generalization tests**

Existing tests build `Bases` via helpers — change helpers to `Bases::default()` / occupancy setters, keep every assertion's meaning identical. New tests:

```rust
#[test]
fn four_base_walk_chain_only_scores_when_all_full() {
    let mut b = Bases::new(4);
    for expected in [0, 0, 0, 0, 1] { assert_eq!(advance_walk(&mut b), expected); }
}

#[test]
fn four_base_hit_advancement() {
    let mut b = Bases::new(4);
    assert_eq!(advance_hit(&mut b, 4), 0);      // batter reaches 4th base
    assert!(b.is_occupied(3));
    assert_eq!(advance_hit(&mut b, 5), 2);      // 5-base homer scores both
    assert_eq!(b, Bases::new(4));
}

#[test]
fn custom_out_threshold_flips_half_inning() {
    let rules = Ruleset { outs_per_half: 4, ..VariantId::Standard.rules() };
    let mut score = ScoreBoard { inning: 1, top_of_inning: true, outs: 3, ..Default::default() };
    let mut bases = Bases::default();
    record_out(&mut score, &mut bases, &rules);
    assert_eq!((score.outs, score.top_of_inning), (0, false));
}
```

- [ ] **Step 2: Run** — `cargo test` → new tests FAIL to compile/pass.
- [ ] **Step 3: Implement** — `Bases { occupied: Vec<bool> }`; `advance_hit` iterates indices high→low moving each runner `hit_bases` forward (index ≥ len scores), batter lands at index `hit_bases-1`; `advance_walk` sets the first empty index else scores 1; `call_ball/call_strike/record_out` compare against `rules.*`. `flow.rs`: systems add `Res<Ruleset>`; `reset_flow` also takes `Res<FieldSpec>` and calls `bases.reset_for(field.base_count())`. `ui.rs`: `spawn_base_diamond(commands, base_count)` places pip *k* (1-based) of *n* at angle `-90° + k·360°/(n+1)` on a radius-34px circle in the 90px box; `BaseIndicator(usize)` 0-based; `update_base_diamond` uses `bases.is_occupied(i)`.
- [ ] **Step 4: Verify** — `cargo test`, clippy, fmt. All 13 original expectations intact.
- [ ] **Step 5: Commit** — `feat: N-base runner model and ruleset-driven counts`

---

### Task 3: Spec-driven classification, `Hit(u32)`, peg-outs (TDD)

**Files:**
- Modify: `src/game/rules.rs` (classify, Outcome, pitch kinematics params + tests)
- Modify: `src/game/flow.rs` (banners, home-run bases, call-site params)
- Modify: `src/game/ball.rs` (mound reset + bounds from spec)

**Interfaces:**
- Produces: `enum Outcome { Foul, Out(OutKind), Hit(u32), HomeRun }`; `OutKind::Pegged`; `classify_batted_ball(vel: Vec3, field: &FieldSpec, rules: &Ruleset) -> Outcome`; `pitch_velocity(aim: Vec2, pitch_distance: f32) -> Vec3`; `mound_reset_pos(pitch_distance: f32) -> Vec3`.
- Consumes: Task 1 resources.

- [ ] **Step 1: Failing tests**

Adapt classification tests to pass `(&standard_field, &standard_rules)` with identical expected outcomes (`Single`→`Hit(1)` etc.), plus:

```rust
#[test]
fn peg_out_low_liner_lands_near_fielder() {
    let (f, r) = (VariantId::FrontYard.field(), VariantId::FrontYard.rules());
    // Flat ~10° liner straight at the pitcher (0, 0, 10).
    let vel = vel_at(10.0, 20.0); // helper: launch deg, speed, straightaway
    assert_eq!(classify_batted_ball(vel, &f, &r), Outcome::Out(OutKind::Pegged));
}

#[test]
fn same_ball_without_peg_rule_is_a_hit() {
    let f = VariantId::FrontYard.field();
    let r = Ruleset { peg_outs: false, ..VariantId::FrontYard.rules() };
    assert!(matches!(classify_batted_ball(vel_at(10.0, 20.0), &f, &r), Outcome::Hit(_)));
}

#[test]
fn high_fly_over_fielder_is_not_pegged() {
    let (f, r) = (VariantId::FrontYard.field(), VariantId::FrontYard.rules());
    let out = classify_batted_ball(vel_at(45.0, 16.0), &f, &r);
    assert!(!matches!(out, Outcome::Out(OutKind::Pegged)));
}

#[test]
fn four_base_field_can_yield_hit_four() {
    let (f, r) = (VariantId::FrontYard.field(), VariantId::FrontYard.rules());
    // Low 15° screamer into the 4-base band (~37 m landing), away from fielders.
    // (pick x-spray so no fielder is within peg_radius)
    assert_eq!(classify_batted_ball(vel_gap(15.0, 33.0), &f, &r), Outcome::Hit(4));
}
```

- [ ] **Step 2: Run** — FAIL.
- [ ] **Step 3: Implement.** In `classify_batted_ball` with `s = field.hit_scale`:
  - fair: `land.z > 1.0 && land.x.abs() <= land.z * field.fair_half_angle.tan() + 0.01` (45° ≡ today).
  - fence: `cos_half = field.fair_half_angle.cos(); centered = ((land.z / dist) - cos_half) / (1.0 - cos_half); fence = field.fence_line + (field.fence_center - field.fence_line) * centered.clamp(0.0, 1.0)`.
  - peg (after fence check): `if rules.peg_outs && launch_deg < 20.0 && field.fielder_positions.iter().any(|p| Vec2::new(land.x - p.x, land.z - p.z).length() < field.peg_radius) { return Outcome::Out(OutKind::Pegged); }`
  - bands: pop `launch > 50° && dist < 55·s`; fly `launch > 20° && dist < 95·s`; ground `dist < 26·s`; hit ladder: start `hit = 1`, boundary `44·s`, while `hit < base_count && dist >= boundary { hit += 1; boundary += 24·s; }` → `Hit(hit)`. (Standard: 44/68 boundaries ≡ today.)
  - `flow.rs::resolve_contact`: `Hit(1..=3)` → SINGLE/DOUBLE/TRIPLE, `Hit(n)` → `"{n} BASES!"`; `HomeRun` → `apply_hit` with `base_count as u32 + 1`; `Out(Pegged)` → `"PEGGED!"` banner (orange-red).
  - `ball.rs`: `spawn_ball`/`reset_ball_if_out_of_bounds` take `Res<FieldSpec>`; reset pos `(0, BALL_RADIUS + 0.25, field.pitch_distance)`; out when `pos.y < -10 || Vec2::new(pos.x, pos.z).length() > field.bounds`.
- [ ] **Step 4: Verify** — `cargo test`, clippy both targets, fmt.
- [ ] **Step 5: Commit** — `feat: field-spec classification with peg-outs and N-base hits`

---

### Task 4: World spawns from data (field, players, camera)

**Files:**
- Modify: `src/game/field.rs` (spec-driven spawn + scenery dispatch)
- Modify: `src/game/player.rs` (fielders from spec)
- Modify: `src/game/camera.rs` (broadcast framing from spec)

**Interfaces:**
- Consumes: `Res<FieldSpec>` everywhere.
- Produces: `Fielder { index: usize }` (replaces `FieldPosition` enum); scenery split into `spawn_stadium(...)` (today's ground/infield/mound/poles/wall verbatim) and `spawn_front_yard(...)` (stub returning lawn ground only this task).

Steps: move stadium spawn code behind `match field.scenery`; bases loop over `field.base_positions` (home plate still spawned at origin); mound/rubber at `field.pitch_distance`; pitcher spawn at `field.pitch_distance`; fielders loop over `field.fielder_positions`; `broadcast_camera` + `BroadcastTarget` reset use `field.broadcast_eye/broadcast_target` (system gains `Res<FieldSpec>`). Verify: `cargo run` — standard game looks and plays exactly as before; `cargo check --target wasm32-unknown-unknown`. Commit: `refactor: spawn field, players, and camera framing from FieldSpec`.

---

### Task 5: Front Yard scenery + menu selector (playable variant)

**Files:**
- Modify: `src/game/field.rs` (`spawn_front_yard` full scenery)
- Modify: `src/game/menu.rs` (variant selector line + start wiring)

**Interfaces:**
- Consumes: `VariantId::next()`, `label()`, resources from Task 1.

Scenery (all cuboid primitives, existing material style): lawn-green ground plane; gray asphalt street band `z ∈ [22, 30]` full width with yellow center stripe; light-gray sidewalk strips `z ∈ [20, 22]` and `z ∈ [30, 32]`; house block (~10×6×5) with door/window cuboids at `z ≈ -8` behind home; two neighbor house blocks across the street at `z ≈ 38`; hedge rows along `x = ±16` for `z ∈ [0, 20]`; base "plates" reuse the standard base mesh. Menu: new line `Field: {label}   (F to change)` + `ControllerStatus`-style live update; `KeyF` / d-pad left-right cycles `config.variant = config.variant.next()`; on start, `menu_select` writes `config.variant.rules()` / `.field()` into the resources (wired in Task 1, now actually varying). Verify: `cargo run`, start a Front Yard 1P game — 4 HUD pips, peg banner reachable, 3-inning game-over. Commit: `feat: Front Yard Ball playable variant with suburban scenery and menu selector`.

---

### Task 6: Full verification gate + web play-test

Run the complete local CI gate (fmt, test, clippy native+wasm, check wasm), then `/run-web` build and play both variants in the browser (standard: one clean half-inning; front yard: menu select, peg-out seen, 4-base HUD, game ends inning 3). Fix anything found, final commit.

---

## Self-review notes

- Spec coverage: data model (T1), rules generalization + thresholds (T2), classification/peg/Hit(u32) (T3), world-from-data incl. camera/ball bounds (T3/T4), front-yard scenery + menu (T5), verification matrix (T6). UI pips covered in T2. Banner text in T3.
- Type consistency: `Bases::is_occupied(usize)`, `classify_batted_ball(Vec3, &FieldSpec, &Ruleset)`, `VariantId::rules()/field()` used consistently across tasks.
- No placeholders: scenery geometry given with concrete coordinates; test helpers (`vel_at`, `vel_gap`) are defined in the test module when written (launch-angle/speed → Vec3, matching existing test style).
