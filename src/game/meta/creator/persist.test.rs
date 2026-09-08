//! Unit tests for [`super`] — the persist module.

use super::*;
use crate::game::appearance::embedded_roster_file;

#[test]
fn save_working_to_round_trips_through_ron() {
    let dir = std::env::temp_dir().join(format!("bb-creator-save-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("valid.ron");
    let path_str = path.to_str().unwrap();

    let working = embedded_roster_file();
    let result = save_working_to(path_str, &working);
    assert!(result.is_ok(), "valid roster file must save: {result:?}");

    let text = std::fs::read_to_string(&path).unwrap();
    let reparsed: RosterFile = ron::from_str(&text).unwrap();
    assert_eq!(reparsed, working);

    std::fs::remove_file(&path).ok();
}

#[test]
fn save_working_to_rejects_an_invalid_name_and_writes_nothing() {
    let dir = std::env::temp_dir().join(format!("bb-creator-save-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("invalid.ron");
    let path_str = path.to_str().unwrap();
    std::fs::remove_file(&path).ok();

    let mut working = embedded_roster_file();
    working.home[0].name = "bad!".to_string();
    let result = save_working_to(path_str, &working);
    assert!(result.is_err(), "an invalid name must be rejected");
    assert!(
        !path.exists(),
        "a rejected save must not write anything to disk"
    );
}
