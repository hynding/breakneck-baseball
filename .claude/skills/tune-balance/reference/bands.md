# Balance Bands — Current Accepted Values

Source of truth: `tests/balance_sim.rs` (consts `K_PCT_BAND` / `RUNS9_BAND` / `HR9_BAND`).
If this file and the test disagree, the test wins — update this file.

> 2026-08-27 (TODO 92 retune): fouls hit past `field.bounds` used to teleport to the mound
> and expire as *fair gathered at the mound* on an ~11 s race clock — a hidden safe-call
> subsidy the old economy was calibrated on. Fixing the call to FOUL (correct) pushed
> K% to 29.6 and runs/9 to 2.70; `cpu_timing_spread_ms` 225 → 195 restored the bands
> (see the balance run in the cycle-3 commit). Bands themselves unchanged.

> 2026-09-17 (TODO 105): the CPU's draws were clock-seeded, so *pacing* changes (result
> holds, the walk-up, the curtain) reshuffled outcomes — a 0.4 s longer strikeout hold
> tripped the HR/9 ceiling. `sim/ai.rs` now seeds from a game salt + the pitch sequence
> (`CpuState::seed`); N=40 is bit-identical across hold lengths. The reseeded draw model
> measures K% 15.8 / runs/9 3.71 / HR/9 2.14 (one process, N=40) — inside every band,
> with the spread row below still the pre-reseed figures until re-measured.

> 2026-10-29 (realism retune, P0 of the bot-vs-bot study): late swings no longer die in the
> catcher's glove, Solid contact is a real batted ball (`rules::hit_velocity` is a continuum),
> grounders are cut off on their roll path (`rules::ground_intercept*`, infield gather radius at
> the grass line), the CPU takes 3-0 and most first pitches, and steals/pickoffs are
> situational. The economy was re-centred on MLB 2025 and six new signals are asserted. The
> HR/9 band moved *down* (1.3..3.2 → 0.6..2.2) because the designed HR rate changed, not to
> pass a run; the K% floor moved 13 → 11 for the same reason (live K% is 14–15). Measured at
> N=40 (one process): K% 14.0, runs/9 5.6, HR/9 1.1, BB% 8.2, HBP% 0, pitches/PA 3.3,
> BABIP .367, CS/9 0.2, pickoffs/9 0.6. `tests/realism_sim.rs` (ignored,
> nine-inning, JSONL events) is the harness those numbers were tuned against.

> 2026-10-29 (outfield range, P1 of the study): the defense set too deep in centre (110 m vs
> MLB 96.9) and too shallow up the middle (2B/SS 36 m vs MLB ~45), so soft flares fell in
> behind the infield and gap bloops rolled for doubles. Fielders now set at MLB-average depth
> (`variant.rs`, per docs/BASEBALL.md). Measured at N=40 (one process): runs/9 3.4, HR/9 0.8,
> K% 14.9, BB% 7.5, BABIP .301, doubles/9 2.4. BABIP ceiling tightened .400 → .360 and a
> doubles/9 band added; both trip on the pre-fix economy (.367, 4.6).

## Asserted bands (N=40, `balance_bands_hold`)

| Signal | Asserted band | Nominal target | Measured (N=40) |
|---|---|---|---|
| K% (strikeouts / PA) | 11.0 ..= 27.0 | 15..30 | 14.0 (pre-retune spread 17.2 .. 21.9) |
| runs / team / 9 | 2.0 ..= 7.5 | 3.0..8.0 | 5.6 (pre-retune 2.8 .. 4.4) |
| HR / team / 9 | 0.6 ..= 2.2 | 0.5..2.5 | 1.1 (pre-retune 1.9 .. 2.7) |
| BB% (walks / PA) | 5.0 ..= 13.0 | 6..11 | 8.2 |
| HBP% | 0.0 ..= 2.5 | 0..2 | 0.0 |
| pitches / PA | 2.9 ..= 4.5 | 3.3..4.3 | 3.3 |
| BABIP | .220 ..= .360 | .250..330 | .301 (pre-range-fix .367) |
| doubles / team / 9 | 0.5 ..= 3.5 | 1.0..2.5 | 2.4 (pre-range-fix 4.6) |
| CS / team / 9 | 0.0 ..= 1.6 | 0..1 | 0.2 |
| pickoffs / team / 9 | 0.0 ..= 1.5 | 0..0.6 | 0.6 |

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

- **HR/9 ceiling (2.2)**: if it trips, the fix is an HR-retune via **CPU-side levers**
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
- `sim/ai.rs` behind-in-the-count package (TODO 10, 2026-08-24): at 2+ balls the pitcher's
  scatter scales to 0.4 with a get-it-over arsenal (mid-zone heater at +0.3 aim, not the
  standard +0.55, whose backspin lift parks at the top edge) **paired with** the batter's
  ahead-count compensation (timing draw ×0.65, chase ×0.5). The pairing is load-bearing:
  a pitcher-only pull converts walk PAs into strikeouts (K% brushed 29.7 in the reverted
  2026-08-21 attempt). `tests/e2e/passive_walks.rs` guards the human-facing symptom
  (passive half ≤ 3 BB; was 10); the bands here still arbitrate the CPU-vs-CPU economy.
