//! Curated Randomize button: coherent combinations, not uniform RGB clown
//! output.

use crate::game::ai::hash01;
use crate::game::appearance::{
    Arms, CelebrationId, Eyewear, FidgetId, Headwear, PlayerAppearance, PlayerDef, SkinTone,
    StanceId, StyleSet, TrotId,
};

/// Deterministic 0..1 roll for one randomize "channel" (skin, headwear, ...)
/// off a shared `seed` — large, distinct per-channel offsets keep channels
/// decorrelated since [`hash01`] is a `sin`-based hash where nearby inputs
/// produce nearby outputs.
fn roll(seed: u32, channel: u32) -> f32 {
    hash01(seed as f32 * 7.0 + channel as f32 * 101.0)
}

/// Uniform pick across every variant of a slice (used where the brief calls
/// a field "uniform" — skin, arms, stance).
fn pick_uniform<T: Copy>(roll: f32, variants: &[T]) -> T {
    let n = variants.len();
    let idx = ((roll * n as f32) as usize).min(n - 1);
    variants[idx]
}

/// Cap 40% / Helmet 25% / CapBackwards 20% / Bare 15%.
fn pick_headwear(roll: f32) -> Headwear {
    if roll < 0.40 {
        Headwear::Cap
    } else if roll < 0.65 {
        Headwear::Helmet
    } else if roll < 0.85 {
        Headwear::CapBackwards
    } else {
        Headwear::Bare
    }
}

/// Bare 60%, the other three variants split evenly over the remaining 40%.
fn pick_eyewear(roll: f32) -> Eyewear {
    const REST: f32 = (1.0 - 0.60) / 3.0;
    if roll < 0.60 {
        Eyewear::Bare
    } else if roll < 0.60 + REST {
        Eyewear::Glasses
    } else if roll < 0.60 + 2.0 * REST {
        Eyewear::Shades
    } else {
        Eyewear::EyeBlack
    }
}

/// None 40%, the two real fidgets split evenly over the remaining 60%.
fn pick_fidget(roll: f32) -> Option<FidgetId> {
    if roll < 0.40 {
        None
    } else if roll < 0.70 {
        Some(FidgetId::HalfSwing)
    } else {
        Some(FidgetId::BatTap)
    }
}

/// Standard 70% / BatFlip 30%.
fn pick_celebration(roll: f32) -> CelebrationId {
    if roll < 0.70 {
        CelebrationId::Standard
    } else {
        CelebrationId::BatFlip
    }
}

/// Curated randomize: coherent combinations, not uniform RGB clown output.
/// Deterministic in `seed` ([`hash01`] mixes) so the same seed always
/// reproduces the same look — the panel's bumping `randomize_seed` counter
/// gets a fresh look per click while staying pinnable in tests. Builds a
/// whole fresh [`PlayerAppearance`] literal (every field named explicitly,
/// no `..PlayerAppearance::default()` spread) so a field can never be
/// silently left un-rolled; `name`/`number` are untouched — randomize only
/// covers appearance, per the brief's curation table.
pub fn randomize_player(def: &mut PlayerDef, seed: u32) {
    let skin = pick_uniform(roll(seed, 0), SkinTone::VARIANTS);
    let headwear = pick_headwear(roll(seed, 1));
    let eyewear = pick_eyewear(roll(seed, 2));
    let chain = roll(seed, 3) < 0.25;
    let arms = pick_uniform(roll(seed, 4), Arms::VARIANTS);
    let stance = pick_uniform(roll(seed, 5), StanceId::VARIANTS);
    let fidget = pick_fidget(roll(seed, 6));
    let celebration = pick_celebration(roll(seed, 7));

    def.appearance = PlayerAppearance {
        skin,
        headwear,
        eyewear,
        arms,
        chain,
        style: StyleSet {
            stance,
            fidget,
            // The only `TrotId` variant today — written explicitly (not via
            // a `..default()` spread) so a future second trot is forced
            // through this same curated seam instead of silently defaulting.
            trot: TrotId::Standard,
            celebration,
        },
    };
}

#[cfg(test)]
mod tests {
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
}
