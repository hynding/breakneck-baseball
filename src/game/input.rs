//! Input abstraction.
//!
//! Every gameplay system reads intent from a single normalized [`Intents`]
//! resource instead of touching keyboards or gamepads directly. Each team's
//! [`TeamIntent`] is refreshed every frame from whatever [`InputSource`] is
//! assigned to that team (a game controller, a keyboard scheme, or the CPU).
//!
//! This is what lets pitching/batting code run identically for a human and the
//! AI: the CPU systems (see `flow.rs`/`player.rs`) simply write into the same
//! [`TeamIntent`] the human input would have produced.

use bevy::input::gamepad::GamepadConnectionEvent;
use bevy::prelude::*;

use crate::game::Team;

// ── Per-team intent ───────────────────────────────────────────────────────────

/// Normalized input for one team for the current frame.
///
/// Meaning depends on whether the team is on offense or defense:
/// - **Defense (pitching):** `aim` steers where the pitch crosses the plate,
///   `action` releases the pitch.
/// - **Offense (batting):** `aim` steers the swing direction (pull/center/oppo),
///   `action` swings.
#[derive(Clone, Copy, Default, Debug)]
pub struct TeamIntent {
    /// Directional aim, components in −1.0..=1.0.
    pub aim: Vec2,
    /// Primary button was pressed this frame (pitch release / swing).
    pub action: bool,
}

/// Normalized intent for both teams, rebuilt every frame.
#[derive(Resource, Default, Debug)]
pub struct Intents {
    pub home: TeamIntent,
    pub away: TeamIntent,
}

impl Intents {
    /// Intent for the given team.
    pub fn get(&self, team: Team) -> TeamIntent {
        match team {
            Team::Home => self.home,
            Team::Away => self.away,
        }
    }

    /// Mutable intent for the given team (used by CPU systems to inject input).
    pub fn get_mut(&mut self, team: Team) -> &mut TeamIntent {
        match team {
            Team::Home => &mut self.home,
            Team::Away => &mut self.away,
        }
    }
}

// ── Input sources ─────────────────────────────────────────────────────────────

/// Two keyboard layouts so two people can share one keyboard when controllers
/// are unavailable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyScheme {
    /// WASD + Space.
    Primary,
    /// Arrow keys + Right-Control.
    Secondary,
}

/// Where a team's input comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputSource {
    /// A connected game controller (the gamepad entity).
    Gamepad(Entity),
    /// A keyboard layout.
    Keyboard(KeyScheme),
    /// Driven by the AI. Intent is written by CPU systems, not this module.
    Cpu,
}

/// Which input source drives each team. Chosen at game start from the selected
/// mode and the set of connected controllers, and updated on hotplug.
#[derive(Resource, Debug)]
pub struct Controllers {
    pub home: InputSource,
    pub away: InputSource,
}

impl Default for Controllers {
    fn default() -> Self {
        // Sensible default before a mode is chosen: single keyboard vs CPU.
        Self {
            home: InputSource::Keyboard(KeyScheme::Primary),
            away: InputSource::Cpu,
        }
    }
}

impl Controllers {
    pub fn source(&self, team: Team) -> InputSource {
        match team {
            Team::Home => self.home,
            Team::Away => self.away,
        }
    }
}

// ── Plugin ────────────────────────────────────────────────────────────────────

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Intents>()
            .init_resource::<Controllers>()
            // Rebuild intents early each frame so all gameplay systems see them.
            .add_systems(PreUpdate, gather_intents)
            .add_systems(Update, handle_gamepad_hotplug);
    }
}

// ── Systems ───────────────────────────────────────────────────────────────────

/// Rebuilds [`Intents`] for both human-driven teams. CPU teams are left at their
/// current value so CPU systems (which run later) can populate them.
fn gather_intents(
    controllers: Res<Controllers>,
    keyboard: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut intents: ResMut<Intents>,
) {
    for team in [Team::Home, Team::Away] {
        match controllers.source(team) {
            InputSource::Gamepad(entity) => {
                if let Ok(pad) = gamepads.get(entity) {
                    *intents.get_mut(team) = gamepad_intent(pad);
                } else {
                    // Controller vanished this frame; neutral until hotplug fixes it.
                    *intents.get_mut(team) = TeamIntent::default();
                }
            }
            InputSource::Keyboard(scheme) => {
                *intents.get_mut(team) = keyboard_intent(&keyboard, scheme);
            }
            // CPU intents are written by AI systems; don't clobber them here.
            InputSource::Cpu => {}
        }
    }
}

fn gamepad_intent(pad: &Gamepad) -> TeamIntent {
    // Prefer the analog stick; fall back to the d-pad for aim.
    let mut aim = pad.left_stick();
    if aim.length() < 0.2 {
        let dpad = pad.dpad();
        if dpad.length() > 0.0 {
            aim = dpad;
        }
    }
    TeamIntent {
        aim,
        action: pad.just_pressed(GamepadButton::South),
    }
}

fn keyboard_intent(keyboard: &ButtonInput<KeyCode>, scheme: KeyScheme) -> TeamIntent {
    let (up, down, left, right, action) = match scheme {
        KeyScheme::Primary => (
            KeyCode::KeyW,
            KeyCode::KeyS,
            KeyCode::KeyA,
            KeyCode::KeyD,
            KeyCode::Space,
        ),
        KeyScheme::Secondary => (
            KeyCode::ArrowUp,
            KeyCode::ArrowDown,
            KeyCode::ArrowLeft,
            KeyCode::ArrowRight,
            KeyCode::ControlRight,
        ),
    };

    let mut aim = Vec2::ZERO;
    if keyboard.pressed(up) {
        aim.y += 1.0;
    }
    if keyboard.pressed(down) {
        aim.y -= 1.0;
    }
    if keyboard.pressed(left) {
        aim.x -= 1.0;
    }
    if keyboard.pressed(right) {
        aim.x += 1.0;
    }

    TeamIntent {
        aim,
        action: keyboard.just_pressed(action),
    }
}

/// Keeps [`Controllers`] valid when a gamepad is unplugged: a disconnected pad
/// falls back to keyboard input so the game keeps running.
fn handle_gamepad_hotplug(
    mut events: EventReader<GamepadConnectionEvent>,
    mut controllers: ResMut<Controllers>,
) {
    for event in events.read() {
        if event.disconnected() {
            for (team, scheme) in [
                (Team::Home, KeyScheme::Primary),
                (Team::Away, KeyScheme::Secondary),
            ] {
                if controllers.source(team) == InputSource::Gamepad(event.gamepad) {
                    let slot = match team {
                        Team::Home => &mut controllers.home,
                        Team::Away => &mut controllers.away,
                    };
                    *slot = InputSource::Keyboard(scheme);
                }
            }
        }
    }
}

/// Assigns input sources to teams given the chosen mode and the currently
/// connected controllers. Used by the menu when a game starts.
pub fn assign_controllers(mode: crate::game::GameMode, pads: &[Entity]) -> Controllers {
    use crate::game::GameMode;
    match mode {
        GameMode::OnePlayer => Controllers {
            home: pads
                .first()
                .copied()
                .map(InputSource::Gamepad)
                .unwrap_or(InputSource::Keyboard(KeyScheme::Primary)),
            away: InputSource::Cpu,
        },
        GameMode::TwoPlayers => Controllers {
            home: pads
                .first()
                .copied()
                .map(InputSource::Gamepad)
                .unwrap_or(InputSource::Keyboard(KeyScheme::Primary)),
            away: pads
                .get(1)
                .copied()
                .map(InputSource::Gamepad)
                .unwrap_or(InputSource::Keyboard(KeyScheme::Secondary)),
        },
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::GameMode;

    fn pad(index: u32) -> Entity {
        Entity::from_raw(index)
    }

    #[test]
    fn one_player_no_pads_is_keyboard_vs_cpu() {
        let c = assign_controllers(GameMode::OnePlayer, &[]);
        assert_eq!(c.home, InputSource::Keyboard(KeyScheme::Primary));
        assert_eq!(c.away, InputSource::Cpu);
    }

    #[test]
    fn one_player_with_pad_uses_it_for_the_human() {
        let c = assign_controllers(GameMode::OnePlayer, &[pad(0)]);
        assert_eq!(c.home, InputSource::Gamepad(pad(0)));
        assert_eq!(c.away, InputSource::Cpu);
    }

    #[test]
    fn two_players_no_pads_split_the_keyboard() {
        let c = assign_controllers(GameMode::TwoPlayers, &[]);
        assert_eq!(c.home, InputSource::Keyboard(KeyScheme::Primary));
        assert_eq!(c.away, InputSource::Keyboard(KeyScheme::Secondary));
    }

    #[test]
    fn two_players_one_pad_gives_p2_the_keyboard() {
        let c = assign_controllers(GameMode::TwoPlayers, &[pad(0)]);
        assert_eq!(c.home, InputSource::Gamepad(pad(0)));
        assert_eq!(c.away, InputSource::Keyboard(KeyScheme::Secondary));
    }

    #[test]
    fn two_players_two_pads_assigns_in_order() {
        let c = assign_controllers(GameMode::TwoPlayers, &[pad(0), pad(1)]);
        assert_eq!(c.home, InputSource::Gamepad(pad(0)));
        assert_eq!(c.away, InputSource::Gamepad(pad(1)));
    }
}
