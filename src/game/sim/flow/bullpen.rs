//! The CPU's pitching changes: when the arm on the mound is spent
//! ([`Fatigue::spent`]), the CPU brings in the first bench arm that hasn't
//! pitched yet, between plate appearances. A human manager makes his own
//! changes from the pause board (`game::subs`); the walk-up card shows his
//! pitcher's count so he knows when.

use bevy::prelude::*;

use super::{BannerTone, Phase, Play, PlayBanner};
use crate::game::ScoreBoard;
use crate::game::fatigue::{Fatigue, pick_reliever};
use crate::game::input::{Controllers, InputSource};
use crate::game::roster::Rosters;

pub(super) fn cpu_bullpen(
    controllers: Res<Controllers>,
    score: Res<ScoreBoard>,
    play: Res<Play>,
    fatigue: Res<Fatigue>,
    mut rosters: ResMut<Rosters>,
    mut banner: MessageWriter<PlayBanner>,
) {
    // Only before a plate appearance's first pitch.
    if play.phase != Phase::PrePitch || score.balls != 0 || score.strikes != 0 {
        return;
    }
    let team = score.fielding_team();
    if controllers.source(team) != InputSource::Cpu {
        return;
    }
    let roster = rosters.team(team);
    if !fatigue.spent(team, roster.card(0).number) {
        return;
    }
    let bench: Vec<u32> = roster.bench.iter().map(|c| c.number).collect();
    let Some(seat) = pick_reliever(&fatigue, team, &bench) else {
        return;
    };
    rosters.team_mut(team).substitute(0, seat);
    let name = rosters.team(team).card(0).name.clone();
    banner.write(PlayBanner::new(
        format!("PITCHING CHANGE: {name}"),
        BannerTone::Info,
    ));
}

#[cfg(test)]
#[path = "bullpen.test.rs"]
mod tests;
