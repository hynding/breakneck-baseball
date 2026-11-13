//! Pitcher fatigue and the bullpen — every arm tires as its pitch count
//! climbs, and the CPU goes to the bullpen when its pitcher is spent.
//!
//! Pure bookkeeping and arithmetic, like [`crate::game::mojo`]: no RNG and
//! no systems (`flow` counts each pitch as it leaves the hand and makes the
//! CPU's pitching changes). Counts are kept per team and jersey number, so
//! a pitcher swapped out and back in keeps his count, and they reset when a
//! game starts.
//!
//! An arm is fresh until [`FADE_PITCHES`] short of its stamina, then fades
//! linearly: by the time the count reaches its stamina the fastball has lost
//! [`VELOCITY_FADE`] of its speed (about 1 mph on a 95 mph heater) and the
//! aim wanders by [`CONTROL_FADE`] of the stick's range, and it keeps fading
//! past that. The starter (whoever is on the mound when the game starts)
//! has [`STARTER_STAMINA`]; anyone after him is a reliever with
//! [`RELIEVER_STAMINA`]. Per docs/BASEBALL.md, "Pitcher fatigue and the
//! bullpen".

use bevy::prelude::Resource;

use crate::game::Team;

/// A starter's pitch count when the CPU goes to the bullpen: MLB starters
/// averaged 85 pitches a start in 2025 (per docs/BASEBALL.md).
pub const STARTER_STAMINA: u32 = 85;
/// A reliever's.
pub const RELIEVER_STAMINA: u32 = 25;
/// An arm starts to fade this many pitches short of its stamina.
pub const FADE_PITCHES: u32 = 25;
/// Fraction of release speed lost per [`FADE_PITCHES`] of fade.
pub const VELOCITY_FADE: f32 = 0.011;
/// Aim wander (in stick units, −1..=1 being the stick's range) per
/// [`FADE_PITCHES`] of fade.
pub const CONTROL_FADE: f32 = 0.12;

/// One team's arms: who started, and every pitch count so far.
#[derive(Clone, Debug, Default, PartialEq)]
struct Staff {
    /// The jersey number on the mound at the first pitch.
    starter: Option<u32>,
    /// `(jersey number, pitches thrown)` for everyone who has pitched.
    counts: Vec<(u32, u32)>,
}

/// Both staffs' pitch counts this game.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct Fatigue {
    home: Staff,
    away: Staff,
}

impl Fatigue {
    fn staff(&self, team: Team) -> &Staff {
        match team {
            Team::Home => &self.home,
            Team::Away => &self.away,
        }
    }

    fn staff_mut(&mut self, team: Team) -> &mut Staff {
        match team {
            Team::Home => &mut self.home,
            Team::Away => &mut self.away,
        }
    }

    /// Counts one pitch thrown by `team`'s pitcher wearing `number`. The
    /// first pitch a team throws marks its starter.
    pub fn record_pitch(&mut self, team: Team, number: u32) {
        let staff = self.staff_mut(team);
        staff.starter.get_or_insert(number);
        match staff.counts.iter_mut().find(|(n, _)| *n == number) {
            Some((_, pitches)) => *pitches += 1,
            None => staff.counts.push((number, 1)),
        }
    }

    /// Pitches thrown so far by `team`'s pitcher wearing `number`.
    pub fn pitches(&self, team: Team, number: u32) -> u32 {
        self.staff(team)
            .counts
            .iter()
            .find(|(n, _)| *n == number)
            .map_or(0, |&(_, p)| p)
    }

    /// Whether `number` has pitched for `team` this game.
    pub fn has_pitched(&self, team: Team, number: u32) -> bool {
        self.staff(team).counts.iter().any(|(n, _)| *n == number)
    }

    /// The stamina of `team`'s pitcher wearing `number`: the starter's if
    /// he started (or nobody has pitched yet), a reliever's otherwise.
    pub fn stamina(&self, team: Team, number: u32) -> u32 {
        match self.staff(team).starter {
            Some(starter) if starter != number => RELIEVER_STAMINA,
            _ => STARTER_STAMINA,
        }
    }

    /// How tired `team`'s pitcher wearing `number` is: 0 while fresh, 1 at
    /// his stamina, more past it.
    pub fn tiredness(&self, team: Team, number: u32) -> f32 {
        tiredness(self.pitches(team, number), self.stamina(team, number))
    }

    /// Whether the CPU should go to the bullpen for `team`'s pitcher
    /// wearing `number`.
    pub fn spent(&self, team: Team, number: u32) -> bool {
        self.pitches(team, number) >= self.stamina(team, number)
    }
}

/// 0 while `pitches` is more than [`FADE_PITCHES`] short of `stamina`,
/// rising by 1 every [`FADE_PITCHES`] after that.
pub fn tiredness(pitches: u32, stamina: u32) -> f32 {
    let fresh = stamina.saturating_sub(FADE_PITCHES);
    pitches.saturating_sub(fresh) as f32 / FADE_PITCHES as f32
}

/// The release-speed multiplier at `tiredness`.
pub fn velocity_scale(tiredness: f32) -> f32 {
    (1.0 - VELOCITY_FADE * tiredness).max(0.8)
}

/// How far (stick units) the aim wanders at `tiredness`.
pub fn control_wander(tiredness: f32) -> f32 {
    CONTROL_FADE * tiredness
}

/// The bench seat of the reliever to bring in: the first arm on `bench`
/// (jersey numbers, in bench order) who hasn't pitched yet. `None` when
/// the bullpen is empty.
pub fn pick_reliever(fatigue: &Fatigue, team: Team, bench: &[u32]) -> Option<usize> {
    bench.iter().position(|&n| !fatigue.has_pitched(team, n))
}

#[cfg(test)]
#[path = "fatigue.test.rs"]
mod tests;
