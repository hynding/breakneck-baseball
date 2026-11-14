//! Unit tests for [`super`] — the roster module.

use super::*;

#[test]
fn roster_roles_resolve_to_identities() {
    use crate::game::ScoreBoard;
    use crate::game::rules::BattingOrder;
    // ScoreBoard::default() is *not* top-1st (top_of_inning defaults to
    // false); construct it the way `mod.rs`'s game-start insert and
    // `ScoreBoard::reset` do.
    let score = ScoreBoard {
        inning: 1,
        top_of_inning: true,
        ..Default::default()
    };
    let order = BattingOrder::default();
    let rosters = Rosters::default();
    let id = RosterRole::Pitcher.identity(&score, &order, &rosters);
    assert_eq!(
        id,
        PlayerIdentity {
            team: Team::Home,
            index: 0
        }
    );
    let id = RosterRole::Fielder(0).identity(&score, &order, &rosters);
    assert_eq!(
        id,
        PlayerIdentity {
            team: Team::Home,
            index: 1
        }
    );
    // Fielder spots wrap on tiny parks, same as TeamRoster::fielding.
    let id = RosterRole::Fielder(8).identity(&score, &order, &rosters);
    assert_eq!(
        id,
        PlayerIdentity {
            team: Team::Home,
            index: 0
        }
    );
    let id = RosterRole::Batter.identity(&score, &order, &rosters);
    assert_eq!(
        id,
        PlayerIdentity {
            team: Team::Away,
            index: 0
        }
    );
}

#[test]
fn default_rosters_field_nine_with_a_bench() {
    let r = Rosters::default();
    for roster in [&r.home, &r.away] {
        assert_eq!(roster.lineup.len(), LINEUP_SIZE as usize);
        assert!(!roster.bench.is_empty());
    }
    for roster in [&r.home, &r.away] {
        let mut numbers: Vec<u32> = roster
            .lineup
            .iter()
            .chain(&roster.bench)
            .map(|c| c.number)
            .collect();
        numbers.sort_unstable();
        numbers.dedup();
        assert_eq!(numbers.len(), roster.lineup.len() + roster.bench.len());
    }
}

#[test]
fn cards_carry_their_authored_appearance() {
    // data/players.ron gives VEGA a helmet; the built roster must keep it.
    let r = Rosters::default();
    let vega = r.home.lineup.iter().find(|c| c.name == "VEGA").unwrap();
    assert_eq!(
        vega.appearance.headwear,
        crate::game::appearance::Headwear::Helmet
    );
}

#[test]
fn substitution_swaps_starter_and_bench() {
    let mut r = Rosters::default().home;
    let starter = r.lineup[2].clone();
    let sub = r.bench[1].clone();
    r.substitute(2, 1);
    assert_eq!(r.lineup[2], sub);
    assert_eq!(r.bench[1], starter);
    r.substitute(99, 0);
    r.substitute(0, 99);
    assert_eq!(r.lineup[2], sub);
}

#[test]
fn positional_lookups_follow_the_arcade_mapping() {
    let r = Rosters::default().home;
    assert_eq!(r.batting(1), &r.lineup[0]);
    assert_eq!(r.batting(9), &r.lineup[8]);
    assert_eq!(r.fielding(None), &r.lineup[0]);
    assert_eq!(r.fielding(Some(0)), &r.lineup[1]);
    assert_eq!(r.fielding(Some(8)), &r.lineup[0]);
    // The clamped direct index the identity systems use.
    assert_eq!(r.card(0), &r.lineup[0]);
    assert_eq!(r.card(99), &r.lineup[8]);
}

#[test]
fn jersey_names_fit_the_procedural_font() {
    let r = Rosters::default();
    for card in r
        .home
        .lineup
        .iter()
        .chain(&r.home.bench)
        .chain(&r.away.lineup)
        .chain(&r.away.bench)
    {
        assert!(
            card.name.chars().all(|c| c.is_ascii_uppercase()),
            "{} must be A-Z only",
            card.name
        );
        assert!(card.name.len() <= 8, "{} too long for the back", card.name);
        assert!(card.number < 100, "two digits max on the back");
    }
}

/// Every grade of a card, by name, for the averaging checks below.
fn grades(c: &PlayerCard) -> [(&'static str, u8); 5] {
    let r = c.ratings;
    [
        ("speed", r.speed),
        ("contact", r.contact),
        ("power", r.power),
        ("arm", r.arm),
        ("fielding", r.fielding),
    ]
}

#[test]
fn the_shipped_lineups_average_out_to_league_average_grades() {
    // Ratings spread the players out without moving the economy the
    // balance sim tuned at "everyone is a 50" — each starting nine (and
    // each bench) averages exactly 50 in every grade.
    let r = Rosters::default();
    for roster in [&r.home, &r.away] {
        for g in 0..5 {
            let name = grades(&roster.lineup[0])[g].0;
            let sum =
                |group: &[PlayerCard]| -> u32 { group.iter().map(|c| grades(c)[g].1 as u32).sum() };
            for group in [&roster.lineup, &roster.bench] {
                assert_eq!(sum(group), 50 * group.len() as u32, "{name}");
            }
            assert!(
                roster.lineup.iter().any(|c| grades(c)[g].1 != 50),
                "{name} is spread out"
            );
            // The top of the order too: the balance sim plays one-inning
            // games, where the first few hitters take most of the plate
            // appearances, so a skewed top of the lineup would skew its
            // economy.
            assert_eq!(sum(&roster.lineup[..4]), 200, "top-4 {name}");
        }
    }
}

#[test]
fn grade_factors_are_one_at_fifty_and_clamp_to_the_scale() {
    let avg = Ratings::AVERAGE;
    assert_eq!(avg.contact_timing_scale(), 1.0);
    assert_eq!(avg.power_factor(), 1.0);
    assert_eq!(avg.arm_factor(), 1.0);
    assert_eq!(avg.range_factor(), 1.0);
    let star = Ratings {
        contact: 70,
        power: 70,
        arm: 70,
        fielding: 70,
        ..avg
    };
    assert!(
        star.contact_timing_scale() < 1.0,
        "a good hitter misses by less"
    );
    assert!(star.power_factor() > 1.0);
    assert!(star.arm_factor() > 1.0);
    assert!(star.range_factor() > 1.0);
    let off_scale = Ratings {
        power: 99,
        arm: 0,
        ..avg
    };
    let top = Ratings { power: 80, ..avg };
    let bottom = Ratings { arm: 20, ..avg };
    assert_eq!(off_scale.power_factor(), top.power_factor());
    assert_eq!(off_scale.arm_factor(), bottom.arm_factor());
}

#[test]
fn an_old_file_without_the_new_grades_reads_as_average() {
    let r: Ratings = ron::from_str("(speed: 65)").unwrap();
    assert_eq!(
        r,
        Ratings {
            speed: 65,
            ..Ratings::AVERAGE
        }
    );
}
