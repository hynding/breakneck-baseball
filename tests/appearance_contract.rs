//! Contract test over `data/players.ron` — the shipped player definitions.
//! AI/hub edits to the file fail here (fast, in CI) instead of breaking
//! rendering silently. Mirrors the invariants `roster.rs` unit tests pin.

use breakneck_baseball::game::appearance::{embedded_roster_file, RosterDefs, APPEARANCE_VERSION};
use breakneck_baseball::game::rules::LINEUP_SIZE;

#[test]
fn shipped_players_file_parses_and_validates() {
    let file = embedded_roster_file();
    assert_eq!(file.version, APPEARANCE_VERSION);
    RosterDefs::validate(&file).expect("data/players.ron violates a roster invariant");
}

#[test]
fn both_teams_field_nine_with_a_bench() {
    let file = embedded_roster_file();
    for pool in [&file.home, &file.away] {
        assert!(
            pool.len() > LINEUP_SIZE as usize,
            "need bench beyond the nine"
        );
    }
}
