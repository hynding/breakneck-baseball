//! The scoreboard card, Swing Meter load bar, and base ring — the HUD's
//! always-on read-outs, spawned once at game start and kept live off
//! [`ScoreBoard`]/[`Bases`]/[`BattingOrder`].

use bevy::prelude::*;

use crate::game::rules::{Bases, BattingOrder, LINEUP_SIZE};
use crate::game::theme::Theme;
use crate::game::variant::{FieldSpec, Ruleset};
use crate::game::{GameplayEntity, ScoreBoard};

use super::{
    BannerPill, BannerText, BaseIndicator, ContactStampText, CountDot, CountKind, InningText,
    MeterFill, ScoreText, hidden_tint,
};

// ── Build the UI tree ─────────────────────────────────────────────────────────

pub(super) fn spawn_hud(
    mut commands: Commands,
    field: Res<FieldSpec>,
    rules: Res<Ruleset>,
    theme: Res<Theme>,
) {
    let ui = &theme.ui;

    // Scoreboard card (bottom-right).
    commands
        .spawn((
            GameplayEntity,
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(14.0),
                right: Val::Px(14.0),
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
                    (CountKind::Ball, "B", rules.counts.balls_per_walk - 1),
                    (CountKind::Strike, "S", rules.counts.strikes_per_out - 1),
                    (CountKind::Out, "O", rules.counts.outs_per_half - 1),
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

    // Swing Meter load bar: a slim vertical track to the left of the
    // scoreboard card. Painted (dim shell, accent fill) at spawn per the wasm
    // UI rule; the fill's height is driven each frame from `MeterLoad`, and a
    // zero-height fill is the hidden state (never despawned).
    commands
        .spawn((
            GameplayEntity,
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(14.0),
                right: Val::Px(210.0),
                width: Val::Px(14.0),
                height: Val::Px(120.0),
                border: UiRect::all(Val::Px(1.5)),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::FlexEnd, // fill grows from the base
                ..default()
            },
            BackgroundColor(hidden_tint(ui.panel_bg)),
            BorderColor(hidden_tint(ui.panel_border)),
            BorderRadius::all(Val::Px(6.0)),
        ))
        .with_children(|track| {
            track.spawn((
                MeterFill,
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(0.0),
                    ..default()
                },
                BackgroundColor(ui.accent),
                BorderRadius::all(Val::Px(5.0)),
            ));
        });

    spawn_base_ring(&mut commands, field.base_count(), &theme);
    super::banner::spawn_duel_panels(&mut commands, &theme);

    // Banner: persistent wrapper root + pill child + text grandchild.
    // wasm/WebGL2 dictates the structure: an element that is fully
    // transparent (or has no renderable at all) when first extracted is
    // never rendered again, even after its colors change. So every banner
    // element keeps a nonzero alpha at all times — "hidden" is a near-zero
    // alpha and an empty string, and show/fade only mutate children of this
    // painted root.
    commands
        .spawn((
            GameplayEntity,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Percent(26.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.01)),
        ))
        .with_children(|wrap| {
            wrap.spawn((
                BannerPill,
                Node {
                    padding: UiRect::axes(Val::Px(30.0), Val::Px(10.0)),
                    border: UiRect::all(Val::Px(1.5)),
                    ..default()
                },
                BackgroundColor(hidden_tint(ui.panel_bg)),
                BorderColor(hidden_tint(ui.panel_border)),
                BorderRadius::all(Val::Px(26.0)),
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

    // Contact stamp (Task B4): a bare text element (no pill chrome) sitting
    // just below the banner row, over the zone-box screen area the
    // catcher's-eye duel view frames the pitch in (`FieldSpec::duel_eye`).
    // Painted at spawn with an empty string — same wasm-safe idiom as the
    // banner above — then shown/blanked by mutating this one text node.
    commands
        .spawn((
            GameplayEntity,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Percent(38.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
            // A container root with no renderable is never re-extracted on
            // wasm/WebGL2 once the first frame culls it — a near-invisible
            // background (never alpha 0, see `hidden_tint`) keeps it live.
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.01)),
        ))
        .with_children(|wrap| {
            wrap.spawn((
                ContactStampText,
                Text::new(""),
                TextFont {
                    font_size: 34.0,
                    ..default()
                },
                TextColor(ui.text_primary),
            ));
        });
    // Controls help now lives in the pause dialog (see `subs.rs`) rather
    // than a bar pinned to the bottom of the screen during play.
}

/// A 96×96 px ring of base pips (top-left): one pip per base, laid out like
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
                left: Val::Px(14.0),
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

pub(super) fn update_inning_text(
    score: Res<ScoreBoard>,
    order: Res<BattingOrder>,
    mut query: Query<&mut Text, With<InningText>>,
) {
    if !score.is_changed() && !order.is_changed() {
        return;
    }
    let half = if score.top_of_inning { "TOP" } else { "BOT" };
    let slot = order.current(score.batting_team());
    for mut text in &mut query {
        **text = format!("{half} {}  AB {slot}/{LINEUP_SIZE}", score.inning);
    }
}

pub(super) fn update_score_text(
    score: Res<ScoreBoard>,
    mut query: Query<&mut Text, With<ScoreText>>,
) {
    if !score.is_changed() {
        return;
    }
    for mut text in &mut query {
        **text = format!("AWAY {}   HOME {}", score.away_runs, score.home_runs);
    }
}

pub(super) fn update_count_dots(
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

/// Drives the Swing Meter fill height from the batting team's live load. Height
/// 0 is the hidden state (Classic/PCI, or between holds); the shell stays
/// painted so the element is never a fresh mid-`Playing` root (wasm UI rule).
pub(super) fn update_meter_bar(
    load: Res<crate::game::batting::MeterLoad>,
    theme: Res<Theme>,
    mut query: Query<(&mut Node, &mut BackgroundColor), With<MeterFill>>,
) {
    let frac = load.0.clamp(0.0, 1.0);
    for (mut node, mut color) in &mut query {
        node.height = Val::Percent(frac * 100.0);
        color.0 = theme.ui.accent;
    }
}

pub(super) fn update_base_ring(
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
