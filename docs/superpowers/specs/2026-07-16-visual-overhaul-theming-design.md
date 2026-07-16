# Visual Overhaul & Theming — Design

**Date:** 2026-07-16
**Status:** Approved scope, implementation pending
**Builds on:** `2026-07-16-game-variants-front-yard-design.md`

## Goal

Make the game *look* like a game: a clean, confident UI that can be reskinned
wholesale; a ball and players you can actually see; a visible bat that tracks
the action; and a camera that pulls in tight for the pitch/swing duel. All
presentation becomes **data** (a `Theme`), mirroring how `Variant` made rules
and fields data — swapping in a new design means writing a new theme
definition, not touching systems.

## Requirements (from the user, made concrete)

| Ask | Decision |
|-----|----------|
| "Cleaner and sexier UI" | Card-based HUD: rounded translucent panels with accent borders, count shown as classic B/S/O indicator dots, banner in a centered pill, restyled menu/game-over screens. One consistent palette per theme. |
| "Themable — swap out new designs for everything" | `Theme` resource in a new `theme.rs`: UI palette + per-team player templates + ball styling. Two built-ins ship to prove the swap: **Daylight Classic** and **Midnight Neon**. **T** on the main menu cycles themes. |
| "Easier to see the ball" | Visual ball mesh ≈ 2.7× the physics radius (collider unchanged — physics identical), emissive glow, and a fading trail while in flight. Colors per theme. |
| "Players … own swappable templates" | `PlayerTemplate { jersey, cap, skin, trim, bat }` per team per theme. Players become composed rigs (torso capsule + head + cap + brim) instead of bare capsules. Defense and batter recolor to the correct team when the half-inning flips. |
| "Zooming in closer when a player is at bat" | New `duel_eye` / `duel_target` per field in `FieldSpec`. Broadcast camera uses the tight duel framing during `PrePitch`/`Pitch`, and smoothly lerps to the wide framing (following the ball) during `InPlay`/`Result`. |
| "Is there visibly a bat?" | There wasn't. The batter rig gains a bat on a shoulder pivot with a ~0.2 s eased swing animation driven by the batting team's action input — works for human and CPU identically (they share `Intents`). |

## Approaches considered

- **A. Data-driven `Theme` resource (chosen).** Mirrors `variant.rs`: plain
  structs, `ThemeId` enum with built-ins, systems read `Res<Theme>`.
  Consistent with the codebase's one abstraction pattern, unit-testable,
  serde-ready for future file-loaded themes.
- **B. Asset-file themes (RON/JSON).** Real reskins without recompiling, but
  adds asset loading + validation machinery nobody needs yet. Deferred; A's
  shape allows it later.
- **C. One-off restyle, no infrastructure.** Fails the "swappable" requirement.

## Architecture

### `theme.rs` (new module — owns all presentation data)

```rust
pub enum ThemeId { DaylightClassic, MidnightNeon }   // next(), label(), build()

pub struct Theme {            // Resource
    pub ui: UiTheme,
    pub home: PlayerTemplate,
    pub away: PlayerTemplate,
    pub ball: BallTheme,
}

pub struct UiTheme {
    pub panel_bg: Color,        // translucent card background
    pub panel_border: Color,
    pub accent: Color,          // titles, selected values, occupied pips
    pub text_primary: Color,
    pub text_dim: Color,
    pub pip_off: Color,
    pub count_ball: Color,      // B/S/O indicator-dot colors
    pub count_strike: Color,
    pub count_out: Color,
    pub tone_good: Color,       // banner tone palette
    pub tone_bad: Color,
    pub tone_info: Color,
    pub tone_epic: Color,
}

pub struct PlayerTemplate {
    pub jersey: Color,
    pub cap: Color,
    pub skin: Color,
    pub bat: Color,
}

pub struct BallTheme {
    pub color: Color,
    pub emissive: LinearRgba,
    pub visual_scale: f32,      // mesh radius multiplier (collider untouched)
    pub trail: Color,           // translucent trail material color
}
```

**Daylight Classic:** navy translucent panels, gold accent, white text;
home = royal blue jerseys / navy caps, away = red jerseys / dark-red caps;
white ball with a warm glow and white trail.
**Midnight Neon:** near-black panels, cyan accent; home = cyan, away =
magenta; neon-yellow ball with a strong glow and yellow trail.

`GameConfig` gains `theme: ThemeId`. The `Theme` resource is app-init to
Daylight Classic; the menu's cycle handler rebuilds it immediately (`*theme =
id.build()`) and respawns the menu UI so the new palette is visible at once.

### Banner tones (`flow.rs` → `ui.rs` decoupling)

`PlayBanner` becomes `{ text: String, tone: BannerTone }` with
`enum BannerTone { Good, Bad, Info, Epic }` (hit / out / count / home-run &
walk). `flow.rs` chooses the tone; `ui.rs` maps tone → `UiTheme` color. Flow
no longer knows any color.

### HUD (`ui.rs`, rebuilt visuals — same data sources)

- **Scoreboard card** (top-left): rounded (12 px) translucent panel with a
  1.5 px border. Line 1: `▲ 1` / `▼ 1` inning marker in accent. Line 2:
  `AWAY 0   HOME 0` with small team-color chips. Line 3: B/S/O indicator
  dots — `threshold − 1` dots each (classic scoreboard lights), filled with
  `count_*` colors as the count climbs; dot counts come from the active
  `Ruleset`, so front-yard rule tweaks render correctly.
- **Base ring** (top-right): existing N-pip ring, restyled — pips get borders
  and use accent (occupied) / `pip_off` (empty).
- **Banner** (center): a pill container (rounded 24 px, panel colors) that is
  `Visibility::Hidden` when empty; text 46 px in the tone color.
- **Controls hint** (bottom): small `text_dim` line in a slim pill.

### Menu & game-over (`menu.rs`)

Single centered card (padding 28, radius 16, theme panel colors): title in
accent + one-line dim subtitle, then options — `[1] One Player (vs CPU)`,
`[2] Two Players`, `[F] Field · <label>`, `[T] Theme · <label>` — then the
controller status line and a dim controls footer. **T** (or gamepad North)
cycles `ThemeId`. Game-over screen becomes the same card style; winner text
uses the winning team's jersey color.

### Ball (`ball.rs`)

- Mesh radius = `BALL_RADIUS * theme.ball.visual_scale` (≈ 0.10 m); collider
  stays `Collider::ball(BALL_RADIUS)` — physics and rules are untouched.
- `StandardMaterial { base_color, emissive }` from the theme.
- **Trail:** while `InFlight` and speed > 8 m/s, spawn a ghost sphere every
  0.025 s (shared mesh + shared translucent material from a `TrailAssets`
  resource built on entering `Playing`). Each ghost shrinks from full scale
  to zero over 0.35 s, then despawns. Ghosts are `GameplayEntity`s.

### Player rigs & bat (`player.rs`)

- `spawn_player_rig(role, template, facing)` builds: parent (existing
  kinematic body + capsule collider + role markers) with mesh children —
  torso capsule (jersey), head sphere (skin), cap disc + brim (cap color;
  brim offset along facing). Shared proportions; ~1.5 m tall to match the
  collider.
- **Team recolor:** materials for both templates are created once per game
  (`TeamPalette` resource). A system watching `ScoreBoard` change reassigns
  jersey/cap material handles: pitcher + fielders wear the *fielding* team's
  template, the batter wears the *batting* team's. (Fixes today's static
  red-vs-blue regardless of who is fielding.)
- **Bat:** child pivot on the batter at shoulder height; bat = cylinder
  (r 0.032, length 0.84) extending +Y from the pivot, colored
  `template.bat`. `Swing` component drives three states: Idle (cocked pose),
  Swinging (sweep −126° → +69° around Y over 0.16 s, eased), Recovering
  (return over 0.25 s). Triggered whenever the batting team's `action` fires
  during `PrePitch`/`Pitch` — same signal for humans and the CPU.

### Camera (`camera.rs` + `variant.rs`)

- `FieldSpec` gains `duel_eye` / `duel_target`:
  standard `(−1.6, 2.2, −5.2)` → `(0.2, 1.15, 5.0)`; front yard
  `(−1.4, 2.0, −4.2)` → `(0.2, 1.0, 4.0)` (offset framing: batter on the
  right third, pitcher center).
- The broadcast system picks desired eye+target by phase (duel for
  `PrePitch`/`Pitch`; wide + ball-follow for `InPlay`/`Result`) and lerps
  **both** with the existing exponential smoothing (a `BroadcastRig { eye,
  target }` resource replaces `BroadcastTarget`), so the zoom in/out is a
  glide, not a cut.

## Error handling & edge cases

- Theme cycling on the menu respawns the menu UI; in-game HUD/world always
  read the `Theme` that was live when the game started (resource is only
  rewritten on the menu, so no mid-game restyle tearing).
- Trail ghosts are capped by lifetime (~14 concurrent max at 60 fps); all are
  `GameplayEntity` so scene teardown cleans them.
- The recolor system runs on `ScoreBoard` change detection only — no per-frame
  material churn.
- Bat animation is purely visual; contact timing still comes from
  `rules::hit_velocity` — a mistimed press animates a whiff, which is correct.

## Testing / verification

1. **Unit tests** (`theme.rs`): cycle wraps and visits both themes; labels
   differ; the two themes' accents/jerseys differ (proves they're distinct
   data). `variant.rs` tests assert the new duel framing fields sit in front
   of home plate (negative Z eye, positive Z target).
2. **CI gate:** fmt, `cargo test`, clippy `-D warnings` native + wasm, wasm
   build.
3. **Browser play-test:** both themes × both fields — screenshot the menu,
   the duel framing at the pitch, the in-play wide framing, the trail, a
   swing (bat visible), and the HUD card; confirm defense/batter colors flip
   with the half-inning.

## Build sequence

1. `theme.rs` data model + tests; `GameConfig.theme`; menu **T** cycle
   (functional, unstyled).
2. HUD + menu + game-over restyle from `UiTheme`; banner-tone refactor.
3. Ball visibility: scaled emissive mesh + trail.
4. Player rigs from templates + half-inning recolor + animated bat.
5. Duel-camera zoom (`FieldSpec` framing + `BroadcastRig`).
6. Full gate + browser verification of both themes and both fields.
