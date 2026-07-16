# Visual Overhaul & Theming Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Themeable presentation layer (swappable UI palette, player templates, ball styling), visible ball + composed player rigs + animated bat, and an at-bat duel camera.

**Architecture:** New `game/theme.rs` owns all presentation data as a `Theme` resource (mirrors `variant.rs`), cycled from the menu with **T**. `ui.rs`/`menu.rs` restyle from `UiTheme`; `flow.rs` emits banner *tones* instead of colors; `ball.rs` gets a scaled emissive mesh + trail ghosts; `player.rs` builds multi-part rigs from `PlayerTemplate` with half-inning recolor and a `Swing`-animated bat; `camera.rs` lerps between duel and wide framings from new `FieldSpec` fields.

**Tech Stack:** Rust, Bevy 0.15, bevy_rapier3d. Dual-target native + wasm32.

## Global Constraints

- PATH prefix for all cargo commands: `export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"`
- CI gate green at every commit: `cargo fmt --check`, `cargo test`, clippy `-D warnings` (native + wasm), `cargo check --target wasm32-unknown-unknown`.
- Physics and rules untouched: ball collider stays `Collider::ball(BALL_RADIUS)`; all 34 existing tests keep passing unmodified.
- No new dependencies, no assets, no `rand` (wasm-safe determinism).

---

### Task 1: Theme data model + menu cycle (functional, unstyled)

**Files:** Create `src/game/theme.rs`; modify `src/game/mod.rs` (register module, `GameConfig.theme`, init resource), `src/game/menu.rs` (T-cycle + theme line).

**Interfaces — Produces:**
- `ThemeId { DaylightClassic, MidnightNeon }` with `next() -> ThemeId`, `label() -> &'static str`, `build() -> Theme`.
- `Theme { ui: UiTheme, home: PlayerTemplate, away: PlayerTemplate, ball: BallTheme }` (`#[derive(Resource, Clone)]`).
- `UiTheme { panel_bg, panel_border, accent, text_primary, text_dim, pip_off, count_ball, count_strike, count_out, tone_good, tone_bad, tone_info, tone_epic: Color }`.
- `PlayerTemplate { jersey, cap, skin, bat: Color }`.
- `BallTheme { color: Color, emissive: LinearRgba, visual_scale: f32, trail: Color }`.
- `GameConfig` gains `pub theme: ThemeId`.

- [ ] **Step 1: Failing tests** (in `theme.rs`):

```rust
#[test]
fn cycle_visits_both_and_wraps() {
    assert_eq!(ThemeId::DaylightClassic.next(), ThemeId::MidnightNeon);
    assert_eq!(ThemeId::MidnightNeon.next(), ThemeId::DaylightClassic);
}
#[test]
fn themes_are_distinct_designs() {
    let (day, night) = (ThemeId::DaylightClassic.build(), ThemeId::MidnightNeon.build());
    assert_ne!(ThemeId::DaylightClassic.label(), ThemeId::MidnightNeon.label());
    assert_ne!(day.ui.accent, night.ui.accent);
    assert_ne!(day.home.jersey, night.home.jersey);
    assert_ne!(day.ball.color, night.ball.color);
    assert!(day.ball.visual_scale > 1.5 && night.ball.visual_scale > 1.5);
}
```

- [ ] **Step 2:** `cargo test theme` → FAIL (module missing).
- [ ] **Step 3:** Implement. Palettes:
  - **Daylight Classic:** panel_bg `srgba(0.04,0.07,0.14,0.85)`, border `srgba(1,1,1,0.14)`, accent `srgb(1.0,0.84,0.25)`, text `WHITE`/`srgba(1,1,1,0.65)`, pip_off `srgba(1,1,1,0.25)`, count ball/strike/out `srgb(0.45,0.85,0.45)`/`srgb(1.0,0.72,0.25)`/`srgb(1.0,0.42,0.35)`, tones good/bad/info/epic `srgb(0.55,1.0,0.65)`/`srgb(1.0,0.5,0.4)`/`srgb(0.95,0.9,0.7)`/`srgb(1.0,0.84,0.25)`. Home `{jersey srgb(0.22,0.42,0.9), cap srgb(0.08,0.14,0.38), skin srgb(0.87,0.67,0.5), bat srgb(0.72,0.5,0.28)}`; Away `{jersey srgb(0.88,0.22,0.2), cap srgb(0.4,0.06,0.06), same skin, same bat}`. Ball `{color WHITE, emissive LinearRgba::rgb(1.5,1.4,1.1), visual_scale 2.7, trail srgba(1,1,0.9,0.35)}`.
  - **Midnight Neon:** panel_bg `srgba(0.01,0.02,0.05,0.88)`, border `srgba(0.2,0.95,1.0,0.35)`, accent `srgb(0.25,0.95,1.0)`, text `srgb(0.92,0.98,1.0)`/`srgba(0.8,0.9,1.0,0.55)`, pip_off `srgba(0.5,0.8,1.0,0.2)`, count `srgb(0.3,1.0,0.7)`/`srgb(1.0,0.85,0.2)`/`srgb(1.0,0.3,0.5)`, tones `srgb(0.4,1.0,0.8)`/`srgb(1.0,0.45,0.3)`/`srgb(0.8,0.9,1.0)`/`srgb(1.0,0.25,0.75)`. Home `{jersey srgb(0.1,0.85,0.95), cap srgb(0.02,0.2,0.3), skin srgb(0.8,0.7,0.62), bat srgb(0.15,0.15,0.2)}`; Away `{jersey srgb(1.0,0.2,0.72), cap srgb(0.3,0.02,0.2), same skin/bat}`. Ball `{color srgb(1.0,0.95,0.35), emissive LinearRgba::rgb(2.2,2.0,0.6), visual_scale 2.7, trail srgba(1.0,0.95,0.4,0.4)}`.
  - `mod.rs`: `pub mod theme;`, `GameConfig { mode, variant, theme: ThemeId }`, `.insert_resource(ThemeId::DaylightClassic.build())`.
  - `menu.rs`: `cycle_theme` system (T key / `GamepadButton::North`): `config.theme = config.theme.next(); *theme_res = config.theme.build();` plus a `Text` line showing the label (plain styling this task).
- [ ] **Step 4:** `cargo test` all green; clippy (temporary `#[allow(dead_code)]` on not-yet-consumed theme fields is acceptable *only* if needed; remove by Task 5).
- [ ] **Step 5:** Commit `feat: theme data model with two built-in themes and menu cycling`.

---

### Task 2: HUD + menu restyle from UiTheme; banner tones

**Files:** Modify `src/game/flow.rs` (PlayBanner tone), `src/game/ui.rs` (card HUD), `src/game/menu.rs` (card menu + game-over).

**Interfaces — Produces:** `pub enum BannerTone { Good, Bad, Info, Epic }`; `PlayBanner { pub text: String, pub tone: BannerTone }`. **Consumes:** `Res<Theme>`, `Res<Ruleset>` (dot counts).

- [ ] **Step 1:** `flow.rs`: replace every `PlayBanner::new(text, Color…)` with tone-based construction — hits→`Good`, outs/strikeout→`Bad`, ball/strike/foul→`Info`, home run & walk→`Epic`.
- [ ] **Step 2:** `ui.rs` rebuild `spawn_hud(commands, field: Res<FieldSpec>, rules: Res<Ruleset>, theme: Res<Theme>)`:
  - Scoreboard card: `Node { padding: 12, row_gap: 6, flex_direction: Column }`, `BackgroundColor(ui.panel_bg)`, `BorderColor(ui.panel_border)`, `border: UiRect::all(1.5)`, `BorderRadius::all(12)`. Children: inning `Text` (accent, 20), score `Text` (text_primary, 22), and a B/S/O row: for each of `[("B", rules.balls_per_walk-1, count_ball), ("S", rules.strikes_per_out-1, count_strike), ("O", rules.outs_per_half-1, count_out)]` a label + that many 10 px `BorderRadius::MAX` dot nodes with `CountDot { kind, index }`.
  - `update_count_dots` system: fill dot background with its color when `index < current`, else `pip_off`.
  - Inning text `▲`/`▼` prefix; score text plain `AWAY n   HOME n` (chips deferred — YAGNI, colors appear in the winner screen and jerseys).
  - Base ring: pips get `BorderColor(ui.panel_border)` + border 1 px; on = `ui.accent`, off = `ui.pip_off`.
  - Banner: pill parent `Node { padding: (10, 26), BorderRadius::all(24), BackgroundColor(panel_bg), BorderColor(panel_border) }` + `BannerPill` marker, `Visibility::Hidden` initially; child `BannerText`. `show_banner` maps tone→color and sets `Visibility::Inherited`; `fade_banner` hides the pill.
  - Controls hint: 13 px `text_dim`.
- [ ] **Step 3:** `menu.rs`: one centered card (`BorderRadius::all(16)`, padding 28, panel colors, `row_gap` 14) containing title (accent 50) + subtitle (dim 15, "backyard arcade baseball"), options block (`[1] One Player (vs CPU)`, `[2] Two Players`, `[F] Field · {label}`, `[T] Theme · {label}` — bracketed keys in accent via separate `TextSpan`s is overkill; single-line texts fine), status line, footer (dim 13). Game-over: same card; winner text colored `theme.home.jersey` / `theme.away.jersey` / text_primary for tie. Theme cycle respawns the menu (despawn `MenuUi` root + call spawn fn) so colors refresh.
- [ ] **Step 4:** `cargo test`, clippy both targets, `cargo run`-free visual check deferred to Task 6.
- [ ] **Step 5:** Commit `feat: theme-driven HUD, menu, and banner tones`.

---

### Task 3: Ball visibility — scaled glow mesh + trail

**Files:** Modify `src/game/ball.rs`.

**Interfaces — Consumes:** `Res<Theme>` (`theme.ball`). **Produces:** `TrailAssets { mesh: Handle<Mesh>, material: Handle<StandardMaterial> }` resource; `TrailGhost(Timer)` component.

- [ ] **Step 1:** `spawn_ball` uses `Sphere::new(BALL_RADIUS * theme.ball.visual_scale)` mesh and `StandardMaterial { base_color: theme.ball.color, emissive: theme.ball.emissive, perceptual_roughness: 0.4 }`. Collider unchanged. Also (same system) insert `TrailAssets` built from a `Sphere::new(BALL_RADIUS * theme.ball.visual_scale * 0.8)` mesh and `StandardMaterial { base_color: theme.ball.trail, alpha_mode: AlphaMode::Blend, unlit: true }`.
- [ ] **Step 2:** Add systems (in `BallPlugin`, `run_if(in_state(GameState::Playing))`):

```rust
fn spawn_trail(time, mut timer: Local<f32>, ball_q: Query<(&Transform, &Velocity), (With<Baseball>, With<InFlight>)>, assets: Option<Res<TrailAssets>>, mut commands) {
    // every 0.025 s while flying faster than 8 m/s, drop a ghost at the ball
}
fn fade_trail(time, mut q: Query<(Entity, &mut TrailGhost, &mut Transform)>, mut commands) {
    // scale = 1 - fraction elapsed of 0.35 s; despawn when finished
}
```

- [ ] **Step 3:** `cargo test` + clippy + wasm check. Commit `feat: high-visibility glowing ball with motion trail`.

---

### Task 4: Player rigs, half-inning recolor, animated bat

**Files:** Modify `src/game/player.rs`.

**Interfaces — Consumes:** `Res<Theme>`, `Res<ScoreBoard>`, `Res<Intents>`, `Res<Play>` (phase). **Produces:** `TeamPalette { home: RigMaterials, away: RigMaterials }` resource where `RigMaterials { jersey, cap, skin, bat: Handle<StandardMaterial> }`; child markers `JerseyPart`, `CapPart`, `BatPart`; `Swing` component.

- [ ] **Step 1:** `build_team_palette` (OnEnter(Playing), before spawns via system ordering `.chain()`): create handles for both templates, insert `TeamPalette`.
- [ ] **Step 2:** `spawn_rig(commands, meshes, palette_side, parent_bundle, facing: f32 /* +1 batter, -1 fielders */)` helper: parent keeps collider/markers/`CollisionGroups`; children:
  - torso `Capsule3d::new(0.3, 0.9)` at origin, `JerseyPart`;
  - head `Sphere::new(0.18)` at `(0, 0.75, 0)`, skin material;
  - cap `Cylinder::new(0.19, 0.09)` at `(0, 0.92, 0)`, `CapPart`;
  - brim `Cuboid::new(0.26, 0.03, 0.16)` at `(0, 0.9, 0.17*facing)`, `CapPart`.
  Pitcher/fielders spawn with fielding-team materials, batter with batting-team materials (initial = Away fields? — top of 1st: Away *bats*, Home fields → pitcher/fielders = home materials, batter = away).
- [ ] **Step 3:** Batter additionally gets a bat pivot child at `(-0.22, 0.5, 0)` with `Swing::default()` + `BatPart`-material bat mesh child `Cylinder::new(0.032, 0.84)` offset `(0, 0.42, 0)`:

```rust
#[derive(Component, Default)]
pub enum Swing { #[default] Idle, Swinging(Timer), Recovering(Timer) }
const IDLE_ROT: fn() -> Quat = || Quat::from_euler(EulerRot::ZXY, -0.5, 0.35, 0.0);
// trigger: batting team intent.action while Phase::PrePitch | Phase::Pitch → Swing::Swinging(Timer 0.16s)
// swinging: t = eased fraction (1-(1-f)^3); pivot rot = Quat::from_rotation_y(-2.2 + 3.4*t) * Quat::from_rotation_z(-1.45)
// finished → Recovering(0.25s): slerp back to IDLE_ROT; finished → Idle
```

- [ ] **Step 4:** `recolor_teams` system (`run_if(resource_changed::<ScoreBoard>)` + in Playing): fielding side → pitcher+fielder `JerseyPart`/`CapPart` handles from that team's `RigMaterials`; batter parts from batting team's. Query children via `Children` of role entities; simplest: give every rig child a `RigRole { role: Role, part: Part }` component at spawn (`Role { Defense, Batter }`, `Part { Jersey, Cap, Bat }`) and reassign `MeshMaterial3d` handles by lookup.
- [ ] **Step 5:** `cargo test`, clippy both targets. Commit `feat: composed player rigs with team recolor and animated bat`.

---

### Task 5: Duel camera zoom

**Files:** Modify `src/game/variant.rs` (+2 fields, both variants, test), `src/game/camera.rs` (BroadcastRig lerp).

**Interfaces — Produces:** `FieldSpec.duel_eye/duel_target: Vec3`. `BroadcastRig { eye: Vec3, target: Vec3 }` resource (replaces `BroadcastTarget`).

- [ ] **Step 1:** Add fields — standard `duel_eye (−1.6, 2.2, −5.2)`, `duel_target (0.2, 1.15, 5.0)`; front yard `duel_eye (−1.4, 2.0, −4.2)`, `duel_target (0.2, 1.0, 4.0)`. Test in `variant.rs`:

```rust
#[test]
fn duel_framing_sits_behind_home_looking_out() {
    for id in [VariantId::Standard, VariantId::FrontYard] {
        let f = id.field();
        assert!(f.duel_eye.z < 0.0 && f.duel_target.z > 0.0);
        assert!(f.duel_eye.z > f.broadcast_eye.z, "duel eye is closer than broadcast");
    }
}
```

- [ ] **Step 2:** `camera.rs`: replace `BroadcastTarget` with `BroadcastRig { eye, target }` (Default = standard wide framing constants). `broadcast_camera` chooses desired per phase: `PrePitch | Pitch` → `(field.duel_eye, field.duel_target)`; `InPlay` → wide eye (+ existing deep-ball pull-back) & ball-follow target; `Result` → wide framing. Lerp both `rig.eye` and `rig.target` with `1 - exp(-5 * dt)`; camera transform from the rig every frame.
- [ ] **Step 3:** `cargo test`, clippy, wasm check. Commit `feat: at-bat duel camera with smooth zoom transitions`.

---

### Task 6: Full gate + browser verification (both themes × both fields)

Run the local CI gate; `/run-web` rebuild; in the browser verify with screenshots: menu (both themes via T), duel framing during PrePitch/Pitch (bat visible on batter, rigs have caps/heads), swing animation on Space during a pitch, ball glow + trail during a pitch/hit, wide framing + trail in play, HUD card + count dots climbing, defense/batter colors swapping when the half-inning flips (play until 3 outs or use a 2P game to force it quickly), game-over card. Fix what's found; final commit `feat: visual overhaul verified across themes and fields`.

## Self-review

- Spec coverage: theme model (T1), HUD/menu/banner (T2), ball (T3), rigs/recolor/bat (T4), duel camera (T5), verification matrix (T6). ✓
- Type consistency: `ThemeId::build()`, `Theme.ui/home/away/ball`, `BannerTone`, `TrailAssets/TrailGhost`, `TeamPalette/RigMaterials/RigRole`, `Swing`, `BroadcastRig`, `FieldSpec.duel_*` used consistently across tasks. ✓
- No placeholders: palettes, positions, sizes, easing, and timings are concrete; code sketches show the intended shape where full code would duplicate the implementation. ✓
