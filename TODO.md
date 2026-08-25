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

Improvement items surfaced by the run were filed as 58-60 (all closed; see TADA Batch 6).
