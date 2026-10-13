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

/// CLAUDE.md invariant: no RNG anywhere in `core/rules/` — advanced rules are
/// deterministic, keyed off data the engine already computes. A source scan,
/// because nothing at runtime can tell a seeded draw from a computed value.
#[test]
fn rules_sources_draw_no_randomness() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/game/core/rules");
    // `*.test.rs` files are skipped, so this list never matches itself.
    let banned = [
        "rand::",
        "fastrand",
        "getrandom",
        "RandomState",
        "thread_rng",
    ];
    let mut scanned = 0;
    for entry in std::fs::read_dir(&dir).expect("rules dir") {
        let path = entry.expect("dir entry").path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if !name.ends_with(".rs") || name.ends_with(".test.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("readable source");
        for token in banned {
            assert!(
                !text.contains(token),
                "{name} uses `{token}` — core/rules must stay RNG-free (CLAUDE.md)"
            );
        }
        scanned += 1;
    }
    assert!(scanned >= 5, "expected the rules sources under {dir:?}");
}
