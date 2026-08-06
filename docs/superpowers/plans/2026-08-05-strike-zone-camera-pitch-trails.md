# Strike Zone Realism, Batter Framing, Caught-Pitch Camera Hold, Pitch Trails — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** MLB-correct strike zone rendered as a darker 3D wireframe; the duel camera frames the batter's full body at 80–90% of screen height; the camera holds the tight at-bat framing after any pitch the catcher gloves; and the pitch gets an adjustable-color fading trail with 5 additional interchangeable 3D trail styles.

**Architecture:** Zone realism starts in `rules.rs` (drawn zone = called zone is an invariant), which ripples into the CPU pitcher's aim mapping and the balance sim. The overlay in `field.rs` becomes a 12-edge 3D wireframe box the depth of home plate. Camera changes are data (per-variant `FieldSpec` framing) plus one new `Play` flag (`pitch_gloved`) read by `broadcast_camera`. Trails live in `fx.rs` (cosmetic only, never touch score/bases) with style + color chosen in `settings.rs` (persisted, serde-defaulted for back-compat).

**Tech Stack:** Rust, Bevy 0.15 ECS, Rapier 3D. Procedural meshes/materials only — no asset files.

## Global Constraints

- Rust PATH prefix required: `export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"`
- Both targets must compile: `cargo check` and `cargo check --target wasm32-unknown-unknown`
- `tests/balance_sim.rs` is the arbiter of the offensive economy (K% 15–30, runs/9 3.0–8.0, HR/9 0.5–2.5)
- No RNG in `rules.rs` (deterministic hash noise only, in `ai.rs`/`fx.rs`)
- `fx.rs` never mutates `ScoreBoard`/`Bases` — cosmetic only
- wasm UI rule: overlay entities spawned at game start, shown/hidden by mutation, never respawned mid-`Playing`
- Real-world facts go in `docs/BASEBALL.md` with sources, cited in code comments
- Roster/labels A–Z only where jersey-rendered (settings labels are UI text, unaffected)
- Commit after each task; run `cargo fmt` + `cargo clippy` before each commit

---

### Task 1: MLB strike zone in the rules (and the aim map that feeds it)

The drawn zone must follow MLB rules, and the codebase invariant is drawn = called. Official Baseball Rules: the strike zone is the area **over home plate** (17 in wide) between the **midpoint of shoulders-to-pants-top** and the **hollow beneath the kneecap**, in the batter's stance; a strike is a pitch **any part of which** passes through **any part** of the zone (so the called width gets a ball-radius allowance each side).

Rig derivation (1.85 m rig, `tools/build_player.py`): knee hollow ≈ 0.50 m (current `ZONE_LOW` already correct); shoulders ≈ 1.52 m, pants top ≈ 1.10 m → midpoint ≈ 1.31, slight stance crouch → **1.30**. Called half-width = plate 0.216 + ball 0.037 = **0.253**.

**Files:**
- Modify: `docs/BASEBALL.md` (new "Strike zone" section with sources)
- Modify: `src/game/rules.rs:22-32` (consts), `:293-307` (`pitch_velocity_kind` recenter), zone unit tests
- Modify: `src/game/ai.rs:297` (CPU batter's fuzzy chase zone tracks the new zone)

**Interfaces:**
- Produces: `rules::ZONE_HALF_WIDTH = 0.253`, `rules::ZONE_LOW = 0.5`, `rules::ZONE_HIGH = 1.30`, plus new `rules::PLATE_HALF_WIDTH_M = 0.216` and `rules::BALL_RADIUS_M = 0.037` (public, so `field.rs` renders the plate-width box and documents the allowance). Task 2 consumes these.

- [ ] **Step 1: Write failing tests in `rules.rs`**

```rust
/// The called zone follows the MLB rulebook (docs/BASEBALL.md, "Strike
/// zone"): plate width plus the any-part-of-the-ball allowance each side,
/// knee hollow to the stance midpoint for the 1.85 m rig.
#[test]
fn zone_is_plate_width_plus_ball_allowance() {
    assert!((ZONE_HALF_WIDTH - (PLATE_HALF_WIDTH_M + BALL_RADIUS_M)).abs() < 1e-6);
    assert!((ZONE_LOW - 0.5).abs() < 1e-6);
    assert!((ZONE_HIGH - 1.30).abs() < 1e-6);
}

/// Neutral aim throws to the middle of the *current* zone — the aim map may
/// never drift off the zone the umpire calls.
#[test]
fn neutral_aim_targets_zone_middle() {
    let v = pitch_velocity_kind(PitchKind::Changeup, Vec2::ZERO, 18.44);
    let flight = 18.44 / PitchKind::Changeup.speed();
    let start = mound_reset_pos(18.44);
    let y_at_plate = start.y + v.y * flight - 0.5 * GRAVITY * flight * flight;
    assert!((y_at_plate - (ZONE_LOW + ZONE_HIGH) / 2.0).abs() < 0.02);
    assert!(v.x.abs() < 0.05);
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test --lib rules` → FAIL (consts don't exist / wrong values)

- [ ] **Step 3: Implement**

In `rules.rs`, next to the existing consts (~line 29):

```rust
/// Home plate half-width (17 in / 2 — docs/BASEBALL.md).
pub const PLATE_HALF_WIDTH_M: f32 = 0.216;
/// Official ball radius (shared with ball.rs's physics constant).
pub const BALL_RADIUS_M: f32 = 0.037;
/// Called zone half-width: the plate plus the rulebook's "any part of the
/// ball" allowance (Official Baseball Rules, definition of STRIKE (b) —
/// docs/BASEBALL.md "Strike zone"). The *drawn* zone is the plate width;
/// a ball grazing the drawn frame's edge is still a strike by exactly its
/// own radius, as in real life.
pub const ZONE_HALF_WIDTH: f32 = PLATE_HALF_WIDTH_M + BALL_RADIUS_M;
/// Zone floor: the hollow beneath the kneecap of the 1.85 m rig (docs/BASEBALL.md).
pub const ZONE_LOW: f32 = 0.5;
/// Zone ceiling: the midpoint between shoulder top and pants top in the
/// rig's slight stance crouch (docs/BASEBALL.md) — not the old arcade 1.45.
pub const ZONE_HIGH: f32 = 1.30;
```

In `pitch_velocity_kind` (~line 297), recenter the aim map on the new zone so "zero = middle of the zone" stays true and the CPU's in-zone pitch rate (hence K%/BB%) survives the shrink:

```rust
    let target_x = -aim.x * 0.6;
    let target_y = (ZONE_LOW + ZONE_HIGH) / 2.0 + aim.y * 0.45;
```

(0.45 spans the zone edge-to-just-outside, replacing the 0.5 that was tuned to the 0.95-centred zone; full-up aim still paints above the letters, full-down still bounces curves in the dirt.)

In `ai.rs:297`, the CPU batter's *misjudged* zone keeps its ±0.15-ish fuzz around the real zone:

```rust
        let in_zone = cross.x.abs() < 0.35 && (0.4..=1.45).contains(&cross.y);
```

- [ ] **Step 4: Add the BASEBALL.md section** (before "Dirt, grass, and mowing"):

```markdown
## Strike zone (Official Baseball Rules definitions; mlb.com rules glossary)

- The strike zone is "that area over home plate the upper limit of which is a
  horizontal line at the midpoint between the top of the shoulders and the top
  of the uniform pants, and the lower level is a line at the hollow beneath
  the kneecap", judged "from the batter's stance as the batter is prepared to
  swing" (OBR, Definitions of Terms). Width is home plate's 17 in.
- A STRIKE (b) is "a pitch ... any part of which passes through any part of
  the strike zone" — a ball whose *edge* nicks the zone counts, so a
  centre-of-ball zone test widens by one ball radius (~1.45 in) each side.
- Broadcast K-zones draw the plate-width rectangle; the edge allowance is
  invisible ("painting the corner").

In the game: `rules::ZONE_*` — plate half-width 0.216 m + ball radius
0.037 m = called half-width 0.253 m; heights 0.50–1.30 m derived from the
1.85 m rig's knee hollow and stance midpoint (`tools/build_player.py`
proportions). `field::spawn_strike_zone` draws the plate-width wireframe;
the calls honour the edge allowance past the frame.
```

- [ ] **Step 5: Fix the existing zone unit tests** in `rules.rs` (any that assert crossings in/out of the 0.34/1.45 zone — adjust the sample points to the new bounds, keeping each test's *intent*).

- [ ] **Step 6: Run** `cargo test --lib` → PASS. Then run the e2e suite `cargo test --test e2e_full_game --test e2e_advanced_rules --test e2e_cpu` — scripted pitches aim mid-zone via the same map, but verify HBP/dropped-third scripts still land their outcomes; adjust scripted aims only if a script regressed.

- [ ] **Step 7: Commit** — `feat: MLB-rulebook strike zone (plate width + ball allowance, stance heights)`

---

### Task 2: Darker 3D wireframe zone overlay

Replace the flat 2-bar frame + bright fill with a 12-edge wireframe box, home-plate deep, in a dark near-black tint. The drawn box is **plate width** (0.432 m), not the called width — the ball-radius allowance is exactly the invisible "painting the corner" margin (documented in Task 1). Keep the `ZoneFlash` pulse (all edges share the frame material) and the PCI cursor, which moves to the box's near face.

**Files:**
- Modify: `src/game/field.rs:914-995` (`spawn_strike_zone`), plus a new geometry test

**Interfaces:**
- Consumes: `rules::PLATE_HALF_WIDTH_M`, `rules::ZONE_LOW/HIGH` (Task 1)
- Produces: unchanged markers `StrikeZoneOverlay`, `PciCursorMarker`; `ZoneFlash` behavior preserved (Task 1–6 systems untouched)

- [ ] **Step 1: Write the failing test** (in `field.rs` tests)

```rust
/// The zone overlay is a 3D wireframe the size of the rulebook zone: plate
/// width, plate depth, knee-to-midpoint tall (docs/BASEBALL.md "Strike
/// zone") — 12 edges + 1 near-face fill + nothing bright: the frame tint
/// must be darker than the old white (all channels < 0.5).
#[test]
fn zone_wireframe_matches_rulebook_dimensions() {
    assert!((ZONE_DRAWN_HALF_WIDTH - rules::PLATE_HALF_WIDTH_M).abs() < 1e-6);
    assert!((ZONE_DEPTH - PLATE_WIDTH).abs() < 1e-6);
    let c = ZONE_FRAME_COLOR.to_srgba();
    assert!(c.red < 0.5 && c.green < 0.5 && c.blue < 0.5);
    assert!(c.alpha > 0.0, "wasm rule: never alpha 0");
}
```

- [ ] **Step 2: Run to verify failure** — consts don't exist.

- [ ] **Step 3: Implement.** In `field.rs`:

```rust
/// The drawn zone is the plate-width rulebook zone; calls extend one ball
/// radius past the frame (see rules::ZONE_HALF_WIDTH's doc).
const ZONE_DRAWN_HALF_WIDTH: f32 = rules::PLATE_HALF_WIDTH_M;
/// The zone volume is as deep as home plate (17 in front edge to point,
/// docs/BASEBALL.md) — the rulebook zone is a prism *over the plate*.
const ZONE_DEPTH: f32 = PLATE_WIDTH;
/// Darker wireframe per the design ask: near-black steel, readable against
/// grass and sky without washing out the PCI cursor or the ball.
const ZONE_FRAME_COLOR: Color = Color::srgba(0.10, 0.11, 0.14, 0.85);
const ZONE_FILL_COLOR: Color = Color::srgba(0.05, 0.06, 0.08, 0.10);
```

Rewrite the spawn body: keep `translucent`, `ZoneFlash` (with `frame_base_color = ZONE_FRAME_COLOR`), and the `part` closure. Then:

```rust
    let hw = ZONE_DRAWN_HALF_WIDTH;
    let hd = ZONE_DEPTH / 2.0;
    let bar = 0.02;
    // Subtle fill on the near (catcher-side) face only, for PCI contrast.
    part(
        Vec3::new(hw * 2.0, height, 0.004),
        Vec3::new(0.0, mid_y, -hd),
        &fill,
    );
    // 12 edges of the zone prism: 4 horizontals front & back...
    for z in [-hd, hd] {
        for y in [rules::ZONE_LOW, rules::ZONE_HIGH] {
            part(Vec3::new(hw * 2.0 + bar, bar, bar), Vec3::new(0.0, y, z), &frame);
        }
        // ...4 verticals...
        for x in [-hw, hw] {
            part(Vec3::new(bar, height + bar, bar), Vec3::new(x, mid_y, z), &frame);
        }
    }
    // ...and 4 depth rails connecting the faces.
    for x in [-hw, hw] {
        for y in [rules::ZONE_LOW, rules::ZONE_HIGH] {
            part(Vec3::new(bar, bar, ZONE_DEPTH + bar), Vec3::new(x, y, 0.0), &frame);
        }
    }
```

PCI cursor translation z becomes `-hd - 0.02` (just off the near face, toward the behind-home camera). Update `spawn_strike_zone`'s doc comment: drawn = plate-width zone; calls honour the ball-radius allowance.

- [ ] **Step 4: Run** `cargo test --lib field` → PASS.

- [ ] **Step 5: Visual check on native** — `cargo run --features dev`, start a game, confirm: dark wireframe box with depth over the plate, readable in catcher POV, flash pulse still fires on solid contact, PCI cursor (settings → PCI style) rides the near face.

- [ ] **Step 6: Commit** — `feat: strike zone as a dark plate-deep 3D wireframe`

---

### Task 3: Duel camera frames the batter's full body at 80–90% of screen height

Pure projection helper + per-variant framing data. Pulling the eye back past the catcher's front surface means the catcher must be hidden in CatcherPov (we're inside his silhouette) — extend `hide_occluders` with a CatcherPov arm that also covers the post-pitch hold phases (Task 4 shares the predicate).

**Files:**
- Modify: `src/game/camera.rs` (new pure fns `framed_ndc_y`, `framed_height_fraction`, `duel_framing_wanted`; `hide_occluders` CatcherPov arm; tests)
- Modify: `src/game/variant.rs` (`duel_eye`/`duel_target` retune, both variants)
- Modify: `src/game/player.rs` (name the batter's box x as `pub const BATTER_STAND_X: f32 = 0.7;`, use it in `spawn_players`)

**Interfaces:**
- Consumes: `DuelView::framing`, `FieldSpec::duel_eye/duel_target`, `DUEL_FOV`, `aspect_safe_duel_vfov`
- Produces: `camera::framed_height_fraction(eye, target, vfov, bottom, top) -> f32` and `camera::framed_ndc_y(eye, target, vfov, p) -> f32` (pub for tests); `duel_framing_wanted(&Play, now: f32) -> bool` (private, shared with Task 4); `player::BATTER_STAND_X`

- [ ] **Step 1: Write the failing test**

```rust
/// The signed vertical NDC (−1 bottom, +1 top) of a world point through a
/// look-at camera with vertical FOV `vfov`.
// (implementation goes in camera.rs; test below drives it)

/// The catcher-POV duel framing must show the batter's entire body —
/// spikes to helmet, RIG_HEIGHT on BATTER_STAND_X's side of the plate —
/// filling 80–90% of the screen height at the 16:9 reference aspect, fully
/// inside the frame, in both parks.
#[test]
fn catcher_pov_frames_the_full_batter_at_80_to_90_percent() {
    const RIG_HEIGHT: f32 = 1.85; // tools/build_player.py
    for id in [VariantId::Standard, VariantId::FrontYard] {
        let f = id.field();
        let (eye, target, vfov) =
            DuelView::CatcherPov.framing(&f, DUEL_REFERENCE_ASPECT);
        let feet = Vec3::new(crate::game::player::BATTER_STAND_X, 0.0, 0.0);
        let head = feet + Vec3::Y * RIG_HEIGHT;
        let frac = framed_height_fraction(eye, target, vfov, feet, head);
        assert!(
            (0.80..=0.90).contains(&frac),
            "{id:?}: batter fills {frac} of screen height"
        );
        for p in [feet, head] {
            let y = framed_ndc_y(eye, target, vfov, p);
            assert!(y.abs() <= 0.98, "{id:?}: {p} clipped at ndc y {y}");
        }
    }
}
```

- [ ] **Step 2: Run to verify failure** — helper missing; then (after Step 3's helper) the current framings fail the band (batter's feet clip today).

- [ ] **Step 3: Implement the helpers** in `camera.rs`:

```rust
/// Signed vertical NDC coordinate (−1 = bottom edge, +1 = top edge) of
/// world point `p` as seen by a look-at camera at `eye` toward `target`
/// with vertical FOV `vfov`. Pure — the framing tests use it to prove the
/// duel shot really contains the batter.
pub fn framed_ndc_y(eye: Vec3, target: Vec3, vfov: f32, p: Vec3) -> f32 {
    let fwd = (target - eye).normalize();
    let right = fwd.cross(Vec3::Y).normalize();
    let up = right.cross(fwd);
    let v = p - eye;
    let depth = v.dot(fwd).max(f32::EPSILON);
    (v.dot(up) / depth) / (vfov / 2.0).tan()
}

/// Fraction of the viewport height the segment `bottom`→`top` spans.
pub fn framed_height_fraction(eye: Vec3, target: Vec3, vfov: f32, bottom: Vec3, top: Vec3) -> f32 {
    ((framed_ndc_y(eye, target, vfov, top) - framed_ndc_y(eye, target, vfov, bottom)) / 2.0).abs()
}
```

- [ ] **Step 4: Retune the framings.** Iterate `duel_eye`/`duel_target` per variant until the test passes (run `cargo test --lib camera` each iteration). Starting geometry (from the angle math): Standard `duel_eye ≈ (0.0, 1.35, -1.25)`, `duel_target ≈ (0.0, 0.30, 4.0)` (a downward tilt ≈ 14°); FrontYard similar with its shorter geometry, eye ≈ `(0.0, 1.3, -1.7)`, target ≈ `(0.0, 0.3, 3.5)`. Constraints to preserve: `duel_eye.y ∈ (0.9, 1.6)` and `duel_eye.z < 0` (variant tests), umpire clearance (FrontYard umpire front surface at z ≈ −1.8 — stay in front of it or rely on Step 5's hiding). Update the `duel_eye` doc comments — the eye is no longer "just past the catcher's head" but a knee-high-plate shot *inside* the catcher's silhouette, which is why Step 5 hides him.

- [ ] **Step 5: Hide the catcher in CatcherPov.** In `hide_occluders`, the CatcherPov eye now sits inside/behind the catcher's front surface, and (Task 4) the camera can *hold* that framing through `InPlay`'s plate hold and a gloved `Result`. Add the shared predicate and use it:

```rust
/// The phases during which the broadcast rig wants (or is still holding)
/// the tight duel framing: the duel itself, the post-contact plate hold,
/// and the result pause of a pitch the catcher gloved (Task 4).
fn duel_framing_wanted(play: &Play, now: f32) -> bool {
    match play.phase {
        Phase::PrePitch | Phase::WindUp | Phase::Pitch => true,
        Phase::InPlay => play.since_contact(now) < BALL_FOLLOW_DELAY,
        Phase::Result => play.pitch_gloved() && !play.is_home_run(),
    }
}
```

(Until Task 4 lands `pitch_gloved`, write the `Result` arm as `false` and switch it in Task 4.) In `hide_occluders`, compute `let pov_hold = *mode == CameraMode::Broadcast && *view == DuelView::CatcherPov && duel_framing_wanted(&play, time.elapsed_secs());` (add `time: Res<Time>` param) and hide a `CatcherRole` subject when `pov_hold`, in addition to the existing `occludes` check for the other views. The plate umpire keeps the plain occlusion path (he sits behind the eye in both variants).

- [ ] **Step 6: Run** `cargo test --lib` (camera + variant tests) → PASS. Then `cargo run --features dev`: batter fills the frame, no catcher body parts brushing the lens, V-cycling still works, post-contact hold then chase still glides.

- [ ] **Step 7: Commit** — `feat: catcher-POV duel view frames the batter's full body at 80-90% height`

---

### Task 4: Camera zooms out after a pitch only when the catcher didn't glove it

New `Play::pitch_gloved` flag set exactly where the mitt receives (both the presentational and official-freeze paths of `catcher_receives`), cleared on the PrePitch reset; `broadcast_camera`'s `Result` arm holds the active duel view instead of the wide framing while it's set. Wild pitches (dirt/high), dropped third strikes, HBP, and anything hit never set it → those still zoom out. Home runs keep the trot orbit (checked first).

**Files:**
- Modify: `src/game/flow.rs` (`Play` field + getter, `catcher_receives` sets, `result_phase` reset clears)
- Modify: `src/game/camera.rs` (`broadcast_camera` Result arm + finish `duel_framing_wanted`)

**Interfaces:**
- Consumes: `catcher_receives`'s existing catch decisions; `duel_framing_wanted` (Task 3)
- Produces: `pub fn Play::pitch_gloved(&self) -> bool`

- [ ] **Step 1: Write the failing test** (flow tests, alongside `Play`'s unit tests — construct `Play` directly):

```rust
/// A gloved pitch is remembered through the result pause (the camera holds
/// the duel framing on it) and forgotten at the next at-bat's reset.
#[test]
fn pitch_gloved_survives_result_and_clears_on_reset() {
    let mut play = Play::default();
    assert!(!play.pitch_gloved());
    play.set_pitch_gloved_for_test();
    assert!(play.pitch_gloved());
}
```

(If `Play`'s fields are directly writable inside the module's test — they are, tests live in `flow.rs` — set the field directly instead of a test helper.) Add the camera-side test in `camera.rs`:

```rust
/// The result pause holds the duel framing for a pitch the catcher gloved
/// (called strikes/balls end tight on the plate); everything the mitt
/// missed — hits, dirt balls, dropped thirds — releases the camera to the
/// wide shot. Home runs orbit instead (checked first in broadcast_camera).
#[test]
fn result_framing_holds_only_for_gloved_pitches() {
    let mut play = Play::default();
    play.phase = Phase::Result; // via a flow test constructor…
    // gloved → wants duel framing
    // not gloved → wants wide framing
}
```

`Play`'s fields are private to `flow.rs`; give `flow.rs` a `#[cfg(test)] pub fn test_play(phase: Phase, gloved: bool) -> Play` constructor so the camera test can build states without opening the fields, and assert through `duel_framing_wanted` (make it `pub(crate)`).

- [ ] **Step 2: Run to verify failure.**

- [ ] **Step 3: Implement.** `flow.rs`:
  - `Play` gains `pitch_gloved: bool` (+ `Default` arm, doc: "the catcher received this at-bat's last pitch in the mitt — the camera holds the tight framing through the result pause; cleared at reset").
  - Getter: `pub fn pitch_gloved(&self) -> bool { self.pitch_gloved }`
  - In `catcher_receives`: set `play.pitch_gloved = true;` beside **both** `caught.send(PitchCaughtEvent)` sites *and* in the official-freeze branch even when `presentational_catch` already popped (the flag is idempotent; the dirt-ball early return must stay before it).
  - In the `result_phase` reset block (`play.phase = Phase::PrePitch; …`): `play.pitch_gloved = false;`
  - `#[cfg(test)] pub fn test_play(...)` constructor as above.
- `camera.rs`:
  - Finish `duel_framing_wanted`'s `Result` arm: `play.pitch_gloved() && !play.is_home_run()`.
  - In `broadcast_camera`, change the plain-Result arm to:

```rust
        // Result pause of a gloved pitch (called strike/ball, strikeout into
        // the mitt): stay in the at-bat view — the umpire's call doesn't
        // deserve a zoom-out. Everything the mitt missed falls through to
        // the wide framing below.
        (Phase::Result, _) if play.pitch_gloved() => view.framing(&field, aspect),
        // Result pause: settle on the wide home framing.
        (Phase::Result, _) => (field.broadcast_eye, field.broadcast_target, BROADCAST_FOV),
```

  (The home-run orbit arm stays above both.)

- [ ] **Step 4: Run** `cargo test --lib` → PASS; run the full e2e suite `cargo test` (the e2e games exercise takes/strikeouts/hits through the real phase machine — camera systems are data-driven and shouldn't affect them, but prove it).

- [ ] **Step 5: Native check** — take a few pitches (camera stays tight), spike a curveball in the dirt (camera releases wide), hit one (normal chase).

- [ ] **Step 6: Commit** — `feat: camera holds the at-bat framing after any pitch the catcher gloves`

---

### Task 5: Pitch-trail settings — style + color rows

Two new persisted fields with serde defaults (old stores must load), two new settings rows, row count goes 3 → 5.

**Files:**
- Modify: `src/game/settings.rs`

**Interfaces:**
- Produces: `pub enum PitchTrailStyle { Comet, Fireball, Frostbite, NeonRings, Stardust, Bubbles }` with `label/next/prev`; `pub enum TrailColor { Ember, Gold, Venom, Ice, Royal, Rose, Frost }` with `label/next/prev` and `pub fn color(self) -> Color`; `Settings { pitch_trail: PitchTrailStyle, trail_color: TrailColor, .. }` (serde-defaulted). Task 6 consumes both.

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn trail_style_and_color_cycle_and_wrap() {
    let mut s = PitchTrailStyle::Comet;
    for _ in 0..6 { s = s.next(); }
    assert_eq!(s, PitchTrailStyle::Comet);
    assert_eq!(PitchTrailStyle::Comet.prev(), PitchTrailStyle::Bubbles);
    let mut c = TrailColor::Ember;
    for _ in 0..7 { c = c.next(); }
    assert_eq!(c, TrailColor::Ember);
}

/// A pre-trail settings store (no trail fields) must still load — the new
/// fields are serde-defaulted, not a breaking schema change.
#[test]
fn legacy_store_without_trail_fields_loads_with_defaults() {
    let legacy = r#"{"batting_style":["ClassicTiming","ClassicTiming"],"volume":0.5}"#;
    let s: Settings = serde_json::from_str(legacy).unwrap();
    assert_eq!(s.pitch_trail, PitchTrailStyle::Comet);
    assert_eq!(s.trail_color, TrailColor::Ember);
    assert!((s.volume - 0.5).abs() < 1e-6);
}
```

- [ ] **Step 2: Run to verify failure.**

- [ ] **Step 3: Implement.** Enums with `Serialize, Deserialize, Default` (`#[default]` on `Comet`/`Ember`); labels: `"Comet (fading path)"`, `"Fireball"`, `"Frostbite"`, `"Neon rings"`, `"Stardust"`, `"Bubble stream"`; colors: Ember `srgb(1.0,0.45,0.15)`, Gold `srgb(1.0,0.85,0.30)`, Venom `srgb(0.45,1.0,0.35)`, Ice `srgb(0.40,0.85,1.0)`, Royal `srgb(0.65,0.45,1.0)`, Rose `srgb(1.0,0.45,0.75)`, Frost `srgb(0.95,0.97,1.0)`. `Settings` fields get `#[serde(default)]`. Extend `ROW_LABELS` to 5 (`"PITCH TRAIL"`, `"TRAIL COLOR"` between P2 style and VOLUME or after — put VOLUME last: `["P1 BATTING STYLE","P2 BATTING STYLE","PITCH TRAIL","TRAIL COLOR","VOLUME"]`), replace the hardcoded `2`/`% 3` cursor math with `ROW_LABELS.len() - 1` / `% ROW_LABELS.len()`, add `edit_settings` match arms (2 → style cycle, 3 → color cycle, 4 → volume), and `paint_settings_screen` value arms.

- [ ] **Step 4: Fix the existing cursor test** (`edit_settings_cycles_style_and_clamps_volume` walks Down twice to reach volume — now needs four Downs; update it and keep its intent).

- [ ] **Step 5: Run** `cargo test --lib settings` → PASS.

- [ ] **Step 6: Commit** — `feat: pitch trail style + color settings (persisted, back-compat)`

---

### Task 6: The fading pitch path and the five 3D trail styles

Cosmetic trail in `fx.rs`: while a pitch is in flight (`Phase::Pitch`, ball `InFlight`), drop a style-specific mote every `spacing` metres of ball travel. Motes age out over a lifetime with per-style animation (existing hash-noise, no RNG). "Fading" is real alpha fade: pre-build a small ladder of alpha-stepped materials per chosen color and step motes down it by age — no per-mote material allocation.

**Files:**
- Modify: `src/game/fx.rs` (assets, spawner, ticker, style behaviors, plugin wiring)
- Test: pure helpers in `fx.rs` tests + one e2e assertion (see Step 6)

**Interfaces:**
- Consumes: `Settings::{pitch_trail, trail_color}` (Task 5), `Play::phase`, `Baseball` + `InFlight`
- Produces: `pub struct TrailMote` (pub so e2e can query), `TrailAssets` resource (private), pure fns `fade_step(age_frac: f32, steps: usize) -> usize`, `should_drop(last: Option<Vec3>, pos: Vec3, spacing: f32) -> bool`, `PitchTrailStyle::{spacing, lifetime}` (extension consts in fx.rs, not settings.rs — presentation numbers stay in the presentation module)

- [ ] **Step 1: Failing pure tests** (in `fx.rs`):

```rust
#[test]
fn fade_step_walks_the_ladder_monotonically() {
    assert_eq!(fade_step(0.0, 6), 0);
    assert_eq!(fade_step(0.999, 6), 5);
    let mut prev = 0;
    for i in 0..=20 {
        let s = fade_step(i as f32 / 20.0, 6);
        assert!(s >= prev && s < 6);
        prev = s;
    }
}

#[test]
fn trail_drops_by_distance_not_frame_rate() {
    assert!(should_drop(None, Vec3::ZERO, 0.5), "first mote drops immediately");
    let last = Some(Vec3::ZERO);
    assert!(!should_drop(last, Vec3::new(0.0, 0.0, -0.3), 0.5));
    assert!(should_drop(last, Vec3::new(0.0, 0.0, -0.6), 0.5));
}
```

- [ ] **Step 2: Run to verify failure.**

- [ ] **Step 3: Implement the data layer.**

```rust
/// One dropped element of the pitch trail. Ages out over `lifetime`;
/// `seed` feeds the per-style hash-noise animation. Pub so the e2e can
/// count the trail without reaching into fx internals.
#[derive(Component)]
pub struct TrailMote {
    style: PitchTrailStyle,
    timer: Timer,
    seed: f32,
    /// Unit direction of ball travel at drop time (NeonRings orient to it).
    dir: Vec3,
}

/// Style-tuned drop spacing (metres of ball travel per mote) and lifetime.
fn trail_spacing(style: PitchTrailStyle) -> f32 { /* Comet 0.45, NeonRings 2.2, others 0.8 */ }
fn trail_lifetime(style: PitchTrailStyle) -> f32 { /* Comet 0.45, Bubbles 0.9, NeonRings 0.7, others 0.6 */ }

const TRAIL_FADE_STEPS: usize = 6;

/// Meshes + the alpha ladder for the chosen color, rebuilt each game start
/// (settings can only change on the menu).
#[derive(Resource)]
struct TrailAssets {
    style: PitchTrailStyle,
    meshes: /* per-style Handle<Mesh> fields or a small array */,
    /// materials[k] = chosen color at alpha (1 - k/steps) — the fade ladder.
    fade: Vec<Handle<StandardMaterial>>,
}

fn fade_step(age_frac: f32, steps: usize) -> usize {
    ((age_frac * steps as f32) as usize).min(steps - 1)
}

fn should_drop(last: Option<Vec3>, pos: Vec3, spacing: f32) -> bool {
    last.map_or(true, |l| l.distance(pos) >= spacing)
}
```

Meshes per style (all Bevy primitives): Comet `Sphere(0.05)`; Fireball `Cone { radius: 0.07, height: 0.16 }`; Frostbite `Tetrahedron::default()` scaled ~0.09; NeonRings `Torus { minor_radius: 0.015, major_radius: 0.16 }`; Stardust `Sphere(0.04)` (twinkle does the work); Bubbles `Sphere(0.06)`. Materials unlit, `AlphaMode::Blend`, base = `settings.trail_color.color()` (Fireball warms it toward orange-white at step 0, Frostbite cools it — small per-style tint fn). Build in `build_fx_assets` (already `game_start()`); add `settings: Res<Settings>` param.

- [ ] **Step 4: Spawner + ticker systems.**

```rust
/// Drops trail motes behind the pitched ball — distance-spaced so density
/// is frame-rate independent. Pitch phase only: the trail is the pitch's
/// signature, not the batted ball's.
fn pitch_trail(
    play: Res<Play>,
    assets: Option<Res<TrailAssets>>,
    ball_q: Query<(&Transform, &Velocity), (With<Baseball>, With<InFlight>)>,
    mut last_drop: Local<Option<Vec3>>,
    mut commands: Commands,
) { /* phase != Pitch → *last_drop = None; return. else drop per should_drop,
       spawning TrailMote + Mesh3d(style mesh) + fade[0] material at ball pos,
       seed = ball.z * 7.7, dir = vel.linvel.normalize_or_zero() */ }

/// Ages, animates, fades, and expires trail motes — per-style motion, all
/// deterministic hash noise on the mote's seed.
fn tick_trail(
    time: Res<Time>,
    assets: Option<Res<TrailAssets>>,
    mut motes: Query<(Entity, &mut TrailMote, &mut Transform, &mut MeshMaterial3d<StandardMaterial>)>,
    mut commands: Commands,
) { /* age = timer fraction; swap material to fade[fade_step(age, TRAIL_FADE_STEPS)];
       per-style: Comet shrink to 0.3; Fireball drift +Y 0.8 m/s, scale flicker
       1.0 + 0.3*noise(seed + t*30); Frostbite fall −0.4 m/s, spin
       rotate_local_y/x by dt * (2.0 + hash01(seed)); NeonRings orient
       Quat looking along mote.dir, grow scale 1.0→1.8; Stardust twinkle
       scale (0.6 + 0.4*sin-ish noise), slight drift; Bubbles rise 0.5 m/s,
       grow 1.0→1.5, die at 0.85 lifetime (the pop). finished → despawn */ }
```

Wire both into `FxPlugin`'s `Update` tuple (`.run_if(in_state(GameState::Playing))`). NeonRings ring orientation: `Transform::from_translation(pos).looking_to(dir, Vec3::Y)` — torus lies in XZ, so add a 90° X rotation so the ball threads it.

- [ ] **Step 5: Run** `cargo test --lib fx` → PASS; `cargo check` clean.

- [ ] **Step 6: e2e assertion.** In `tests/e2e_full_game.rs`, during a scripted pitch (after the pitch is thrown, before the plate), assert trail motes exist:

```rust
let motes = app.world_mut().query::<&TrailMote>().iter(app.world()).count();
assert!(motes > 0, "a pitched ball must leave a trail");
```

(Place it at an existing mid-pitch checkpoint; import `breakneck_baseball::game::fx::TrailMote`. If the harness advances in coarse steps that never observe mid-pitch, assert `>= 0` is useless — instead run until `Phase::Pitch` with `run_until`, step a few frames, then assert.)

- [ ] **Step 7: Native check per style** — cycle all six styles + a few colors in settings, throw pitches, confirm each reads (fading path; flames; shards; threaded rings; twinkle; bubbles), no perf hitch, trails absent on batted balls.

- [ ] **Step 8: Commit** — `feat: adjustable fading pitch trail with five interchangeable 3D styles`

---

### Task 7: Full verification + balance re-anchor

**Files:**
- Possibly modify: `src/game/rules.rs` (aim span), `src/game/ai.rs` (chase fuzz), `tests/balance_sim.rs` (bands only as a last, documented resort)

- [ ] **Step 1:** `cargo fmt && cargo clippy --all-targets` — clean.
- [ ] **Step 2:** `cargo test` (full suite: lib + all e2e) — PASS.
- [ ] **Step 3:** `cargo test --test balance_sim -- --nocapture` — the zone shrink shifts takes (more balls above 1.30). If K% / runs / HR leave their bands: first lever is the aim-map span (`0.45`) and the CPU chase fuzz (Task 1 Step 3) — retune those and re-run; only re-anchor the bands if the *measured healthy* distribution genuinely moved (document why in the test's doc comment, as the previous re-anchor commit did).
- [ ] **Step 4:** `cargo check --target wasm32-unknown-unknown` — both targets green.
- [ ] **Step 5:** `cargo run --features dev` — one last eyeball of all four features together; re-read TODO.md for mid-session additions.
- [ ] **Step 6:** Commit any tuning — `test: re-balance after the rulebook zone shrink` — and stop for the user's merge/PR call.

---

## Self-Review Notes

- **Spec coverage:** darker 3D wireframe + MLB zone rules → Tasks 1–2; batter 80–90% full-body framing → Task 3; no zoom-out unless uncaught → Task 4; adjustable colored fading path + 5 interchangeable 3D styles → Tasks 5–6 (Comet is the path; the other five are the interchangeable styles). Balance/dual-target constraints → Task 7.
- **Type consistency:** `PitchTrailStyle`/`TrailColor` defined in Task 5, consumed by name in Task 6; `pitch_gloved` defined in Task 4, referenced by Task 3's predicate (with an explicit stub-until-Task-4 note); `PLATE_HALF_WIDTH_M` defined in Task 1, consumed in Task 2.
- **Known risks:** (1) framing numbers need iteration — the test is the arbiter, Step 4 of Task 3 says to iterate; (2) balance bands may bust — Task 7 Step 3 defines the lever order; (3) e2e scripts assume zone-relative aims — Task 1 Step 6 checks them early, not at the end.
