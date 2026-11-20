//! Unit tests for [`super`] — the particle allowance.

use super::*;

#[test]
fn a_burst_scales_with_the_tier() {
    assert_eq!(allowance(20, 1.0, 0, 900), 20);
    assert_eq!(allowance(20, 0.6, 0, 400), 12);
    assert_eq!(allowance(20, 0.25, 0, 150), 5);
}

#[test]
fn a_wanted_burst_gets_at_least_one_mote() {
    assert_eq!(allowance(1, 0.25, 0, 150), 1);
    assert_eq!(allowance(0, 1.0, 0, 150), 0);
}

#[test]
fn the_cap_clips_a_burst_and_then_stops_it() {
    assert_eq!(allowance(20, 1.0, 140, 150), 10);
    assert_eq!(allowance(20, 1.0, 150, 150), 0);
    assert_eq!(
        allowance(20, 1.0, 400, 150),
        0,
        "over the cap after a step down"
    );
}

/// Every tier's cap holds however many bursts land at once.
#[test]
fn the_live_count_never_passes_the_cap() {
    for tier in [QualityTier::Low, QualityTier::Medium, QualityTier::High] {
        let b = tier.budget();
        let mut live = 0;
        for _ in 0..500 {
            live += allowance(34, b.particle_scale, live, b.particle_cap);
        }
        assert_eq!(live, b.particle_cap, "{tier:?}");
    }
}
