# Layered Module Refactor — Design Spec

**Date:** 2026-08-19
**Status:** Approved (brainstorm 2026-08-19)
**Scope decisions (user-approved):** structural split + code-level rust-skills pass + edition 2024 migration; full layered hierarchy; phased pipeline (Approach A).

## Goal

Reorganize the crate's ~25.7k lines from a flat 31-file `src/game/` into a
layered directory hierarchy with no file over ~700 lines, migrate to Rust
edition 2024, and apply mechanical code-quality rules from the rust-skills
guidelines — all with **zero behavior change**. `tests/balance_sim.rs` is the
economy arbiter and must not shift; no gameplay, balance, or timing value
changes anywhere.

## Invariants (hold at every phase boundary)

1. `cargo test` fully green (unit + all e2e + balance sim).
2. `cargo check` green on native **and** `wasm32-unknown-unknown`.
3. `cargo fmt --check` clean; from Phase 4 on, `cargo clippy` clean per the new lint table.
4. **The public API is the facade.** `game::rules`, `game::flow`, etc. remain
   the canonical paths. `game/mod.rs` re-exports the layer directories
   (`pub use sim::flow;` style), so tests, CLAUDE.md references, and
   cross-module `use crate::game::flow::…` imports never change.
5. **Moves are pure moves.** Phase 2/3 commits contain no semantic edits.
   Semantic changes live only in Phases 1 and 4.
6. Work happens on branch `refactor/layered-modules`; `main` gets a single
   merge at the end (CI deploys `web/` to Pages on every push to `main`).
7. No Blender/glTF artifacts are touched; `tests/model_contract.rs` and
   `tests/appearance_contract.rs` pass untouched.

## Target structure

```
src/game/
├─ mod.rs                  # GameState, ScoreBoard, GamePlugin + layer re-exports (facade)
├─ core/                   # pure rules & data — no Bevy systems, no rendering
│  ├─ mod.rs
│  ├─ rules/               # split of rules.rs (3,177 lines; ~1,490 code + ~1,685 tests)
│  │  ├─ mod.rs            # zone/physics consts, Bases, BattingOrder, Outcome/OutKind, pub use of submodules
│  │  ├─ pitch.rs          # PitchKind, pitch_velocity_kind, hit_velocity/spin, zone + HBP checks
│  │  ├─ contact.rs        # ContactKind/ContactClass, classify_contact, contact_quality, PCI grading, RunnerBreak
│  │  ├─ resolve.rs        # resolve_catch/resolve_gathered/resolve_thrown, throw_target, race math helpers
│  │  ├─ count.rs          # call_ball/call_strike/foul, charge_out/record_out, OutPlay, apply_batted_out/DP/FC
│  │  ├─ advance.rs        # advance_hit/advance_walk/apply_hit, advance_runners_only/advance_trailing, tag_up
│  │  ├─ steal.rs          # steal_candidate, attempt_steal, attempt_pickoff, double_off_lead_runner
│  │  └─ predict.rs        # predict_landing*, catch_time, fence_at, best_catcher, geometry helpers
│  ├─ variant.rs           # Ruleset, FieldSpec (699 lines — stays whole)
│  ├─ roster.rs
│  ├─ theme.rs
│  ├─ scenario.rs
│  └─ model_assets.rs
├─ sim/                    # gameplay systems that decide what happens
│  ├─ mod.rs
│  ├─ flow/                # split of flow.rs (1,408)
│  │  ├─ mod.rs            # Phase, Play, LeadState, BannerTone, events, PhaseSet, FlowPlugin
│  │  ├─ pitch.rs          # reset_flow, pre_pitch, wind_up, pitch_live, catcher_receives, swing_dt_ms/late_swing_z
│  │  ├─ live.rs           # in_play, resolve_live_play, resolve_contact, announce_wall_bang
│  │  └─ result.rs         # result_phase, hit, add_ball, add_strike, resolve_steal, end_pitch
│  ├─ fielding.rs
│  ├─ runner.rs
│  ├─ ball.rs
│  ├─ batting.rs
│  └─ ai.rs
├─ present/                # everything the player sees/hears
│  ├─ mod.rs
│  ├─ field/               # split of field.rs (1,511)
│  │  ├─ mod.rs            # marker components, FieldSurfaces, FieldPlugin
│  │  ├─ textures.rs       # grass_image, dirt_image, tiling_image
│  │  ├─ diamond.rs        # spawn_field/bases/chalk/batters_box/mound, geometry helpers
│  │  ├─ stadium.rs        # ground slab/stadium ground, front yard, foul poles, outfield wall, lighting
│  │  └─ zone.rs           # strike-zone overlay, PCI cursor, zone flash
│  ├─ camera/              # split of camera.rs (880)
│  │  ├─ mod.rs            # CameraMode, DuelView, plugin, spawn/toggles
│  │  ├─ framing.rs        # aspect_safe_duel_vfov, framed_ndc_y, framed_height_fraction, occludes (pure math)
│  │  └─ rigs.rs           # broadcast/orbit/zoom drivers, hide_occluders, CameraKick systems
│  ├─ player/              # split of player.rs (865)
│  │  ├─ mod.rs            # marker components (Pitcher/Batter/Fielder/…), PlayerPlugin
│  │  ├─ rig.rs            # build_materials, build_rig_model, spawn_players/spawn_rig, recolor_*, sync_identities
│  │  └─ behavior.rs       # batter_stance, batter_fidgets, catcher_crouch, trigger_swing, celebrate_home_run
│  ├─ animation/           # split of animation.rs (771)
│  │  ├─ mod.rs            # AnimClip API, Playing, MoveIntent, clip lookup helpers
│  │  ├─ poses.rs          # limb_pose, root_drop, root_pitch, self_pose, bat rotations, ease
│  │  └─ driver.rs         # sample_clips, settle_removed, meter_stance_sink, locomote, graph-rig drivers
│  ├─ ui/                  # split of ui.rs (740)
│  │  ├─ mod.rs            # hidden_tint, marker components, UiPlugin
│  │  ├─ hud.rs            # spawn_hud, base ring, inning/score/count/meter updates
│  │  └─ banner.rs         # duel panels, show/fade banner, contact stamps
│  ├─ fx/                  # split of fx.rs (730)
│  │  ├─ mod.rs            # FxPlugin, hit-stop start/end
│  │  ├─ trail.rs          # pitch trail assets/spawn/tick
│  │  └─ particles.rs      # landing ring, contact/wall-bang bursts, fireworks, bounce dust, tick_particles
│  ├─ jersey.rs
│  ├─ audio.rs
│  └─ juice.rs
└─ meta/                   # shell around the game: menus, persistence, tooling
   ├─ mod.rs               # carries the cfg(feature = "debug") gates for debug-only modules
   ├─ settings/            # split of settings.rs (801)
   │  ├─ mod.rs            # Settings model, BattingStyle/trail enums, load/save, native+wasm read/write_store cfg pair
   │  └─ screen.rs         # settings UI: spawn/paint/edit/toggle/close systems
   ├─ menu.rs
   ├─ input.rs
   ├─ subs.rs
   ├─ gear.rs
   ├─ appearance.rs        # (521 lines — stays whole)
   ├─ debug.rs             # cfg(feature = "debug")
   ├─ portraits.rs         # cfg(feature = "debug")
   └─ creator/             # split of creator.rs (1,313), cfg(feature = "debug")
      ├─ mod.rs            # CreatorState/CreatorTab, enter/exit, apply/revert, external-reload sync
      ├─ panel.rs          # creator_panel + egui tab renderers, radio_grid
      ├─ preview.rs        # CreatorStage, camera_target/lerp, preview_idle, retint_preview, PreviewRig
      ├─ randomize.rs      # roll/pick_* seeded helpers, randomize_player
      └─ persist.rs        # save_working / save_working_to
```

Rules of thumb:

- **Split threshold ~700 lines**; targets land at roughly 200–550 lines per
  file. `variant.rs` (699), `runner.rs` (647), `debug.rs` (593), `audio.rs`
  (580), `fielding.rs` (574) stay whole — each is one coherent concern.
- **Unit tests travel with their functions** (`test-cfg-test-module`): each
  `rules/` submodule carries its own `#[cfg(test)] mod tests`.
- **Visibility widens only as far as needed**: helpers that were private and
  are now called across sibling submodules become `pub(super)`; `pub(crate)`
  only where a different layer needs them (`proj-pub-super-parent`,
  `proj-pub-crate-internal`).
- Layer `mod.rs` files are thin: `pub mod` lines (plus the `debug`-feature
  gates in `meta/mod.rs`); split-directory `mod.rs` files keep shared
  types/consts and `pub use` their submodules so existing item paths
  (`game::rules::resolve_thrown`) resolve unchanged.

## Phases

### Phase 1 — Edition 2024 migration

- Bump `edition = "2024"` in `Cargo.toml`; add `rust-version` (MSRV;
  `proj-msrv-declare`) matching what CI's toolchain supports.
- `cargo fix --edition --all-targets`, then manual fallout: RPIT capture
  rules, if-let temporary scopes, `gen` keyword collisions, prelude changes.
  (Crate has zero `unsafe`, so no `unsafe extern`/`unsafe attribute` work.)
- Re-verify on wasm (`cargo check --target wasm32-unknown-unknown`);
  wasm-bindgen version in `Cargo.lock` must remain 0.2.126 (CI derives the
  CLI version from the lockfile).

### Phase 2 — Layered move (pure `git mv`)

- Create `core/`, `sim/`, `present/`, `meta/` with thin `mod.rs` files;
  `git mv` each module to its layer per the table above (whole files, no
  splits yet — `rules.rs` moves to `core/rules.rs`, etc.).
- `game/mod.rs`: declare the four layers, re-export every module at its old
  path (`pub use core::rules;` …). The `#[cfg(feature = "debug")]` gates move
  to `meta/mod.rs`, with the re-exports in `game/mod.rs` gated identically.
- Note: `core` collides with the `core` crate under uniform path resolution,
  so `use`/`pub use` statements referring to the layer must be written
  `self::core::…` (in `game/mod.rs`) or `crate::game::core::…` — never a bare
  leading `core::`. If this proves noisy in practice, the fallback layer name
  is `corelogic`; the facade re-exports make either choice invisible to callers.
- No content edits beyond `mod`/`use` wiring.

### Phase 3 — Big-file splits

- One commit per split (or small batches), in this order: `rules`, `flow`,
  `field`, `creator`, `camera`, `player`, `settings`, `animation`, `ui`, `fx`.
- Mechanics per split: file → directory; shared types/consts stay in
  `mod.rs`; function clusters move whole (with their tests); `pub use`
  facade keeps item paths stable; visibility widened minimally.
- Largest file in the crate drops from 3,177 to roughly 550 lines.

### Phase 4 — Code-level rust-skills pass

1. **Lint table** in `Cargo.toml` (`[lints.clippy]`/`[lints.rust]`):
   deny `correctness`; warn `suspicious`, `style`, `complexity`, `perf`;
   enable `unexpected_cfgs` with `dev`/`debug` features declared
   (`lint-deny-correctness`, `lint-warn-*`, `lint-cfg-check`).
   Fix findings layer by layer: `core` → `sim` → `present` → `meta`.
2. **Hot-path memory passes** (`mem-with-capacity`, `mem-avoid-format`,
   `mem-write-over-format`): jersey RGBA texture building, fx particle
   spawns, field texture synthesis, per-frame HUD string updates — only
   where the fix is locally provable. No speculative optimization
   (`perf-profile-first`).
3. **Idiom fixes**: `matches!`/`let-else` where clippy suggests;
   iterator-over-index where it removes a bounds check and stays readable;
   `#[must_use]` on pure `rules::` result types (`Outcome`, `OutPlay`,
   `ContactQuality`, steal/pickoff results) where dropping one is a bug.
4. **Out of scope**: error-handling overhaul (startup-invariant panics are
   the accepted idiom in this game loop), `thiserror`/`anyhow`, async
   anything, `clippy::pedantic`, any tuning-value or timing change.

### Phase 5 — Docs & landing

- Update CLAUDE.md architecture section for the new layout (module paths,
  the layer map, the facade rule for future modules).
- Check off the corresponding TODO.md entry if one exists; note completion
  in TADA.md per project convention.
- Merge `refactor/layered-modules` → `main` (single merge, phase commits
  preserved for bisectability); confirm Pages CI deploys green.

## Verification

Per-phase gate (all must pass before the phase's commit):

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"
cargo test
cargo check --target wasm32-unknown-unknown
cargo fmt --check
cargo clippy --all-targets   # from Phase 4 on
```

Also once per phase: `cargo check --features "dev debug"` so the debug-only
modules (`creator`, `debug`, `portraits`) stay wired.

## Risks

- **Edition 2024 subtleties** (Phase 1): if-let temporary-scope changes can
  alter drop timing. Mitigation: the full e2e suite runs at 240 Hz virtual
  time and is sensitive to frame-order regressions; any failure here blocks
  the phase.
- **`add_plugins` 15-tuple limit** (`mod.rs` comment): layer re-exports don't
  change plugin registration, but any wiring edits must respect the existing
  two-tuple split.
- **Facade drift**: future modules must be added to both the layer `mod.rs`
  and the `game/mod.rs` re-export list. CLAUDE.md update (Phase 5) documents
  this rule.
