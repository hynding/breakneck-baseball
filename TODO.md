# TODO

NOTE: Everything that has been completed gets moved to TADA.md

## Needs a human / native hardware session

11. [ ] nice audio — Audio events presumed firing (crowd/cracks synthesized at startup;
    `audio.rs` gesture note) but unheard over automation; a fresh-load listen on web should
    confirm the crowd loop starts after the first menu keypress (autoplay unlock). Needs an ear.
33. [ ] nice verification — Leftovers only a hands-on native session can close out:
    the three deferred playtest captures (Perfect-contact hit-stop/slow-mo, HR fireworks +
    orbit trot, a turned force play — force contact via the F1 Scenario tab,
    `cargo run --features "dev debug"`), the native F1 fps readout to pair with the recorded
    wasm numbers (120 fps display-capped at 2560×1488, Msaa 2x/1024 shadow map — TADA 46),
    and a real-gamepad pass over the subs board bindings (D-pad + South/North added
    2026-08-21, exercised headlessly only via the keyboard path).
    *Progress 2026-08-21 (session cut short, resume here):* Perfect-contact capture banked —
    PERFECT! stamp + landing ring + FLY OUT sequence (playtest 24-25); hit-stop/slow-mo feel
    unconfirmed (stills can't show it, need the player's yes/no). Still open: HR + force-play
    captures, fps readout, gamepad subs pass, and the item-11 crowd-loop listen. Walkthrough
    steps 5-12 in the 2026-08-21 session transcript still apply.

## Balance-gated (attempted 2026-08-21, reverted — the bands said no)

10. [ ] nice timing — Passive human batters draw CPU walk chains (observed 7 BB/inning;
    reproduced headlessly at ~6 BB per passive half). A behind-in-the-count zone pull in
    `ai::cpu_defense` (shrunken scatter + get-it-over arsenal at 2 or 3 balls) fixes the
    chains (down to 1-2 BB) but was reverted: every effective variant converts walk PAs
    into strikeout-or-contact at the CPU's fixed whiff rate, pushing K% toward its 27
    ceiling (measured up to 29.7) and HR/9 onto its 1.3 floor, and the count-dependent
    branch roughly doubles run-to-run band variance. A future fix must pair the pitcher
    pull with a CPU-batter-side compensation (e.g. more patience against grooved pitches)
    tuned as one package through `tests/balance_sim.rs`. (Post-revert baseline with the
    TADA-47 fielding fix in place is healthy and stable: K% 20.6-21.0, runs/9 4.16-4.84,
    HR/9 2.14-2.59 across repeat runs — squarely inside the historical anchors.)

## Engine upgrade

29. [ ] nice engine — Bevy 0.15.3 / bevy_rapier3d 0.28 → latest is Bevy 0.19.1 / rapier 0.35
    (four majors). Recommendation: **ship first, upgrade after** the production-readiness
    ship-blockers (resolved 2026-08-20 — TADA Batch 3) — then do it as four sequential gated
    migrations (~4–5 sessions), not one jump. Full analysis:
    `docs/agent/BEVY-UPGRADE-ASSESSMENT.md`.

## Coach findings 2026-08-24

First full instrumented run: matrix headless (7 cells, ~37 s) + e2e_coach
(scripted game + CPU half-inning) + native watched run (150 s, 3,988 samples)
+ wasm visual CPU-vs-CPU run (3+ innings, 12,249 samples). **Zero
violation-grade findings across all of it; KNOWN_ISSUES allowlist is empty.**
The one violation the very first run produced (catcher-receives, game_time
2.57 s, CPU-vs-CPU scenario, expected "untouched pitch at rest in the mitt",
observed "ball rolling at z −11") was an *observer* gray zone — the dirt
exemption judged at the glove line while flow judges it deeper — fixed in the
observer (`sim/coach.rs` now samples both points), not a sim bug.

Improvement items surfaced by the run (not gameplay violations):

58. [ ] polish testing — One full-suite parallel-load failure (single-test
    suite, 71.55 s — profile matches `tests/e2e_fielder_spots.rs`) that passed
    standalone and on rerun. Suspect executor-order sensitivity (the
    multi-threaded executor's ambiguous-order tie-break, same source
    `deterministic_headless_app` exists to remove). Repro: full `cargo test`
    under load. If it recurs, run fielder_spots single-threaded like
    balance_sim. Owner: `tests/e2e_fielder_spots.rs` / `tests/common/mod.rs`.
59. [ ] nice coach — The observer recognizes dropped-third plays by banner
    text ("DROPPED 3RD"), the one string-match in the snapshot builder. Give
    `flow::Play` a read-only "last strike call" getter so the Coach consumes
    the decision, not the announcement. Owner: `src/game/sim/flow/`,
    `src/game/sim/coach.rs`.
60. [ ] nice autoplay — wasm autoplay always plays 9-inning CPU-vs-CPU
    attract games; innings/script are env-configurable natively only. Add a
    query-param (or localStorage) switch for the web build so CI browser runs
    can be one inning. Owner: `src/game/meta/autoplay.rs`, `web/index.html`.
