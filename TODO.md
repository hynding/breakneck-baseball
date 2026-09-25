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
    migrations (~4–5 sessions), not one jump. Full analysis + progress log:
    `docs/agent/BEVY-UPGRADE-ASSESSMENT.md`.
    Steps 1–2 DONE on branch `upgrade/bevy-0.17` (2026-08-25): 0.16.1 then 0.17.3, all gates
    green incl. browser wasm — the week-long wasm banner bug root-caused (per-frame `ResMut`
    tick in a UI-writing system; fixed with fade deadlines, see the wasm-ui-and-present skill).
    Remaining: 0.18 (AnimationTarget split + UI extraction rework), then 0.19. Watch item
    carried from step 1: the headless sim runs ~72% slower than 0.15 (balance run 199 s vs
    115 s) — profile during the 0.18 step.
    *2026-09-25: branch caught up with `main`* (13 commits: playtest cycles 1–5, the Clean
    Code pass, touch controls, the SMB3 presentation pass). Main's side won every one of
    the 24 conflicts (it carries the newer structure); the 0.17 API was re-applied on top
    (Messages, `BorderColor::all`, `Justify`, `Query::single` → `Result`, `despawn`
    recursion, `Volume::Linear`, `WindowResolution::new`, `InputSystems`). Main's three
    banner `Timer` resources plus `BannerFollowUp` were ported to the fade-deadline pattern
    (`BannerFadeAt`/`StampFadeAt`/`SpeedFadeAt` in `present/ui/banner.rs`) — the wasm
    invariant this branch established. `drive_curtain` (`present/ui/curtain.rs`) still
    writes `ResMut<Curtain>` every frame while holding the curtain root's `&mut Node`/
    `&mut BackgroundColor` — the same shape as the bug — and **renders fine anyway**: a
    CDP screencast of the wasm build (JPEG frame sizes as a brightness proxy) shows the
    fade-in from black at game start and the strike beat's 0.25 s dip + 0.35 s reopen, so
    the trigger is narrower than "any per-frame ResMut + UI query"; left as-is, noted for
    the upstream repro. All gates green 2026-09-25: clippy `-D warnings` on default /
    dev+debug / autoplay, both-target checks, full suite (36 result blocks, identical to
    main's), browser run (menu → game → STRIKE pill + MPH → STRIKEOUT! → "OUT n" follow-up
    → walk-up card → duel; the three `B0004` warnings at boot remain). Lock now wants
    wasm-bindgen 0.2.127 (web-sys 0.3.104; 0.2.126 no longer resolves), so bump the CLI
    with `cargo binstall wasm-bindgen-cli --version 0.2.127 -y` when this merges and update
    the CLAUDE.md toolchain note. **Ready to merge into `main`.**

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
77. [x] nice theme — fx palette pulls from five color sources (ui.accent ring, ball.trail
    halo/sparks, hardcoded dust + firework colors, settings trail_color); theme swaps
    repaint only part. Proposed fix: Theme-owned fx colors in `core/theme.rs` +
    `present/fx/particles.rs`.
    *Done 2026-09-08 — see TADA 94.* Four of the five sources are now one `FxTheme` on
    `Theme`; `present/fx/` holds zero hardcoded colours. The fifth, `settings.trail_color`,
    stays a player setting **by design** — a theme swap must not silently overwrite the
    player's own choice — and `FxTheme`'s docs say so. Not yet eyeballed on screen: the
    night dust and neon shells need a home run under Midnight Neon, which the 1P
    human-pitching setup makes fiddly; fold it into item 33's hands-on capture list.
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
96. [x] nice theme — Midnight Neon's field/dirt stay daylight-bright (only sky/UI/jerseys
    change), so "night" reads as a black void over a sunny field; consider dimmed/cooler
    field materials per theme (core/theme.rs + present/field/).
    Screenshot: docs/agent/playtest/2026-08-27/09-neon-zone-before.jpeg.
    *Done 2026-09-25 — see TADA 104.*
97. [ ] nice ui — the hidden banner pill's near-zero-alpha keep-alive tint reads as a faint
    ghost rectangle against Midnight Neon's pure-black sky (top-center). Consider matching
    hidden_tint's alpha to theme darkness or keying the pill's hidden state off Visibility
    on 0.17+ (present/ui/hud.rs).
    Screenshot: docs/agent/playtest/2026-08-27/10-neon-call.jpeg.
    *Deliberately NOT fixed in the 2026-09-07 refactor pass — take the second option, not
    the first.* Item 29's `upgrade/bevy-0.16` branch has **already** moved banner show/hide
    off the 0.15 `hidden_tint` alpha trick to `Visibility` toggling with a painted debut,
    and its own note says to keep that either way. So this is solved there. Doing the
    theme-dependent-alpha variant on `main` now would (a) add a way to get `hidden_tint`
    wrong on the single most dangerous wasm invariant we have — alpha 0 at first extract
    culls the subtree permanently — for a cosmetic nit on one theme, and (b) be discarded
    by that branch on merge. Close this together with 29.
    *2026-09-25: no longer solved by 29.* The branch's `Visibility` banner was a workaround
    from before the real root cause (the ticking `ResMut`) was found, and main's banner had
    since grown the follow-up line, pitch-speed read-out and result-beat holds around the
    `hidden_tint` idiom — so the catch-up merge kept main's pill and ported only the
    deadline rule. The ghost rectangle stays open; the `Visibility` route is still the right
    fix, now as its own small change on top of the merged branch.

## Refactor follow-ups (from the 2026-09-07 Clean Code pass)

98. [ ] nice refactor — **Deliberate non-actions; do not "fix" these.** The pedantic
    line-count lint still flags four functions that are correct as they stand, and a
    future pass should not churn them: `ui::hud::spawn_hud` (185), `menu::build_menu`
    (140), and `gear::dress_rigs` (137) are linear declarative trees with near-zero
    branching — splitting scatters a layout that reads top-to-bottom; and
    `variant::Ruleset::diff_literal` (cognitive complexity 30) is ~40 invocations of the
    `diff!` macro that already collapsed its duplication, i.e. data, not logic. If any
    of them ever grows real *branching*, that is the moment to revisit — not the line
    count. Recorded because two of these were considered and rejected in TADA 84-89.
99. [x] nice refactor — `sim::coach::observe` (151 lines) and `subs::update_board` (112)
    are the two remaining over-length functions with genuine branching. `observe`
    already uses `SystemParam` bundles (`WorldFacts`/`PlayReports`/`WorldRigs`), so the
    win there is splitting the per-check snapshot assembly from the dispatch, mirroring
    what `core::coach` now does across `mod.rs`/`checks.rs`. `update_board` is a paint
    loop that could take the same treatment as the overlay helpers in TADA 85. Neither
    is urgent; both are well covered by tests if picked up.
    *Done 2026-09-08 — see TADA 92, 93.* The pedantic line-count list now contains only
    the four deliberate non-actions recorded in 98.

## Reference-video review 2026-09-15 (SMB3 batting presentation)

Frame-level watch of the user's reference (Super Mega Baseball 3, 0:10–3:00). Full notes,
measured beat timings, and the plan detail: `docs/agent/SMB3-REFERENCE-NOTES.md`. The
user's complaint — "no time to observe the hit or miss" — is a *readability* problem
(camera + missing feedback + a shapeless beat), not a duration problem: SMB3's per-pitch
beats are ≤1 s. Order: 100 → 101 → 102 → 103 → 104.

100. [x] high camera — **SMB3 batting view as the default duel view** (Plan A). Retune
    `BattingZoom` / add `DuelView::BehindBatter`: behind and beside the box, eye ≈
    `(0.9, 1.6, -2.6)`, 55–60° vFOV, pitcher ~35% down, zone ~55%, plate ~85%, whole
    batter in frame; catcher/umpire stay hidden (the 4 m cone already does it). Default
    for every mode; `V` still cycles; pin the composition in `framing.test.rs`.
    *Done 2026-09-15 — see TADA 96.*
101. [x] high flow/ui — **Shape the take/miss beat** (Plan B): EARLY/LATE stamp on
    `Whiff` (today `show_contact_stamp` returns `None`), a pitch-mph pill, per-outcome
    pause table in `PaceTuning` (ball 0.8 / strike 1.0 / foul 1.6 / K 2.5), banner ≤
    pause, and a 0.25 s dip-to-black at Result end for strikes/fouls that hides the mound
    reset. Dip node painted opaque at game start (wasm alpha-0 invariant) — free fade-in
    on PLAY BALL. Coach `result_stuck` must read the per-outcome value.
    *Done 2026-09-15 — see TADA 97.*
102. [x] high camera — **Cut-based shot list after contact** (Plan C): `Shot` enum +
    `rig.cut()`; cut at ≤0.25 s (`BALL_FOLLOW_DELAY` 1.0 → 0.25, `RUN_OUT_DELAY` 0.15 →
    0.25); ball-follow from behind the mound for flies / high home shot for grounders;
    fielder cam on catchable flies; base cam on `Thrown { base }` so SAFE!/OUT lands in
    that shot; result hold out 1.5 s / hit 2.0 s (+1 s score text). "OUT #n" banners.
    *Done 2026-09-15 — see TADA 98 (the out/hit hold split and "OUT #n" text deferred to 104).*
103. [x] nice flow/ui — **Wipe + walk-up card between plate appearances** (Plan D): ~1 s
    theme wipe, close plate cam with catcher/umpire visible, batter card + fidget, dismiss
    on confirm or 3 s auto (autoplay/Director must not stall); all resets behind the wipe.
    *Done 2026-09-15 — see TADA 99 (the curtain stands in for a themed wipe).*
104. [x] nice present — **Strikeout reaction cam + score text** (Plan E): side-on plate
    cam from the 1B side, OUT #n + LATE/EARLY + mph, batter walks off ≈2.5 s; "1 - 0"
    score text ≈1 s after SAFE! when a run scores. Umpire gesture clips demoted to nice
    (invisible in the batting view).
    *Done 2026-09-15 — see TADA 100 (walk-off and "OUT n" follow-up included). The umpire
    clips and the themed wipe followed on 2026-09-17 — TADA 102, 103. Nothing left open.*
105. [x] high balance — **`balance_sim` is pacing-sensitive; HR/9 sits at its ceiling.** The
    CPU's decision noise is seeded from `Time::elapsed_secs` (`sim/ai.rs`: pitch aim/kind,
    swing timing, steal rolls), so *any* change to the game's pacing reshuffles every later
    draw. Lengthening the strikeout hold from 1.8 s to 2.2 s (2026-09-15) reshuffled the N=40
    harness from green to HR/9 = 3.38 against the 3.2 ceiling (K% 17.8, runs/9 4.73 fine)
    while every other change of the day passed five full runs; restoring 1.8 s restored green.
    Two fixes, both per the harness's own comment ("if it ever trips, the fix is an HR-retune
    ticket, NOT a wider band"): (a) seed the CPU noise from a per-pitch counter (pitch number
    + inning + batter slot) instead of the wall clock, so presentation pacing can never
    change outcomes — then re-run the harness across a few seeds; (b) an HR retune on the
    CPU-side levers (`cpu_timing_spread_ms`, the `ai.rs` launch-aim distribution) so HR/9
    lands mid-band with headroom. Do (a) first; it is what the `tune-balance` skill's
    "outcomes, not wall time" promise assumes. Until then, do not change `PaceTuning` holds
    or `walkup_secs` without a `balance_sim` run.
    *Done 2026-09-17 — see TADA 101: (a) shipped; (b) not needed, the reseeded draws land
    mid-band (K% 15.8 / runs 3.71 / HR 2.14) and are identical at a 1.8 s and a 2.2 s hold.*
