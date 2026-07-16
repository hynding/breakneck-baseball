//! Main menu and game-over screens.
//!
//! The menu gates entry into [`GameState::Playing`]: the player picks a mode
//! (1 player vs CPU, or 2 players) and this module builds the [`Controllers`]
//! assignment from the connected gamepads before starting the game.

use bevy::prelude::*;

use crate::game::input::{assign_controllers, Controllers};
use crate::game::variant::{FieldSpec, Ruleset};
use crate::game::{GameConfig, GameMode, GameState, ScoreBoard};

/// Marker for menu-screen UI so it can be torn down on exit.
#[derive(Component)]
struct MenuUi;

/// The line that shows how many controllers are connected.
#[derive(Component)]
struct ControllerStatus;

/// The line that shows the currently selected field variant.
#[derive(Component)]
struct FieldChoice;

/// Marker for game-over UI.
#[derive(Component)]
struct GameOverUi;

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameState::MainMenu), spawn_menu)
            .add_systems(OnExit(GameState::MainMenu), despawn::<MenuUi>)
            .add_systems(
                Update,
                (update_controller_status, cycle_field_choice, menu_select)
                    .run_if(in_state(GameState::MainMenu)),
            )
            .add_systems(OnEnter(GameState::GameOver), spawn_game_over)
            .add_systems(OnExit(GameState::GameOver), despawn::<GameOverUi>)
            .add_systems(
                Update,
                game_over_restart.run_if(in_state(GameState::GameOver)),
            );
    }
}

// ── Main menu ─────────────────────────────────────────────────────────────────

fn spawn_menu(mut commands: Commands) {
    commands
        .spawn((
            MenuUi,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(18.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.06, 0.12, 0.94)),
        ))
        .with_children(|root| {
            root.spawn((
                Text::new("BREAKNECK BASEBALL"),
                TextFont {
                    font_size: 44.0,
                    ..default()
                },
                TextColor(Color::srgb(1.0, 0.86, 0.2)),
                TextLayout::new_with_justify(JustifyText::Center),
            ));
            root.spawn((
                Text::new("Press  1   -   One Player (vs CPU)\nPress  2   -   Two Players"),
                TextFont {
                    font_size: 24.0,
                    ..default()
                },
                TextColor(Color::WHITE),
                TextLayout::new_with_justify(JustifyText::Center),
            ));
            root.spawn((
                FieldChoice,
                Text::new(""),
                TextFont {
                    font_size: 20.0,
                    ..default()
                },
                TextColor(Color::srgb(1.0, 0.86, 0.2)),
                TextLayout::new_with_justify(JustifyText::Center),
            ));
            root.spawn((
                ControllerStatus,
                Text::new(""),
                TextFont {
                    font_size: 17.0,
                    ..default()
                },
                TextColor(Color::srgb(0.6, 0.85, 0.6)),
                TextLayout::new_with_justify(JustifyText::Center),
            ));
            root.spawn((
                Text::new("Controller: A pitch/swing, stick to aim\nKeyboard: WASD + Space (P1), Arrows + Right-Ctrl (P2)"),
                TextFont {
                    font_size: 15.0,
                    ..default()
                },
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.7)),
                TextLayout::new_with_justify(JustifyText::Center),
            ));
        });
}

fn update_controller_status(
    pads: Query<(), With<Gamepad>>,
    mut query: Query<&mut Text, With<ControllerStatus>>,
) {
    let count = pads.iter().count();
    let msg = match count {
        0 => "No controllers detected - keyboard fallback active".to_string(),
        1 => "1 controller connected".to_string(),
        n => format!("{n} controllers connected"),
    };
    for mut text in &mut query {
        if text.as_str() != msg {
            **text = msg.clone();
        }
    }
}

/// Cycles the field variant with **F** (or a controller's West/X button) and
/// keeps the menu line in sync.
fn cycle_field_choice(
    keyboard: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    mut config: ResMut<GameConfig>,
    mut query: Query<&mut Text, With<FieldChoice>>,
) {
    let pressed = keyboard.just_pressed(KeyCode::KeyF)
        || pads.iter().any(|p| p.just_pressed(GamepadButton::West));
    if pressed {
        config.variant = config.variant.next();
    }

    let label = format!("Field:  {}     (F / X to change)", config.variant.label());
    for mut text in &mut query {
        if text.as_str() != label {
            **text = label.clone();
        }
    }
}

fn menu_select(
    keyboard: Res<ButtonInput<KeyCode>>,
    pads: Query<(Entity, &Gamepad)>,
    mut config: ResMut<GameConfig>,
    mut controllers: ResMut<Controllers>,
    mut rules: ResMut<Ruleset>,
    mut field: ResMut<FieldSpec>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    // Selection: keyboard digits, or a controller face button (1P) / start (2P).
    let want_one = keyboard.just_pressed(KeyCode::Digit1)
        || keyboard.just_pressed(KeyCode::Numpad1)
        || pads
            .iter()
            .any(|(_, p)| p.just_pressed(GamepadButton::South));
    let want_two = keyboard.just_pressed(KeyCode::Digit2)
        || keyboard.just_pressed(KeyCode::Numpad2)
        || pads
            .iter()
            .any(|(_, p)| p.just_pressed(GamepadButton::Start));

    let mode = if want_two {
        GameMode::TwoPlayers
    } else if want_one {
        GameMode::OnePlayer
    } else {
        return;
    };

    let pad_entities: Vec<Entity> = pads.iter().map(|(e, _)| e).collect();
    config.mode = mode;
    *controllers = assign_controllers(mode, &pad_entities);
    // Materialize the chosen variant so every gameplay system reads this
    // game's rules and park.
    *rules = config.variant.rules();
    *field = config.variant.field();
    next_state.set(GameState::Playing);
}

// ── Game over ─────────────────────────────────────────────────────────────────

fn spawn_game_over(mut commands: Commands, score: Res<ScoreBoard>) {
    let (winner, color) = if score.home_runs > score.away_runs {
        ("HOME WINS", Color::srgb(0.4, 0.6, 1.0))
    } else if score.away_runs > score.home_runs {
        ("AWAY WINS", Color::srgb(1.0, 0.45, 0.35))
    } else {
        ("TIE GAME", Color::WHITE)
    };

    commands
        .spawn((
            GameOverUi,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(18.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.06, 0.12, 0.94)),
        ))
        .with_children(|root| {
            root.spawn((
                Text::new(winner),
                TextFont {
                    font_size: 52.0,
                    ..default()
                },
                TextColor(color),
            ));
            root.spawn((
                Text::new(format!(
                    "Final — Away {}   Home {}",
                    score.away_runs, score.home_runs
                )),
                TextFont {
                    font_size: 30.0,
                    ..default()
                },
                TextColor(Color::WHITE),
            ));
            root.spawn((
                Text::new("Press  Enter  or  A  to return to the menu"),
                TextFont {
                    font_size: 20.0,
                    ..default()
                },
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.8)),
            ));
        });
}

fn game_over_restart(
    keyboard: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    let confirm = keyboard.just_pressed(KeyCode::Enter)
        || keyboard.just_pressed(KeyCode::Space)
        || pads.iter().any(|p| p.just_pressed(GamepadButton::South));
    if confirm {
        next_state.set(GameState::MainMenu);
    }
}

// ── Shared ────────────────────────────────────────────────────────────────────

/// Generic despawn-by-marker used on state exit.
fn despawn<T: Component>(mut commands: Commands, query: Query<Entity, With<T>>) {
    for entity in &query {
        commands.entity(entity).despawn_recursive();
    }
}
