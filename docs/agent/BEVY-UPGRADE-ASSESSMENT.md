# Bevy Upgrade Assessment — 2026-08-20

Research only; no upgrade started. Sources: bevy.org news/migration guides, dimforge
bevy_rapier CHANGELOG (fetched 2026-08-20).

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
  `tests/model_contract.rs` and the 150 ms cross-fade behavior re-tested by eye.
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
