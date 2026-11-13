---
name: tune-balance
description: Use when the game "feels too easy/too hard", when asked to tune batting or pitching, when "the CPU strikes out too much / never scores / hits too many homers", or when editing perfect_ms, solid_ms, foul_ms, exit_solid, exit_perfect, pull_yaw_per_ms, or cpu_timing_spread_ms on Ruleset, or the ai.rs decision noise. The balance economy is tuned by simulation, never by eyeballing.
---

# Tune the Balance Economy

`tests/balance_sim.rs` is the **arbiter** of the offensive economy. Nobody tunes feel by
eyeballing — every dial change is validated by the sim. Current accepted bands, the variance
model, and edge-specific rules: `reference/bands.md` (read it before touching anything).

## The loop

1. **Identify the dial.** K% too high → widen `solid_ms`/`foul_ms` or shrink
   `cpu_timing_spread_ms`. HR/9 out of band → CPU-side levers first (`cpu_timing_spread_ms`,
   `ai.rs` launch-aim distribution) — the human-facing `Ruleset` exit multipliers are frozen.
   Runs/9 is the chained, leveraged signal — prefer moving it via K%/contact dials and re-measure.
2. **Change it** in `src/game/core/variant.rs` defaults — or experiment live in the debug
   panel's Tune tab (`cargo run --features "dev debug"`, F1), which edits `Ruleset`/`FieldSpec`
   in-game and offers a **paste-ready diff export** to bring the numbers back to code.
3. **Run the sim:**
   ```sh
   export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"   # macOS only
   cargo test --test balance_sim            # N=40, ~1.5 min: quick read
   cargo test --test balance_sim -- --ignored   # N=100, ~5 min: the CI gate
   ```
   CI asserts the bands on the **N=100** run (`balance_sim_long`, the `balance` job in
   `ci.yml`), not N=40. The sim is deterministic per binary, so a re-run never clears an
   N=40 edge miss; check N=100 before moving a band or calling a change broken.
   The run prints K%, runs/9, HR/9 plus a full PA-outcome and pitch-event breakdown
   (BB/HBP/hits/DP/FC/fouls/called-strike/whiff) — use the breakdown to see *which* mechanism
   moved, not just the headline rates.
4. **Compare to bands** (`reference/bands.md`). In band → done. Out of band → revert or iterate.
5. **If a band intentionally moves**: update the consts in `tests/balance_sim.rs`, update
   `reference/bands.md`, and say why in the commit message. Band edges are anchored to measured
   spread — never widen one just to make a failing run pass.

## Invariants while tuning

- **The CPU always bats Classic** (`batting::style_for`), regardless of `Settings::batting_style`.
  Batting-adapter changes (Swing Meter, PCI) must therefore not move the economy — if
  `balance_sim` shifts after an adapter change, that's a bug in the adapter seam, not a retune.
- `SIM_DT = 1/100` is a pinned measurement condition. The economy is DT-dependent; never change
  the DT to change a number.
- One-inning games, per-9 extrapolation — don't "improve" the harness to nine innings; the
  runtime budget and the band anchors both assume 1/9 samples.
- Sim results are bit-deterministic within a process and can drift sub-ULP across processes
  (Rapier core), though in practice CI reproduces a binary's numbers exactly across machines
  (PR #27: identical output on two CPUs). An edge miss is a property of the change, not of
  the run: read N=100 rather than re-running N=40.
