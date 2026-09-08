//! Unit tests for [`super`] — the rules module.

use super::*;

#[test]
fn batting_order_rotates_nine_and_wraps() {
    let mut order = BattingOrder::default();
    assert_eq!(order.current(Team::Home), 1);
    for _ in 0..8 {
        order.advance(Team::Home);
    }
    assert_eq!(order.current(Team::Home), 9);
    order.advance(Team::Home);
    assert_eq!(order.current(Team::Home), 1);
    // Teams rotate independently.
    assert_eq!(order.current(Team::Away), 1);
}
