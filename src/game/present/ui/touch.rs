//! On-screen chrome for the touch swing schemes: the Zone Pad's pad and
//! SWING button outlines, plus a one-line scheme hint.
//!
//! The hit-testing itself lives in `game::touch` — this module only *draws*
//! the same rectangles (`touch::pad_rect` / `touch::swing_button_rect`), so
//! the visuals and the input regions can never disagree. Spawned once at
//! game start and repainted by child mutation only (the wasm UI rule; see
//! `mod.rs`), with [`hidden_tint`] standing in for "off".

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::game::flow::Phase;
use crate::game::settings::{Settings, TouchScheme};
use crate::game::theme::Theme;
use crate::game::{GameplayEntity, ScoreBoard};

use super::hidden_tint;

/// Root of the touch chrome (full-screen, non-interactive).
#[derive(Component)]
pub(super) struct TouchOverlay;

/// Which chrome region an entity is — one kind component and one paint
/// query instead of a marker-per-element with an O(n²) `Without` wall; a
/// new region is a variant plus a match arm.
#[derive(Component, Clone, Copy, PartialEq)]
pub(super) enum TouchChrome {
    /// The Zone Pad's aiming rectangle.
    Pad,
    /// The Zone Pad's SWING button circle.
    SwingButton,
    /// The pause button (right edge) — the touch path to the pause board,
    /// which is otherwise Esc/P/Start only.
    PauseButton,
}

/// Which chrome text an entity is (same shape as [`TouchChrome`]).
#[derive(Component, Clone, Copy, PartialEq)]
pub(super) enum TouchLabel {
    /// The SWING button's label.
    Swing,
    /// The one-line scheme hint along the top of the screen.
    Hint,
    /// The pause button's label.
    Pause,
}

/// Marker for the pause Button's `Interaction` query (`tap_pause`).
#[derive(Component)]
pub(super) struct TouchPauseButton;

/// The hint line's container, moved by [`paint_touch_overlay`] for the
/// window's orientation.
#[derive(Component)]
pub(super) struct TouchHintRow;

/// Where the hint line sits in a portrait window, in UI pixels from the
/// top: below the duel card, which fills the top-right corner there and
/// leaves no room beside the minimap.
const PORTRAIT_HINT_TOP_PX: f32 = 205.0;
/// …and in landscape, the strip between the top corner cards.
const LANDSCAPE_HINT_TOP_PX: f32 = 6.0;

/// The overlay's last-painted appearance key, ON the overlay root — not in
/// system `Local`s, whose lifetime is the system's: caches that outlived
/// the between-games despawn/respawn shipped a never-repainted fresh tree
/// (TADA 64) and then needed an `Added<TouchOverlay>` repair query. A
/// component dies and re-defaults with the tree it describes, so the whole
/// staleness class is gone (`shown: None` can never match a real state).
#[derive(Component, Default)]
pub(super) struct LastPainted {
    size: Vec2,
    /// The [`UiScale`] the geometry was converted at (see [`paint_region`]).
    ui_scale: f32,
    shown: Option<(bool, bool)>,
    line: &'static str,
}

/// Where unpainted chrome parks: far enough off-screen to be unhittable at
/// any resolution, and overwritten by [`paint_region`] the moment a touch
/// reveals the chrome.
const OFFSCREEN_PX: f32 = -10_000.0;

/// Spawns the (hidden) touch chrome at game start.
pub(super) fn spawn_touch_overlay(mut commands: Commands, theme: Res<Theme>) {
    let ui = &theme.ui;
    // The ONE chrome-region bundle (the spawn-side sibling of
    // [`paint_region`]): these two colors carry the wasm invariant — an
    // element alpha-0 at first extract never renders again — so a fourth
    // region, or a change to the hidden-tint convention, cannot reach two
    // regions and miss the third. Same rationale as `subs::line_bundle`.
    // Callers add their own markers and geometry is set by the painter.
    let region = |radius: Val, centered: bool| {
        (
            Node {
                position_type: PositionType::Absolute,
                // Parked off-screen until the painter places it. With
                // `Auto` insets an absolute node falls back to its static
                // position — the overlay's top-left corner — so on a
                // machine that never sees a touch (the painter is gated on
                // `touchscreen_seen`) the pause `Button` sat there
                // unpainted at tier 31, a tiny invisible click-blocker
                // above every other UI. Parked, it can block nothing, and
                // `paint_region` overwrites all four fields on show.
                left: Val::Px(OFFSCREEN_PX),
                top: Val::Px(OFFSCREEN_PX),
                border: UiRect::all(Val::Px(2.0)),
                align_items: if centered {
                    AlignItems::Center
                } else {
                    AlignItems::default()
                },
                justify_content: if centered {
                    JustifyContent::Center
                } else {
                    JustifyContent::default()
                },
                border_radius: BorderRadius::all(radius),
                ..default()
            },
            BackgroundColor(hidden_tint(ui.panel_bg)),
            BorderColor::all(hidden_tint(ui.accent)),
        )
    };
    commands
        .spawn((
            TouchOverlay,
            LastPainted::default(),
            GameplayEntity,
            super::KeepAliveUi,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            BackgroundColor(hidden_tint(ui.panel_bg)),
        ))
        .with_children(|root| {
            root.spawn((TouchChrome::Pad, region(Val::Px(12.0), false)));
            root.spawn((TouchChrome::SwingButton, region(Val::Percent(50.0), true)))
                .with_child((
                    TouchLabel::Swing,
                    Text::new(""),
                    TextFont {
                        font_size: 18.0,
                        ..default()
                    },
                    TextColor(ui.text_primary),
                ));
            root.spawn((
                // Marked like the settings card: a hidden-tinted container
                // no paint system ever re-touches (only its Text child is)
                // — the exact shape the wasm keep-alive rule covers.
                super::KeepAliveUi,
                TouchHintRow,
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(LANDSCAPE_HINT_TOP_PX),
                    left: Val::Percent(20.0),
                    width: Val::Percent(60.0),
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                BackgroundColor(hidden_tint(ui.panel_bg)),
            ))
            .with_child((
                TouchLabel::Hint,
                Text::new(""),
                TextFont {
                    font_size: 14.0,
                    ..default()
                },
                TextColor(ui.text_dim),
            ));
            root.spawn((
                TouchChrome::PauseButton,
                TouchPauseButton,
                Button,
                // One rung above the pause board (`subs::spawn_board`) —
                // see `ui::z` for why. Safe under the wasm z-index rule:
                // this node is painted with real colors on show, not a
                // transparent never-repainted keep-alive root.
                GlobalZIndex(super::z::TOUCH_CHROME),
                // Positioned by `paint_touch_overlay` from
                // `touch::pause_button_rect` — the same rect the input
                // translator excludes from gameplay hit-testing.
                region(Val::Px(12.0), true),
            ))
            .with_child((
                TouchLabel::Pause,
                Text::new(""),
                TextFont {
                    font_size: 18.0,
                    ..default()
                },
                TextColor(ui.text_primary),
            ));
        });
}

/// The hint line for the scheme, ownership, role, phase window, and whether
/// a touch has actually been seen; empty = hide. Owns the whole
/// shown-vs-hidden mapping so it has exactly one home. `Off` shows the
/// generic mapping only once a real touch proves a touchscreen exists —
/// desktop players never see it. Once the ball is in play the swing lines
/// give way to runner steering: the pad/button regions have released (see
/// `zone_pad_active`), and a hint still advertising them would send taps
/// into controls that no longer exist.
fn hint_line(
    scheme: TouchScheme,
    driven: bool,
    playing: bool,
    batting: bool,
    phase: Phase,
    seen: bool,
) -> &'static str {
    // Paused: the gameplay hint over the pause board would describe
    // controls the (Playing-gated) translator isn't serving. Owned HERE —
    // this function's contract is the whole shown-vs-hidden mapping.
    if !driven || !playing {
        return "";
    }
    // `Off` shows anything only once a real touch proves a touchscreen
    // exists — desktop players never see it. Past that gate, `Off` rides
    // the same phase ladder as every scheme: its generic stick steers
    // runners and the chaser during live play like theirs do, so it gets
    // the same live-ball lines and Result-pause quiet.
    if scheme == TouchScheme::Off && !seen {
        return "";
    }
    // ONE phase ladder for every scheme and both roles (the per-role copies
    // each got the same Result-pause fix in different cycles — the ladder
    // is the drift surface, so it exists once): live ball → steering text,
    // outside the duel window → quiet (deriving from `pre_contact()`, the
    // predicate the translator claims by, so a future phase falls silent
    // instead of falling through to a swing hint whose taps would steer
    // runners), the duel itself → the role's controls.
    if phase == Phase::InPlay {
        return if batting {
            "BALL IS LIVE - DRAG DOWN TO SEND RUNNERS, UP TO HOLD"
        } else {
            "BALL IS LIVE - DRAG STEERS THE CHASER - SECOND FINGER THROWS"
        };
    }
    if !phase.pre_contact() {
        return "";
    }
    if !batting {
        return "TOUCH: DRAG TO AIM - SECOND FINGER PITCHES / THROWS";
    }
    match scheme {
        // No "(G)" here: this line renders only during gameplay, where G
        // and the settings screen are inert (both are MainMenu-only) — an
        // instruction the player cannot follow. The menu's own
        // "G Touch swing" row advertises schemes where they're actionable.
        TouchScheme::Off => "TOUCH: DRAG = AIM - TAP = ACTION",
        TouchScheme::Tap => "TAP TO SWING - TAP POSITION AIMS",
        TouchScheme::Flick => "FLICK UP THROUGH THE BALL TO SWING",
        TouchScheme::HoldRelease => "HOLD TO LOAD - RELEASE TO SWING",
        TouchScheme::ZonePad => "LEFT PAD AIMS - RIGHT BUTTON SWINGS",
    }
}

/// Positions one region and paints its fill/stroke. One body for the pad
/// and the button, so a tint fix (e.g. the wasm alpha rule) can never apply
/// to one and drift from the other; callers resolve shown-vs-hidden colors.
fn paint_region(
    node: &mut Node,
    bg: &mut BackgroundColor,
    border: &mut BorderColor,
    rect: Rect,
    ui_scale: f32,
    (fill, stroke): (Color, Color),
) {
    // `rect` is in window pixels (the translator hit-tests touches there);
    // `Val::Px` is in UI units, which `UiScale` multiplies back up.
    let ui_scale = ui_scale.max(f32::EPSILON);
    node.left = Val::Px(rect.min.x / ui_scale);
    node.top = Val::Px(rect.min.y / ui_scale);
    node.width = Val::Px(rect.width() / ui_scale);
    node.height = Val::Px(rect.height() / ui_scale);
    // Plain writes, not `set_if_neq`: the caller's `&mut` reborrow of the
    // query's `Mut` already marked these components changed, so an inner
    // guard could not spare a tick — unlike the settings painter, which
    // calls `set_if_neq` on the un-dereferenced `Mut` itself.
    bg.0 = fill;
    *border = BorderColor::all(stroke);
}

/// Whether the Zone Pad and SWING button are drawn: exactly while the
/// translator claims their regions (the shared predicate, so drawn
/// controls whose touches would do something else entirely can never stay
/// on screen). The translator only runs in `Playing`, so while Paused the
/// gameplay chrome hides: an active-looking pad over the pause board would
/// be dead to the touch.
fn zone_pad_shown(
    state: &State<crate::game::GameState>,
    controllers: &crate::game::input::Controllers,
    score: &ScoreBoard,
    settings: &Settings,
    play: &crate::game::flow::Play,
) -> bool {
    let touch_team = controllers.touch_team;
    *state.get() == crate::game::GameState::Playing
        && touch_team.is_some()
        && crate::game::touch::zone_pad_active(
            settings.touch_scheme,
            touch_team == Some(score.batting_team()),
            play.phase,
        )
}

/// The HUD's bottom corners, in UI pixels: (AT BAT card's left, its
/// bottom, the bottom-right cluster's right, its bottom).
pub(super) type CornerInsets = [f32; 4];

/// Where the bottom HUD cards rest: 14 px in from each corner.
pub(super) const RESTING_INSETS: CornerInsets = [14.0, 14.0, 14.0, 14.0];

/// Gap kept between a moved card and the control it steps clear of (UI px).
const DODGE_GAP_PX: f32 = 10.0;

/// The bottom cards' widths (UI px) a slide must make room for: the AT BAT
/// card's fixed width, and the corner cluster (meter track, gap and a
/// scoreboard card whose width follows its text) with some slack.
const AT_BAT_W_PX: f32 = 214.0;
const CLUSTER_W_PX: f32 = 275.0;
/// The AT BAT card's height (UI px), with some slack.
const AT_BAT_H_PX: f32 = 125.0;

/// A bottom card's window-pixel rect: `side` px in from its own edge (the
/// left when `from_left`, else the right) and `bottom` px up, `w` by `h`,
/// all in UI px at `scale`.
fn card_rect(
    size: Vec2,
    scale: f32,
    side: f32,
    bottom: f32,
    w: f32,
    h: f32,
    from_left: bool,
) -> Rect {
    let x0 = if from_left {
        side * scale
    } else {
        size.x - (side + w) * scale
    };
    let y1 = size.y - bottom * scale;
    Rect::from_corners(Vec2::new(x0, y1 - h * scale), Vec2::new(x0 + w * scale, y1))
}

/// The corner cluster's height (UI px): the 120 px meter track and its
/// border, or the scoreboard card, with some slack.
const CLUSTER_H_PX: f32 = 130.0;

/// Where the bottom HUD cards go while the Zone Pad shows, for a window of
/// `size` logical pixels at UI scale `scale`: the pad fills the
/// bottom-left corner and the SWING button the bottom-right, right where
/// the AT BAT card and the scoreboard sit. When the gap between the two
/// controls holds both cards (a landscape phone) they slide inward beside
/// the controls, keeping the field above clear; otherwise (portrait, or a
/// tablet whose controls are big for its width) they lift above the pad's
/// top edge.
pub(super) fn dodge_insets(size: Vec2, scale: f32) -> CornerInsets {
    let scale = scale.max(1e-3);
    let pad = crate::game::touch::pad_rect(size);
    let swing = crate::game::touch::swing_button_rect(size);
    let room = (swing.min.x - pad.max.x) / scale - 3.0 * DODGE_GAP_PX;
    if room >= AT_BAT_W_PX + CLUSTER_W_PX {
        [
            pad.max.x / scale + DODGE_GAP_PX,
            RESTING_INSETS[1],
            (size.x - swing.min.x) / scale + DODGE_GAP_PX,
            RESTING_INSETS[3],
        ]
    } else {
        // The AT BAT card lifts over the pad. The cluster takes the first
        // spot that clears every control and the card: over the SWING
        // button in its corner (which keeps it under the pause button
        // where the screen is wide enough), stepped left of the pause
        // button, then stacked above the AT BAT card (a portrait phone,
        // where the two side by side would reach over the pad).
        let over_pad = (size.y - pad.min.y) / scale + DODGE_GAP_PX;
        let over_swing = (size.y - swing.min.y) / scale + DODGE_GAP_PX;
        let pause = crate::game::touch::pause_button_rect(size);
        let left_of_pause = (size.x - pause.min.x) / scale + DODGE_GAP_PX;
        let stacked = over_pad + AT_BAT_H_PX + DODGE_GAP_PX;
        let at_bat = card_rect(
            size,
            scale,
            RESTING_INSETS[0],
            over_pad,
            AT_BAT_W_PX,
            AT_BAT_H_PX,
            true,
        );
        let candidates = [
            (RESTING_INSETS[2], over_swing),
            (left_of_pause, over_swing),
            (RESTING_INSETS[2], stacked),
            (left_of_pause, stacked),
        ];
        let clear = |&(right, bottom): &(f32, f32)| {
            let r = card_rect(
                size,
                scale,
                right,
                bottom,
                CLUSTER_W_PX,
                CLUSTER_H_PX,
                false,
            );
            [pad, swing, pause, at_bat]
                .iter()
                .all(|o| o.intersect(r).is_empty())
                && r.min.x >= 0.0
                && r.min.y >= 0.0
        };
        let (right, bottom) = candidates
            .iter()
            .copied()
            .find(|c| clear(c))
            .unwrap_or(candidates[3]);
        [RESTING_INSETS[0], over_pad, right, bottom]
    }
}

/// Moves the bottom HUD cards clear of the Zone Pad and SWING button while
/// the touch player bats with the pad, and back to their corners for the
/// other half. On a phone the cards sat
/// right over both (portrait and landscape alike), so a thumb aiming at
/// the pad landed on the batter's card. Only the painted roots' offsets
/// change, and only when the target moved (the wasm UI rule; no per-frame
/// `Node` writes).
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn dodge_zone_pad(
    windows: Query<&Window, With<PrimaryWindow>>,
    controllers: Res<crate::game::input::Controllers>,
    settings: Res<Settings>,
    score: Res<ScoreBoard>,
    ui_scale: Res<UiScale>,
    mut at_bat: Query<&mut Node, (With<super::AtBatCard>, Without<super::HudCornerCluster>)>,
    mut cluster: Query<&mut Node, (With<super::HudCornerCluster>, Without<super::AtBatCard>)>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    // The whole at-bat half, not just the pre-pitch window the pad is
    // drawn in: tied to the pad's own window the cards jumped away and back
    // on every pitch.
    let batting_with_pad = controllers.touch_team.is_some()
        && controllers.touch_team == Some(score.batting_team())
        && settings.touch_scheme == TouchScheme::ZonePad;
    let insets = if batting_with_pad {
        dodge_insets(window.size(), ui_scale.0)
    } else {
        RESTING_INSETS
    };
    let [left, low_left, right, low_right] = insets.map(Val::Px);
    for mut node in &mut at_bat {
        if node.left != left || node.bottom != low_left {
            node.left = left;
            node.bottom = low_left;
        }
    }
    for mut node in &mut cluster {
        if node.right != right || node.bottom != low_right {
            node.right = right;
            node.bottom = low_right;
        }
    }
}

/// Repaints the chrome from the selected scheme and whose turn it is —
/// but only when an input actually changed (or the window resized): the
/// unguarded version dirtied `Node`s every frame, forcing a UI relayout per
/// tick for every player, touch or not. Child mutation only; geometry comes
/// from the same layout functions the translator hit-tests with.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn paint_touch_overlay(
    windows: Query<&Window, With<PrimaryWindow>>,
    state: Res<State<crate::game::GameState>>,
    controllers: Res<crate::game::input::Controllers>,
    settings: Res<Settings>,
    score: Res<ScoreBoard>,
    play: Res<crate::game::flow::Play>,
    theme: Res<Theme>,
    ui_scale: Res<UiScale>,
    gestures: Res<crate::game::touch::TouchGestures>,
    mut last: Query<&mut LastPainted, With<TouchOverlay>>,
    mut regions: Query<(
        &TouchChrome,
        &mut Node,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
    mut labels: Query<(&TouchLabel, &mut Text)>,
    mut hint_rows: Query<&mut Node, (With<TouchHintRow>, Without<TouchChrome>)>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let size = window.size();
    // Gate on the *outputs*, not the inputs: the chrome's whole appearance
    // is (show_zone_pad, show_pause, hint line, geometry from size, colors
    // from theme), so recompute those cheaply and repaint only when one
    // changed. (Input change ticks are unusable here — `TouchGestures`
    // mutates every held-finger frame, `ScoreBoard` on every ball/strike.)
    let seen = gestures.touch_seen();
    // The same resolved ownership the translator merges by.
    let touch_team = controllers.touch_team;
    let scheme = settings.touch_scheme;
    let playing = *state.get() == crate::game::GameState::Playing;
    let batting = touch_team == Some(score.batting_team());
    let show_zone_pad = zone_pad_shown(&state, &controllers, &score, &settings, &play);
    // One predicate shared with `tap_pause`, so the button can never be an
    // invisible-but-live target.
    let show_pause = crate::game::touch::pause_chrome_visible(touch_team, seen);
    let line = hint_line(
        scheme,
        touch_team.is_some(),
        playing,
        batting,
        play.phase,
        seen,
    );
    let Ok(mut last) = last.single_mut() else {
        return;
    };
    // A freshly spawned tree's default key (`shown: None`) can never match
    // a real state, so its first paint always fires — no respawn repair
    // query needed (see [`LastPainted`]).
    if !(last.size != size
        || last.ui_scale != ui_scale.0
        || last.shown != Some((show_zone_pad, show_pause))
        || last.line != line
        || theme.is_changed())
    {
        return;
    }
    *last = LastPainted {
        size,
        ui_scale: ui_scale.0,
        shown: Some((show_zone_pad, show_pause)),
        line,
    };
    let ui = &theme.ui;
    let hidden = (hidden_tint(ui.panel_bg), hidden_tint(ui.accent));
    let hint_top = Val::Px(if size.y > size.x {
        PORTRAIT_HINT_TOP_PX
    } else {
        LANDSCAPE_HINT_TOP_PX
    });
    for mut node in &mut hint_rows {
        node.top = hint_top;
    }

    for (kind, mut node, mut bg, mut border) in &mut regions {
        let (rect, shown) = match kind {
            TouchChrome::Pad => (
                crate::game::touch::pad_rect(size),
                show_zone_pad.then(|| (ui.panel_bg.with_alpha(0.18), ui.accent.with_alpha(0.7))),
            ),
            TouchChrome::SwingButton => (
                crate::game::touch::swing_button_rect(size),
                show_zone_pad.then(|| (ui.accent.with_alpha(0.25), ui.accent.with_alpha(0.8))),
            ),
            TouchChrome::PauseButton => (
                crate::game::touch::pause_button_rect(size),
                show_pause.then(|| (ui.panel_bg.with_alpha(0.35), ui.accent.with_alpha(0.7))),
            ),
        };
        paint_region(
            &mut node,
            &mut bg,
            &mut border,
            rect,
            ui_scale.0,
            shown.unwrap_or(hidden),
        );
    }
    for (label, mut text) in &mut labels {
        let want = match label {
            TouchLabel::Swing => {
                if show_zone_pad {
                    "SWING"
                } else {
                    ""
                }
            }
            TouchLabel::Pause => {
                if show_pause {
                    "II"
                } else {
                    ""
                }
            }
            TouchLabel::Hint => line,
        };
        super::set_text_if_neq(&mut text, want);
    }
}

/// Forwards a press on the touch pause button to the pause board's own
/// input seam (`subs::PauseTapped`) — the dead-ball gate, refused-press
/// banner, and resume path all live there, shared with Esc/P/Start. Runs in
/// `Paused` too: the same button resumes. Gated on the same visibility
/// predicate the paint uses (a hidden button is only tint-hidden — its
/// `Interaction` stays geometrically live, and a desktop mouse click on the
/// invisible node must not pause the game).
pub(super) fn tap_pause(
    interactions: Query<&Interaction, (Changed<Interaction>, With<TouchPauseButton>)>,
    mouse: Res<ButtonInput<MouseButton>>,
    gestures: Res<crate::game::touch::TouchGestures>,
    mut tapped: ResMut<crate::game::subs::PauseTapped>,
) {
    // Interactions change only on press/release/hover edges; skip the
    // resource read on the near-universal empty frame. The liveness bit is
    // the translator's per-frame decision (from *pre-frame* device state),
    // so a hidden button — including on the very frame a first touch
    // reveals it — can never be a live target.
    if interactions.is_empty() || !gestures.pause_chrome_live() {
        return;
    }
    // The `Interaction` path exists for the mouse only (see
    // `input::mouse_pressed_on` for the attribution policy): touch presses
    // on this rect are claimed raw by the translator (`read_touch` while
    // Playing, `paused_pause_region_taps` while Paused).
    if crate::game::input::mouse_pressed_on(&mouse, interactions.iter()) {
        tapped.0 = true;
    }
}

#[cfg(test)]
#[path = "touch.test.rs"]
mod tests;
