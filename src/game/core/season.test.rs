//! Unit tests for [`super`] — the short season's schedule and standings.

use super::*;

/// Plays the human's whole regular season with a fixed score each game.
fn play_out(season: &mut Season, human: u32, opponent: u32) {
    while season.next_game().is_some_and(|m| !m.is_final) {
        assert!(season.record(human, opponent));
    }
}

#[test]
fn every_club_plays_the_full_schedule_and_meets_everyone_equally() {
    for length in SEASON_LENGTHS {
        let mut season = Season::new(1, length);
        play_out(&mut season, 3, 2);
        let table = season.standings();
        for s in &table {
            assert_eq!(
                s.wins + s.losses + s.ties,
                length,
                "{} played the wrong number of games in a {length}-game season",
                CLUB_NAMES[s.team]
            );
        }
        // Every pair meets length / 3 times.
        for a in 0..TEAMS {
            for b in (a + 1)..TEAMS {
                let meetings = season
                    .results
                    .iter()
                    .filter(|g| (g.home == a && g.away == b) || (g.home == b && g.away == a))
                    .count() as u32;
                assert_eq!(meetings, length / 3, "clubs {a} and {b}");
            }
        }
    }
}

#[test]
fn standings_add_up() {
    let mut season = Season::new(7, 9);
    play_out(&mut season, 4, 4);
    let table = season.standings();
    let wins: u32 = table.iter().map(|s| s.wins).sum();
    let losses: u32 = table.iter().map(|s| s.losses).sum();
    assert_eq!(wins, losses);
    let rf: u32 = table.iter().map(|s| s.runs_for).sum();
    let ra: u32 = table.iter().map(|s| s.runs_against).sum();
    assert_eq!(rf, ra);
    // Sorted best first.
    assert!(table.windows(2).all(|w| w[0].points() >= w[1].points()));
    // The human tied every game; simulated games never tie.
    assert_eq!(season.human_record().ties, 9);
    assert!(
        season
            .results
            .iter()
            .filter(|g| g.home != HUMAN)
            .all(|g| g.winner().is_some())
    );
}

#[test]
fn a_winning_human_reaches_the_final_and_can_win_it() {
    let mut season = Season::new(1, 6);
    play_out(&mut season, 10, 0);
    assert!(season.regular_season_done());
    assert_eq!(season.standings()[0].team, HUMAN);
    let fin = season.next_game().expect("the top seed plays the final");
    assert!(fin.is_final);
    assert_eq!(fin.game, 7);
    assert_ne!(fin.opponent, HUMAN);
    assert_eq!(fin.opponent, season.finalists().1);

    // A tied final is replayed, not recorded.
    assert!(!season.record(2, 2));
    assert_eq!(season.next_game(), Some(fin));

    assert!(season.record(5, 3));
    assert!(season.is_complete());
    assert_eq!(season.champion(), Some(HUMAN));
    assert_eq!(season.next_game(), None);
    assert!(!season.record(1, 0), "a finished season records nothing");
    assert_eq!(season.status_line(), "Champions!  New season");
    assert_eq!(season.next_line(), "SEASON CHAMPIONS!");
}

#[test]
fn a_losing_human_misses_the_final_and_it_is_simulated() {
    let mut season = Season::new(3, 9);
    play_out(&mut season, 0, 10);
    assert!(season.is_complete(), "the CPU final plays itself");
    let champion = season.champion().expect("a simulated final has a winner");
    assert_ne!(champion, HUMAN);
    let (a, b) = season.finalists();
    assert!(champion == a || champion == b);
    assert!(season.status_line().contains("won it"));
}

#[test]
fn simulated_scores_are_deterministic_per_season_number() {
    let mut first = Season::new(5, 12);
    let mut again = Season::new(5, 12);
    let mut other = Season::new(6, 12);
    for s in [&mut first, &mut again, &mut other] {
        play_out(s, 3, 1);
    }
    assert_eq!(first, again);
    assert_ne!(first.results, other.results);
    // And plausible: a few runs a game, not blowouts.
    let sims: Vec<_> = first.results.iter().filter(|g| g.home != HUMAN).collect();
    let mean = sims
        .iter()
        .map(|g| f64::from(g.home_runs + g.away_runs))
        .sum::<f64>()
        / (2.0 * sims.len() as f64);
    assert!((2.0..7.0).contains(&mean), "mean runs per side {mean}");
}

#[test]
fn rotation_names_the_next_opponent() {
    let mut season = Season::new(1, 6);
    let opponents: Vec<usize> = (0..6)
        .map(|_| {
            let m = season.next_game().unwrap();
            season.record(1, 0);
            m.opponent
        })
        .collect();
    assert_eq!(opponents, [1, 2, 3, 1, 2, 3]);
}

#[test]
fn status_line_reports_progress() {
    let mut season = Season::new(1, 9);
    assert_eq!(season.status_line(), "Game 1/9 vs Rockets  (0-0)");
    season.record(4, 2);
    season.record(1, 3);
    assert_eq!(season.status_line(), "Game 3/9 vs Comets  (1-1)");
    assert_eq!(season.next_line(), "Next: game 3/9 vs Comets");
}

#[test]
fn standings_rows_are_fixed_width_and_star_the_finalists() {
    let mut season = Season::new(1, 6);
    play_out(&mut season, 6, 1);
    let rows = season.standings_rows();
    assert_eq!(rows.len(), TEAMS + 1);
    let width = rows[0].chars().count();
    assert!(rows.iter().all(|r| r.chars().count() == width), "{rows:#?}");
    assert!(rows[1].starts_with("You*"), "{rows:#?}");
    assert_eq!(rows.iter().filter(|r| r.contains('*')).count(), 2);
}

#[test]
fn validity_rejects_corrupt_stores() {
    let mut season = Season::new(1, 9);
    season.record(2, 1);
    assert!(season.is_valid());

    let mut bad_length = season.clone();
    bad_length.length = 7;
    assert!(!bad_length.is_valid());

    let mut bad_team = season.clone();
    bad_team.results[0].away = TEAMS;
    assert!(!bad_team.is_valid());

    let mut odd = season.clone();
    odd.results.pop();
    assert!(!odd.is_valid());

    let mut early_final = season.clone();
    early_final.final_game = Some(season.results[0]);
    assert!(
        !early_final.is_valid(),
        "a final before the schedule is done"
    );
}

#[test]
fn round_trips_through_json() {
    let mut season = Season::new(2, 12);
    play_out(&mut season, 2, 5);
    let text = serde_json::to_string(&season).unwrap();
    assert_eq!(serde_json::from_str::<Season>(&text).unwrap(), season);
}

#[test]
fn season_length_cycles_and_recovers() {
    assert_eq!(next_season_length(6), 9);
    assert_eq!(next_season_length(9), 12);
    assert_eq!(next_season_length(12), 6);
    assert_eq!(next_season_length(5), 6);
    assert!(SEASON_LENGTHS.contains(&DEFAULT_SEASON_LENGTH));
    assert!(SEASON_LENGTHS.iter().all(|n| n % (TEAMS as u32 - 1) == 0));
}
