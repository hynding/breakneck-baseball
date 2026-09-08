//! Unit tests for [`super`] — the input module.

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

#[test]
fn player_index_maps_p1_p2_and_cpu() {
    let one = assign_controllers(GameMode::OnePlayer, &[]);
    assert_eq!(one.player_index(Team::Home), Some(0));
    assert_eq!(one.player_index(Team::Away), None);
    let two = assign_controllers(GameMode::TwoPlayers, &[]);
    assert_eq!(two.player_index(Team::Away), Some(1));
}
