# Debug Mode — Design

**Date:** 2026-08-05
**Status:** Approved

A debug mode for tuning, reproducing, and understanding gameplay: live constant
tuning with a copy-ready export, instant scenario setup shared with the
headless test harness, live state inspection, and 3D gizmo overlays — all
behind a dedicated cargo feature so shipped builds carry none of it.

## Goals

1. **Live tuning** — adjust gameplay-feel constants in-game and feel the
   result immediately, with a paste-ready path back into `variant.rs`.
2. **Scenario setup** — jump directly to any game situation (runners, count,
   outs, inning, score, next pitch) to reproduce and test rule interactions.
3. **State inspection** — read the live internals (`Play` phase, pending
   call, swing timing, fielder assignments) that explain why a play resolved
   the way it did.
4. **Visual gizmos** — draw the invisible data: true strike zone, predicted
   trajectory, intercept plans, PCI radius, collider wireframes.
5. **Better tests** — scenario definitions are a shared library, so a bug
   found while playing becomes a named regression test with no new setup code.

Non-goals: cheat codes in shipped builds, replay recording, network debugging.

## 1. Structure & gating

- New cargo feature **`debug`**, independent of `dev` (composable:
  `cargo run --features "dev debug"`).
- The feature enables two optional dependencies: `bevy_egui` and
  `bevy-inspector-egui`, at versions matched to Bevy 0.15 (`bevy_egui`
  0.31.x, `bevy-inspector-egui` 0.28.x — exact pins verified during
  implementation). egui renders itself, so the wasm/WebGL2 bevy_ui alpha-zero
  gotcha does not apply to the panel.
- New module **`src/game/debug.rs`** defines `DebugPlugin`, registered from
  `mod.rs` under `#[cfg(feature = "debug")]`. Builds without the feature
  compile zero debug code.
- **`src/game/scenario.rs` is always compiled** (lib target, no feature
  gate): the headless tests consume it and run without the feature. The
  `debug` feature gates only UI, gizmos, and time controls.
- **F1** toggles the panel. A `DebugState` resource holds panel visibility
  and per-gizmo toggles. The panel works in `Playing` and `Paused` alike.
- CI adds `cargo check --features debug` for native **and**
  `wasm32-unknown-unknown`, so the feature can't rot on either target.

## 2. Tuning surface

Tunable constants are **promoted into `Ruleset`** — already "the data that
makes a game of baseball *this* game" — not into a parallel debug-only
resource that could drift. They become variant data with defaults equal to
today's consts, nested in a `pace` sub-struct to keep `Ruleset` readable:

| From | Promoted constants |
|---|---|
| `rules.rs` | `PITCH_SPEED`, `RUNNER_SPEED`, `FIELDER_SPEED`, `REACTION`, `THROW_FLIGHT_SPEED`, `THROW_TRANSFER`, `RELAY_TRANSFER`, `HIT_AND_RUN_JUMP`, `STRETCH_GRACE`, `RUNNER_MARGIN` |
| `flow.rs` | `RESULT_SECS`, `PICKOFF_COOLDOWN_SECS`, the ~0.6 s hold auto-throw delay |

Rules functions that don't already take `&Ruleset` gain it; purity and
determinism are untouched, and `tests/balance_sim.rs` automatically remains
the arbiter for every promoted value. Staying put: zone geometry
(`ZONE_*`) and ball mass/radius (regulation facts per docs/BASEBALL.md),
and camera framing (already variant data in `FieldSpec`).

**Tune tab:** derive `Reflect` on `Ruleset` and `FieldSpec` and hand both to
`bevy-inspector-egui` — every field becomes a drag-slider, and any future
field added to either struct appears in the UI with zero UI code.

**Dump diff:** a button prints only the fields differing from the active
variant's defaults as a paste-ready Rust literal labeled with the
`VariantId`, to stdout and the clipboard (via egui). Round-trip: paste into
`variant.rs`, re-run `balance_sim`.

## 3. Scenario library (shared with tests)

`src/game/scenario.rs`, always compiled:

```rust
pub struct Scenario {
    pub bases: Vec<bool>,        // occupancy per base, sized to FieldSpec::base_count()
    pub outs: u32,
    pub balls: u32,
    pub strikes: u32,
    pub inning: u32,
    pub top: bool,
    pub score: (u32, u32),       // (home, away)
    pub batter_slot: Option<usize>,
    pub next_cpu_pitch: Option<PitchKind>,
}
```

Named presets in a `PRESETS` table, at minimum:

- "Bases loaded, 2 out, full count"
- "DP setup: R1, 0 out"
- "Steal duel: R1"
- "Tag-up: R3, 1 out"
- "Dropped-third: 2 strikes"
- "Walk-off: bottom 9, down 1, R2"

`Scenario::apply(...)` writes the authoritative resources (`ScoreBoard`,
`Bases`, batting-order slot, resets `Play` to a fresh at-bat) and fires a
`ScenarioAppliedEvent` that runner/jersey/UI systems consume to re-mirror
rigs and repaint — respecting the existing rule that fx/fielding/runner
never mutate `ScoreBoard`/`Bases`. Apply is **refused while the ball is
live**, the same gating as pause.

**Scenario tab:** preset buttons plus a custom builder (base checkboxes,
count/outs/inning spinners, score fields, next-pitch picker), and a
**forced-contact override** (`Off` / `Whiff` / `FoulTip` / `Weak` / `Solid`
/ `Perfect`) that pins the outcome of `contact_quality` for deterministic
swing-outcome testing. The override lives in `DebugState` (debug-only —
it is not scenario data and never reaches the lib target).

## 4. State inspection, gizmos, time controls

**State tab** (read-only): `Play` phase and `pending_call`, last swing
`dt_ms` + `ContactQuality`, steal-window countdown with `LeadState` /
`window_lead`, chaser entity and assignment, ball speed/height,
`RunnersSettled`, FPS / frame time.

**Gizmos** (built-in `bevy::gizmos`, each individually toggleable in
`DebugState`):

- True strike-zone box (`ZONE_*` consts) at plate depth.
- Live-ball predicted trajectory and touchdown point (reusing `fx.rs`'s
  landing prediction).
- Chaser intercept line and throw-target base highlight.
- PCI radius ring.
- Runner target lines (current bag each runner is running to).
- Rapier collider wireframes — toggle `RapierDebugRenderPlugin`, whose
  `debug-render-3d` feature is already compiled in.

**Time controls:** speed presets (¼×, ½×, 1×, 2×) and single-step. These
compose with `juice.rs` instead of fighting it: juice gains a base-speed
resource (default 1.0) that debug sets; hit-stop/slow-mo multiply the base,
and the watchdog / `OnExit(Playing)` restore returns to *base*, not a
hardcoded 1.0. The headless harness keeps `JuiceDisabled` and a base of 1.0,
so scripted timing is unaffected.

## 5. Testing

- **Unit tests:** scenario invariants (legal counts, `bases` sized to the
  variant), dump-diff formatting, preset-table completeness.
- **New e2e** (`tests/e2e_scenarios.rs`): boot the shared harness, `apply` a
  preset, drive one play, assert the rule outcome — the template for future
  rule regressions. Existing e2e tests migrate opportunistically.
- **CI:** `cargo check --features debug` on native and wasm; all existing
  suites unchanged. Promoted defaults equal today's consts by construction —
  `balance_sim` bands must not move.

## Risks & interplay

- **juice.rs time composition** is the one refactor with cross-cutting
  behavior; it is guarded by the existing juice watchdog tests plus the
  harness's `JuiceDisabled` path.
- **Threading `&Ruleset` into rules functions** touches many call sites but
  is mechanical; unit tests pin behavior at defaults.
- **Scenario apply mid-play** is refused (ball live), mirroring pause
  gating, so it can never corrupt a resolving play.
- **egui on wasm** works, but the debug feature is never part of the Pages
  release build; CI checks compilation only.
