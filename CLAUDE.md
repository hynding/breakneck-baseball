# CLAUDE.md

Guidance for Claude Code in this repo. The full long-form architecture narrative lives in
`docs/agent/ARCHITECTURE-FULL.md`; domain detail loads on demand via the skills listed below.

## Toolchain (this machine)

Rust is installed via Homebrew's rustup and is **not on the default PATH**. Prefix commands with:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"
```

`wasm-bindgen-cli` must exactly match the `wasm-bindgen` version in `Cargo.lock` (currently 0.2.126).
If `cargo update` bumps it, reinstall with `cargo binstall wasm-bindgen-cli --version <new-version> -y`
(binstall = prebuilt, seconds; avoid plain `cargo install`).

## Commands

```sh
cargo check                          # fast compile check (~45 s cold, seconds warm)
cargo run                            # native desktop build
cargo run --features dev             # faster iteration: links Bevy as a dylib + .glb hot-reload
cargo run --features "dev debug"     # + F1 in-game debug panel
cargo test                           # unit tests + headless e2e (run after flow/rules/menu/input/ai changes)
cargo build --target wasm32-unknown-unknown   # web build (debug)
wasm-bindgen --out-dir web/out --target web target/wasm32-unknown-unknown/debug/breakneck-baseball.wasm
python3 -m http.server --directory web 8080   # serve, then open http://localhost:8080

blender --background --python tools/build_player.py                       # (re)build assets-src/player.blend
blender --background assets-src/player.blend --python tools/export_glb.py # export -> src/game/models/player.glb
```

Release web build: `--profile wasm-release` (size-optimized); the wasm-bindgen input path becomes
`target/wasm32-unknown-unknown/wasm-release/`. The `/run-web` skill packages build-and-serve.
The Blender pair always runs in that order — never hand-export from the GUI (see Invariants).

## Layer map

`src/game/` has four layers, registered by `GamePlugin` in `src/game/mod.rs`
(which also owns `GameState`: `MainMenu → Playing ⇄ Paused → GameOver`, and `ScoreBoard`):

- `core/` — pure rules & data, no Bevy systems (`rules/`, `variant.rs`, `roster.rs`, `theme.rs`)
- `sim/` — gameplay systems that decide what happens (`flow/`, `fielding.rs`, `runner.rs`, `ball.rs`, `batting.rs`, `ai.rs`, `scenario.rs`)
- `present/` — everything seen/heard (`field/`, `camera/`, `player/`, `animation/`, `ui/`, `fx/`, `jersey.rs`, `audio.rs`, `juice.rs`)
- `meta/` — shell: menus, persistence, tooling (`settings/`, `menu.rs`, `input.rs`, `subs.rs`, plus debug-gated modules)

**The public API is the facade**: `game::<module>` is the canonical import path. A new module must be
declared in its layer's `mod.rs` *and* re-exported from `src/game/mod.rs` (`pub use self::core::rules;`
style). The `core` layer name collides with the `core` crate — always write `self::core::…` /
`crate::game::core::…`, never a bare leading `core::`. Big files split into same-named subdirectories
whose `mod.rs` re-exports the split, so item paths (`rules::resolve_thrown`) never change.

## Invariants

Violating any of these breaks the build, breaks wasm, or corrupts gameplay state.

- Spawn-at-game-start systems key on the `game_start()` transition schedule, never `OnEnter(Playing)` — otherwise they re-run on every unpause (`src/game/mod.rs`).
- wasm UI: an element that is alpha-0 at first extract never renders again; container roots need a `BackgroundColor`; UI roots spawned mid-`Playing` don't render — show/hide by mutating children of roots painted at spawn (`ui::hidden_tint`, `src/game/present/ui/`).
- `model_assets.rs` and `src/game/models/` never move from `src/game/` top level — `embedded_asset!` derives both the `include_bytes!` path and the `embedded://` asset path from the file's own location (`src/game/model_assets.rs`).
- No RNG anywhere in `src/game/core/rules/` — advanced rules are deterministic, keyed off data the engine already computes.
- `fx`, `fielding`, and `runner` never mutate `ScoreBoard` or `Bases` — they report or mirror; only `flow` applies rules (`src/game/sim/flow/`).
- Any writer of `Time<Virtual>` `relative_speed` must compose with `juice::BaseSpeed`, never assume 1.0 (`src/game/present/juice.rs`).
- Keep the `bevy` `wav` feature in `Cargo.toml` — procedural audio synthesizes in-memory WAVs and needs bevy_audio's decoder.
- Keep `getrandom_backend="wasm_js"` rustflags in `.cargo/config.toml` — getrandom ≥ 0.3 fails to compile on wasm without it.
- `tests/e2e_*` inject input from the `DriveGame` schedule, never from the test body — the input plugin's `PreUpdate` clear wipes presses made outside it (`tests/common/mod.rs`).
- Scripted e2e batted balls must be sprayed at a *set* fielder's spot — the steal window means the defense is back in position before every pitch (`tests/common/mod.rs` helpers).
- Roster names are A–Z only — jersey lettering uses a built-in 5×7 bitmap font (`src/game/present/jersey.rs`).
- Never hand-export the player model from the Blender GUI — `tools/export_glb.py` pins the settings the runtime loader and `tests/model_contract.rs` depend on; always run the build/export script pair.
- All rig motion flows through `src/game/present/animation/` (`Playing`/`MoveIntent`) — never rotate rig parts or step rig transforms directly.
- The ball ignores player capsules via collision groups (`BALL_GROUP`/`PLAYER_GROUP`) — a pitch glancing off the batter's collider would corrupt the called count (`src/game/sim/ball.rs`).
- The CPU always bats Classic regardless of settings (`batting::style_for`) and `tests/balance_sim.rs` is the arbiter of the offensive economy — retune windows/multipliers/spread there, not by feel.
- After physics or rendering changes, verify **both** targets: `cargo check` and `cargo check --target wasm32-unknown-unknown`.
- Real-world baseball facts come from `docs/BASEBALL.md` (with sources) — check it before modeling something physical, extend it when short, cite it in comments ("per docs/BASEBALL.md").
- Tests touching `BREAKNECK_SETTINGS_PATH` serialize through `ENV_LOCK` — the settings module's `set_var`/`remove_var` calls are the crate's only `unsafe` (`src/game/meta/settings/`).
- Keep `Cargo.lock` committed — CI derives the wasm-bindgen version from it (`.github/workflows/pages.yml`).

## Skills

Loaded on trigger from `.claude/skills/`; each SKILL.md says when.

- `gameplay-rules` — flow phases, steal window/pickoff, live-play resolution, batting spine, balance economy. Load before touching `src/game/core/rules/`, `src/game/sim/`, or `src/game/meta/input.rs`.
- `rigs-and-animation` — AnimClip API, CLIP_TABLE/model contract, Blender pipeline, jerseys. Load before touching `src/game/present/animation/`, `src/game/present/player/`, `player.glb`, `tools/*.py`, `jersey.rs`.
- `wasm-ui-and-present` — the wasm UI gotcha in full, Theme/BannerTone, cameras, juice, settings persistence. Load before touching `src/game/present/ui/`, `camera/`, `menu.rs`, `subs.rs`, `settings/`.
- `run-web` — build, serve, and verify the browser build.
- `rust-skills` — generic Rust guidelines (265 rules); use for any Rust authoring/review.

Long-form narrative (how every subsystem fits together): `docs/agent/ARCHITECTURE-FULL.md`.
The user's work queue is `TODO.md`; completed items move to `TADA.md`.

## Dual-target constraints

- The crate builds for native and `wasm32-unknown-unknown`. Target-specific deps live in `Cargo.toml` `[target.'cfg(...)']` sections (SIMD Rapier is native-only; `wasm-bindgen`/`getrandom` are wasm-only).
- CI (`.github/workflows/pages.yml`) deploys `web/` to GitHub Pages on every push to `main`.
- The crate has a lib target (`src/lib.rs`, exposes `game` for tests) and the bin — keep both compiling.
