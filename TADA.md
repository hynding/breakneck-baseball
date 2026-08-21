# TADA

1. [x] The catcher should be catching pitched balls that aren't hit. — `flow::catcher_receives` stops clean takes/whiffs in the mitt (glove-up + pop); balls in the dirt (dropped third) still get away.
2. [x] The current play, including a home run, should have ended before the next batter is up to bat. — the result pause now waits for every runner rig to finish its path (`runner::RunnersSettled`), trot included, with a 20 s safety cap.
3. [x] At the end of a play, steal or attempted steal, if there is still a runner on any base. Wait 5 seconds to give the opportunity to make an attempt to steal. — the pre-pitch steal window (`Ruleset::steal_window_secs`, 5 s) gates the pitch whenever a runner is actually in a position to steal; a pickoff out takes the normal result pause, after which runners still aboard earn a fresh window.
4. [x] Runners on base should understand the concept of leading off. There should be controls to steal. Likewise, there should be controls for picking off players on base. — runners take a real leadoff (walk off the bag); offense holds Down to stretch it (guaranteed steal jump on the pitch), defense presses action during the window to throw over (`rules::attempt_pickoff`: out only if the lead was stretched). The CPU plays both sides.
5. [x] When the ball is hit into the air, there should be a contextually sized ring directly underneath for better indication where and when it will land. — `fx.rs` landing ring at the live-predicted touchdown, radius shrinking with remaining hang time.
6. [x] There should be umpires. — data-driven `FieldSpec::umpire_positions` crew in fixed blacks (4 in the stadium, 1 on the lawn); the plate umpire crouches through the duel.
7. [x] There should be controls for throwing a variety of pitches. — five-pitch arsenal by held-aim octant: up fastball, down curveball, left slider, right sinker, neutral changeup (duel-panel legend updated).
8. [x] There should be a roster of players on each team, with alternates for substitutions. There should be methods to stop the game to make them at any time when the ball is not in play. — `roster.rs` (9 starters + 4 bench per team); Esc/P (gamepad Start) between plays opens the substitution board (`subs.rs`).
9. [x] Team members should have names and numbers assigned to them, with the last name and number appearing on the back of their jersey (and number on front and shoulders) like typical baseball jerseys. — `jersey.rs` draws each card with a built-in 5×7 bitmap font into runtime textures: name + number on the back, number on chest and both shoulders.
10. [x] We should be prepared to use different player models, including humanoid-like ones, that have realistic running, swinging, catching, diving, sliding, throwing etc. animations. — `Theme::player_model` (`PlayerModelId`) seam in the rig builder, plus new `Dive`/`Slide` clips and a `root_pitch` body-lean channel; a humanoid model plugs in as a new arm with richer poses behind the same `AnimClip` names.

## Batch 2

11. [x] Maintain a document of research you've gathered on baseball; reference it and run more research online when needed. — `docs/BASEBALL.md`: web-sourced field/mound/base/groundskeeping specs with sources, mapped to the code that models them; CLAUDE.md points to it as the convention for future features.
12. [x] The batter should be facing the plate. — the batter rig now stands side-on to the pitcher in the box, facing home plate (−X), per the batter's-box notes in docs/BASEBALL.md.
13. [x] The catcher should catch the ball if it's a ball or a (non-foul ball) strike. — already in place from `flow::catcher_receives` (takes and swinging strikes end in the mitt; fouls come off the bat and stay live; the dropped third stays loose in the dirt); verified in the browser.
14. [x] The player-at-bat/pitcher-ready view needs to be from the catcher's point of view. — the duel camera now sits just over the crouched catcher's helmet (eye ~2.3 m, right behind the plate), strike zone floating above his head, delivery coming straight in; verified with browser screenshots on the wasm build.
15. [x] We need to see the batter swing the bat. — new `AnimClip::BatterSwing` drives the batter's arms through the swing alongside the bat-pivot sweep, and the run-out rig now waits hidden through a short `RunDelay` (0.4 s; 0.9 s on homers) so the real batter visibly finishes the follow-through before the swap — fouls no longer make him vanish at all.
16. [x] Don't follow the ball until 1 second after the batter has made contact with it. — `camera::BALL_FOLLOW_DELAY` holds the plate framing for the first second after contact before cutting to the ball.
17. [x] There should be an umpire behind the catcher. — already in place (`FieldSpec::umpire_positions`, plate umpire crouching behind the catcher); kept behind the new catcher-POV lens.
18. [x] There should be a pause button ("P" / gamepad Start) that brings up a pause menu. — already in place via `subs.rs`; the board is now titled PAUSED with the substitutions section beneath, verified rendering on the wasm build.
19. [x] There should be a pitcher's mound. — upgraded to regulation per docs/BASEBALL.md: 18 ft diameter, 10 in high, a low skirt approximating the 1-in-per-foot slope, and the white 24×6 in pitching rubber.
20. [x] The bases need to be more defined. — regulation 18 in bags (2023 rule), raised with a touch of glow, plus the ~13 ft dirt cutout circles at each bag and around home plate.
21. [x] The outfield and infield should have more realistic textures. — procedural runtime textures (no asset files): mow-striped grass on the outfield and lawn, speckled clay on the dirt diamond/basepaths/mound, with a grass infield inside the dirt basepath band.
22. [x] Player models should be humanoid-like with realistic animations, not blocky rigs. — a skinned glTF player model (`src/game/models/player.glb`) built by `tools/build_player.py` → `assets-src/player.blend` → `tools/export_glb.py`, embedded via `embedded_asset!` and pinned by `tests/model_contract.rs` against `model_assets::CLIP_TABLE`; `animation.rs` now drives Bevy's `AnimationPlayer`/`AnimationGraph` with 150 ms cross-fades behind the same `AnimClip`/`Playing`/`MoveIntent` API (the old procedural `Blocky` rig stays as a fallback arm); `--features dev` file-watches the `.glb` for Blender hot-reload.
23. [x] Batters start rounding the bases as soon as they hit the ball; runners on base run intelligently by outs and fly-out risk. — the run-out rig activates at contact, and a pure `rules::runner_break` (sourced in docs/BASEBALL.md §Baserunning: two-out go, forced-grounder go, halfway on catchable flies, tag-up on deep) drives `runner.rs` break choreography, with the call/outcome machinery byte-untouched.
24. [x] The controls at the bottom of the screen only show in a dialogue box when paused. — the help text moved into a `ControlsDialog` card under the pause board, painted at spawn per the wasm UI rule.
25. [x] HUD boxes to the four corners (score BR, player BL, pitching TR, on-base TL). — corner anchors at the existing 14 px margins in `ui.rs`.
26. [x] White chalk: batter's-box outlines and foul lines out through first/third. — regulation 4×6 ft boxes and 3 in foul lines painted as flat decals from `foul_line_span`, layered above every ground decal (`field.rs` chalk consts + ordering test).
27. [x] Several at-bat camera views toggled by a button, with the catcher/ump invisible when in the way. — **V** cycles CatcherPov / BehindPitcher / BattingZoom / BroadcastPlate (`DuelView`), with a pure occlusion cone hiding `CatcherRole`/`PlateUmpire` only when they block the active broadcast view.
28. [x] The batter holds the bat correctly, including swinging it to hit the ball. — the `Bat` bone now sits in the hands (rest-gap fixed), a looping `BattingStance` clip poses a coiled two-handed stance through the duel, and `BatterSwing` re-keyed so both hands ride the bat through a level sweep (left arm solved per-frame against the grip).
29. [x] The `src/game` tree was a flat 31-file module with files running past 3,000 lines — reorganize it for maintainability without touching gameplay. — migrated to Rust edition 2024, split into a layered `core`/`sim`/`present`/`meta` hierarchy behind the existing `game::<module>` facade (largest file dropped from 3,177 to 770 lines — `core/rules/resolve.rs`, a named, accepted exception to the ~700-line target), added a repo-wide clippy lint table, hoisted shared geometry/physics constants into `core`, and applied a `#[must_use]` pass on pure `rules::` result types — zero behavior change, `tests/balance_sim.rs`'s offensive economy untouched. See `docs/superpowers/specs/2026-08-19-layered-refactor-design.md`.

## Batch 3 — playtest & production-readiness fixes (2026-08-20)

30. [x] Playtest TODO 1 — the live ball was nearly invisible in outfield chase framing. — `BallHalo` in `src/game/present/fx/particles.rs`: an unlit theme-trail-tinted shell parked on the ball during `Phase::InPlay`, scaled by broadcast-camera distance (≈ screen-constant, clamped ball-size…0.9 m) so the chase reads at range; hidden otherwise.
31. [x] Playtest TODO 2 — hidden HUD duel cards ghosted as dark boxes against the sky on wasm. — root cause: the tint-and-blank idiom left stale glyphs/chrome rendering dimly on wasm/WebGL2 (pixel-sampled at ~10–15 % opacity); `update_duel_panels` now flips root `Visibility` (the pause-board mechanism), belt-and-braces keeping every alpha nonzero.
32. [x] Playtest TODO 3 — "zone box reads as a ground-to-chest column": verified correct, no change. The drawn zone is `ZONE_LOW..ZONE_HIGH` (0.45–1.28 m), plate-wide/deep, pinned by `zone_wireframe_matches_rulebook_dimensions`; the column read was the near-face fill panel + perspective.
33. [x] Playtest TODO 4 — the plate umpire clipped the parked catcher-POV lens during result pauses. — `hide_occluders` now gates on the same `duel_framing_wanted` predicate the broadcast rig holds framing with, and catcher-POV hides both plate rigs outright (the umpire stands up *behind* the eye, where the look-ahead cone can never flag him). No clipping observed across a full browser session post-fix.
34. [x] Audit TODO 12 — wasm panic hook. — `src/main.rs` installs a `panic::set_hook` on wasm that `console.error`s the message and posts `bb-panic:<msg>` to the page; `web/index.html` swaps the dead canvas for a "Something broke — Reload" card with the panic text. Verified via synthetic message (screenshot 13).
35. [x] Audit TODO 13 — WebGL context-loss handling. — `web/index.html` attaches `webglcontextlost` to the Bevy canvas post-init and shows the reload card. Verified via `WEBGL_lose_context` (screenshot 14).
36. [x] Audit TODO 14 — mobile/touch stance. — coarse-pointer-only devices get a dismissible "needs keyboard or gamepad" overlay instead of a silently unresponsive canvas. Verified via device emulation (screenshot 15).
37. [x] Audit TODO 15 — loading progress + wasm-opt. — `web/index.html` streams the wasm fetch with a real progress bar (percent against Content-Length when served raw; MB counter under gzip where decoded-byte percent would lie) and a "Compiling…" stage; `pages.yml` gained a `wasm-opt -Oz` step (bulk-memory + nontrapping-fp flags). Verified under Fast-4G throttle (screenshot 12). Residual: raw size still ~43 MB pre-opt vs the ≤30 MB budget — follow-up filed.
38. [x] Audit TODO 19 — `src/game/.DS_Store` untracked; `.DS_Store` gitignored.
39. [x] Audit TODO 28 — favicon (⚾ emoji SVG data URI) in `web/index.html`.

## Batch 4 — release-polish backlog (2026-08-21)

40. [x] TODO 16-18, 25 — release hygiene. — `LICENSE-MIT` + `LICENSE-APACHE` matching
    `Cargo.toml`'s declared dual license; `README.md` rewritten against the four-layer map,
    the five-pitch/steal-duel controls, the live Pages link, and current build commands;
    `pages.yml` gained a post-deploy smoke-test job (curls the live page and wasm artifact,
    asserting 200 / `application/wasm` / byte size matching the built binary); the menu
    shows `v0.1.0` (`env!("CARGO_PKG_VERSION")`) bottom-right, verified in the browser.
41. [x] TODO 20 — browser focus-loss auto-pause. — tab-hidden (`WindowOccluded`) or focus
    loss arms a pending pause that lands at the first dead-ball moment (reusing the subs
    pause path) instead of freezing `Time<Virtual>` under a live ball, so the juice
    invariant stays intact; regaining focus first disarms it.
42. [x] TODO 22 — subs-board gamepad bindings. — D-pad mirrors the arrows, South swaps,
    North switches team, Start already resumed via `pause_pressed`; hint line updated.
    Hardware pass still owed (TODO 33).
43. [x] TODO 23 — reduce-motion accessibility toggle. — new serde-defaulted
    `Settings::reduce_motion` + REDUCE MOTION settings row; `juice::motion_enabled` gates
    hit-stop/slow-mo and the camera-kick impulses, reading (never writing) the harness's
    `JuiceDisabled`. Verified end-to-end on web incl. localStorage persistence.
44. [x] TODO 24 — settings schema decision. — no version field until a genuinely breaking
    rename (every change so far is additive + serde-defaulted); an unparseable store is now
    preserved under `settings.json.bak` / the `.bak` localStorage key before defaults load,
    with a regression test.
45. [x] TODO 26 — pause-refusal feedback. — a refused mid-play Esc flashes PLAY IN PROGRESS
    through the existing banner channel (`PlayBanner::new` made pub for meta emitters).
46. [x] TODO 21 — explicit Msaa/shadow choices. — camera spawns `Msaa::Sample4` native /
    `Sample2` wasm; `DirectionalLightShadowMap` 2048 native / 1024 wasm (was implicit
    defaults everywhere). Measured wasm: 120 fps display-capped at 2560×1488 on M4 Max.
    Native F1 readout owed (TODO 33).
47. [x] TODO 30 — fielder set-spot drift (the CRUZ-at-home bug). — root cause: on an
    instantly-resolved play (liner caught the same frame covers went out),
    `fielding::return_to_spots` fired while every fielder was still within its already-set
    tolerance, sent nobody home, consumed its one-shot state, and left the cover
    `MoveIntent`s live — the defense then ran to the bases *after* the play and parked.
    Fix: play end voids every outstanding fielding order. New `e2e_fielder_spots`
    regression (full CPU game, no fielder off-spot 3+ consecutive deliveries).
49. [x] TODO 32 + deploy boot regression — wasm-opt sizing and the binaryen fix. — CI
    wasm-opt (first run) cut the deployed binary 43.3 MB → 16.9 MB raw (−61%, inside the
    ≤30 MB budget); gzip wire 5.6 MB (was 7.96 MB, inside ≤10 MB). But apt's binaryen 108
    mangled wasm-bindgen 0.2.126's externref table — the deploy died at init with
    `WebAssembly.Table.grow(): failed to grow table by 4` (the TODO-12 panic surface caught
    it with a clean reload card). `pages.yml` now pins the upstream binaryen `version_132`
    release with `--enable-reference-types`; the exact flag set verified booting locally.
50. [x] TODO 6 — settings card translucency. — the open card now paints `panel_bg` at full
    alpha (theme panels carry ~0.85 alpha for field layering); menu text no longer bleeds
    through. Verified on web.
51. [x] TODO 7 — the "floating tan ring" at the batter's chest. — identified: STONE #21's
    gold-chain gear prop (`meta/gear.rs`), whose 7 cm spine-bone standoff detached visually
    in the stance lean. Re-anchored to (0, 0.24, 0.135); reads as worn gear on both themes.
52. [x] TODO 5 — bat parked vertically behind the back between pitches. — the `Idle` clip
    never keyed the `Bat` bone, so idle frames showed the rest pose (solved for
    raised-arm stances). `Idle` now keys the barrel down-forward — a loose at-the-side
    carry (rebuilt `player.blend`/`player.glb`, `model_contract` green). The
    shoulder-quad "float" sighting resolved with the chain fix (51).
53. [x] TODO 8 — chest number wrapping onto the torso side. — the chest quad standoff
    dropped 0.16 → 0.14 off the spine bone, killing the parallax that read as
    side-face lettering at oblique angles.
54. [x] TODO 9 — catcher-POV batter dominance. — `duel_eye` steps 0.4 m (0.35 m FrontYard)
    toward first base plus a half-step closer to hold the 80–90% batter-height contract;
    the zone box and pitcher now fully clear the batter's silhouette (screenshot
    03-duel-offset-eye). Camera framing tests re-pinned.
55. [x] TODO 27 — colour-blind emulation pass. — deuteranopia + protanopia (SVG
    colour-matrix over the canvas) on both themes: team split stays legible (yellow-olive
    vs lavender), jersey lettering backstops, B/S/O dots are position-labelled. Recorded in
    `theme.rs` module docs; screenshots in `docs/agent/playtest/2026-08-21/`.
56. [x] TODO 31 — BallHalo oversized-disc sighting. — not reproduced across a browser
    session; clamp math re-verified by inspection (dist × 0.007 into [0.05, 0.9] m world
    radius; the multi-camera fallback degrades to the *minimum*, not max). Closed as
    unreproducible; reopen with a screenshot if it recurs.
