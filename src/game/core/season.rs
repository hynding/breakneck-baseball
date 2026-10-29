//! Short season mode: a four-club league where the human plays a 6-, 9- or
//! 12-game schedule against the three CPU clubs, the top two in the
//! standings meet in a one-game final, and the whole thing persists through
//! the settings store between launches (`settings::Settings::season`).
//!
//! Pure data and arithmetic — the Bevy glue (launching a season game from
//! the menu, recording its final score) lives in `meta/season_mode.rs`.
//! Every round the human plays one CPU club while the other two play each
//! other; that CPU-vs-CPU game is not played out on the field but scored by
//! a deterministic hash of the season number and round, so a season replays
//! identically and the standings stay testable.

use serde::{Deserialize, Serialize};

/// Clubs in the league. Index 0 is always the human's club.
pub const TEAMS: usize = 4;

/// The human's club — always [`crate::game::Team::Home`] in a season game.
pub const HUMAN: usize = 0;

/// Standings names, indexed by club. The CPU names are A–Z only like the
/// rosters, so they could ever go on a jersey (`roster.rs`).
pub const CLUB_NAMES: [&str; TEAMS] = ["You", "Rockets", "Gators", "Comets"];

/// Regular-season lengths the menu cycles through, in human games — each a
/// multiple of the three CPU opponents, so everyone meets everyone equally.
pub const SEASON_LENGTHS: [u32; 3] = [6, 9, 12];

/// The length a fresh settings store starts with.
pub const DEFAULT_SEASON_LENGTH: u32 = 9;

/// The next season length in the menu cycle (wraps; values not in the list
/// restart it) — the same shape as `variant::next_innings`.
pub fn next_season_length(current: u32) -> u32 {
    match SEASON_LENGTHS.iter().position(|&n| n == current) {
        Some(i) => SEASON_LENGTHS[(i + 1) % SEASON_LENGTHS.len()],
        None => SEASON_LENGTHS[0],
    }
}

/// One finished game between two clubs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameResult {
    pub home: usize,
    pub away: usize,
    pub home_runs: u32,
    pub away_runs: u32,
}

impl GameResult {
    /// The winning club, or `None` for a tie.
    pub fn winner(&self) -> Option<usize> {
        match self.home_runs.cmp(&self.away_runs) {
            std::cmp::Ordering::Greater => Some(self.home),
            std::cmp::Ordering::Less => Some(self.away),
            std::cmp::Ordering::Equal => None,
        }
    }
}

/// The human's next game: who, and whether it is the final.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Matchup {
    pub opponent: usize,
    /// 1-based regular-season game number (`length + 1` for the final).
    pub game: u32,
    pub is_final: bool,
}

/// One club's line in the standings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Standing {
    pub team: usize,
    pub wins: u32,
    pub losses: u32,
    pub ties: u32,
    pub runs_for: u32,
    pub runs_against: u32,
}

impl Standing {
    /// Two points a win, one a tie — every club plays the same number of
    /// games, so points rank exactly like winning percentage.
    pub fn points(&self) -> u32 {
        2 * self.wins + self.ties
    }

    pub fn run_diff(&self) -> i64 {
        i64::from(self.runs_for) - i64::from(self.runs_against)
    }
}

/// A season in progress or just finished.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Season {
    /// Counts up across seasons; seeds the CPU-vs-CPU scores.
    pub number: u32,
    /// Regular-season games for every club (one of [`SEASON_LENGTHS`]).
    pub length: u32,
    /// Regular-season results in play order: two per round, the human's
    /// game first, then the CPU-vs-CPU game.
    pub results: Vec<GameResult>,
    /// The final, once played (or simulated, when the human missed it).
    pub final_game: Option<GameResult>,
}

impl Season {
    pub fn new(number: u32, length: u32) -> Self {
        Self {
            number,
            length,
            results: Vec::new(),
            final_game: None,
        }
    }

    /// Regular-season rounds completed (= human games played).
    pub fn played(&self) -> u32 {
        (self.results.len() / 2) as u32
    }

    pub fn regular_season_done(&self) -> bool {
        self.played() >= self.length
    }

    pub fn is_complete(&self) -> bool {
        self.final_game.is_some()
    }

    /// The champion, once the final is in.
    pub fn champion(&self) -> Option<usize> {
        self.final_game.and_then(|g| g.winner())
    }

    /// The opponent in regular-season round `round` (0-based): the three CPU
    /// clubs in rotation.
    fn opponent_in(round: u32) -> usize {
        1 + (round as usize % (TEAMS - 1))
    }

    /// The human's next game, or `None` once the season is over (including
    /// a regular season the human finished outside the top two).
    pub fn next_game(&self) -> Option<Matchup> {
        if self.is_complete() {
            return None;
        }
        if !self.regular_season_done() {
            return Some(Matchup {
                opponent: Self::opponent_in(self.played()),
                game: self.played() + 1,
                is_final: false,
            });
        }
        let (a, b) = self.finalists();
        (a == HUMAN || b == HUMAN).then(|| Matchup {
            opponent: if a == HUMAN { b } else { a },
            game: self.length + 1,
            is_final: true,
        })
    }

    /// Records the human's game (human at home, as every season game is
    /// played) and advances the league: in the regular season the round's
    /// CPU-vs-CPU game is scored alongside it, and once the schedule is done
    /// a final the human missed is simulated at once. A tied final records
    /// nothing — it is replayed. Returns whether anything was recorded.
    pub fn record(&mut self, human_runs: u32, opponent_runs: u32) -> bool {
        let Some(matchup) = self.next_game() else {
            return false;
        };
        let game = GameResult {
            home: HUMAN,
            away: matchup.opponent,
            home_runs: human_runs,
            away_runs: opponent_runs,
        };
        if matchup.is_final {
            if game.winner().is_none() {
                return false;
            }
            self.final_game = Some(game);
            return true;
        }
        let round = self.played();
        self.results.push(game);
        self.results.push(self.simulated_round_game(round));
        if self.regular_season_done() {
            let (a, b) = self.finalists();
            if a != HUMAN && b != HUMAN {
                self.final_game = Some(self.simulated(a, b, u64::from(self.length)));
            }
        }
        true
    }

    /// The CPU-vs-CPU game of `round`: the two clubs the human didn't play,
    /// alternating who hosts.
    fn simulated_round_game(&self, round: u32) -> GameResult {
        let opponent = Self::opponent_in(round);
        let mut others = (1..TEAMS).filter(|&t| t != opponent);
        let (a, b) = (
            others.next().expect("three CPU clubs"),
            others.next().expect("three CPU clubs"),
        );
        let (home, away) = if round % 2 == 0 { (a, b) } else { (b, a) };
        self.simulated(home, away, u64::from(round))
    }

    /// A deterministic score for an off-screen game: three small hash draws
    /// per side (about four runs a game), and a tie broken by one more run
    /// for the side a hash bit picks — the extra-innings walk-off.
    fn simulated(&self, home: usize, away: usize, slot: u64) -> GameResult {
        let h = mix(u64::from(self.number) << 32 | slot);
        let runs = |x: u64| (x % 4 + (x >> 8) % 4 + (x >> 16) % 3) as u32;
        let mut home_runs = runs(h);
        let mut away_runs = runs(h >> 24);
        if home_runs == away_runs {
            if h >> 63 == 0 {
                home_runs += 1;
            } else {
                away_runs += 1;
            }
        }
        GameResult {
            home,
            away,
            home_runs,
            away_runs,
        }
    }

    /// Regular-season standings, best first: points, then run differential,
    /// then runs scored, then club order (the human first on a dead heat).
    pub fn standings(&self) -> Vec<Standing> {
        let mut table: Vec<Standing> = (0..TEAMS)
            .map(|team| Standing {
                team,
                ..Default::default()
            })
            .collect();
        for g in &self.results {
            let (home, away) = (g.home, g.away);
            table[home].runs_for += g.home_runs;
            table[home].runs_against += g.away_runs;
            table[away].runs_for += g.away_runs;
            table[away].runs_against += g.home_runs;
            match g.winner() {
                Some(w) => {
                    let l = if w == home { away } else { home };
                    table[w].wins += 1;
                    table[l].losses += 1;
                }
                None => {
                    table[home].ties += 1;
                    table[away].ties += 1;
                }
            }
        }
        table.sort_by(|a, b| {
            b.points()
                .cmp(&a.points())
                .then(b.run_diff().cmp(&a.run_diff()))
                .then(b.runs_for.cmp(&a.runs_for))
                .then(a.team.cmp(&b.team))
        });
        table
    }

    /// The top two in the standings, first seed first.
    pub fn finalists(&self) -> (usize, usize) {
        let table = self.standings();
        (table[0].team, table[1].team)
    }

    /// The human's own regular-season line.
    pub fn human_record(&self) -> Standing {
        self.standings()
            .into_iter()
            .find(|s| s.team == HUMAN)
            .expect("the human is always in the league")
    }

    /// Whether stored data describes a season this code can continue —
    /// checked after deserializing, so a hand-edited or stale store can
    /// never index out of the league or loop past its schedule.
    pub fn is_valid(&self) -> bool {
        let game_ok = |g: &GameResult| g.home < TEAMS && g.away < TEAMS && g.home != g.away;
        SEASON_LENGTHS.contains(&self.length)
            && self.results.len() % 2 == 0
            && self.played() <= self.length
            && self.results.iter().all(game_ok)
            && self
                .final_game
                .is_none_or(|g| game_ok(&g) && g.winner().is_some() && self.regular_season_done())
    }

    /// One-line progress for the menu, e.g. `Game 4/9 vs Gators  (2-1)`.
    pub fn status_line(&self) -> String {
        let me = self.human_record();
        let record = if me.ties > 0 {
            format!("{}-{}-{}", me.wins, me.losses, me.ties)
        } else {
            format!("{}-{}", me.wins, me.losses)
        };
        match (self.next_game(), self.champion()) {
            (Some(m), _) if m.is_final => {
                format!("Final vs {}  ({record})", CLUB_NAMES[m.opponent])
            }
            (Some(m), _) => format!(
                "Game {}/{} vs {}  ({record})",
                m.game, self.length, CLUB_NAMES[m.opponent]
            ),
            (None, Some(HUMAN)) => "Champions!  New season".to_string(),
            (None, Some(c)) => format!("{} won it  -  New season", CLUB_NAMES[c]),
            (None, None) => "New season".to_string(),
        }
    }

    /// The game-over card's season line: what comes next, or how it ended.
    pub fn next_line(&self) -> String {
        match (self.next_game(), self.champion()) {
            (Some(m), _) if m.is_final => {
                format!("Next: the final vs {}", CLUB_NAMES[m.opponent])
            }
            (Some(m), _) => format!(
                "Next: game {}/{} vs {}",
                m.game, self.length, CLUB_NAMES[m.opponent]
            ),
            (None, Some(HUMAN)) => "SEASON CHAMPIONS!".to_string(),
            (None, Some(c)) => format!("{} won the final", CLUB_NAMES[c]),
            (None, None) => String::new(),
        }
    }

    /// The standings as fixed-width text rows (the default UI font is
    /// monospace), header first. The tie column only appears once a tie has.
    pub fn standings_rows(&self) -> Vec<String> {
        let table = self.standings();
        let ties = table.iter().any(|s| s.ties > 0);
        let mut rows = vec![if ties {
            format!("{:<9}{:>3}{:>3}{:>3}{:>6}", "", "W", "L", "T", "RD")
        } else {
            format!("{:<9}{:>3}{:>3}{:>6}", "", "W", "L", "RD")
        }];
        for (seed, s) in table.iter().enumerate() {
            let name = format!("{}{}", CLUB_NAMES[s.team], if seed < 2 { "*" } else { "" });
            let rd = format!("{:+}", s.run_diff());
            rows.push(if ties {
                format!("{name:<9}{:>3}{:>3}{:>3}{rd:>6}", s.wins, s.losses, s.ties)
            } else {
                format!("{name:<9}{:>3}{:>3}{rd:>6}", s.wins, s.losses)
            });
        }
        rows
    }
}

/// SplitMix64's finalizer — a cheap, well-mixed hash for the simulated
/// scores. Not an RNG: the same season number and slot always score alike.
fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

#[cfg(test)]
#[path = "season.test.rs"]
mod tests;
