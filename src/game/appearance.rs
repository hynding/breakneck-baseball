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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Eyewear {
    Glasses,
    Shades,
    EyeBlack,
    #[default]
    #[serde(other)]
    Bare,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Arms {
    WristbandL,
    WristbandR,
    WristbandsBoth,
    #[default]
    #[serde(other)]
    Bare,
}

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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FidgetId {
    HalfSwing,
    #[default]
    #[serde(other)]
    BatTap,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrotId {
    #[default]
    #[serde(other)]
    Standard,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CelebrationId {
    BatFlip,
    #[default]
    #[serde(other)]
    Standard,
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
