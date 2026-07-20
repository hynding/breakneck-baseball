# Live-Play Resolution, Mirrored Diamond & Ball-Follow Camera

> Executed inline (autonomous session). Tasks tracked via commits.

**Goal:** (1) Un-mirror the diamond so runners visually run to first; (2) replace
at-contact outcome resolution with live fielding races so the call happens during
the play; (3) make the broadcast camera visibly follow the ball.

**Task 1 — mirror:** Bevy renders world +X on screen-left for the behind-home
camera, so first base (index 0) belongs at −X. Negate base/fielder X in both
variants; negate aim.x in `pitch_velocity_kind`/`hit_velocity` so stick-right
still means screen-right; batter box stays +X (3B side, right-handed). Update
aim signs in unit tests + e2e HBP stages; lock the convention with a test.

**Task 2 — live play:** `classify_batted_ball` (bands) is deleted. Contact only
settles physics: `classify_contact` → HomeRun (over fence, from the live
`predict_landing`) or Live. Fielding chases with per-frame re-predicted landing
(`predict_landing_from`), emits `LiveBallEvent::{Caught, Landed, Gathered}`
(events, never score mutations). Flow resolves: Caught → FoulPop/Pop/Fly{deep by
catch spot} via `resolve_catch` + `apply_batted_out`; Landed foul → FOUL;
Gathered → `resolve_gathered` runner-vs-throw race (contest first only from
infield radius; concede + stretch race for extra bases; peg_outs flavors the
infield out) → Hit(n)/Out. Hard-cap timer force-resolves. Steal/hit-and-run,
DP, tag-ups, batting order all apply at resolution. Runner ghosts run on every
live fair ball; sync_runners adopts the ghost's position for continuity.
Constants (RUNNER_SPEED 7.5 shared with rigs, FIELDER_SPEED 7.0 = CHASE_SPEED,
REACTION, THROW flight/transfer, RUNNER_MARGIN, INFIELD_RADIUS·hit_scale) are
locked by unit tests on representative gathers (weak grounder → out, shallow
gather → single, deep gap → double, wall → triple, routine fly → caught).

**Task 3 — camera:** InPlay framing: eye tracks ball.x laterally and pulls back
with depth; faster target lerp.

**Docs:** CLAUDE.md — fielding is no longer cosmetic: it reports physical
events; rules races decide. e2e full-game stays valid (dead-red HR immediate;
walk-off may take two swings).
