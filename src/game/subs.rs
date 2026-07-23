//! Pause & substitutions — stop the game between plays and work the bench.
//!
//! **Esc / P** (or a gamepad's Start) pauses while the ball is dead (waiting
//! on a pitch, or in the result pause with the ball back in hand) and brings
//! up the substitution board: pick a lineup slot (Up/Down), pick a bench
//! player (Left/Right), swap them in (Enter), switch which team you're
//! managing (T), and resume (Esc / P). Gameplay systems all gate on
//! `GameState::Playing`, so the world freezes underneath the board;
//! substitutions rewrite [`Rosters`], which the jerseys and the duel HUD
//! pick up on the next frame.
//!
//! The board itself obeys the wasm/WebGL2 UI rule (see CLAUDE.md): its root
//! is spawned *painted* with the rest of the game UI at game start (hidden
//! behind [`hidden_tint`]) and shown/hidden by mutating the children of that
//! painted root — never despawned or respawned mid-session.

use bevy::prelude::*;

use crate::game::ball::{Baseball, InFlight};
use crate::game::flow::{Phase, Play};
use crate::game::roster::Rosters;
use crate::game::rules::LINEUP_SIZE;
use crate::game::theme::Theme;
use crate::game::ui::hidden_tint;
use crate::game::{GameState, GameplayEntity, ScoreBoard, Team};

/// Marker for the board's overlay root (full-screen dim).
#[derive(Component)]
struct SubsUi;

/// Marker for the board's inner card.
#[derive(Component)]
struct SubsCard;

/// One line of the board, painted by [`update_board`].
#[derive(Component)]
struct SubsLine(SubsLineKind);

#[derive(Clone, Copy, PartialEq, Eq)]
enum SubsLineKind {
    Title,
    LineupHeader,
    Row(usize),
    BenchHeader,
    Bench,
    Hint,
}

/// The board's cursor state.
#[derive(Resource)]
struct SubsMenu {
    team: Team,
    slot: usize,
    bench: usize,
}

impl Default for SubsMenu {
    fn default() -> Self {
        Self {
            team: Team::Home,
            slot: 0,
            bench: 0,
        }
    }
}

pub struct SubsPlugin;

impl Plugin for SubsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SubsMenu>()
            .add_systems(crate::game::game_start(), spawn_board)
            .add_systems(Update, open_pause.run_if(in_state(GameState::Playing)))
            .add_systems(Update, board_controls.run_if(in_state(GameState::Paused)))
            .add_systems(Update, update_board);
    }
}

fn pause_pressed(keyboard: &ButtonInput<KeyCode>, pads: &Query<&Gamepad>) -> bool {
    keyboard.just_pressed(KeyCode::Escape)
        || keyboard.just_pressed(KeyCode::KeyP)
        || pads.iter().any(|p| p.just_pressed(GamepadButton::Start))
}

/// Pauses only while the ball is truly dead: between plays, and never while
/// the ball is still physically in flight (Rapier steps regardless of game
/// state, so a "paused" flying ball would keep moving under the board).
#[allow(clippy::too_many_arguments)]
fn open_pause(
    keyboard: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    play: Res<Play>,
    score: Res<ScoreBoard>,
    flying: Query<(), (With<Baseball>, With<InFlight>)>,
    mut menu: ResMut<SubsMenu>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    if !pause_pressed(&keyboard, &pads) {
        return;
    }
    if !matches!(play.phase, Phase::PrePitch | Phase::Result) || !flying.is_empty() {
        return;
    }
    // Never clobber a transition already decided this frame (e.g. the
    // game-ending resolution setting GameOver).
    if !matches!(*next_state, NextState::Unchanged) {
        return;
    }
    *menu = SubsMenu {
        team: score.batting_team(),
        ..default()
    };
    next_state.set(GameState::Paused);
}

/// Cursor moves and swaps while paused. The board repaints via change
/// detection on [`SubsMenu`] / [`Rosters`]; nothing is respawned.
fn board_controls(
    keyboard: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    mut menu: ResMut<SubsMenu>,
    mut rosters: ResMut<Rosters>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    if pause_pressed(&keyboard, &pads) {
        next_state.set(GameState::Playing);
        return;
    }

    let lineup_len = rosters.team(menu.team).lineup.len();
    let bench_len = rosters.team(menu.team).bench.len().max(1);
    if keyboard.just_pressed(KeyCode::ArrowUp) {
        menu.slot = (menu.slot + lineup_len - 1) % lineup_len;
    }
    if keyboard.just_pressed(KeyCode::ArrowDown) {
        menu.slot = (menu.slot + 1) % lineup_len;
    }
    if keyboard.just_pressed(KeyCode::ArrowLeft) {
        menu.bench = (menu.bench + bench_len - 1) % bench_len;
    }
    if keyboard.just_pressed(KeyCode::ArrowRight) {
        menu.bench = (menu.bench + 1) % bench_len;
    }
    if keyboard.just_pressed(KeyCode::KeyT) {
        menu.team = menu.team.other();
        menu.slot = 0;
        menu.bench = 0;
    }
    if keyboard.just_pressed(KeyCode::Enter) {
        let (slot, bench) = (menu.slot, menu.bench);
        rosters.team_mut(menu.team).substitute(slot, bench);
    }
}

/// Builds the board once at game start, hidden: every element painted with a
/// nonzero-alpha colour so the wasm extractor keeps it forever.
fn spawn_board(mut commands: Commands, theme: Res<Theme>) {
    let ui = &theme.ui;

    commands
        .spawn((
            SubsUi,
            GameplayEntity,
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
            BackgroundColor(hidden_tint(ui.panel_bg)),
        ))
        .with_children(|screen| {
            screen
                .spawn((
                    SubsCard,
                    Node {
                        padding: UiRect::axes(Val::Px(36.0), Val::Px(24.0)),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        row_gap: Val::Px(6.0),
                        border: UiRect::all(Val::Px(1.5)),
                        ..default()
                    },
                    BackgroundColor(hidden_tint(ui.panel_bg)),
                    BorderColor(hidden_tint(ui.panel_border)),
                    BorderRadius::all(Val::Px(16.0)),
                ))
                .with_children(|card| {
                    let mut line = |kind: SubsLineKind, size: f32| {
                        card.spawn((
                            SubsLine(kind),
                            Text::new(""),
                            TextFont {
                                font_size: size,
                                ..default()
                            },
                            TextColor(ui.text_primary),
                        ));
                    };
                    line(SubsLineKind::Title, 30.0);
                    line(SubsLineKind::LineupHeader, 14.0);
                    for i in 0..LINEUP_SIZE as usize {
                        line(SubsLineKind::Row(i), 18.0);
                    }
                    line(SubsLineKind::BenchHeader, 14.0);
                    line(SubsLineKind::Bench, 17.0);
                    line(SubsLineKind::Hint, 13.0);
                });
        });
}

/// Paints the board while paused and blanks it (alpha kept nonzero) in every
/// other state — the show/hide-by-mutation pattern the HUD uses.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn update_board(
    state: Res<State<GameState>>,
    menu: Res<SubsMenu>,
    rosters: Res<Rosters>,
    theme: Res<Theme>,
    added: Query<(), Added<SubsLine>>,
    mut roots: Query<&mut BackgroundColor, (With<SubsUi>, Without<SubsCard>)>,
    mut cards: Query<(&mut BackgroundColor, &mut BorderColor), With<SubsCard>>,
    mut lines: Query<(&SubsLine, &mut Text, &mut TextColor)>,
) {
    let repaint =
        state.is_changed() || menu.is_changed() || rosters.is_changed() || !added.is_empty();
    if !repaint {
        return;
    }
    let ui = &theme.ui;
    let visible = *state.get() == GameState::Paused;

    for mut bg in &mut roots {
        bg.0 = if visible {
            ui.panel_bg.with_alpha(0.9)
        } else {
            hidden_tint(ui.panel_bg)
        };
    }
    for (mut bg, mut border) in &mut cards {
        if visible {
            bg.0 = ui.panel_bg;
            border.0 = ui.panel_border;
        } else {
            bg.0 = hidden_tint(ui.panel_bg);
            border.0 = hidden_tint(ui.panel_border);
        }
    }

    let roster = rosters.team(menu.team);
    for (line, mut text, mut color) in &mut lines {
        if !visible {
            **text = String::new();
            continue;
        }
        let (value, tint) = match line.0 {
            SubsLineKind::Title => (format!("SUBSTITUTIONS - {}", menu.team.label()), ui.accent),
            SubsLineKind::LineupHeader => ("LINEUP".to_string(), ui.text_dim),
            SubsLineKind::Row(i) => {
                let Some(player) = roster.lineup.get(i) else {
                    **text = String::new();
                    continue;
                };
                let selected = i == menu.slot;
                let marker = if selected { ">" } else { " " };
                (
                    format!("{marker} {}. {} #{}", i + 1, player.name, player.number),
                    if selected { ui.accent } else { ui.text_primary },
                )
            }
            SubsLineKind::BenchHeader => ("BENCH".to_string(), ui.text_dim),
            SubsLineKind::Bench => {
                let value = if roster.bench.is_empty() {
                    "(empty)".to_string()
                } else {
                    roster
                        .bench
                        .iter()
                        .enumerate()
                        .map(|(i, p)| {
                            if i == menu.bench {
                                format!("[{} #{}]", p.name, p.number)
                            } else {
                                format!(" {} #{} ", p.name, p.number)
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("  ")
                };
                (value, ui.text_primary)
            }
            SubsLineKind::Hint => (
                "Up/Down slot   Left/Right bench   Enter swap   T team   Esc/P resume".to_string(),
                ui.text_dim,
            ),
        };
        **text = value;
        color.0 = tint;
    }
}
