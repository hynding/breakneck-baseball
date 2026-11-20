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
  `Visibility::Hidden` alongside its tint is fine on 0.17 and 0.18 wasm (verified 2026-09-26, 2026-11-20).
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
  `&mut` queries on wasm-rendered UI.** On Bevy 0.16–0.18 wasm/WebGL2 such a system keeps the
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
ball-follow → fielder cam on a descending fly (until the ball first comes down or hits the wall) → base cam on the
throw, where the call lands; a grounder takes the ground-ball shot instead and skips the fielder cam.
A throw in from the outfield (released beyond three quarters of the foul-line fence distance, 75 m in the stadium) first takes the
runner cam (`Shot::RunnerCam`): standing off the race of the runner it chases, on the side away
from the ball, so the throw comes into frame from beyond him; it hands over to the base cam once
the ball is 0.6 s from the bag (a deep throw flies 1.2 s, so the two shots share it), and never returns after the base cam is up
(`shots::runner_cam_wanted`; e2e `runner_cam.rs` checks a double and a triple). There is no
cutoff-relay shot: the fielding sim has no outfield cutoff man, only the double-play pivot.

**Instant replay** (`game::replay`, `src/game/present/replay/`): a recorder samples every rig root
and bone plus the ball into a ring (`clip::Recording`, 30 Hz, `RenderBudget::replay_secs` of play:
8/10/12 s on Low/Medium/High) in PostUpdate, after animation and Rapier writeback, before transform
propagation. On a big play (home run, walk-off, a throw home, or a dive; `Settings.replays`:
Off/BigPlays/Always) it starts 0.6 s into the result pause: it **pauses `Time<Virtual>`** (so flow,
physics, animation and locomotion all freeze), plays `clip::plan`'s segments on real time (the
dugout angle; a second, centre-field angle on Medium and High, at half speed around the key moment
on High), writing recorded poses onto the rigs just before propagation and putting the live ones
back in `Last`, so gameplay state is never touched and nothing is re-simulated. The action button
skips it after a 0.35 s grace. The tag (`ui/replay_tag.rs`, "REPLAY" / "SLOW MOTION" / the skip
prompt) follows the wasm UI rule like the walk-up card. e2e `replay.rs` pins frozen clock, untouched
live transforms, a moving camera, the tag, skip, and Off. Memory per tier: docs/DEVICE-CHECKS.md.

**Big-moment sequences** (`camera/sequence.rs`, pure): a short timed run of shots through the
result pause of a home run, a walk-off (not a home run) or a dive. `replay::BigMomentCue` reads the
moment off the play at the call (`called_at`, timed with `since_call`, since the flow's result timer
clamps at 1.2 s while the pause runs on for the trot), and the broadcast rig takes
`sequence_shot(moment, elapsed)` ahead of the ordinary Result shots: home run = `Shot::CrowdCam`
(up into the seats the ball landed among) for `CROWD_SECS`, the replay, then the trot orbit;
walk-off = `Shot::PileUp` on the plate for the whole pause, the replay inside it; dive =
`Shot::DiveCam(fielder)` low and tight for `DIVE_SECS`, then the replay. `replay_at` delays the
replay until the opening shot is done (the camera runs after `replay::BigMomentSystems`, so the call
frame already shows it). The crowd shot is Stadium-only (the front yard has no stands; its homer
goes straight to the orbit). An action press after the skip grace drops the sequence and its
replay: the ordinary shot list is back that same frame, and a dive's shot that ended or was skipped
hands the pause to the bag the throw went to, else the ball. e2e `big_moments.rs` pins each sequence
and the skip.
The grounder call is made once, just after the plate hold, by `shots::PlayRead` (low and coming
down, or already down during the hold per `ActivePlay::landed_at`). The ball-follow leads the ball to its predicted landing spot and fits its
lens to both, within an 80° cap; `shots::tests::the_ball_stays_in_the_middle_of_the_frame` pins the
ball inside the middle 80% of the frame for a sample of flies (to landing) and grounders (first 1.2 s);
smoothing only within a shot). The shots live in a pure catalog, `camera/shots.rs`: `pick_shot`
chooses, `Shot::spec` frames (a `ShotSpec`: eye, look-at, FOV, follow rate) and
`Shot::transition_from` says how to get there (`Transition::Cut` for every shot today; `Ease` and
`WhipPan` exist for new shots). Adding a shot is a `Shot` variant, its `spec` arm and a `pick_shot`
arm; `rigs.rs` only gathers inputs and moves the lens (`RigMotion`). The strike zone (`rules::ZONE_*`) is
drawn as a floating box; the batter finishes `BatterSwing` before the hidden run-out rig takes
over after its `RunDelay`.

## Graphics quality

`src/game/present/quality.rs` resolves `Settings::graphics` (Auto/Low/Medium/High) to a
`QualityTier` and owns MSAA on every `Camera3d`, the `DirectionalLightShadowMap` size, and the
render scale. The main camera (`camera::MainCamera`) always renders into an offscreen image that a
2D `UpscaleCamera` stretches over the window; that 2D camera is the pinned `IsDefaultUiCamera`, so
UI is always full resolution. Don't move the UI between cameras at runtime (or route the main
camera straight to the window "at 100%") — on wasm the whole UI went blank after such a switch.
Auto: native by GPU class (discrete High, integrated Medium, software Low), desktop browser
Medium, coarse-pointer browser or native mobile Low.

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
