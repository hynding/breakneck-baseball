//! Heads-up display — score board, count, inning indicator, and key bindings.
//!
//! Uses Bevy's built-in UI system (rendered via wgpu) to display live game
//! data from the [`ScoreBoard`] resource.

use bevy::prelude::*;

use crate::game::{GameState, ScoreBoard};

// ── Marker components ─────────────────────────────────────────────────────────
/// Root node of the score-board UI.
#[derive(Component)]
struct ScoreBoardRoot;

/// The individual text node that shows the live score / count.
#[derive(Component)]
struct ScoreText;

/// Helper bar shown at the bottom of the screen.
#[derive(Component)]
struct ControlsHint;

// ── Plugin ────────────────────────────────────────────────────────────────────
pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameState::Playing), spawn_hud)
            .add_systems(
                Update,
                update_score_text.run_if(in_state(GameState::Playing)),
            );
    }
}

// ── Startup: build the UI tree ────────────────────────────────────────────────
fn spawn_hud(mut commands: Commands) {
    // ── Score board (top-left) ───────────────────────────────────────────────
    commands
        .spawn((
            ScoreBoardRoot,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(10.0),
                left: Val::Px(10.0),
                padding: UiRect::all(Val::Px(8.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(4.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.65)),
            BorderRadius::all(Val::Px(6.0)),
        ))
        .with_children(|parent| {
            parent.spawn((
                ScoreText,
                Text::new(""),
                TextFont {
                    font_size: 20.0,
                    ..default()
                },
                TextColor(Color::WHITE),
            ));
        });

    // ── Controls hint (bottom-centre) ────────────────────────────────────────
    commands.spawn((
        ControlsHint,
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(10.0),
            left: Val::Percent(50.0),
            ..default()
        },
        Text::new("SPACE — Pitch   |   WASD/Arrows — Orbit camera   |   Q/E — Zoom   |   R — Reset camera"),
        TextFont {
            font_size: 14.0,
            ..default()
        },
        TextColor(Color::srgba(1.0, 1.0, 1.0, 0.8)),
    ));
}

// ── Update: refresh the score text every frame ────────────────────────────────
fn update_score_text(score: Res<ScoreBoard>, mut query: Query<&mut Text, With<ScoreText>>) {
    if !score.is_changed() {
        return;
    }

    let half = if score.top_of_inning { "Top" } else { "Bot" };
    let count_str = format!("{}-{}", score.balls, score.strikes);

    for mut text in &mut query {
        **text = format!(
            "Inning: {} {}\nAway: {}   Home: {}\nCount: {}   Outs: {}",
            score.inning, half, score.away_runs, score.home_runs, count_str, score.outs,
        );
    }
}
