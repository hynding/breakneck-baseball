# CLAUDE.md

Guidance for Claude Code in this repo. The full long-form architecture narrative lives in
`docs/agent/ARCHITECTURE-FULL.md`; domain detail loads on demand via the skills listed below.

## Toolchain

**macOS (the maintainer's machine):** Rust comes from Homebrew's rustup and is **not on the default
PATH**. Prefix commands with:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"
```

**Claude Code cloud sessions (Linux):** cargo is already on PATH, and `.claude/hooks/session-start.sh`
provisions the container: it installs Bevy's system libraries, the wasm target and the matching
`wasm-bindgen`, sets line-tables-only debug info, and warms `target/` in the background. Progress
goes to `target/.session-warm.log`. A `cargo` command run meanwhile waits on the build lock and then
reuses the result. A cold warm-up takes ~15–20 min on the 4-core container, so start with work that
doesn't need cargo.

`wasm-bindgen-cli` must exactly match the `wasm-bindgen` version in `Cargo.lock` (currently 0.2.127).
If `cargo update` bumps it, reinstall with `cargo binstall wasm-bindgen-cli --version <new-version> -y`
(binstall = prebuilt, seconds; avoid plain `cargo install`). The cloud hook re-matches it each session.

## Commands

```sh
cargo check                          # fast compile check (~45 s cold, seconds warm)
cargo run                            # native desktop build
cargo run --features dev             # faster iteration: links Bevy as a dylib + .glb hot-reload
cargo run --features "dev debug"     # + F1 in-game debug panel
cargo test                           # unit tests + headless e2e (run after flow/rules/menu/input/ai changes)
cargo test --lib                     # unit tests only — the fast inner loop
cargo test --test e2e matrix::       # one e2e suite (every suite is a module of the one tests/e2e/ binary)
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
- `meta/` — shell: menus, persistence, tooling (`settings/`, `menu.rs`, `input.rs`, `touch.rs`, `subs.rs`, plus debug-gated modules)

**The public API is the facade**: `game::<module>` is the canonical import path. A new module must be
declared in its layer's `mod.rs` *and* re-exported from `src/game/mod.rs` (`pub use self::core::rules;`
style). The `core` layer name collides with the `core` crate — always write `self::core::…` /
`crate::game::core::…`, never a bare leading `core::`. Big files split into same-named subdirectories
whose `mod.rs` re-exports the split, so item paths (`rules::resolve_thrown`) never change.

## Invariants

Violating any of these breaks the build, breaks wasm, or corrupts gameplay state.

- Spawn-at-game-start systems key on the `game_start()` transition schedule, never `OnEnter(Playing)` — otherwise they re-run on every unpause (`src/game/mod.rs`).
- wasm UI: an element that is alpha-0 at first extract never renders again; container roots need a `BackgroundColor`; UI roots spawned mid-`Playing` don't render — show/hide by mutating children of roots painted at spawn (`ui::hidden_tint`, `src/game/present/ui/`). Hidden chrome also toggles `Visibility` (spawning `Hidden` is fine on 0.17 wasm) so the keep-alive tint never ghosts over a dark sky.
- wasm UI: never tick a per-frame `ResMut` (Timer resource) in a system that also holds `&mut` queries on rendered UI — the queried entities stop being extracted on WebGL2; hold a fade *deadline* instead (`BannerFadeAt` in `src/game/present/ui/banner.rs`, wasm-ui-and-present skill).
- `model_assets.rs` and `src/game/models/` never move from `src/game/` top level — `embedded_asset!` derives both the `include_bytes!` path and the `embedded://` asset path from the file's own location (`src/game/model_assets.rs`).
- No RNG anywhere in `src/game/core/rules/` — advanced rules are deterministic, keyed off data the engine already computes (guard: `rules::tests::rules_sources_draw_no_randomness`).
- `fx`, `fielding`, and `runner` never mutate `ScoreBoard` or `Bases` — they report or mirror; only `flow` applies rules (`src/game/sim/flow/`).
- Any writer of `Time<Virtual>` `relative_speed` must compose with `juice::BaseSpeed`, never assume 1.0 (`src/game/present/juice.rs`; guard: `juice::tests::watchdog_restores_to_base_speed_not_one`).
- Keep the `bevy` `wav` feature in `Cargo.toml` — procedural audio synthesizes in-memory WAVs and needs bevy_audio's decoder.
- Keep `getrandom_backend="wasm_js"` rustflags in `.cargo/config.toml` — getrandom ≥ 0.3 fails to compile on wasm without it.
- Unit tests live in a sibling `<name>.test.rs`, pulled in by the source file's last item:
  `#[cfg(test)]` + `#[path = "<name>.test.rs"]` + `mod tests;` (for a `mod.rs`, the sibling is
  `mod.test.rs`). It is still a *child module*, so `use super::*;` reaches private items exactly
  as an inline `mod tests` did — do not move unit tests to `tests/`, whose files are separate
  crates that see only the public API and would force `pub` on internals. A `<name>/` directory
  in this repo means "split into production submodules", which is why tests get a sibling file
  rather than `<name>/tests.rs`.
- `tests/e2e/` suites inject input from the `DriveGame` schedule, never from the test body — the input plugin's `PreUpdate` clear wipes presses made outside it (`tests/common/mod.rs`). Exemption: raw *window events* (`TouchInput`) are double-buffered and survive to `InputSystem`, so `tests/e2e/touch_pipeline.rs` sends them from the test body; the rule is about `ButtonInput` presses.
- Every `tests/e2e/` suite is a module of one test binary, so they share a process: never `std::env::set_var`/`remove_var` or otherwise change process-global state there. A test that must gets its own binary (`tests/e2e_settings.rs`; `tests/balance_sim.rs` stands alone because it pins Bevy's process-global task pools).
- Scripted e2e batted balls must be sprayed at a *set* fielder's spot — the steal window means the defense is back in position before every pitch (`tests/common/mod.rs` helpers).
- Roster names are A–Z only — jersey lettering uses a built-in 5×7 bitmap font (`src/game/present/jersey.rs`; guards: `roster::tests::jersey_names_fit_the_procedural_font`, `tests/e2e/appearance_contract.rs`).
- Never hand-export the player model from the Blender GUI — `tools/export_glb.py` pins the settings the runtime loader and `tests/e2e/model_contract.rs` depend on; always run the build/export script pair.
- All rig motion flows through `src/game/present/animation/` (`Playing`/`MoveIntent`) — never rotate rig parts or step rig transforms directly.
- The ball ignores player capsules via collision groups (`BALL_GROUP`/`PLAYER_GROUP`) — a pitch glancing off the batter's collider would corrupt the called count (`src/game/sim/ball.rs`).
- The CPU always bats Classic regardless of settings (`batting::style_for`) and `tests/balance_sim.rs` is the arbiter of the offensive economy — retune windows/multipliers/spread there, not by feel.
- After physics or rendering changes, verify **both** targets: `cargo check` and `cargo check --target wasm32-unknown-unknown`.
- Real-world baseball facts come from `docs/BASEBALL.md` (with sources) — check it before modeling something physical, extend it when short, cite it in comments ("per docs/BASEBALL.md").
- Tests touching `BREAKNECK_SETTINGS_PATH` serialize through `ENV_LOCK` — the settings module's `set_var`/`remove_var` calls are the crate's only `unsafe` (`src/game/meta/settings/`).
- The Coach (`game::coach`) observes and never mutates gameplay state; the Director's `DriveGame` schedule (`game::director`) is the only synthetic-input seam — new *gameplay* control mechanisms must route through `Intents`/`SwingCommands` so scripts, tests, and autoplay cover them automatically. Shell chrome (pause, menus, settings, quit) reads devices directly, as Esc/P/Start always have — the rule covers what plays baseball, not what drives screens.
- Keep `Cargo.lock` committed — CI derives the wasm-bindgen version from it (`.github/workflows/pages.yml`).
- `autoplay::AutoplayPlugin` registers in `src/main.rs`, never in `GamePlugin` — the lib is every test harness's plugin, and the self-driver's menu presses + Startup `Director` insert hijack a harness's own setup (`cargo test --features autoplay` must behave exactly like plain `cargo test`).

## Skills

Loaded on trigger from `.claude/skills/`; each SKILL.md says when.

- `gameplay-rules` — flow phases, steal window/pickoff, live-play resolution, batting spine, balance economy. Load before touching `src/game/core/rules/`, `src/game/sim/`, or `src/game/meta/input.rs`.
- `rigs-and-animation` — AnimClip API, CLIP_TABLE/model contract, Blender pipeline, jerseys. Load before touching `src/game/present/animation/`, `src/game/present/player/`, `player.glb`, `tools/*.py`, `jersey.rs`.
- `wasm-ui-and-present` — the wasm UI gotcha in full, Theme/BannerTone, cameras, juice, settings persistence. Load before touching `src/game/present/ui/`, `camera/`, `menu.rs`, `subs.rs`, `settings/`.
- `verify-change` — routes what-you-touched to the checks/tests to run, with durations. Load before committing or claiming "done".
- `tune-balance` — the dial → `balance_sim` → bands loop; bands in its `reference/bands.md`. Load before touching any `Ruleset` window/multiplier/spread.
- `playtest-review` — moment list + rubric producing a ranked TODO.md work queue. Load for "review the game" / "what should I work on next".
- `production-readiness` — web-first ship audit; checklist in its `reference/checklist.md`. Load before a release.
- `bevy-perf` — Bevy performance practice (ECS, change detection, Rapier, wasm limits; written against 0.15, the crate is on 0.17). Load for "slow"/"stutter"/"optimize".
- `coach` — the always-on expectation checker: what it checks, tolerances, reading `CoachReport`, adding a check. Load when players misbehave or before touching `sim/fielding.rs`, `sim/runner.rs`, `sim/flow/`.
- `auto-playtest` — the Director, `.ron` scripts, the mode matrix, and self-driving native/wasm runs. Load for "playtest", "verify 2 player", "test PCI/Meter", or when adding an input device or batting adapter.
- `run-web` — build, serve, and verify the browser build.
- `rust-skills` — generic Rust guidelines (265 rules, one file each under `.agents/skills/rust-skills/rules/`) plus where this crate departs from them. Load for reviews, refactors, `unsafe`, or hot-path work — not for routine edits that follow the surrounding code.

Long-form narrative (how every subsystem fits together): `docs/agent/ARCHITECTURE-FULL.md`.
The user's work queue is `TODO.md` — its "Start here" table lists what an agent can close alone,
each with a done-when check; completed items move to `TADA.md`.

## Agent workflow

- Cargo builds serialize on the `target/` lock: a second build started meanwhile only queues. A
  `cargo test` releases the lock once its binaries start running, so the next compile can overlap a
  long test run. Overlap non-cargo work (reading, docs, browser checks) freely.
- Multi-session work (e.g. TODO 29's Bevy migrations) goes in a git worktree so the main checkout stays
  usable. A worktree gets its own `target/` (a cold build), unless you point `CARGO_TARGET_DIR` at the
  main checkout's `target/` to reuse compiled dependencies; its builds then share that lock.
- Ground claims in the measuring tools rather than prose: `balance_sim` for the economy, the Coach for
  player behaviour, `model_contract` for the rig, and the autoplay report's `game`/`frames` summary for
  run-to-run comparisons (auto-playtest skill).

## Dual-target constraints

- The crate builds for native and `wasm32-unknown-unknown`. Target-specific deps live in `Cargo.toml` `[target.'cfg(...)']` sections (SIMD Rapier is native-only; `wasm-bindgen`/`getrandom` are wasm-only).
- CI (`.github/workflows/pages.yml`) deploys `web/` to GitHub Pages on every push to `main`.
- The crate has a lib target (`src/lib.rs`, exposes `game` for tests) and the bin — keep both compiling.
