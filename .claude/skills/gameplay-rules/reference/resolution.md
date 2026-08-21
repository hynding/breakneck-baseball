# Live-Play Resolution — Full Narrative

Verbatim source: the pre-slim CLAUDE.md (also preserved whole in `docs/agent/ARCHITECTURE-FULL.md`).

## Advanced rules & the steal window

Advanced rules are deterministic (no RNG anywhere in `src/game/core/rules/`), keyed off data the
engine already computes: tag-ups/sac flies off the fly's `deep` flag, double plays off base state
and outs remaining, hit-by-pitch off the plate-crossing point, dropped third strike and steal
outcomes off the pitch kind (curveball = in the dirt; fastball = catcher's throw wins),
hit-and-run off the windup-held steal flag, and a nine-slot `BattingOrder` per team.

Whenever a runner is in a position to steal (`rules::steal_candidate` — a runner parked on third
alone gates nothing), every new at-bat opens a **steal window** (`Ruleset::steal_window_secs`,
default 1.5 s; `Play::in_steal_window`): the pitch is gated while the leadoff duel runs. The
offense holding Down stretches the lead runner's lead (`LeadState`, mirrored by the runner rigs
as a real walk off the bag); while extended, a defensive action press is a **pickoff** throw
(`rules::attempt_pickoff`: out if extended, safe dive-back otherwise, on a reload cooldown so a
held button can't spam the bag; a pickoff out takes the normal result pause, after which runners
still aboard earn a fresh window). Only a lead stretched *during* the window (`window_lead` — the
stretch that was actually exposed to the pickoff) earns `big_jump`, the steal no pitch beats, at
delivery; stretching after the window closes, or arming with Down during the windup only, is a
late break that keeps the classic off-speed-safe/fastball-out race. The CPU plays both sides of
the window with hash-noise decisions in `src/game/sim/ai.rs`.

Untouched pitches really end in the mitt: `flow::catcher_receives` stops the ball at the
`CatcherRole` glove (skipping balls in the dirt — the dropped third stays loose) and fires
`PitchCaughtEvent` for the glove pop.

## Live resolution

Ball-in-play outcomes are resolved **live**, not at contact: contact settles only what physics
settles (a home run over the fence via `rules::classify_contact`); `src/game/sim/fielding.rs`
runs a real chase (the assigned fielder re-plans its intercept from the live ball each frame,
free fielders cover the bases, the next-best fielder backs up the play) and reports physical
milestones as `flow::LiveBallEvent`s (caught / landed / thrown / settled); `flow::resolve_live_play`
turns those into the call through pure runner-vs-throw race functions in `src/game/core/rules/`
(`resolve_catch`, `resolve_thrown`).

A thrown resolution is **decided at the throw but announced at the arrival**: the outcome sits in
`Play::pending_call` while the ball flies (phase stays `InPlay`, the camera stays with the play),
the batter ghost converts into a real runner rounding the bases and the runners aboard break for
the bags the call gives them (`runner::run_out_pending_call`), and only when fielding reports
`LiveBallEvent::Settled` (throw and any relay received; flight-capped as a backstop) does the
banner fire and the scoreboard change — so a conceded double *looks* like a throw to second that
just doesn't get there in time (`rules::throw_target` throws ahead of the batter when no force is
winnable, instead of a token toss to first).

A gathered ball is **held** briefly: the human defense may throw to any base (hold the base's
direction — right = first, up = second, left = third, down = home — and press action) on the
honest race clock, else after ~0.6 s the holder auto-throws to `rules::throw_target` (the lead
force the throw can beat) with the race clock backdated to the gather instant, so auto play
balance matches the pre-hold model. Force plays race honestly in `resolve_thrown`: a force at a
later bag relays to first and either turns two (`Outcome::DoublePlay`, with the relay leg
choreographed) or concedes the batter (`Outcome::FieldersChoice`) — grounder double plays are
never awarded by fiat, and a hit-and-run jump takes the runner forces off the table.

The batting side has a live say too (`RunnerCall`, read from held aim at resolution): Down sends
the batter for the extra base (cut down `Stretching` if the race is lost), Up holds him a base
early. A human defense also steers the chaser directly with aim during the chase (`steer_chaser` —
the CPU never does). While an uncalled fly ball is up, `src/game/present/fx/` parks a landing ring
on its live-predicted touchdown spot, sized by the remaining hang time.

A play is over only when it *looks* over: the result pause holds the next at-bat until every
runner rig has finished its base path (`runner::RunnersSettled`, hard-capped so a stray path
can't stall the game) — the home-run trot completes before the next batter steps in. The stadium
outfield wall is spawned from the spec fence (`rules::fence_at`) with fixed colliders, so live
balls carom off it (`WallBangEvent` → banner, camera thump, sparks); fielders are kinematic, so
their chase targets are capped inside the fence instead.

`src/game/present/fx/`, `src/game/sim/fielding.rs`, and `src/game/sim/runner.rs` never mutate
`ScoreBoard` or `Bases` — they report or mirror; only flow applies rules. First base is at world
−X (the behind-home camera renders −X on screen-right), and aim.x is negated in the pitch/hit
mappings to match.

## Physics

Physics constants use real-world SI units (official MLB ball: 0.037 m radius, 0.148 kg) with a
custom drag force applied per physics tick. The ball ignores player capsules via collision groups
(`BALL_GROUP`/`PLAYER_GROUP`) — outcomes are resolved analytically at contact, and a pitch
glancing off the batter's collider would corrupt the called count.
