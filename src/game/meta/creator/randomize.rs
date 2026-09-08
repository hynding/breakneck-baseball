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
#[path = "randomize.test.rs"]
mod tests;
