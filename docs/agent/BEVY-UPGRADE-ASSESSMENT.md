# Bevy Upgrade Assessment — 2026-08-20

Research only at time of writing; progress log at the bottom. Sources: bevy.org
news/migration guides, dimforge bevy_rapier CHANGELOG (fetched 2026-08-20).

## Current vs latest

| Crate | Locked | Latest (2026-08) | Gap |
|---|---|---|---|
| bevy | 0.15.3 | **0.19.1** (0.19 released 2026-06-19) | 4 majors |
| bevy_rapier3d | 0.28.0 | **0.35.0** (2026-07-12, targets Bevy 0.19) | 7 releases |
| bevy-inspector-egui (debug feature) | 0.28.1 | needs matching bump per Bevy major | — |

Version pairing along the path: Bevy 0.16 ↔ rapier 0.29/0.30 · 0.17 ↔ 0.31/0.32 ·
0.18 ↔ 0.33/0.34 · 0.19 ↔ 0.35.

## Official migration guides (each step is a real migration)

- 0.15 → 0.16: https://bevy.org/learn/migration-guides/0-15-to-0-16/
- 0.16 → 0.17: https://bevy.org/learn/migration-guides/0-16-to-0-17/
- 0.17 → 0.18: https://bevy.org/learn/migration-guides/0-17-to-0-18/
- 0.18 → 0.19: https://bevy.org/learn/migration-guides/0-18-to-0-19/

## What breaks for the APIs this codebase leans on hardest

- **Buffered events → Messages (0.17)** — the single biggest hit here. The whole crate is
  event-driven (`PitchEvent`, `HitEvent`, `LiveBallEvent`, `ContactEvent`, `PlayBanner`,
  `WallBangEvent`, `PitchCaughtEvent`, `ScenarioAppliedEvent`, …): every `Event` derive,
  `EventReader`/`EventWriter` becomes `Message`/`MessageReader`/`MessageWriter`; 0.16 already
  renames `EventWriter::send` → `write`. Mechanical but touches nearly every system file,
  including the e2e harness. bevy_rapier 0.32 made the same migration on its side.
- **Hierarchy & spawning (0.16)** — `Parent` → `ChildOf`, `with_children` closure type change,
  `despawn_recursive()` → `despawn()`. Hits rig construction (`present/player/rig.rs`), jersey
  quads hung off rig roots, and every UI tree (`ui/`, `menu.rs`, `subs.rs`, `settings/screen.rs`).
- **`Query::single` returns `Result` (0.16)** — pervasive small edits across systems and tests.
- **AnimationGraph (0.17, 0.18)** — 0.17 requires re-saving serialized graphs (ours are built in
  code, so likely light), but 0.18 **splits the `AnimationTarget` component** — the glTF
  clip-driver seam in `present/animation/driver.rs` must be re-verified against
  `tests/e2e/model_contract.rs` and the 150 ms cross-fade behavior re-tested by eye.
- **UI internals (0.16–0.18)** — `UiImage` → `ImageNode` (0.16), extraction `z_order` type change
  (0.18). Our UI is `Node`/`BackgroundColor`/text-heavy, so mostly renames — but the
  **wasm/WebGL2 alpha-0-at-first-extract gotcha is undocumented behavior** of the 0.15
  extraction path; whether it still holds after the 0.18 extraction rework must be re-verified
  in the browser (it may even be fixed, which would let us simplify `hidden_tint`).
- **Rapier API (0.28 → 0.35)** — `Velocity` fields renamed (`linvel` → `linear`,
  `angvel` → `angular`) in 0.34; `RapierQueryPipeline` no longer a component (0.31);
  Message-API migration (0.32). Ball spawn/drag/pitch code and the wall colliders touch these.
- **States / `embedded_asset!`** — no headline breaking changes found for `OnTransition`
  schedules or `embedded_asset!` in the four guides, but both are load-bearing here
  (`game_start()`, `model_assets.rs`) and get a dedicated smoke test per step.
- **0.19 renames** — `bevy_scene` → `bevy_world_serialization` (we don't use scenes;
  feature-name fallout only).

## Effort estimate

Four sequential majors, each gated by the full invariant suite (`cargo test` ≈ 7 min, both-target
checks, balance bands, model contract, browser wasm smoke):

- 0.15 → 0.16: **the big one** (hierarchy, Query::single, event rename) — 1–2 sessions.
- 0.16 → 0.17: Message split, pervasive but mechanical — 1 session.
- 0.17 → 0.18: AnimationTarget split + UI extraction rework + wasm-gotcha re-verification — 1
  session, higher risk (rendering/animation eyes needed).
- 0.18 → 0.19: light — 0.5 session.

Total: **~4–5 sessions**, done as separate branches per step with the balance bands as the
behavioral regression gate. Skipping straight to 0.19 in one jump is not cheaper — the
intermediate rapier releases are the only tested pairings, and bisecting a 4-major diff against
a chaotic physics sim is much worse than four clean gates.

## Recommendation

**Ship first, upgrade after.** The production-readiness ship-blockers (panic surface, load
size/progress, audio unlock — see TODO.md) are user-facing and independent of engine version;
0.15.3 + rapier 0.28 is stable and CI-green today. The upgrade's main payoffs (perf work on
newer rendering, ecosystem currency, `enhanced-determinism` options) are real but not blocking.
Do the upgrade as the first majors-long effort *after* the browser release is presentable, before
new feature waves make the diff bigger. One caveat that could flip the order: if a
production-readiness fix needs an upstream Bevy fix that only exists post-0.15 (none identified
so far), upgrade that far first.

## Progress log

- **Step 1, 0.15.3 → 0.16.1** (2026-08-24, commit `a9800e3` on `upgrade/bevy-0.17`):
  hierarchy/`ChildOf`, `Query::single` Results, event renames, rapier 0.30,
  inspector-egui 0.31. All native gates green; wasm blocked on the banner bug below.
  Headless sim ~72% slower than 0.15 (balance run 199 s vs 115 s) — carried forward.
  *Measured 2026-10-09* with `tests/e2e/sim_profile.rs` (bevy-perf skill): one CPU-vs-CPU
  inning at 100 Hz, single-threaded, 4-core cloud container, 8161 frames at 2.3–2.6 ms/frame.
  **340 distinct systems, ~380 system runs per frame, and only 48% of the frame is inside any
  system** — the rest is executor cost (run conditions, command application, change ticks)
  that scales with system count. Inside systems: `bevy_animation` 12% (`animate_targets`,
  skeletal sampling), `bevy_ui` 10% (layout 4%, text measure + layout 4%, every frame),
  `bevy_rapier3d` 5%, `bevy_transform` 4%, the game's own systems **4.8%**. `PostUpdate`
  alone is 57% of the frame. So there is no hot system to fix: 0.17 runs more engine systems
  per frame than 0.15 did (light and camera visibility, picking, UI picking, gizmos are all
  registered headless) and the sim pays the fixed cost of each. Levers, by expected payoff:
  (1) check whether some HUD text is rewritten every frame — text measure + layout at 4% with
  no window suggests so, and it would cost the wasm build too; (2) let the balance harness
  skip skeletal sampling (bone poses are cosmetic — confirm nothing in `sim/` reads a bone
  transform, then check the bands are unmoved); (3) disable plugins the headless app never
  needs (picking, gizmos, light/camera visibility) in the harness, each removing its systems'
  executor cost — after checking which e2e suites rely on them. Re-run the probe after each.
  *Levers tried 2026-10-10* (probe numbers, same container; `cargo nextest run --test e2e
  sim_profile:: --run-ignored only --features profile`):
  **(1) done, shipped** — the duel cards and the Swing Meter fill were repainted every frame
  (11 `Text` + 3 colours + a `Node` dirtied per frame with nothing changing; the probe's
  change-detection counters found them). Guarded writes: `ui_layout_system` 92 → 52 µs/frame,
  `measure_text_system` 50 → 7, `bevy_ui` 10.2% → 5.5% of the frame, the inning 20.5 → 17.7 s
  (**2.5 → 2.16 ms/frame**). Applies to the shipped build too.
  **(2) measured, not adopted** — `HeadlessConfig::skip_skeletal_sampling` strips
  `AnimationTarget` from bones: 2.16 → **1.82 ms/frame** (−16%; `animate_targets` 207 → 11 µs,
  transform propagation 72 → 21, bone `Transform` churn 191 → 19 per frame). But it is not
  outcome-neutral: the same inning ends 45 frames apart; first difference at frame 444, where
  fielders break on contact one frame earlier *with* sampling. Not physics, not a bone read, not
  the stripping system's schedule placement — the open suspect is archetype-order-dependent
  query iteration in a fielding decision (`tests/e2e/skeletal_switch.rs` has the finding, the
  ignored gate test, and a lockstep diagnostic that prints the first divergent frame).
  Attribute that, and the balance sim gets the 16%.
  **(3) not worth it** — picking is ~1% of the frame and drives `Interaction` for the touch UI,
  settings taps, menu and pause board; gizmos 0.4%; light/camera visibility (~3.5%) can't be
  removed without the present layer's asset types. Left alone.
  Net so far: 2.5 → 2.16 ms/frame headless (−14%), 52% of the frame still executor overhead
  across ~380 system runs — the 0.15 → 0.17 system-count story stands.
- **Step 2, 0.16.1 → 0.17.3** (2026-08-25, this commit): Messages API crate-wide,
  `BorderColor::all`, `Justify`, `SystemCondition`, `bevy::light`, `WindowResolution(u32)`,
  rapier 0.32, inspector-egui 0.34. Clippy `-D warnings` green on default/dev+debug/autoplay;
  full suite + both-target checks green; browser wasm verified (menu → game → banners → outs).
- **The wasm banner bug, root-caused** (blocked both 0.16 and 0.17 browser gates for a week of
  probe builds): a system that ticks a `ResMut<Timer>` **every frame** while also holding
  `&mut` queries on a UI entity prevents that entity from ever being extracted on
  wasm/WebGL2 — ECS-side `Visibility`/`InheritedVisibility` stay correct, native renders
  fine, and the identical system with an empty body (same params, registered) is harmless.
  Bisected via a 2×2 (show/fade on/off) plus an empty-body ghost. Every earlier data-shaped
  theory (alpha, markers, position, spawn order, fonts, borders) was a phantom correlation.
  Fix: `BannerFadeAt`/`StampFadeAt` deadline resources (`Option<f64>` against
  `Time::elapsed_secs_f64`) — the show systems stamp a deadline once, the fade systems only
  read until it passes, then take their single mutable step. Rule going forward: **never tick
  a per-frame `ResMut` inside a system that also writes wasm-rendered UI**; hold deadlines.
  Worth a minimal upstream repro against bevy 0.17/0.18 before the 0.18 step (its UI
  extraction rework may fix or mask it).
- Open lead: three `B0004` warnings at boot (menu-tree children with `GlobalTransform` under
  a parent without, entities 48–51) — cosmetic so far, filed to clean up during the 0.18 step.
- **Catch-up merge with `main`** (2026-09-25): 13 commits of main (playtest cycles, the
  Clean Code pass, touch, the SMB3 presentation pass) merged in; 24 conflicts, all resolved to
  main's side, then the 0.17 API re-applied and the three new banner timers + the follow-up
  line ported to deadlines. Gates: clippy `-D warnings` on default / dev+debug / autoplay,
  both-target checks, full suite, browser run — recorded in TODO 29. Lock moved to
  wasm-bindgen 0.2.127 (pulled by web-sys 0.3.104; 0.2.126 no longer resolves), so the CLI
  bump lands with the merge.
