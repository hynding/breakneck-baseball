# TODO

NOTE: Everything that has been completed gets moved to TADA.md

## Playtest review 2026-08-20

Browser pass on the `wasm-release` build (Midnight Neon theme, 1-inning 1P game driven via
Chrome DevTools; screenshots in `docs/agent/playtest/2026-08-20/`). Console was clean for the
whole session — no panics or warnings. Verified working end-to-end: steal 2nd→3rd on the pitch,
wall carom → conceded double (decided-at-throw), bases-loaded walk-off firing GameOver only
after the play finished, pause gating, GAME OVER → menu restart. Not captured (blind swing
timing over automation; Scenario tab is native-only): Perfect-contact hit-stop, HR fireworks
show, a turned force play — a native `--features debug` pass should cover those three.

5. [ ] polish visual-consistency — Between pitches the batter parks the bat vertically behind his back, reading as a detached/floating bat, and jersey shoulder-number quads float off the torso during the stance lean. Screenshots: 09 (floating quads), 13/16 (bat). Proposed fix: re-aim the idle/stance grip in `src/game/present/animation/` poses and re-anchor number quads in `src/game/present/jersey.rs`.
6. [ ] polish hud-legibility — The settings card is translucent enough that menu text bleeds through and collides with its labels. Screenshot: 03-settings-screen.png. Proposed fix: raise the card `BackgroundColor` alpha in `src/game/meta/settings/screen.rs` (keep it nonzero per the wasm rule).
7. [ ] polish visual-consistency — Unexplained floating tan ring near the batter's chest during a result pause at the plate. Screenshot: 16 (inline). Proposed fix: identify the owning marker (pickoff-reload ring? on-deck marker?) in `src/game/present/fx/` and anchor or hide it.
8. [ ] nice theme-consistency — Chest number "44" wraps onto the torso's side face at some angles. Screenshot: 14 (inline). Proposed fix: nudge the chest quad outward/inward in `src/game/present/jersey.rs`.
9. [ ] nice readability — In catcher POV the batter occupies ~a third of the frame at 2560-wide; consider a small lateral offset of `FieldSpec::duel_eye` so the zone and pitcher both clear the batter's silhouette (`src/game/core/variant.rs` per-variant framing).
10. [ ] nice timing — A passive human batter induces long CPU walk chains (seven walks in one observed inning, two walked-in runs); arcade pacing may want the CPU pitcher's zone rate raised against non-swinging batters (`Ruleset`/`sim/ai.rs` — retune via `tests/balance_sim.rs` only).
11. [ ] nice audio — Audio events presumed firing (crowd/cracks synthesized at startup; `audio.rs` gesture note) but unheard over automation; a fresh-load listen on web should confirm the crowd loop starts after the first menu keypress (autoplay unlock).

## Production readiness audit 2026-08-20

Web-first audit per `.claude/skills/production-readiness/reference/checklist.md`. Measured this
session: wasm-release binary **43.25 MB raw / 7.96 MB gzip / 5.57 MB brotli** (live Pages serves
gzip, 8.23 MB wire); full `cargo test` green in 7:02; console clean through a full browser game.
Not verified this session (no ear/GPU-meter/phone): audio audible on first gesture, sustained
wasm fps, touch hardware behavior, 1366×768 HUD legibility.

16. [ ] polish release — `pages.yml` has no post-deploy smoke test: a deploy that ships a broken/missing artifact passes silently. Add a job after `deploy` that curls the page URL and `out/breakneck-baseball_bg.wasm` (expect 200, content-type `application/wasm`, byte size matching the built artifact).
17. [ ] polish release — `README.md` is badly stale: it describes a 6-file `src/game/`, Space-as-only-pitch, and no live URL. Rewrite against the current layer map, five-pitch/steal/duel-view controls, and link the Pages deploy.
18. [ ] polish release — License files are missing: `Cargo.toml` declares `MIT OR Apache-2.0` but neither `LICENSE-MIT` nor `LICENSE-APACHE` exists at the repo root. Add both standard texts.
20. [ ] polish robustness — Browser focus loss is unhandled: no `visibilitychange` hook; Bevy's `Time` max-delta clamp probably prevents ball teleportation after a backgrounded tab, but it is unverified, and the game keeps consuming a turn the player isn't watching. Auto-pause (reuse the subs-board pause path when the ball is dead; freeze `Time<Virtual>` otherwise) on visibility loss.
21. [ ] polish performance — MSAA and shadow-map settings are Bevy defaults (no `Msaa` or shadow config anywhere in `src/`); on WebGL2 that's an accidental cost. Choose explicitly (e.g. `Msaa::Sample4` native / lower on wasm, shadow map sized for one directional light) and record the fps delta from the debug F1 readout.
22. [ ] polish input — Gamepad parity is broad (aim/action, menu, pause, settings, camera all bound) but the substitution board's slot/bench navigation has no verified pad path (`subs.rs` `board_controls`; a code comment notes no test presses a `GamepadButton`). Verify with hardware and bind D-pad + South/Start if missing.
23. [ ] polish accessibility — No reduced-motion option: hit-stop, slow-mo, camera thump, and fireworks have no off switch (`juice.rs` has `JuiceDisabled` as a headless resource, not a setting). Surface a "reduce motion" toggle in `meta/settings/` that inserts/removes `JuiceDisabled` and mutes the camera kick.
24. [ ] nice robustness — Settings schema: new fields are `#[serde(default)]`-covered with legacy-store tests, and a corrupt store falls back to defaults (never bricks) — but a failed parse silently resets ALL settings, and there's no version field for future renames. Add a `version` field at the first breaking change; consider keeping the unparseable blob under a `.bak` key for recovery.
25. [ ] nice release — No version visibility: `Cargo.toml` is 0.1.0 and nothing in-game shows it. Print `env!("CARGO_PKG_VERSION")` small on the menu (`meta/menu.rs`) so bug reports can name a build.
26. [ ] nice accessibility — The mid-flight pause refusal is silent; flash the existing banner ("PLAY IN PROGRESS") so the Esc press feels acknowledged (`meta/subs.rs` refusal path → `BannerTone`).
27. [ ] nice accessibility — Colour-blind check: pink-vs-light-blue built-ins are plausibly deuteranopia-safe and jerseys carry names/numbers, but nobody has run the devtools deficiency emulation; do so for both themes and record it in the theme docs.

## Engine upgrade

29. [ ] nice engine — Bevy 0.15.3 / bevy_rapier3d 0.28 → latest is Bevy 0.19.1 / rapier 0.35 (four majors). Recommendation: **ship first, upgrade after** the production-readiness ship-blockers (resolved 2026-08-20 — TADA Batch 3) — then do it as four sequential gated migrations (~4–5 sessions), not one jump. Full analysis: `docs/agent/BEVY-UPGRADE-ASSESSMENT.md`.

## Follow-ups from the 2026-08-20 fix session

30. [ ] polish gameplay — Fielder set-spot drift: after plays involving base coverage, a fielder can end up parked at the wrong set spot for subsequent at-bats (observed: CRUZ #12 standing at home plate through several at-bats after a play at the plate; a fielder on the infield grass off any spot — screenshots in session log, browser game 2). Not caused by the presentation fixes (they cannot move rigs). Reproduce natively (`cargo run --features "dev debug"`, State tab) and check `sim/fielding.rs` set-spot re-assignment after covering.
31. [ ] nice fx — Verify `BallHalo` scale clamps in the browser: one sighting of an oversized halo disc shortly after contact (screen size inconsistent with the `dist·0.007` clamp math). If confirmed, consider a screen-space cap or tighter `HALO_MAX` in `src/game/present/fx/particles.rs`.
32. [ ] nice load — wasm-release raw size is ~43 MB before the CI `wasm-opt` pass (budget ≤ 30 MB raw / ≤ 10 MB compressed; gzip wire size 7.96 MB passes). Measure the deployed post-wasm-opt size, then pursue Bevy default-feature trimming for the rest.
