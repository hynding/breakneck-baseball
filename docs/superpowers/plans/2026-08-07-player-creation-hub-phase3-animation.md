# Player Creation Hub — Phase 3: Animation Personality — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Players move like individuals: three batting-stance variants, two idle fidgets, and a bat-flip home-run celebration, selected per player by the `StyleSet` already authored in `data/players.ron`.

**Architecture:** Six new Blender actions land in `tools/build_player.py`'s `CLIPS` and flow through the proven end-to-end recipe (rebuild pair → `AnimClip` variants → `CLIP_TABLE` rows → contract test). Style *resolution* lives in `animation.rs` (pure `StanceId/FidgetId/CelebrationId → AnimClip` functions — `appearance.rs` stays schema-pure with no animation imports). `batter_stance` and `trigger_swing` become stance-aware via `PlayerIdentity`; a fidget scheduler fires deterministic hash-noise-timed fidgets during dead-ball duels only (with a harness kill-switch, the `JuiceDisabled` pattern); the celebration rides `Playing.next` after the swing so the follow-through is never cut.

**Critical animation constraint:** `BatterSwing`'s frame-0 arm pose equals `BattingStance`'s constant arm values (`UpperArm.R rx −0.95 / rz −0.8`, `UpperArm.L rx −0.95 / rz 0.85` — grid-search-solved, see the long comments in `build_player.py`). Every stance variant and fidget MUST hold/return to those exact arm channel values so the 150 ms crossfade into the swing never pops. Stance personality therefore lives in the **legs, hips, spine, bat, and head channels only**; fidgets may move arms mid-clip but must end on the stance arm pose.

**Tech Stack:** Blender CLI (headless, per CLAUDE.md's sacred pair), Bevy 0.15 AnimationGraph (existing driver), test harness.

**Spec:** `docs/superpowers/specs/2026-08-07-player-creation-hub-design.md` §4 (Phase 3 of §8). Note from the Phase 2 ledger: Layer-1 *bat tint* was descoped in Phase 2 (no schema field) — do not resurrect it here.

## Global Constraints

- PATH prefix for every cargo command: `export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"`.
- Blender rebuilds are ALWAYS the pair, in order: `blender --background --python tools/build_player.py` then `blender --background assets-src/player.blend --python tools/export_glb.py`. Never hand-export. `tools/render_pose_sheet.py` is the QA companion.
- Full `cargo test` green EXCEPT the two known pre-existing failures (`e2e_camera_views::cycling_v_changes_view_and_toggles_the_catchers_visibility`, `e2e_settings::settings_edit_persists_and_game_starts`).
- `tests/balance_sim.rs` must stay green un-retuned — styles are cosmetic; the CPU still bats Classic with the same timing dial. If balance drifts, the implementation leaked style into timing — fix the leak, never the bands.
- `cargo check --target wasm32-unknown-unknown`, clippy `-D warnings`, fmt — all clean.
- glb budgets enforced by `model_contract.rs` (`MAX_GLB_BYTES` 512 KiB, bones ≤ 48); clip-name set equality means Blender and `CLIP_TABLE` must land in the same commit.
- One action per NLA track (multi-strip tracks export wrong) — `bake_clips` already guarantees this; don't bypass it. Never keyframe wardrobe/mesh objects.
- Fidgets/celebrations respect `Time<Virtual>` (they ride `Playing` timers, which already do) and NEVER fire during the steal window, windup, or pitch flight.
- Commit per task; messages end with:
Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>

---

### Task 1: Six clips end-to-end (Blender + `AnimClip` + `CLIP_TABLE`)

**Files:**
- Modify: `tools/build_player.py` (six `CLIPS` entries)
- Regenerate: `assets-src/player.blend`, `src/game/models/player.glb` (the sacred pair)
- Modify: `src/game/animation.rs` (`AnimClip` variants + `duration()` + `looping()` + `limb_pose()` delegation arms), `src/game/model_assets.rs` (`CLIP_TABLE` rows)

**Interfaces:**
- Produces (Tasks 2–4 rely on): `AnimClip::{StanceOpen, StanceClosed, StanceWaggle, FidgetBatTap, FidgetHalfSwing, CelebrateBatFlip}`; glb animations named `"StanceOpen"`, `"StanceClosed"`, `"StanceWaggle"`, `"FidgetBatTap"`, `"FidgetHalfSwing"`, `"CelebrateBatFlip"`.
- Durations/looping: stances 1.2 s looping; `FidgetBatTap` 0.8 s, `FidgetHalfSwing` 0.9 s, `CelebrateBatFlip` 0.85 s — all one-shot. (0.85 keeps the flip mostly inside runner.rs's `TROT_DELAY = 0.9` before the trot-rig handoff.)

- [ ] **Step 1: Author the six `CLIPS` entries**

Add to `CLIPS` in `tools/build_player.py`. These starting tables are derived from the committed `BattingStance` values (arm channels **identical** to it, per the critical constraint above); expect to tune leg/spine/bat numbers in Step 3 — but never the arm base values:

```python
    # Open crouch: wide base, sunk hips, same solved arm/bat hold as
    # BattingStance so the swing crossfade never pops.
    "StanceOpen": (1.2, True, {
        "UpperArm.R": {"rx": [(0, -0.95), (0.25, -0.92), (0.5, -0.95), (0.75, -0.98), (1, -0.95)],
                        "rz": [(0, -0.8), (1, -0.8)]},
        "UpperArm.L": {"rx": [(0, -0.95), (0.25, -0.92), (0.5, -0.95), (0.75, -0.98), (1, -0.95)],
                        "rz": [(0, 0.85), (1, 0.85)]},
        "Bat": {"rx": [(0, 0.6), (1, 0.6)]},
        "UpperLeg.L": {"rx": [(0, 0.5), (1, 0.5)], "rz": [(0, 0.22), (1, 0.22)]},
        "UpperLeg.R": {"rx": [(0, 0.5), (1, 0.5)], "rz": [(0, -0.22), (1, -0.22)]},
        "LowerLeg.L": {"rx": [(0, -0.5), (1, -0.5)]},
        "LowerLeg.R": {"rx": [(0, -0.5), (1, -0.5)]},
        "Hips": {"dz": [(0, -0.10), (1, -0.10)]},
        "Spine": {"ry": [(0, 0.25), (0.25, 0.29), (0.5, 0.25), (0.75, 0.21), (1, 0.25)]},
    }),
    # Upright closed: tall, quiet legs, bat cocked more vertical, deeper coil.
    "StanceClosed": (1.2, True, {
        "UpperArm.R": {"rx": [(0, -0.95), (0.25, -0.93), (0.5, -0.95), (0.75, -0.97), (1, -0.95)],
                        "rz": [(0, -0.8), (1, -0.8)]},
        "UpperArm.L": {"rx": [(0, -0.95), (0.25, -0.93), (0.5, -0.95), (0.75, -0.97), (1, -0.95)],
                        "rz": [(0, 0.85), (1, 0.85)]},
        "Bat": {"rx": [(0, 0.95), (1, 0.95)]},
        "UpperLeg.L": {"rx": [(0, 0.1), (1, 0.1)]},
        "UpperLeg.R": {"rx": [(0, 0.1), (1, 0.1)]},
        "LowerLeg.L": {"rx": [(0, -0.1), (1, -0.1)]},
        "LowerLeg.R": {"rx": [(0, -0.1), (1, -0.1)]},
        "Spine": {"ry": [(0, 0.38), (0.25, 0.41), (0.5, 0.38), (0.75, 0.35), (1, 0.38)]},
    }),
    # Waggle: BattingStance legs, restless barrel + bigger torso sway.
    "StanceWaggle": (1.2, True, {
        "UpperArm.R": {"rx": [(0, -0.95), (0.25, -0.92), (0.5, -0.95), (0.75, -0.98), (1, -0.95)],
                        "rz": [(0, -0.8), (1, -0.8)]},
        "UpperArm.L": {"rx": [(0, -0.95), (0.25, -0.92), (0.5, -0.95), (0.75, -0.98), (1, -0.95)],
                        "rz": [(0, 0.85), (1, 0.85)]},
        "Bat": {"rx": [(0, 0.6), (0.25, 0.95), (0.5, 0.6), (0.75, 0.95), (1, 0.6)]},
        "UpperLeg.L": {"rx": [(0, 0.3), (1, 0.3)]},
        "UpperLeg.R": {"rx": [(0, 0.3), (1, 0.3)]},
        "LowerLeg.L": {"rx": [(0, -0.3), (1, -0.3)]},
        "LowerLeg.R": {"rx": [(0, -0.3), (1, -0.3)]},
        "Spine": {"ry": [(0, 0.25), (0.25, 0.32), (0.5, 0.25), (0.75, 0.18), (1, 0.25)]},
    }),
    # Bat tap: dip the barrel to the plate and back; starts and ENDS on the
    # BattingStance hold so Playing::then(fidget, stance) re-enters clean.
    "FidgetBatTap": (0.8, False, {
        "UpperArm.R": {"rx": [(0, -0.95), (0.4, -0.55), (0.6, -0.55), (1, -0.95)],
                        "rz": [(0, -0.8), (1, -0.8)]},
        "UpperArm.L": {"rx": [(0, -0.95), (0.4, -0.55), (0.6, -0.55), (1, -0.95)],
                        "rz": [(0, 0.85), (1, 0.85)]},
        "Bat": {"rx": [(0, 0.6), (0.4, -0.35), (0.6, -0.35), (1, 0.6)]},
        "Spine": {"rx": [(0, 0), (0.4, 0.18), (0.6, 0.18), (1, 0)],
                   "ry": [(0, 0.25), (1, 0.25)]},
        "UpperLeg.L": {"rx": [(0, 0.3), (1, 0.3)]},
        "UpperLeg.R": {"rx": [(0, 0.3), (1, 0.3)]},
        "LowerLeg.L": {"rx": [(0, -0.3), (1, -0.3)]},
        "LowerLeg.R": {"rx": [(0, -0.3), (1, -0.3)]},
    }),
    # Practice half swing: partial unwind and back, arms riding the torso.
    "FidgetHalfSwing": (0.9, False, {
        "UpperArm.R": {"rx": [(0, -0.95), (0.45, -0.95), (1, -0.95)],
                        "rz": [(0, -0.8), (0.45, -0.35), (1, -0.8)]},
        "UpperArm.L": {"rx": [(0, -0.95), (0.45, -0.95), (1, -0.95)],
                        "rz": [(0, 0.85), (0.45, 0.45), (1, 0.85)]},
        "Bat": {"rx": [(0, 0.6), (0.45, 0.3), (1, 0.6)]},
        "Spine": {"ry": [(0, 0.25), (0.45, -0.15), (1, 0.25)]},
        "UpperLeg.L": {"rx": [(0, 0.3), (1, 0.3)]},
        "UpperLeg.R": {"rx": [(0, 0.3), (1, 0.3)]},
        "LowerLeg.L": {"rx": [(0, -0.3), (1, -0.3)]},
        "LowerLeg.R": {"rx": [(0, -0.3), (1, -0.3)]},
    }),
    # Bat flip: arms sweep up and out, barrel flicks skyward, chest opens.
    # Plays via Playing.next after BatterSwing, so frame 0 matches the
    # swing's END pose region (arms driven through — approximate with the
    # follow-through-side arm values; tune against the pose sheet).
    "CelebrateBatFlip": (0.85, False, {
        "UpperArm.R": {"rx": [(0, -0.95), (0.35, -2.3), (1, -1.5)],
                        "rz": [(0, 0.6), (0.35, 0.2), (1, 0.3)]},
        "UpperArm.L": {"rx": [(0, -0.95), (0.35, -2.0), (1, -1.3)],
                        "rz": [(0, -0.5), (0.35, -0.2), (1, -0.3)]},
        "Bat": {"rx": [(0, 0.6), (0.3, 2.2), (1, 1.4)]},
        "Spine": {"rx": [(0, 0), (0.4, -0.22), (1, -0.1)],
                   "ry": [(0, -0.25), (1, 0.0)]},
        "Head": {"rx": [(0, 0), (0.4, -0.3), (1, -0.15)]},
    }),
```

`BatterSwing`'s actual end-pose arm values may differ from the `CelebrateBatFlip` frame-0 guesses above — read `BatterSwing`'s final keys in `CLIPS` and set the flip's frame-0 arm values to match before first export.

- [ ] **Step 2: Rust side in the same commit**

- `AnimClip` gains the six variants with doc comments; `duration()` arms (1.2/1.2/1.2/0.8/0.9/0.85); `looping()` adds the three stances to the matches!.
- `limb_pose()` (Blocky fallback): delegate — the three stances reuse `BattingStance`'s arm/leg branch, the two fidgets and the celebration reuse `Idle`'s. Implement by matching the new variants into the existing branches (the compiler's exhaustive match walks you to every site — including `root_drop`/`root_pitch` if they match on clip; delegate those identically).
- `CLIP_TABLE` gains six rows with the exact glb names above. `node_for` needs no new arms (each clip has its own action).

- [ ] **Step 3: Rebuild, QA, iterate**

```sh
blender --background --python tools/build_player.py
blender --background assets-src/player.blend --python tools/export_glb.py
cargo test --test model_contract   # name-set equality + budgets
```

Render the pose sheet (`blender --background assets-src/player.blend --python tools/render_pose_sheet.py`) and LOOK at it: stances must read as three visibly different silhouettes; fidget end frames must match the stance hold; the flip's frame 0 must not teleport arms from the swing's end. Tune leg/spine/bat keys as needed (never the stance arm bases). Report what you saw and adjusted honestly.

- [ ] **Step 4: Suite + commit**

`cargo test` (green except the two known — includes `e2e_gltf_model`/`e2e_gltf_rig` proving the 17-clip graph builds), wasm check, clippy, fmt.

```bash
git add tools/build_player.py assets-src/player.blend src/game/models/player.glb src/game/animation.rs src/game/model_assets.rs
git commit -m "feat: six personality clips (stances, fidgets, bat flip) through the model contract"
```

---

### Task 2: Stance resolution — the batter stands like himself

**Files:**
- Modify: `src/game/animation.rs` (style→clip resolution fns + unit tests)
- Modify: `src/game/player.rs` (`batter_stance`, `trigger_swing`)
- Test: extend `tests/e2e_identity.rs`

**Interfaces:**
- Produces (Tasks 3–4 rely on): in `animation.rs` —

```rust
use crate::game::appearance::{CelebrationId, FidgetId, StanceId};

/// StyleSet → clip resolution. Lives here (not appearance.rs) so the schema
/// module stays serde-pure with no animation dependency.
pub fn stance_clip(id: StanceId) -> AnimClip {
    match id {
        StanceId::Standard => AnimClip::BattingStance,
        StanceId::OpenCrouch => AnimClip::StanceOpen,
        StanceId::UprightClosed => AnimClip::StanceClosed,
        StanceId::BatWaggle => AnimClip::StanceWaggle,
    }
}

pub fn fidget_clip(id: FidgetId) -> AnimClip {
    match id {
        FidgetId::BatTap => AnimClip::FidgetBatTap,
        FidgetId::HalfSwing => AnimClip::FidgetHalfSwing,
    }
}

pub fn celebration_clip(id: CelebrationId) -> Option<AnimClip> {
    match id {
        CelebrationId::Standard => None,
        CelebrationId::BatFlip => Some(AnimClip::CelebrateBatFlip),
    }
}

/// Any of the four held batting stances (shared or personal).
pub fn is_stance(clip: AnimClip) -> bool {
    matches!(
        clip,
        AnimClip::BattingStance
            | AnimClip::StanceOpen
            | AnimClip::StanceClosed
            | AnimClip::StanceWaggle
    )
}
```

(All exhaustive — adding a style id forces a mapping. Unit-test: every `StanceId` maps to a clip `is_stance` accepts; `celebration_clip(Standard)` is `None`.)

- [ ] **Step 1: Failing e2e** — extend `tests/e2e_identity.rs`:

```rust
#[test]
fn batter_holds_his_personal_stance() {
    use breakneck_baseball::game::animation::Playing;
    let mut app = headless_app();
    start_game(&mut app, KeyCode::Digit1);
    // Away leadoff (STONE) has stance UprightClosed in data/players.ron →
    // his duel hold must be StanceClosed, not the shared BattingStance.
    let held = run_until(&mut app, 5_000, |app| {
        let world = app.world_mut();
        world
            .query_filtered::<&Playing, With<Batter>>()
            .iter(world)
            .next()
            .map(|p| p.clip == breakneck_baseball::game::animation::AnimClip::StanceClosed)
            .unwrap_or(false)
    });
    assert!(held.is_some(), "batter must hold his personal stance clip");
}
```

(Verify STONE's authored stance in `data/players.ron` first; if the away leadoff's stance id differs from `UprightClosed`, assert whatever `stance_clip` maps his actual id to — the point is personal-not-shared. If his style is `Standard`, pick a better-suited assertion target by consulting the file, and say so.) RED: resolution doesn't exist.

- [ ] **Step 2: Implement**

- Resolution fns as above (+ their unit tests in `animation.rs`'s test module).
- `batter_stance` (player.rs): add `identities: Query<&PlayerIdentity>` lookup + `rosters: Res<Rosters>`; the insert arm becomes `Playing::new(stance)` where `stance = animation::stance_clip(rosters.team(id.team).card(id.index).appearance.style.stance)` (fall back to `AnimClip::BattingStance` when the batter has no identity yet — `identities.get(entity)` miss). The removal arm's condition becomes `animation::is_stance(playing.clip)`. ALSO: if the batter holds a *different* stance than his resolved one (identity changed mid-duel — new at-bat), replace it.
- `trigger_swing`: the `swingable` gate becomes `match playing.map(|p| p.clip) { None => true, Some(c) => animation::is_stance(c) }`.
- Grep for other `AnimClip::BattingStance` matchers (`grep -rn "BattingStance" src/`) — any system that special-cases the stance hold (flow, camera, batting adapters) must go through `is_stance` instead. Fix every site the grep reveals and list them in your report.

- [ ] **Step 3: Suite + commit**

Full `cargo test` (the stance e2e GREEN; balance_sim green un-retuned), wasm, clippy, fmt.

```bash
git add src/game/animation.rs src/game/player.rs tests/e2e_identity.rs
git commit -m "feat: batters hold their personal stance via StyleSet resolution"
```

---

### Task 3: Fidget scheduler + harness kill-switch

**Files:**
- Modify: `src/game/animation.rs` (`FidgetsDisabled` resource)
- Modify: `src/game/player.rs` (`batter_fidgets` system, registered with the batter systems)
- Modify: `tests/common/mod.rs` (harness inserts `FidgetsDisabled` beside `JuiceDisabled`)
- Test: new test in `tests/e2e_dressing.rs` or `e2e_identity.rs` (fidgets fire when enabled)

**Interfaces:**
- Produces: `animation::FidgetsDisabled` (unit Resource, pub); `player::batter_fidgets` (private).

- [ ] **Step 1: Kill-switch + harness first**

```rust
/// Insert to suppress idle fidgets outright — the scripted e2e harness
/// does (a fidget replaces the batter's Playing state mid-script, which
/// perturbs timing-sensitive drivers even though swings can interrupt it).
/// The `JuiceDisabled` pattern.
#[derive(Resource)]
pub struct FidgetsDisabled;
```

`tests/common/mod.rs`: `.insert_resource(breakneck_baseball::game::animation::FidgetsDisabled)` directly under the `JuiceDisabled` insert, with a comment mirroring its style. Run the full suite — everything still green (fidgets don't exist yet; this is the safety net going in first).

- [ ] **Step 2: The scheduler**

In `player.rs`:

```rust
/// Between pitches a batter with an authored fidget occasionally breaks
/// his stance hold — helmet tap, practice half-swing — then settles back
/// into it (`Playing::then`). Dead-ball only: `Phase::PrePitch`, never
/// during the steal window (the duel there is gameplay-legible timing),
/// and only while he's actually holding a stance. Cadence is deterministic
/// hash noise (the ai.rs convention), 4–9 s per at-bat-slot, so replays
/// and tests are reproducible.
fn batter_fidgets(
    play: Res<Play>,
    score: Res<ScoreBoard>,
    order: Res<BattingOrder>,
    rosters: Res<Rosters>,
    time: Res<Time>,
    disabled: Option<Res<animation::FidgetsDisabled>>,
    mut since_stance: Local<f32>,
    batters: Query<(Entity, &PlayerIdentity, Option<&Playing>), With<Batter>>,
    mut commands: Commands,
) {
    if disabled.is_some() || play.phase != Phase::PrePitch || play.in_steal_window() {
        *since_stance = 0.0;
        return;
    }
    let Ok((entity, id, playing)) = batters.get_single() else { return };
    let Some(playing) = playing else { return };
    if !animation::is_stance(playing.clip) {
        *since_stance = 0.0;
        return;
    }
    let card = rosters.team(id.team).card(id.index);
    let Some(fidget) = card.appearance.style.fidget else { return };
    *since_stance += time.delta_secs();
    // Deterministic per-at-bat interval in [4, 9): hash the inning/slot so
    // it varies between at-bats but never between runs.
    let h = (score.inning * 31 + order.current(id.team) * 7 + id.index as u32 * 13) % 100;
    let interval = 4.0 + 5.0 * (h as f32 / 100.0);
    if *since_stance >= interval {
        *since_stance = 0.0;
        commands.entity(entity).insert(Playing::then(
            animation::fidget_clip(fidget),
            animation::stance_clip(card.appearance.style.stance),
        ));
    }
}
```

(Adapt names to the real imports; `time.delta_secs()` is the virtual clock — correct, it must freeze under debug pause. If `order.current` isn't reachable here, derive the hash from `id.index` + `score.inning` + `score.balls * 3 + score.strikes` instead — any deterministic mix is fine; document the one you use.) Register in `PlayerPlugin` with the other batter systems, `run_if(in_state(GameState::Playing))`.

Swing safety already holds: Task 2's `trigger_swing` gate treats only stances as swingable — a swing pressed mid-fidget waits for the `then`-chained stance return (≤ 0.9 s). That is a deliberate, small realism trade documented here; the steal-window/PrePitch gating keeps it out of every timing-critical moment. If review disputes it, the alternative (fidgets also swingable) is a one-line gate change.

- [ ] **Step 3: The e2e (fidgets fire when enabled)**

```rust
#[test]
fn fidgets_fire_between_pitches_when_enabled() {
    use breakneck_baseball::game::animation::{AnimClip, FidgetsDisabled, Playing};
    let mut app = headless_app();
    app.world_mut().remove_resource::<FidgetsDisabled>(); // harness default off
    start_game(&mut app, KeyCode::Digit1);
    // Away leadoff needs an authored fidget for this test — verify in
    // data/players.ron and target accordingly (STONE has none as authored:
    // if so, run to the second batter or assert on whichever leadoff has
    // Some(fidget); state your choice in the test comment).
    let fidgeted = run_until(&mut app, 240 * 12, |app| {
        let world = app.world_mut();
        world
            .query_filtered::<&Playing, With<Batter>>()
            .iter(world)
            .next()
            .map(|p| matches!(p.clip, AnimClip::FidgetBatTap | AnimClip::FidgetHalfSwing))
            .unwrap_or(false)
    });
    assert!(fidgeted.is_some(), "an authored fidget must fire within ~12 s of PrePitch");
}
```

The 12-sim-second window covers the max 9 s interval; the CPU pitcher will deliver pitches — PrePitch recurs between them, and the `since_stance` accumulator only counts PrePitch time. If pitch cadence makes this flaky, pin the scenario seam (`scenario::apply_to_world`) to hold a dead-ball state instead, and note it.

- [ ] **Step 4: Suite + commit**

Full suite (all existing e2es green — the kill-switch proves itself here), wasm, clippy, fmt.

```bash
git add src/game/animation.rs src/game/player.rs tests/common/mod.rs tests/e2e_identity.rs
git commit -m "feat: deterministic idle fidgets between pitches, harness-disabled"
```

---

### Task 4: The bat flip

**Files:**
- Modify: `src/game/player.rs` (`celebrate_home_run` system)
- Test: extend `tests/e2e_identity.rs`

**Interfaces:**
- Consumes: `BallInPlayEvent { kind: ContactKind::HomeRun, .. }` (the event runner.rs's `batter_runs` reads — import from the same module it does), `celebration_clip`, `PlayerIdentity`.

- [ ] **Step 1: Failing e2e**

```rust
#[test]
fn home_run_queues_the_authored_celebration() {
    use breakneck_baseball::game::animation::{AnimClip, Playing};
    let mut app = headless_app();
    start_game(&mut app, KeyCode::Digit1);
    // Reach a wired batter, hand him a swing in flight, then declare a
    // homer: the celebration must chain via Playing.next, never cutting
    // the swing. Target a batter whose authored celebration is BatFlip
    // (check data/players.ron; away FOX and home OKAFOR/KANE have it —
    // use the scenario seam or batting-order advance to put one up, or
    // stamp the identity directly on the batter rig for this synthetic
    // test and say so in a comment).
    /* …boot to Playing, ensure batter rig exists… */
    // Synthetic: swing in flight + HR event.
    let batter = /* query the Batter rig entity */;
    app.world_mut().entity_mut(batter).insert(Playing::new(AnimClip::BatterSwing));
    app.world_mut().send_event(/* BallInPlayEvent homer, matching its real fields */);
    for _ in 0..4 { app.update(); }
    let world = app.world_mut();
    let playing = world.get::<Playing>(batter).expect("swing still in flight");
    assert_eq!(playing.clip, AnimClip::BatterSwing, "swing must not be cut");
    assert_eq!(playing.next, Some(AnimClip::CelebrateBatFlip), "flip chains after");
}
```

Fill the elided boot/query lines from the harness patterns already in this file; match `BallInPlayEvent`'s REAL fields (read its definition — runner.rs imports it) when constructing the synthetic event. The two assertions are the contract: swing preserved, celebration chained.

- [ ] **Step 2: Implement**

```rust
/// A homer with an authored celebration chains it after the swing
/// follow-through (`Playing.next`), so the flip rides the same rig the
/// camera is holding on. The trot rig takes over at RunDelay expiry
/// (TROT_DELAY 0.9 s) — the flip's 0.85 s mostly fits; the handoff
/// truncating the last frames on slow swings is an accepted arcade
/// trade (the HR orbit camera is already moving by then).
fn celebrate_home_run(
    mut events: EventReader<BallInPlayEvent>,
    rosters: Res<Rosters>,
    mut batters: Query<(&PlayerIdentity, &mut Playing), With<Batter>>,
) {
    for ev in events.read() {
        if !matches!(ev.kind, ContactKind::HomeRun) {
            continue;
        }
        for (id, mut playing) in &mut batters {
            let card = rosters.team(id.team).card(id.index);
            if let Some(clip) =
                crate::game::animation::celebration_clip(card.appearance.style.celebration)
            {
                if playing.clip == AnimClip::BatterSwing && playing.next.is_none() {
                    playing.next = Some(clip);
                }
            }
        }
    }
}
```

(Match `BallInPlayEvent`/`ContactKind`'s real shape; if `kind` carries fields, pattern-match accordingly. Register with the batter systems.) The `next.is_none()` guard keeps re-entrancy safe if the event ever double-fires.

- [ ] **Step 3: Suite + commit**

Full suite (including `e2e_home_run_moment` and `e2e_full_game` — the walk-off script's batter has no authored celebration or has one; either way the game must flow to GAME OVER unchanged), wasm, clippy, fmt.

```bash
git add src/game/player.rs tests/e2e_identity.rs
git commit -m "feat: authored bat-flip celebration chains after the home-run swing"
```

---

## Phase-exit checklist

- [ ] Suite green except the two known; balance_sim un-retuned and green; wasm/clippy/fmt clean.
- [ ] Pose sheet reviewed and reported honestly (Task 1 Step 3).
- [ ] TODO.md re-checked.
- [ ] Ledger notes judgment calls for the Phase 4 planner (especially anything the pose sheet left unpolished — Phase 4's portrait harness is the venue for the combined visual QA pass).
