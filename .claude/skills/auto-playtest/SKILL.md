---
name: auto-playtest
description: Use for "playtest", "run a game automatically", "verify 2 player", "test PCI/Meter/Classic", attract mode, or when adding an input device or batting adapter. Covers the Director (scripted/CPU control of every player slot), the mode matrix, .ron scripts, and headless vs native vs wasm self-driving runs with the Coach watching.
---

# Automated playtesting

The **Director** (`src/game/sim/director.rs`, facade `game::director`) is the
one synthetic-input seam. It writes `Intents` in the `DriveGame` schedule —
after the input plugin's `PreUpdate` clear, before `Update` — identically
headless, native windowed, and wasm. Absent the `Director` resource it is
completely inert.

**The covering rule**: any new control mechanism (input device, batting
adapter, control scheme) must route through `Intents` → the batting
adapters' `SwingCommands`. Do that and the Director — and every script and
matrix cell — covers it automatically. A mechanism that bypasses `Intents`
is untestable here and violates the seam; don't build one.

## Policies

`Director { home, away }`, one `Policy` per slot:

- `Human` — real input passes through (default).
- `Cpu` — the slot's `InputSource` is routed to the existing AI (attract
  mode). CPU slots always bat Classic — `batting::style_for` — regardless
  of settings; `tests/e2e_matrix.rs::cpu_vs_cpu_ignores_style` pins this.
- `Scripted(script)` — a data script drives the slot. Scripted slots stay
  keyboard-sourced (pseudo-human), so the configured batting style applies.

## Scripts

Data, not closures: `.ron` files in `tests/scripts/`, embedded into the
binary (works on wasm) and loaded by name with `director::script("name")`.
Built-ins: `balanced` (pitch + dead-red swing), `take-all` (strikeout
mill), `steal-artist` (lead-stretch + send + swing).

Vocabulary (see `director.rs` for the full enums):

- **Conditions**: `Phase(...)`, `OnOffense`/`OnDefense`, `InStealWindow`,
  `PlateEta(early_ms)` (the live pitch's signed timing error has reached
  `-early_ms` — fires the first frame it holds), `BallGathered`,
  `PhaseElapsed(at_least)`, and `All`/`Any`/`Not`.
- **Actions**: `Aim(x, y)`, `Press` (raw edge), `HoldPress` (raw level),
  `ThrowTo(First|Second|Third|Home)` (once per play, while gathered), and
  `Swing` — the style-aware commit: the director synthesizes a press
  (Classic/PCI) or a load-and-release (Swing Meter); `adapt_swings` still
  grades it. Write `Swing`, never a style-specific pattern, and one script
  covers all three styles.

Reactive rules are what play baseball; `steps` (timed intents) exist only
for boot choreography. To add a script: write the `.ron`, register it in
`BUILTIN_SCRIPTS`, and the `every_builtin_script_parses` unit test covers
parsing.

## The mode matrix

`tests/e2e_matrix.rs`: {1P vs CPU, 2P} × {Classic, Meter, PCI} + one
CPU-vs-CPU cell, each a short `balanced`-scripted game asserting progress,
a judged swing, and **zero Coach violations**. ~37 s wall for all seven
cells — runs on every `cargo test`, not `#[ignore]`d. The harness seam is
`common::start_matrix_game(app, mode, style, script)`.

Headless conventions still apply (`tests/common/mod.rs`): inject from
`DriveGame` only (the schedule now lives in the game and the harness
re-exports it), `JuiceDisabled` stays inserted, 240 Hz virtual time.

**Determinism**: the CPU's "randomness" is hash noise over *virtual elapsed
time* (`ai::hash01(time.elapsed_secs() * k)`), and the harness steps time
manually — so runs are reproducible without a seed as long as the schedule
order is fixed. `deterministic_headless_app()` pins single-threaded
execution for frame-for-frame reproducibility (the balance sim uses it);
the default multi-threaded harness is reproducible in outcome but may
jitter on ambiguous system orderings.

## Visual runs (`--features autoplay`)

A separate additive feature (not folded into `dev`, whose dylib linking it
doesn't want). The game drives itself: menus scripted (1 to start, Enter
past game-over — an endless attract loop), slots handed to the Director
(default CPU vs CPU), Coach on. Findings stream as `COACH_FINDING {json}`
lines; the report persists every ~10 s and finally at game end.

Native watched run:

```sh
cargo run --features autoplay                      # attract mode, watch it play
BREAKNECK_AUTOPLAY_SCRIPT=balanced \               # script Home vs the CPU
BREAKNECK_AUTOPLAY_INNINGS=1 \                     # short game
BREAKNECK_AUTOPLAY_ONCE=1 \                        # exit after the report
BREAKNECK_COACH_REPORT=coach-report.json \
  cargo run --features autoplay
```

Findings → stderr; `coach-report.json` → cwd (or the env path).

Browser run (extends the `run-web` flow):

```sh
cargo build --target wasm32-unknown-unknown --features autoplay
wasm-bindgen --out-dir web/out --target web target/wasm32-unknown-unknown/debug/breakneck-baseball.wasm
python3 -m http.server --directory web 8080
```

Load it with the Chrome DevTools tooling, **click the canvas first** (audio
needs a user gesture), and watch the console: `bb-state menu/playing`,
`bb-first-pitch`, `COACH_FINDING …`. Pull the report any time from
`localStorage.getItem('bb-coach-report')`; screenshot at findings and at
the `playtest-review` skill's moment list. Screenshots/reports go under
`playtest-artifacts/` (gitignored).

## The real-input smoke test

Everything above goes through the Director seam by design; one thin check
keeps the *actual* input plugin honest:

```sh
node tools/web_input_check.mjs http://localhost:8080/
```

Builds **without** autoplay only (the autoplay menu driver would defeat
it): synthetic browser keyboard events drive menu → first pitch, watched
via the always-on wasm beacon breadcrumbs (`game::autoplay::WebBeaconPlugin`).
