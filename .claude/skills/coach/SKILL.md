---
name: coach
description: Use when "players aren't running/fielding right", "the fielder just stood there", a rig ignores a call, or when editing src/game/sim/fielding.rs, src/game/sim/runner.rs, or src/game/sim/flow/. The Coach is the always-on observer that knows what every player should be doing and reports drift as findings; this skill covers its checks, tolerances, reports, and how to add a check.
---

# The Coach

An always-available observer that derives what every player *should* be doing
from the rules layer and flags when the sim/presentation layer drifts. Two
halves behind one facade (`game::coach`):

- `src/game/core/coach.rs` — the **pure checks**: `Coach::observe(&CoachSnapshot)
  -> Vec<CoachFinding>`. No ECS, no RNG, deterministic; per-check watchdogs
  decide Late (happened outside tolerance) vs Violation (never happened).
- `src/game/sim/coach.rs` — the **observer**: per-frame fact tracking (events a
  sample would miss) plus a ~30 Hz virtual-time sampler that builds the
  snapshot and accumulates `CoachReport` + `CoachFindingEvent`s.

**The Coach never mutates gameplay state** — not `ScoreBoard`, `Bases`,
`Play`, rigs, or physics. Only flow applies rules. Anything the Coach needs
to see gets a read-only accessor (e.g. `fielding::ActivePlay::chaser()`),
never a write path.

## Enabling

Gated on the `CoachEnabled` resource: present by default in `--features
debug` builds and inserted by `tests/common/mod.rs` for every headless e2e;
absent (fully inert, ~zero cost) in release. Frame cost when enabled is
within run-to-run noise (<1% of headless frame time). Toggle live from the
F1 panel's **Coach** tab (hotkey 6), which also has per-check switches
(`CoachConfig::disabled`), the findings feed, and gizmo overlays (chaser
intercept, expected runner-break arrows, uncovered force bags).

## The checks

| CheckId | Expectation (source of truth) |
|---|---|
| `runner-breaks` | Runners aboard break per `rules::runner_break` within tolerance; nobody leaves early on a tag-up. Skipped when the runners were sent (`steal_armed`). |
| `chaser-convergence` | A chaser is assigned promptly and its intercept tracks the live ball (predicted landing airborne, the ball itself after the bounce). |
| `base-coverage` | Every force-relevant bag (batter's first + each forced runner's next) has a cover assignment shortly after contact. |
| `catcher-receives` | An untouched, catchable pitch ends at rest in the mitt. Exempt: dirt/sailed (judged at the same two observation points flow judges them), HBP (re-derived via `rules::hits_batter` from the crossing), and the dropped third (read off `Play::last_strike_call`). |
| `throw-discipline` | A held gathered ball auto-throws by `pace.auto_throw_delay_secs`; a decided `pending_call` is announced inside its settle cap. |
| `steal-window` | No delivery while the window still gates the pitch. |
| `settlement` | Runners settle inside the result pause (+ trot allowance); Result never sticks past flow's hard cap. |
| `idle-discipline` | Between plays every fielder returns to (and holds) his `FieldSpec` spot. |

Deliberately cut (could not be made non-flaky within tolerance): monotonic
intercept-closing (steering/re-planning legitimately reverses it), the
backup-behind-the-throw-line geometry, and post-window steal outcomes (the
pure rules own those; sim can't drift there without tripping other checks).

## Tolerances

All in one struct: `CoachTolerances` (`core/coach.rs`), defaults ~300–400 ms
of virtual time for reaction-type checks plus distance slacks. Construct the
sim observer's `Coach` with different values only if a check flakes — and
prefer fixing the check. Severities: `Violation` (rules say X must happen,
it didn't), `Late` (it happened past tolerance), `Style` (legal but ugly).

## Reading reports

- `CoachReport` (resource): `count(check, severity)`, `total(severity)`,
  `recent` ring buffer (64), `samples`.
- Headless: `tests/e2e_coach.rs` prints the per-check table and fails on any
  violation not in its `KNOWN_ISSUES` allowlist. The allowlist is printed on
  every run and every entry must link a TODO.md item — keep it loud, keep it
  short, and empty it as fixes land.
- Autoplay runs stream `COACH_FINDING {json}` lines (stderr native, console
  wasm) and write `coach-report.json` at game end (file native; localStorage
  key `bb-coach-report` + a `COACH_REPORT` console line on wasm). See the
  `auto-playtest` skill.

## Adding a check

1. **Pure predicate first**: add the watchdog to `core/coach.rs` — a
   `CheckId` variant (+ `ALL` + `label()`), fields on `PlayMemory` if it
   needs deadlines, a `check_*` method called from `observe`, and an entry in
   `CoachTolerances`. Derive the expectation from an existing `rules::*`
   function — never re-encode baseball logic that can drift.
2. **Snapshot field**: if the world fact isn't in `CoachSnapshot`, add it and
   fill it in `sim/coach.rs`'s sampler (or the per-frame tracker, if a 33 ms
   sample could miss it — that's why contact, the bounce, the crossing, and
   the glove-line height are tracked per frame).
3. **Unit tests both ways** in `core/coach.rs`: a passing sequence and a
   violating sequence, in the same file, `core/rules` test style.
4. **Run the gate**: `cargo test --test e2e_coach` against the real game
   before trusting the check — the first version of a check usually needs its
   exemptions taught (see the dirt-ball two-observation-point lesson in
   `sim/coach.rs`).

A check that can't be made non-flaky inside its tolerance gets cut, not
shipped: the Coach's value is trust — a small set of airtight checks beats a
big set of flaky ones.
