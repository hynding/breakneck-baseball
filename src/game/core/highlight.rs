//! The play of the game — the one moment the final card names.
//!
//! Pure scoring, like [`crate::game::mojo`]: no RNG and no systems (`flow`
//! weighs each finished play into [`BestPlay`]). A play's weight grows with
//! the runs it scored, how late it came, whether it put its side ahead, and
//! tops out at a walk-off; the heaviest play of the game is kept, the
//! earlier one winning a tie.

use bevy::prelude::Resource;

/// What one finished play did.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayFacts {
    /// Runs that scored on it.
    pub runs: u32,
    /// The inning it came in, and the game's regulation length.
    pub inning: u32,
    pub innings: u32,
    /// It turned a tie or a deficit into a lead.
    pub go_ahead: bool,
    /// It ended the game in the home side's last at-bat.
    pub walk_off: bool,
    /// It was a home run.
    pub home_run: bool,
}

/// How big a play was. 0 for a play that scored nothing and wasn't a home
/// run — the final card only names plays that moved the scoreboard.
pub fn play_weight(f: &PlayFacts) -> f32 {
    if f.runs == 0 && !f.home_run {
        return 0.0;
    }
    // Later innings count for more: ×1 in the first, ×2 by the last.
    let lateness = 1.0 + (f.inning.min(f.innings) as f32 - 1.0) / (f.innings.max(2) as f32 - 1.0);
    let mut w = f.runs as f32 * lateness;
    if f.home_run {
        w += 0.5;
    }
    if f.go_ahead {
        w += 1.5 * lateness;
    }
    if f.walk_off {
        w += 10.0;
    }
    w
}

/// The game's biggest play so far: its weight, its call, and when.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct BestPlay {
    pub weight: f32,
    /// The umpire's call, as the banner read ("HOME RUN!  +2").
    pub call: String,
    pub inning: u32,
    pub top: bool,
}

impl BestPlay {
    /// Keeps `call` if it outweighs the best so far.
    pub fn offer(&mut self, facts: &PlayFacts, call: &str, top: bool) {
        let weight = play_weight(facts);
        if weight > self.weight {
            *self = BestPlay {
                weight,
                call: call.to_string(),
                inning: facts.inning,
                top,
            };
        }
    }

    /// The final card's line, e.g. "BOT 9  -  WALK-OFF!  SINGLE  +1";
    /// `None` when nothing scored all game.
    pub fn line(&self) -> Option<String> {
        (self.weight > 0.0).then(|| {
            let half = if self.top { "TOP" } else { "BOT" };
            format!("{half} {}  -  {}", self.inning, self.call)
        })
    }
}

#[cfg(test)]
#[path = "highlight.test.rs"]
mod tests;
