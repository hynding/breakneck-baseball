//! Per-player appearance recipes — the data core of the player creation hub.
//!
//! Everything here is serde data with stable ids (never indices): the RON
//! file `data/players.ron` is the source of truth both the in-game creator
//! hub and direct file edits round-trip through. Unknown ids parse as the
//! default variant (`#[serde(other)]`) so old files survive new options —
//! per docs/superpowers/specs/2026-08-07-player-creation-hub-design.md.

use serde::{Deserialize, Serialize};

/// Schema version stamped in `data/players.ron`.
pub const APPEARANCE_VERSION: u32 = 1;

/// Defines a fieldless enum together with a `NAMES` const listing every
/// variant's RON identifier, generated from the exact same variant list the
/// enum declares — the single source of truth the strict-identifier check in
/// `tests/appearance_contract.rs` reads (an id in `data/players.ron` that
/// isn't in `NAMES` for its field is a typo, not a forward-compat unknown).
/// Because `NAMES` is built with `stringify!` over the same token list as
/// the enum body, the two cannot drift apart the way a hand-duplicated
/// string list could.
macro_rules! appearance_enum {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $(
                $(#[$vmeta:meta])*
                $variant:ident
            ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        $vis enum $name {
            $(
                $(#[$vmeta])*
                $variant
            ),+
        }

        impl $name {
            /// Every variant's RON identifier, in declaration order.
            pub const NAMES: &'static [&'static str] = &[$(stringify!($variant)),+];
        }
    };
}

appearance_enum! {
/// Curated skin swatch ids — resolved to actual colours by the dressing
/// systems (Phase 2), never raw RGB in the data file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkinTone {
    Porcelain,
    Light,
    Tan,
    Brown,
    Deep,
    #[default]
    #[serde(other)]
    Medium,
}
}

appearance_enum! {
/// What sits on the head. `Cap` is today's baked-in model cap.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Headwear {
    CapBackwards,
    Helmet,
    Bare,
    #[default]
    #[serde(other)]
    Cap,
}
}

appearance_enum! {
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Eyewear {
    Glasses,
    Shades,
    EyeBlack,
    #[default]
    #[serde(other)]
    Bare,
}
}

appearance_enum! {
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Arms {
    WristbandL,
    WristbandR,
    WristbandsBoth,
    #[default]
    #[serde(other)]
    Bare,
}
}

appearance_enum! {
/// Batting-stance id. Only `Standard` resolves to a clip until Phase 3
/// lands the new Blender actions; the ids exist now so `data/players.ron`
/// can be fully authored once.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum StanceId {
    OpenCrouch,
    UprightClosed,
    BatWaggle,
    #[default]
    #[serde(other)]
    Standard,
}
}

appearance_enum! {
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FidgetId {
    HalfSwing,
    #[default]
    #[serde(other)]
    BatTap,
}
}

appearance_enum! {
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrotId {
    #[default]
    #[serde(other)]
    Standard,
}
}

appearance_enum! {
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CelebrationId {
    BatFlip,
    #[default]
    #[serde(other)]
    Standard,
}
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
        if file.version != APPEARANCE_VERSION {
            return Err(format!(
                "unsupported players.ron version {} (expected {APPEARANCE_VERSION})",
                file.version
            ));
        }
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

/// The dev-only watcher (native only — wasm has no fs) polls the repo file
/// once a second; on change it swaps the defs *and* rebuilds live `Rosters`
/// so a running game repaints (identity sync + jersey dressing react to
/// `rosters.is_changed()`; a mid-game reload resets substitutions — accepted
/// for a dev tool, noted in the system doc).
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
        let timer = timer.get_or_insert_with(|| Timer::from_seconds(1.0, TimerMode::Repeating));
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
        // Parseable, otherwise-valid, but a wrong schema version: rejected —
        // a hot-reloaded `version: 99` must not slip in silently.
        let wrong_version = EMBEDDED_PLAYERS_RON.replacen("version: 1", "version: 99", 1);
        let err = apply_reload(&wrong_version, &mut defs)
            .expect_err("a mismatched version must be rejected");
        assert!(
            err.contains("version"),
            "rejection reason should mention the version mismatch: {err}"
        );
        assert!(defs.0.home.iter().any(|d| d.name == "VEGO"));
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
