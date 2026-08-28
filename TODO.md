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

## Playtest review 2026-08-27 (cycle 1)

Debug autoplay build, wasm CPU-vs-CPU 3-inning game (`?innings=3`) + four parallel
code-review passes (controls/UX, UI layout, sim/physics, presentation/audio). Game 1:
3 innings, 0-0 tie, **zero coach findings in 10,032 samples**; game 2 produced one
Late-grade chaser-convergence report (fielder #6, target 3.6 m short of goal >0.40 s) —
watch, not violation. The sim/rules pass found **no confident defects**. Screenshots:
docs/agent/playtest/2026-08-27/. Menu/settings/duel-view/staged moments deferred to
cycle 2+ (autoplay auto-advances past them; needs plain build + scenario staging).

61. [x] ship-blocker timing — fx's HitStop duplicates juice's freeze on the same contact and
    races it on `Time<Virtual>` relative_speed (can cancel Perfect's slow-mo tail), and it
    ignores reduce-motion/`JuiceDisabled`. Proposed fix: delete `HitStop` from
    `present/fx/mod.rs`; `present/juice.rs` owns relative_speed alone.
62. [x] ship-blocker HUD — Swing Meter track (right:210) overlaps the scoreboard card
    (~233 px wide at right:14): bar's bottom 103 px sits inside the card. Proposed fix:
    one right-anchored flex row in `present/ui/hud.rs`.
63. [x] ship-blocker theme — strike-zone frame/fill colors are hardcoded near-black and
    vanish against the Midnight Neon sky; flash/PCI cursor already use theme accent.
    Proposed fix: derive zone colors from `Theme.ui` in `present/field/zone.rs`.
64. [x] ship-blocker controls — orbit camera (C) reads WASD/arrows while play is live, the
    same keys that aim pitches/swings (P2 loses everything). Proposed fix: drive orbit from
    a non-gameplay input in `present/camera/rigs.rs` or restrict to dead-ball phases.
65. [x] ship-blocker UX — no quit-to-menu path exists in-game; wrong mode/innings means
    playing it out or reloading. Proposed fix: confirm-then-MainMenu entry on the pause
    board (`meta/subs.rs`), listed in the hint line.
66. [x] polish layout (fixed via a fixed 92 px offset anchor for the stamp + banner max-width; the shared-column approach broke wasm extraction and was reverted) — banner pill collides with the contact stamp below ~642 px viewport
    height and with the px-anchored PITCHING card on narrow windows (banner is %-anchored).
    Proposed fix: shared centered column + banner max-width in `present/ui/hud.rs`/`banner.rs`.
67. [x] polish layout (menu 10 / settings 20 / pause 30 shipped; the banner's tier 40 was dropped — GlobalZIndex on the transparent banner root stopped wasm extraction, see hud.rs comment) — zero explicit ZIndex anywhere; overlay stacking (menu vs settings vs
    pause) is spawn-order luck. Proposed fix: `GlobalZIndex` tiers in `meta/menu.rs`,
    `meta/settings/screen.rs`, `meta/subs.rs`, banner/stamp roots.
68. [x] polish audio — coverage gaps: routine ground-out (Thrown/Settled) is silent, whiff
    and pitch release are silent, steals/pickoffs are silent, and a WALK fires the same Epic
    stinger as a home run. Proposed fix: extend `present/audio.rs` event map; downgrade
    WALK's tone in `sim/flow/result.rs`.
69. [x] polish camera — C toggle hard-cuts both ways while V-cycling eases; orbit/zoom write
    the transform directly. Proposed fix: route through the smoothed rig in
    `present/camera/rigs.rs`.
70. [x] polish fx — HR fireworks (z 42..76) are behind the orbiting trot camera for ~half the
    show; orbit azimuth derives from wall-clock so the start phase is arbitrary. Proposed
    fix: seed azimuth at play start + bias arc behind home in `present/camera/framing.rs`.
71. [x] polish UX-docs — controls drift: pause help omits the P2 keyboard scheme and the
    pad bindings; V/Z have no pad equivalent at all; menu shows key hints but not pad
    equivalents; settings screen has no key-hint footer. Proposed fix: generate
    `CONTROLS_TEXT` from `KeyScheme` in `meta/subs.rs`; add pad bindings for view/zone;
    hint rows in `meta/menu.rs` + `settings/screen.rs`.
72. [x] polish input — gamepad hotplug is one-way (disconnect drops to keyboard silently,
    reconnect never rebinds) and in 1P a plugged-but-idle pad makes the keyboard dead.
    Proposed fix: handle the connect edge + banner both edges + merge keyboard/pad intents
    last-input-wins in `meta/input.rs`.
73. [x] polish input-feel — keyboard aim sums per-axis (diagonal |aim| 1.41 vs stick 1.0 →
    wider pitch envelope, 41% faster diagonal PCI) and sticks have no explicit dead-zone
    (drift integrates into the PCI cursor). Proposed fix: `clamp_length_max(1.0)` + 0.1
    dead-zone in `meta/input.rs`.
74. [x] polish HUD — AT BAT card width jumps every batter (min_width only) and long creator
    names grow it unbounded. Proposed fix: fixed width + ellipsize in `present/ui/banner.rs`.
75. [x] polish legibility — instructional text (menu controls block, pause hints, version
    tag) is 12-13 px at 55-65% alpha, the smallest type in the game and the only place
    controls are documented. Proposed fix: 15-16 px `text_primary` in `meta/menu.rs`,
    `meta/subs.rs`.
76. [x] nice rules — NOT A BUG: `rules::is_game_over` already plays extra innings on a tie
    (unit tests `tie_after_regulation_goes_to_extras` / `one_inning_tie_goes_to_extras`);
    the cycle-1 "0-0 game over" was a mid-game sample, not the final score.
77. [ ] nice theme — fx palette pulls from five color sources (ui.accent ring, ball.trail
    halo/sparks, hardcoded dust + firework colors, settings trail_color); theme swaps
    repaint only part. Proposed fix: Theme-owned fx colors in `core/theme.rs` +
    `present/fx/particles.rs`.
78. [x] nice perf/robustness (meter is_changed guard shipped; the pause-board height cap for <530 px viewports remains open — rare, revisit with a real mobile pass) — pause board has no max-height/scroll (clips below ~530 px
    viewports); `update_meter_bar` dirties Node every frame forcing full-tree relayout in
    Classic. Proposed fix: height cap in `meta/subs.rs`; `is_changed` guard in
    `present/ui/hud.rs`.
79. [x] nice settings (STRIKE ZONE row shipped on the menu screen; in-game settings access deferred — the pause board's cursor keys would clash) — `show_strike_zone` persists but has no settings row (only the
    undiscoverable Z on the pause board), and no Settings entry is reachable in-game.
    Proposed fix: STRIKE ZONE row + run screen in Paused too (`meta/settings/`).
80. [x] nice UX — auto-pause on focus loss never auto-resumes on refocus. Proposed fix:
    watch the refocus edge in `meta/subs.rs` (only when the auto-pause opened the board).
81. [x] nice hygiene — `.playwright-mcp/` session artifacts got committed (734f7ce) and the
    dir isn't ignored. Proposed fix: add to `.gitignore`, `git rm -r --cached`.
82. [x] nice readability — AWAY's salmon jersey reads skin-toned at broadcast distance and
    the mound pitcher is low-contrast vs the green; verify against each Theme's
    `PlayerTemplate` and consider deepening the away tone (`core/theme.rs`).
    Screenshot: docs/agent/playtest/2026-08-27/05-sample.jpeg.

## Playtest review 2026-08-27 (cycle 2 additions)

Web-shell pass (verified against the live Pages site) + a measured pacing/dead-air pass.
Cycle-2 fixes shipped alongside: 66, 67, 68, 74, 80 (see TADA when checked off).

83. [x] polish web — the download progress bar is dead on the live site: Pages serves gzip so
    content-length is absent and the bar pins to 100% for the whole ~17 MB stream. Fix:
    have pages.yml stamp its computed wasm_size into web/index.html and drive % off that.
84. [x] polish web — no stall watchdog: a hung fetch leaves the spinner forever. Fix: swap
    status text after ~15 s without bytes; offer Reload after ~60 s (web/index.html).
85. [x] polish web — no <noscript>: JS-off visitors see a spinner claiming "Downloading".
    Fix: noscript block hiding #loading and stating JS+WebGL2 required.
86. [x] polish web — wasm is fully buffered (two copies) then non-streaming instantiated;
    compile can't overlap download. Fix: progress-counting TransformStream Response →
    instantiateStreaming path (web/index.html).
87. [x] polish web — dismissing an overlay leaves focus on the button; keyboard dead until
    a canvas click. Fix: canvas.focus() after hiding each overlay (web/index.html).
88. [x] nice web — bundle: page metadata (description/og/theme-color), fatal() should hide
    #touch-note, guard the bb-panic message listener with event.source === window, and
    aria-live/progressbar roles on the loading UI (web/index.html).
89. [x] polish pace — a third out with runners on blocks the changeover up to ~11 s while
    retired runners jog multi-base despawn paths home (banner gone after 1.6 s). Fix:
    despawn leftovers immediately when bases.clear() came from the half flip
    (sim/runner.rs:313, gated by flow/result.rs settle).
90. [x] polish pace — a home run costs ~15.5 s contact→next pitch (0.9 s delay + 4 bases at
    runner_speed). Fix: HR-specific trot speed or ~8 s HR settle cap (sim/runner.rs).
91. [x] polish pace — the 1.5 s steal window and the CPU's 0.7-1.2 s pitch delay stack
    (ai.rs resets pitch_delay every window frame): 2.7-3.2 s of held ball per pitch with
    runners on. Fix: let the delay tick during the window (sim/ai.rs:178) and/or shorten
    the window.
92. [x] polish pace — a foul into the stands teleports the ball to the mound without ever
    firing Landed, dead-airing up to the 11 s LIVE_PLAY_MAX. Fix: send Landed at the
    pre-reset position from reset_ball_if_out_of_bounds (sim/ball.rs:344).
93. [x] polish pace — every ordinary foul holds ~3.8 s while the batter ghost runs out a
    dead ball (RunnersSettled gates Result). Fix: despawn BatterGhost on Outcome::Foul
    (flow/result.rs / sim/runner.rs).
94. [x] nice pace — a walk takes 6.4-6.9 s to the next pitch (advance is run at 3.66 s/base,
    then the steal window). Fix: boosted dead-ball advance speed (sim/runner.rs).
95. [x] nice pace — settle caps oversized: THROW_SETTLE_CAP 4 s (throw crosses in ~1 s),
    RESULT_SETTLE_CAP 20 s. Fix: ~1.5-2 s and ~8 s once 90 lands (flow/live.rs, result.rs).
96. [ ] nice theme — Midnight Neon's field/dirt stay daylight-bright (only sky/UI/jerseys
    change), so "night" reads as a black void over a sunny field; consider dimmed/cooler
    field materials per theme (core/theme.rs + present/field/).
    Screenshot: docs/agent/playtest/2026-08-27/09-neon-zone-before.jpeg.
97. [ ] nice ui — the hidden banner pill's near-zero-alpha keep-alive tint reads as a faint
    ghost rectangle against Midnight Neon's pure-black sky (top-center). Consider matching
    hidden_tint's alpha to theme darkness or keying the pill's hidden state off Visibility
    on 0.17+ (present/ui/hud.rs).
    Screenshot: docs/agent/playtest/2026-08-27/10-neon-call.jpeg.
