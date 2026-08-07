# Player Creation Hub — Phase 1: Data Core & Identity — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move player definitions into `data/players.ron` (name, number, appearance recipe), give every rig a `PlayerIdentity`, and make jerseys/runners follow it — the foundation the dressing, animation-style, and hub phases build on.

**Architecture:** A new `appearance` module owns the serde schema (stable enum ids, `#[serde(default)]`, unknown-id fallback) and the embedded/dev-reload loading of `data/players.ron`. `roster.rs` builds `Rosters` from the parsed file (`PlayerCard.name` becomes `String` and gains `appearance`). Rigs get a static `RosterRole` and a derived `PlayerIdentity` component kept fresh by a sync system; `dress_jerseys` stops re-deriving identity positionally and reads `PlayerIdentity` instead. Runner rigs get identity + jerseys for the first time.

**Tech Stack:** Bevy 0.15 ECS, serde + `ron` (new dep), existing test harness (`tests/common/mod.rs`).

**Spec:** `docs/superpowers/specs/2026-08-07-player-creation-hub-design.md` (this plan implements its Phase 1; Phases 2–4 get their own plans as this lands).

## Global Constraints

- Rust is not on PATH; prefix every cargo/rustc command with:
  `export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"`
- After roster/flow-adjacent changes run the full `cargo test` (lib + e2e), and both targets: `cargo check` and `cargo check --target wasm32-unknown-unknown`.
- Jersey font constraint: roster names A–Z only, ≤ 8 chars; numbers < 100, unique per team.
- No RNG in `rules.rs`; no gameplay behavior changes in this phase (appearance is data only until Phase 2).
- Keep `Cargo.lock` committed (CI derives wasm-bindgen version from it).
- The wasm build has no filesystem: disk reads/writes are `#[cfg]`-gated to native (`not(target_arch = "wasm32")`), and shipping builds use the embedded RON only.
- Commit after every green task with the `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>` trailer.

---

### Task 1: `appearance` schema module + `ron` dependency

**Files:**
- Modify: `Cargo.toml` (add `ron = "0.8"` to `[dependencies]`)
- Create: `src/game/appearance.rs`
- Modify: `src/game/mod.rs` (add `pub mod appearance;` next to the other module decls)

**Interfaces:**
- Produces: `PlayerAppearance { skin: SkinTone, headwear: Headwear, eyewear: Eyewear, arms: Arms, chain: bool, style: StyleSet }`, `StyleSet { stance: StanceId, fidget: Option<FidgetId>, trot: TrotId, celebration: CelebrationId }`, `RosterFile { version: u32, home: Vec<PlayerDef>, away: Vec<PlayerDef> }`, `PlayerDef { name: String, number: u32, appearance: PlayerAppearance }`, `parse_roster_file(&str) -> Result<RosterFile, ron::error::SpannedError>`. All types `Clone + Debug + PartialEq + Serialize + Deserialize`; every appearance enum `Copy + Eq + Default`.

- [ ] **Step 1: Add the dependency**

In `Cargo.toml` `[dependencies]`, next to `serde_json`:

```toml
ron = "0.8"
```

Run: `cargo check` — expect clean (dependency resolves; `Cargo.lock` updates and stays committed).

- [ ] **Step 2: Write the failing tests**

Create `src/game/appearance.rs` with the module doc, empty for now except the test module (tests first — they won't compile until Step 4 adds the types; that compile failure *is* the red state):

```rust
//! Per-player appearance recipes — the data core of the player creation hub.
//!
//! Everything here is serde data with stable ids (never indices): the RON
//! file `data/players.ron` is the source of truth both the in-game creator
//! hub and direct file edits round-trip through. Unknown ids parse as the
//! default variant (`#[serde(other)]`) so old files survive new options —
//! per docs/superpowers/specs/2026-08-07-player-creation-hub-design.md.

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appearance_round_trips_through_ron() {
        let app = PlayerAppearance {
            skin: SkinTone::Tan,
            headwear: Headwear::Helmet,
            eyewear: Eyewear::EyeBlack,
            arms: Arms::WristbandsBoth,
            chain: true,
            style: StyleSet {
                stance: StanceId::OpenCrouch,
                fidget: Some(FidgetId::BatTap),
                trot: TrotId::Standard,
                celebration: CelebrationId::BatFlip,
            },
        };
        let text = ron::to_string(&app).unwrap();
        let back: PlayerAppearance = ron::from_str(&text).unwrap();
        assert_eq!(back, app);
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        // An empty record is a fully valid appearance (forward compat).
        let app: PlayerAppearance = ron::from_str("()").unwrap();
        assert_eq!(app, PlayerAppearance::default());
        // A partial record keeps its one field and defaults the rest.
        let app: PlayerAppearance = ron::from_str("(headwear: Helmet)").unwrap();
        assert_eq!(app.headwear, Headwear::Helmet);
        assert_eq!(app.skin, SkinTone::default());
    }

    #[test]
    fn unknown_enum_ids_parse_as_the_default_variant() {
        // A future file may name gear this build doesn't know. serde(other)
        // maps it onto the default variant instead of failing the file.
        let app: PlayerAppearance =
            ron::from_str("(headwear: PropellerBeanie, skin: Chartreuse)").unwrap();
        assert_eq!(app.headwear, Headwear::default());
        assert_eq!(app.skin, SkinTone::default());
    }

    #[test]
    fn roster_file_parses_with_per_player_appearance() {
        let text = r#"(
            version: 1,
            home: [
                (name: "VEGA", number: 7, appearance: (headwear: Helmet)),
                (name: "OKAFOR", number: 23),
            ],
            away: [ (name: "STONE", number: 21) ],
        )"#;
        let file = parse_roster_file(text).unwrap();
        assert_eq!(file.version, 1);
        assert_eq!(file.home.len(), 2);
        assert_eq!(file.home[0].appearance.headwear, Headwear::Helmet);
        // Appearance omitted entirely → default recipe.
        assert_eq!(file.home[1].appearance, PlayerAppearance::default());
        assert_eq!(file.away[0].name, "STONE");
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --lib appearance`
Expected: compile errors — `PlayerAppearance` etc. not found. That is the red state.

- [ ] **Step 4: Write the schema**

Above the test module in `src/game/appearance.rs`:

```rust
use serde::{Deserialize, Serialize};

/// Schema version stamped in `data/players.ron`.
pub const APPEARANCE_VERSION: u32 = 1;

/// Curated skin swatch ids — resolved to actual colours by the dressing
/// systems (Phase 2), never raw RGB in the data file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkinTone {
    Porcelain,
    Light,
    #[default]
    #[serde(other)]
    Medium,
    Tan,
    Brown,
    Deep,
}

/// What sits on the head. `Cap` is today's baked-in model cap.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Headwear {
    #[default]
    #[serde(other)]
    Cap,
    CapBackwards,
    Helmet,
    Bare,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Eyewear {
    #[default]
    #[serde(other)]
    Bare,
    Glasses,
    Shades,
    EyeBlack,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Arms {
    #[default]
    #[serde(other)]
    Bare,
    WristbandL,
    WristbandR,
    WristbandsBoth,
}

/// Batting-stance id. Only `Standard` resolves to a clip until Phase 3
/// lands the new Blender actions; the ids exist now so `data/players.ron`
/// can be fully authored once.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum StanceId {
    #[default]
    #[serde(other)]
    Standard,
    OpenCrouch,
    UprightClosed,
    BatWaggle,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FidgetId {
    #[default]
    #[serde(other)]
    BatTap,
    HalfSwing,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrotId {
    #[default]
    #[serde(other)]
    Standard,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CelebrationId {
    #[default]
    #[serde(other)]
    Standard,
    BatFlip,
}

/// Per-player animation-personality overrides over the shared base clips.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StyleSet {
    pub stance: StanceId,
    pub fidget: Option<FidgetId>,
    pub trot: TrotId,
    pub celebration: CelebrationId,
}

/// One player's personal look. Team themes keep owning uniform colours;
/// these are the personal channels only.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlayerAppearance {
    pub skin: SkinTone,
    pub headwear: Headwear,
    pub eyewear: Eyewear,
    pub arms: Arms,
    pub chain: bool,
    pub style: StyleSet,
}

/// One player as authored in `data/players.ron`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlayerDef {
    pub name: String,
    pub number: u32,
    #[serde(default)]
    pub appearance: PlayerAppearance,
}

/// The whole authored file: both team pools, nine starters then bench each.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RosterFile {
    pub version: u32,
    pub home: Vec<PlayerDef>,
    pub away: Vec<PlayerDef>,
}

pub fn parse_roster_file(text: &str) -> Result<RosterFile, ron::error::SpannedError> {
    ron::from_str(text)
}
```

And in `src/game/mod.rs`, alongside the existing module declarations:

```rust
pub mod appearance;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib appearance`
Expected: 4 tests PASS. If `unknown_enum_ids_parse_as_the_default_variant` fails because RON refuses the unknown identifier before serde's `other` sees it, that invalidates the fallback design — stop and check `ron`'s enum handling (the fix is a custom `Deserialize` via `deserialize_str` + `match`; apply it to every appearance enum through one macro if needed). Do not delete the test.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/game/appearance.rs src/game/mod.rs
git commit -m "feat: appearance schema module (RON recipes, stable ids, unknown-id fallback)"
```

---

### Task 2: `data/players.ron` + embedded loading + contract test

**Files:**
- Create: `data/players.ron`
- Modify: `src/game/appearance.rs` (embedded loader + `RosterDefs` resource)
- Create: `tests/appearance_contract.rs`

**Interfaces:**
- Consumes: Task 1's `RosterFile`/`parse_roster_file`.
- Produces: `appearance::EMBEDDED_PLAYERS_RON: &str`, `appearance::embedded_roster_file() -> RosterFile`, `#[derive(Resource)] pub struct RosterDefs(pub RosterFile)` with `impl Default` (parses embedded), `impl RosterDefs { pub fn validate(file: &RosterFile) -> Result<(), String> }`.

- [ ] **Step 1: Author the data file**

Create `data/players.ron` — the 26 existing players (names/numbers copied verbatim from `roster.rs` `HOME_POOL`/`AWAY_POOL`, same order: nine starters then bench), each given a hand-varied appearance so the roster reads as 26 individuals. Author it fully — vary skin across the six tones, sprinkle headwear/eyewear/arms/chains and stances; leave a few players entirely default (a plain player is a valid look). Example shape (first entries shown; write all 26):

```ron
(
    version: 1,
    home: [
        (name: "VEGA", number: 7, appearance: (
            skin: Tan, headwear: Helmet, arms: WristbandsBoth,
            style: (stance: OpenCrouch, fidget: Some(BatTap)),
        )),
        (name: "OKAFOR", number: 23, appearance: (
            skin: Deep, eyewear: EyeBlack, chain: true,
            style: (stance: BatWaggle, celebration: BatFlip),
        )),
        (name: "BLAZE", number: 44, appearance: (
            skin: Light, headwear: CapBackwards,
            style: (fidget: Some(HalfSwing)),
        )),
        (name: "TANAKA", number: 5, appearance: (skin: Medium, eyewear: Glasses)),
        (name: "CRUZ", number: 12, appearance: (skin: Brown, arms: WristbandL,
            style: (stance: UprightClosed))),
        (name: "HOLT", number: 28),
        (name: "DIAZ", number: 3, appearance: (skin: Tan, chain: true)),
        (name: "MERCER", number: 19, appearance: (skin: Porcelain, eyewear: Shades)),
        (name: "KANE", number: 31, appearance: (skin: Medium,
            style: (stance: OpenCrouch, celebration: BatFlip))),
        (name: "RIOS", number: 51, appearance: (skin: Brown, headwear: Helmet)),
        (name: "PYE", number: 8),
        (name: "NOVAK", number: 60, appearance: (skin: Light, arms: WristbandR)),
        (name: "ASHFORD", number: 14, appearance: (skin: Deep,
            style: (fidget: Some(BatTap)))),
    ],
    away: [
        (name: "STONE", number: 21, appearance: (
            skin: Light, headwear: Helmet, chain: true,
            style: (stance: UprightClosed))),
        (name: "IBARRA", number: 9, appearance: (skin: Brown, eyewear: EyeBlack,
            style: (fidget: Some(BatTap)))),
        (name: "FOX", number: 33, appearance: (skin: Medium, arms: WristbandsBoth,
            style: (stance: BatWaggle, celebration: BatFlip))),
        (name: "NAKANO", number: 2, appearance: (skin: Tan, eyewear: Glasses)),
        (name: "REYES", number: 17, appearance: (skin: Deep, headwear: CapBackwards)),
        (name: "BOONE", number: 45),
        (name: "LUKIC", number: 6, appearance: (skin: Porcelain, arms: WristbandL,
            style: (stance: OpenCrouch))),
        (name: "HALE", number: 26, appearance: (skin: Medium, eyewear: Shades, chain: true)),
        (name: "OSEI", number: 38, appearance: (skin: Deep, headwear: Helmet,
            style: (fidget: Some(HalfSwing)))),
        (name: "QUINN", number: 55, appearance: (skin: Light)),
        (name: "MARSH", number: 11, appearance: (skin: Tan, arms: WristbandR,
            style: (celebration: BatFlip))),
        (name: "IKEDA", number: 4),
        (name: "COLE", number: 29, appearance: (skin: Brown,
            style: (stance: UprightClosed, fidget: Some(BatTap)))),
    ],
)
```

- [ ] **Step 2: Write the failing contract test**

Create `tests/appearance_contract.rs`:

```rust
//! Contract test over `data/players.ron` — the shipped player definitions.
//! AI/hub edits to the file fail here (fast, in CI) instead of breaking
//! rendering silently. Mirrors the invariants `roster.rs` unit tests pin.

use breakneck_baseball::game::appearance::{
    embedded_roster_file, RosterDefs, APPEARANCE_VERSION,
};
use breakneck_baseball::game::rules::LINEUP_SIZE;

#[test]
fn shipped_players_file_parses_and_validates() {
    let file = embedded_roster_file();
    assert_eq!(file.version, APPEARANCE_VERSION);
    RosterDefs::validate(&file).expect("data/players.ron violates a roster invariant");
}

#[test]
fn both_teams_field_nine_with_a_bench() {
    let file = embedded_roster_file();
    for pool in [&file.home, &file.away] {
        assert!(pool.len() > LINEUP_SIZE as usize, "need bench beyond the nine");
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test --test appearance_contract`
Expected: compile error — `embedded_roster_file`/`RosterDefs` not found.

- [ ] **Step 4: Implement the loader and validation**

Append to `src/game/appearance.rs` (above the tests):

```rust
use bevy::prelude::Resource;

/// The shipped definitions, embedded so wasm and release builds need no
/// filesystem. `data/` lives at the repo root, beside `src/`.
pub const EMBEDDED_PLAYERS_RON: &str = include_str!("../../data/players.ron");

/// Parses the embedded file. Panicking is correct here: the contract test
/// (`tests/appearance_contract.rs`) makes a bad file unshippable, so at
/// runtime this is an assertion, not error handling.
pub fn embedded_roster_file() -> RosterFile {
    parse_roster_file(EMBEDDED_PLAYERS_RON)
        .expect("embedded data/players.ron must parse — see tests/appearance_contract.rs")
}

/// The live player definitions: embedded content at startup, replaced by
/// the dev file-watcher (Task 6) when `data/players.ron` changes on disk.
#[derive(Resource, Clone, Debug)]
pub struct RosterDefs(pub RosterFile);

impl Default for RosterDefs {
    fn default() -> Self {
        Self(embedded_roster_file())
    }
}

impl RosterDefs {
    /// Roster invariants shared by the contract test and the dev reloader:
    /// jersey-font-safe names, two-digit unique numbers, benches present.
    pub fn validate(file: &RosterFile) -> Result<(), String> {
        for (label, pool) in [("home", &file.home), ("away", &file.away)] {
            if pool.len() <= crate::game::rules::LINEUP_SIZE as usize {
                return Err(format!("{label}: need more than nine players for a bench"));
            }
            let mut numbers: Vec<u32> = Vec::new();
            for def in pool {
                if def.name.is_empty()
                    || def.name.len() > 8
                    || !def.name.chars().all(|c| c.is_ascii_uppercase())
                {
                    return Err(format!(
                        "{label}: name {:?} must be A-Z only, 1-8 chars (jersey font)",
                        def.name
                    ));
                }
                if def.number >= 100 {
                    return Err(format!("{label}: #{} needs two digits max", def.number));
                }
                if numbers.contains(&def.number) {
                    return Err(format!("{label}: duplicate number {}", def.number));
                }
                numbers.push(def.number);
            }
        }
        Ok(())
    }
}
```

Check the `include_str!` path compiles: `src/game/appearance.rs` → `../../data/players.ron` resolves to the repo-root `data/` directory.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --test appearance_contract && cargo test --lib appearance`
Expected: all PASS. If validation fails, fix `data/players.ron`, not the validator.

- [ ] **Step 6: Commit**

```bash
git add data/players.ron src/game/appearance.rs tests/appearance_contract.rs
git commit -m "feat: shipped player definitions in data/players.ron with contract test"
```

---

### Task 3: Rosters build from the definitions (`PlayerCard` grows)

**Files:**
- Modify: `src/game/roster.rs`
- Modify: `src/game/jersey.rs:216` (`JerseyCache` key: `&'static str` → `String`) and `jersey.rs:374` (key construction)
- Modify: `src/game/mod.rs:229` area (init `RosterDefs`) and `mod.rs:277-280` (`reset_scoreboard` rebuilds from defs)

**Interfaces:**
- Consumes: Task 2's `RosterDefs`, `PlayerDef`, `PlayerAppearance`.
- Produces: `PlayerCard { pub name: String, pub number: u32, pub appearance: PlayerAppearance }`; `TeamRoster::from_defs(&[PlayerDef]) -> TeamRoster`; `TeamRoster::card(&self, index: usize) -> &PlayerCard` (clamped); `Rosters::from_defs(&RosterDefs) -> Rosters`. `Rosters::default()` now parses the embedded file. `HOME_POOL`/`AWAY_POOL` and `from_pool` are deleted.

- [ ] **Step 1: Update the roster unit tests to the new construction**

In `src/game/roster.rs` tests: replace every `TeamRoster::from_pool(HOME_POOL)` with `Rosters::default().home`, and add coverage for the new pieces. The full replacement test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_rosters_field_nine_with_a_bench() {
        let r = Rosters::default();
        for roster in [&r.home, &r.away] {
            assert_eq!(roster.lineup.len(), LINEUP_SIZE as usize);
            assert!(!roster.bench.is_empty());
        }
        for roster in [&r.home, &r.away] {
            let mut numbers: Vec<u32> = roster
                .lineup
                .iter()
                .chain(&roster.bench)
                .map(|c| c.number)
                .collect();
            numbers.sort_unstable();
            numbers.dedup();
            assert_eq!(numbers.len(), roster.lineup.len() + roster.bench.len());
        }
    }

    #[test]
    fn cards_carry_their_authored_appearance() {
        // data/players.ron gives VEGA a helmet; the built roster must keep it.
        let r = Rosters::default();
        let vega = r.home.lineup.iter().find(|c| c.name == "VEGA").unwrap();
        assert_eq!(
            vega.appearance.headwear,
            crate::game::appearance::Headwear::Helmet
        );
    }

    #[test]
    fn substitution_swaps_starter_and_bench() {
        let mut r = Rosters::default().home;
        let starter = r.lineup[2].clone();
        let sub = r.bench[1].clone();
        r.substitute(2, 1);
        assert_eq!(r.lineup[2], sub);
        assert_eq!(r.bench[1], starter);
        r.substitute(99, 0);
        r.substitute(0, 99);
        assert_eq!(r.lineup[2], sub);
    }

    #[test]
    fn positional_lookups_follow_the_arcade_mapping() {
        let r = Rosters::default().home;
        assert_eq!(r.batting(1), &r.lineup[0]);
        assert_eq!(r.batting(9), &r.lineup[8]);
        assert_eq!(r.fielding(None), &r.lineup[0]);
        assert_eq!(r.fielding(Some(0)), &r.lineup[1]);
        assert_eq!(r.fielding(Some(8)), &r.lineup[0]);
        // The clamped direct index the identity systems use.
        assert_eq!(r.card(0), &r.lineup[0]);
        assert_eq!(r.card(99), &r.lineup[8]);
    }

    #[test]
    fn jersey_names_fit_the_procedural_font() {
        let r = Rosters::default();
        for card in r
            .home
            .lineup
            .iter()
            .chain(&r.home.bench)
            .chain(&r.away.lineup)
            .chain(&r.away.bench)
        {
            assert!(
                card.name.chars().all(|c| c.is_ascii_uppercase()),
                "{} must be A-Z only",
                card.name
            );
            assert!(card.name.len() <= 8, "{} too long for the back", card.name);
            assert!(card.number < 100, "two digits max on the back");
        }
    }
}
```

- [ ] **Step 2: Run tests to verify the new ones fail**

Run: `cargo test --lib roster`
Expected: compile errors (`card` missing, `appearance` field missing).

- [ ] **Step 3: Implement**

In `src/game/roster.rs`:

1. Replace the `PlayerCard` struct and imports:

```rust
use bevy::prelude::Resource;

use crate::game::appearance::{PlayerAppearance, PlayerDef, RosterDefs};
use crate::game::rules::LINEUP_SIZE;
use crate::game::Team;

/// One player: jersey name (A–Z only — the procedural jersey font's
/// alphabet), number, and personal appearance recipe (authored in
/// `data/players.ron`).
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerCard {
    pub name: String,
    pub number: u32,
    pub appearance: PlayerAppearance,
}
```

(`Eq` derive drops — `PlayerAppearance` is `Eq` but keep `PartialEq` only,
matching what the tests use. If the compiler allows `Eq`, keep it.)

2. Replace `from_pool` with `from_defs`, add `card()`:

```rust
impl TeamRoster {
    fn from_defs(defs: &[PlayerDef]) -> Self {
        let mut cards: Vec<PlayerCard> = defs
            .iter()
            .map(|d| PlayerCard {
                name: d.name.clone(),
                number: d.number,
                appearance: d.appearance,
            })
            .collect();
        let bench = cards.split_off(LINEUP_SIZE as usize);
        Self {
            lineup: cards,
            bench,
        }
    }

    /// Direct lineup access by 0-based index, clamped like [`Self::batting`]
    /// — the lookup [`PlayerIdentity`](crate::game::roster::PlayerIdentity)
    /// consumers use.
    pub fn card(&self, index: usize) -> &PlayerCard {
        &self.lineup[index.min(self.lineup.len() - 1)]
    }
    // batting / fielding / substitute unchanged
}
```

3. Replace `Rosters::default` and add `from_defs`; delete `HOME_POOL`/`AWAY_POOL`:

```rust
impl Rosters {
    pub fn from_defs(defs: &RosterDefs) -> Self {
        Self {
            home: TeamRoster::from_defs(&defs.0.home),
            away: TeamRoster::from_defs(&defs.0.away),
        }
    }
    // team / team_mut unchanged
}

impl Default for Rosters {
    fn default() -> Self {
        Self::from_defs(&RosterDefs::default())
    }
}
```

4. In `src/game/mod.rs`: register the resource next to `.init_resource::<Rosters>()` (line ~229):

```rust
.init_resource::<crate::game::appearance::RosterDefs>()
```

and make `reset_scoreboard` (line 277) rebuild from the live defs instead of the embedded default:

```rust
fn reset_scoreboard(
    mut score: ResMut<ScoreBoard>,
    mut rosters: ResMut<Rosters>,
    defs: Res<crate::game::appearance::RosterDefs>,
) {
    // …existing score reset lines unchanged…
    *rosters = Rosters::from_defs(&defs);
}
```

5. In `src/game/jersey.rs`: the cache key holds an owned name now —

```rust
struct JerseyCache(HashMap<(Team, String, u32, JerseyFace), Handle<StandardMaterial>>);
```

and at the key construction site (line ~374): `let key = (team, card.name.clone(), card.number, quad.face);`. Where `card.name` feeds `fit_scale`/`draw_text`/`text_width` (`build_texture`), pass `&card.name`. Fix any remaining `&'static str` expectations the compiler reports (jersey tests build `PlayerCard` literals — give them `name: "OKAFOR".to_string()` and an `appearance: default()`).

- [ ] **Step 4: Run the full suite**

Run: `cargo test`
Expected: all lib + e2e tests PASS (e2e boots real games off `Rosters::default()`, now RON-backed — this proves the data file end-to-end). Also run: `cargo check --target wasm32-unknown-unknown` — `include_str!` embeds fine on wasm; expect clean.

- [ ] **Step 5: Commit**

```bash
git add src/game/roster.rs src/game/jersey.rs src/game/mod.rs
git commit -m "feat: rosters build from data/players.ron; PlayerCard carries appearance"
```

---

### Task 4: `PlayerIdentity` on rigs; jerseys read it

**Files:**
- Modify: `src/game/roster.rs` (the two new components)
- Modify: `src/game/player.rs` (insert `RosterRole` in `spawn_players`; new `sync_identities` system registered in `PlayerPlugin`)
- Modify: `src/game/jersey.rs` (`JerseyQuad` back-references its rig; `attach_jerseys` drops the role param; `dress_jerseys` reads `PlayerIdentity`)

**Interfaces:**
- Consumes: Task 3's `TeamRoster::card`.
- Produces: `#[derive(Component)] pub struct PlayerIdentity { pub team: Team, pub index: usize }` (0-based lineup index, `Copy + Eq`); `#[derive(Component)] pub enum RosterRole { Pitcher, Fielder(usize), Batter }` (both in `roster.rs`); `attach_jerseys(commands, rig: Entity, assets: &JerseyAssets)` (role param gone — Task 5 and future dressing systems rely on this signature); `JerseyRole` deleted.

- [ ] **Step 1: Write the failing identity unit test**

`sync_identities`'s team/index derivation is pure — extract it so it's testable without ECS. In `src/game/roster.rs` tests:

```rust
#[test]
fn roster_roles_resolve_to_identities() {
    use crate::game::rules::BattingOrder;
    use crate::game::ScoreBoard;
    let score = ScoreBoard::default(); // top 1st: Away bats, Home fields
    let order = BattingOrder::default();
    let rosters = Rosters::default();
    let id = RosterRole::Pitcher.identity(&score, &order, &rosters);
    assert_eq!(id, PlayerIdentity { team: Team::Home, index: 0 });
    let id = RosterRole::Fielder(0).identity(&score, &order, &rosters);
    assert_eq!(id, PlayerIdentity { team: Team::Home, index: 1 });
    // Fielder spots wrap on tiny parks, same as TeamRoster::fielding.
    let id = RosterRole::Fielder(8).identity(&score, &order, &rosters);
    assert_eq!(id, PlayerIdentity { team: Team::Home, index: 0 });
    let id = RosterRole::Batter.identity(&score, &order, &rosters);
    assert_eq!(id, PlayerIdentity { team: Team::Away, index: 0 });
}
```

(If `ScoreBoard`/`BattingOrder` don't implement `Default` with "top 1st, slot 1" semantics, construct them the way `tests/` and `flow.rs` do — check `ScoreBoard`'s definition in `mod.rs` and mirror it; do not invent new constructors.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib roster::tests::roster_roles_resolve_to_identities`
Expected: compile error — `RosterRole`/`PlayerIdentity` not found.

- [ ] **Step 3: Implement the components**

In `src/game/roster.rs` (add `Component` to the bevy import):

```rust
use bevy::prelude::{Component, Resource};

/// Which roster seat a rig is playing *right now* — team-relative, so the
/// same physical rig means a different player after a half-inning flip.
/// Static per rig; [`PlayerIdentity`] is the derived, refreshed answer.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum RosterRole {
    Pitcher,
    Fielder(usize),
    Batter,
}

/// Who a rig currently is: the key every appearance system looks up cards
/// with. Kept fresh by `player::sync_identities`; runner rigs get theirs
/// stamped once at spawn (a runner never changes person mid-play).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayerIdentity {
    pub team: Team,
    pub index: usize,
}

impl RosterRole {
    pub fn identity(
        &self,
        score: &crate::game::ScoreBoard,
        order: &crate::game::rules::BattingOrder,
        rosters: &Rosters,
    ) -> PlayerIdentity {
        match self {
            RosterRole::Pitcher => PlayerIdentity {
                team: score.fielding_team(),
                index: 0,
            },
            RosterRole::Fielder(i) => {
                let team = score.fielding_team();
                PlayerIdentity {
                    team,
                    index: (i + 1) % rosters.team(team).lineup.len(),
                }
            }
            RosterRole::Batter => {
                let team = score.batting_team();
                let len = rosters.team(team).lineup.len();
                PlayerIdentity {
                    team,
                    index: (order.current(team) as usize - 1).min(len - 1),
                }
            }
        }
    }
}
```

Run: `cargo test --lib roster` — expect PASS.

- [ ] **Step 4: Insert roles at spawn and sync identities**

In `src/game/player.rs`:

1. In `spawn_players`, add `RosterRole` beside each role marker (import `crate::game::roster::{PlayerIdentity, RosterRole}`):
   - after line ~310 `commands.entity(pitcher).insert(Pitcher);` → also `.insert(RosterRole::Pitcher)`
   - after `commands.entity(fielder).insert(Fielder { index });` → also `.insert(RosterRole::Fielder(index))`
   - after `commands.entity(batter).insert(Batter);` → also `.insert(RosterRole::Batter)`
   - umpires get nothing.

2. New system at the end of the systems section:

```rust
/// Keeps every seated rig's [`PlayerIdentity`] matching the live game:
/// re-stamps on scoreboard flips (the defense becomes the other team's
/// nine), batting-order advances (the batter rig becomes the next hitter),
/// and roster rewrites (substitutions, dev file reloads). Inserting is the
/// change signal — `dress_jerseys` chains after this and watches
/// `Changed<PlayerIdentity>`.
fn sync_identities(
    score: Res<ScoreBoard>,
    order: Res<BattingOrder>,
    rosters: Res<Rosters>,
    mut commands: Commands,
    rigs: Query<(Entity, &RosterRole)>,
    added: Query<(), Added<RosterRole>>,
) {
    let refresh =
        score.is_changed() || order.is_changed() || rosters.is_changed() || !added.is_empty();
    if !refresh {
        return;
    }
    for (entity, role) in &rigs {
        commands
            .entity(entity)
            .insert(role.identity(&score, &order, &rosters));
    }
}
```

(Imports: `BattingOrder` from `crate::game::rules`, `ScoreBoard` from `crate::game`.) Register it in `PlayerPlugin` chained *before* the jersey dressing runs; jerseys are a separate plugin, so export ordering via a label is overkill — instead register in `JerseyPlugin` (step 5) as `(player::sync_identities, dress_jerseys, mount_jerseys_on_bones)` — move the registration there if `sync_identities` must be `pub(crate)`. Simplest: make `sync_identities` `pub(crate)` in `player.rs` and change `JerseyPlugin::build` to:

```rust
.add_systems(
    Update,
    (crate::game::player::sync_identities, dress_jerseys)
        .chain()
        .run_if(in_state(GameState::Playing)),
)
.add_systems(
    Update,
    mount_jerseys_on_bones.run_if(in_state(GameState::Playing)),
)
```

(`.chain()` gives dress the same-frame view of freshly inserted identities — Bevy auto-inserts the needed sync point between chained systems.)

3. In `src/game/jersey.rs`:

```rust
/// One lettered quad on a rig. Carries its rig root so dressing can look up
/// who that rig currently is even after the quad re-parents onto a bone.
#[derive(Component)]
pub struct JerseyQuad {
    rig: Entity,
    face: JerseyFace,
}
```

`attach_jerseys` loses the `role` param — signature `pub fn attach_jerseys(commands: &mut Commands, rig: Entity, assets: &JerseyAssets)`, spawning `JerseyQuad { rig, face }`. Delete `JerseyRole`. Update the three call sites in `player.rs` (drop the role argument).

`dress_jerseys` re-derivation block is replaced by the identity lookup:

```rust
fn dress_jerseys(
    rosters: Res<Rosters>,
    theme: Res<Theme>,
    assets: Option<Res<JerseyAssets>>,
    mut cache: ResMut<JerseyCache>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    identities: Query<&PlayerIdentity>,
    changed: Query<(), Changed<PlayerIdentity>>,
    added: Query<(), Added<JerseyQuad>>,
    mut quads: Query<(&JerseyQuad, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    if assets.is_none() {
        return;
    }
    let refresh = rosters.is_changed() || !changed.is_empty() || !added.is_empty();
    if !refresh {
        return;
    }
    for (quad, mut material) in &mut quads {
        let Ok(id) = identities.get(quad.rig) else {
            continue; // rig not seated yet — next identity stamp repaints
        };
        let card = rosters.team(id.team).card(id.index);
        let key = (id.team, card.name.clone(), card.number, quad.face);
        let handle = cache.0.entry(key).or_insert_with(|| {
            let template = match id.team {
                Team::Home => &theme.home,
                Team::Away => &theme.away,
            };
            let image = build_texture(card, quad.face, contrast_color(template.jersey));
            materials.add(StandardMaterial {
                base_color_texture: Some(images.add(image)),
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                ..default()
            })
        });
        if material.0 != *handle {
            material.0 = handle.clone();
        }
    }
}
```

(Imports adjust: `PlayerIdentity` from roster; `ScoreBoard`/`BattingOrder` imports go away with the derivation.)

- [ ] **Step 5: Run the full suite**

Run: `cargo test`
Expected: all PASS — in particular `tests/e2e_gltf_rig.rs` (jersey mounting/lettering assertions) and `tests/e2e_pause_subs.rs` (substitution re-lettering, now flowing through `Rosters` change → identity re-stamp → dress). If a jersey e2e fails on lettering timing, check the chain registration (identity must be stamped before dress runs in the same frame — the `.chain()` in `JerseyPlugin`).

- [ ] **Step 6: Commit**

```bash
git add src/game/roster.rs src/game/player.rs src/game/jersey.rs
git commit -m "feat: rigs carry PlayerIdentity; jerseys dress off identity, not position"
```

---

### Task 5: Runner rigs get identity and jerseys

**Files:**
- Modify: `src/game/runner.rs` (both `spawn_rig` call sites: `batter_runs` line ~337 and `sync_runners` line ~279)
- Create: `tests/e2e_identity.rs`

**Interfaces:**
- Consumes: Task 4's `PlayerIdentity`, `attach_jerseys(commands, rig, assets)`.
- Produces: every runner/run-out rig spawns with a `PlayerIdentity` and four `JerseyQuad` children.

- [ ] **Step 1: Write the failing e2e**

Create `tests/e2e_identity.rs` using the shared harness (`tests/common/mod.rs`) and the `scenario.rs` situation seam — the spec-mandated jump-cut for reaching game situations without inning-scripting (`tests/e2e_scenarios.rs` is the pattern this follows):

```rust
//! Identity plumbing e2e: rigs know who they are; runners wear jerseys.

mod common;

use bevy::prelude::*;
use breakneck_baseball::game::jersey::JerseyQuad;
use breakneck_baseball::game::player::{Batter, Pitcher};
use breakneck_baseball::game::roster::PlayerIdentity;
use breakneck_baseball::game::runner::Runner;
use breakneck_baseball::game::scenario::{apply_to_world, presets, PRESET_LOADED};
use breakneck_baseball::game::Team;
use common::{headless_app, run_until, start_game};

/// JerseyQuads start as rig-root children and re-parent onto bones once the
/// async glTF wiring lands — either way they stay descendants of the root.
fn count_quads(world: &mut World, root: Entity) -> usize {
    let mut count = 0;
    let mut stack = vec![root];
    while let Some(e) = stack.pop() {
        if world.get::<JerseyQuad>(e).is_some() {
            count += 1;
        }
        if let Some(children) = world.get::<Children>(e) {
            stack.extend(children.iter().copied());
        }
    }
    count
}

#[test]
fn seated_rigs_are_identified_at_kickoff() {
    let mut app = headless_app();
    start_game(&mut app, KeyCode::Digit1);
    // Top 1st: Away bats slot 1, Home pitches.
    let world = app.world_mut();
    let batter_id = *world
        .query_filtered::<&PlayerIdentity, With<Batter>>()
        .single(world);
    assert_eq!(batter_id, PlayerIdentity { team: Team::Away, index: 0 });
    let pitcher_id = *world
        .query_filtered::<&PlayerIdentity, With<Pitcher>>()
        .single(world);
    assert_eq!(pitcher_id, PlayerIdentity { team: Team::Home, index: 0 });
}

#[test]
fn runner_rigs_are_identified_and_wear_jerseys() {
    let mut app = headless_app();
    start_game(&mut app, KeyCode::Digit1);
    let s = presets()
        .into_iter()
        .find(|s| s.name == PRESET_LOADED)
        .unwrap();
    apply_to_world(app.world_mut(), &s).expect("ball is dead at PrePitch");
    let settled = run_until(&mut app, 5_000, |app| {
        let mut q = app.world_mut().query::<&Runner>();
        q.iter(app.world()).count() == 3
    });
    assert!(settled.is_some(), "three runner rigs must appear for bases loaded");

    // Every runner knows who it is (scenario-manifested runners take the
    // batter-side fallback identity) and carries the four lettered quads.
    let world = app.world_mut();
    let runners: Vec<Entity> = world
        .query_filtered::<Entity, With<Runner>>()
        .iter(world)
        .collect();
    for rig in runners {
        let id = world
            .get::<PlayerIdentity>(rig)
            .expect("runner rig must carry PlayerIdentity");
        assert_eq!(id.team, Team::Away, "runners belong to the batting team");
        assert_eq!(count_quads(world, rig), 4, "runner must wear its jerseys");
    }
}
```

(`Runner`, `PlayerIdentity`, and `JerseyQuad` are all already `pub` — `e2e_scenarios.rs` imports `Runner` today; Task 4 made `JerseyQuad` a pub struct with private fields, which `world.get::<JerseyQuad>` only needs the type for.)

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --test e2e_identity`
Expected: first test may already pass (Task 4 covers seated rigs); the runner test FAILS — no `PlayerIdentity` on runner rigs, no quads.

- [ ] **Step 3: Implement**

In `src/game/runner.rs`:

1. `batter_runs` (line ~307): add system params
   `assets: Option<Res<crate::game::jersey::JerseyAssets>>,` and
   `batter_identity: Query<&crate::game::roster::PlayerIdentity, With<Batter>>,`.
   After the `spawn_rig` call (line ~337), stamp and dress — the batter rig's
   current identity *is* the hitter (contact happens before the order
   advances):

```rust
if let Ok(id) = batter_identity.get_single() {
    commands.entity(entity).insert(*id);
}
if let Some(assets) = &assets {
    crate::game::jersey::attach_jerseys(&mut commands, entity, assets);
}
```

2. `sync_runners` (line ~279 spawn): the fresh runner inherits the run-out
   ghost's identity when one is being converted (the ghost was stamped at
   contact — correct even if the order has advanced since); else fall back
   to the batter rig (walks/HBP spawn with no ghost, before the order
   advances). Add the same two system params plus `&PlayerIdentity` to the
   ghost query's fetch, and:

```rust
let inherited = ghosts
    .iter()
    .next()
    .map(|(_, _, id)| *id)
    .or_else(|| batter_identity.get_single().ok().copied());
let entity = spawn_rig(&mut commands, &rig_model, RigUnit::Batter, mats, start, 1.0);
if let Some(id) = inherited {
    commands.entity(entity).insert(id);
}
if let Some(assets) = &assets {
    crate::game::jersey::attach_jerseys(&mut commands, entity, assets);
}
```

(Adjust the existing ghost tuple destructuring — it currently iterates `(ghost, tf)`; it becomes `(ghost, tf, id)`. The despawn/position logic is untouched.)

- [ ] **Step 4: Run the suite**

Run: `cargo test`
Expected: `e2e_identity` PASSES; the full suite stays green (runner quads are extra children — `DespawnAtPathEnd`'s `despawn_recursive` removes them with the rig; verify no e2e counts entities in a way the four extra quads break — if one does, that count was implicitly fragile; update it with a comment).

- [ ] **Step 5: Commit**

```bash
git add src/game/runner.rs tests/e2e_identity.rs
git commit -m "feat: runner rigs carry identity and wear jerseys"
```

---

### Task 6: Dev hot-reload of `data/players.ron`

**Files:**
- Modify: `src/game/appearance.rs` (watcher system + pure reload fn + `AppearancePlugin`)
- Modify: `src/game/mod.rs` (register `AppearancePlugin` with the other sub-plugins; move the `RosterDefs` init into it)

**Interfaces:**
- Consumes: Tasks 2–3 (`RosterDefs`, `RosterDefs::validate`, `Rosters::from_defs`).
- Produces: `appearance::AppearancePlugin` (owns `RosterDefs` init + the dev watcher); `apply_reload(text: &str, defs: &mut RosterDefs) -> Result<bool, String>` (pure, unit-tested: `Ok(true)` = new content applied).

- [ ] **Step 1: Write the failing unit test for the pure reload**

In `src/game/appearance.rs` tests:

```rust
#[test]
fn apply_reload_swaps_defs_only_on_valid_new_content() {
    let mut defs = RosterDefs::default();
    // Same content: no-op.
    assert_eq!(apply_reload(EMBEDDED_PLAYERS_RON, &mut defs), Ok(false));
    // Valid new content: applied.
    let edited = EMBEDDED_PLAYERS_RON.replacen("VEGA", "VEGO", 1);
    assert_eq!(apply_reload(&edited, &mut defs), Ok(true));
    assert!(defs.0.home.iter().any(|d| d.name == "VEGO"));
    // Broken content: rejected, last good defs kept.
    assert!(apply_reload("(version: 1", &mut defs).is_err());
    assert!(defs.0.home.iter().any(|d| d.name == "VEGO"));
    // Parseable but invariant-violating content: rejected too.
    let bad = EMBEDDED_PLAYERS_RON.replacen("VEGA", "vega!", 1);
    assert!(apply_reload(&bad, &mut defs).is_err());
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --lib appearance`
Expected: compile error — `apply_reload` not found.

- [ ] **Step 3: Implement**

In `src/game/appearance.rs`:

```rust
/// Applies edited file content to the live defs: parse, validate, swap.
/// Pure so the dev watcher stays a thin shell around a tested core.
pub fn apply_reload(text: &str, defs: &mut RosterDefs) -> Result<bool, String> {
    let file = parse_roster_file(text).map_err(|e| e.to_string())?;
    RosterDefs::validate(&file)?;
    if file == defs.0 {
        return Ok(false);
    }
    defs.0 = file;
    Ok(true)
}
```

The dev-only watcher (native only — wasm has no fs) polls the repo file
once a second; on change it swaps the defs *and* rebuilds live `Rosters`
so a running game repaints (identity sync + jersey dressing react to
`rosters.is_changed()`; a mid-game reload resets substitutions — accepted
for a dev tool, noted in the system doc):

```rust
#[cfg(all(feature = "dev", not(target_arch = "wasm32")))]
mod dev_watch {
    use super::*;
    use bevy::prelude::*;

    /// Repo-relative source path — dev builds run from the workspace.
    const PLAYERS_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data/players.ron");

    /// Polls `data/players.ron` and hot-swaps definitions into the running
    /// game — the AI/editor round-trip seam. NOTE: rebuilding [`Rosters`]
    /// mid-game discards substitutions made this game (dev-only trade-off).
    pub fn watch_players_file(
        time: Res<Time<Real>>,
        mut timer: Local<Option<Timer>>,
        mut defs: ResMut<RosterDefs>,
        mut rosters: ResMut<crate::game::roster::Rosters>,
    ) {
        let timer =
            timer.get_or_insert_with(|| Timer::from_seconds(1.0, TimerMode::Repeating));
        if !timer.tick(time.delta()).just_finished() {
            return;
        }
        let Ok(text) = std::fs::read_to_string(PLAYERS_PATH) else {
            return; // transient editor save states are fine to skip
        };
        match apply_reload(&text, &mut defs) {
            Ok(true) => {
                *rosters = crate::game::roster::Rosters::from_defs(&defs);
                info!("players.ron reloaded");
            }
            Ok(false) => {}
            Err(e) => warn!("players.ron rejected: {e}"),
        }
    }
}

pub struct AppearancePlugin;

impl bevy::prelude::Plugin for AppearancePlugin {
    fn build(&self, app: &mut bevy::prelude::App) {
        app.init_resource::<RosterDefs>();
        #[cfg(all(feature = "dev", not(target_arch = "wasm32")))]
        app.add_systems(bevy::prelude::Update, dev_watch::watch_players_file);
    }
}
```

In `src/game/mod.rs`: add `appearance::AppearancePlugin` to the plugin registrations (before `PlayerPlugin` — order with the other sub-plugins) and remove the bare `.init_resource::<RosterDefs>()` added in Task 3 (the plugin owns it now).

- [ ] **Step 4: Run the suite + both targets + a manual smoke**

Run: `cargo test` — all PASS.
Run: `cargo check --target wasm32-unknown-unknown` — clean (watcher cfg'd out).
Run: `cargo check --features dev` — clean (watcher compiled).
Manual smoke (do it — this is the phase's headline capability): `cargo run --features dev`, start a game, edit a player's name in `data/players.ron`, save, and confirm the jersey re-letters within ~a second. Report what you saw.

- [ ] **Step 5: Commit**

```bash
git add src/game/appearance.rs src/game/mod.rs
git commit -m "feat: dev hot-reload of data/players.ron into the running game"
```

---

## Phase-exit checklist

- [ ] `cargo test` fully green; `cargo clippy` clean; `cargo fmt --check` clean.
- [ ] `cargo check` and `cargo check --target wasm32-unknown-unknown` both clean.
- [ ] Manual smoke: hot reload demonstrated in a running dev game.
- [ ] TODO.md checked for new queued items before wrapping (user edits it mid-session).
- [ ] Phases 2–4 remain specced in the design doc; Phase 2's plan gets written against this landed code.
