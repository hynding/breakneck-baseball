//! The duel cards (batter/pitcher corners), the play-result banner pill, and
//! the contact-quality stamp — the HUD's event-driven, phase-gated read-outs.

use bevy::prelude::*;

use crate::game::flow::{BannerTone, ContactEvent, Phase, Play, PlayBanner};
use crate::game::roster::Rosters;
use crate::game::rules::{BattingOrder, ContactQuality, LINEUP_SIZE};
use crate::game::theme::Theme;
use crate::game::{GameplayEntity, ScoreBoard, Team};

use super::{
    BannerPill, BannerText, ContactStampText, DuelLine, DuelLineKind, DuelPanel, hidden_tint,
};

/// How long the current banner stays visible before clearing.
#[derive(Resource)]
pub(super) struct BannerTimer(Timer);

impl Default for BannerTimer {
    fn default() -> Self {
        Self(Timer::from_seconds(1.6, TimerMode::Once))
    }
}

/// How long the contact stamp (Task B4) stays up before clearing — quick
/// enough to read as a reaction to *this* swing, gone well before the next.
const CONTACT_STAMP_SECS: f32 = 0.8;

/// How long the current contact stamp stays visible before clearing.
#[derive(Resource)]
pub(super) struct ContactStampTimer(Timer);

impl Default for ContactStampTimer {
    fn default() -> Self {
        Self(Timer::from_seconds(CONTACT_STAMP_SECS, TimerMode::Once))
    }
}

/// The two cards anchored to the bottom-left (batter/"AT BAT") and top-right
/// (pitcher/"PITCHING", with the pitch-selection legend) corners — visible
/// only during the pitch duel, hidden while the ball is in play. Corners are
/// fixed regardless of which team is batting or fielding.
///
/// Both roots are painted at spawn and shown/hidden by mutating colours and
/// text (never alpha 0 / despawn): on wasm/WebGL2 an element extracted fully
/// transparent is culled for good.
pub(super) fn spawn_duel_panels(commands: &mut Commands, theme: &Theme) {
    let ui = &theme.ui;
    let lines: [(&[DuelLineKind], f32); 2] = [
        (
            &[
                DuelLineKind::BatterTitle,
                DuelLineKind::BatterTeam,
                DuelLineKind::BatterSlot,
                DuelLineKind::BatterRuns,
            ],
            14.0,
        ),
        (
            &[
                DuelLineKind::PitcherTitle,
                DuelLineKind::PitcherTeam,
                DuelLineKind::LegendFast,
                DuelLineKind::LegendChange,
                DuelLineKind::LegendCurve,
                DuelLineKind::LegendSlider,
                DuelLineKind::LegendSinker,
            ],
            14.0,
        ),
    ];

    for (side, (kinds, _)) in lines.into_iter().enumerate() {
        let mut node = Node {
            position_type: PositionType::Absolute,
            padding: UiRect::axes(Val::Px(14.0), Val::Px(12.0)),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(5.0),
            border: UiRect::all(Val::Px(1.5)),
            min_width: Val::Px(150.0),
            ..default()
        };
        if side == 0 {
            // Batter/"AT BAT" card: bottom-left corner.
            node.bottom = Val::Px(14.0);
            node.left = Val::Px(14.0);
        } else {
            // Pitcher/"PITCHING" card: top-right corner.
            node.top = Val::Px(14.0);
            node.right = Val::Px(14.0);
        }
        commands
            .spawn((
                DuelPanel,
                GameplayEntity,
                node,
                BackgroundColor(ui.panel_bg),
                BorderColor(ui.panel_border),
                BorderRadius::all(Val::Px(12.0)),
            ))
            .with_children(|card| {
                for kind in kinds {
                    let (size, color) = match kind {
                        DuelLineKind::BatterTitle | DuelLineKind::PitcherTitle => (13.0, ui.accent),
                        DuelLineKind::BatterTeam | DuelLineKind::PitcherTeam => {
                            (22.0, ui.text_primary)
                        }
                        _ => (14.0, ui.text_dim),
                    };
                    card.spawn((
                        DuelLine(*kind),
                        Text::new(""),
                        TextFont {
                            font_size: size,
                            ..default()
                        },
                        TextColor(color),
                    ));
                }
            });
    }
}

/// Fills the duel cards during the pitch duel and blanks them (keeping every
/// alpha nonzero for wasm) once the ball is in play.
pub(super) fn update_duel_panels(
    play: Res<Play>,
    score: Res<ScoreBoard>,
    order: Res<BattingOrder>,
    rosters: Res<Rosters>,
    theme: Res<Theme>,
    mut panels: Query<(&mut BackgroundColor, &mut BorderColor), With<DuelPanel>>,
    mut lines: Query<(&DuelLine, &mut Text, &mut TextColor)>,
) {
    let visible = matches!(play.phase, Phase::PrePitch | Phase::WindUp | Phase::Pitch);
    let ui = &theme.ui;
    for (mut bg, mut border) in &mut panels {
        if visible {
            bg.0 = ui.panel_bg;
            border.0 = ui.panel_border;
        } else {
            bg.0 = hidden_tint(ui.panel_bg);
            border.0 = hidden_tint(ui.panel_border);
        }
    }

    let team_label = |team: Team| team.label();
    let batting = score.batting_team();
    let batting_runs = match batting {
        Team::Home => score.home_runs,
        Team::Away => score.away_runs,
    };
    for (line, mut text, mut color) in &mut lines {
        if !visible {
            **text = String::new();
            continue;
        }
        let (value, tint) = match line.0 {
            DuelLineKind::BatterTitle => ("AT BAT".to_string(), ui.accent),
            DuelLineKind::BatterTeam => (team_label(batting).to_string(), ui.text_primary),
            DuelLineKind::BatterSlot => {
                let card = rosters.team(batting).batting(order.current(batting));
                (
                    format!(
                        "AB {}/{}  {} #{}",
                        order.current(batting),
                        LINEUP_SIZE,
                        card.name,
                        card.number
                    ),
                    ui.text_dim,
                )
            }
            DuelLineKind::BatterRuns => (format!("RUNS {batting_runs}"), ui.text_dim),
            DuelLineKind::PitcherTitle => ("PITCHING".to_string(), ui.accent),
            DuelLineKind::PitcherTeam => (
                team_label(score.fielding_team()).to_string(),
                ui.text_primary,
            ),
            DuelLineKind::LegendFast => ("AIM UP:    FASTBALL".to_string(), ui.text_dim),
            DuelLineKind::LegendChange => ("NEUTRAL:   CHANGEUP".to_string(), ui.text_dim),
            DuelLineKind::LegendCurve => ("AIM DOWN:  CURVEBALL".to_string(), ui.text_dim),
            DuelLineKind::LegendSlider => ("AIM LEFT:  SLIDER".to_string(), ui.text_dim),
            DuelLineKind::LegendSinker => ("AIM RIGHT: SINKER".to_string(), ui.text_dim),
        };
        **text = value;
        color.0 = tint;
    }
}

/// Paints the pill and its text for the latest banner event.
pub(super) fn show_banner(
    mut events: EventReader<PlayBanner>,
    theme: Res<Theme>,
    mut timer: ResMut<BannerTimer>,
    mut pill_q: Query<(&mut BackgroundColor, &mut BorderColor), With<BannerPill>>,
    mut text_q: Query<(&mut Text, &mut TextColor), With<BannerText>>,
) {
    // Show only the latest banner this frame.
    let Some(banner) = events.read().last() else {
        return;
    };
    let ui = &theme.ui;
    let tone_color = match banner.tone {
        BannerTone::Good => ui.tone_good,
        BannerTone::Bad => ui.tone_bad,
        BannerTone::Info => ui.tone_info,
        BannerTone::Epic => ui.tone_epic,
    };
    for (mut text, mut color) in &mut text_q {
        **text = banner.text.clone();
        color.0 = tone_color;
    }
    for (mut bg, mut border) in &mut pill_q {
        bg.0 = ui.panel_bg;
        border.0 = ui.panel_border;
    }
    timer.0 = Timer::from_seconds(1.6, TimerMode::Once);
}

/// Clears the pill once its display time is up.
pub(super) fn fade_banner(
    time: Res<Time>,
    mut timer: ResMut<BannerTimer>,
    mut pill_q: Query<(&mut BackgroundColor, &mut BorderColor), With<BannerPill>>,
    mut text_q: Query<(&mut Text, &mut TextColor), With<BannerText>>,
) {
    if timer.0.finished() {
        return;
    }
    if timer.0.tick(time.delta()).just_finished() {
        for (mut bg, mut border) in &mut pill_q {
            bg.0 = hidden_tint(bg.0);
            border.0 = hidden_tint(border.0);
        }
        for (mut text, _color) in &mut text_q {
            **text = String::new();
        }
    }
}

/// Stamps the graded swing timing over the zone-box area: `PERFECT!` for
/// dead-on contact; `EARLY`/`LATE` for `Solid` (and the as-yet-unreachable
/// `Weak`, per its doc comment in `rules.rs`) by `dt_ms`'s sign; `FOUL TIP`
/// for a foul; nothing for `Whiff` — the classic strike/ball banner already
/// covers a swing-and-miss.
pub(super) fn show_contact_stamp(
    mut events: EventReader<ContactEvent>,
    theme: Res<Theme>,
    mut timer: ResMut<ContactStampTimer>,
    mut text_q: Query<(&mut Text, &mut TextColor), With<ContactStampText>>,
) {
    let Some(ev) = events.read().last() else {
        return;
    };
    let ui = &theme.ui;
    let stamp = match ev.quality {
        ContactQuality::Perfect => Some(("PERFECT!", ui.tone_epic)),
        ContactQuality::Solid | ContactQuality::Weak => {
            let label = if ev.dt_ms < 0.0 { "EARLY" } else { "LATE" };
            Some((label, ui.tone_info))
        }
        ContactQuality::FoulTip => Some(("FOUL TIP", ui.tone_info)),
        ContactQuality::Whiff => None,
    };
    let Some((label, color)) = stamp else {
        return;
    };
    for (mut text, mut text_color) in &mut text_q {
        **text = label.to_string();
        text_color.0 = color;
    }
    timer.0 = Timer::from_seconds(CONTACT_STAMP_SECS, TimerMode::Once);
}

/// Blanks the contact stamp once its display time is up.
pub(super) fn fade_contact_stamp(
    time: Res<Time>,
    mut timer: ResMut<ContactStampTimer>,
    mut text_q: Query<&mut Text, With<ContactStampText>>,
) {
    if timer.0.finished() {
        return;
    }
    if timer.0.tick(time.delta()).just_finished() {
        for mut text in &mut text_q {
            **text = String::new();
        }
    }
}
