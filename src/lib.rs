//! Library target so integration tests (`tests/`) can build the real game app.
//! The binary (`src/main.rs`) assembles the same [`game::GamePlugin`] plus the
//! windowed Bevy defaults.

pub mod game;
