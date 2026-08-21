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

1. [ ] ship-blocker readability — The live ball is nearly invisible in outfield chase framing (tiny pale dot against pale grass/wall; Midnight Neon fielders wash out too). Screenshot: 08-wall-bang-landing-ring.png. Proposed fix: ball glow/outline or a stronger flight trail while `Phase::InPlay` in `src/game/present/fx/`, plus a contrast pass on `core/theme.rs` field palettes.
2. [ ] polish hud-legibility — Hidden/emptied HUD containers stay visible as dark rounded boxes against the black sky (emptied TR pitching panel during play, floating banner "pill" top-center). Screenshots: 04, 08. Proposed fix: hide via `Display::None`/`Visibility` on children of painted roots rather than empty-but-painted, honoring the wasm rule in `src/game/present/ui/`.
3. [ ] polish outcome-clarity — The called-zone box renders as a ground-to-chest column instead of the knees-to-letters band, so called strikes read wrong. Screenshots: 04, 06. Proposed fix: verify the zone quad's Y extent against `rules::ZONE_*` in `src/game/present/field/` (zone overlay).
4. [ ] polish camera-intent — The plate umpire's helmet/shoulders clip through the catcher-POV camera as large brown blobs at the frame bottom. Screenshot: 16 (inline; reproduce at plate with runners). Proposed fix: add the plate umpire to the occlusion-hide set for CatcherPov or push `FieldSpec::duel_eye` forward in `src/game/present/camera/`.
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

12. [ ] ship-blocker robustness — No panic hook: a runtime wasm panic leaves a frozen canvas with nothing in-page (`grep console_error_panic_hook src/` is empty; `web/index.html` only catches the init throw). Proposed approach: install a `std::panic::set_hook` at wasm startup (feature-gated `console_error_panic_hook` crate, or a hand-rolled hook) that logs to console AND posts a message the page listens for; `index.html` swaps the canvas for the existing `#loading` card restyled as "Something broke — reload" with the panic text. One small `[target.'cfg(target_arch = "wasm32")'.dependencies]` addition plus ~20 lines of JS; verify by force-panicking behind a debug key.
13. [ ] ship-blocker robustness — No WebGL context-loss handling: losing the WebGL2 context (GPU reset, tab pressure) silently freezes the canvas (`webglcontextlost` appears nowhere in `web/`). Proposed approach: in `index.html`, `canvas.addEventListener('webglcontextlost', ...)` showing the same reload overlay as item 12 — Bevy 0.15 cannot recreate the context, so an honest "reload" prompt is the correct minimum; test via `WEBGL_lose_context.loseContext()` in devtools.
14. [ ] ship-blocker platform — No mobile/touch stance: on a phone the canvas loads and silently ignores all input (no touch handlers, no detection anywhere in `web/` or `src/`). Proposed approach: cheap detection in `index.html` (`'ontouchstart' in window && !navigator.keyboard`-style heuristic) showing a styled notice — "Breakneck Baseball needs a keyboard or gamepad — play it on a desktop" — over the canvas, dismissible for gamepad-on-tablet users. Full touch controls stay a separate future feature.
15. [ ] ship-blocker load — 43 MB raw wasm with an indeterminate spinner: the 8 MB compressed download shows no progress, and past the download sits several seconds of silent wasm compile (`web/index.html` spinner only). Proposed approach: fetch the `.wasm` manually with a streamed `ReadableStream` reader accumulating bytes against `Content-Length` to drive a real progress bar, then hand the bytes to `init(bytes)`; label the tail phase "compiling…". Pair with a `wasm-opt -Oz` step in `pages.yml` after wasm-bindgen (typically −10–20 %) and adopt a stated budget: ≤ 30 MB raw / ≤ 10 MB compressed (25 Mbps loads 10 MB in ~3.5 s).
16. [ ] polish release — `pages.yml` has no post-deploy smoke test: a deploy that ships a broken/missing artifact passes silently. Add a job after `deploy` that curls the page URL and `out/breakneck-baseball_bg.wasm` (expect 200, content-type `application/wasm`, byte size matching the built artifact).
17. [ ] polish release — `README.md` is badly stale: it describes a 6-file `src/game/`, Space-as-only-pitch, and no live URL. Rewrite against the current layer map, five-pitch/steal/duel-view controls, and link the Pages deploy.
18. [ ] polish release — License files are missing: `Cargo.toml` declares `MIT OR Apache-2.0` but neither `LICENSE-MIT` nor `LICENSE-APACHE` exists at the repo root. Add both standard texts.
19. [ ] polish release — `src/game/.DS_Store` is committed and `.DS_Store` is not gitignored. `git rm --cached src/game/.DS_Store` and add `.DS_Store` to `.gitignore`.
20. [ ] polish robustness — Browser focus loss is unhandled: no `visibilitychange` hook; Bevy's `Time` max-delta clamp probably prevents ball teleportation after a backgrounded tab, but it is unverified, and the game keeps consuming a turn the player isn't watching. Auto-pause (reuse the subs-board pause path when the ball is dead; freeze `Time<Virtual>` otherwise) on visibility loss.
21. [ ] polish performance — MSAA and shadow-map settings are Bevy defaults (no `Msaa` or shadow config anywhere in `src/`); on WebGL2 that's an accidental cost. Choose explicitly (e.g. `Msaa::Sample4` native / lower on wasm, shadow map sized for one directional light) and record the fps delta from the debug F1 readout.
22. [ ] polish input — Gamepad parity is broad (aim/action, menu, pause, settings, camera all bound) but the substitution board's slot/bench navigation has no verified pad path (`subs.rs` `board_controls`; a code comment notes no test presses a `GamepadButton`). Verify with hardware and bind D-pad + South/Start if missing.
23. [ ] polish accessibility — No reduced-motion option: hit-stop, slow-mo, camera thump, and fireworks have no off switch (`juice.rs` has `JuiceDisabled` as a headless resource, not a setting). Surface a "reduce motion" toggle in `meta/settings/` that inserts/removes `JuiceDisabled` and mutes the camera kick.
24. [ ] nice robustness — Settings schema: new fields are `#[serde(default)]`-covered with legacy-store tests, and a corrupt store falls back to defaults (never bricks) — but a failed parse silently resets ALL settings, and there's no version field for future renames. Add a `version` field at the first breaking change; consider keeping the unparseable blob under a `.bak` key for recovery.
25. [ ] nice release — No version visibility: `Cargo.toml` is 0.1.0 and nothing in-game shows it. Print `env!("CARGO_PKG_VERSION")` small on the menu (`meta/menu.rs`) so bug reports can name a build.
26. [ ] nice accessibility — The mid-flight pause refusal is silent; flash the existing banner ("PLAY IN PROGRESS") so the Esc press feels acknowledged (`meta/subs.rs` refusal path → `BannerTone`).
27. [ ] nice accessibility — Colour-blind check: pink-vs-light-blue built-ins are plausibly deuteranopia-safe and jerseys carry names/numbers, but nobody has run the devtools deficiency emulation; do so for both themes and record it in the theme docs.
28. [ ] nice load — Add a favicon to `web/` (two 404s on every load; also makes the tab identifiable).

## Engine upgrade

29. [ ] nice engine — Bevy 0.15.3 / bevy_rapier3d 0.28 → latest is Bevy 0.19.1 / rapier 0.35 (four majors). Recommendation: **ship first, upgrade after** the production-readiness ship-blockers (items 12–15) — then do it as four sequential gated migrations (~4–5 sessions), not one jump. Full analysis: `docs/agent/BEVY-UPGRADE-ASSESSMENT.md`.
