---
name: gameplay-rules
description: Use when editing anything under src/game/core/rules/, src/game/sim/ (flow, fielding, runner, ball, batting, ai, scenario), or src/game/meta/input.rs; when answering why a call/out/steal happened; or when tuning swing windows, exit speeds, or CPU timing. Covers Play phases, the steal window/pickoff duel, live-play resolution, the batting spine, and the balance economy.
---

# Gameplay Rules & Flow

How a pitch becomes a call. Full narrative: `reference/resolution.md` (live-play pipeline)
and `reference/batting-and-balance.md` (batting spine + economy). Read the relevant one
before changing behavior — most "bugs" here are deliberate design.

## Core principles

- **Rules are pure and deterministic.** `src/game/core/rules/` has no ECS and no RNG; functions
  take `Ruleset`/`FieldSpec` as parameters. The CPU's "randomness" is hash noise in `sim/ai.rs`.
- **Only `flow` applies rules.** `fx`, `fielding`, `runner` report or mirror — they never mutate
  `ScoreBoard` or `Bases`. Cross-module communication is event-driven (`PitchEvent`/`HitEvent`,
  `LiveBallEvent`, `ContactEvent`, `PitchCaughtEvent`, `WallBangEvent`).
- **Variants are data, not code.** `core/variant.rs` `Ruleset` + `FieldSpec` resources; never
  hardcode baseball facts in systems. Home plate at origin, +Z toward field, first base at
  world −X (aim.x is negated in pitch/hit mappings).

## The at-bat loop

1. **Steal window** — whenever `rules::steal_candidate` says a runner can steal, each at-bat opens
   a window (`Ruleset::steal_window_secs`; `Play::in_steal_window`) gating the pitch. Offense holds
   Down to stretch the lead (`LeadState`); while extended, a defensive action press is a pickoff
   (`rules::attempt_pickoff`, on a reload cooldown). Only a lead stretched *during* the window
   (`window_lead`) earns `big_jump` — the steal no pitch beats. Late breaks keep the classic
   off-speed-safe / fastball-out race. Dropped third = curveball in the dirt; catcher's throw
   wins vs fastball.
2. **Pitch** — five-pitch arsenal by dominant held-aim axis at release (`PitchKind::from_aim`):
   up fastball, down curveball, left slider, right sinker, neutral changeup. Untouched pitches end
   in the mitt: `flow::catcher_receives` (skips balls in the dirt) fires `PitchCaughtEvent`.
3. **Swing** — `flow::swing_dt_ms` measures timing error; `rules::contact_quality` grades it via
   the active `Ruleset` windows. Three input adapters in `sim/batting.rs` (Classic / Swing Meter /
   PCI) feed one `SwingCommands` seam; `adapt_swings` chains between `wind_up` and `pitch_live`.
   **The CPU always bats Classic** (`batting::style_for`).
4. **Live play** — contact settles only what physics settles (HR via `rules::classify_contact`).
   `sim/fielding.rs` runs a real chase and reports milestones as `flow::LiveBallEvent`s;
   `flow::resolve_live_play` turns them into the call via pure race functions (`resolve_catch`,
   `resolve_thrown`). A thrown resolution is **decided at the throw, announced at the arrival**
   (`Play::pending_call` until `LiveBallEvent::Settled`). Gathered balls are held ~0.6 s for a
   human throw choice, else auto-throw to `rules::throw_target` with the race clock backdated to
   the gather. Batting side steers via `RunnerCall` (Down = stretch, Up = hold); human defense
   steers the chaser with aim (`steer_chaser` — CPU never does).
5. **Result** — the pause holds until every runner rig finishes its path (`runner::RunnersSettled`,
   hard-capped). Game-ending calls fire `GameState::GameOver` only from `result_phase`.

## Balance economy

`tests/balance_sim.rs` (N CPU-vs-CPU one-inning games, per-9 extrapolated) is the **arbiter** of
K% / runs / HR bands. Retune `perfect_ms`/`solid_ms`/`foul_ms`, `exit_solid`/`exit_perfect`,
`pull_yaw_per_ms`, or `cpu_timing_spread_ms` there — never by feel. Because the CPU bats Classic,
adapter changes must not move the economy; if they do, that's a bug. (See the `tune-balance`
skill if present.)

## Testing this area

Run `cargo test` after touching flow/rules/menu/input/ai. E2e harness rules (`tests/common/mod.rs`):
inject input from the `DriveGame` schedule only, and spray scripted batted balls at a *set*
fielder's spot — the steal window puts the defense back in position before every pitch. Live sim
yields force outs, not turned twos; double-play relay math is pinned by `resolve_thrown` unit
tests. For staged situations (bases/count/inning/pitch presets), use `sim/scenario.rs`'s
`apply_to_world` — don't script an inning by hand.
