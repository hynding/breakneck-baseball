//! Unit tests for [`super`] — the zone module.

use super::*;

/// The zone overlay is a 3D wireframe the size of the rulebook zone:
/// plate width, plate depth, knee-to-midpoint tall (docs/BASEBALL.md
/// "Strike zone"), drawn with hairline rails. Colors moved to the Theme
/// (per-theme ghosts) — their alpha/contrast pins live in
/// `theme::tests::zone_ghost_reads_against_every_sky`.
#[test]
#[allow(clippy::assertions_on_constants)]
fn zone_wireframe_matches_rulebook_dimensions() {
    assert!((ZONE_DRAWN_HALF_WIDTH - rules::PLATE_HALF_WIDTH_M).abs() < 1e-6);
    assert!((ZONE_DEPTH - super::super::diamond::PLATE_WIDTH).abs() < 1e-6);
    assert!(
        ZONE_BAR <= 0.005,
        "rails should stay hairline, got {ZONE_BAR}"
    );
}
