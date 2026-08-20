//! Team rosters — named, numbered players with a bench for substitutions.
//!
//! Pure bookkeeping data, like [`crate::game::variant`]: the rules engine
//! still races anonymous kinematics, but every lineup slot is a real player
//! whose name and number the jerseys ([`crate::game::jersey`]) and the duel
//! HUD display. Arcade convention: batting-order slot `i` also plays
//! defensive position `i` (slot 0 pitches, slots 1.. take the field spots in
//! spec order). The pause menu swaps bench players into lineup slots between
//! plays; re-entry is allowed — this is backyard ball, not the rulebook.

use bevy::prelude::{Component, Resource};

use crate::game::Team;
use crate::game::appearance::{PlayerAppearance, PlayerDef, RosterDefs};
use crate::game::rules::LINEUP_SIZE;

/// One player: jersey name (A–Z only — the procedural jersey font's
/// alphabet), number, and personal appearance recipe (authored in
/// `data/players.ron`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerCard {
    pub name: String,
    pub number: u32,
    pub appearance: PlayerAppearance,
}

/// One team's nine starters plus the bench.
#[derive(Clone, Debug)]
pub struct TeamRoster {
    /// Batting order == defensive assignment (arcade rules; see module doc).
    pub lineup: Vec<PlayerCard>,
    /// Alternates available to substitute in.
    pub bench: Vec<PlayerCard>,
}

impl TeamRoster {
    fn from_defs(defs: &[PlayerDef]) -> Self {
        let mut cards: Vec<PlayerCard> = defs
            .iter()
            .map(|d| PlayerCard {
                name: d.name.clone(),
                number: d.number,
                appearance: d.appearance,
            })
            .collect();
        let bench = cards.split_off(LINEUP_SIZE as usize);
        Self {
            lineup: cards,
            bench,
        }
    }

    /// The card batting in 1-based lineup `slot` (the value
    /// [`crate::game::rules::BattingOrder::current`] reports).
    pub fn batting(&self, slot: u32) -> &PlayerCard {
        &self.lineup[(slot as usize - 1).min(self.lineup.len() - 1)]
    }

    /// The card at defensive position: the pitcher is lineup slot 0 and
    /// fielder spot `i` is lineup slot `i + 1` (wrapping for small parks).
    pub fn fielding(&self, spot: Option<usize>) -> &PlayerCard {
        match spot {
            None => &self.lineup[0],
            Some(i) => &self.lineup[(i + 1) % self.lineup.len()],
        }
    }

    /// Direct lineup access by 0-based index, clamped like [`Self::batting`]
    /// — the lookup [`crate::game::roster`] identity consumers use.
    pub fn card(&self, index: usize) -> &PlayerCard {
        &self.lineup[index.min(self.lineup.len() - 1)]
    }

    /// Swaps bench player `bench_index` into lineup `slot` (0-indexed); the
    /// replaced starter takes the bench seat.
    pub fn substitute(&mut self, slot: usize, bench_index: usize) {
        if slot < self.lineup.len() && bench_index < self.bench.len() {
            std::mem::swap(&mut self.lineup[slot], &mut self.bench[bench_index]);
        }
    }
}

/// Both teams' rosters, reset to the default squads when a game starts.
#[derive(Resource, Clone, Debug)]
pub struct Rosters {
    pub home: TeamRoster,
    pub away: TeamRoster,
}

impl Rosters {
    pub fn team(&self, team: Team) -> &TeamRoster {
        match team {
            Team::Home => &self.home,
            Team::Away => &self.away,
        }
    }

    pub fn team_mut(&mut self, team: Team) -> &mut TeamRoster {
        match team {
            Team::Home => &mut self.home,
            Team::Away => &mut self.away,
        }
    }
}

impl Rosters {
    pub fn from_defs(defs: &RosterDefs) -> Self {
        Self {
            home: TeamRoster::from_defs(&defs.0.home),
            away: TeamRoster::from_defs(&defs.0.away),
        }
    }
}

impl Default for Rosters {
    fn default() -> Self {
        Self::from_defs(&RosterDefs::default())
    }
}

/// Which roster seat a rig is playing *right now* — team-relative, so the
/// same physical rig means a different player after a half-inning flip.
/// Static per rig; [`PlayerIdentity`] is the derived, refreshed answer.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum RosterRole {
    Pitcher,
    Fielder(usize),
    Batter,
}

/// Who a rig currently is: the key every appearance system looks up cards
/// with. Kept fresh by `player::sync_identities`; runner rigs get theirs
/// stamped once at spawn (a runner never changes person mid-play).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayerIdentity {
    pub team: Team,
    pub index: usize,
}

impl RosterRole {
    pub fn identity(
        &self,
        score: &crate::game::ScoreBoard,
        order: &crate::game::rules::BattingOrder,
        rosters: &Rosters,
    ) -> PlayerIdentity {
        match self {
            RosterRole::Pitcher => PlayerIdentity {
                team: score.fielding_team(),
                index: 0,
            },
            RosterRole::Fielder(i) => {
                let team = score.fielding_team();
                PlayerIdentity {
                    team,
                    index: (i + 1) % rosters.team(team).lineup.len(),
                }
            }
            RosterRole::Batter => {
                let team = score.batting_team();
                let len = rosters.team(team).lineup.len();
                PlayerIdentity {
                    team,
                    index: (order.current(team) as usize - 1).min(len - 1),
                }
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
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
}
