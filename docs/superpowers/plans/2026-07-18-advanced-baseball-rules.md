# Advanced Baseball Rules Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement every deferred regulation rule — caught foul pops, tag-ups/sacrifice flies, double plays, hit-by-pitch, dropped third strike, steals (with hit-and-run and doubled-off), and batting lineups — deterministically inside the existing analytic at-contact model.

**Architecture:** All rule logic stays in pure functions in `rules.rs` (unit-tested, no ECS); `flow.rs` translates results into banners; `runner.rs` rigs mirror `Bases` automatically. No RNG: every new rule keys off data the engine already computes (launch/distance bands, pitch kind, base occupancy). Pickoffs are explicitly out of scope — the analytic model has no leadoffs, so there is nothing to pick off; steals are the base-running risk mechanic instead.

**Tech Stack:** Rust, Bevy 0.15.

## Global Constraints

- Prefix all cargo commands: `export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"`
- Verify both targets after gameplay changes: `cargo check` AND `cargo check --target wasm32-unknown-unknown`
- fx/fielding/runner modules must never mutate `ScoreBoard` or `Bases`
- The e2e (`tests/e2e_full_game.rs`) must stay green: Away takes center changeups (no HBP/steal/dropped-third triggers), Home homers (unchanged path)
- Commit per task; messages end with `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`

## Rule Designs (deterministic, no RNG)

| Rule | Trigger | Effect |
|---|---|---|
| Caught foul pop | launch > 50° and dist < 55·s, landing foul | `Out(FoulPop)` — infield pops are caught fair or foul |
| Sac fly / tag-up | `Fly { deep: dist ≥ 65·s }`, not inning-ending | runner on last base scores; runner one behind moves up |
| Double play | Ground out, runner on 1st, ≥2 outs left, runners not going | 2 outs (force + relay); other runners advance; inning-ending force scores nothing |
| Productive ground out | Ground out, not inning-ending | all runners advance one base |
| Hit-by-pitch | Take crossing the batter's body window (x ≥ 0.52, 0 < y ≤ 1.7; batter stands at +0.7) | first base awarded, force advancement; swinging negates |
| Dropped 3rd strike | Swinging strike three on a **curveball** (ball in the dirt), 1st base open | batter reaches first, no out; strategic pitch-selection deterrent |
| Steal | Batting team holds aim.y < −0.7 during the windup; lead runner with an open next base (never home) | on a take: safe vs off-speed, caught stealing vs fastball (count call still applies) |
| Hit-and-run | Steal armed + fair hit | existing runners take one extra base (first-to-third) |
| Doubled off | Steal armed + caught fly/pop | the sent runner is also out; no tag-ups |
| Batting order | every completed plate appearance | 9-slot lineup per team rotates; shown in HUD |

Pitch aim widens: `target_x = aim.x * 0.6` (was 0.35) so far-inside pitches can reach the batter — painting corners now risks the free base.

---

### Task 1: Caught foul pops

**Files:** Modify `src/game/rules.rs` (OutKind, classify, tests), `src/game/flow.rs` (banner), `src/game/fielding.rs` (match arms)

`OutKind` gains `FoulPop`; `Fly` becomes `Fly { deep: bool }` (payload used in Task 2 — do both variants now so fielding.rs is touched once).

classify_batted_ball: hoist the pop band above the fair check —

```rust
// Towering infield pop-ups are caught, fair or foul.
if launch_deg > 50.0 && dist < 55.0 * s {
    return Outcome::Out(if fair { OutKind::Pop } else { OutKind::FoulPop });
}
if !fair {
    return Outcome::Foul;
}
```

(requires computing `speed`/`launch_deg` before the fair check). Fly band returns `OutKind::Fly { deep: dist >= TAG_UP_MIN_DIST * s }` with `const TAG_UP_MIN_DIST: f32 = 65.0;`.

flow banner: `OutKind::FoulPop => "FOUL POP OUT"`, `OutKind::Fly { .. } => "FLY OUT"`.
fielding.rs: `OutKind::Fly { .. } | OutKind::Pop | OutKind::FoulPop` in the airborne match.

Tests (TDD first): steep short pop sprayed outside the wedge → `Out(FoulPop)`; low foul liner still `Foul`; existing fly test updated to `Fly { .. }` patterns.

Commit: `feat: caught foul pops — infield pops are outs fair or foul`

---

### Task 2: Tag-ups, sacrifice flies, double plays, productive outs

**Files:** Modify `src/game/rules.rs`, `src/game/flow.rs`

rules.rs — split `record_out` and add the batted-out applicator:

```rust
/// Charges one out without ending the at-bat (a retired base-runner).
pub fn charge_out(score: &mut ScoreBoard, bases: &mut Bases, rules: &Ruleset) {
    score.outs += 1;
    if score.outs >= rules.outs_per_half {
        score.outs = 0;
        reset_count(score);
        bases.clear();
        if score.top_of_inning {
            score.top_of_inning = false;
        } else {
            score.top_of_inning = true;
            score.inning += 1;
        }
    }
}

pub fn record_out(score: &mut ScoreBoard, bases: &mut Bases, rules: &Ruleset) {
    reset_count(score);
    charge_out(score, bases, rules);
}

/// The base-running consequences of a batted-ball out.
pub struct OutPlay {
    pub outs: u32,
    pub runs: u32,
    pub double_play: bool,
    pub doubled_off: bool,
}

pub fn apply_batted_out(
    score: &mut ScoreBoard,
    bases: &mut Bases,
    rules: &Ruleset,
    kind: OutKind,
    runners_going: bool,
) -> OutPlay {
    let outs_left = rules.outs_per_half.saturating_sub(score.outs);
    let (mut outs, mut runs, mut double_play, mut doubled_off) = (1u32, 0u32, false, false);
    match kind {
        OutKind::Ground => {
            if !runners_going && bases.is_occupied(0) && outs_left >= 2 {
                bases.set(0, false);
                outs = 2;
                double_play = true;
            }
            if outs < outs_left {
                runs = advance_trailing(bases);
            }
        }
        OutKind::Fly { deep } => {
            if runners_going {
                doubled_off = double_off_lead_runner(bases);
                if doubled_off { outs += 1; }
            } else if deep && outs < outs_left {
                runs = tag_up(bases);
            }
        }
        OutKind::Pop | OutKind::FoulPop => {
            if runners_going {
                doubled_off = double_off_lead_runner(bases);
                if doubled_off { outs += 1; }
            }
        }
        OutKind::Pegged => {}
    }
    score.add_runs(runs);
    reset_count(score);
    for _ in 0..outs {
        charge_out(score, bases, rules);
    }
    OutPlay { outs, runs, double_play, doubled_off }
}
```

with private helpers `advance_trailing` (walk lead-to-trail, everyone up one, count runs), `tag_up` (last base scores, one behind advances), `double_off_lead_runner` (remove the steal-eligible lead runner, if any; returns whether one was removed) and `steal_candidate(bases) -> Option<usize>` (shared with Task 5: highest occupied base whose next base is open and isn't home).

flow.rs `resolve_contact`: the `Outcome::Out` arm calls `apply_batted_out(..., false)` (Task 5 threads the real flag) and banners from `OutPlay`: double_play → "DOUBLE PLAY!", sac runs on a fly → "SAC FLY  +n", doubled_off → "DOUBLED OFF!".

Tests (TDD): DP removes both, R3 scores on non-ending DP, inning-ending force scores nothing, no DP with 2 outs, routine grounder advances everyone, deep fly scores R3 / advances R2, shallow fly holds, 2-out deep fly ends half scoreless, caught-stealing charge_out keeps the count (new), doubled-off Fly removes lead runner.

Commit: `feat: tag-ups, sacrifice flies, double plays, productive ground outs`

---

### Task 3: Hit-by-pitch

**Files:** Modify `src/game/rules.rs`, `src/game/flow.rs`

rules.rs: `target_x = aim.x * 0.6` in `pitch_velocity_kind`; add

```rust
/// The batter's body window at the plate (he stands at x ≈ +0.7).
const BATTER_X_MIN: f32 = 0.52;
const BATTER_Y_MAX: f32 = 1.7;

/// Does a plate-crossing point plunk the batter? Only meaningful on a take —
/// swinging at the pitch negates a hit-by-pitch, as in the rulebook.
pub fn hits_batter(crossing: Vec2) -> bool {
    crossing.x >= BATTER_X_MIN && crossing.y > 0.0 && crossing.y <= BATTER_Y_MAX
}

/// Awards first base after a hit-by-pitch (dead ball: forced runners only).
pub fn hit_by_pitch(score: &mut ScoreBoard, bases: &mut Bases) -> u32 {
    let runs = advance_walk(bases);
    score.add_runs(runs);
    reset_count(score);
    runs
}
```

flow.rs take path: check `hits_batter(cross)` before the zone call → banner "HIT BY PITCH" (Good; Epic if it forces in a run).

Tests: sim test `fastball aimed full inside crosses in the batter window` (uses `simulate_pitch(PitchKind::Fastball, Vec2::new(1.0, 0.0))` — assert `hits_batter`), center pitches still strikes (existing test), `hits_batter` boundary cases, `hit_by_pitch` forces like a walk.

Commit: `feat: hit-by-pitch — far-inside takes award first base`

---

### Task 4: Dropped third strike

**Files:** Modify `src/game/rules.rs`, `src/game/flow.rs`

`StrikeCall` gains `DroppedThird`; `call_strike` gains `dropped_third: bool`:

```rust
pub fn call_strike(
    score: &mut ScoreBoard,
    bases: &mut Bases,
    rules: &Ruleset,
    dropped_third: bool,
) -> StrikeCall {
    score.strikes += 1;
    if score.strikes >= rules.strikes_per_out {
        if dropped_third {
            reset_count(score);
            bases.set(0, true);
            StrikeCall::DroppedThird
        } else {
            record_out(score, bases, rules);
            StrikeCall::Strikeout
        }
    } else {
        StrikeCall::Strike
    }
}
```

flow computes eligibility at the swing-and-miss site: `swinging && play.live_kind == Some(PitchKind::Curveball) && !bases.is_occupied(0)` (`live_kind` recorded at release — added here, reused by Task 5). Banner: "DROPPED 3RD STRIKE!" (Good tone — the batter lives).

Tests: flag true at two strikes → batter on first, no out, fresh count; flag false → strikeout; flag true before strike three → plain strike.

Commit: `feat: dropped third strike — swinging through a curve with first open`

---

### Task 5: Steals, hit-and-run, doubled off

**Files:** Modify `src/game/rules.rs`, `src/game/flow.rs`, `src/game/menu.rs` (help text)

rules.rs:

```rust
/// What sending the runner produced once the pitch was taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StealResult {
    Stolen { base: usize },
    Caught,
    NoRunner,
}

/// Resolves a straight steal on a taken pitch: safe on off-speed (the jump
/// beats the throw), caught on a fastball.
pub fn attempt_steal(
    score: &mut ScoreBoard,
    bases: &mut Bases,
    rules: &Ruleset,
    off_speed: bool,
) -> StealResult {
    let Some(runner) = steal_candidate(bases) else {
        return StealResult::NoRunner;
    };
    if off_speed {
        bases.set(runner, false);
        bases.set(runner + 1, true);
        StealResult::Stolen { base: runner + 1 }
    } else {
        bases.set(runner, false);
        charge_out(score, bases, rules);
        StealResult::Caught
    }
}
```

`advance_hit` gains `jump: bool` (existing runners step `hit_bases + 1`, batter unchanged); `apply_hit` threads it.

flow.rs: `Play` gains `steal_armed: bool` + `live_kind: Option<PitchKind>` (from Task 4). `wind_up` arms on batting-team `aim.y < -0.7`. Take path: after the ball/strike call (skipping walks and HBP — dead/awarded balls), `if play.steal_armed` resolve `attempt_steal(.., off_speed = live_kind != Some(Fastball))` → banners "STOLEN BASE!" (Good) / "CAUGHT STEALING" (Bad). Contact path threads `play.steal_armed` into `resolve_contact` → `apply_hit(.., jump)` and `apply_batted_out(.., runners_going)`. Reset both fields with the other play state in `result_phase`.

menu.rs help line gains: `Hold Down during the windup to send the runner`.

Pickoffs: **not applicable** — no leadoffs exist in the analytic model (runners sit on the bag until the pitch), so there is nothing to pick off. Documented here and in CLAUDE.md.

Tests: off-speed steal advances the lead runner only; fastball is caught stealing (out charged, count intact); no candidate → NoRunner; runner can't steal home; hit-and-run single sends first-to-third; strike-'em-out-throw-'em-out double play (K + CS = 2 outs).

Commit: `feat: steals, hit-and-run, and doubled-off base-running`

---

### Task 6: Batting lineups

**Files:** Modify `src/game/rules.rs`, `src/game/flow.rs`, `src/game/ui.rs`

rules.rs:

```rust
/// Batters per lineup (regulation nine).
pub const LINEUP_SIZE: u32 = 9;

/// Each team's place in its batting order. The order itself is implicit
/// (slot 1..=9); what matters for the rules is that it rotates.
#[derive(Resource, Default, Debug, Clone, PartialEq, Eq)]
pub struct BattingOrder {
    home: u32,
    away: u32,
}

impl BattingOrder {
    /// 1-based lineup slot of the batter currently up for `team`.
    pub fn current(&self, team: Team) -> u32 { ... % LINEUP_SIZE + 1 }
    /// The plate appearance ended; the next batter steps in.
    pub fn advance(&mut self, team: Team) { ... }
}
```

flow.rs: `init_resource::<BattingOrder>`, reset in `reset_flow`, and `order.advance(score.batting_team())` at every plate-appearance end: hits/home runs, batted outs, walks, strikeouts, dropped third, HBP — **not** fouls, balls, strikes, or steal results. (Advance before the count call mutates `top_of_inning` via a half-flip.)

ui.rs: `update_inning_text` also reads `BattingOrder` → `"TOP 1 · AB 4"` (existing painted text element — wasm-safe).

Tests: order starts at slot 1, advances per team independently, wraps 9 → 1.

Commit: `feat: nine-slot batting orders rotate per plate appearance`

---

### Task 7: Verification & docs

- `cargo test` (unit + e2e), `cargo clippy --all-targets`, `cargo check --target wasm32-unknown-unknown`
- CLAUDE.md: extend the architecture paragraph — advanced rules are deterministic analytic extensions (tag-ups, DPs, HBP, dropped third, steals; pickoffs N/A — no leadoffs); update the previous plan's backlog note.
- Commit: `docs: advanced rules in CLAUDE.md`
