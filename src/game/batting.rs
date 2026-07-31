//! Batting input adapters (spec §3): each style is a front end that turns raw
//! [`Intents`] into the same [`SwingInput`]; `flow::pitch_live` consumes the
//! command and never sees the style. The CPU always routes Classic.

use bevy::prelude::*;

use crate::game::flow::{Phase, Play};
use crate::game::input::{Controllers, Intents};
use crate::game::settings::{BattingStyle, Settings};
use crate::game::{ScoreBoard, Team};

/// One swing, decided this frame. The swing instant is implicit (the frame
/// the command exists); `pci_offset` is the PCI cursor's zone-plane position
/// at the press (world x / height y), `None` for Classic and Meter.
pub struct SwingInput {
    pub aim: Vec2,
    pub pci_offset: Option<Vec2>,
}

/// This frame's swing command per team, produced by [`adapt_swings`] and
/// consumed (once) by `flow::pitch_live`. Commands are single-frame: cleared
/// at the top of every `adapt_swings` run.
#[derive(Resource, Default)]
pub struct SwingCommands {
    home: Option<SwingInput>,
    away: Option<SwingInput>,
}

impl SwingCommands {
    /// Peek at the pending command for `team`, if any.
    pub fn get(&self, team: Team) -> Option<&SwingInput> {
        match team {
            Team::Home => self.home.as_ref(),
            Team::Away => self.away.as_ref(),
        }
    }

    /// Consume the pending command for `team`, if any.
    pub fn take(&mut self, team: Team) -> Option<SwingInput> {
        match team {
            Team::Home => self.home.take(),
            Team::Away => self.away.take(),
        }
    }

    /// Set the pending command for `team`.
    pub fn set(&mut self, team: Team, cmd: SwingInput) {
        match team {
            Team::Home => self.home = Some(cmd),
            Team::Away => self.away = Some(cmd),
        }
    }
}

/// Which batting style drives `team`'s swing this at-bat. The CPU always
/// routes Classic (spec §3) regardless of the settings screen's per-player
/// choices — those only apply to a human-controlled slot.
pub fn style_for(team: Team, controllers: &Controllers, settings: &Settings) -> BattingStyle {
    match controllers.player_index(team) {
        None => BattingStyle::ClassicTiming, // CPU: always Classic (spec §3)
        Some(i) => settings.batting_style[i],
    }
}

/// The adapter: runs after `cpu_offense` (so CPU edges are visible) and
/// before `pre_pitch`/`pitch_live` (so a command lands the same frame).
pub fn adapt_swings(
    intents: Res<Intents>,
    controllers: Res<Controllers>,
    settings: Res<Settings>,
    score: Res<ScoreBoard>,
    play: Res<Play>,
    mut commands: ResMut<SwingCommands>,
) {
    let team = score.batting_team();
    let intent = intents.get(team);
    // Commands are single-frame: clear both slots first.
    *commands = SwingCommands::default();
    if play.phase != Phase::Pitch {
        return;
    }
    match style_for(team, &controllers, &settings) {
        BattingStyle::ClassicTiming => {
            if intent.action {
                commands.set(
                    team,
                    SwingInput {
                        aim: intent.aim,
                        pci_offset: None,
                    },
                );
            }
        }
        // Meter and PCI arms land in C2/C4; until then they fall through to
        // Classic so the game stays playable in every style.
        BattingStyle::SwingMeter | BattingStyle::PciCursor => {
            if intent.action {
                commands.set(
                    team,
                    SwingInput {
                        aim: intent.aim,
                        pci_offset: None,
                    },
                );
            }
        }
    }
}

/// Registers [`SwingCommands`]; the [`adapt_swings`] system itself is chained
/// explicitly by `FlowPlugin` so ordering relative to `cpu_offense`/
/// `pre_pitch` is visible in one place.
pub struct BattingPlugin;

impl Plugin for BattingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SwingCommands>();
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::input::assign_controllers;
    use crate::game::GameMode;

    #[test]
    fn cpu_always_routes_classic() {
        let controllers = assign_controllers(GameMode::OnePlayer, &[]);
        let settings = Settings {
            batting_style: [BattingStyle::PciCursor, BattingStyle::SwingMeter],
            ..Settings::default()
        };
        assert_eq!(
            style_for(Team::Away, &controllers, &settings),
            BattingStyle::ClassicTiming
        );
        assert_eq!(
            style_for(Team::Home, &controllers, &settings),
            BattingStyle::PciCursor
        );
    }
}
