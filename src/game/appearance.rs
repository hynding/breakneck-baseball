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
            /// Every variant value, same declaration order as [`Self::NAMES`]
            /// — built from the identical token list, so the two lists are
            /// compiler-structurally incapable of drifting apart. The
            /// Creator panel's radio grids iterate this to render a button
            /// per variant labelled from `NAMES`.
            pub const VARIANTS: &'static [Self] = &[$(Self::$variant),+];
        }
    };
}

appearance_enum! {
/// Curated skin swatch ids — resolved to actual colours by the dressing
/// systems (Phase 2), never raw RGB in the data file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
/// Batting-stance id. All four resolve to a clip (`animation::stance_clip`) —
/// `Standard` to the shared `BattingStance`, the other three to their own
/// personality clips (`StanceOpen`/`StanceClosed`/`StanceWaggle`). Kept here
/// rather than in animation.rs so the schema module stays serde-pure with no
/// animation dependency.
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
/// Pure so the dev watcher stays a thin shell around a tested core. Watcher
/// note: the `file == defs.0` short-circuit here is a *content* compare and
/// stays correct on its own; it's the watcher's *disk-text* short-circuit
/// (`dev_watch::watch_players_file`'s `last_disk_text`) that keeps this
/// function from ever being called on an unchanged poll once something else
/// (the Creator panel) has diverged `defs.0` from disk without saving.
pub fn apply_reload(text: &str, defs: &mut RosterDefs) -> Result<bool, String> {
    let file = parse_roster_file(text).map_err(|e| e.to_string())?;
    RosterDefs::validate(&file)?;
    if file == defs.0 {
        return Ok(false);
    }
    defs.0 = file;
    Ok(true)
}

/// Whether the watcher should reconsider reloading at all: `true` only when
/// `current` (freshly read off disk) differs from `last_seen` (the disk text
/// as of the previous poll — `None` on the very first poll, so that one
/// always proceeds). Pulled out as a pure, always-compiled helper — rather
/// than inlined in `dev_watch::watch_players_file`, which only compiles
/// under `dev` + native — so the watcher-clobber fix has real unit coverage
/// under the default test suite. See `dev_watch::watch_players_file`'s doc
/// comment for the bug this closes: comparing against live `defs.0` (which
/// the Creator panel diverges from disk without saving) instead of the last
/// *disk* text made an unedited-on-disk file look "new" on every poll.
pub fn disk_text_changed(last_seen: &Option<String>, current: &str) -> bool {
    last_seen.as_deref() != Some(current)
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
    ///
    /// Watcher-clobber note: this used to compare freshly-read disk text
    /// against the *live* `defs.0` (re-serialized implicitly by
    /// `apply_reload`'s `file == defs.0` check). Once the Creator panel
    /// (`creator.rs`) starts writing edited content straight into
    /// `RosterDefs` — diverging it from disk without a save — that compare
    /// treats the unchanged-on-disk file as "new" on the very next 1 s poll
    /// and silently reverts the panel's edit. Comparing against the last
    /// *disk* text instead (held in `last_disk_text`) makes an unchanged
    /// file a true no-op regardless of what's live, while a real disk edit
    /// still reloads exactly as before.
    pub fn watch_players_file(
        time: Res<Time<Real>>,
        mut timer: Local<Option<Timer>>,
        mut last_disk_text: Local<Option<String>>,
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
        if !disk_text_changed(&last_disk_text, &text) {
            return; // disk hasn't moved since our last poll — don't reconsider
        }
        last_disk_text.replace(text.clone());
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
    fn disk_text_changed_only_flags_real_disk_edits() {
        // First poll: no baseline yet, always reconsider.
        assert!(disk_text_changed(&None, "a"));
        // Same text as last poll — the watcher-clobber bug this guards
        // against: a live-only edit (Creator panel) must not look like a
        // fresh disk change just because it diverged from `defs.0`.
        assert!(!disk_text_changed(&Some("a".to_string()), "a"));
        // A genuinely different disk read still wins.
        assert!(disk_text_changed(&Some("a".to_string()), "b"));
    }

    #[test]
    fn variants_len_matches_names_for_every_appearance_enum() {
        // Both consts are generated from the same token list inside
        // `appearance_enum!`, but pin the invariant explicitly per every
        // enum the macro produces so a future hand-edit that breaks the
        // pattern (e.g. a manually-added variant to one list only) fails
        // loudly here instead of silently mis-sizing a radio grid.
        assert_eq!(SkinTone::VARIANTS.len(), SkinTone::NAMES.len());
        assert_eq!(Headwear::VARIANTS.len(), Headwear::NAMES.len());
        assert_eq!(Eyewear::VARIANTS.len(), Eyewear::NAMES.len());
        assert_eq!(Arms::VARIANTS.len(), Arms::NAMES.len());
        assert_eq!(StanceId::VARIANTS.len(), StanceId::NAMES.len());
        assert_eq!(FidgetId::VARIANTS.len(), FidgetId::NAMES.len());
        assert_eq!(TrotId::VARIANTS.len(), TrotId::NAMES.len());
        assert_eq!(CelebrationId::VARIANTS.len(), CelebrationId::NAMES.len());
    }

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
        let colors: Vec<[f32; 4]> = tones
            .iter()
            .map(|t| t.color().to_srgba().to_f32_array())
            .collect();
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
