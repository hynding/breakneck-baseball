# Breakneck Baseball

A fast, arcade-flavored 3-D baseball game built in Rust with **Bevy** (ECS + wgpu
rendering) and **Rapier** (physics). Runs natively on desktop and in the browser
via WebAssembly.

**▶ Play it now:** <https://hynding.github.io/breakneck-baseball/>

## Features

- Full nine-inning (or 1/3/5-inning) games: 1P vs CPU, or 2P on one keyboard / two gamepads
- A five-pitch arsenal with Magnus-effect ball flight, selected by aim at release
- The steal duel: stretch a lead for a guaranteed jump — at pickoff risk
- Advanced rules: tag-ups, double plays, dropped third strike, HBP, caught pops
- Four duel-view camera framings plus a broadcast chase camera
- Procedurally synthesized audio (no sound assets), themable teams and fields
- Substitution board, persistent settings, pause — all working in the browser build

## Controls

### Main menu

| Key | Action |
|---|---|
| `1` / `2` | Choose one-player / two-player mode |
| `F` | Cycle field variant |
| `T` | Cycle theme |
| `I` | Cycle game length (innings) |
| `S` | Open settings (batting styles, volume) |
| `Enter` / `Space` | Start game |

### In game

| Input | P1 keyboard | P2 keyboard | Gamepad |
|---|---|---|---|
| Aim | `WASD` | Arrow keys | Left stick / D-pad |
| Action (pitch / swing) | `Space` | `Right Ctrl` | South button (A / ✕) |
| Camera framing | `V` (cycles four duel views) | — | — |
| Pause / substitution board | `Esc` | `Esc` | Start |

**Pitching** — hold an aim direction and press action to release. The dominant
aim axis picks the pitch: up = fastball, down = curveball, left = slider,
right = sinker, neutral = changeup. Aim keeps steering location too, so
aiming high *means* throwing the heater upstairs.

**Batting** — time the swing with action; aim pulls the ball (pull / center /
opposite field).

**The steal window** — before the wind-up, the offense can hold aim toward the
next base to stretch the lead runner off the bag: held through the window, it
buys a guaranteed jump on the pitch. The defense can answer with a pickoff
throw (action during the window) — get caught stretched and you're picked off.

## Building

Requires Rust (edition 2024, `rust-version` in `Cargo.toml`).

### Desktop (native)

```sh
cargo run                            # standard build
cargo run --features dev             # faster iteration (dylib + asset hot-reload)
cargo run --features "dev debug"     # + F1 in-game debug panel
cargo test                           # unit tests + headless e2e
```

> **Linux prerequisite:** `libasound2-dev` and `libudev-dev` for Bevy's
> audio/input backends.

### Web (WASM)

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli        # must match the wasm-bindgen version in Cargo.lock

cargo build --profile wasm-release --target wasm32-unknown-unknown
wasm-bindgen --out-dir web/out --target web \
  target/wasm32-unknown-unknown/wasm-release/breakneck-baseball.wasm

python3 -m http.server --directory web 8080
# open http://localhost:8080
```

Every push to `main` deploys the web build to GitHub Pages via
`.github/workflows/pages.yml` (tests gate the deploy; the wasm is `wasm-opt`
size-optimized in CI).

## Architecture

`src/game/` is split into four layers, registered by `GamePlugin`:

```
src/game/
├── core/       — pure rules & data, no Bevy systems (rules/, variant, roster, theme)
├── sim/        — gameplay decisions (flow/, fielding, runner, ball, batting, ai)
├── present/    — everything seen/heard (field/, camera/, player/, animation/, ui/, fx/, audio)
└── meta/       — shell: menus, input, settings, substitutions, debug tooling
```

The full architecture narrative lives in `docs/agent/ARCHITECTURE-FULL.md`;
real-world baseball reference data (with sources) in `docs/BASEBALL.md`.

## Dependencies

| Crate | Role |
|---|---|
| `bevy` 0.15 | Game engine (ECS, windowing, wgpu rendering, audio, input) |
| `bevy_rapier3d` 0.28 | 3-D rigid-body physics (pitches, hits, caroms) |

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
