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

#[test]
fn speed_grades_scale_sprint_speed_around_the_average() {
    // 50 is today's runner; each 10 grade points is one MLB sprint-speed
    // standard deviation (~6%), and grades off the 20–80 scale clamp.
    assert_eq!(Runner::AVERAGE.speed_factor(), 1.0);
    let f = |speed| Runner { speed }.speed_factor();
    assert!((f(60) - 1.06).abs() < 1e-6);
    assert!((f(40) - 0.94).abs() < 1e-6);
    assert_eq!(f(99), f(80));
    assert_eq!(f(0), f(20));
    // An empty base reads as an average runner.
    assert_eq!(Bases::default().speed_factor(0), 1.0);
}
