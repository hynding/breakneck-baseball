//! Heads-up display — scoreboard card, count dots, base ring, and banners.
//!
//! All live game data comes from the [`ScoreBoard`] and [`Bases`] resources
//! and from [`PlayBanner`] events fired by `flow.rs`. Every colour and
//! styling knob comes from the active [`Theme`] — the HUD owns layout only.

use bevy::prelude::*;

use crate::game::flow::{BannerTone, ContactEvent, Phase, Play, PlayBanner};
use crate::game::roster::Rosters;
use crate::game::rules::{Bases, BattingOrder, ContactQuality, LINEUP_SIZE};
use crate::game::theme::Theme;
use crate::game::variant::{FieldSpec, Ruleset};
use crate::game::{GameState, GameplayEntity, ScoreBoard, Team};

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

/// The banner pill chrome (persistent; painted/cleared via child mutations).
#[derive(Component)]
struct BannerPill;

/// The Swing Meter's load bar: a slim vertical track beside the count HUD. The
/// [`MeterFill`] child's height follows the batting team's meter load; the
/// track shell stays painted (dim) per the wasm UI rule — never despawned.
#[derive(Component)]
struct MeterFill;

/// The banner text inside the pill.
#[derive(Component)]
struct BannerText;

/// The contact-quality stamp (PERFECT! / EARLY / LATE / FOUL TIP), painted at
/// spawn near the zone-box screen area and shown by text mutation only — see
/// the wasm UI rule on [`hidden_tint`]. Public so e2e tests can query its
/// `Text` content directly (the same pattern `player::CatcherRole` uses for
/// `e2e_camera_views`'s `Visibility` check).
#[derive(Component)]
pub struct ContactStampText;

/// Root of one of the two duel cards flanking the catcher's-eye pitch view.
#[derive(Component)]
struct DuelPanel;

/// One line of a duel card, updated (and shown/hidden) by phase.
#[derive(Component)]
struct DuelLine(DuelLineKind);

#[derive(Clone, Copy, PartialEq, Eq)]
enum DuelLineKind {
    BatterTitle,
    BatterTeam,
    BatterSlot,
    BatterRuns,
    PitcherTitle,
    PitcherTeam,
    LegendFast,
    LegendChange,
    LegendCurve,
    LegendSlider,
    LegendSinker,
}

/// A colour reduced to near-invisibility. Never fully transparent: on the
/// wasm target an element extracted with alpha 0 is culled for good.
pub(crate) fn hidden_tint(color: Color) -> Color {
    color.with_alpha(0.004)
}

/// How long the current banner stays visible before clearing.
#[derive(Resource)]
struct BannerTimer(Timer);

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
struct ContactStampTimer(Timer);

impl Default for ContactStampTimer {
    fn default() -> Self {
        Self(Timer::from_seconds(CONTACT_STAMP_SECS, TimerMode::Once))
    }
}

// ── Plugin ────────────────────────────────────────────────────────────────────

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BannerTimer>()
            .init_resource::<ContactStampTimer>()
            .add_systems(crate::game::game_start(), spawn_hud)
            .add_systems(
                Update,
                (
                    update_inning_text,
                    update_score_text,
                    update_count_dots,
                    update_meter_bar,
                    update_base_ring,
                    update_duel_panels,
                    show_banner,
                    fade_banner,
                    show_contact_stamp,
                    fade_contact_stamp,
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
    spawn_duel_panels(&mut commands, &theme);

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

/// The two cards anchored to the bottom-left (batter/"AT BAT") and top-right
/// (pitcher/"PITCHING", with the pitch-selection legend) corners — visible
/// only during the pitch duel, hidden while the ball is in play. Corners are
/// fixed regardless of which team is batting or fielding.
///
/// Both roots are painted at spawn and shown/hidden by mutating colours and
/// text (never alpha 0 / despawn): on wasm/WebGL2 an element extracted fully
/// transparent is culled for good.
fn spawn_duel_panels(commands: &mut Commands, theme: &Theme) {
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
fn update_duel_panels(
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

fn update_inning_text(
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

/// Drives the Swing Meter fill height from the batting team's live load. Height
/// 0 is the hidden state (Classic/PCI, or between holds); the shell stays
/// painted so the element is never a fresh mid-`Playing` root (wasm UI rule).
fn update_meter_bar(
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

/// Paints the pill and its text for the latest banner event.
fn show_banner(
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
fn fade_banner(
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
fn show_contact_stamp(
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
fn fade_contact_stamp(
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
