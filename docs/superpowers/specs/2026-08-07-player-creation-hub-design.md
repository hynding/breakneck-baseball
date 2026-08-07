# Player Creation Hub — Design

Date: 2026-08-07
Status: approved (brainstorm complete; implementation plan to follow)

## Goal

Give players personality: per-player looks (skin tone, gear, accessories) and
per-player animation style (batting stances, idle fidgets, celebrations),
managed through a creation hub usable by both the human designer (in-game
editor stage) and the AI collaborator (data files + portrait renders +
contract tests). Ship the *results* — richer rosters — in every build; the
hub itself is a dev tool this round.

## Decisions made during brainstorming

- **Audience**: design tool first (dev-gated, like the F1 debug panel). A
  player-facing creator can be layered on the same data later; nothing in the
  data model may assume dev-only use.
- **Scope**: visual + animation personality now. The schema reserves room for
  gameplay attributes (speed, power, contact), but wiring stats into the
  balance economy (`tests/balance_sim.rs` guarded) is a separate later project.
- **Team vs player**: the team/theme keeps owning uniform base colors
  (jersey, cap, pants — `PlayerTemplate` untouched as the team surface).
  Player definitions own only personal channels: skin tone, headwear,
  eyewear, arm accessories, chain, animation style.
- **AI access**: repo-committed RON definitions with dev hot-reload, a
  headless portrait-render harness, and contract tests validating every
  definition. All three were explicitly requested.
- **Hub form**: a dedicated dev-gated Creator game state with a lit preview
  stage and egui panel (not a tab in the F1 panel; not a standalone binary).

## Research foundations (what shaped this design)

- Industry modular-character stack is three layers, cheapest first:
  (1) palette/material variation, (2) rigid props parented to named bones,
  (3) skinned mesh variants only where the silhouette must deform. This game
  already has working machinery for (1) (team retints, procedural jerseys)
  and (2) (jersey quads mounted on `RigBones`).
- Discrete curated options beat sliders for stylized low-poly games
  (Mii Maker / Animal Crossing precedent); sliders need morphable meshes we
  don't have and don't want.
- MLB The Show demonstrates batting stances are the highest-identity
  personality carrier in a baseball game; its structure — a shared base
  animation set plus per-player named overrides — is the `StyleSet` pattern.
- Appearance serialization: flat recipe struct, stable string/enum ids
  (never indices), version field, unknown-id fallback to defaults, presets
  are just saved recipes.
- Editor honesty: the editor mutates only the recipe; one shared
  "dress the rig" path reacts to it everywhere (game, substitutions,
  preview), so the preview can never drift from the shipped look.
- Bevy 0.15 has no built-in bone attachment; the idiomatic pattern is
  name-matched bone entities + `add_child` (props inherit animated
  transforms). Names are stringly-typed, so contract tests pin them.

## 1. Data core

### `data/players.ron`

The hardcoded name/number pools in `roster.rs` move into `data/players.ron`,
defining both team pools (13 players each): name, number, and a
`PlayerAppearance` per player.

```ron
(
  version: 1,
  home: [ (
      name: "RIVERA", number: 7,
      appearance: (
        skin: Tan,               // SwatchId — curated palette, not raw RGB
        headwear: Helmet,        // Cap | CapBackwards | Helmet | Bare
        eyewear: EyeBlack,       // Bare | Glasses | Shades | EyeBlack
        arms: WristbandsBoth,    // Bare | WristbandL | WristbandR | WristbandsBoth
        chain: true,
        style: (
          stance: OpenCrouch,    // falls back to shared BattingStance
          fidget: Some(BatTap),  // None = no fidget
          trot: Default,
          celebration: BatFlip,
        ),
      ),
  ), /* … */ ],
  away: [ /* … */ ],
)
```

Rules:

- **Stable enum ids with pinned serde names.** Never indices. Inserting an
  option must not re-mean existing files.
- **`#[serde(default)]` everywhere**; an unknown or missing value falls back
  to the default look instead of failing the file (forward compatibility).
- **Colors are curated `SwatchId`s** resolved through the active `Theme`,
  keeping team palettes authoritative and theme cycling (T) coherent.
- A reserved, currently-unread `attributes` section may appear in the schema
  as a commented placeholder only — no code reads gameplay stats this round.

### Loading

Mirrors the model-asset pattern (`model_assets::player_model_path`):

- Shipping/wasm: embedded at compile time (`include_str!`).
- `--features dev`: read from disk and re-parsed on change (poll for a
  changed mtime/hash), re-dressing live rigs — the hub's saves and direct
  AI file edits both hot-reload into a running game.
- Parse failure at runtime: log an error and keep the last good data
  (dev) / fall back to embedded (release). Never panic.

### `PlayerCard` growth

`PlayerCard` gains `appearance: PlayerAppearance`; `name` becomes `String`
(RON-loaded), which ripples into `JerseyCache`'s key (currently
`&'static str`). Roster invariants stay pinned by tests: names A–Z only,
≤ 8 chars (5×7 jersey font constraint), number < 100.

## 2. Identity plumbing (prerequisite)

Rigs currently don't know who they are: `dress_jerseys` re-derives identity
positionally from `ScoreBoard`/`BattingOrder`/`Rosters` each refresh, and
run-out/base-runner rigs (`runner.rs`) get no jerseys at all.

- New component `PlayerIdentity { team: Team, slot: usize }`, inserted by
  `spawn_rig` for every human rig: pitcher, fielders, batter, and the
  runner/run-out rigs. Umpires get none.
- `dress_jerseys` and all new appearance systems read `PlayerIdentity`
  directly instead of re-deriving roles. `JerseyRole` collapses into this.
- Substitutions rewrite `Rosters`; identity components on affected rigs are
  refreshed the same way jerseys re-letter today.
- Side effect (intended): runners finally wear their jerseys and their
  player's look.

## 3. Appearance tech

Three layers, cheapest first. Layers 1–2 are this round; layer 3 is deferred.

### Layer 1 — palette tints (skin, bat)

- `wire_rigs` currently tags only Jersey/Cap meshes (`GltfPart` has no
  `Skin`/`Bat` arm). Add both arms plus `skin_material`/`bat_material` base
  handles in `RigAnimations`.
- The fixed 6-handle `GltfTeamMaterials` cache generalizes to a small map
  keyed by swatch id — bounded by palette size, not player count (the
  `JerseyCache` precedent). Draw-call growth is bounded and acceptable at
  18 rigs.
- Blocky fallback rig gets the same treatment through `TeamPalette`.

### Layer 2 — rigid gear as procedural Bevy meshes on bones

The jersey-quad recipe (`jersey.rs::mount_jerseys_on_bones`), generalized:

- Gear items — batting helmet, backwards cap, glasses/shades, eye-black
  quads, wristbands, chain — are built from Bevy primitives in Rust
  (`gear.rs`), spawned as children in `spawn_rig`, and re-parented onto bone
  entities on `Added<RigBones>`. **No Blender round-trip to add a gear
  item** — this is what makes hub iteration fast for the AI.
- `RigBones` gains `head`, `hips`, `lower_arm_l`, `lower_arm_r` (all bones
  already exported and guarded by `ATTACH_BONES`/`model_contract.rs`;
  `wire_rigs`'s name match extends accordingly).
- The cap baked into `player.glb` (own `Cap` material) is hidden per-rig via
  the existing material-handle visibility mechanism (the bat precedent) when
  the player's headwear replaces it.
- Props are parented, never transform-copied per frame (respects the
  "never step rig transforms directly" rule and avoids one-frame lag).
- Batting helmets remain gear the *player* chose (headwear id), not
  state-switched this round; a batter-context helmet swap is future work.

### Layer 3 — skinned wardrobe variants (deferred, schema-reserved)

Sleeve/sock/hair silhouette changes require Blender wardrobe meshes weighted
to the shared armature, exported in the one `player.glb`, despawned per
recipe. The schema reserves the ids; no mesh work this round. If/when built:
one file, one skeleton, variants under naming convention (`Hair_*`, etc.),
contract-tested; never cross-file skin binding.

## 4. Animation personality

### `StyleSet`

Per-player clip overrides resolving against the shared base set:

```rust
struct StyleSet { stance: StanceId, fidget: Option<FidgetId>, trot: TrotId, celebration: CelebId }
```

Each id maps to an `AnimClip`; `Default` variants map to today's shared
clips. Resolution is one indirection at `Playing`-insert time: systems that
hardcode a clip (`batter_stance`, home-run trot, celebration sites) look up
the rig's `PlayerIdentity` → card → style first. The CPU/human distinction
is irrelevant — style is cosmetic.

### New clips this round

3 batting stances (open crouch, upright closed, bat waggle), 2 idle fidgets
(helmet tap, practice half-swing), 1 celebration (bat flip trot variant).
Each follows the proven end-to-end recipe: action in `CLIPS`
(`build_player.py`) → rebuild pair (`build_player.py` then `export_glb.py`,
always in order) → `AnimClip` variant (compiler forces `duration()`,
`looping()`, `limb_pose()` arms) → `CLIP_TABLE` row → contract test.

Constraints:

- A stance loop must settle into `BatterSwing`'s entry pose (crossfade can't
  hide a pose pop); `tools/render_pose_sheet.py` is the QA tool.
- Fidgets fire on deterministic hash-noise timers (the `ai.rs`/`audio.rs`
  convention) during dead-ball phases only — never during the steal window,
  windup, or pitch flight, where timing is gameplay-legible — and respect
  `Time<Virtual>`/`juice::BaseSpeed` composition.
- Fidgets get a harness kill-switch resource (the `JuiceDisabled` pattern)
  so scripted e2e timing is untouched.
- glb budgets (`MAX_GLB_BYTES`, clip count) extended consciously in
  `model_contract.rs`; current headroom is comfortable (134 KB of 512 KB,
  12 of 48 bones).

## 5. The Creator hub (dev-gated game state)

- `GameState::Creator` compiled behind the `debug` feature, entered with
  **C** on the main menu (debug builds), exits back to `MainMenu`.
- Own stage: ground plane, three-point lighting, one preview rig spawned by
  the same `spawn_rig` factory the game uses. No `game_start()` machinery
  involved (its transition schedule is `MainMenu → Playing` only).
- egui side panel (a `creator.rs` sibling of `debug.rs`, reusing its editing
  patterns; the F1 panel stays gameplay-scoped):
  - Player selector: team + roster slot, bench included.
  - Category tabs: **Identity / Gear / Colors / Animations**, each a
    discrete option grid (no sliders). Identity edits name (A–Z, ≤ 8) and
    number (< 100) — they live in the same RON record; Gear covers
    headwear/eyewear/arms/chain; Colors covers skin (and future personal
    channels); Animations covers stance/fidget/celebration. (A Body tab
    for height/bulk arrives with the deferred body-variation work.)
  - **Randomize** (curated combination tables — coherent output only),
    **Revert** (appearance snapshot taken on entry; Esc restores),
    **Save** (writes `data/players.ron`, native only),
    **Portraits** (runs the render harness).
- Camera lerps to frame the active category: head close-up (Gear, Colors),
  full body (Identity), batter's-box framing (Animations). The preview clip
  matches the category — stance loop while browsing stances, celebration
  while picking one, idle otherwise. The rig never freezes.
- Honesty property: the panel mutates only appearance data; the same
  dressing systems that run in-game react. The preview cannot drift from
  the shipped look.

## 6. AI access

- **Files first**: the AI edits `data/players.ron` directly; under
  `--features "dev debug"` the running game hot-reloads and repaints rigs.
  Hub UI and AI edits round-trip through the identical file.
- **Portrait harness**: `cargo run --features "dev debug" -- --portraits
  <dir>` boots windowless, dresses each roster player, renders front and
  three-quarter portraits plus one stance frame per player to PNGs
  (render-to-texture + readback), and exits. The AI's eyes: edit RON → run
  portraits → look → iterate, no human screenshotting.
- **Contract tests**: `tests/appearance_contract.rs` validates the shipped
  RON parses; every referenced swatch/gear/style id exists; name/number
  invariants hold; every `StyleSet` clip resolves to a `CLIP_TABLE` row.
  AI file edits fail fast in CI.

## 7. Testing

- Existing suites pass untouched (fidget kill-switch guards e2e timing; the
  balance economy is untouched because style is cosmetic and the CPU still
  bats Classic).
- New:
  - `tests/appearance_contract.rs` (above).
  - Dressing e2e: spawn rigs from a known definition; assert props are
    parented to the right bone entities, skin material applied, baked cap
    hidden when headwear replaces it, runner rigs dressed.
  - StyleSet resolution unit tests (override vs `Default` fallback).
  - RON round-trip + unknown-id-fallback unit tests (serde behavior).
  - `model_contract.rs` extended: new clip names, bone list, budgets.
- Both targets checked after physics/render-adjacent changes:
  `cargo check` and `cargo check --target wasm32-unknown-unknown`.

## 8. Phasing (one spec, four implementation phases / PRs)

1. **Data core & identity** — RON schema + loading + embedding, `PlayerCard`
   growth (`String` names), `PlayerIdentity` on all rigs (runner jerseys
   fixed), appearance contract test. No visible gear yet.
2. **Dressing** — `GltfPart::Skin`/`Bat`, swatch material cache, `gear.rs`
   procedural props, `RigBones` growth, cap-hiding, the shared
   dress-the-rig system, dressing e2e.
3. **Animation personality** — 6 new Blender actions, `AnimClip`/`CLIP_TABLE`
   growth, `StyleSet` resolution at every clip site, fidget scheduler +
   kill-switch, pose-sheet QA.
4. **The hub** — `GameState::Creator`, stage, egui panel, camera framing,
   randomize/revert/save, portrait harness.

Each phase lands green (`cargo test`, both-target checks, clippy) before the
next starts.

## Out of scope (explicit)

- Gameplay attributes wired into rules/balance (schema-reserved only).
- Skinned wardrobe meshes (hair/sleeves/socks silhouettes).
- Player-facing in-game creator UI (bevy_ui build of the same hub).
- MLB-style numeric pose-offset sliders (needs an additive pose layer).
- Batter-context automatic helmet swap.
- Left-handed batters (separate roadmap item; composes with `StyleSet`
  later).

## Known pitfalls (carried from research)

- Index-based serialization is the classic save-breaker — stable ids only.
- Bone/clip names are stringly-typed and `AnimationTargetId` is name-derived:
  a Blender rename is a silent no-op without contract tests. Pin every new
  name.
- Props must be parented to bones, never per-frame transform-copied.
- One action per NLA track (multi-strip tracks export wrong).
- Budget glb/clip growth in `model_contract.rs` — wasm size is the guardrail.
- Randomize without curation produces clown output; use combination tables.
