//! Unit tests for [`super`] — the camera module.

use super::*;

#[test]
fn duel_view_cycles_through_all_four_and_wraps() {
    let v = DuelView::default();
    assert_eq!(
        v,
        DuelView::BattingZoom,
        "the batting view is the default (TODO 100)"
    );
    let v = v.next();
    assert_eq!(v, DuelView::CatcherPov);
    let v = v.next();
    assert_eq!(v, DuelView::BehindPitcher);
    let v = v.next();
    assert_eq!(v, DuelView::BroadcastPlate);
    let v = v.next();
    assert_eq!(v, DuelView::BattingZoom);
}
