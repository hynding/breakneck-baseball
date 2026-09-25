//! Unit tests for [`super`] — the randomize module.

use super::*;

fn blank_def() -> PlayerDef {
    PlayerDef {
        name: "TEST".to_string(),
        number: 0,
        appearance: PlayerAppearance::default(),
    }
}

#[test]
fn randomize_is_deterministic_for_the_same_seed() {
    let mut a = blank_def();
    let mut b = blank_def();
    randomize_player(&mut a, 42);
    randomize_player(&mut b, 42);
    assert_eq!(a.appearance, b.appearance);
}

#[test]
fn randomize_leaves_name_and_number_untouched() {
    let mut def = blank_def();
    randomize_player(&mut def, 7);
    assert_eq!(def.name, "TEST");
    assert_eq!(def.number, 0);
}

/// Every curated field must vary across enough seeds to prove it's
/// actually driven by the roll, not silently left at a default via a
/// `..PlayerAppearance::default()` spread — "every field written" from
/// the brief. Headwear additionally must hit every one of its four
/// variants (the brief's explicit coverage requirement). None of the
/// appearance enums derive `Hash`, so coverage is tracked with plain
/// `Vec::contains` rather than a `HashSet`.
#[test]
fn randomize_covers_every_field_over_many_seeds() {
    let mut skins = Vec::new();
    let mut headwears = Vec::new();
    let mut eyewears = Vec::new();
    let mut chains = Vec::new();
    let mut arms = Vec::new();
    let mut stances = Vec::new();
    let mut fidgets = Vec::new();
    let mut celebrations = Vec::new();

    for seed in 0..100u32 {
        let mut def = blank_def();
        randomize_player(&mut def, seed);
        let a = &def.appearance;
        if !skins.contains(&a.skin) {
            skins.push(a.skin);
        }
        if !headwears.contains(&a.headwear) {
            headwears.push(a.headwear);
        }
        if !eyewears.contains(&a.eyewear) {
            eyewears.push(a.eyewear);
        }
        if !chains.contains(&a.chain) {
            chains.push(a.chain);
        }
        if !arms.contains(&a.arms) {
            arms.push(a.arms);
        }
        if !stances.contains(&a.style.stance) {
            stances.push(a.style.stance);
        }
        if !fidgets.contains(&a.style.fidget) {
            fidgets.push(a.style.fidget);
        }
        if !celebrations.contains(&a.style.celebration) {
            celebrations.push(a.style.celebration);
        }
        assert_eq!(a.style.trot, TrotId::Standard);
    }

    assert_eq!(
        headwears.len(),
        Headwear::VARIANTS.len(),
        "every headwear variant must appear over 100 seeds"
    );
    assert!(skins.len() > 1, "skin must vary across seeds");
    assert!(eyewears.len() > 1, "eyewear must vary across seeds");
    assert!(
        chains.contains(&true) && chains.contains(&false),
        "chain must land both true and false across seeds"
    );
    assert!(arms.len() > 1, "arms must vary across seeds");
    assert!(stances.len() > 1, "stance must vary across seeds");
    assert!(
        fidgets.contains(&None)
            && fidgets.contains(&Some(FidgetId::HalfSwing))
            && fidgets.contains(&Some(FidgetId::BatTap)),
        "fidget must hit None and both real variants across seeds"
    );
    assert!(
        celebrations.contains(&CelebrationId::Standard)
            && celebrations.contains(&CelebrationId::BatFlip),
        "celebration must hit both variants across seeds"
    );
}

/// Loose statistical check on the curated weights (not a strict RNG —
/// `ai::hash01` is a sin-based hash, so give it a generous band) over a
/// much bigger sample: headwear's `Cap` is the plurality pick (~40%),
/// never a minority sliver, and never a de-facto uniform 25% either.
#[test]
fn randomize_headwear_weights_favor_cap() {
    let mut cap_count = 0u32;
    let n = 2000u32;
    for seed in 0..n {
        let mut def = blank_def();
        randomize_player(&mut def, seed);
        if def.appearance.headwear == Headwear::Cap {
            cap_count += 1;
        }
    }
    let frac = cap_count as f32 / n as f32;
    assert!(
        (0.30..=0.50).contains(&frac),
        "Cap should land near its curated 40% weight, got {frac}"
    );
}
