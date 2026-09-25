//! The duel cards (batter/pitcher corners), the play-result banner pill, and
//! the contact-quality stamp — the HUD's event-driven, phase-gated read-outs.

use bevy::prelude::*;

use crate::game::flow::{BannerTone, ContactEvent, Phase, Play, PlayBanner, ResultBeat};
use crate::game::roster::Rosters;
use crate::game::rules::{BattingOrder, ContactQuality, LINEUP_SIZE};
use crate::game::theme::Theme;
use crate::game::variant::Ruleset;
use crate::game::{GameplayEntity, ScoreBoard, Team};

use super::{
    BannerPill, BannerText, ContactStampText, DuelLine, DuelLineKind, DuelPanel, PitchSpeedText,
    hidden_tint,
};

/// Metres per second → miles per hour, for the pitch-speed read-out.
const MPH_PER_MPS: f32 = 2.236_94;

/// How long a banner that is *not* a result pause's announcement (a wall
/// bang mid-play, "BACK IN TIME" on a pickoff) stays up. A result pause's
/// banner lives exactly as long as its beat instead (see [`beat_secs`]).
const FREE_BANNER_SECS: f32 = 1.6;

/// Seconds into a scoring play's beat before the banner swaps to the score
/// line (the reference holds SAFE! about a second, then the score).
const FOLLOW_UP_SECS: f32 = 1.0;

/// The banner's pending follow-up line (a scoring play's new score) and
/// when (in `Time::elapsed_secs_f64` terms) it takes over the pill. A
/// deadline, not a timer — see [`BannerFadeAt`].
#[derive(Resource, Default)]
pub(super) struct BannerFollowUp(Option<(String, f64)>);

/// Swaps the pill's text to the follow-up line once its time comes; the
/// pill's own deadline still decides when the whole thing clears.
pub(super) fn swap_banner_follow_up(
    time: Res<Time>,
    mut follow_up: ResMut<BannerFollowUp>,
    mut text_q: Query<&mut Text, With<BannerText>>,
) {
    let now = time.elapsed_secs_f64();
    if !follow_up.0.as_ref().is_some_and(|(_, at)| now >= *at) {
        return;
    }
    let Some((line, _)) = follow_up.0.take() else {
        return;
    };
    for mut text in &mut text_q {
        **text = line.clone();
    }
}

/// When (in `Time::elapsed_secs_f64` terms) the banner pill should clear;
/// `None` while nothing is showing.
///
/// A deadline, deliberately not a ticking `Timer`: on wasm/WebGL2 (Bevy
/// 0.16/0.17) a system that ticks a `ResMut` timer every frame while also
/// holding the pill's queries kept the pill from ever rendering — ECS said
/// visible, the screen stayed empty (bisected build-by-build 2026-08-25,
/// TODO 29; the same system with an untouched body was harmless). The fade
/// systems therefore only *read* until the deadline passes, and take their
/// one mutable step when it does. [`BannerFollowUp`], [`StampFadeAt`] and
/// [`SpeedFadeAt`] follow the same rule.
#[derive(Resource, Default)]
pub(super) struct BannerFadeAt(Option<f64>);

/// When the contact stamp should clear; `None` while nothing is showing.
#[derive(Resource, Default)]
pub(super) struct StampFadeAt(Option<f64>);

/// When the pitch-speed read-out should clear; `None` while nothing is
/// showing.
#[derive(Resource, Default)]
pub(super) struct SpeedFadeAt(Option<f64>);

/// Whether a deadline has come — `false` while nothing is showing.
fn due(fade_at: Option<f64>, now: f64) -> bool {
    fade_at.is_some_and(|at| now >= at)
}

/// The deadline a read-out shown right now should get: for the length of
/// the result pause when one is in progress — however long it runs, runners
/// settling included; [`clear_read_outs_on_result_exit`] blanks it the frame
/// the pause ends, so the text vanishes with the beat (under the curtain,
/// for beats that dip), never before, never after — else `free` seconds
/// from `now`.
fn fade_deadline(play: &Play, now: f64, free: f32) -> f64 {
    if play.phase == Phase::Result {
        HELD_FOR_THE_BEAT
    } else {
        now + f64::from(free)
    }
}

/// "Until the beat ends": a deadline no clock reaches, cut short by
/// [`clear_read_outs_on_result_exit`].
const HELD_FOR_THE_BEAT: f64 = f64::INFINITY;

/// Blanks every read-out raised for a result pause — banner, timing stamp,
/// speed — the frame the pause ends, and drops their deadlines.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn clear_read_outs_on_result_exit(
    play: Res<Play>,
    mut prev_phase: Local<Option<Phase>>,
    mut banner_fade: ResMut<BannerFadeAt>,
    mut stamp_fade: ResMut<StampFadeAt>,
    mut speed_fade: ResMut<SpeedFadeAt>,
    mut follow_up: ResMut<BannerFollowUp>,
    mut pill_q: Query<(&mut BackgroundColor, &mut BorderColor), With<BannerPill>>,
    mut banner_q: Query<&mut Text, (With<BannerText>, Without<ContactStampText>)>,
    mut stamp_q: Query<&mut Text, (With<ContactStampText>, Without<BannerText>)>,
    mut speed_q: Query<
        &mut Text,
        (
            With<PitchSpeedText>,
            Without<BannerText>,
            Without<ContactStampText>,
        ),
    >,
) {
    let left_result = *prev_phase == Some(Phase::Result) && play.phase != Phase::Result;
    *prev_phase = Some(play.phase);
    if !left_result {
        return;
    }
    banner_fade.0 = None;
    stamp_fade.0 = None;
    speed_fade.0 = None;
    follow_up.0 = None;
    for (mut bg, mut border) in &mut pill_q {
        bg.0 = hidden_tint(bg.0);
        *border = BorderColor::all(hidden_tint(border.top));
    }
    for mut text in banner_q
        .iter_mut()
        .chain(stamp_q.iter_mut())
        .chain(speed_q.iter_mut())
    {
        **text = String::new();
    }
}

/// How long the contact stamp (Task B4) stays up before clearing — quick
/// enough to read as a reaction to *this* swing, gone well before the next.
const CONTACT_STAMP_SECS: f32 = 0.8;

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
            // Fixed, not min: a text-driven width made the card's edge jump
            // every at-bat as names changed (TODO 74). Wide enough for the
            // longest legend line and the clamped batter line below.
            width: Val::Px(214.0),
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
                BorderColor::all(ui.panel_border),
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

/// Fills the duel cards during the pitch duel and hides them once the ball
/// is in play. Hiding flips root `Visibility` (the subtree skips rendering
/// entirely — the mechanism the pause board relies on) *and* still keeps
/// every colour's alpha nonzero: on wasm/WebGL2 the tint-and-blank idiom
/// alone left a dim ghost of the painted card floating over the sky
/// (playtest 2026-08-20, TODO 2) — stale glyphs/chrome kept rendering after
/// the mutation. The roots spawn visible, so the alpha-0-at-first-extract
/// cull never applies to them.
pub(super) fn update_duel_panels(
    play: Res<Play>,
    score: Res<ScoreBoard>,
    order: Res<BattingOrder>,
    rosters: Res<Rosters>,
    theme: Res<Theme>,
    mut panels: Query<(&mut BackgroundColor, &mut BorderColor, &mut Visibility), With<DuelPanel>>,
    mut lines: Query<(&DuelLine, &mut Text, &mut TextColor)>,
) {
    // Hidden through the walk-up too: its card is the only chrome then.
    let visible = play.phase.pre_contact() && !play.walkup_active();
    let ui = &theme.ui;
    for (mut bg, mut border, mut visibility) in &mut panels {
        let desired = if visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != desired {
            *visibility = desired;
        }
        if visible {
            bg.0 = ui.panel_bg;
            *border = BorderColor::all(ui.panel_border);
        } else {
            bg.0 = hidden_tint(ui.panel_bg);
            *border = BorderColor::all(hidden_tint(ui.panel_border));
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
                        // Clamped so a long creator-authored name can't
                        // outgrow the fixed card (TODO 74).
                        card.name.chars().take(9).collect::<String>(),
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
#[allow(clippy::too_many_arguments)]
pub(super) fn show_banner(
    mut events: MessageReader<PlayBanner>,
    theme: Res<Theme>,
    play: Res<Play>,
    time: Res<Time>,
    mut fade_at: ResMut<BannerFadeAt>,
    mut follow_up: ResMut<BannerFollowUp>,
    mut pill_q: Query<(&mut BackgroundColor, &mut BorderColor), With<BannerPill>>,
    mut text_q: Query<(&mut Text, &mut TextColor), With<BannerText>>,
) {
    // Show only the latest banner this frame.
    let Some(banner) = events.read().last() else {
        return;
    };
    let now = time.elapsed_secs_f64();
    follow_up.0 = banner
        .follow_up
        .clone()
        .map(|line| (line, now + f64::from(FOLLOW_UP_SECS)));
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
        *border = BorderColor::all(ui.panel_border);
    }
    fade_at.0 = Some(fade_deadline(&play, now, FREE_BANNER_SECS));
}

/// Clears the pill once its deadline passes. Reads only until then (see
/// [`BannerFadeAt`] for why this must not tick).
pub(super) fn fade_banner(
    time: Res<Time>,
    mut fade_at: ResMut<BannerFadeAt>,
    mut pill_q: Query<(&mut BackgroundColor, &mut BorderColor), With<BannerPill>>,
    mut text_q: Query<(&mut Text, &mut TextColor), With<BannerText>>,
) {
    if !due(fade_at.0, time.elapsed_secs_f64()) {
        return;
    }
    fade_at.0 = None;
    for (mut bg, mut border) in &mut pill_q {
        bg.0 = hidden_tint(bg.0);
        *border = BorderColor::all(hidden_tint(border.top));
    }
    for (mut text, _color) in &mut text_q {
        **text = String::new();
    }
}

/// Stamps the graded swing timing over the zone-box area: `PERFECT!` for
/// dead-on contact; `EARLY`/`LATE` by `dt_ms`'s sign for `Solid` (and the
/// as-yet-unreachable `Weak`, per its doc comment in `game::rules`) **and
/// for a `Whiff`** — the swing the batter most needs timing feedback on
/// (TODO 101; the reference shows LATE under STRIKE on a miss); `FOUL TIP`
/// for a foul. A stamp raised during a result pause lives as long as the
/// beat, one raised at contact for its own short window.
pub(super) fn show_contact_stamp(
    mut events: MessageReader<ContactEvent>,
    theme: Res<Theme>,
    play: Res<Play>,
    time: Res<Time>,
    mut fade_at: ResMut<StampFadeAt>,
    mut text_q: Query<(&mut Text, &mut TextColor), With<ContactStampText>>,
) {
    let Some(ev) = events.read().last() else {
        return;
    };
    let ui = &theme.ui;
    let early_late = |tone| {
        let label = if ev.dt_ms < 0.0 { "EARLY" } else { "LATE" };
        (label, tone)
    };
    let (label, color) = match ev.quality {
        ContactQuality::Perfect => ("PERFECT!", ui.tone_epic),
        ContactQuality::Solid | ContactQuality::Weak => early_late(ui.tone_info),
        ContactQuality::Whiff => early_late(ui.tone_bad),
        ContactQuality::FoulTip => ("FOUL TIP", ui.tone_info),
    };
    for (mut text, mut text_color) in &mut text_q {
        **text = label.to_string();
        text_color.0 = color;
    }
    fade_at.0 = Some(fade_deadline(
        &play,
        time.elapsed_secs_f64(),
        CONTACT_STAMP_SECS,
    ));
}

/// How long the pitch-speed read-out stays up when raised outside a result
/// pause (it never is today — every pitch beat is a pause — but the
/// deadline needs a length for the free case).
const PITCH_SPEED_SECS: f32 = 1.0;

/// Shows the just-judged pitch's release speed ("97 MPH") by the plate for
/// the length of its beat — the read-out the reference pins under every
/// strike and strikeout (TODO 101). Raised on the frame a result pause
/// begins for any pitch beat (not a batted ball's pause: the camera has
/// left the plate by then); the speed is the kind's release speed under the
/// pace dial, the same number the pitch was thrown at.
pub(super) fn show_pitch_speed(
    play: Res<Play>,
    rules: Res<Ruleset>,
    theme: Res<Theme>,
    time: Res<Time>,
    mut prev_phase: Local<Option<Phase>>,
    mut fade_at: ResMut<SpeedFadeAt>,
    mut text_q: Query<(&mut Text, &mut TextColor), With<PitchSpeedText>>,
) {
    let entered_result = play.phase == Phase::Result && *prev_phase != Some(Phase::Result);
    *prev_phase = Some(play.phase);
    if !entered_result {
        return;
    }
    let pitch_beat = matches!(
        play.result_beat(),
        Some(
            ResultBeat::Ball
                | ResultBeat::Strike
                | ResultBeat::Foul
                | ResultBeat::Strikeout
                | ResultBeat::Walk
        )
    );
    let Some(kind) = play.pitch_kind().filter(|_| pitch_beat) else {
        return;
    };
    let mph = kind.speed() * rules.pace.pitch_speed_scale * MPH_PER_MPS;
    for (mut text, mut color) in &mut text_q {
        **text = format!("{} MPH", mph.round() as i32);
        color.0 = theme.ui.text_primary;
    }
    fade_at.0 = Some(fade_deadline(
        &play,
        time.elapsed_secs_f64(),
        PITCH_SPEED_SECS,
    ));
}

/// Blanks the pitch-speed read-out once its deadline passes (deadline-
/// driven, never ticking — see [`BannerFadeAt`]).
pub(super) fn fade_pitch_speed(
    time: Res<Time>,
    mut fade_at: ResMut<SpeedFadeAt>,
    mut text_q: Query<&mut Text, With<PitchSpeedText>>,
) {
    if !due(fade_at.0, time.elapsed_secs_f64()) {
        return;
    }
    fade_at.0 = None;
    for mut text in &mut text_q {
        **text = String::new();
    }
}

/// Blanks the contact stamp once its deadline passes (deadline-driven,
/// never ticking — see [`BannerFadeAt`]).
pub(super) fn fade_contact_stamp(
    time: Res<Time>,
    mut fade_at: ResMut<StampFadeAt>,
    mut text_q: Query<&mut Text, With<ContactStampText>>,
) {
    if !due(fade_at.0, time.elapsed_secs_f64()) {
        return;
    }
    fade_at.0 = None;
    for mut text in &mut text_q {
        **text = String::new();
    }
}
