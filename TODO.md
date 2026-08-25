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

29. [ ] nice engine — Bevy 0.15.3 / bevy_rapier3d 0.28 → 0.19.1 / rapier 0.35 as four
    sequential gated migrations. Full analysis: `docs/agent/BEVY-UPGRADE-ASSESSMENT.md`.
    *Step 1 (0.15 → 0.16.1 + rapier 0.30 + inspector-egui 0.31) — code-complete on branch
    `upgrade/bevy-0.16` (worktree `.worktrees/bevy016`), 2026-08-25.* Green: full suite
    (29 suites incl. balance bands; headless sim ~72% slower per run — investigate), clippy
    -D warnings on default/dev+debug/autoplay, both-target checks, native visual run,
    browser boot + real-input smoke test. **Merge-blocked on one wasm/WebGL2 bug**: the
    play-banner and contact-stamp UI trees don't render — the tree lays out correctly
    (pill 126×47 at screen center) but computes `InheritedVisibility=false` from its first
    frame and its `Text` measures 0×1, while structurally identical probe trees (every
    wrapper/alpha/position/depth variant) render and native renders the real tree
    perfectly. Smells upstream. Next: try step 2 (0.17 reworked UI extraction) on top and
    re-verify, or build a minimal repro for a bevy issue. Repro: build the branch's wasm,
    serve `web/`, start a game — no "PLAY BALL!" debut pill at screen center. The branch
    also moved banner show/hide from the 0.15 hidden_tint alpha trick to Visibility
    toggling with a painted debut — keep that either way. Owner: branch
    `upgrade/bevy-0.16`, `src/game/present/ui/`.

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
