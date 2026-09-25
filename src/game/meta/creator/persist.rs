//! Save-to-disk: validate then write `working` as pretty RON.

use crate::game::appearance::{RosterDefs, RosterFile};

/// Where [`save_working`] writes by default — the repo's authored roster
/// file. Anchored by `CARGO_MANIFEST_DIR` (not the process's cwd, which
/// nothing guarantees points at the workspace).
const PLAYERS_RON_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data/players.ron");

/// Validates then writes `working` as pretty RON to `path`. Pulled the path
/// out into a parameter (rather than hardcoding [`PLAYERS_RON_PATH`] here) so
/// tests can point this at a temp file instead of the repo's real data —
/// [`save_working`] is the thin wrapper that supplies the real path. Validates
/// *before* writing so a bad working copy (e.g. an unauthored `!` in a name)
/// never touches disk. NOTE: a save always re-serializes the *whole* file in
/// this pretty-RON formatting, so the first save after this lands produces a
/// one-time diff of formatting-only churn against the hand-authored
/// `data/players.ron` — accepted; the Creator hub owns the file's format from
/// here on.
pub fn save_working_to(path: &str, working: &RosterFile) -> Result<(), String> {
    RosterDefs::validate(working)?;
    let text = ron::ser::to_string_pretty(working, ron::ser::PrettyConfig::new())
        .map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

/// Saves `working` to the repo's real `data/players.ron`. The panel's Save
/// button calls this directly; on `Ok` it also refreshes `cs.snapshot` (the
/// saved state becomes the new revert point) and on `Err` routes the message
/// into `cs.status` — both at the call site, since this fn only ever touches
/// the file, never `CreatorState`.
pub fn save_working(working: &RosterFile) -> Result<(), String> {
    save_working_to(PLAYERS_RON_PATH, working)
}

#[cfg(test)]
#[path = "persist.test.rs"]
mod tests;
