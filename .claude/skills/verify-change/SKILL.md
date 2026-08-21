---
name: verify-change
description: Use before committing, at the end of any task that edited src/ or tools/, or whenever asking "did I break anything". Routes from what was touched to exactly which checks and tests to run, with commands and expected durations. Also use when a test fails and you need to know whether it guards the thing you changed.
---

# Verify a Change

Map what you touched to what you must run. Always prefix:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"
```

## Routing table

| You touched | Run | Why |
|---|---|---|
| Anything in `src/` | `cargo check` (seconds warm, ~45 s cold) | baseline compile |
| Physics, rendering, or any `present/` code | `cargo check --target wasm32-unknown-unknown` **as well** | the crate ships dual-target; wasm-only breakage is common (getrandom, SIMD Rapier, WebGL2) |
| `sim/flow/`, `core/rules/`, `meta/menu.rs`, `meta/input.rs`, `sim/ai.rs` | `cargo test` (~7 min warm, measured 2026-08-20: unit + all e2e + balance sim) | the headless e2e suite scripts full games through these systems |
| Only pure rules logic (quick loop) | `cargo test --lib` (fast) then full `cargo test` before commit | unit tests for rules/variant/input/theme/roster/jersey live in the lib target |
| `model_assets.rs`, `tools/*.py`, `assets-src/`, `player.glb`, `AnimClip`/`CLIP_TABLE` | `cargo test --test model_contract` | pins clip/material/bone names + tri/bone/size budgets against the .glb |
| Any `Ruleset` window/multiplier/spread (`perfect_ms`, `solid_ms`, `foul_ms`, `exit_*`, `pull_yaw_per_ms`, `cpu_timing_spread_ms`) or `sim/ai.rs` decision noise | `cargo test --test balance_sim` (~1.5 min, N=40) | the arbiter of the offensive economy — see the `tune-balance` skill |
| UI (`present/ui/`, `subs.rs`, `settings/screen.rs`, menu) | web build + browser check via `/run-web` | the wasm UI gotcha only reproduces in the browser |
| `meta/settings/` persistence | `cargo test --lib` (settings tests serialize via `ENV_LOCK`) | the env-var seam is easy to break |
| `Cargo.toml` / `Cargo.lock` / `.cargo/config.toml` | both-target `cargo check`; if `wasm-bindgen` bumped, reinstall CLI to match (`cargo binstall wasm-bindgen-cli --version <lock version> -y`) | CI derives the bindgen version from the committed lockfile |
| `.github/workflows/pages.yml` or `web/` | full wasm-release build: `cargo build --profile wasm-release --target wasm32-unknown-unknown` + bindgen + serve | Pages deploys `web/` on every push to main |

Multiple rows can match one change — run the union. When in doubt, `cargo test` is the
comprehensive answer; it covers unit + e2e + balance.

## Before every commit

1. `cargo check` on native, plus wasm if any matched row says so.
2. The matched test commands above, **foregrounded** (backgrounded cargo tests stall subagents).
3. `cargo fmt --check` — the PostToolUse hook formats on write, but catch stragglers.
4. Read the actual output. "Finished" ≠ "passed"; look for `test result: ok`.

## E2e harness rules (when writing/altering tests)

- Inject input from the `DriveGame` schedule, never the test body — the input plugin's
  `PreUpdate` clear wipes presses made outside it (`tests/common/mod.rs` has `tap_key`/`start_game`).
- Spray scripted batted balls at a *set* fielder's spot — the steal window puts the defense
  back in position before every pitch.
- The live sim yields force outs, not turned twos — double-play relay math is pinned by
  `resolve_thrown` unit tests, don't try to stage it e2e.
- For situations (bases/count/inning), use `sim/scenario.rs`'s `apply_to_world`, not a
  hand-scripted inning.
- The harness inserts `JuiceDisabled` — never remove it; slow-mo corrupts scripted timing.
