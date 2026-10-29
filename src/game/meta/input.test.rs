//! Unit tests for [`super`] — the input module.

use super::*;
use crate::game::GameMode;

fn pad(index: u32) -> Entity {
    Entity::from_raw_u32(index).unwrap()
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

#[test]
fn player_index_maps_p1_p2_and_cpu() {
    let one = assign_controllers(GameMode::OnePlayer, &[]);
    assert_eq!(one.player_index(Team::Home), Some(0));
    assert_eq!(one.player_index(Team::Away), None);
    let two = assign_controllers(GameMode::TwoPlayers, &[]);
    assert_eq!(two.player_index(Team::Away), Some(1));
}

#[test]
fn stick_steps_once_per_push_with_hysteresis() {
    let up = Vec2::new(0.0, 0.9);
    // A push latches the dominant axis.
    let latched = stick_step(IVec2::ZERO, up);
    assert_eq!(latched, IVec2::Y);
    // Easing off into the slack band holds the latch (no second step).
    assert_eq!(stick_step(latched, Vec2::new(0.0, 0.45)), IVec2::Y);
    // Back near centre unlatches, so the next push is a fresh step.
    assert_eq!(stick_step(latched, Vec2::new(0.0, 0.1)), IVec2::ZERO);
    // Diagonals pick the dominant axis; a weak tilt from rest does nothing.
    assert_eq!(stick_step(IVec2::ZERO, Vec2::new(-0.8, 0.5)), -IVec2::X);
    assert_eq!(stick_step(IVec2::ZERO, Vec2::new(0.4, 0.4)), IVec2::ZERO);
}
