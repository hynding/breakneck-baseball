# Production Readiness Checklist — Web-First (GitHub Pages)

Each item: **what** to check, **how**, and the **pass bar**. Run the whole list before a
release; append findings to `TODO.md` with severity tags (`ship-blocker` / `polish` / `nice`).

## Load & size

- **Wasm binary size.** How: build `cargo build --profile wasm-release --target wasm32-unknown-unknown`,
  run wasm-bindgen, then `ls -l web/out/*.wasm` and `gzip -k9` / `brotli -k` a copy to measure
  compressed size. Pass: ≤ 30 MB raw and ≤ 10 MB compressed (a mid-tier connection at 25 Mbps
  loads 10 MB in ~3.5 s; past that, bounce risk climbs). Record the number in the audit.
- **wasm-opt in the pipeline.** How: read `.github/workflows/pages.yml` for a `wasm-opt -Oz`
  step between build and bindgen (order matters: wasm-opt the bindgen *output*). Pass: present,
  or an explicit decision not to (record the measured delta; typically 10–20 % on Bevy).
- **Compression actually served.** How: `curl -sI -H 'Accept-Encoding: br, gzip' <pages-url>/out/breakneck-baseball_bg.wasm | grep -i content-encoding`.
  Pass: `br` or `gzip`. GitHub Pages compresses common types but verify wasm specifically.
- **Loading screen with real progress.** How: read `web/index.html`. Pass: the fetch is streamed
  with a progress indicator (bytes loaded / total from `Content-Length`), not just an
  indeterminate spinner — an 8–30 MB download with no progress reads as frozen.
- **Time-to-first-frame, throttled.** How: Chrome DevTools MCP `emulate` (Fast 3G / 4x CPU),
  hard reload, measure to first canvas frame. Pass: interactive < 30 s on Fast 3G; loading UI
  visible and honest the whole time.

## Robustness

- **Panic surface.** How: grep for `console_error_panic_hook` / `panic::set_hook` in `src/`;
  confirm a runtime panic shows an in-page message, not a frozen canvas (test by forcing a
  panic in a debug wasm build). Pass: hook installed at wasm startup and `index.html` (or the
  hook) surfaces the message to the user with a reload prompt.
- **WebGL context loss.** How: grep `web/index.html` and `src/` for `webglcontextlost`; in the
  browser, devtools → `canvas.getContext('webgl2').getExtension('WEBGL_lose_context').loseContext()`.
  Pass: at minimum an overlay prompting reload; silent freeze fails.
- **Audio autoplay unlock.** How: load the page fresh (no prior interaction), start a game with
  a keypress, listen (or inspect `AudioContext.state` via devtools). `audio.rs` claims the menu
  keypress satisfies the gesture gate — verify Bevy actually resumes the context on it.
  Pass: crowd murmur audible after the first gesture; no console autoplay warnings.
- **Settings schema versioning.** How: read `src/game/meta/settings/mod.rs`. Current state:
  bad stores fall back to defaults (never bricks), newer fields are `#[serde(default)]`, and
  legacy-store tests exist. Pass: every *new* field gets `serde(default)` + a legacy-store
  test; a `version` field is the bar once a breaking rename ever happens — until then the
  default-fallback is acceptable (a failed parse silently resets all settings; note the UX cost).

## Input & platform

- **Mobile/touch stance.** How: grep for touch handling; load on a phone or devtools device
  emulation. Pass: either touch controls, or user-agent/touch detection showing a clear
  "desktop + keyboard/gamepad" notice — not a silently unresponsive canvas.
- **Gamepad parity.** How: `grep -rn GamepadButton src/game/` and compare against every
  keyboard binding in `meta/input.rs` + menu/settings/subs/camera keys. Pass: every reachable
  action has a pad path (aim/action, menu nav, pause, view cycle, settings). Verify the subs
  board specifically.
- **Controls reference outside the pause dialog.** How: check menu/HUD for a controls listing
  reachable before starting a game. Pass: a new player can learn the controls without pausing
  mid-play (menu screen, HUD hint, or README/page text under the canvas).
- **Browser focus loss.** How: start a pitch, switch tabs, return. Bevy wasm throttles rAF in
  background tabs — virtual time may jump. Pass: the game auto-pauses (or clamps the step) on
  `visibilitychange`/focus loss; a ball teleporting across the field on return fails.

## Performance

- **Frame-time readout.** How: debug builds have `FrameTimeDiagnosticsPlugin` (F1 panel).
  Pass: 60 fps sustained during a full play (pitch → hit → chase → throw) on a mid-tier
  integrated GPU; record the wasm number separately — WebGL2 is the slow path.
- **Per-frame allocations.** How: audit `present/fx/` (particles), `present/jersey.rs`
  (textures/materials — cached per player by design), `present/field/textures.rs` (startup
  only by design). Pass: no `Image`/`StandardMaterial` created per frame; handles cached.
- **Hot-query filters.** How: review `Changed<T>`/`Added<T>` on recolour, jersey, HUD-mirror
  systems. Pass: mirror systems are change-detected, not every-frame rewrites.
- **Rapier cost.** How: count colliders in a live scene (debug State tab); confirm SIMD is
  native-only (wasm builds non-SIMD — expected). Pass: collider count stable across plays
  (no leak from FX/balls), physics step within frame budget on wasm.
- **Shadows & MSAA on wasm.** How: grep for `Msaa`, shadow settings; Bevy defaults are
  MSAA 4x + 2048 shadow maps. Pass: an explicit, tested choice for WebGL2 — not defaults
  inherited by accident.

## Release process

- **Post-deploy smoke test.** How: read `.github/workflows/pages.yml` for a step after
  deploy that curls the live URL, checks the `.js` and `.wasm` are reachable (200) and the
  wasm byte size matches the built artifact. Pass: present and failing loudly.
- **README matches reality.** How: read `README.md` against the current tree. Pass: layer
  layout, current controls (five pitches, V/T/I/S/Esc), features list, and a link to the
  live Pages URL.
- **Version visibility.** How: `Cargo.toml` `version` bumped per release; the menu or HUD
  shows `env!("CARGO_PKG_VERSION")`. Pass: a bug report can name the version it saw.
- **License files.** How: `ls LICENSE*`. Pass: `Cargo.toml` says `MIT OR Apache-2.0`, so both
  `LICENSE-MIT` and `LICENSE-APACHE` exist at the repo root.
- **No junk files.** How: `git ls-files | grep -i ds_store` (and similar). Pass: none
  committed; `.DS_Store` in `.gitignore`.

## Accessibility baseline

- **Pause availability.** Pass: Esc/P (pad Start) works whenever the ball is dead (current
  design refuses mid-flight — acceptable, but the refusal should be visible, not silent).
- **Reduced motion.** How: check settings for a toggle covering hit-stop/slow-mo
  (`juice.rs`), camera thump, fireworks. Pass: a "reduce motion/flash" setting exists.
- **Colour-blind-safe teams.** How: check each `Theme`'s two `PlayerTemplate` palettes for
  red/green-only separation; jerseys carry names/numbers (helps). Pass: teams distinguishable
  under deuteranopia simulation (devtools rendering emulation) in at least one built-in theme.
- **HUD text size.** How: screenshot at 1366×768 (typical laptop). Pass: count/score/inning
  legible without leaning in; banners readable at a glance.
