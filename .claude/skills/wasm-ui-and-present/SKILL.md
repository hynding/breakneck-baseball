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
- Give container roots a `BackgroundColor`.
- Spawn UI roots at game start (painted at spawn), then show/hide by **mutating children** of
  those roots. The pause/substitution board (`src/game/meta/subs.rs`) is the reference example:
  spawned hidden at game start, painted by mutating children.
- Spawn-at-game-start systems key on the `game_start()` transition schedule
  (`OnTransition { MainMenu → Playing }`), never `OnEnter(Playing)` — otherwise they re-run on
  every unpause (`Playing ⇄ Paused` leaves the scene intact; teardown is `Playing → GameOver`).

Verify UI changes on the web target (the `/run-web` skill), not just natively.

## Theme: data-driven colour

`src/game/core/theme.rs` `Theme` owns the UI palette, per-team `PlayerTemplate`s, ball styling,
sky/`ClearColor`, and `PlayerModelId`; cycled on the menu with T. UI reads `Res<Theme>`;
`src/game/sim/flow/` emits `BannerTone`s and **never colours** — presentation maps tone → colour.

## Cameras

Default duel view is the catcher's POV (`FieldSpec::duel_eye`); **V** cycles four `DuelView`
framings (catcher POV / behind-pitcher / batting zoom / broadcast plate). The catcher
(`CatcherRole`, any fielder spawned at z < 0) and plate umpire are auto-hidden when they'd block
the active broadcast view. After contact the broadcast camera holds the plate framing for
`camera::BALL_FOLLOW_DELAY` (1 s) before chasing the ball. The strike zone (`rules::ZONE_*`) is
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
