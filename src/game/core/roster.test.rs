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
