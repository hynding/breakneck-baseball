# Rule-Scenario E2E Tests & CPU Steal Calls Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Exercise the advanced rules (HBP, stolen base, caught stealing, double play, hit-and-run, dropped third strike) through the real app in scripted e2e scenarios, and teach the CPU to occasionally call steals.

**Architecture:** A shared headless harness moves to `tests/common/mod.rs` (windowless app boot, `DriveGame` schedule after `PreUpdate`, plugin finish/cleanup). Scenario tests use a stage counter advanced by the outer loop when a milestone predicate on `Bases`/`ScoreBoard` is observed; the in-app driver reads the stage and writes `Intents`. CPU steal calls latch one decision per windup in `CpuState` (deterministic hash noise, like every other CPU decision).

**Tech Stack:** Rust, Bevy 0.15, cargo integration tests.

## Global Constraints

- PATH prefix for cargo; verify native + wasm targets; zero clippy warnings
- Deterministic scripts only — milestones on resource state, generous frame caps
- Commits end with `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`

## Scenario designs (all physics-verified against rules.rs constants)

- **HBP**: pitcher aims full inside `(1.0, 0)` → crossing x ≈ 0.55–0.6 ≥ 0.52.
- **Stolen base**: batting side holds aim.y = −1 through the windup; changeup (off-speed) → safe.
- **Caught stealing**: same arm, fastball (aim.y = 0.6 at release) → runner out, count intact.
- **Double play**: runner on first, batter swings very late (press when ball z ≤ −0.95): timing < −1.35 → quality clamps to 0.08 → weak ~7 m grounder → Ground; DP flips the half at 1 out.
- **Hit-and-run**: arm steal + swing at z ≤ 0.8 with aim.y = −1: launch ≈ 8–9°, dist ≈ 29–31 m → Hit(1); jump sends first-to-third.
- **Dropped third**: pitcher throws curveballs (aim.y = −1); batter "swings" while the ball is unreachable (z > 5) → three swinging strikes; third is dropped (curve + first open) → batter reaches.
- **CPU half-inning**: 1P game, scripted human pitches center changeups; CPU offense (with steal calls) must complete the top half.

---

### Task 1: Shared harness `tests/common/mod.rs`

Extract `headless_app()` (windowless DefaultPlugins, Rapier, GamePlugin, 240 Hz manual time, plugins finish/cleanup) and the `DriveGame` schedule label; `e2e_full_game.rs` consumes it unchanged in behaviour. Run both existing tests green. Commit: `refactor: shared headless e2e harness`.

### Task 2: `tests/e2e_advanced_rules.rs`

Two tests:
1. `hbp_steals_double_play_and_hit_and_run` — stages S0–S6 (HBP → SB → CS → HBP → DP half-flip → HBP on Home → hit-and-run first-to-third), milestone-driven.
2. `dropped_third_strike_lets_the_batter_reach` — three whiffed curves; assert batter on first, no out, fresh count.

Commit: `test: e2e scenarios for HBP, steals, double plays, hit-and-run, dropped third`.

### Task 3: CPU steal calls

`CpuState` gains `steal_call: Option<bool>`; `cpu_offense` decides once per windup (`steal_candidate().is_some() && hash01(t·6.1) < 0.3`) and holds aim down for the rest of the delivery. Add e2e `cpu_offense_completes_a_half_inning` (1P, scripted pitching, run until the top half flips). Commit: `feat: CPU occasionally calls steals; e2e for a CPU half-inning`.

### Task 4: Verification & docs

Full suite, clippy, wasm check; CLAUDE.md sentence on CPU steal calls. Commit: `docs: CPU steal calls`.
