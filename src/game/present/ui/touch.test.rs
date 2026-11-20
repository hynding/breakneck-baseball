//! Unit tests for [`super`] — the touch chrome.

use super::*;
use crate::game::touch::{pad_rect, pause_button_rect, swing_button_rect};

/// The bottom cards' footprints in UI pixels (the widths the dodge plans
/// with; heights a little over what they measure on screen).
const AT_BAT_UI: Vec2 = Vec2::new(AT_BAT_W_PX, AT_BAT_H_PX);
const CLUSTER_UI: Vec2 = Vec2::new(CLUSTER_W_PX, CLUSTER_H_PX);

/// The two cards' window-pixel rects for `insets` in a window of `size`.
fn cards(size: Vec2, insets: CornerInsets) -> (Rect, Rect) {
    let scale = super::super::fit::fit_scale(size);
    let [left, low_left, right, low_right] = insets.map(|v| v * scale);
    let at_bat = Rect::from_corners(
        Vec2::new(left, size.y - low_left - AT_BAT_UI.y * scale),
        Vec2::new(left + AT_BAT_UI.x * scale, size.y - low_left),
    );
    let cluster = Rect::from_corners(
        Vec2::new(
            size.x - right - CLUSTER_UI.x * scale,
            size.y - low_right - CLUSTER_UI.y * scale,
        ),
        Vec2::new(size.x - right, size.y - low_right),
    );
    (at_bat, cluster)
}

fn overlaps(a: Rect, b: Rect) -> bool {
    !a.intersect(b).is_empty()
}

/// Phones in both orientations (iPhone 13, a small Android, an iPhone SE)
/// and tablets: with the Zone Pad up, neither bottom card covers the pad,
/// the SWING button or the pause button, both stay on screen, and they
/// don't cover each other. (The footprints are the sizes the dodge plans
/// with; the screenshots in the PR check them against the real cards.)
#[test]
fn the_bottom_cards_step_clear_of_the_zone_pad() {
    for size in [
        Vec2::new(390.0, 664.0),
        Vec2::new(750.0, 342.0),
        Vec2::new(360.0, 640.0),
        Vec2::new(640.0, 360.0),
        Vec2::new(375.0, 553.0),
        Vec2::new(667.0, 375.0),
        Vec2::new(1024.0, 768.0),
        Vec2::new(800.0, 600.0),
        Vec2::new(375.0, 548.0),
        Vec2::new(430.0, 932.0),
        Vec2::new(932.0, 430.0),
        Vec2::new(568.0, 320.0),
        Vec2::new(1366.0, 1024.0),
        Vec2::new(744.0, 1133.0),
        Vec2::new(768.0, 1024.0),
    ] {
        let scale = super::super::fit::fit_scale(size);
        let insets = dodge_insets(size, scale);
        let (at_bat, cluster) = cards(size, insets);
        let screen = Rect::from_corners(Vec2::ZERO, size);
        for (name, card) in [("AT BAT", at_bat), ("scoreboard", cluster)] {
            assert!(
                !overlaps(card, pad_rect(size)),
                "{size}: {name} covers the pad"
            );
            assert!(
                !overlaps(card, swing_button_rect(size)),
                "{size}: {name} covers SWING"
            );
            assert!(
                !overlaps(card, pause_button_rect(size)),
                "{size}: {name} covers the pause button"
            );
            assert_eq!(screen.union(card), screen, "{size}: {name} off screen");
        }
        assert!(!overlaps(at_bat, cluster), "{size}: the cards collide");
    }
}

/// The resting corners are what the HUD spawns with, so dodging back
/// lands exactly where the cards started.
#[test]
fn the_cards_rest_in_their_corners() {
    assert_eq!(RESTING_INSETS, [14.0; 4]);
}
