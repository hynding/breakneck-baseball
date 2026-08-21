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

## Release robustness follow-up

34. [ ] nice release — The post-deploy smoke test verifies 200/content-type/byte-size but
    cannot catch boot failures (the binaryen-108 table clamp shipped a deploy that died at
    init while every smoke signal passed — TADA 49). Consider a headless boot check in CI:
    fetch the deployed page in headless Chrome (or instantiate the wasm in Node with a
    stubbed import object) and assert the canvas appears / no `bb-panic` message fires.

## Engine upgrade

29. [ ] nice engine — Bevy 0.15.3 / bevy_rapier3d 0.28 → latest is Bevy 0.19.1 / rapier 0.35
    (four majors). Recommendation: **ship first, upgrade after** the production-readiness
    ship-blockers (resolved 2026-08-20 — TADA Batch 3) — then do it as four sequential gated
    migrations (~4–5 sessions), not one jump. Full analysis:
    `docs/agent/BEVY-UPGRADE-ASSESSMENT.md`.
