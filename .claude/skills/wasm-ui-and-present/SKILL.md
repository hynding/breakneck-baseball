---
name: wasm-ui-and-present
description: Use when touching src/game/present/ui/, src/game/present/camera/, src/game/meta/menu.rs, src/game/meta/subs.rs, or src/game/meta/settings/; when a UI element "doesn't show up on web" or renders natively but not in the browser; or when changing banners, HUD, themes, cameras, game-feel (juice), or settings persistence. Covers the wasm/WebGL2 UI rendering gotcha in full.
---

# Wasm UI & Presentation

Full narrative in `reference/presentation.md`. The wasm gotcha below is the #1 cause of
"works natively, invisible on web" bugs — check it first.

## The wasm/WebGL2 UI gotcha

A UI element that is **fully transparent when first extracted** (alpha 0, or a bare container
root with no renderable component) is never rendered again, even after its colours change or
children are added — and **UI roots spawned mid-`Playing` don't render at all**. Therefore:

- Keep every element's alpha nonzero — use `ui::hidden_tint` for "invisible but renderable".
  The tint is the *paint* rule, not the hide mechanism: hidden chrome (banner pill, walk-up
  card, pause board, settings screen) also flips `Visibility` — `Hidden` while off, `Inherited`
  on show — so the 0.004-alpha panel never ghosts over a black sky (TODO 97). Spawning a root
  `Visibility::Hidden` alongside its tint is fine on 0.17 wasm (verified 2026-09-26).
- Give container roots a `BackgroundColor`.
- Spawn UI roots at game start (painted at spawn), then show/hide by **mutating children** of
  those roots. The pause/substitution board (`src/game/meta/subs.rs`) is the reference example:
  spawned hidden at game start, painted by mutating children. The curtain
  (`present/ui/curtain.rs`) is the opaque-at-spawn example: a full-screen black root painted
  solid at game start (a fade-in on PLAY BALL) and driven by alpha afterwards, floored at
  `hidden_tint`; the walk-up card (`present/ui/walkup.rs`) is the hidden-at-spawn example.
- Spawn-at-game-start systems key on the `game_start()` transition schedule
  (`OnTransition { MainMenu → Playing }`), never `OnEnter(Playing)` — otherwise they re-run on
  every unpause (`Playing ⇄ Paused` leaves the scene intact; teardown is `Playing → GameOver`).
- **Never tick a per-frame `ResMut` (e.g. a `Timer` resource) inside a system that also holds
  `&mut` queries on wasm-rendered UI.** On Bevy 0.16/0.17 wasm/WebGL2 such a system keeps the
  queried entities from ever being extracted — ECS visibility stays correct, native renders
  fine, and the bug survives every data-side probe (root-caused by system bisect 2026-08-25,
  TODO 29). Hold a deadline instead: the show system stamps `Some(elapsed + linger)` once, the
  fade system only *reads* until the deadline passes, then takes its single mutable step —
  `BannerFadeAt`/`StampFadeAt` in `src/game/present/ui/banner.rs` are the reference.
- Do **not** put `GlobalZIndex` on a *transparent, never-repainted* keep-alive root: on wasm the
  whole subtree stops extracting (bisected 2026-08-27 — alpha alone and z-index alone are fine;
  the pause board survives its tier because `update_board` repaints its root). Reserve explicit
  tiers for roots painted with real colors or repainted on show; keep announcement banners on
  the proven two-wrapper structure in `present/ui/hud.rs` and let spawn order stack them.

Verify UI changes on the web target (the `/run-web` skill), not just natively.

## Theme: data-driven colour

`src/game/core/theme.rs` `Theme` owns the UI palette, per-team `PlayerTemplate`s, ball styling,
the effect palette (`FxTheme`), the field dressing (`FieldTheme`: ground tints + lights),
sky/`ClearColor`, and `PlayerModelId`; cycled on the menu with T. UI reads `Res<Theme>`;
`src/game/sim/flow/` emits `BannerTone`s and **never colours** — presentation maps tone → colour.

## Cameras

Default duel view is the batting view (`DuelView::BattingZoom`, `FieldSpec::batting_zoom_eye`:
2 m behind the plate, whole batter screen-left, zone at centre — the reference composition in
`docs/agent/SMB3-REFERENCE-NOTES.md`, pinned by a framing test); **V** cycles four `DuelView`
framings (batting view / catcher POV / behind-pitcher / broadcast plate). The catcher
(`CatcherRole`, any fielder spawned at z < 0) and plate umpire are auto-hidden when they'd block
the active broadcast view. After contact the broadcast camera holds the plate framing for
`camera::BALL_FOLLOW_DELAY` (0.25 s), then **cuts** through a one-subject shot list (`camera::Shot`:
ball-follow → fielder cam on a descending fly → base cam on the throw, where the call lands;
smoothing only within a shot). The strike zone (`rules::ZONE_*`) is
drawn as a floating box; the batter finishes `BatterSwing` before the hidden run-out rig takes
over after its `RunDelay`.

## Juice (game feel)

`src/game/present/juice.rs` runs hit-stop (Solid/Perfect) and slow-mo (Perfect) by dialing
`Time<Virtual>` `relative_speed`, with a real-clock watchdog and `OnExit(Playing)` restore.
Any other writer of that speed (e.g. the debug Time tab) must compose with `juice::BaseSpeed`,
never assume 1.0. The headless test harness inserts `JuiceDisabled` — a slowed virtual clock
would corrupt scripted timing.

## Settings persistence

`src/game/meta/settings/` persists to the platform config dir on native
(`BREAKNECK_SETTINGS_PATH` overrides — the test seam) and to browser `localStorage` on wasm.
Its test-only `set_var`/`remove_var` calls are the crate's only `unsafe`, made sound by the
`ENV_LOCK` mutex — any test touching that env var must serialize through it. On the menu, **S**
opens the settings screen (per-player batting styles + master volume), **I** cycles game length.

## Audio

`src/game/present/audio.rs` synthesizes every effect at startup into in-memory WAVs
(deterministic hash noise, no asset files) and plays them off gameplay events — the `bevy`
`wav` Cargo feature is what lets bevy_audio decode them; don't drop it.
