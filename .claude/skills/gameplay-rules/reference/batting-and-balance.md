# Batting Spine & Balance Economy — Full Narrative

Verbatim source: the pre-slim CLAUDE.md (also preserved whole in `docs/agent/ARCHITECTURE-FULL.md`).

Batting *feel* is a data-driven spine, not a hit-or-miss check: every judged swing measures its
timing error against the ball's plate ETA (`flow::swing_dt_ms`, early = negative) and
`rules::contact_quality` grades it into `ContactQuality::{Whiff, FoulTip, Weak, Solid, Perfect}`
off the active `Ruleset` windows (`perfect_ms`/`solid_ms`/`foul_ms`), with the exit speed scaled
by that quality's multiplier (`exit_solid`/`exit_perfect`) and the launch pulled by
`pull_yaw_per_ms · dt_ms` — `Weak` is the PCI adapter's arm only, never produced by the Classic
windows, but kept on every exhaustive match.

Flow fires one `ContactEvent { quality, batting_team, dt_ms }` per swing (whiffs included) as a
read-only report for presentation; the rule/physics consequence is applied at the swing site.

The *input* front-ends are three real adapters in `src/game/sim/batting.rs` (the `SwingCommands`
seam that `flow::pitch_live` consumes without seeing the style — `adapt_swings` is chained between
`wind_up` and `pitch_live`):

- **Classic** swings on the action edge.
- **Swing Meter** holds to load (the batter's stance sinks as it fills, via `MeterLoad`) and
  releases to swing; a still-held button past the late edge forces a whiff.
- **PCI** glides a velocity-steered aiming cursor (keyboard-playable — `PciState`, drawn by
  `src/game/present/field/`'s `PciCursorMarker` over the zone plane) whose distance from the ball
  at contact drives `rules::pci_contact_quality` (dead-center = full windows; at `pci_radius_m`
  Perfect→0 and Solid halved; beyond → `FoulTip`) and whose offset (not raw aim) sets the hit
  direction via `rules::pci_aim`.

Routing is per-player via `Settings::batting_style` + `Controllers::player_index`; the **CPU
always bats Classic** regardless of settings (`batting::style_for`), so the balance economy is
untouched by style.

Off the `ContactEvent`, `src/game/present/juice.rs` runs the game feel — a frame-counted hit-stop
on Solid/Perfect and a slow-mo tail on Perfect, all by dialing `Time<Virtual>`'s `relative_speed`
with a real-clock watchdog and an `OnExit(Playing)` restore so it can never stick — behind a
`JuiceDisabled` resource the headless harness inserts (a slowed virtual clock would corrupt
scripted timing). `src/game/present/audio.rs` layers a looping crowd murmur under a roar (Perfect
swing or deep fly) / crowd-peak roar (home run, louder) / groan (swinging K), plus three
quality-keyed bat cracks.

The CPU's timing is a single dial, `Ruleset::cpu_timing_spread_ms` (how far its swings scatter
off dead-on), and `tests/balance_sim.rs` (N one-inning CPU-vs-CPU games to GAME OVER, per-9
extrapolated) is the **arbiter** of the offensive economy (K% / runs / HR bands) — retune the
windows/multipliers/spread there, not by feel.

The home run is the payoff moment: `src/game/present/fx/` launches a fireworks show over the
outfield (denser off a Perfect, read from `Play::last_contact_quality`), the broadcast camera
orbits the diamond through the trot (`Play::is_home_run()` during `Phase::Result`), and — the one
deferral — a game-ending call (walk-off included) fires `GameState::GameOver` only from
`result_phase`, once the play has fully finished on screen (banner shown, `RunnersSettled` true),
so the slow-mo and trot are never cut off at contact even though the scoreboard still updates
when flow applies the rules.
