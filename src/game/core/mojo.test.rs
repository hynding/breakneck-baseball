//! Unit tests for [`super`] — the mojo module.

use super::*;

const HIT: PaResult = PaResult {
    reached: true,
    runs: 0,
};
const OUT: PaResult = PaResult {
    reached: false,
    runs: 0,
};

#[test]
fn a_streak_heats_a_batter_and_cools_the_pitcher() {
    let mut m = Mojo::default();
    for _ in 0..3 {
        m.record(Team::Away, 4, HIT);
    }
    assert_eq!(MojoLevel::of(m.batter(Team::Away, 4)), MojoLevel::OnFire);
    assert!(m.pitcher(Team::Home) < 0.0);
    // Nobody else's mojo moved.
    assert_eq!(m.batter(Team::Away, 5), 0.0);
    assert_eq!(m.batter(Team::Home, 4), 0.0);
    assert_eq!(m.pitcher(Team::Away), 0.0);
}

#[test]
fn an_average_hitter_drifts_nowhere() {
    // An OBP-.320 hitter who drives in a run every ninth PA, over and over:
    // averaged over a cycle his score sits near even rather than ratcheting
    // either way — and so does the pitcher he faces.
    let mut m = Mojo::default();
    let (mut bat, mut pitch) = (0.0, 0.0);
    let pa = |i: u32| PaResult {
        reached: i % 25 < 8,
        runs: u32::from(i % 9 == 4),
    };
    for i in 0..2250 {
        m.record(Team::Home, 1, pa(i));
        if i >= 2025 {
            bat += m.batter(Team::Home, 1) / 225.0;
            pitch += m.pitcher(Team::Away) / 225.0;
        }
    }
    assert!(bat.abs() < 0.05, "batter mean {bat}");
    assert!(pitch.abs() < 0.05, "pitcher mean {pitch}");
}

#[test]
fn mojo_is_bounded_and_fades() {
    let mut m = Mojo::default();
    for _ in 0..50 {
        m.record(
            Team::Home,
            2,
            PaResult {
                reached: true,
                runs: 4,
            },
        );
    }
    assert_eq!(m.batter(Team::Home, 2), 1.0);
    assert!(m.pitcher(Team::Away) >= -1.0);
    // An ordinary run of outs brings even a hot hitter back down.
    for _ in 0..15 {
        m.record(Team::Home, 2, OUT);
    }
    assert!(MojoLevel::of(m.batter(Team::Home, 2)) <= MojoLevel::Even);
}

#[test]
fn a_sac_fly_still_counts_for_the_hitter() {
    let mut m = Mojo::default();
    m.record(
        Team::Away,
        3,
        PaResult {
            reached: false,
            runs: 1,
        },
    );
    assert!(m.batter(Team::Away, 3) > 0.0);
}

#[test]
fn the_edge_scales_the_timing_error_both_ways() {
    let mut m = Mojo::default();
    assert_eq!(m.timing_scale(Team::Away, 1), 1.0);
    for _ in 0..5 {
        m.record(Team::Away, 1, HIT);
    }
    let hot = m.timing_scale(Team::Away, 1);
    assert!((1.0 - MOJO_TIMING_EDGE..1.0).contains(&hot));
    // A cold hitter facing a pitcher who has been dealing misses by more.
    for _ in 0..8 {
        m.record(Team::Home, 6, OUT);
    }
    let cold = m.timing_scale(Team::Home, 6);
    assert!(cold > 1.0 && cold <= 1.0 + MOJO_TIMING_EDGE, "{cold}");
}
