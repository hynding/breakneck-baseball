//! Heads-up display — scoreboard, base-runner diamond, and result banners.
//!
//! All live game data comes from the [`ScoreBoard`] and [`Bases`] resources and
//! from [`PlayBanner`] events fired by `flow.rs`.

use bevy::prelude::*;

use crate::game::flow::PlayBanner;
use crate::game::rules::Bases;
use crate::game::variant::FieldSpec;
use crate::game::{GameState, GameplayEntity, ScoreBoard};

// ── Markers ───────────────────────────────────────────────────────────────────

#[derive(Component)]
struct ScoreBoardRoot;

#[derive(Component)]
struct ScoreText;

/// One base-occupancy pip (0-indexed base number).
#[derive(Component)]
struct BaseIndicator(usize);

/// The large transient result text in the centre of the screen.
#[derive(Component)]
struct BannerText;

/// How long the current banner stays fully visible before clearing.
#[derive(Resource)]
struct BannerTimer(Timer);

impl Default for BannerTimer {
    fn default() -> Self {
        Self(Timer::from_seconds(1.6, TimerMode::Once))
    }
}

const BASE_ON: Color = Color::srgb(1.0, 0.86, 0.2);
const BASE_OFF: Color = Color::srgba(1.0, 1.0, 1.0, 0.32);

// ── Plugin ────────────────────────────────────────────────────────────────────

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BannerTimer>()
            .add_systems(OnEnter(GameState::Playing), spawn_hud)
            .add_systems(
                Update,
                (
                    update_score_text,
                    update_base_diamond,
                    show_banner,
                    fade_banner,
                )
                    .run_if(in_state(GameState::Playing)),
            );
    }
}

// ── Build the UI tree ─────────────────────────────────────────────────────────

fn spawn_hud(mut commands: Commands, field: Res<FieldSpec>) {
    // Scoreboard (top-left).
    commands
        .spawn((
            ScoreBoardRoot,
            GameplayEntity,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(12.0),
                left: Val::Px(12.0),
                padding: UiRect::all(Val::Px(10.0)),
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

    // Base-runner diamond (top-right).
    spawn_base_diamond(&mut commands, field.base_count());

    // Result banner (centre).
    commands.spawn((
        BannerText,
        GameplayEntity,
        Node {
            position_type: PositionType::Absolute,
            top: Val::Percent(30.0),
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            ..default()
        },
        Text::new(""),
        TextFont {
            font_size: 52.0,
            ..default()
        },
        TextColor(Color::NONE),
    ));

    // Controls hint (bottom-centre).
    commands.spawn((
        GameplayEntity,
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(10.0),
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            ..default()
        },
        Text::new("Aim: Stick / WASD / Arrows     A / Space: Pitch & Swing     C: Camera"),
        TextFont {
            font_size: 15.0,
            ..default()
        },
        TextColor(Color::srgba(1.0, 1.0, 1.0, 0.75)),
    ));
}

/// Spawns a 90×90 px ring of base pips in the top-right corner: one pip per
/// base, laid out like the field (home at the bottom, first base to the
/// right, running counter-clockwise).
fn spawn_base_diamond(commands: &mut Commands, base_count: usize) {
    const BOX: f32 = 90.0;
    const RADIUS: f32 = 34.0;
    const PIP: f32 = 18.0;

    commands
        .spawn((
            GameplayEntity,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(14.0),
                right: Val::Px(18.0),
                width: Val::Px(BOX),
                height: Val::Px(BOX),
                ..default()
            },
        ))
        .with_children(|d| {
            // Home sits at the bottom of the ring (angle −90°); base k of n
            // takes the k-th step around the circle of n + 1 points.
            let step = std::f32::consts::TAU / (base_count as f32 + 1.0);
            for base in 0..base_count {
                let angle = -std::f32::consts::FRAC_PI_2 + step * (base as f32 + 1.0);
                let left = BOX / 2.0 + RADIUS * angle.cos() - PIP / 2.0;
                let top = BOX / 2.0 - RADIUS * angle.sin() - PIP / 2.0;
                d.spawn((
                    BaseIndicator(base),
                    Node {
                        position_type: PositionType::Absolute,
                        top: Val::Px(top),
                        left: Val::Px(left),
                        width: Val::Px(PIP),
                        height: Val::Px(PIP),
                        ..default()
                    },
                    BackgroundColor(BASE_OFF),
                    // Rotate 45° so the square reads as a base.
                    Transform::from_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_4)),
                ));
            }
        });
}

// ── Update systems ────────────────────────────────────────────────────────────

fn update_score_text(score: Res<ScoreBoard>, mut query: Query<&mut Text, With<ScoreText>>) {
    if !score.is_changed() {
        return;
    }
    let half = if score.top_of_inning { "TOP" } else { "BOT" };
    for mut text in &mut query {
        **text = format!(
            "{half} {}\nAWAY {}   HOME {}\nB {}   S {}   O {}",
            score.inning, score.away_runs, score.home_runs, score.balls, score.strikes, score.outs,
        );
    }
}

fn update_base_diamond(
    bases: Res<Bases>,
    mut query: Query<(&BaseIndicator, &mut BackgroundColor)>,
) {
    if !bases.is_changed() {
        return;
    }
    for (indicator, mut color) in &mut query {
        let occupied = bases.is_occupied(indicator.0);
        color.0 = if occupied { BASE_ON } else { BASE_OFF };
    }
}

fn show_banner(
    mut events: EventReader<PlayBanner>,
    mut timer: ResMut<BannerTimer>,
    mut query: Query<(&mut Text, &mut TextColor), With<BannerText>>,
) {
    // Show only the latest banner this frame.
    if let Some(banner) = events.read().last() {
        for (mut text, mut color) in &mut query {
            **text = banner.text.clone();
            color.0 = banner.color;
        }
        timer.0 = Timer::from_seconds(1.6, TimerMode::Once);
    }
}

fn fade_banner(
    time: Res<Time>,
    mut timer: ResMut<BannerTimer>,
    mut query: Query<(&mut Text, &mut TextColor), With<BannerText>>,
) {
    if timer.0.finished() {
        return;
    }
    if timer.0.tick(time.delta()).just_finished() {
        for (mut text, mut color) in &mut query {
            **text = String::new();
            color.0 = Color::NONE;
        }
    }
}
