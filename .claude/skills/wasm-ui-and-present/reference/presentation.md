# Presentation Layer — Full Narrative

Verbatim source: the pre-slim CLAUDE.md (also preserved whole in `docs/agent/ARCHITECTURE-FULL.md`).

## The wasm/WebGL2 UI gotcha

A UI element that is fully transparent (alpha 0, or a bare container root with no renderable
component) when first extracted is never rendered again, even after its colours change or
children are added — and UI roots spawned mid-`Playing` don't render at all. Keep every
element's alpha nonzero (see `ui::hidden_tint`), give container roots a `BackgroundColor`, and
show/hide by mutating children of roots that were painted at spawn.

## State machine & scene lifecycle

`src/game/mod.rs` defines `GameState` (`MainMenu → Playing ⇄ Paused → GameOver`; gameplay systems
use `.run_if(in_state(GameState::Playing))`). Scene spawn/reset systems key on the `game_start()`
transition schedule (`OnTransition { MainMenu → Playing }`, never `OnEnter(Playing)`) and
teardown on `Playing → GameOver`, so pausing (`Playing ⇄ Paused`) leaves the whole scene intact —
a new spawn-at-game-start system must use `game_start()` or it will re-run on every unpause.

**Esc/P** (gamepad Start) pauses while the ball is dead — refused while the ball is physically in
flight, since Rapier steps regardless of state — and shows the substitution board
(`src/game/meta/subs.rs`, spawned hidden at game start and painted by mutating children): each
team has a `src/game/core/roster.rs` `Rosters` lineup of nine named, numbered players plus a
bench (batting slot i = defensive position i, arcade style); swaps rewrite `Rosters` and the
jerseys/HUD follow.

## Theme & tones

`src/game/core/theme.rs` defines `Theme` (UI palette, per-team `PlayerTemplate`s, ball styling,
sky/`ClearColor`, and the `PlayerModelId` that picks the rig construction) with built-ins behind
`ThemeId`, cycled on the menu with T. UI reads `Res<Theme>`; `src/game/sim/flow/` emits
`BannerTone`s and never colours.

## Cameras

The default duel view is the catcher's point of view — the lens just over the crouched catcher's
helmet (`FieldSpec::duel_eye`); **V** cycles four at-bat framings (`DuelView`: catcher POV /
behind-pitcher / batting zoom / broadcast plate), with the catcher (`CatcherRole`, any fielder
spawned at z < 0) and plate umpire auto-hidden when they'd block the active broadcast view —
drawing the exact called zone (`rules::ZONE_*` consts) as a floating box above his head and
keeping the catcher in a crouch clip through the pitch; after contact the broadcast camera holds
the plate framing for `camera::BALL_FOLLOW_DELAY` (1 s) before chasing the ball, while the batter
(side-on in the box, facing the plate) finishes the `BatterSwing` follow-through and the hidden
run-out rig takes over after its `RunDelay`. First base is at world −X (the behind-home camera
renders −X on screen-right).

## Field surfaces & FX

Field surfaces are procedural runtime textures (mow-striped grass, speckled clay —
`src/game/present/field/`, no asset files) layered per docs/BASEBALL.md: dirt basepath diamond,
grass infield, 13 ft cutouts, regulation mound + rubber and 18 in bags. While an uncalled fly
ball is up, `src/game/present/fx/` parks a landing ring on its live-predicted touchdown spot,
sized by the remaining hang time. The home run launches a fireworks show over the outfield
(denser off a Perfect, read from `Play::last_contact_quality`), and the broadcast camera orbits
the diamond through the trot (`Play::is_home_run()` during `Phase::Result`).

## Juice

Off the `ContactEvent`, `src/game/present/juice.rs` runs the game feel — a frame-counted
hit-stop on Solid/Perfect and a slow-mo tail on Perfect, all by dialing `Time<Virtual>`'s
`relative_speed` with a real-clock watchdog and an `OnExit(Playing)` restore so it can never
stick — behind a `JuiceDisabled` resource the headless harness inserts (a slowed virtual clock
would corrupt scripted timing). Because both `juice.rs` and the debug Time tab write
`Time<Virtual>`'s `relative_speed`, any future writer of that speed must compose with
`juice::BaseSpeed` — the "normal" speed effects restore to — rather than assuming 1.0, so
slow-mo and manual dial-downs never fight over the clock.

## Audio

Sound is procedural: `src/game/present/audio.rs` synthesizes every effect at startup into
in-memory WAVs (deterministic hash noise, no asset files) and plays them off gameplay events —
the `bevy = { features = ["wav"] }` Cargo feature is what lets bevy_audio decode them, so don't
drop it. It layers a looping crowd murmur under a roar (Perfect swing or deep fly) / crowd-peak
roar (home run, louder) / groan (swinging K), plus three quality-keyed bat cracks; the glove pop
plays off `PitchCaughtEvent`.

## Settings

Settings are managed via `src/game/meta/settings/`, persisting to the platform config directory
on native (overridable via `BREAKNECK_SETTINGS_PATH` for tests) and to browser `localStorage` on
wasm; its test-only `std::env::set_var`/`remove_var` calls are the crate's only `unsafe`, made
sound by the `ENV_LOCK` mutex serializing every test that touches that env var. On the menu,
**I** cycles game length (1/3/6/9 innings) and **S** opens the settings screen (per-player
batting styles — consumed by the batting adapters in `src/game/sim/batting.rs` — and master
volume).
