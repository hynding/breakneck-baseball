//! Main menu and game-over screens.
//!
//! The menu gates entry into [`GameState::Playing`]: the player picks a mode
//! (1 player vs CPU, or 2 players), a field variant (**F**), and a theme
//! (**T**). Styling comes entirely from the active [`Theme`]; cycling either
//! option rebuilds the menu so the new look/labels show immediately.

use bevy::color::Alpha;
use bevy::prelude::*;

use crate::game::input::{assign_controllers, Controllers};
use crate::game::theme::Theme;
use crate::game::variant::{self, FieldSpec, Ruleset};
use crate::game::{GameConfig, GameMode, GameState, ScoreBoard};

/// Marker for menu-screen UI so it can be torn down on exit or rebuild.
#[derive(Component)]
struct MenuUi;

/// The line that shows how many controllers are connected.
#[derive(Component)]
struct ControllerStatus;

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
                (update_controller_status, cycle_options, menu_select)
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

fn spawn_menu(mut commands: Commands, config: Res<GameConfig>, theme: Res<Theme>) {
    build_menu(&mut commands, &config, &theme);
}

/// Builds the full menu tree. Called on entering the menu and again whenever
/// an option cycles (the old tree is despawned first).
fn build_menu(commands: &mut Commands, config: &GameConfig, theme: &Theme) {
    let ui = &theme.ui;

    commands
        .spawn((
            MenuUi,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(ui.panel_bg.with_alpha(0.97)),
        ))
        .with_children(|screen| {
            screen
                .spawn((
                    Node {
                        padding: UiRect::axes(Val::Px(44.0), Val::Px(30.0)),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        row_gap: Val::Px(13.0),
                        border: UiRect::all(Val::Px(1.5)),
                        ..default()
                    },
                    BackgroundColor(ui.panel_bg),
                    BorderColor(ui.panel_border),
                    BorderRadius::all(Val::Px(16.0)),
                ))
                .with_children(|card| {
                    card.spawn((
                        Text::new("BREAKNECK BASEBALL"),
                        TextFont {
                            font_size: 48.0,
                            ..default()
                        },
                        TextColor(ui.accent),
                    ));
                    card.spawn((
                        Text::new("backyard arcade baseball"),
                        TextFont {
                            font_size: 15.0,
                            ..default()
                        },
                        TextColor(ui.text_dim),
                        Node {
                            margin: UiRect::bottom(Val::Px(10.0)),
                            ..default()
                        },
                    ));

                    for line in [
                        "1   One Player  (vs CPU)".to_string(),
                        "2   Two Players".to_string(),
                    ] {
                        card.spawn((
                            Text::new(line),
                            TextFont {
                                font_size: 23.0,
                                ..default()
                            },
                            TextColor(ui.text_primary),
                        ));
                    }

                    // Option lines: dim key/label, accent value.
                    for (label, value) in [
                        ("F   Field", config.variant.label().to_string()),
                        ("I   Innings", config.innings.to_string()),
                        ("T   Theme", config.theme.label().to_string()),
                    ] {
                        card.spawn((
                            Text::new(format!("{label}   ")),
                            TextFont {
                                font_size: 19.0,
                                ..default()
                            },
                            TextColor(ui.text_dim),
                        ))
                        .with_child((
                            TextSpan::new(value),
                            TextFont {
                                font_size: 19.0,
                                ..default()
                            },
                            TextColor(ui.accent),
                        ));
                    }

                    card.spawn((
                        ControllerStatus,
                        Text::new(""),
                        TextFont {
                            font_size: 15.0,
                            ..default()
                        },
                        TextColor(ui.count_ball),
                        Node {
                            margin: UiRect::top(Val::Px(10.0)),
                            ..default()
                        },
                    ));
                    card.spawn((
                        Text::new(
                            "Controller: A pitch/swing, stick to aim\nKeyboard: WASD + Space (P1), Arrows + Right-Ctrl (P2)",
                        ),
                        TextFont {
                            font_size: 13.0,
                            ..default()
                        },
                        TextColor(ui.text_dim),
                        TextLayout::new_with_justify(JustifyText::Center),
                    ));
                });
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

/// Cycles the field (**F** / gamepad West), innings (**I** / gamepad East),
/// and theme (**T** / gamepad North), then rebuilds the menu so labels and
/// palette refresh together.
fn cycle_options(
    keyboard: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    mut config: ResMut<GameConfig>,
    mut theme: ResMut<Theme>,
    menu_q: Query<Entity, With<MenuUi>>,
    mut commands: Commands,
) {
    let field_pressed = keyboard.just_pressed(KeyCode::KeyF)
        || pads.iter().any(|p| p.just_pressed(GamepadButton::West));
    let theme_pressed = keyboard.just_pressed(KeyCode::KeyT)
        || pads.iter().any(|p| p.just_pressed(GamepadButton::North));
    let innings_pressed = keyboard.just_pressed(KeyCode::KeyI)
        || pads.iter().any(|p| p.just_pressed(GamepadButton::East));

    if !field_pressed && !theme_pressed && !innings_pressed {
        return;
    }
    if field_pressed {
        config.variant = config.variant.next();
        // A new park brings its own regulation length; I re-cycles from there.
        config.innings = config.variant.rules().innings;
    }
    if innings_pressed {
        config.innings = variant::next_innings(config.innings);
    }
    if theme_pressed {
        config.theme = config.theme.next();
        *theme = config.theme.build();
    }

    for entity in &menu_q {
        commands.entity(entity).despawn_recursive();
    }
    build_menu(&mut commands, &config, &theme);
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
    // game's rules and park; the menu's game-length choice overrides the
    // variant's default.
    *rules = config.variant.rules();
    rules.innings = config.innings;
    *field = config.variant.field();
    next_state.set(GameState::Playing);
}

// ── Game over ─────────────────────────────────────────────────────────────────

fn spawn_game_over(mut commands: Commands, score: Res<ScoreBoard>, theme: Res<Theme>) {
    let ui = &theme.ui;
    let (winner, color) = if score.home_runs > score.away_runs {
        ("HOME WINS", theme.home.jersey)
    } else if score.away_runs > score.home_runs {
        ("AWAY WINS", theme.away.jersey)
    } else {
        ("TIE GAME", ui.text_primary)
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
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(ui.panel_bg.with_alpha(0.92)),
        ))
        .with_children(|screen| {
            screen
                .spawn((
                    Node {
                        padding: UiRect::axes(Val::Px(50.0), Val::Px(34.0)),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        row_gap: Val::Px(14.0),
                        border: UiRect::all(Val::Px(1.5)),
                        ..default()
                    },
                    BackgroundColor(ui.panel_bg),
                    BorderColor(ui.panel_border),
                    BorderRadius::all(Val::Px(16.0)),
                ))
                .with_children(|card| {
                    card.spawn((
                        Text::new(winner),
                        TextFont {
                            font_size: 52.0,
                            ..default()
                        },
                        TextColor(color),
                    ));
                    card.spawn((
                        Text::new(format!(
                            "Final   AWAY {}  -  HOME {}",
                            score.away_runs, score.home_runs
                        )),
                        TextFont {
                            font_size: 28.0,
                            ..default()
                        },
                        TextColor(ui.text_primary),
                    ));
                    card.spawn((
                        Text::new("Enter / A  -  back to the menu"),
                        TextFont {
                            font_size: 17.0,
                            ..default()
                        },
                        TextColor(ui.text_dim),
                    ));
                });
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
