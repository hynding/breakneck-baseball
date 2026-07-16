//! Heads-up display — scoreboard card, count dots, base ring, and banners.
//!
//! All live game data comes from the [`ScoreBoard`] and [`Bases`] resources
//! and from [`PlayBanner`] events fired by `flow.rs`. Every colour and
//! styling knob comes from the active [`Theme`] — the HUD owns layout only.

use bevy::prelude::*;

use crate::game::flow::{BannerTone, PlayBanner};
use crate::game::rules::Bases;
use crate::game::theme::Theme;
use crate::game::variant::{FieldSpec, Ruleset};
use crate::game::{GameState, GameplayEntity, ScoreBoard};

// ── Markers ───────────────────────────────────────────────────────────────────

#[derive(Component)]
struct InningText;

#[derive(Component)]
struct ScoreText;

/// Which at-bat counter a dot belongs to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CountKind {
    Ball,
    Strike,
    Out,
}

/// One indicator dot: lights up while `index <` the current count.
#[derive(Component)]
struct CountDot {
    kind: CountKind,
    index: u32,
}

/// One base-occupancy pip (0-indexed base number).
#[derive(Component)]
struct BaseIndicator(usize);

/// The banner pill container (hidden when nothing is being announced).
#[derive(Component)]
struct BannerPill;

/// The banner text inside the pill.
#[derive(Component)]
struct BannerText;

/// How long the current banner stays visible before clearing.
#[derive(Resource)]
struct BannerTimer(Timer);

impl Default for BannerTimer {
    fn default() -> Self {
        Self(Timer::from_seconds(1.6, TimerMode::Once))
    }
}

// ── Plugin ────────────────────────────────────────────────────────────────────

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BannerTimer>()
            .add_systems(OnEnter(GameState::Playing), spawn_hud)
            .add_systems(
                Update,
                (
                    update_inning_text,
                    update_score_text,
                    update_count_dots,
                    update_base_ring,
                    show_banner,
                    fade_banner,
                )
                    .run_if(in_state(GameState::Playing)),
            );
    }
}

// ── Build the UI tree ─────────────────────────────────────────────────────────

fn spawn_hud(
    mut commands: Commands,
    field: Res<FieldSpec>,
    rules: Res<Ruleset>,
    theme: Res<Theme>,
) {
    let ui = &theme.ui;

    // Scoreboard card (top-left).
    commands
        .spawn((
            GameplayEntity,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(14.0),
                left: Val::Px(14.0),
                padding: UiRect::axes(Val::Px(16.0), Val::Px(12.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                border: UiRect::all(Val::Px(1.5)),
                ..default()
            },
            BackgroundColor(ui.panel_bg),
            BorderColor(ui.panel_border),
            BorderRadius::all(Val::Px(12.0)),
        ))
        .with_children(|card| {
            card.spawn((
                InningText,
                Text::new(""),
                TextFont {
                    font_size: 16.0,
                    ..default()
                },
                TextColor(ui.accent),
            ));
            card.spawn((
                ScoreText,
                Text::new(""),
                TextFont {
                    font_size: 22.0,
                    ..default()
                },
                TextColor(ui.text_primary),
            ));

            // Count row: classic B / S / O indicator lights. The dot counts
            // follow the active ruleset, so custom thresholds render right.
            card.spawn(Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(12.0),
                align_items: AlignItems::Center,
                margin: UiRect::top(Val::Px(2.0)),
                ..default()
            })
            .with_children(|row| {
                let groups = [
                    (CountKind::Ball, "B", rules.balls_per_walk - 1),
                    (CountKind::Strike, "S", rules.strikes_per_out - 1),
                    (CountKind::Out, "O", rules.outs_per_half - 1),
                ];
                for (kind, label, dots) in groups {
                    row.spawn(Node {
                        flex_direction: FlexDirection::Row,
                        column_gap: Val::Px(4.0),
                        align_items: AlignItems::Center,
                        ..default()
                    })
                    .with_children(|group| {
                        group.spawn((
                            Text::new(label),
                            TextFont {
                                font_size: 14.0,
                                ..default()
                            },
                            TextColor(ui.text_dim),
                        ));
                        for index in 0..dots {
                            group.spawn((
                                CountDot { kind, index },
                                Node {
                                    width: Val::Px(10.0),
                                    height: Val::Px(10.0),
                                    ..default()
                                },
                                BackgroundColor(ui.pip_off),
                                BorderRadius::MAX,
                            ));
                        }
                    });
                }
            });
        });

    spawn_base_ring(&mut commands, field.base_count(), &theme);

    // Banner: a centred pill that appears only while announcing a result.
    commands
        .spawn((
            GameplayEntity,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Percent(26.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .with_children(|wrap| {
            wrap.spawn((
                BannerPill,
                Node {
                    padding: UiRect::axes(Val::Px(30.0), Val::Px(10.0)),
                    border: UiRect::all(Val::Px(1.5)),
                    ..default()
                },
                BackgroundColor(ui.panel_bg),
                BorderColor(ui.panel_border),
                BorderRadius::all(Val::Px(26.0)),
                Visibility::Hidden,
            ))
            .with_children(|pill| {
                pill.spawn((
                    BannerText,
                    Text::new(""),
                    TextFont {
                        font_size: 46.0,
                        ..default()
                    },
                    TextColor(ui.text_primary),
                ));
            });
        });

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
        Text::new("Aim: Stick / WASD / Arrows      A / Space: Pitch & Swing      C: Camera"),
        TextFont {
            font_size: 13.0,
            ..default()
        },
        TextColor(ui.text_dim),
        TextLayout::new_with_justify(JustifyText::Center),
    ));
}

/// A 96×96 px ring of base pips (top-right): one pip per base, laid out like
/// the field — home at the bottom, first base to the right, counter-clockwise.
fn spawn_base_ring(commands: &mut Commands, base_count: usize, theme: &Theme) {
    const BOX: f32 = 96.0;
    const RADIUS: f32 = 34.0;
    const PIP: f32 = 17.0;

    commands
        .spawn((
            GameplayEntity,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(14.0),
                right: Val::Px(14.0),
                width: Val::Px(BOX),
                height: Val::Px(BOX),
                border: UiRect::all(Val::Px(1.5)),
                ..default()
            },
            BackgroundColor(theme.ui.panel_bg),
            BorderColor(theme.ui.panel_border),
            BorderRadius::all(Val::Px(12.0)),
        ))
        .with_children(|ring| {
            let step = std::f32::consts::TAU / (base_count as f32 + 1.0);
            for base in 0..base_count {
                let angle = -std::f32::consts::FRAC_PI_2 + step * (base as f32 + 1.0);
                let left = BOX / 2.0 + RADIUS * angle.cos() - PIP / 2.0;
                let top = BOX / 2.0 - RADIUS * angle.sin() - PIP / 2.0;
                ring.spawn((
                    BaseIndicator(base),
                    Node {
                        position_type: PositionType::Absolute,
                        top: Val::Px(top),
                        left: Val::Px(left),
                        width: Val::Px(PIP),
                        height: Val::Px(PIP),
                        ..default()
                    },
                    BackgroundColor(theme.ui.pip_off),
                    BorderRadius::all(Val::Px(4.0)),
                    // Rotate 45° so the square reads as a base.
                    Transform::from_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_4)),
                ));
            }
        });
}

// ── Update systems ────────────────────────────────────────────────────────────

fn update_inning_text(score: Res<ScoreBoard>, mut query: Query<&mut Text, With<InningText>>) {
    if !score.is_changed() {
        return;
    }
    let half = if score.top_of_inning { "TOP" } else { "BOT" };
    for mut text in &mut query {
        **text = format!("{half} {}", score.inning);
    }
}

fn update_score_text(score: Res<ScoreBoard>, mut query: Query<&mut Text, With<ScoreText>>) {
    if !score.is_changed() {
        return;
    }
    for mut text in &mut query {
        **text = format!("AWAY {}   HOME {}", score.away_runs, score.home_runs);
    }
}

fn update_count_dots(
    score: Res<ScoreBoard>,
    theme: Res<Theme>,
    mut query: Query<(&CountDot, &mut BackgroundColor)>,
) {
    if !score.is_changed() {
        return;
    }
    for (dot, mut color) in &mut query {
        let (value, on_color) = match dot.kind {
            CountKind::Ball => (score.balls, theme.ui.count_ball),
            CountKind::Strike => (score.strikes, theme.ui.count_strike),
            CountKind::Out => (score.outs, theme.ui.count_out),
        };
        color.0 = if dot.index < value {
            on_color
        } else {
            theme.ui.pip_off
        };
    }
}

fn update_base_ring(
    bases: Res<Bases>,
    theme: Res<Theme>,
    mut query: Query<(&BaseIndicator, &mut BackgroundColor)>,
) {
    if !bases.is_changed() {
        return;
    }
    for (indicator, mut color) in &mut query {
        color.0 = if bases.is_occupied(indicator.0) {
            theme.ui.accent
        } else {
            theme.ui.pip_off
        };
    }
}

fn show_banner(
    mut events: EventReader<PlayBanner>,
    theme: Res<Theme>,
    mut timer: ResMut<BannerTimer>,
    mut pill_q: Query<&mut Visibility, With<BannerPill>>,
    mut text_q: Query<(&mut Text, &mut TextColor), With<BannerText>>,
) {
    // Show only the latest banner this frame.
    if let Some(banner) = events.read().last() {
        let tone_color = match banner.tone {
            BannerTone::Good => theme.ui.tone_good,
            BannerTone::Bad => theme.ui.tone_bad,
            BannerTone::Info => theme.ui.tone_info,
            BannerTone::Epic => theme.ui.tone_epic,
        };
        for (mut text, mut color) in &mut text_q {
            **text = banner.text.clone();
            color.0 = tone_color;
        }
        for mut visibility in &mut pill_q {
            *visibility = Visibility::Inherited;
        }
        timer.0 = Timer::from_seconds(1.6, TimerMode::Once);
    }
}

fn fade_banner(
    time: Res<Time>,
    mut timer: ResMut<BannerTimer>,
    mut pill_q: Query<&mut Visibility, With<BannerPill>>,
) {
    if timer.0.finished() {
        return;
    }
    if timer.0.tick(time.delta()).just_finished() {
        for mut visibility in &mut pill_q {
            *visibility = Visibility::Hidden;
        }
    }
}
