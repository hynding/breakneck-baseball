//! Unit tests for [`super`] — the creator module.

use super::*;
use crate::game::appearance::embedded_roster_file;
use crate::game::rules::LINEUP_SIZE;

#[test]
fn selected_def_spans_lineup_then_bench() {
    let mut file = embedded_roster_file();
    let lineup_name = file.home[3].name.clone();
    assert_eq!(selected_def(&mut file, Team::Home, 3).name, lineup_name);

    // Bench index 10 is pool index 10 (lineup 0..9, bench from 9).
    let bench_name = file.home[10].name.clone();
    assert_eq!(selected_def(&mut file, Team::Home, 10).name, bench_name);

    // Out-of-range index clamps to the last player rather than panicking.
    let last_name = file.away.last().unwrap().name.clone();
    let far_index = file.away.len() + 50;
    assert_eq!(
        selected_def(&mut file, Team::Away, far_index).name,
        last_name
    );
}

#[test]
fn selected_def_edits_are_visible_through_the_same_file() {
    let mut file = embedded_roster_file();
    selected_def(&mut file, Team::Home, 0).appearance.headwear =
        crate::game::appearance::Headwear::Bare;
    assert_eq!(
        file.home[0].appearance.headwear,
        crate::game::appearance::Headwear::Bare
    );
}

#[test]
fn bench_selection_remaps_to_lineup_slot_zero() {
    let file = embedded_roster_file();
    let lineup_size = LINEUP_SIZE as usize;
    let bench_index = lineup_size + 1; // second bench player (0-based within bench)
    let bench_name = file.home[bench_index].name.clone();

    let (rosters, id) = preview_rosters_and_identity(&file, Team::Home, bench_index);

    assert_eq!(
        id,
        PlayerIdentity {
            team: Team::Home,
            index: 0
        }
    );
    assert_eq!(rosters.home.lineup[0].name, bench_name);
}

#[test]
fn lineup_selection_keeps_index_and_ordering_unmodified() {
    let file = embedded_roster_file();
    let (rosters, id) = preview_rosters_and_identity(&file, Team::Home, 3);

    assert_eq!(
        id,
        PlayerIdentity {
            team: Team::Home,
            index: 3
        }
    );
    let expected = Rosters::from_defs(&RosterDefs(file.clone()));
    assert_eq!(
        rosters
            .home
            .lineup
            .iter()
            .map(|c| &c.name)
            .collect::<Vec<_>>(),
        expected
            .home
            .lineup
            .iter()
            .map(|c| &c.name)
            .collect::<Vec<_>>(),
    );
}
