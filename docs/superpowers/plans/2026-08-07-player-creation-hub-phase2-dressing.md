# Player Creation Hub — Phase 2: Dressing — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `PlayerAppearance` visible: per-player skin tones on the glTF rigs, procedural gear (helmets, backwards caps, glasses, shades, eye black, wristbands, chains) mounted on bones, and the baked cap hidden when headwear replaces it.

**Architecture:** `wire_rigs` starts collecting each rig's skin/cap mesh entities onto the root (`RigSkinMeshes`/`RigCapMeshes`) and resolving three more contract bones (`Head`, `LowerArm.L/R`). A new `gear.rs` owns everything identity-driven-visual beyond jerseys: a lazy swatch→material cache for skin tones, procedural gear meshes built once into `GearAssets`, and one dressing system that (re)applies skin material, cap visibility, and gear props whenever a rig is wired or its identity's *appearance-relevant* data actually changes (guarded by a `DressedAs` cache component so per-pitch identity re-stamps don't churn). Ordering joins the identity flow via a new `IdentitySet` SystemSet (implementing the Phase 1 final reviewer's recommendation). `GltfPart` is deliberately NOT extended — its exhaustive team-keyed match in `recolor_gltf` is the wrong shape for player-keyed data.

**Tech Stack:** Bevy 0.15 (procedural meshes, `StandardMaterial`), existing glTF wiring (`model_assets.rs`), existing test harness.

**Spec:** `docs/superpowers/specs/2026-08-07-player-creation-hub-design.md` §3 (Phase 2 of §8).

## Global Constraints

- Rust not on PATH; prefix every cargo command: `export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"`.
- Full `cargo test` green EXCEPT the two known pre-existing failures (`e2e_camera_views::cycling_v_changes_view_and_toggles_the_catchers_visibility`, `e2e_settings::settings_edit_persists_and_game_starts`) — never touch them.
- `cargo check --target wasm32-unknown-unknown` clean after render-adjacent changes; `cargo clippy --lib -- -D warnings`; `cargo fmt --check`.
- No Blender changes in this phase — all gear is procedural Bevy primitives (the `Head`/`LowerArm.*` bones already exist in the glb and `ATTACH_BONES` guards them).
- Umpires have no `PlayerIdentity` and are never dressed (base skin, team-recolored cap as today). The Blocky fallback rig is out of scope (both built-in themes use the glTF model; documented judgment call).
- No gameplay behavior changes; `fx/fielding/runner` still never mutate `ScoreBoard`/`Bases`.
- Commit per task; messages end with:
Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>

---

### Task 1: Skin-tone layer + `IdentitySet` + rig mesh lists

**Files:**
- Modify: `src/game/model_assets.rs` (SKIN const, `RigAnimations.skin_material`, `wire_rigs` collects mesh lists + inserts them on the root)
- Modify: `src/game/appearance.rs` (`SkinTone::color()`)
- Modify: `src/game/player.rs` + `src/game/jersey.rs` (`IdentitySet` definition + chain membership)
- Create: `src/game/gear.rs` (swatch cache + `dress_rigs` skin arm only; headwear/gear arms come in Task 3)
- Modify: `src/game/mod.rs` (register `GearPlugin`)
- Modify: `tests/model_contract.rs` (Skin joins the named-material loop)

**Interfaces:**
- Consumes: `RigAnimations`, `wire_rigs`, `PlayerIdentity`, `Rosters` (Phase 1).
- Produces (Tasks 2–4 rely on these): `model_assets::SKIN_MATERIAL: &str = "Skin"`; `RigAnimations.skin_material: Handle<StandardMaterial>` (pub); `#[derive(Component)] pub struct RigSkinMeshes(pub Vec<Entity>)` and `pub struct RigCapMeshes(pub Vec<Entity>)` (in `model_assets.rs`, inserted on rig roots by `wire_rigs` in the same batch as `RigBones`); `appearance::SkinTone::color(self) -> bevy::color::Color`; `gear::SkinMaterials` resource with `fn get(&mut self, tone: SkinTone, base: &Handle<StandardMaterial>, materials: &mut Assets<StandardMaterial>) -> Handle<StandardMaterial>` (lazy per-tone cache); `gear::DressedAs` component; `gear::dress_rigs` system; `pub struct IdentitySet;` SystemSet (in `player.rs`).

- [ ] **Step 1: Failing unit test for swatch colors**

In `src/game/appearance.rs` tests:

```rust
#[test]
fn skin_tones_resolve_to_distinct_colors() {
    use bevy::color::ColorToComponents;
    let tones = [
        SkinTone::Porcelain,
        SkinTone::Light,
        SkinTone::Medium,
        SkinTone::Tan,
        SkinTone::Brown,
        SkinTone::Deep,
    ];
    let colors: Vec<[f32; 4]> = tones.iter().map(|t| t.color().to_srgba().to_f32_array()).collect();
    for (i, a) in colors.iter().enumerate() {
        for b in &colors[i + 1..] {
            assert_ne!(a, b, "every swatch must be visually distinct");
        }
    }
    // Luminance ordering: the list runs light → deep.
    let lum = |c: &[f32; 4]| 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2];
    for w in colors.windows(2) {
        assert!(lum(&w[0]) > lum(&w[1]), "tones must darken monotonically");
    }
}
```

Run `cargo test --lib appearance` — RED (`color` not found).

- [ ] **Step 2: Implement `SkinTone::color()`**

In `appearance.rs` (bevy is already a dependency of the module via `Resource`):

```rust
impl SkinTone {
    /// Curated swatch colours (sRGB). Data files reference tones by id;
    /// only this function knows the pixels, so retuning the palette never
    /// touches player data.
    pub fn color(self) -> bevy::color::Color {
        use bevy::color::Color;
        match self {
            SkinTone::Porcelain => Color::srgb(0.96, 0.87, 0.79),
            SkinTone::Light => Color::srgb(0.88, 0.72, 0.59),
            SkinTone::Medium => Color::srgb(0.76, 0.57, 0.42),
            SkinTone::Tan => Color::srgb(0.62, 0.44, 0.30),
            SkinTone::Brown => Color::srgb(0.45, 0.30, 0.20),
            SkinTone::Deep => Color::srgb(0.28, 0.18, 0.12),
        }
    }
}
```

(Exhaustive match, no wildcard — adding a tone forces a colour.) GREEN.

Also in this step: `SkinTone`'s derive list gains `Hash` (it becomes a
`HashMap` key in Step 4):

```rust
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
```

- [ ] **Step 3: Skin material through the contract**

In `model_assets.rs`:
- Add `pub const SKIN_MATERIAL: &str = "Skin";` beside the other material consts.
- `RigAnimations` gains `pub skin_material: Handle<StandardMaterial>,`; `build_rig_animations` resolves it exactly like `bat_material` (`gltf.named_materials.get(SKIN_MATERIAL)`).
- `wire_rigs`: add a `skin_meshes` vec collected by `mat.0 == anims.skin_material`; insert `RigSkinMeshes(skin_meshes)` and `RigCapMeshes(cap_meshes.clone())` on the root in the same `commands.entity(root).insert((...))` batch as `RigBones` (cap meshes still ALSO get their `GltfJerseyMesh` tags — team recolor keeps working; the new component is the per-rig show/hide + who-am-I seam).

```rust
/// Skinned-mesh entities wearing the model's Skin material, per rig —
/// the per-player tint seam. Umpire rigs get one too but are never
/// dressed (no PlayerIdentity).
#[derive(Component)]
pub struct RigSkinMeshes(pub Vec<Entity>);

/// This rig's cap submeshes — headwear dressing shows/hides them per
/// player while team recolouring keeps owning their material.
#[derive(Component)]
pub struct RigCapMeshes(pub Vec<Entity>);
```

In `tests/model_contract.rs`: add `SKIN_MATERIAL` to the required-materials loop (it iterates the named material consts — extend the array).

- [ ] **Step 4: `IdentitySet` and the gear plugin skeleton**

In `player.rs`, next to `sync_identities`:

```rust
/// Systems that stamp [`crate::game::roster::PlayerIdentity`] run in this
/// set; identity *consumers* order `.after(IdentitySet)` and get Bevy's
/// auto-inserted sync point, seeing the same-frame stamps. Promoted from a
/// bare `.chain()` per the Phase 1 review so new consumers join
/// declaratively.
#[derive(bevy::ecs::schedule::SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct IdentitySet;
```

In `jersey.rs`'s plugin, the existing chain becomes membership + ordering (semantics unchanged — `sync_identities` still precedes `dress_jerseys` with a sync point, still `.after(runner::sync_runners)`):

```rust
.add_systems(
    Update,
    (
        crate::game::player::sync_identities
            .in_set(crate::game::player::IdentitySet)
            .after(crate::game::runner::sync_runners),
        dress_jerseys.after(crate::game::player::IdentitySet),
    )
        .run_if(in_state(GameState::Playing)),
)
```

Create `src/game/gear.rs` with the module doc, `SkinMaterials`, `DressedAs`, and `dress_rigs` (skin arm only this task):

```rust
//! Per-player visual dressing beyond jerseys: skin tones, headwear, and
//! gear props. Everything here is driven by [`PlayerIdentity`] →
//! [`PlayerAppearance`]; the systems only mutate materials, visibility,
//! and prop children — never rules, never `ScoreBoard`.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::game::appearance::{PlayerAppearance, SkinTone};
use crate::game::model_assets::{RigAnimations, RigSkinMeshes};
use crate::game::roster::{PlayerIdentity, Rosters};
use crate::game::{GameState, Team};

/// Lazy cache of tinted skin materials, one per swatch — bounded by the
/// palette, not the roster (the `JerseyCache` precedent).
#[derive(Resource, Default)]
pub struct SkinMaterials(HashMap<SkinTone, Handle<StandardMaterial>>);

impl SkinMaterials {
    fn get(
        &mut self,
        tone: SkinTone,
        base: &Handle<StandardMaterial>,
        materials: &mut Assets<StandardMaterial>,
    ) -> Handle<StandardMaterial> {
        self.0
            .entry(tone)
            .or_insert_with(|| {
                let mut m = materials.get(base).cloned().unwrap_or_default();
                m.base_color = tone.color();
                materials.add(m)
            })
            .clone()
    }
}

/// What a rig is currently dressed as. Skipping unchanged re-stamps here
/// keeps the per-pitch identity refresh from churning materials/props.
#[derive(Component, PartialEq, Clone, Copy)]
pub struct DressedAs {
    team: Team,
    appearance: PlayerAppearance,
}

/// Applies per-player skin (this task; headwear/gear arms land in Task 3)
/// whenever a rig is freshly wired or its identity's look actually changed.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn dress_rigs(
    mut commands: Commands,
    rosters: Res<Rosters>,
    anims: Option<Res<RigAnimations>>,
    mut skins: ResMut<SkinMaterials>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    rigs: Query<
        (Entity, &PlayerIdentity, &RigSkinMeshes, Option<&DressedAs>),
        Or<(Changed<PlayerIdentity>, Added<RigSkinMeshes>)>,
    >,
    mut mesh_mats: Query<&mut MeshMaterial3d<StandardMaterial>>,
) {
    let Some(anims) = anims else { return };
    for (rig, id, skin_meshes, dressed) in &rigs {
        let card = rosters.team(id.team).card(id.index);
        let target = DressedAs {
            team: id.team,
            appearance: card.appearance,
        };
        if dressed == Some(&target).as_ref().copied().as_ref() {
            continue; // same look — per-pitch re-stamp, nothing to do
        }
        let skin = skins.get(card.appearance.skin, &anims.skin_material, &mut materials);
        for &mesh in &skin_meshes.0 {
            if let Ok(mut mat) = mesh_mats.get_mut(mesh) {
                mat.0 = skin.clone();
            }
        }
        commands.entity(rig).insert(target);
    }
}

pub struct GearPlugin;

impl Plugin for GearPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SkinMaterials>().add_systems(
            Update,
            dress_rigs
                .after(crate::game::player::IdentitySet)
                .run_if(in_state(GameState::Playing)),
        );
    }
}
```

(If the `dressed == ...` comparison line fights the borrow checker, the
plain form is `if dressed.map(|d| *d) == Some(target) { continue; }` —
keep the semantics, not the syntax.) `RigCapMeshes`/`RigBones` are
imported in Task 3 when their arms land, not now (clippy `-D warnings`
is a hard gate). Also give `DressedAs` a pub accessor now — Task 4's
tests read it:

```rust
impl DressedAs {
    pub fn team(&self) -> Team {
        self.team
    }
}
```

Register in `mod.rs`: `gear::GearPlugin` next to `JerseyPlugin`, plus `pub mod gear;`.

- [ ] **Step 5: e2e assertion (extend `tests/e2e_identity.rs`)**

New test in the existing file (harness + wiring already proven there):

```rust
#[test]
fn skin_tones_dress_the_wired_rigs() {
    let mut app = headless_app();
    start_game(&mut app, KeyCode::Digit1);
    // Wait for glTF wiring + dressing (async asset load).
    let dressed = run_until(&mut app, 5_000, |app| {
        let world = app.world_mut();
        let mut q = world.query::<&breakneck_baseball::game::gear::DressedAs>();
        q.iter(world).count() > 0
    });
    assert!(dressed.is_some(), "at least one rig must dress after wiring");
    // A dressed rig's skin meshes must not wear the shared base material.
    let world = app.world_mut();
    let base = world
        .resource::<breakneck_baseball::game::model_assets::RigAnimations>()
        .skin_material
        .clone();
    let mut rigs = world.query_filtered::<
        &breakneck_baseball::game::model_assets::RigSkinMeshes,
        With<breakneck_baseball::game::gear::DressedAs>,
    >();
    let skin_meshes: Vec<Entity> = rigs.iter(world).flat_map(|m| m.0.clone()).collect();
    assert!(!skin_meshes.is_empty());
    for mesh in skin_meshes {
        let mat = world
            .get::<MeshMaterial3d<StandardMaterial>>(mesh)
            .expect("skin mesh keeps its material component");
        assert_ne!(mat.0, base, "dressed skin must be a swatch clone, not the base");
    }
}
```

(`DressedAs` needs `pub` visibility for this — make the struct pub with private fields.) Headless caveat: the glb loads and scenes instantiate without a GPU (proven by `e2e_gltf_rig.rs`) — if `DressedAs` never appears, check `dress_rigs` ran after `wire_rigs` inserted `RigSkinMeshes` (the `Added` trigger) before suspecting assets.

- [ ] **Step 6: Full suite + wasm + commit**

`cargo test` (green except the two known), `cargo check --target wasm32-unknown-unknown`, clippy, fmt.

```bash
git add src/game/model_assets.rs src/game/appearance.rs src/game/gear.rs src/game/player.rs src/game/jersey.rs src/game/mod.rs tests/model_contract.rs tests/e2e_identity.rs
git commit -m "feat: per-player skin tones on glTF rigs via IdentitySet-ordered dressing"
```

---

### Task 2: Contract bones — `Head`, `LowerArm.L`, `LowerArm.R`

**Files:**
- Modify: `src/game/model_assets.rs` (`RigBones` fields, `wire_rigs` name match, `ATTACH_BONES`)

**Interfaces:**
- Produces (Task 3 relies on): `RigBones { spine, upper_arm_l, upper_arm_r, bat, head: Entity, lower_arm_l: Entity, lower_arm_r: Entity }`.

- [ ] **Step 1: Extend the contract first**

`ATTACH_BONES` becomes:

```rust
pub const ATTACH_BONES: &[&str] = &[
    "Hips",
    "Spine",
    "Head",
    "UpperArm.L",
    "UpperArm.R",
    "LowerArm.L",
    "LowerArm.R",
    "Bat",
];
```

Run `cargo test --test model_contract` — expect GREEN immediately (all three bones already exist in the committed glb; this is a pin, not a change — say so in the commit message).

Documented deviation from spec §3: `hips` is deliberately NOT added to `RigBones` — no Phase 2 gear mounts there, and an unused resolved bone is dead weight (YAGNI). The spec's Layer-2 list is a menu, not a mandate; add `hips` when a prop actually needs it.

- [ ] **Step 2: Failing compile via `RigBones` growth**

Add `pub head: Entity, pub lower_arm_l: Entity, pub lower_arm_r: Entity` to `RigBones`; `cargo check` — RED at `wire_rigs`'s struct literal.

- [ ] **Step 3: Resolve them in `wire_rigs`**

Extend the locals `(mut head, mut lal, mut lar)`, the name match (`"Head"`, `"LowerArm.L"`, `"LowerArm.R"`), the `let (Some(...)...)` gate, and the struct literal. GREEN: `cargo test --test e2e_gltf_rig --test e2e_identity` (wiring still resolves; nothing else changed).

- [ ] **Step 4: Commit**

```bash
git add src/game/model_assets.rs
git commit -m "feat: pin Head/LowerArm bones in the rig contract for gear mounting"
```

---

### Task 3: Procedural gear + headwear (cap-hiding)

**Files:**
- Modify: `src/game/gear.rs` (GearAssets, prop spawning, headwear visibility — extends `dress_rigs`)

**Interfaces:**
- Consumes: Task 1's `DressedAs`/`dress_rigs`/`RigCapMeshes`, Task 2's `RigBones` bones, `GltfTeamMaterials` (pub, `cap(team)`), `PlayerAppearance` fields (`headwear`, `eyewear`, `arms`, `chain`).
- Produces: `gear::GearProp` marker component (pub, for tests); `gear::RigGear(Vec<Entity>)` component on rig roots tracking spawned props.

- [ ] **Step 1: Failing e2e (extend `tests/e2e_identity.rs`)**

```rust
#[test]
fn headwear_hides_the_baked_cap_and_mounts_gear() {
    let mut app = headless_app();
    start_game(&mut app, KeyCode::Digit1);
    // VEGA (home slot 0 → the pitcher in the top 1st) wears a Helmet in
    // data/players.ron: his baked cap must hide and a helmet prop appear.
    // Gate on the PITCHER RIG specifically being dressed — rigs wire
    // asynchronously per-entity, so "any gear exists" would race.
    let done = run_until(&mut app, 5_000, |app| {
        let world = app.world_mut();
        let mut q = world.query_filtered::<
            &breakneck_baseball::game::gear::RigGear,
            With<breakneck_baseball::game::player::Pitcher>,
        >();
        q.iter(world).next().map(|g| !g.0.is_empty()).unwrap_or(false)
    });
    assert!(done.is_some(), "the helmeted pitcher must dress with gear props");

    let world = app.world_mut();
    // Find the pitcher rig (identity Home/0 = VEGA per data/players.ron).
    let mut pitchers = world.query_filtered::<
        (&breakneck_baseball::game::model_assets::RigCapMeshes,
         &breakneck_baseball::game::gear::RigGear),
        With<breakneck_baseball::game::player::Pitcher>,
    >();
    let (caps, gear) = pitchers.single(world);
    let cap_entities = caps.0.clone();
    let gear_entities = gear.0.clone();
    assert!(!gear_entities.is_empty(), "helmet wearer must own gear props");
    for cap in cap_entities {
        assert_eq!(
            world.get::<Visibility>(cap).copied(),
            Some(Visibility::Hidden),
            "baked cap must hide under a helmet"
        );
    }
    // Spec §7: props are parented to the right bone entities — the helmet
    // must be a child of the pitcher rig's Head bone.
    let mut pitcher_bones = world.query_filtered::<
        &breakneck_baseball::game::model_assets::RigBones,
        With<breakneck_baseball::game::player::Pitcher>,
    >();
    let head = pitcher_bones.single(world).head;
    let on_head = gear_entities
        .iter()
        .any(|&p| world.get::<Parent>(p).map(|par| par.get()) == Some(head));
    assert!(on_head, "the helmet prop must hang off the Head bone");
}
```

RED: `GearProp`/`RigGear` don't exist.

- [ ] **Step 2: Implement gear assets and the full dressing arms**

Extend `gear.rs`. Requirements (implement all; starting dimensions are
suggestions — tune visually in Step 3, correctness is parenting/visibility):

```rust
/// Shared prop meshes + fixed materials, built once at startup. Team-tinted
/// props (helmet, backwards cap) borrow GltfTeamMaterials at spawn instead.
#[derive(Resource)]
pub struct GearAssets {
    helmet: Handle<Mesh>,      // Sphere(0.15) — covers the cap-less head top
    cap_crown: Handle<Mesh>,   // Cylinder(0.13, 0.08) — backwards-cap crown
    cap_brim: Handle<Mesh>,    // Cuboid(0.20, 0.02, 0.12) — worn at the BACK
    lens: Handle<Mesh>,        // Cuboid(0.07, 0.05, 0.02) — one glasses lens
    visor: Handle<Mesh>,       // Cuboid(0.22, 0.05, 0.02) — shades
    eye_black: Handle<Mesh>,   // Cuboid(0.05, 0.025, 0.005) — cheek smear
    wristband: Handle<Mesh>,   // Cylinder(0.055, 0.05) — forearm band
    chain: Handle<Mesh>,       // Torus { minor_radius: 0.012, major_radius: 0.09 } — necklace
    dark: Handle<StandardMaterial>,   // near-black, for lenses/visor/eye black
    white: Handle<StandardMaterial>,  // wristbands
    gold: Handle<StandardMaterial>,   // chain (metallic-ish: base gold, low roughness)
}

/// One spawned gear prop (marker; the owning rig tracks them in RigGear).
#[derive(Component)]
pub struct GearProp;

/// The gear prop entities this rig currently wears — despawned and rebuilt
/// whenever the look changes.
#[derive(Component, Default)]
pub struct RigGear(pub Vec<Entity>);
```

`dress_rigs` grows the headwear/gear arms (same change-guard, same loop):
1. **Cap visibility** (`RigCapMeshes`): `Visibility::Inherited` iff `appearance.headwear == Headwear::Cap`, else `Hidden`.
2. **Despawn old props**: `for e in take(rig_gear.0) { commands.entity(e).despawn_recursive(); }` (query `Option<&mut RigGear>`; insert default when absent).
3. **Spawn props per appearance**, each `(GearProp, Mesh3d, MeshMaterial3d, Transform)` **parented to the right bone via `commands.entity(bone).add_child(prop)`** (never per-frame transform copying):
   - `Helmet`: helmet mesh on `bones.head`, `GltfTeamMaterials::cap(id.team)` material, offset ~`(0.0, 0.06, 0.0)`.
   - `CapBackwards`: crown on `head` (~`(0.0, 0.10, 0.0)`) + brim on `head` rotated to sit at the back (~`(0.0, 0.08, -0.13)`), both team cap material.
   - `Bare`: nothing (cap already hidden).
   - `Eyewear::Glasses`: two lenses on `head`, front (~`(±0.055, 0.02, 0.11)`), dark. `Shades`: one visor, dark. `EyeBlack`: two smears lower (~`(±0.05, -0.03, 0.11)`), dark.
   - `Arms::WristbandL/R/Both`: wristband mesh on `bones.lower_arm_l`/`lower_arm_r` near the hand end (~`(0.0, -0.18, 0.0)` bone-local; confirm sign visually — bone Y runs along the limb).
   - `chain: true`: torus on `bones.spine` at the neck (~`(0.0, 0.30, 0.02)`, rotated flat ~85° around X so it drapes), gold.
4. Record all spawned entities in `RigGear`, insert alongside `DressedAs`.

The system needs `Option<Res<GltfTeamMaterials>>` (skip dressing until it exists — same frame-retry pattern as everything else) and `&RigBones` joins the rig query (keeps umpires out twice over). Rig query grows `Added<RigBones>` in the `Or<>` so late wiring re-triggers.

`GearPlugin::build` gains a `Startup` system building `GearAssets` (meshes + the three fixed materials).

- [ ] **Step 3: Visual tune (required, honest)**

`cargo run --features dev` (a window opens on this machine — expected), start a 1P game, and eyeball: helmet sits on the head through the windup; backwards-cap brim points backward; wristbands ride the forearms during `BattingStance` and `BatterSwing`; the chain doesn't float. Adjust the offsets above as needed (they are bone-local; the bones animate, so props follow for free). Note in your report what you changed and what you verified — if you cannot judge something from a quick run, say so rather than claiming polish.

- [ ] **Step 4: Full suite + wasm + commit**

`cargo test` (the new e2e now GREEN; suite green except the two known), wasm check, clippy, fmt.

```bash
git add src/game/gear.rs tests/e2e_identity.rs
git commit -m "feat: procedural bone-mounted gear + per-player headwear"
```

---

### Task 4: Dressing e2e depth + churn guard

**Files:**
- Create: `tests/e2e_dressing.rs`
- Test: churn regression + half-inning flip redress

**Interfaces:**
- Consumes: everything above; the scenario seam (`scenario::apply_to_world`, `presets`) if useful.

- [ ] **Step 1: Write the failing/verifying e2e**

```rust
//! Dressing e2e: looks follow identity across flips, without per-pitch churn.

mod common;

use bevy::prelude::*;
use breakneck_baseball::game::gear::{GearProp, RigGear};
use breakneck_baseball::game::player::Batter;
use breakneck_baseball::game::roster::PlayerIdentity;
use breakneck_baseball::game::{ScoreBoard, Team};
use common::{headless_app, run_until, start_game};

/// Per-pitch identity re-stamps must not rebuild gear: prop entity ids for
/// an unchanged look stay stable across scoreboard changes.
#[test]
fn gear_survives_count_changes_without_respawning() {
    let mut app = headless_app();
    start_game(&mut app, KeyCode::Digit1);
    // Readiness = the dressed-rig count has been STABLE for 60 frames
    // (async per-rig wiring means "some gear exists" races a late rig).
    let mut stable_frames = 0u32;
    let mut last_count = 0usize;
    let ready = run_until(&mut app, 10_000, |app| {
        let world = app.world_mut();
        let count = world
            .query::<&breakneck_baseball::game::gear::DressedAs>()
            .iter(world)
            .count();
        if count > 0 && count == last_count {
            stable_frames += 1;
        } else {
            stable_frames = 0;
            last_count = count;
        }
        stable_frames >= 60
    });
    assert!(ready.is_some(), "dressed-rig count never stabilized");
    let world = app.world_mut();
    let before: Vec<Entity> = world.query_filtered::<Entity, With<GearProp>>().iter(world).collect();
    // Force a scoreboard change (a ball on the count) — identities re-stamp.
    world.resource_mut::<ScoreBoard>().balls += 1;
    for _ in 0..8 {
        app.update();
    }
    let world = app.world_mut();
    let after: Vec<Entity> = world.query_filtered::<Entity, With<GearProp>>().iter(world).collect();
    assert_eq!(before, after, "unchanged looks must not respawn props on count changes");
}

/// After a half-inning flip the batter rig is a different team's player —
/// its DressedAs must follow (the old batter look would be a wrong-team leak).
#[test]
fn batter_redresses_on_half_inning_flip() {
    let mut app = headless_app();
    start_game(&mut app, KeyCode::Digit1);
    let ready = run_until(&mut app, 5_000, |app| {
        let world = app.world_mut();
        world
            .query_filtered::<&PlayerIdentity, With<Batter>>()
            .iter(world)
            .next()
            .is_some()
    });
    assert!(ready.is_some());
    // Flip the half-inning wholesale (outs reset, sides swap).
    {
        let mut score = app.world_mut().resource_mut::<ScoreBoard>();
        score.top_of_inning = false;
    }
    // The claim under test is the DRESSING following the flip, not just
    // identity (Phase 1 already pins identity) — read DressedAs::team().
    let flipped = run_until(&mut app, 1_000, |app| {
        let world = app.world_mut();
        world
            .query_filtered::<&breakneck_baseball::game::gear::DressedAs, With<Batter>>()
            .iter(world)
            .next()
            .map(|d| d.team() == Team::Home)
            .unwrap_or(false)
    });
    assert!(flipped.is_some(), "batter dressing must follow the flip");
}

/// Spec §7: runner rigs are dressed too. Manifest bases-loaded runners via
/// the scenario seam (the e2e_identity pattern) and assert each carries
/// DressedAs once wired.
#[test]
fn runner_rigs_are_dressed() {
    use breakneck_baseball::game::gear::DressedAs;
    use breakneck_baseball::game::runner::Runner;
    use breakneck_baseball::game::scenario::{apply_to_world, presets, PRESET_LOADED};
    let mut app = headless_app();
    start_game(&mut app, KeyCode::Digit1);
    let s = presets().into_iter().find(|s| s.name == PRESET_LOADED).unwrap();
    apply_to_world(app.world_mut(), &s).expect("ball is dead at PrePitch");
    let dressed = run_until(&mut app, 5_000, |app| {
        let world = app.world_mut();
        let runners: Vec<Entity> =
            world.query_filtered::<Entity, With<Runner>>().iter(world).collect();
        runners.len() == 3 && runners.iter().all(|&r| world.get::<DressedAs>(r).is_some())
    });
    assert!(dressed.is_some(), "all three scenario runners must dress");
}
```

(If `ScoreBoard` fields differ (`top_of_inning`, `balls`), match the real struct — check `mod.rs`; the intent is: mutate the resource so `is_changed()` fires with/without a look change. If direct field mutation trips other systems in a way that makes the test flaky, use the scenario seam instead and note it.)

- [ ] **Step 2: Run, fix anything real it catches, full suite, commit**

The churn test is the teeth on Task 1's `DressedAs` guard — if it fails, fix `dress_rigs`' guard, not the test.

```bash
git add tests/e2e_dressing.rs
git commit -m "test: dressing follows identity across flips without per-pitch churn"
```

---

## Phase-exit checklist

- [ ] `cargo test` green except the two known pre-existing failures; wasm + dev checks clean; clippy/fmt clean.
- [ ] Visual tune performed and reported honestly (Task 3 Step 3).
- [ ] TODO.md re-checked.
- [ ] Ledger notes any judgment calls for the Phase 3 planner.
