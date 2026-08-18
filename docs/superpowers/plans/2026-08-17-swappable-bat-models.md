# Swappable Bat Models Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Separate the bat from player.glb into a swappable `bats.glb` library whose grip/contact markers parameterize the analytic contact model, per-player via the creator hub.

**Architecture:** One Blender scene per bat in a new `bats.glb`; marker empties (suffix-resolved, object-scale radii) collapse to a pure `BatProfile` that the `rules::contact_quality` family takes as a parameter with a `NEUTRAL` default. The player rig's `Bat` bone becomes an attachment socket; a `dress_bats` system hangs the identified bat scene off it. CPU batters always grade `NEUTRAL`.

**Tech Stack:** Bevy 0.15 / Rapier, Blender (background scripts), `gltf` crate for contract tests, RON persistence.

**Spec:** `docs/superpowers/specs/2026-08-17-swappable-bat-models-design.md` — read it first; it pins every formula, invariant, and mechanism this plan implements.

## Global Constraints

- Every cargo command needs: `export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"`
- Blender model regeneration is always the pair, in order: `blender --background --python tools/build_<x>.py` then `blender --background assets-src/<x>.blend --python tools/export_glb.py`. Never hand-export.
- After physics/rendering changes verify both targets: `cargo check` AND `cargo check --target wasm32-unknown-unknown`.
- Task 5 onward must also pass `cargo check --features "dev debug"` (the 16-system-param limit only bites there).
- Existing e2e + balance suites must pass unchanged except the tests this plan explicitly edits. No `Ruleset` window retuning.
- Roster names are A–Z only (jersey font); `data/players.ron` is never edited by this plan.
- Commit at the end of every task with the `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>` trailer.

---

### Task 1: docs/BASEBALL.md bat-dimensions section

**Files:**
- Modify: `docs/BASEBALL.md` (append a new section; keep the file's existing heading/source style)

**Interfaces:**
- Produces: the sourced regulation numbers Task 3's authoring comments cite ("per docs/BASEBALL.md").

- [ ] **Step 1: Read the file's existing section style**

Run: `head -60 docs/BASEBALL.md` and skim two existing sections to copy their heading + "Sources:" formatting.

- [ ] **Step 2: Append the bat section**

Add (adapting heading level to the file's convention):

```markdown
## Bats

- A professional (MLB) bat is a smooth, round stick of solid wood, not more
  than 2.61 inches (6.6 cm) in diameter at the thickest part and not more
  than 42 inches (106.7 cm) long (Official Baseball Rules, Rule 3.02).
  Common game lengths are 33–34 in (~0.84–0.86 m).
- Bat mass is typically 31–34 oz (~0.88–0.96 kg) for adult wood bats; the
  "drop" (length in inches minus weight in ounces) is around −3 for pro wood.
- The sweet spot (maximum energy transfer / minimal sting) is centred roughly
  5–7 inches (13–18 cm) from the barrel end, not at the tip.
- The handle tapers to roughly 0.9–1.0 in (~2.4 cm) diameter with a flared
  knob at the base to keep the bottom hand from sliding off.
- **Breakneck deviation:** the in-game `BatClassic` replicates the original
  arcade bat — 0.713 m knob-to-tip (the rig's `Bat` bone head→tail length),
  0.032 m barrel radius — so swapping the bat pipeline in is a visual no-op.
  `BatLumber` (0.86 m) is the regulation-length archetype.

Sources: MLB Official Baseball Rules Rule 3.02 (The Bat); Adair, *The
Physics of Baseball* (sweet spot); standard manufacturer sizing charts
(length/weight/drop).
```

- [ ] **Step 3: Commit**

```bash
git add docs/BASEBALL.md
git commit -m "docs: BASEBALL.md bat dimensions (per swappable-bat spec step 1)"
```

---

### Task 2: `BatId` + `PlayerAppearance.bat` + every appearance-side test edit

This task compiles and tests green on its own. The enum's declaration order (`Lumber, Quick, Classic`) follows the codebase convention: the `#[default] #[serde(other)]` variant is declared **last** (see `Headwear`).

**Files:**
- Modify: `src/game/appearance.rs` (new enum after `CelebrationId` ~line 167; `PlayerAppearance` ~line 183; round-trip test ~line 381; `variants_len...` test ~line 466)
- Modify: `src/game/creator.rs:985` (`randomize_player`'s appearance literal)
- Modify: `tests/appearance_contract.rs:45-56` (`known_fields`), `:142-152` (typo fixture), `:154-159` (count), `:160-169` (field loop)

**Interfaces:**
- Produces: `appearance::BatId` (`Lumber | Quick | Classic`, `Classic` default, `Copy + Eq + Hash + Serialize + Deserialize`, `BatId::NAMES`/`BatId::VARIANTS`), and `PlayerAppearance.bat: BatId`. Tasks 4–7 consume both.

- [ ] **Step 1: Write the failing test edits**

In `tests/appearance_contract.rs`: add the import, grow `known_fields`, and extend the typo fixture + assertions:

```rust
// import list gains BatId:
use breakneck_baseball::game::appearance::{
    embedded_roster_file, Arms, BatId, CelebrationId, Eyewear, FidgetId, Headwear, RosterDefs,
    SkinTone, StanceId, TrotId, APPEARANCE_VERSION, EMBEDDED_PLAYERS_RON,
};

fn known_fields() -> [(&'static str, &'static [&'static str]); 9] {
    [
        ("skin", SkinTone::NAMES),
        ("headwear", Headwear::NAMES),
        ("eyewear", Eyewear::NAMES),
        ("arms", Arms::NAMES),
        ("bat", BatId::NAMES),
        ("stance", StanceId::NAMES),
        ("fidget", FidgetId::NAMES),
        ("trot", TrotId::NAMES),
        ("celebration", CelebrationId::NAMES),
    ]
}
```

In `strict_identifier_check_catches_a_typo_in_every_appearance_field`, add `bat: Mapel,` to the fixture's appearance record (after `arms: Wristband,`), change the count assertion to `problems.len(), 9`, and add `"bat"` to the field loop array.

In `src/game/appearance.rs` tests: add `assert_eq!(BatId::VARIANTS.len(), BatId::NAMES.len());` to `variants_len_matches_names_for_every_appearance_enum`, and give the round-trip test's literal a `bat: BatId::Lumber,` field.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --test appearance_contract 2>&1 | tail -20`
Expected: compile error — `BatId` not found.

- [ ] **Step 3: Implement the enum and field**

In `src/game/appearance.rs`, after the `CelebrationId` block:

```rust
appearance_enum! {
/// Which bat model this player swings — resolved to a scene + measured
/// `BatSpec` by `bat_assets::BatLibrary`. The bat's geometry IS its stats
/// (see the swappable-bat spec §2); this id is only the pointer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BatId {
    Lumber,
    Quick,
    #[default]
    #[serde(other)]
    Classic,
}
}
```

Add to `PlayerAppearance` (after `arms`):

```rust
    pub bat: BatId,
```

In `src/game/creator.rs:985` (`randomize_player`), add to the literal — pinned to Classic here; Task 7 makes it a rolled channel:

```rust
        arms,
        bat: BatId::Classic, // rolled in the Creator task (channel 8)
        chain,
```

and add `BatId` to creator.rs's `appearance::` import list.

- [ ] **Step 4: Run tests to verify pass**

Run: `cargo test --lib appearance && cargo test --test appearance_contract`
Expected: PASS (including the pre-existing serde-default tests — the struct-level `#[serde(default)]` covers the new field).

- [ ] **Step 5: Commit**

```bash
git add src/game/appearance.rs src/game/creator.rs tests/appearance_contract.rs
git commit -m "feat: BatId appearance field with strict-identifier coverage"
```

---

### Task 3: `build_bats.py` + generalized exporter + `bats.glb` + geometric contract test

**Files:**
- Create: `tools/build_bats.py`
- Modify: `tools/export_glb.py` (derive OUT from the opened blend's filename)
- Create: `assets-src/bats.blend` (generated), `src/game/models/bats.glb` (generated, committed)
- Create: `tests/bat_contract.rs` (geometric checks only; names hardcoded — Task 4 rebases them onto `BAT_TABLE`)

**Interfaces:**
- Produces: `src/game/models/bats.glb` with Blender scenes `BatClassic`/`BatLumber`/`BatQuick`, each scene holding one root node of the same name whose direct children are `<BatName>.Mesh` plus five marker empties `<BatName>.Grip.Knob`, `.Grip.Choke`, `.Contact.Start`, `.Contact.Sweet`, `.Contact.End`. Marker object scale = local barrel radius (contact markers), grip markers scale 1. Bats modeled along Blender +Z (knob at origin) → glTF +Y after `export_yup`.

- [ ] **Step 1: Write the failing contract test**

`tests/bat_contract.rs`:

```rust
//! Validates the committed bats.glb against the bat-model contract
//! (docs/superpowers/specs/2026-08-17-swappable-bat-models-design.md §1/§6).
//! Pure gltf-crate parsing — no Bevy app. Task 4 rebases these hardcoded
//! names onto bat_assets::BAT_TABLE and adds profile-derived checks.

const BATS_GLB: &str = "src/game/models/bats.glb";
const BATS: [&str; 3] = ["BatClassic", "BatLumber", "BatQuick"];
const MARKER_SUFFIXES: [&str; 5] = [
    "Grip.Knob",
    "Grip.Choke",
    "Contact.Start",
    "Contact.Sweet",
    "Contact.End",
];
const MAX_BATS_GLB_BYTES: usize = 128 * 1024;
const MAX_BATS_TRIANGLES: usize = 3_000;
/// Sane barrel-radius band (m) for the contact markers' encoded scale.
const RADIUS_BAND: (f32, f32) = (0.015, 0.05);

/// (suffix -> (glTF translation, uniform scale)) for one bat's root node.
fn markers_of(node: &gltf::Node) -> Vec<(String, [f32; 3], [f32; 3])> {
    node.children()
        .filter_map(|c| {
            let name = c.name()?.to_owned();
            let (t, _r, s) = c.transform().decomposed();
            Some((name, t, s))
        })
        .collect()
}

#[test]
fn bats_glb_satisfies_geometric_contract() {
    let bytes = std::fs::read(BATS_GLB).unwrap_or_else(|e| {
        panic!("{BATS_GLB} unreadable ({e}) — run tools/build_bats.py then tools/export_glb.py")
    });
    assert!(
        bytes.len() <= MAX_BATS_GLB_BYTES,
        "bats.glb is {} bytes (ceiling {MAX_BATS_GLB_BYTES}) — the wasm deploy pays for this",
        bytes.len()
    );
    let (doc, _buffers, _images) =
        gltf::import_slice(&bytes).expect("bats.glb failed to parse as glTF");

    let tris: usize = doc
        .meshes()
        .flat_map(|m| m.primitives().map(|p| p.indices().map_or(0, |i| i.count() / 3)).collect::<Vec<_>>())
        .sum();
    assert!(tris > 0 && tris <= MAX_BATS_TRIANGLES, "{tris} triangles (budget {MAX_BATS_TRIANGLES})");

    for bat in BATS {
        // One scene per bat, holding a root node of the same name.
        let scene = doc
            .scenes()
            .find(|s| s.name() == Some(bat))
            .unwrap_or_else(|| panic!("missing scene {bat}"));
        let root = scene
            .nodes()
            .find(|n| n.name() == Some(bat))
            .unwrap_or_else(|| panic!("scene {bat}: missing root node {bat}"));
        let markers = markers_of(&root);

        // All five markers present as DIRECT children, resolved by suffix.
        let find = |suffix: &str| {
            markers
                .iter()
                .find(|(name, _, _)| name.ends_with(suffix))
                .unwrap_or_else(|| panic!("{bat}: no direct child marker with suffix {suffix}"))
        };
        let knob = find("Grip.Knob");
        let choke = find("Grip.Choke");
        let start = find("Contact.Start");
        let sweet = find("Contact.Sweet");
        let end = find("Contact.End");

        // Knob sits at the root origin (the attach alignment point).
        assert!(knob.1[1].abs() < 1e-4, "{bat}: Grip.Knob.y = {}", knob.1[1]);

        // Grip and contact ordering along the grip->barrel axis: glTF +Y
        // (Blender +Z under export_yup — spec §1 axis convention).
        assert!(knob.1[1] < choke.1[1], "{bat}: knob below choke");
        assert!(
            start.1[1] < sweet.1[1] && sweet.1[1] < end.1[1],
            "{bat}: Contact.Start < Sweet < End on glTF Y violated: {} {} {}",
            start.1[1], sweet.1[1], end.1[1]
        );

        // Contact markers encode radius as UNIFORM object scale in a sane band.
        for (label, m) in [("Start", start), ("Sweet", sweet), ("End", end)] {
            let [sx, sy, sz] = m.2;
            assert!(
                (sx - sy).abs() < 1e-4 && (sy - sz).abs() < 1e-4,
                "{bat}: Contact.{label} scale not uniform: {:?}", m.2
            );
            assert!(
                (RADIUS_BAND.0..=RADIUS_BAND.1).contains(&sx),
                "{bat}: Contact.{label} radius {sx} outside {RADIUS_BAND:?}"
            );
        }
    }

    // BatClassic replicates the current in-game bat: tip at 0.713 (spec §1).
    let classic_scene = doc.scenes().find(|s| s.name() == Some("BatClassic")).unwrap();
    let classic_root = classic_scene.nodes().find(|n| n.name() == Some("BatClassic")).unwrap();
    let end_y = markers_of(&classic_root)
        .iter()
        .find(|(n, _, _)| n.ends_with("Contact.End"))
        .unwrap()
        .1[1];
    assert!((end_y - 0.713).abs() < 1e-3, "BatClassic tip at {end_y}, want 0.713");
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --test bat_contract 2>&1 | tail -5`
Expected: FAIL — "src/game/models/bats.glb unreadable".

- [ ] **Step 3: Generalize the exporter**

In `tools/export_glb.py`, replace the hardcoded OUT:

```python
ROOT = os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
# Output derives from the opened blend's filename: player.blend -> player.glb,
# bats.blend -> bats.glb. One pinned-settings exporter, no per-asset copies.
STEM = os.path.splitext(os.path.basename(bpy.data.filepath))[0]
OUT = os.path.join(ROOT, "src", "game", "models", STEM + ".glb")
```

(Everything else — the `export_scene.gltf` call and its settings — stays byte-identical. `use_active_scene` defaults to False, so every scene exports.)

- [ ] **Step 4: Write `tools/build_bats.py`**

```python
"""Builds assets-src/bats.blend from scratch: one Blender SCENE per bat
(Bevy spawns whole glTF scenes, so per-scene = per-spawnable-bat), each
holding a root empty whose direct children are the tapered bat mesh and the
five marker empties the runtime/contract read (spec §1). Bats are modeled
along Blender +Z (knob at origin) so export_yup lands grip->barrel on glTF
+Y, matching the rig's Bat bone head->tail axis. Marker OBJECT SCALE (never
empty display size, which does not export) encodes local barrel radius.

Real dims per docs/BASEBALL.md (Bats): BatClassic deliberately replicates
the original arcade bat (0.713 m, r=0.032) so the swap-in is a visual no-op;
BatLumber is the regulation-length archetype.

Run: blender --background --python tools/build_bats.py
Then: blender --background assets-src/bats.blend --python tools/export_glb.py
"""
import os

import bmesh
import bpy

ROOT = os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
OUT = os.path.join(ROOT, "assets-src", "bats.blend")

# name -> (length, handle_r, barrel_r, choke_z, contact_start_z, sweet_z)
# Contact.End is pinned at the physical tip (= length) per spec §1. The
# authoring coupling (spec §1): Classic's sweet zone is deliberately small
# relative to its contact segment so Quick's perfect_scale > 1 is authorable.
BATS = {
    "BatClassic": (0.713, 0.016, 0.032, 0.18, 0.36, 0.50),
    "BatLumber":  (0.86,  0.017, 0.033, 0.22, 0.40, 0.76),
    "BatQuick":   (0.60,  0.015, 0.030, 0.15, 0.28, 0.44),
}


def taper_radius(z, length, handle_r, barrel_r):
    """Linear handle->barrel taper; the marker radii are measured off the
    same function that shapes the mesh, so the model and its encoded contact
    surface cannot disagree."""
    return handle_r + (barrel_r - handle_r) * (z / length)


def make_material():
    m = bpy.data.materials.new("BatWood")
    m.use_nodes = True
    bsdf = m.node_tree.nodes["Principled BSDF"]
    bsdf.inputs["Base Color"].default_value = (0.72, 0.50, 0.28, 1.0)  # old Bat mat
    bsdf.inputs["Roughness"].default_value = 0.8
    return m


def make_bat_mesh(name, length, handle_r, barrel_r, mat):
    """Tapered cone (knob->barrel) + knob disc, along +Z from z=0..length."""
    bm = bmesh.new()
    ret = bmesh.ops.create_cone(
        bm, cap_ends=True, segments=10, radius1=handle_r, radius2=barrel_r, depth=length
    )
    # create_cone is centred on the origin — shift so the knob end sits at z=0.
    bmesh.ops.translate(bm, verts=ret["verts"], vec=(0, 0, length / 2))
    knob = bmesh.ops.create_cone(
        bm, cap_ends=True, segments=10,
        radius1=handle_r * 1.5, radius2=handle_r * 1.2, depth=0.03,
    )
    bmesh.ops.translate(bm, verts=knob["verts"], vec=(0, 0, 0.015))
    mesh = bpy.data.meshes.new(name)
    bm.to_mesh(mesh)
    bm.free()
    mesh.materials.append(mat)
    return bpy.data.objects.new(name, mesh)


def make_marker(name, z, radius):
    obj = bpy.data.objects.new(name, None)  # empty
    obj.empty_display_type = "PLAIN_AXES"
    obj.location = (0, 0, z)
    if radius is not None:
        # OBJECT scale is what exports; empty_display_size does not.
        obj.scale = (radius, radius, radius)
    return obj


def main():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    mat = make_material()
    default_scene = bpy.context.scene
    for name, (length, handle_r, barrel_r, choke_z, start_z, sweet_z) in BATS.items():
        scene = bpy.data.scenes.new(name)
        root = bpy.data.objects.new(name, None)
        scene.collection.objects.link(root)
        children = [make_bat_mesh(f"{name}.Mesh", length, handle_r, barrel_r, mat)]
        r_at = lambda z: taper_radius(z, length, handle_r, barrel_r)
        children += [
            make_marker(f"{name}.Grip.Knob", 0.0, None),
            make_marker(f"{name}.Grip.Choke", choke_z, None),
            make_marker(f"{name}.Contact.Start", start_z, r_at(start_z)),
            make_marker(f"{name}.Contact.Sweet", sweet_z, r_at(sweet_z)),
            make_marker(f"{name}.Contact.End", length, r_at(length)),
        ]
        for child in children:
            scene.collection.objects.link(child)
            child.parent = root  # direct children of the bat root (spec §1)
    bpy.data.scenes.remove(default_scene)  # only bat scenes export
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    bpy.ops.wm.save_as_mainfile(filepath=OUT)
    print(f"wrote {OUT}")


main()
```

- [ ] **Step 5: Generate and export**

Run:
```bash
blender --background --python tools/build_bats.py
blender --background assets-src/bats.blend --python tools/export_glb.py
```
Expected: `wrote .../assets-src/bats.blend`, then `wrote .../src/game/models/bats.glb`.

- [ ] **Step 6: Run the contract test to verify pass**

Run: `cargo test --test bat_contract`
Expected: PASS. If a marker assertion trips, fix `build_bats.py` (never the committed glb by hand) and re-run the Step 5 pair.

Also confirm the exporter generalization didn't move the player pipeline: `blender --background assets-src/player.blend --python tools/export_glb.py && git diff --stat src/game/models/player.glb` — output path must still be `player.glb` (a re-export may produce byte-level diffs; `git checkout -- src/game/models/player.glb` after confirming the path, since Task 6 regenerates it properly).

- [ ] **Step 7: Commit**

```bash
git add tools/build_bats.py tools/export_glb.py assets-src/bats.blend src/game/models/bats.glb tests/bat_contract.rs
git commit -m "feat: bats.glb library with grip/contact markers + contract test"
```

---

### Task 4: `bat_assets.rs` — `BatSpec`/`BatProfile`/`BatLibrary` + profile-derived contract rows

**Files:**
- Create: `src/game/bat_assets.rs`
- Modify: `src/game/rules.rs` (add `BatProfile` struct + `NEUTRAL` only — function signatures change in Task 5)
- Modify: `src/game/mod.rs` (declare `pub mod bat_assets;`, register `BatAssetsPlugin` in the SECOND `add_plugins` tuple — the first is at Bevy's 15-plugin cap)
- Modify: `tests/bat_contract.rs` (rebase names onto `BAT_TABLE`; add profile monotonicity + window-ordering rows)

**Interfaces:**
- Consumes: `appearance::BatId` (Task 2), `bats.glb` (Task 3).
- Produces:
  - `rules::BatProfile { perfect_scale, solid_scale, exit_scale }` + `BatProfile::NEUTRAL` (all `f32`, `Copy`).
  - `bat_assets::BatSpec` — plain data (`[f32; 3]` positions, no Bevy types), `pub fn new(grip_knob, grip_choke, contact_start, contact_sweet, contact_end: [f32; 3], radii: [f32; 3]) -> BatSpec`, `pub fn profile(&self, classic: &BatSpec) -> BatProfile`.
  - `bat_assets::{BAT_TABLE, MARKER_SUFFIXES, BATS_GLB, MAX_BATS_GLB_BYTES, MAX_BATS_TRIANGLES}` consts.
  - `bat_assets::BatLibrary` resource: `pub fn entry(&self, id: BatId) -> Option<&BatEntry>` (`BatEntry { scene: Handle<Scene>, spec: BatSpec, profile: BatProfile }`), `pub fn profile(&self, id: BatId) -> BatProfile`.
  - `bat_assets::resolve_bat_profile(human: bool, id: BatId, lib: Option<&BatLibrary>) -> BatProfile` (pure; Task 5's flow helper calls it).
  - `bat_assets::BatDressed { pub id: BatId, pub scene: Entity }` component (Task 6's guard; defined here so the dev-reload handler can clear it).
  - `bat_assets::BatAssetsPlugin`.

- [ ] **Step 1: Write the failing unit tests (formula properties, synthetic specs)**

In the new `src/game/bat_assets.rs`, bottom:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::rules::BatProfile;

    /// Straight-axis spec along +Y from knob y=0, tip at `len`.
    fn spec(len: f32, choke: f32, start: f32, sweet: f32, r_sweet: f32) -> BatSpec {
        BatSpec::new(
            [0.0, 0.0, 0.0],
            [0.0, choke, 0.0],
            [0.0, start, 0.0],
            [0.0, sweet, 0.0],
            [0.0, len, 0.0],
            [r_sweet * 0.9, r_sweet, r_sweet * 1.05],
        )
    }

    #[test]
    fn profile_of_classic_against_itself_is_exactly_neutral() {
        let classic = spec(0.713, 0.18, 0.36, 0.50, 0.0272);
        assert_eq!(classic.profile(&classic), BatProfile::NEUTRAL);
    }

    #[test]
    fn derived_lengths_match_the_spec_formulas() {
        let s = spec(0.713, 0.18, 0.36, 0.50, 0.0272);
        assert!((s.contact_len() - 0.353).abs() < 1e-6);
        // sweet_len = 2 * min(0.50-0.36, 0.713-0.50) = 2 * 0.14
        assert!((s.sweet_len() - 0.28).abs() < 1e-6);
        // mass_proxy = knob-to-tip * r_sweet^2
        assert!((s.mass_proxy() - 0.713 * 0.0272 * 0.0272).abs() < 1e-9);
    }

    #[test]
    fn longer_heavier_bat_trades_windows_for_exit() {
        let classic = spec(0.713, 0.18, 0.36, 0.50, 0.0272);
        // Longer bat, sweet point pushed near the tip (smaller sweet zone),
        // fatter sweet radius -> more mass.
        let lumber = spec(0.86, 0.22, 0.40, 0.76, 0.0311);
        let p = lumber.profile(&classic);
        assert!(p.perfect_scale < 1.0, "perfect {}", p.perfect_scale);
        assert!(p.solid_scale > 1.0, "solid {}", p.solid_scale);
        assert!(p.exit_scale > 1.0, "exit {}", p.exit_scale);
    }

    #[test]
    fn shorter_lighter_bat_trades_exit_for_windows() {
        let classic = spec(0.713, 0.18, 0.36, 0.50, 0.0272);
        let quick = spec(0.60, 0.15, 0.28, 0.44, 0.026);
        let p = quick.profile(&classic);
        assert!(p.perfect_scale > 1.0, "perfect {}", p.perfect_scale);
        assert!(p.solid_scale < 1.0, "solid {}", p.solid_scale);
        assert!(p.exit_scale < 1.0, "exit {}", p.exit_scale);
    }

    #[test]
    fn resolve_bat_profile_is_neutral_for_cpu_and_while_loading() {
        // CPU: NEUTRAL regardless of id or library presence.
        assert_eq!(resolve_bat_profile(false, BatId::Lumber, None), BatProfile::NEUTRAL);
        // Human but the asset hasn't landed yet: NEUTRAL fallback.
        assert_eq!(resolve_bat_profile(true, BatId::Lumber, None), BatProfile::NEUTRAL);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib bat_assets 2>&1 | tail -5`
Expected: compile error — module doesn't exist.

- [ ] **Step 3: Add `BatProfile` to `rules.rs`**

Near the Contact quality section header (~line 1362):

```rust
/// The bat's collapse into the analytic contact model (swappable-bat spec
/// §2): pure scales over the `Ruleset` timing windows and exit multipliers,
/// derived from measured bat geometry by `bat_assets::BatSpec::profile`.
/// `NEUTRAL` reproduces the pre-bat-library behaviour bit-for-bit — the CPU
/// always grades with it (spec §4), so `tests/balance_sim.rs` stays the
/// untouched arbiter of the offensive economy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BatProfile {
    pub perfect_scale: f32,
    pub solid_scale: f32,
    pub exit_scale: f32,
}

impl BatProfile {
    pub const NEUTRAL: BatProfile = BatProfile {
        perfect_scale: 1.0,
        solid_scale: 1.0,
        exit_scale: 1.0,
    };
}
```

- [ ] **Step 4: Write `src/game/bat_assets.rs`**

```rust
//! The glTF bat library: contract constants shared by the runtime loader and
//! `tests/bat_contract.rs`, the marker extraction into [`BatSpec`], and the
//! [`BatLibrary`] resource gameplay reads. Mirrors `model_assets.rs`. The
//! bat's measured geometry IS its gameplay profile (spec §2) — no side table.

use std::collections::HashMap;

use bevy::asset::embedded_asset;
use bevy::gltf::{Gltf, GltfNode};
use bevy::prelude::*;

use crate::game::appearance::BatId;
use crate::game::rules::BatProfile;

/// Repo-relative path of the committed library (contract test + exporter).
pub const BATS_GLB: &str = "src/game/models/bats.glb";

/// BatId -> the bat's Blender scene / root-node name in bats.glb.
pub const BAT_TABLE: &[(BatId, &str)] = &[
    (BatId::Classic, "BatClassic"),
    (BatId::Lumber, "BatLumber"),
    (BatId::Quick, "BatQuick"),
];

/// Marker names are authored with a per-bat prefix (Blender object names are
/// globally unique per FILE), so both the runtime and the contract resolve
/// them as children-of-root matched by these suffixes (spec §1).
pub const MARKER_SUFFIXES: [&str; 5] = [
    "Grip.Knob",
    "Grip.Choke",
    "Contact.Start",
    "Contact.Sweet",
    "Contact.End",
];

pub const MAX_BATS_GLB_BYTES: usize = 128 * 1024;
pub const MAX_BATS_TRIANGLES: usize = 3_000;

/// One bat's measured geometry, bat-local (glTF axes: +Y = grip->barrel).
/// Plain data with no Bevy types so `tests/bat_contract.rs` can build one
/// straight from gltf-crate parsing and compute profiles without an App.
#[derive(Clone, Debug, PartialEq)]
pub struct BatSpec {
    pub grip_knob: [f32; 3],
    pub grip_choke: [f32; 3],
    pub contact_start: [f32; 3],
    pub contact_sweet: [f32; 3],
    pub contact_end: [f32; 3],
    /// Barrel radii at Contact.Start/Sweet/End (the markers' object scale).
    pub r_start: f32,
    pub r_sweet: f32,
    pub r_end: f32,
}

impl BatSpec {
    pub fn new(
        grip_knob: [f32; 3],
        grip_choke: [f32; 3],
        contact_start: [f32; 3],
        contact_sweet: [f32; 3],
        contact_end: [f32; 3],
        radii: [f32; 3],
    ) -> BatSpec {
        BatSpec {
            grip_knob,
            grip_choke,
            contact_start,
            contact_sweet,
            contact_end,
            r_start: radii[0],
            r_sweet: radii[1],
            r_end: radii[2],
        }
    }

    /// Contact-segment length along the grip->barrel axis (glTF Y).
    pub fn contact_len(&self) -> f32 {
        self.contact_end[1] - self.contact_start[1]
    }

    /// Sweet-zone length: the symmetric band around the sweet point, capped
    /// by the contact segment's ends (spec §2: 2·min(Sweet−Start, End−Sweet)).
    pub fn sweet_len(&self) -> f32 {
        2.0 * (self.contact_sweet[1] - self.contact_start[1])
            .min(self.contact_end[1] - self.contact_sweet[1])
    }

    /// Knob-to-tip length times sweet-radius squared (spec §2). Contact.End
    /// is pinned at the physical tip by the contract, so this is honest.
    pub fn mass_proxy(&self) -> f32 {
        (self.contact_end[1] - self.grip_knob[1]) * self.r_sweet * self.r_sweet
    }

    /// The pure rules-facing collapse: geometry ratios against the Classic
    /// spec (spec §2's pinned formulas). Ratios of identical f32s are exactly
    /// 1.0, so `classic.profile(classic) == NEUTRAL` exactly.
    pub fn profile(&self, classic: &BatSpec) -> BatProfile {
        BatProfile {
            perfect_scale: self.sweet_len() / classic.sweet_len(),
            solid_scale: self.contact_len() / classic.contact_len(),
            exit_scale: (self.mass_proxy() / classic.mass_proxy()).sqrt(),
        }
    }
}

/// Both NEUTRAL rules in one place (spec §4): the CPU always grades NEUTRAL
/// (mirrors `batting::style_for`'s CPU-always-Classic), and a human grades
/// NEUTRAL until the library has actually built. Pure for unit tests;
/// `flow::bat_profile_for` is the thin system-side wrapper.
pub fn resolve_bat_profile(human: bool, id: BatId, lib: Option<&BatLibrary>) -> BatProfile {
    if !human {
        return BatProfile::NEUTRAL;
    }
    lib.map(|l| l.profile(id)).unwrap_or(BatProfile::NEUTRAL)
}

pub struct BatEntry {
    pub scene: Handle<Scene>,
    pub spec: BatSpec,
    pub profile: BatProfile,
}

/// Built once bats.glb loads: everything gameplay needs per bat. The full
/// `BatSpec` geometry stays here on purpose — the future bat-space contact
/// (spec option B) and collider (option C) read it without asset rework.
#[derive(Resource)]
pub struct BatLibrary {
    entries: HashMap<BatId, BatEntry>,
}

impl BatLibrary {
    pub fn entry(&self, id: BatId) -> Option<&BatEntry> {
        self.entries.get(&id)
    }
    pub fn profile(&self, id: BatId) -> BatProfile {
        self.entries.get(&id).map(|e| e.profile).unwrap_or(BatProfile::NEUTRAL)
    }
}

/// Guard stamped on a `Batter` rig ONLY after its bat scene actually spawned
/// (spec §3): a rig dressed before the library exists retries every frame
/// and self-heals, instead of a change-filter trigger being consumed by an
/// early return. `scene` is the only reference to the spawned bat — despawn
/// through it, never by child-walking.
#[derive(Component)]
pub struct BatDressed {
    pub id: BatId,
    pub scene: Entity,
}

/// Asset path the runtime loads (mirror of `player_model_path`).
pub fn bat_model_path() -> &'static str {
    if cfg!(feature = "dev") {
        "game/models/bats.glb"
    } else {
        "embedded://breakneck_baseball/game/models/bats.glb"
    }
}

#[derive(Resource)]
struct BatModelHandle(Handle<Gltf>);

/// Marker mirroring `RigAnimationsFailed`: a permanently-broken glb logs
/// once instead of every frame.
#[derive(Resource)]
struct BatLibraryFailed;

fn load_bat_model(asset_server: Res<AssetServer>, mut commands: Commands) {
    commands.insert_resource(BatModelHandle(asset_server.load(bat_model_path())));
}

/// Suffix-resolved marker extraction from one bat's root node. Direct
/// children only (spec §1: node-local == bat-local).
fn extract_spec(root: &GltfNode, nodes: &Assets<GltfNode>) -> Result<BatSpec, String> {
    let mut found: HashMap<&'static str, ([f32; 3], f32)> = HashMap::new();
    for child_handle in &root.children {
        let Some(child) = nodes.get(child_handle) else {
            return Err("child node not loaded yet".into());
        };
        for suffix in MARKER_SUFFIXES {
            if child.name.ends_with(suffix) {
                let t = child.transform.translation;
                let s = child.transform.scale;
                found.insert(suffix, ([t.x, t.y, t.z], s.x));
            }
        }
    }
    let get = |suffix: &str| {
        found
            .get(suffix)
            .copied()
            .ok_or_else(|| format!("missing marker suffix {suffix}"))
    };
    let (knob, _) = get("Grip.Knob")?;
    let (choke, _) = get("Grip.Choke")?;
    let (start, r_start) = get("Contact.Start")?;
    let (sweet, r_sweet) = get("Contact.Sweet")?;
    let (end, r_end) = get("Contact.End")?;
    Ok(BatSpec::new(knob, choke, start, sweet, end, [r_start, r_sweet, r_end]))
}

/// Polls until the Gltf (and its nodes) are in, then builds the library by
/// BAT_TABLE name lookup — never by index. Runs behind a run_if so it costs
/// nothing once built (the `build_rig_animations` pattern).
fn build_bat_library(
    mut commands: Commands,
    handle: Res<BatModelHandle>,
    gltfs: Res<Assets<Gltf>>,
    nodes: Res<Assets<GltfNode>>,
) {
    let Some(gltf) = gltfs.get(&handle.0) else {
        return;
    };
    let mut specs: HashMap<BatId, BatSpec> = HashMap::new();
    for (id, name) in BAT_TABLE {
        let Some(root) = gltf.named_nodes.get(*name).and_then(|h| nodes.get(h)) else {
            return; // nodes still streaming in — retry next frame
        };
        match extract_spec(root, &nodes) {
            Ok(spec) => {
                specs.insert(*id, spec);
            }
            Err(e) if e.contains("not loaded yet") => return, // retry
            Err(e) => {
                error!("bats.glb {name}: {e} — bat contract violated");
                commands.insert_resource(BatLibraryFailed);
                return;
            }
        }
    }
    let classic = specs[&BatId::Classic].clone();
    let entries = BAT_TABLE
        .iter()
        .map(|(id, name)| {
            let spec = specs.remove(id).expect("filled above");
            let profile = spec.profile(&classic);
            let scene = gltf.named_scenes[*name].clone();
            (*id, BatEntry { scene, spec, profile })
        })
        .collect();
    commands.insert_resource(BatLibrary { entries });
}

/// Dev-only Blender round-trip: a re-export must refresh the extracted
/// specs AND the in-hand visuals. Despawns each stamp's stored scene BEFORE
/// removing the stamp (clearing alone would orphan the old scene under the
/// bone and leave duplicate bats — spec §2), then drops the library so the
/// poll rebuilds. No ordering constraint vs `dress_bats`: both act through
/// Commands; worst case the re-dress lands a frame later.
#[cfg(feature = "dev")]
fn reload_bat_library(
    mut events: EventReader<AssetEvent<Gltf>>,
    handle: Res<BatModelHandle>,
    mut commands: Commands,
    dressed: Query<(Entity, &BatDressed)>,
) {
    for ev in events.read() {
        if let AssetEvent::Modified { id } = ev {
            if *id == handle.0.id() {
                for (rig, d) in &dressed {
                    commands.entity(d.scene).despawn_recursive();
                    commands.entity(rig).remove::<BatDressed>();
                }
                commands.remove_resource::<BatLibrary>();
                commands.remove_resource::<BatLibraryFailed>();
                info!("bats.glb reloaded — bat library rebuilding");
            }
        }
    }
}

pub struct BatAssetsPlugin;

impl Plugin for BatAssetsPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "models/bats.glb");
        app.add_systems(Startup, load_bat_model).add_systems(
            Update,
            build_bat_library.run_if(
                |built: Option<Res<BatLibrary>>, failed: Option<Res<BatLibraryFailed>>| {
                    built.is_none() && failed.is_none()
                },
            ),
        );
        #[cfg(feature = "dev")]
        app.add_systems(Update, reload_bat_library);
    }
}
```

Note: this plan was verified against Bevy 0.15.3 where `GltfNode` exposes `name: String`, `children: Vec<Handle<GltfNode>>`, and `transform: Transform`. If the vendored patch differs (e.g. inline child values), adapt `extract_spec`'s lookups accordingly — the suffix-matching logic is the contract, not the handle plumbing.

- [ ] **Step 5: Register the module and plugin**

In `src/game/mod.rs`: add `pub mod bat_assets;` beside the other module decls, and `bat_assets::BatAssetsPlugin` into the SECOND `add_plugins` tuple (after `JuicePlugin`).

- [ ] **Step 6: Run unit tests**

Run: `cargo test --lib bat_assets`
Expected: all five tests PASS.

- [ ] **Step 7: Rebase `tests/bat_contract.rs` onto the lib and add profile rows**

Replace the local `BATS`/`MARKER_SUFFIXES`/budget consts with imports, and append the profile-derived checks:

```rust
use breakneck_baseball::game::appearance::BatId;
use breakneck_baseball::game::bat_assets::{
    BatSpec, BAT_TABLE, BATS_GLB, MARKER_SUFFIXES, MAX_BATS_GLB_BYTES, MAX_BATS_TRIANGLES,
};
use breakneck_baseball::game::rules::BatProfile;
use breakneck_baseball::game::variant::VariantId;
```

(Iterate `BAT_TABLE.iter().map(|(_, name)| *name)` where the old `BATS` array was; keep the geometric assertions identical. `MARKER_SUFFIXES` replaces the local copy.)

New test in the same file — build each `BatSpec` via the pure constructor from the parsed markers, then pin the six monotonicity directions and the window-ordering invariant per shipped variant:

```rust
fn spec_from_gltf(doc: &gltf::Document, bat: &str) -> BatSpec {
    let scene = doc.scenes().find(|s| s.name() == Some(bat)).unwrap();
    let root = scene.nodes().find(|n| n.name() == Some(bat)).unwrap();
    let find = |suffix: &str| {
        root.children()
            .find(|c| c.name().is_some_and(|n| n.ends_with(suffix)))
            .unwrap_or_else(|| panic!("{bat}: missing {suffix}"))
            .transform()
            .decomposed()
    };
    let m = |suffix: &str| find(suffix).0;
    let r = |suffix: &str| find(suffix).2[0];
    BatSpec::new(
        m("Grip.Knob"),
        m("Grip.Choke"),
        m("Contact.Start"),
        m("Contact.Sweet"),
        m("Contact.End"),
        [r("Contact.Start"), r("Contact.Sweet"), r("Contact.End")],
    )
}

#[test]
fn shipped_bat_profiles_hold_directions_and_window_ordering() {
    let bytes = std::fs::read(BATS_GLB).unwrap();
    let (doc, _, _) = gltf::import_slice(&bytes).unwrap();
    let classic = spec_from_gltf(&doc, "BatClassic");

    assert_eq!(classic.profile(&classic), BatProfile::NEUTRAL);

    let lumber = spec_from_gltf(&doc, "BatLumber").profile(&classic);
    assert!(lumber.perfect_scale < 1.0 && lumber.solid_scale > 1.0 && lumber.exit_scale > 1.0,
        "Lumber directions violated: {lumber:?}");
    let quick = spec_from_gltf(&doc, "BatQuick").profile(&classic);
    assert!(quick.perfect_scale > 1.0 && quick.solid_scale < 1.0 && quick.exit_scale < 1.0,
        "Quick directions violated: {quick:?}");

    // Window-ordering invariant per bat x shipped variant (spec §4):
    // perfect_scale*perfect_ms <= solid_scale*solid_ms <= foul_ms.
    for variant in [VariantId::Standard, VariantId::FrontYard] {
        let b = variant.rules().batting;
        for (label, p) in [("Classic", BatProfile::NEUTRAL), ("Lumber", lumber), ("Quick", quick)] {
            let scaled_perfect = p.perfect_scale * b.perfect_ms;
            let scaled_solid = p.solid_scale * b.solid_ms;
            assert!(
                scaled_perfect <= scaled_solid && scaled_solid <= b.foul_ms,
                "{label} x {variant:?}: {scaled_perfect} / {scaled_solid} / {} not ordered",
                b.foul_ms
            );
        }
    }
}
```

(If `VariantId` or `BattingTuning` fields aren't `pub`-reachable from the test crate, make the minimal visibility fix in `variant.rs` rather than duplicating window numbers.)

- [ ] **Step 8: Run all touched tests**

Run: `cargo test --test bat_contract && cargo test --lib bat_assets && cargo check`
Expected: PASS / PASS / clean.

- [ ] **Step 9: Commit**

```bash
git add src/game/bat_assets.rs src/game/rules.rs src/game/mod.rs tests/bat_contract.rs
git commit -m "feat: BatSpec/BatProfile/BatLibrary with profile-derived contract rows"
```

---

### Task 5: Rules parameterization + `flow::bat_profile_for`

**Files:**
- Modify: `src/game/rules.rs` (`contact_quality:1395`, `apply_contact_quality:1423`, `pci_contact_quality:1455`, new `effective_windows`; every rules unit test calling these gains `&BatProfile::NEUTRAL`)
- Modify: `src/game/flow.rs` (`pitch_live:625` — new grouped `SystemParam`; profile passed at `:679/:681/:712`)

**Interfaces:**
- Consumes: `rules::BatProfile` (Task 4), `bat_assets::{resolve_bat_profile, BatLibrary}` (Task 4), `input::Controllers::player_index`, `Rosters::team(..).batting(..)`, `BattingOrder::current`.
- Produces: `rules::effective_windows(rules: &Ruleset, profile: &BatProfile) -> (f32, f32)` (eff_perfect, eff_solid); the three grading fns' new `profile: &BatProfile` last parameter; `flow::BatParams` SystemParam + `flow::bat_profile_for(batting: Team, order: &BattingOrder, p: &BatParams) -> BatProfile`.

- [ ] **Step 1: Write the failing rules unit tests**

Append to rules.rs's test module (near the existing contact-quality tests ~line 3055):

```rust
#[test]
fn effective_windows_clamp_chain_never_inverts() {
    let rules = crate::game::variant::VariantId::Standard.rules();
    // Hostile profile: solid pushed past foul, perfect pushed past solid.
    let hostile = BatProfile { perfect_scale: 10.0, solid_scale: 10.0, exit_scale: 1.0 };
    let (p, s) = effective_windows(&rules, &hostile);
    assert!(p <= s && s <= rules.batting.foul_ms, "{p} / {s} / {}", rules.batting.foul_ms);
    // NEUTRAL reproduces the raw windows exactly.
    let (p, s) = effective_windows(&rules, &BatProfile::NEUTRAL);
    assert_eq!((p, s), (rules.batting.perfect_ms, rules.batting.solid_ms));
}

#[test]
fn bat_profile_scales_grading_windows() {
    let rules = crate::game::variant::VariantId::Standard.rules(); // 40/90/130
    let big = BatProfile { perfect_scale: 1.5, solid_scale: 1.2, exit_scale: 1.0 };
    // 50ms: FoulTip-adjacent under NEUTRAL (over 40), Perfect under 1.5x.
    assert_eq!(contact_quality(50.0, &rules, &BatProfile::NEUTRAL), ContactQuality::Solid);
    assert_eq!(contact_quality(50.0, &rules, &big), ContactQuality::Perfect);
    // 100ms: FoulTip under NEUTRAL (over 90), Solid under 1.2x (108).
    assert_eq!(contact_quality(100.0, &rules, &BatProfile::NEUTRAL), ContactQuality::FoulTip);
    assert_eq!(contact_quality(100.0, &rules, &big), ContactQuality::Solid);
    // foul_ms is unscaled: 131ms whiffs under any profile.
    assert_eq!(contact_quality(131.0, &rules, &big), ContactQuality::Whiff);
}

#[test]
fn pci_composition_scales_base_windows_before_the_miss_shrink() {
    let rules = crate::game::variant::VariantId::Standard.rules();
    let big = BatProfile { perfect_scale: 1.5, solid_scale: 1.2, exit_scale: 1.0 };
    // Half-radius miss: effective perfect = eff_perfect*(1-0.5).
    // NEUTRAL: 40*0.5=20 -> 25ms is Solid. Scaled: 60*0.5=30 -> Perfect.
    let half_miss = rules.batting.pci_radius_m * 0.5;
    assert_eq!(
        pci_contact_quality(25.0, half_miss, &rules, &BatProfile::NEUTRAL),
        ContactQuality::Solid
    );
    assert_eq!(pci_contact_quality(25.0, half_miss, &rules, &big), ContactQuality::Perfect);
}

#[test]
fn exit_scale_multiplies_the_quality_exit() {
    let rules = crate::game::variant::VariantId::Standard.rules();
    let hot = BatProfile { perfect_scale: 1.0, solid_scale: 1.0, exit_scale: 1.2 };
    let base = Vec3::new(0.0, 10.0, 30.0);
    let neutral = apply_contact_quality(base, ContactQuality::Solid, 0.0, &rules, &BatProfile::NEUTRAL);
    let scaled = apply_contact_quality(base, ContactQuality::Solid, 0.0, &rules, &hot);
    assert!((scaled.length() / neutral.length() - 1.2).abs() < 1e-5);
    // Whiff/FoulTip return base unchanged regardless of profile.
    assert_eq!(apply_contact_quality(base, ContactQuality::Whiff, 0.0, &rules, &hot), base);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib rules::tests 2>&1 | tail -5`
Expected: compile errors (wrong arity — the new tests pass 3/4/5 args).

- [ ] **Step 3: Implement `effective_windows` and re-sign the three functions**

In rules.rs (before `contact_quality`):

```rust
/// The single window-scaling seam all three grading paths share (spec §4).
/// `foul_ms` is deliberately unscaled — it is also the spatial reach gate
/// (`flow::late_swing_z`, the Swing Meter's forced-whiff edge). The pinned
/// clamp chain guarantees ordering for ALL inputs, including hostile
/// debug-Tune live edits: eff_solid = min(solid_scale*solid_ms, foul_ms);
/// eff_perfect = min(perfect_scale*perfect_ms, eff_solid).
pub fn effective_windows(rules: &Ruleset, profile: &BatProfile) -> (f32, f32) {
    let eff_solid = (profile.solid_scale * rules.batting.solid_ms).min(rules.batting.foul_ms);
    let eff_perfect = (profile.perfect_scale * rules.batting.perfect_ms).min(eff_solid);
    (eff_perfect, eff_solid)
}
```

`contact_quality` gains `profile: &BatProfile` and grades off the effective windows:

```rust
pub fn contact_quality(dt_ms: f32, rules: &Ruleset, profile: &BatProfile) -> ContactQuality {
    let dt = dt_ms.abs();
    let (eff_perfect, eff_solid) = effective_windows(rules, profile);
    if dt <= eff_perfect {
        ContactQuality::Perfect
    } else if dt <= eff_solid {
        ContactQuality::Solid
    } else if dt <= rules.batting.foul_ms {
        ContactQuality::FoulTip
    } else {
        ContactQuality::Whiff
    }
}
```

`pci_contact_quality` scales the BASE windows first (spec §4 — one consistent effective Ruleset), including the Weak clip band:

```rust
pub fn pci_contact_quality(
    dt_ms: f32,
    miss_m: f32,
    rules: &Ruleset,
    profile: &BatProfile,
) -> ContactQuality {
    let dt = dt_ms.abs();
    if dt > rules.batting.foul_ms {
        return ContactQuality::Whiff;
    }
    let frac = (miss_m / rules.batting.pci_radius_m).max(0.0);
    if frac > 1.0 {
        return ContactQuality::FoulTip;
    }
    let (base_perfect, base_solid) = effective_windows(rules, profile);
    let perfect_eff = base_perfect * (1.0 - frac);
    let solid_eff = base_solid * (1.0 - frac / 2.0);
    if dt <= perfect_eff {
        ContactQuality::Perfect
    } else if dt <= solid_eff {
        ContactQuality::Solid
    } else if dt <= base_solid {
        ContactQuality::Weak
    } else {
        ContactQuality::FoulTip
    }
}
```

`apply_contact_quality` gains the parameter and multiplies exit:

```rust
pub fn apply_contact_quality(
    base: Vec3,
    quality: ContactQuality,
    dt_ms: f32,
    rules: &Ruleset,
    profile: &BatProfile,
) -> Vec3 {
    let exit_mult = match quality {
        ContactQuality::Perfect => rules.batting.exit_perfect,
        ContactQuality::Solid => rules.batting.exit_solid,
        ContactQuality::Weak => rules.batting.exit_weak,
        ContactQuality::Whiff | ContactQuality::FoulTip => return base,
    } * profile.exit_scale;
    // ... (rest of the body unchanged: scale, pull-yaw rotation)
```

Then update every pre-existing rules unit test call site to pass `&BatProfile::NEUTRAL` as the new last argument (mechanical; the compiler enumerates them).

- [ ] **Step 4: Wire flow**

In `src/game/flow.rs`:

```rust
use crate::game::bat_assets::{resolve_bat_profile, BatLibrary};
use crate::game::input::Controllers;
use crate::game::roster::Rosters;
use bevy::ecs::system::SystemParam;

/// Grouped so `pitch_live` stays under Bevy's 16-system-param limit — it is
/// at 14 under `--features debug`; three loose additions would hit 17 and
/// fail to compile only in that configuration (spec §4).
#[derive(SystemParam)]
pub struct BatParams<'w> {
    rosters: Res<'w, Rosters>,
    controllers: Res<'w, Controllers>,
    library: Option<Res<'w, BatLibrary>>,
}

/// The single convergence point for both NEUTRAL rules (spec §4): CPU
/// batters and not-yet-loaded libraries grade NEUTRAL; a human batter gets
/// their roster bat's measured profile.
pub fn bat_profile_for(
    batting: crate::game::Team,
    order: &rules::BattingOrder,
    p: &BatParams,
) -> rules::BatProfile {
    let human = p.controllers.player_index(batting).is_some();
    let id = p
        .rosters
        .team(batting)
        .batting(order.current(batting))
        .appearance
        .bat;
    resolve_bat_profile(human, id, p.library.as_deref())
}
```

`pitch_live` gains one param (`bat: BatParams,`) after `order`. At the swing site (just after `let dt_ms = ...` at ~line 671):

```rust
        let profile = bat_profile_for(batter, &order, &bat);
```

and thread it: `rules::pci_contact_quality(dt_ms, miss, &rules, &profile)`, `rules::contact_quality(dt_ms, &rules, &profile)`, `rules::apply_contact_quality(base, quality, dt_ms, &rules, &profile)`. The `#[cfg(feature = "debug")] ForcedContact` override arm is untouched (forced qualities still take `exit_scale` — accepted in the spec).

- [ ] **Step 5: Run the full gate**

Run:
```bash
cargo test --lib && cargo check --features "dev debug" && cargo check --target wasm32-unknown-unknown
```
Expected: all rules/flow/bat tests PASS; both feature/target checks clean. The `dev debug` check is the 16-param regression guard.

- [ ] **Step 6: Run the e2e suites (must pass unchanged)**

Run: `cargo test --test e2e_full_game --test e2e_advanced_rules --test e2e_cpu`
Expected: PASS with zero edits — every default is Classic → NEUTRAL.

- [ ] **Step 7: Commit**

```bash
git add src/game/rules.rs src/game/flow.rs
git commit -m "feat: BatProfile parameterizes contact grading; CPU pinned NEUTRAL"
```

---

### Task 6: Player-model surgery + `dress_bats` + e2e rewrite

**Files:**
- Modify: `tools/build_player.py` (delete `_bat_mesh_part()` + its `PARTS.append`, delete the `"Bat"` row from `MATERIALS`; the `Bat` BONE row and every clip's `"Bat"` channels STAY)
- Regenerate: `assets-src/player.blend`, `src/game/models/player.glb`
- Modify: `tests/model_contract.rs` (drop `BAT_MATERIAL` import + its row in the material loop)
- Modify: `src/game/model_assets.rs` (delete `BAT_MATERIAL:17`, `RigAnimations.bat_material:110`, its lookup `:267-271`/`:300`, `bat_meshes` collection `:372/:396-397`, the visibility loop `:453-460`, and `Has<Batter>` from `wire_rigs`' query; keep the `"Bat"` name→`RigBones.bat` wiring and `ATTACH_BONES`)
- Modify: `src/game/gear.rs` (new `dress_bats` system registered in `GearPlugin`)
- Modify: `tests/e2e_gltf_rig.rs` (rewrite `bat_shows_only_on_the_batter`)

**Interfaces:**
- Consumes: `bat_assets::{BatLibrary, BatDressed}` (Task 4), `RigBones.bat`, `player::Batter`, `roster::PlayerIdentity`.
- Produces: the bat as a spawned scene child of `RigBones.bat` on `Batter` rigs only; `RigAnimations` without `bat_material` (compile-visible to any stale consumer).

- [ ] **Step 1: Rewrite the e2e test first (it will fail until dress_bats lands)**

Replace `bat_shows_only_on_the_batter` in `tests/e2e_gltf_rig.rs`:

```rust
use breakneck_baseball::game::bat_assets::BatDressed;

/// The bat is no longer skinned into player.glb — `gear::dress_bats` spawns
/// the roster-identified bat scene under the Batter rig's `Bat` bone (the
/// socket every swing clip animates), and ONLY there: run-out rigs
/// (`RigUnit::Batter` but no `Batter` marker), fielders, and umpires carry
/// no bat scene at all.
#[test]
fn bat_scene_dresses_only_the_batter() {
    let mut app = common::headless_app();
    common::start_game(&mut app, KeyCode::Digit2);

    // The bat library loads async like the rig scenes; run until the plate
    // batter's rig carries a stamped, spawned bat.
    let dressed = common::run_until(&mut app, 4_000, |app| {
        let world = app.world_mut();
        world
            .query_filtered::<&BatDressed, With<Batter>>()
            .iter(world)
            .next()
            .is_some()
    });
    assert!(dressed.is_some(), "batter rig never got a BatDressed bat scene");

    let world = app.world_mut();

    // Exactly the Batter-marked rigs are dressed.
    let batter_stamps = world
        .query_filtered::<(), (With<BatDressed>, With<Batter>)>()
        .iter(world)
        .count();
    let stray_stamps = world
        .query_filtered::<(), (With<BatDressed>, Without<Batter>)>()
        .iter(world)
        .count();
    assert!(batter_stamps >= 1, "no Batter rig dressed");
    assert_eq!(stray_stamps, 0, "non-Batter rigs must never dress a bat");

    // The stamped scene really hangs off the rig's Bat bone.
    let (bones, stamp) = world
        .query_filtered::<(&RigBones, &BatDressed), With<Batter>>()
        .iter(world)
        .next()
        .expect("dressed batter");
    let (bat_bone, scene) = (bones.bat, stamp.scene);
    let parent = world
        .get::<Parent>(scene)
        .expect("bat scene must be parented")
        .get();
    assert_eq!(parent, bat_bone, "bat scene must be a child of the Bat bone");
}
```

Run: `cargo test --test e2e_gltf_rig 2>&1 | tail -5` — expected: the new test FAILS (no `dress_bats` yet); `bat_shows_only_on_the_batter` is gone.

- [ ] **Step 2: Implement `dress_bats` in gear.rs**

```rust
use crate::game::bat_assets::{BatDressed, BatLibrary};
use crate::game::model_assets::RigBones;
use crate::game::player::Batter;

/// Hangs the roster-identified bat scene off the Batter rig's `Bat` bone
/// (spec §3). Presence-marker guard, NOT change filters: `BatDressed` is
/// stamped only after a successful spawn, so a rig wired before the library
/// exists retries every frame and self-heals the moment the asset lands.
/// Run-out rigs carry `RigUnit::Batter` without the `Batter` marker and stay
/// bat-less (the old `wire_rigs` semantics, pinned by e2e_gltf_rig).
fn dress_bats(
    mut commands: Commands,
    rosters: Res<Rosters>,
    library: Option<Res<BatLibrary>>,
    rigs: Query<(Entity, &PlayerIdentity, &RigBones, Option<&BatDressed>), With<Batter>>,
) {
    let Some(library) = library else { return };
    for (rig, id, bones, dressed) in &rigs {
        let bat = rosters.team(id.team).card(id.index).appearance.bat;
        if dressed.map(|d| d.id) == Some(bat) {
            continue; // same lumber — per-pitch identity re-stamp, nothing to do
        }
        if let Some(d) = dressed {
            commands.entity(d.scene).despawn_recursive();
        }
        let Some(entry) = library.entry(bat) else { continue };
        // Grip.Knob -> bone origin (the hands); orientation is identity —
        // the bat's +Y is the bone's +Y by the export axis convention, and
        // bats are rotationally symmetric.
        let knob = entry.spec.grip_knob;
        let scene = commands
            .spawn((
                SceneRoot(entry.scene.clone()),
                Transform::from_translation(-Vec3::new(knob[0], knob[1], knob[2])),
            ))
            .id();
        commands.entity(bones.bat).add_child(scene);
        commands.entity(rig).insert(BatDressed { id: bat, scene });
    }
}
```

Register in `GearPlugin::build`, mirroring `dress_rigs`:

```rust
            .add_systems(
                Update,
                dress_bats
                    .after(crate::game::player::IdentitySet)
                    .run_if(crate::game::dressing_active),
            )
```

- [ ] **Step 3: Player-model surgery**

`tools/build_player.py`: delete the `_bat_mesh_part` function and its `PARTS.append(...)` line; delete `"Bat": (0.72, 0.50, 0.28, 1.0),` from `MATERIALS`. Do NOT touch the `BONES["Bat"]` row or any clip's `"Bat"` channels — the animated socket is the contract.

`src/game/model_assets.rs`: remove `BAT_MATERIAL`, `RigAnimations.bat_material` + its `named_materials` lookup + struct-literal field, `bat_meshes` + its `mats_q` arm + the `bat_visibility` loop, and `Has<Batter>` (with its `is_batter` binding) from `wire_rigs`; update the `wire_rigs` doc comment (bat dressing now lives in `gear::dress_bats`). Keep the `"Bat" => bat = Some(e)` name arm and `RigBones.bat`.

`tests/model_contract.rs`: drop `BAT_MATERIAL` from the import and from the material loop array.

- [ ] **Step 4: Regenerate the player model**

Run:
```bash
blender --background --python tools/build_player.py
blender --background assets-src/player.blend --python tools/export_glb.py
```
Expected: both `wrote ...` lines; `git status` shows `player.blend`/`player.glb` modified.

- [ ] **Step 5: Run the full suite**

Run: `cargo test 2>&1 | tail -20`
Expected: everything PASSES — including `model_contract` (bat material gone, `Bat` bone still in joints), the rewritten `e2e_gltf_rig`, and the untouched e2e/balance suites.

- [ ] **Step 6: Both-target check**

Run: `cargo check --features "dev debug" && cargo check --target wasm32-unknown-unknown`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add tools/build_player.py assets-src/player.blend src/game/models/player.glb \
  src/game/model_assets.rs src/game/gear.rs tests/model_contract.rs tests/e2e_gltf_rig.rs
git commit -m "feat: bat unbaked from player.glb; dress_bats hangs bat scenes off the Bat bone"
```

---

### Task 7: Creator hub — bat cycling, hover framing, randomize channel

**Files:**
- Modify: `src/game/creator.rs` (`CreatorState` ~line 61; `camera_target:474`; `lerp_creator_camera:490`; `render_gear_tab:816`; `render_creator_panel:721-726`; randomize `:975-1001`)

**Interfaces:**
- Consumes: `BatId::{VARIANTS, NAMES}` (Task 2); the preview rig already carries `player::Batter` so `dress_bats` (Task 6) dresses it live.
- Produces: `CreatorState.bat_row_hovered: bool`; `camera_target(tab: CreatorTab, bat_row_hovered: bool)`; `render_gear_tab(ui, def) -> (bool, bool)` (changed, bat_row_hovered); `pick_bat(roll: f32) -> BatId`.

- [ ] **Step 1: Write the failing randomize test**

In creator.rs's test module (add one if absent — grep `mod tests` in creator.rs first; follow the file's existing test style):

```rust
    #[test]
    fn pick_bat_boundaries() {
        // Classic 60% / Lumber 20% / Quick 20%.
        assert_eq!(pick_bat(0.0), BatId::Classic);
        assert_eq!(pick_bat(0.59), BatId::Classic);
        assert_eq!(pick_bat(0.60), BatId::Lumber);
        assert_eq!(pick_bat(0.79), BatId::Lumber);
        assert_eq!(pick_bat(0.80), BatId::Quick);
        assert_eq!(pick_bat(0.99), BatId::Quick);
    }
```

Run: `cargo test --lib creator 2>&1 | tail -5` — expected: compile failure, `pick_bat` undefined.

- [ ] **Step 2: Implement randomize + state + camera + tab**

`pick_bat` beside the other curated pickers:

```rust
/// Classic 60% / Lumber 20% / Quick 20% — bats are cosmetic for CPU rosters
/// (CPU grades NEUTRAL, spec §4), so randomize can hand them out freely.
fn pick_bat(roll: f32) -> BatId {
    if roll < 0.60 {
        BatId::Classic
    } else if roll < 0.80 {
        BatId::Lumber
    } else {
        BatId::Quick
    }
}
```

In `randomize_player`, replace the Task-2 pin with a rolled channel:

```rust
    let bat = pick_bat(roll(seed, 8));
    ...
        bat,
```

`CreatorState` gains (with doc comment):

```rust
    /// True while the pointer sits over the Gear tab's bat row — read by
    /// `camera_target` to swap in the full-body framing (the head close-up
    /// hides the bat). Written through the panel's bypassed borrow and MUST
    /// NOT feed the `changed` report: hovering is not an edit, and flagging
    /// it would make `apply_creator_edits` rebuild rosters every hovered
    /// frame (see `creator_panel`'s bypass doc).
    pub bat_row_hovered: bool,
```

(and `bat_row_hovered: false,` in its `Default`.)

`camera_target` gains the flag:

```rust
fn camera_target(tab: CreatorTab, bat_row_hovered: bool) -> (Vec3, Vec3) {
    match tab {
        CreatorTab::Identity => (Vec3::new(0.0, 1.1, 3.2), Vec3::new(0.0, 1.0, 0.0)),
        // Hovering the bat row needs the whole silhouette — reuse the
        // Identity full-body framing (spec §5).
        CreatorTab::Gear if bat_row_hovered => (Vec3::new(0.0, 1.1, 3.2), Vec3::new(0.0, 1.0, 0.0)),
        CreatorTab::Gear | CreatorTab::Colors => {
            (Vec3::new(0.35, 1.55, 1.1), Vec3::new(0.0, 1.5, 0.0))
        }
        CreatorTab::Animations => (Vec3::new(2.2, 1.4, 2.2), Vec3::new(0.0, 1.0, 0.0)),
    }
}
```

`lerp_creator_camera`: `let (target_pos, look_at) = camera_target(cs.tab, cs.bat_row_hovered);` (it reads `Res<CreatorState>` unconditionally each frame, so the bypassed write is seen).

`render_gear_tab` — new signature returning the hover flag (it can't see `CreatorState`; `render_creator_panel` writes it):

```rust
fn render_gear_tab(ui: &mut egui::Ui, def: &mut PlayerDef) -> (bool, bool) {
    let mut changed = false;
    // ... existing headwear/eyewear/arms/chain rows unchanged ...
    ui.separator();
    ui.label("Bat");
    let bat_resp = ui.horizontal_wrapped(|ui| {
        let mut changed = false;
        for (variant, name) in BatId::VARIANTS.iter().zip(BatId::NAMES.iter()) {
            changed |= ui
                .selectable_value(&mut def.appearance.bat, *variant, *name)
                .changed();
        }
        changed
    });
    changed |= bat_resp.inner;
    let bat_row_hovered = ui.rect_contains_pointer(bat_resp.response.rect);
    (changed, bat_row_hovered)
}
```

In `render_creator_panel`'s tab dispatch:

```rust
        changed |= match tab {
            CreatorTab::Identity => render_identity_tab(ui, def),
            CreatorTab::Gear => {
                let (tab_changed, hovered) = render_gear_tab(ui, def);
                cs.bat_row_hovered = hovered; // bypassed borrow: no changed flag
                tab_changed
            }
            CreatorTab::Colors => render_colors_tab(ui, def),
            CreatorTab::Animations => render_animations_tab(ui, def),
        };
```

(Borrow note: `def` is `selected_def(&mut cs.working, ...)` — a borrow of `cs.working` — so write `cs.bat_row_hovered` AFTER the `def` borrow's scope block ends, or restructure to capture `hovered` in a local and assign outside the block, exactly as the existing `{ let def = ...; }` scope allows. Also reset `cs.bat_row_hovered = false` for the non-Gear arms so leaving the tab drops the framing.)

- [ ] **Step 3: Run tests + debug-features check**

Run: `cargo test --lib creator && cargo check --features "dev debug"`
Expected: PASS / clean.

- [ ] **Step 4: Visual smoke check (native)**

Run: `cargo run --features "dev debug"` → open the Creator (per the menu), Gear tab → cycle Lumber/Quick/Classic on a player, watch the preview rig's bat swap and the camera pull back while hovering the bat row. Ctrl-C when confirmed. Report what you saw honestly.

- [ ] **Step 5: Commit**

```bash
git add src/game/creator.rs
git commit -m "feat: Creator bat cycling with bat-visible framing + randomize channel"
```

---

### Task 8: Full verification + CLAUDE.md touch-up

**Files:**
- Modify: `CLAUDE.md` (two stale sentences)

- [ ] **Step 1: Full test suite**

Run: `cargo test 2>&1 | tail -25`
Expected: ALL suites pass — unit (rules/bat_assets/appearance/creator), contracts (model/bat/appearance), e2e (full game, advanced rules, cpu, pause/subs, scenarios, gltf rig), and `balance_sim` within its bands with zero retuning. Paste the summary line into the task report.

- [ ] **Step 2: Every build configuration**

Run:
```bash
cargo check && cargo check --features dev && cargo check --features "dev debug" \
  && cargo check --target wasm32-unknown-unknown
```
Expected: all clean.

- [ ] **Step 3: Update CLAUDE.md's stale bat sentences**

In the Architecture section: the parenthetical "(the bat is a bone, so one clip covers body and bat)" near the CLIP_TABLE description stays true (the bone still animates) — but extend the model paragraph with one sentence:

```
Bats are a separate swappable library (`src/game/models/bats.glb`, built by
`tools/build_bats.py` + the shared exporter): one glTF scene per bat with
suffix-resolved grip/contact marker empties whose measured geometry collapses
into `rules::BatProfile` (`bat_assets.rs`, pinned by `tests/bat_contract.rs`);
`gear::dress_bats` hangs the roster-identified bat scene off the rig's `Bat`
bone, per-player via `PlayerAppearance.bat`, and CPU batters always grade
`BatProfile::NEUTRAL` so `balance_sim` stays the untouched arbiter.
```

- [ ] **Step 4: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: CLAUDE.md notes the swappable bat library"
```

---

## Self-Review Notes (already applied)

- Spec coverage: §1→Tasks 1/3, §2→Tasks 2/4, §3→Task 6, §4→Task 5, §5→Task 7, §6→Tasks 3–8. The dev hot-reload despawn-before-clear (spec §2) is in Task 4's `reload_bat_library`.
- Type consistency: `BatProfile` lives in `rules.rs`; `BatSpec`/`BatLibrary`/`BatDressed`/`resolve_bat_profile` in `bat_assets.rs`; `dress_bats` in `gear.rs`; `bat_profile_for`/`BatParams` in `flow.rs` — matching the spec's registration pins.
- The one intentional deviation from strict TDD sequencing: Task 3 Step 6's player-glb re-export check is verification-only (reverted), since Task 6 owns the real regeneration.
