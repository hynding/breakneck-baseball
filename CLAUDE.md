# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Toolchain (this machine)

Rust is installed via Homebrew's rustup and is **not on the default PATH**. Prefix commands with:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"
```

`wasm-bindgen-cli` must exactly match the `wasm-bindgen` version in `Cargo.lock` (currently 0.2.126). If `cargo update` bumps it, reinstall with `cargo binstall wasm-bindgen-cli --version <new-version> -y` (binstall = prebuilt, seconds; avoid plain `cargo install`).

## Commands

```sh
cargo check                          # fast compile check (~45 s cold, seconds warm)
cargo run                            # native desktop build
cargo run --features dev             # faster iteration: links Bevy as a dylib
cargo build --target wasm32-unknown-unknown   # web build (debug)
wasm-bindgen --out-dir web/out --target web target/wasm32-unknown-unknown/debug/breakneck-baseball.wasm
python3 -m http.server --directory web 8080   # serve, then open http://localhost:8080
```

There is no test suite yet. For the release web build, use `--profile wasm-release` (size-optimized) and adjust the wasm-bindgen input path to `target/wasm32-unknown-unknown/wasm-release/`.

The `/run-web` skill packages the web build-and-serve workflow.

## Architecture

Bevy 0.15 ECS app with Rapier 3D physics. `src/main.rs` builds the `App` from `DefaultPlugins` + `RapierPhysicsPlugin` + `GamePlugin`.

`src/game/mod.rs` is the hub: it defines the `GameState` state machine (`MainMenu → Playing → Paused → GameOver`; gameplay systems use `.run_if(in_state(GameState::Playing))`), the `ScoreBoard` resource (innings/balls/strikes/outs shared across systems), and registers the five sub-plugins in dependency order: `FieldPlugin`, `BallPlugin`, `PlayerPlugin`, `CameraPlugin`, `UiPlugin`.

Cross-module communication is event-driven: `ball.rs` defines `PitchEvent` / `HitEvent`, which player and UI systems consume rather than touching ball entities directly. Physics constants use real-world SI units (official MLB ball: 0.037 m radius, 0.148 kg) with a custom drag force applied per physics tick.

## Dual-target constraints

- The crate builds for native and `wasm32-unknown-unknown`. Target-specific deps live in `Cargo.toml` `[target.'cfg(...)']` sections (SIMD Rapier is native-only; `wasm-bindgen`/`getrandom` are wasm-only).
- `.cargo/config.toml` sets `getrandom_backend="wasm_js"` rustflags for the wasm target — don't remove it; getrandom ≥ 0.3 fails to compile on wasm without it.
- CI (`.github/workflows/pages.yml`) deploys the `web/` directory to GitHub Pages on every push to `main`; it derives the wasm-bindgen version from `Cargo.lock`, so keeping the lockfile committed is load-bearing.
- After changing physics or rendering code, verify on **both** targets: `cargo check` and `cargo check --target wasm32-unknown-unknown`.
