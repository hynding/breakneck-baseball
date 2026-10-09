---
name: verify-change
description: Use before committing, at the end of any task that edited src/ or tools/, or whenever asking "did I break anything". Routes from what was touched to exactly which checks and tests to run, with commands and expected durations. Also use when a test fails and you need to know whether it guards the thing you changed.
---

# Verify a Change

Map what you touched to what you must run. On macOS, prefix with
`export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"` (cloud sessions already
have cargo on PATH; see CLAUDE.md → Toolchain).

## Test binaries

| Binary | What | Why it stands alone |
|---|---|---|
| lib (`cargo test --lib`) | unit tests in sibling `<name>.test.rs` files | — |
| `tests/e2e/` (`--test e2e`) | every headless e2e suite + the model/appearance contracts + the ambiguity audit, one module each | one link of Bevy instead of 30+ |
| `tests/balance_sim.rs` | the offensive-economy arbiter | pins single-threaded Bevy task pools, which are process-global |
| `tests/e2e_settings.rs` | settings-store round trip | sets `BREAKNECK_SETTINGS_PATH`, which is process-global |

Filter to one suite with its module path: `cargo test --test e2e matrix::`, or a single test with
`cargo test --test e2e matrix::cpu_vs_cpu_ignores_style`. Several suites in one run go after
`--`: `cargo test --test e2e -- matrix:: cpu:: coach::` (a test matching any filter runs).

## Routing table

| You touched | Run | Why |
|---|---|---|
| Anything in `src/` | `cargo check` (seconds warm, ~45 s cold) | baseline compile |
| Physics, rendering, or any `present/` code | `cargo check --target wasm32-unknown-unknown` **as well** | the crate ships dual-target; wasm-only breakage is common (getrandom, SIMD Rapier, WebGL2) |
| `sim/flow/`, `core/rules/`, `meta/menu.rs`, `meta/input.rs`, `sim/ai.rs` | inner loop: `cargo test --lib` + the guarding suites below; before commit: full `cargo test` | the headless e2e suite scripts full games through these systems |
| Only pure rules logic (quick loop) | `cargo test --lib` (fast) then full `cargo test` before commit | unit tests for rules/variant/input/theme/roster/jersey live in the lib target |
| `model_assets.rs`, `tools/*.py`, `assets-src/`, `player.glb`, `AnimClip`/`CLIP_TABLE` | `cargo test --test e2e model_contract::` (+ `gltf_` suites) | pins clip/material/bone names + tri/bone/size budgets against the .glb |
| `data/players.ron`, `meta/appearance.rs`, roster/jersey | `cargo test --test e2e appearance_contract::` + `identity::` + `dressing::` | shipped player definitions + identity plumbing |
| Any `Ruleset` window/multiplier/spread (`perfect_ms`, `solid_ms`, `foul_ms`, `exit_*`, `pull_yaw_per_ms`, `cpu_timing_spread_ms`) or `sim/ai.rs` decision noise | `cargo test --test balance_sim` (~1.5 min, N=40) | the arbiter of the offensive economy — see the `tune-balance` skill |
| New/changed system ordering (any `add_systems`) | `cargo test --test e2e ambiguity_audit::` | schedule-ambiguity gate |
| UI (`present/ui/`, `subs.rs`, `settings/screen.rs`, menu) | web build + browser check via `/run-web` | the wasm UI gotcha only reproduces in the browser |
| `meta/settings/` persistence | `cargo test --lib` (settings tests serialize via `ENV_LOCK`) + `cargo test --test e2e_settings` | the env-var seam is easy to break |
| `Cargo.toml` / `Cargo.lock` / `.cargo/config.toml` | both-target `cargo check`; if `wasm-bindgen` bumped, reinstall CLI to match (`cargo binstall wasm-bindgen-cli --version <lock version> -y`) | CI derives the bindgen version from the committed lockfile |
| `.github/workflows/pages.yml` or `web/` | full wasm-release build: `cargo build --profile wasm-release --target wasm32-unknown-unknown` + bindgen + serve | Pages deploys `web/` on every push to main |

Multiple rows can match one change — run the union. When in doubt, `cargo test` is the
comprehensive answer; it covers unit + e2e + balance.

Measured 2026-10-09 in a 4-core cloud container (expect a fast Mac to be ~3× quicker): full
`cargo test` 21.5 min run time (`balance_sim` 9 min, the `e2e` binary 12.5 min with its suites in
parallel; the slowest suites are `fielder_spots`, `batter_runs`, `cpu_timing`, `matrix`). After a
`src/` edit, the test build takes ~30 s. A cold build is ~16 min.

## Guarding suites by area (`cargo test --test e2e <module>::`)

| Area | Modules |
|---|---|
| Pitch/swing timing, batting adapters | `swing_startup`, `contact_timing`, `cpu_timing`, `batting_styles`, `call_beat`, `contact_stamp` |
| Live play, runners, fielders | `batter_runs`, `baserunning_breaks`, `fielder_spots`, `catcher_crouch`, `advanced_rules`, `passive_walks` |
| Whole games, CPU, control matrix | `full_game`, `cpu`, `matrix`, `coach`, `scenarios` |
| Flow pacing, walk-up, home run | `walkup`, `home_run_moment`, `call_beat` |
| Cameras | `camera_views`, `base_cam` |
| Menu, pause, input devices | `pause_subs`, `touch_pipeline` (+ `e2e_settings` binary) |
| Rigs, models, jerseys | `gltf_model`, `gltf_rig`, `identity`, `dressing`, `model_contract`, `appearance_contract`, `creator` (`--features debug`) |
| Change detection / repaint hygiene | `change_detection` |
| System ordering | `ambiguity_audit` |

## Before every commit

1. `cargo check` on native, plus wasm if any matched row says so.
2. The matched test commands above. Read the actual output: "Finished" ≠ "passed"; look for
   `test result: ok` on every binary.
3. `cargo fmt --check` — the PostToolUse hook formats on write, but catch stragglers.
4. `cargo clippy --all-targets -- -D warnings` — CI denies warnings. The Stop hook runs clippy
   after any turn that changed `.rs` files and hands findings back, but don't rely on it alone.

**Running long commands.** In the main session a full `cargo test` can run in the background
(you're re-invoked when it exits) while you do other work. Two cargo *builds* in one `target/`
serialize on the build lock, so a "parallel" wasm check queues behind a test build. Once the
test binaries are running, cargo has released the lock and the next build can proceed.
Subagents should run cargo in the foreground and report the result.

## E2e harness rules (when writing/altering tests)

- Every e2e suite shares one process. Never `std::env::set_var`/`remove_var` from a
  `tests/e2e/` module, and never rely on process-global state another suite could change.
  A test that must mutate the environment gets its own binary, like `tests/e2e_settings.rs`.
- A new suite is a new module: add `tests/e2e/<name>.rs`, declare it in `tests/e2e/main.rs`,
  and reach the harness through `use crate::common;`.
- Inject input from the `DriveGame` schedule, never the test body — the input plugin's
  `PreUpdate` clear wipes presses made outside it (`tests/common/mod.rs` has `tap_key`/`start_game`).
- Spray scripted batted balls at a *set* fielder's spot — the steal window puts the defense
  back in position before every pitch.
- The live sim yields force outs, not turned twos — double-play relay math is pinned by
  `resolve_thrown` unit tests, don't try to stage it e2e.
- For situations (bases/count/inning), use `sim/scenario.rs`'s `apply_to_world`, not a
  hand-scripted inning.
- The harness inserts `JuiceDisabled` — never remove it; slow-mo corrupts scripted timing.
