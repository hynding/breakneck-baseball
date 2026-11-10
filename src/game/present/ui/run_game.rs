//! The running-game prompt: while a runner can steal, the human side(s)
//! see which button runs their half of the duel — the defense's pickoff,
//! the offense's steal call — and the offense sees its call once it's on.

use bevy::prelude::*;

use crate::game::ScoreBoard;
use crate::game::flow::{LeadState, Phase, Play};
use crate::game::input::Controllers;
use crate::game::rules::{self, Bases};

use super::RunGameHintText;

/// Everything the prompt reads, so the wording is a pure function.
pub(super) struct HintCues {
    pub phase: Phase,
    /// The walk-up hold or a throw in the air: no prompt.
    pub busy: bool,
    /// A runner can steal (`rules::steal_candidate`).
    pub runner: Option<usize>,
    /// The defense's running-game button label, when it's human.
    pub defense_key: Option<&'static str>,
    /// The offense's, when it's human.
    pub offense_key: Option<&'static str>,
    /// The offense has called the steal.
    pub called: bool,
}

/// The prompt line ("" when there's nothing to say).
pub(super) fn hint_line(c: &HintCues) -> String {
    let Some(runner) = c.runner else {
        return String::new();
    };
    if c.busy {
        return String::new();
    }
    let mut parts: Vec<String> = Vec::new();
    // The pickoff is only on before the delivery starts.
    if c.phase == Phase::PrePitch {
        if let Some(key) = c.defense_key {
            parts.push(format!("{key}  PICKOFF {}", rules::base_name(runner)));
        }
    }
    if matches!(c.phase, Phase::PrePitch | Phase::WindUp) {
        if let Some(key) = c.offense_key {
            parts.push(if c.called {
                "STEAL ON!".to_string()
            } else {
                format!("{key}  STEAL {}", rules::base_name(runner + 1))
            });
        }
    }
    parts.join("      ")
}

pub(super) fn update_run_game_hint(
    play: Res<Play>,
    bases: Res<Bases>,
    score: Res<ScoreBoard>,
    lead: Res<LeadState>,
    controllers: Res<Controllers>,
    mut text_q: Query<&mut Text, With<RunGameHintText>>,
) {
    let cues = HintCues {
        phase: play.phase,
        busy: play.walkup_active() || play.duel_throw().is_some(),
        runner: rules::steal_candidate(&bases),
        defense_key: controllers.secondary_label(score.fielding_team()),
        offense_key: controllers.secondary_label(score.batting_team()),
        called: lead.called,
    };
    let line = hint_line(&cues);
    for mut text in &mut text_q {
        if text.0 != line {
            text.0.clone_from(&line);
        }
    }
}

#[cfg(test)]
#[path = "run_game.test.rs"]
mod tests;
