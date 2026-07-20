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

`cargo test` runs the unit tests (rules/variant/input/theme, in the lib target) plus headless e2e tests — a scripted 1-inning game to GAME OVER (`tests/e2e_full_game.rs`), staged rule scenarios for HBP/steals/double plays/hit-and-run/dropped third (`tests/e2e_advanced_rules.rs`), and a CPU-driven half-inning (`tests/e2e_cpu.rs`), all sharing the harness in `tests/common/mod.rs` (windowless boot, 240 Hz virtual time, a `DriveGame` schedule that writes `Intents` after `PreUpdate`). Run them after touching flow/rules/menu/input/ai. The crate has both a lib target (`src/lib.rs`, exposes `game` for tests) and the bin. On the menu, **I** cycles game length (1/3/6/9 innings).

For the release web build, use `--profile wasm-release` (size-optimized) and adjust the wasm-bindgen input path to `target/wasm32-unknown-unknown/wasm-release/`.

The `/run-web` skill packages the web build-and-serve workflow.

## Architecture

Bevy 0.15 ECS app with Rapier 3D physics. `src/main.rs` builds the `App` from `DefaultPlugins` + `RapierPhysicsPlugin` + `GamePlugin`.

`src/game/mod.rs` is the hub: it defines the `GameState` state machine (`MainMenu → Playing → Paused → GameOver`; gameplay systems use `.run_if(in_state(GameState::Playing))`), the `ScoreBoard` resource (innings/balls/strikes/outs shared across systems), and registers the sub-plugins in dependency order (`InputPlugin`, `MenuPlugin`, `FieldPlugin`, `BallPlugin`, `PlayerPlugin`, `AnimationPlugin`, `FlowPlugin`, `FxPlugin`, `FieldingPlugin`, `RunnerPlugin`, `CameraPlugin`, `UiPlugin`).

Game variants are data, not code: `variant.rs` defines `Ruleset` (count thresholds, innings, peg-outs) and `FieldSpec` (base positions, pitch distance, fair wedge, fence, fielder spots, scenery, duel + broadcast camera framing) as resources the menu writes when a game starts. The pure rules in `rules.rs` (unit-tested, no ECS) take them as parameters; `field.rs`/`player.rs`/`ui.rs`/`camera.rs` spawn from them. Home plate is at the world origin with +Z toward the field in every variant. To add a variant, add a `VariantId` arm — don't hardcode baseball facts in systems.

Advanced rules are deterministic (no RNG anywhere in `rules.rs`), keyed off data the engine already computes: tag-ups/sac flies off the fly's `deep` flag, double plays off base state and outs remaining, hit-by-pitch off the plate-crossing point, dropped third strike and steal outcomes off the pitch kind (curveball = in the dirt; fastball = catcher's throw wins), hit-and-run off the windup-held steal flag, and a nine-slot `BattingOrder` per team. The CPU offense calls steals too (one hash-noise decision per windup in `ai.rs`, held through the delivery). Pickoffs are deliberately absent: the analytic model has no leadoffs — runners sit on the bag until the pitch — so there is nothing to pick off.

Presentation is equally data-driven: `theme.rs` defines `Theme` (UI palette, per-team `PlayerTemplate`s, ball styling) with built-ins behind `ThemeId`, cycled on the menu with T. UI reads `Res<Theme>`; `flow.rs` emits `BannerTone`s and never colours. Players are multi-part rigs recoloured to the fielding/batting team on scoreboard changes.

All rig motion flows through `animation.rs`: systems insert a named `Playing` clip (`AnimClip`) or write a `MoveIntent` — never rotate rig parts or step transforms directly. The clip sampler is the seam for a future `AnimationGraph` backend, and `MoveIntent` is the seam for future player-controlled fielding (CPU choreography writes the same component a controller would). Ball-in-play outcomes are resolved **live**, not at contact: contact settles only what physics settles (a home run over the fence via `rules::classify_contact`); `fielding.rs` runs a real chase (the assigned fielder re-plans its intercept from the live ball each frame) and reports physical milestones as `flow::LiveBallEvent`s (caught / landed / gathered); `flow::resolve_live_play` turns those into the call through pure runner-vs-throw race functions in `rules.rs` (`resolve_catch`, `resolve_gathered`). `fx.rs`, `fielding.rs`, and `runner.rs` still never mutate `ScoreBoard` or `Bases` — they report or mirror; only flow applies rules. First base is at world −X (the behind-home camera renders −X on screen-right), and aim.x is negated in the pitch/hit mappings to match.

**wasm/WebGL2 UI gotcha:** a UI element that is fully transparent (alpha 0, or a bare container root with no renderable component) when first extracted is never rendered again, even after its colours change or children are added — and UI roots spawned mid-`Playing` don't render at all. Keep every element's alpha nonzero (see `ui::hidden_tint`), give container roots a `BackgroundColor`, and show/hide by mutating children of roots that were painted at spawn.

Cross-module communication is event-driven: `ball.rs` defines `PitchEvent` / `HitEvent`, which player and UI systems consume rather than touching ball entities directly. Physics constants use real-world SI units (official MLB ball: 0.037 m radius, 0.148 kg) with a custom drag force applied per physics tick. The ball ignores player capsules via collision groups (`BALL_GROUP`/`PLAYER_GROUP`) — outcomes are resolved analytically at contact, and a pitch glancing off the batter's collider would corrupt the called count.

## Dual-target constraints

- The crate builds for native and `wasm32-unknown-unknown`. Target-specific deps live in `Cargo.toml` `[target.'cfg(...)']` sections (SIMD Rapier is native-only; `wasm-bindgen`/`getrandom` are wasm-only).
- `.cargo/config.toml` sets `getrandom_backend="wasm_js"` rustflags for the wasm target — don't remove it; getrandom ≥ 0.3 fails to compile on wasm without it.
- CI (`.github/workflows/pages.yml`) deploys the `web/` directory to GitHub Pages on every push to `main`; it derives the wasm-bindgen version from `Cargo.lock`, so keeping the lockfile committed is load-bearing.
- After changing physics or rendering code, verify on **both** targets: `cargo check` and `cargo check --target wasm32-unknown-unknown`.
