# Batting Adapters (Plan C: Swing Meter + PCI Cursor) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The two remaining batting input styles from the batting-feel spec — Swing Meter (hold-release) and PCI Cursor — land as front-end adapters over the proven contact spine, routed per player by `Settings::batting_style`, with the CPU always on Classic semantics.

**Architecture:** A new `src/game/batting.rs` adapter layer turns raw `Intents` into per-team `SwingInput` commands each frame; `flow.rs`'s `pitch_live` consumes a `SwingInput` instead of the raw action edge, so Classic play and every existing test are byte-equivalent pass-throughs. PCI's quality/direction math is pure `rules.rs` functions (window shrink with cursor miss distance; hit direction from contact-point offset). All presentation (meter bar, stance deepen, cursor marker) consumes adapter state read-only.

**Tech Stack:** Bevy 0.15 ECS, Rapier 3D, existing contact spine (`rules::contact_quality`, `ContactEvent`, `Ruleset` windows), `settings.rs` persistence from Plan A.

## Global Constraints

- `rules.rs` stays pure and RNG-free; only `flow.rs` mutates `ScoreBoard`/`Bases`/`Play` outcome state; fx/audio/camera/ui never do.
- CPU batting always uses Classic semantics regardless of settings (spec §3) — `tests/balance_sim.rs::balance_bands_hold` and every CPU e2e must stay green untouched.
- All rig motion flows through `animation.rs` (`Playing` clips / `MoveIntent` / its own root-height systems); other modules never step rig transforms. Non-rig markers (the PCI cursor quad) may be moved directly, like `fx.rs`'s landing ring.
- wasm/WebGL2 UI rule: every UI element painted at spawn with nonzero alpha (`ui::hidden_tint`), container roots get a `BackgroundColor`, show/hide by mutating children; no UI roots spawned mid-`Playing`. Scene spawns key on `crate::game::game_start()`, never `OnEnter(Playing)`.
- Spec §3 exact behaviors: Meter — hold to load, release = swing instant, still holding past the FoulTip window = a swinging whiff (not a take). PCI — velocity-based cursor glide (keyboard playable), quality degrades with cursor-to-ball distance at the cross: dead-center = full windows, at cursor radius Perfect→0 and Solid halved, beyond radius best case FoulTip; hit direction derives from contact-point offset + Δt instead of raw aim.
- Tuned Ruleset values are frozen (perfect 40 / solid 90 / foul 130 ms; exits 0.65/0.95/1.28; `cpu_timing_spread_ms` 225): adapters must not retune them.
- Dual-target: after each task, `cargo check` and `cargo check --target wasm32-unknown-unknown`.
- Toolchain prefix for every command: `export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"`.

## File Structure

- **Create `src/game/batting.rs`** — the adapter layer: `SwingInput`, `SwingCommands` resource, per-style adapter systems, `MeterState`/`PciState`/`MeterLoad` resources, style routing. One responsibility: *raw intent → swing command*.
- **Modify `src/game/input.rs`** — `TeamIntent` gains `action_held`; `Controllers::player_index`.
- **Modify `src/game/flow.rs`** — `pitch_live` swings off `SwingCommands`; PCI branch grades via the new rules fns; registers the adapter system in its existing `.chain()` between `cpu_offense` and `pre_pitch`.
- **Modify `src/game/rules.rs`** — pure PCI math: `pci_contact_quality`, `pci_aim`; `Ruleset::pci_radius_m`.
- **Modify `src/game/variant.rs`** — `pci_radius_m` defaults on both variants.
- **Modify `src/game/animation.rs`** — meter stance-deepen: root sink for the `Batter` rig from `MeterLoad` (animation.rs owns rig root motion).
- **Modify `src/game/ui.rs`** — meter load bar (painted at spawn, fill mutated).
- **Modify `src/game/field.rs`** — PCI cursor marker on the zone plane (game-start spawn, hidden by default).
- **Modify `src/game/settings.rs`** — "(gamepad recommended)" hint on the PCI row label.
- **Tests:** unit tests in `batting.rs`/`rules.rs`/`input.rs`; new staged e2e `tests/e2e_batting_styles.rs`.

---

### Task C1: SwingInput plumbing — Classic pass-through and style routing

**Files:**
- Create: `src/game/batting.rs`
- Modify: `src/game/input.rs` (TeamIntent, gamepad_intent, keyboard_intent, Controllers)
- Modify: `src/game/flow.rs` (`pitch_live` swing gate; FlowPlugin chain)
- Modify: `src/game/mod.rs` (register `BattingPlugin` before `FlowPlugin`)
- Test: unit tests inside `batting.rs` + `input.rs`; whole existing suite as the pass-through proof

**Interfaces:**
- Consumes: `Intents`/`TeamIntent` (input.rs), `Controllers`/`InputSource` (input.rs), `Settings::batting_style: [BattingStyle; 2]` + `BattingStyle` (settings.rs), `Play.phase` + `Phase` (flow.rs), `ScoreBoard::batting_team()` / `Team`.
- Produces (later tasks rely on these exact names):
  - `pub struct SwingInput { pub aim: Vec2, pub pci_offset: Option<Vec2> }`
  - `#[derive(Resource, Default)] pub struct SwingCommands { home: Option<SwingInput>, away: Option<SwingInput> }` with `pub fn get(&self, team: Team) -> Option<&SwingInput>`, `pub fn take(&mut self, team: Team) -> Option<SwingInput>`, `pub fn set(&mut self, team: Team, cmd: SwingInput)`
  - `pub fn style_for(team: Team, controllers: &Controllers, settings: &Settings) -> BattingStyle`
  - `pub fn adapt_swings(...)` — the adapter system flow.rs chains in
  - `input.rs`: `TeamIntent.action_held: bool`; `Controllers::player_index(&self, team: Team) -> Option<usize>`

- [ ] **Step 1: `action_held` on `TeamIntent`**

In `src/game/input.rs`, add the field with a doc comment and wire both sources (the struct derives `Default`, so `Default` stays derived):

```rust
pub struct TeamIntent {
    /// Directional aim, components in −1.0..=1.0.
    pub aim: Vec2,
    /// Primary button was pressed this frame (pitch release / swing).
    pub action: bool,
    /// Primary button is currently held (the Swing Meter's load state —
    /// `action` is the edge, this is the level).
    pub action_held: bool,
}
```

In `gamepad_intent`: `action_held: pad.pressed(GamepadButton::South)`.
In `keyboard_intent`: `action_held: keyboard.pressed(action)`.

- [ ] **Step 2: `Controllers::player_index`**

```rust
impl Controllers {
    /// Which settings slot (P1 = 0, P2 = 1) drives this team, or `None` for
    /// the CPU. Home is always P1's team when human; Away is P2's (or the
    /// solo player's opponent).
    pub fn player_index(&self, team: Team) -> Option<usize> {
        match (team, self.source(team)) {
            (_, InputSource::Cpu) => None,
            (Team::Home, _) => Some(0),
            (Team::Away, _) => Some(1),
        }
    }
}
```

Unit test beside the existing `assign_controllers` tests:

```rust
#[test]
fn player_index_maps_p1_p2_and_cpu() {
    let one = assign_controllers(GameMode::OnePlayer, &[]);
    assert_eq!(one.player_index(Team::Home), Some(0));
    assert_eq!(one.player_index(Team::Away), None);
    let two = assign_controllers(GameMode::TwoPlayers, &[]);
    assert_eq!(two.player_index(Team::Away), Some(1));
}
```

- [ ] **Step 3: create `src/game/batting.rs` with types, routing, and the Classic arm**

```rust
//! Batting input adapters (spec §3): each style is a front end that turns raw
//! [`Intents`] into the same [`SwingInput`]; `flow::pitch_live` consumes the
//! command and never sees the style. The CPU always routes Classic.

/// One swing, decided this frame. The swing instant is implicit (the frame
/// the command exists); `pci_offset` is the PCI cursor's zone-plane position
/// at the press (world x / height y), `None` for Classic and Meter.
pub struct SwingInput {
    pub aim: Vec2,
    pub pci_offset: Option<Vec2>,
}

#[derive(Resource, Default)]
pub struct SwingCommands { /* home/away Option<SwingInput> + get/take/set */ }

pub fn style_for(team: Team, controllers: &Controllers, settings: &Settings) -> BattingStyle {
    match controllers.player_index(team) {
        None => BattingStyle::ClassicTiming, // CPU: always Classic (spec §3)
        Some(i) => settings.batting_style[i],
    }
}

/// The adapter: runs after `cpu_offense` (so CPU edges are visible) and
/// before `pre_pitch`/`pitch_live` (so a command lands the same frame).
pub fn adapt_swings(
    intents: Res<Intents>,
    controllers: Res<Controllers>,
    settings: Res<Settings>,
    score: Res<ScoreBoard>,
    play: Res<Play>,
    mut commands: ResMut<SwingCommands>,
) {
    let team = score.batting_team();
    let intent = intents.get(team);
    // Commands are single-frame: clear both slots first.
    *commands = SwingCommands::default();
    if play.phase != Phase::Pitch {
        return;
    }
    match style_for(team, &controllers, &settings) {
        BattingStyle::ClassicTiming => {
            if intent.action {
                commands.set(team, SwingInput { aim: intent.aim, pci_offset: None });
            }
        }
        // Meter and PCI arms land in C2/C4; until then they fall through to
        // Classic so the game stays playable in every style.
        BattingStyle::SwingMeter | BattingStyle::PciCursor => {
            if intent.action {
                commands.set(team, SwingInput { aim: intent.aim, pci_offset: None });
            }
        }
    }
}
```

`BattingPlugin` registers `SwingCommands` (`init_resource`) and nothing else; the system itself is chained by FlowPlugin (next step) so ordering is explicit.

- [ ] **Step 4: consume in `flow.rs` and chain the adapter**

In `FlowPlugin`'s existing `.chain()` tuple, insert `crate::game::batting::adapt_swings` between `cpu_offense` and `pre_pitch`. In `pitch_live`, replace the `intents`-based swing gate: change the params to take `mut swing_commands: ResMut<crate::game::batting::SwingCommands>` alongside the existing `intents` (still used for pickoffs/steals elsewhere in the fn — check each use), and replace

```rust
if intent.action {
```

with

```rust
if let Some(swing) = swing_commands.take(batter) {
```

using `swing.aim` where the arm used `intent.aim` for `rules::hit_velocity`. Ignore `swing.pci_offset` for now (C4 consumes it). Everything else in the arm is untouched.

- [ ] **Step 5: register `BattingPlugin` in `mod.rs`** (before `FlowPlugin` in the plugin list so the resource exists when flow's systems run).

- [ ] **Step 6: unit tests in `batting.rs`**

```rust
#[test]
fn cpu_always_routes_classic() {
    let controllers = assign_controllers(GameMode::OnePlayer, &[]);
    let mut settings = Settings::default();
    settings.batting_style = [BattingStyle::PciCursor, BattingStyle::SwingMeter];
    assert_eq!(style_for(Team::Away, &controllers, &settings), BattingStyle::ClassicTiming);
    assert_eq!(style_for(Team::Home, &controllers, &settings), BattingStyle::PciCursor);
}
```

- [ ] **Step 7: run the FULL existing suite** — this is the pass-through proof. `cargo test` (lib + all e2e; run `balance_bands_hold` once). Every test must pass unmodified: the e2e harness drives CPU-sourced teams, which route Classic and behave byte-identically.

- [ ] **Step 8: gates + commit**

`cargo clippy --all-targets -- -D warnings && cargo fmt --all && cargo check --target wasm32-unknown-unknown`

```bash
git add -A
git commit -m "feat: swing commands route through per-player batting styles"
```

---

### Task C2: the Swing Meter — hold to load, release to swing

**Files:**
- Modify: `src/game/batting.rs` (MeterState, MeterLoad, the meter arm of `adapt_swings`)
- Modify: `src/game/animation.rs` (stance-deepen root sink)
- Modify: `src/game/ui.rs` (meter bar)
- Test: unit tests in `batting.rs`; new `tests/e2e_batting_styles.rs`

**Interfaces:**
- Consumes: C1's `SwingCommands`/`SwingInput`/`style_for`/`adapt_swings`; `flow::{swing_dt_ms, late_swing_z}` (`pub(crate)`); `Baseball` + `Velocity` ball query; `Batter` marker + `RigBaseY` (animation/player).
- Produces:
  - `#[derive(Resource, Default)] pub struct MeterState { home: Option<f32>, away: Option<f32> }` — hold start in `Time::elapsed_secs`, `pub fn loading(&self, team: Team) -> bool`, `pub fn load_frac(&self, team: Team, now: f32) -> f32` (0..1 over `METER_FULL_SECS = 1.0`)
  - `#[derive(Resource, Default)] pub struct MeterLoad(pub f32)` — the *batting team's* current load fraction, for presentation (animation sink + UI bar)

- [ ] **Step 1: failing unit tests for the state machine** (pure logic — factor the per-frame decision into a pure fn so it tests without ECS):

```rust
/// What the meter arm does this frame, given (held, was_loading,
/// ball_passed_late_edge). Returns (now_loading, fire_swing).
pub(crate) fn meter_step(held: bool, was_loading: bool, ball_past: bool) -> (bool, bool) {
    match (held, was_loading, ball_past) {
        (true, false, false) => (true, false),  // press: start loading
        (true, true, false) => (true, false),   // keep loading
        (false, true, _) => (false, true),      // release: swing NOW
        (true, _, true) => (false, true),       // held too long: forced swing → whiff
        _ => (false, false),
    }
}

#[test]
fn meter_release_fires_the_swing() { assert_eq!(meter_step(false, true, false), (false, true)); }
#[test]
fn meter_holding_past_the_window_is_a_swinging_whiff() { assert_eq!(meter_step(true, true, true), (false, true)); }
#[test]
fn meter_press_starts_loading_without_swinging() { assert_eq!(meter_step(true, false, false), (true, false)); }
```

- [ ] **Step 2: the meter arm in `adapt_swings`.** Add `time: Res<Time>`, `mut meter: ResMut<MeterState>`, `mut load: ResMut<MeterLoad>`, `rules: Res<Ruleset>`, and the ball query `Query<(&Transform, &Velocity), With<Baseball>>`. In the `SwingMeter` arm:

```rust
BattingStyle::SwingMeter => {
    let ball_past = ball_q.get_single().is_ok_and(|(tf, vel)| {
        tf.translation.z < crate::game::flow::late_swing_z(vel.linvel.z, rules.foul_ms)
    });
    let was = meter.loading(team);
    let (now_loading, fire) = meter_step(intent.action_held, was, ball_past);
    if now_loading && !was {
        meter.start(team, time.elapsed_secs());
    }
    if !now_loading {
        meter.clear(team);
    }
    if fire {
        commands.set(team, SwingInput { aim: intent.aim, pci_offset: None });
    }
}
```

The forced swing on `ball_past` reaches `pitch_live` with the ball already beyond `late_swing_z`, so the existing reachability gate grades it a Whiff — a swinging strike, exactly the spec's "still holding past the FoulTip window = whiff", with zero new flow logic. Reset `MeterState`/`MeterLoad` when `play.phase != Phase::Pitch` (put the clear in the early-return path). Write `MeterLoad.0 = meter.load_frac(team, now)` every frame (0.0 when not loading).

- [ ] **Step 3: stance deepen in `animation.rs`.** New system (registered in AnimationPlugin's Update set, after the base-y settle/straighten system, `.chain()`ed with it):

```rust
/// The Swing Meter's visible load: the batter's stance deepens as the meter
/// fills — a bounded root sink composed over `RigBaseY`, owned here because
/// animation.rs owns rig root height.
const METER_SINK_M: f32 = 0.12;
fn meter_stance_sink(
    load: Res<crate::game::batting::MeterLoad>,
    mut batters: Query<(&mut Transform, &RigBaseY), With<crate::game::player::Batter>>,
) {
    for (mut tf, base) in &mut batters {
        tf.translation.y = base.0 - load.0 * METER_SINK_M;
    }
}
```

(Verify the existing settle system's name and chain after it so the sink wins the frame; if `RigBaseY`/`Batter` visibility needs a `pub(crate)`, widen it.)

- [ ] **Step 4: meter bar in `ui.rs`.** A slim vertical bar beside the count HUD corner: root container spawned at game start with `BackgroundColor` + `ui::hidden_tint` alpha rules, one child fill node whose `Node::height` is mutated to `Val::Percent(load * 100.0)` each frame from `Res<MeterLoad>`; fill uses the theme's accent color. Hidden (zero-height fill, dim shell) whenever load is 0 — never despawned.

- [ ] **Step 5: staged e2e `tests/e2e_batting_styles.rs`** using the shared harness (`tests/common/mod.rs`; inject via `DriveGame`-written `Intents`; remember `tap_key`-style menu injection and that the harness teams are CPU — for THIS test, set the batting team's `Controllers` slot to `InputSource::Keyboard(KeyScheme::Primary)` and write `Intents` directly from the `DriveGame` schedule, and set `Settings::batting_style[0] = BattingStyle::SwingMeter` right after `start_game`). Stages:
  1. **Routing proof (spec §7):** with style[0] = SwingMeter, a bare `action` edge (no hold) during the pitch produces NO swing (pitch reaches the mitt / counts a called pitch) — proves the settings row routed away from Classic.
  2. **Release swing:** hold `action_held` from delivery, release when the live `swing_dt_ms` (recompute in the test from the ball query, as `e2e_cpu_timing` does) is inside the solid window → assert a `ContactEvent` with quality better than FoulTip and a `HitEvent`.
  3. **Hold-through:** hold `action_held` and never release → assert a `ContactEvent` with `ContactQuality::Whiff` (the strike; a third such strike is the K).

- [ ] **Step 6: full gates + commit**

`cargo test` (all suites; balance once) && clippy `-D warnings` && fmt && wasm check.

```bash
git add -A
git commit -m "feat: swing meter — load the stance, release the swing"
```

---

### Task C3: PCI rules math (pure)

**Files:**
- Modify: `src/game/rules.rs` (two pure fns + tests)
- Modify: `src/game/variant.rs` (`pci_radius_m` on both variants)

**Interfaces:**
- Consumes: `ContactQuality`, `Ruleset` windows (perfect_ms/solid_ms/foul_ms), `ZONE_HALF_WIDTH`/`ZONE_LOW`/`ZONE_HIGH`.
- Produces:
  - `Ruleset::pci_radius_m: f32` (default **0.20** both variants — a hair over half the zone half-width, so the zone holds ~2 cursor radii)
  - `pub fn pci_contact_quality(dt_ms: f32, miss_m: f32, rules: &Ruleset) -> ContactQuality`
  - `pub fn pci_aim(offset: Vec2) -> Vec2` where `offset` = cursor − ball crossing, in meters

- [ ] **Step 1: failing tests pinning the spec's exact shrink** (spec §3: dead-center full windows; at radius Perfect→0, Solid halved; beyond, best case FoulTip). Weak gets its natural home: timing inside the FULL solid window but outside the SHRUNK one is clipped contact — `Weak`:

```rust
#[test]
fn pci_dead_center_keeps_full_windows() {
    let r = test_rules(); // perfect 40 / solid 90 / foul 130, pci_radius_m 0.20
    assert_eq!(pci_contact_quality(30.0, 0.0, &r), ContactQuality::Perfect);
    assert_eq!(pci_contact_quality(80.0, 0.0, &r), ContactQuality::Solid);
}
#[test]
fn pci_at_radius_perfect_vanishes_and_solid_halves() {
    let r = test_rules();
    assert_eq!(pci_contact_quality(10.0, 0.20, &r), ContactQuality::Solid); // no Perfect left
    assert_eq!(pci_contact_quality(80.0, 0.20, &r), ContactQuality::Weak);  // outside solid/2=45 → clipped
    assert_eq!(pci_contact_quality(40.0, 0.20, &r), ContactQuality::Solid); // inside 45
}
#[test]
fn pci_beyond_radius_caps_at_foul_tip() {
    let r = test_rules();
    assert_eq!(pci_contact_quality(10.0, 0.35, &r), ContactQuality::FoulTip);
    assert_eq!(pci_contact_quality(200.0, 0.35, &r), ContactQuality::Whiff); // timing still whiffs
}
#[test]
fn pci_aim_signs_loft_and_pull() {
    // Cursor UNDER the ball (offset.y negative) undercuts → lofts (aim.y +).
    assert!(pci_aim(Vec2::new(0.0, -0.1)).y > 0.0);
    // Cursor toward +x of the ball: same sense as raw aim.x (the −X pull
    // negation happens inside hit_velocity, exactly as for raw aim).
    assert!(pci_aim(Vec2::new(0.1, 0.0)).x > 0.0);
    // Saturates to the aim domain.
    assert!(pci_aim(Vec2::new(9.0, -9.0)).length() <= std::f32::consts::SQRT_2 + 1e-5);
}
```

- [ ] **Step 2: implement**

```rust
/// PCI contact grading (spec §3): the timing windows shrink linearly with the
/// cursor's miss distance. `frac = miss/radius`; effective perfect =
/// `perfect_ms·(1−frac)` (0 at the radius), effective solid =
/// `solid_ms·(1−frac/2)` (halved at the radius). Timing inside the FULL solid
/// window but outside the shrunk one is clipped contact → `Weak` (the only
/// source of Weak in the game). Beyond the radius the bat's sweet spot never
/// reaches the ball: best case FoulTip on timing alone.
pub fn pci_contact_quality(dt_ms: f32, miss_m: f32, rules: &Ruleset) -> ContactQuality {
    let dt = dt_ms.abs();
    if dt > rules.foul_ms {
        return ContactQuality::Whiff;
    }
    let frac = (miss_m / rules.pci_radius_m).max(0.0);
    if frac >= 1.0 {
        return ContactQuality::FoulTip;
    }
    let perfect_eff = rules.perfect_ms * (1.0 - frac);
    let solid_eff = rules.solid_ms * (1.0 - frac / 2.0);
    if dt <= perfect_eff {
        ContactQuality::Perfect
    } else if dt <= solid_eff {
        ContactQuality::Solid
    } else if dt <= rules.solid_ms {
        ContactQuality::Weak
    } else {
        ContactQuality::FoulTip
    }
}

/// PCI hit direction (spec §3): derived from the contact-point offset, not
/// raw aim. Normalized against the cursor-radius scale so a half-radius miss
/// is a half-strength aim; components clamp to the aim domain. Signs: cursor
/// under the ball lofts (+y); the x component keeps raw-aim's sense (the −X
/// pull negation lives in `hit_velocity`, per CLAUDE.md).
pub fn pci_aim(offset: Vec2) -> Vec2 {
    const PCI_AIM_SCALE_M: f32 = 0.20;
    Vec2::new(
        (offset.x / PCI_AIM_SCALE_M).clamp(-1.0, 1.0),
        (-offset.y / PCI_AIM_SCALE_M).clamp(-1.0, 1.0),
    )
}
```

`variant.rs`: add `pci_radius_m: 0.20,` to both `Ruleset` literals with a one-line comment citing spec §3.

- [ ] **Step 3: run the new tests + existing rules tests** (`cargo test --lib rules`), then full clippy/fmt/wasm gates.

- [ ] **Step 4: commit**

```bash
git add -A
git commit -m "feat: PCI contact math — windows shrink with the cursor miss"
```

---

### Task C4: PCI adapter, cursor marker, flow integration, and docs

**Files:**
- Modify: `src/game/batting.rs` (PciState, the PCI arm)
- Modify: `src/game/flow.rs` (`pitch_live` PCI branch)
- Modify: `src/game/field.rs` (cursor marker on the zone plane)
- Modify: `src/game/settings.rs` (PCI row hint)
- Modify: `CLAUDE.md` (styles are now real)
- Test: `batting.rs` unit tests; extend `tests/e2e_batting_styles.rs`

**Interfaces:**
- Consumes: C1's plumbing, C3's `pci_contact_quality`/`pci_aim`/`pci_radius_m`, `ZONE_*` consts, `Play.crossing` recording in `pitch_live`.
- Produces: `#[derive(Resource)] pub struct PciState { home: Vec2, away: Vec2 }` (cursor in zone coords: x meters world, y meters height; `Default` = zone center `(0, (ZONE_LOW+ZONE_HIGH)/2)`), `pub fn cursor(&self, team: Team) -> Vec2`; `PciCursorMarker` component in field.rs.

- [ ] **Step 1: cursor glide in the PCI arm** (velocity-based per spec — keyboard playable):

```rust
BattingStyle::PciCursor => {
    // Glide: aim is a velocity, not a position. Stick-right moves the
    // cursor toward screen-right; from the behind-home camera that is
    // world −X (first base side), matching the pitch-aim mapping's
    // negation (CLAUDE.md).
    const PCI_SPEED_MPS: f32 = 1.6;
    let c = pci.cursor_mut(team);
    c.x -= intent.aim.x * PCI_SPEED_MPS * time.delta_secs();
    c.y += intent.aim.y * PCI_SPEED_MPS * time.delta_secs();
    c.x = c.x.clamp(-rules::ZONE_HALF_WIDTH, rules::ZONE_HALF_WIDTH);
    c.y = c.y.clamp(rules::ZONE_LOW, rules::ZONE_HIGH);
    if intent.action {
        commands.set(team, SwingInput { aim: intent.aim, pci_offset: Some(*c) });
    }
}
```

Reset the cursor to zone center whenever `play.phase != Phase::Pitch` (same early-return path that clears the meter). NOTE: with PCI, `aim` steers the cursor, so the swing's direction comes from the offset — `pitch_live` must not feed raw aim to `hit_velocity` for PCI swings (next step). The Down-hold runner-send read (`wants_send`) also reads aim; spec accepts PCI's aim being cursor-bound during the pitch (document the collision in a comment: sending runners mid-flight and steering the cursor share the stick by design — the leadoff send decision (`steal_armed`) is made pre-delivery, so the real conflict window is small).

- [ ] **Step 2: PCI branch in `pitch_live`.** Where C1 left `swing_commands.take(batter)`:

```rust
if let Some(swing) = swing_commands.take(batter) {
    let reachable = /* unchanged spatial band */;
    let dt_ms = swing_dt_ms(pos.z, ball_vel.linvel.z);
    let quality = if !reachable {
        rules::ContactQuality::Whiff
    } else if let Some(cursor) = swing.pci_offset {
        let miss = cursor.distance(Vec2::new(pos.x, pos.y));
        rules::pci_contact_quality(dt_ms, miss, &rules)
    } else {
        rules::contact_quality(dt_ms, &rules)
    };
    // Direction: PCI derives it from the contact-point offset (spec §3).
    let aim = match swing.pci_offset {
        Some(cursor) => rules::pci_aim(cursor - Vec2::new(pos.x, pos.y)),
        None => swing.aim,
    };
    // ... existing arms unchanged, `rules::hit_velocity(pos.z, aim)` ...
```

- [ ] **Step 3: cursor marker in `field.rs`.** Alongside `spawn_strike_zone`: a small unlit quad (~0.06 m) at plate z on the zone plane, `PciCursorMarker` component, `Visibility::Hidden` at spawn. A `field.rs` system (chained near `strike_zone_visibility`) each frame: visible iff the batting team is human, its style is `PciCursor`, and the phase is `PrePitch | WindUp | Pitch`; position from `PciState::cursor(batting_team)` (this is a marker like `fx.rs`'s landing ring — direct transform is fine, it is not a rig).

- [ ] **Step 4: settings hint.** In `settings.rs`'s screen painting, the PCI row's value label becomes `"PCI Cursor (gamepad recommended)"` — implement in `BattingStyle::label()` only if that doesn't overflow the row layout; otherwise a fixed hint line under the rows painted at spawn. Keep `label()` used by tests consistent (update any asserting test).

- [ ] **Step 5: extend `tests/e2e_batting_styles.rs`** with a PCI stage (style[0] = PciCursor):
  1. Hold aim so the cursor glides off-center for ≥ the whole flight, press at a well-timed instant → assert `ContactEvent` quality is degraded (≤ Solid; with a far cursor, `FoulTip`/`Weak`) versus stage 2's meter/classic contact — proving cursor distance feeds grading.
  2. Neutral aim (cursor stays center), press timed inside the (shrunk-at-0 = full) perfect window → `Perfect` still reachable dead-center.
  (Recompute live `swing_dt_ms` from the ball query for press timing, as in C2's stage; keep margins fat per the repo's flake discipline — assert quality *bands*, not exact values, where physics jitter could flip a knife-edge.)

- [ ] **Step 6: CLAUDE.md.** Update the batting-feel paragraph: the three styles are now real adapters in `batting.rs` (`SwingCommands` seam in `pitch_live`; CPU always Classic; meter = hold/release with the stance sinking as it loads; PCI = velocity-glide cursor with `rules::pci_contact_quality` window shrink and offset-derived direction; per-player routing via `Settings::batting_style` + `Controllers::player_index`).

- [ ] **Step 7: full gates + commit** — `cargo test` (all suites, `balance_bands_hold` once — the CPU never routes PCI/Meter, so bands must hold untouched), clippy `-D warnings`, fmt, wasm check.

```bash
git add -A
git commit -m "feat: PCI cursor batting — put the barrel on the ball"
```

---

## Self-Review

**Spec coverage (§3, §7 of `2026-07-30-batting-feel-design.md`):** SwingInput unification → C1; per-player routing + CPU-always-Classic → C1; Classic timing-scored (already live from Plan B) → C1 pass-through; Meter hold/release/overhold-whiff + visible load → C2; PCI velocity glide, keyboard playable, window shrink (dead-center full / radius Perfect-0 Solid-halved / beyond FoulTip), offset-derived direction → C3+C4; "(gamepad recommended)" labeling → C4; §7 unit tests (meter edges, PCI scaling) → C2/C3; §7 e2e (meter release timing, settings toggling routes the other adapter) → C2 stage 1-3; settings row already shipped in Plan A. Weak's origin documented (C3) consistent with flow.rs's existing "`Weak` is the Plan-C PCI adapter's outcome" comment.

**Placeholders:** none — every step carries code or an exact edit. C1 Step 3 shows the full adapter body; C2/C4 arms are complete.

**Type consistency:** `SwingInput { aim: Vec2, pci_offset: Option<Vec2> }` used identically in C1 Step 3/4, C2 Step 2, C4 Steps 1-2. `pci_contact_quality(dt_ms: f32, miss_m: f32, &Ruleset)` matches C3 tests and C4 Step 2. `MeterLoad(pub f32)` consumed by both C2 Steps 3-4. `player_index` (C1 Step 2) consumed by `style_for` (C1 Step 3).

**Known judgment calls the implementer should NOT relitigate:** Weak = clipped-contact band (inside full solid, outside shrunk solid); forced meter whiff routed through the existing reachability gate rather than new flow logic; PCI stick collision with runner-send accepted and documented; `pci_radius_m` 0.20 / `METER_FULL_SECS` 1.0 / `PCI_SPEED_MPS` 1.6 / `METER_SINK_M` 0.12 are the starting tune (visual pass may adjust).
