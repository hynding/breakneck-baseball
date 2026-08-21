# Balance Bands — Current Accepted Values

Source of truth: `tests/balance_sim.rs` (consts `K_PCT_BAND` / `RUNS9_BAND` / `HR9_BAND`).
If this file and the test disagree, the test wins — update this file.

## Asserted bands (N=40, `balance_bands_hold`)

| Signal | Asserted band | Nominal target | Measured spread (N=40, cross-process) |
|---|---|---|---|
| K% (strikeouts / PA) | 13.0 ..= 27.0 | 15..30 | 17.2 .. 21.9 |
| runs / team / 9 | 2.0 ..= 7.5 | 3.0..8.0 | 2.8 .. 4.4 |
| HR / team / 9 | 1.3 ..= 3.2 | 0.5..2.5 | 1.9 .. 2.7 |

Asserted bands are wider than nominal on purpose: they're sized to the measured per-process
spread plus margin for a binary-layout shift, so a seed change can nudge the mean but never
cross a band. Reaching a floor means a *systematic* regression, not noise.

## How the numbers are produced

- N=40 **one-inning** CPU-vs-CPU games to `GameState::GameOver`, single-threaded
  (`deterministic_headless_app`), virtual clock at `SIM_DT = 1/100 s` — a pinned measurement
  condition; the economy is DT-dependent, so never "fix" the DT to change a number.
- K% is a rate (scale-free). Runs and HR are counting stats, extrapolated per-9:
  per-9 = total ÷ (2 teams · N) · 9.
- Per-game decorrelation: each game idles `game_index · 37` frames first, so the N games are
  genuinely different games.
- Within one process the sim is bit-deterministic. Across processes a sub-ULP, entropy/ASLR-seeded
  perturbation in Rapier's core survives; the bands bound it (fixing it needs Rapier's
  `enhanced-determinism` feature, which would change the shipped binary — out of scope).

## Edge-specific rules

- **HR/9 ceiling (3.2)**: if it trips, the fix is an HR-retune via **CPU-side levers**
  (`cpu_timing_spread_ms`, the `ai.rs` launch-aim distribution) — NOT a wider band and NOT
  the frozen human-facing `Ruleset` exit multipliers.
- **runs/9 floor (2.0)**: catches offensive collapse; ~0.8 below measured min so seed noise
  can't reach it.
- Diagnostics (BB/HBP/hits/DP/FC/fouls/called-strike/whiff breakdown) print with every run —
  use them to make tuning targeted; they are probes, not asserts.

## The dials

All on `Ruleset` (`src/game/core/variant.rs`), consumed by `rules::contact_quality` and flow:

- `perfect_ms` / `solid_ms` / `foul_ms` — timing windows grading `swing_dt_ms`
- `exit_solid` / `exit_perfect` — exit-speed multipliers per quality
- `pull_yaw_per_ms` — launch direction pulled by timing error
- `cpu_timing_spread_ms` — the single dial for CPU swing scatter (the CPU always bats Classic)
- `sim/ai.rs` hash-noise decision distributions (swing choice, launch aim) — CPU-side only
