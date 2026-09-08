//! Unit tests for [`super`] — the appearance module.

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
    let err =
        apply_reload(&wrong_version, &mut defs).expect_err("a mismatched version must be rejected");
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
