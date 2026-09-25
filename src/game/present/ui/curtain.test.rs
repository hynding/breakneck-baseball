//! Tests for [`super`] — the wipe's sweep geometry.

use super::*;

/// The wipe panel starts a full width off the left edge, arrives flush at
/// full coverage, and leaves out the right edge as coverage falls again.
#[test]
fn the_wipe_sweeps_in_from_the_left_and_out_to_the_right() {
    assert_eq!(wipe_left_percent(0.0, true), -100.0);
    assert_eq!(wipe_left_percent(0.5, true), -50.0);
    assert_eq!(wipe_left_percent(1.0, true), 0.0);
    assert_eq!(wipe_left_percent(1.0, false), 0.0);
    assert_eq!(wipe_left_percent(0.25, false), 75.0);
    assert_eq!(wipe_left_percent(0.0, false), 100.0);
    // Out-of-range coverage never puts the panel somewhere strange.
    assert_eq!(wipe_left_percent(2.0, true), 0.0);
    assert_eq!(wipe_left_percent(-1.0, false), 100.0);
}
