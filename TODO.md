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
