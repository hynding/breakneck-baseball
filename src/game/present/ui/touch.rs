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

/// The overlay's last-painted appearance key, ON the overlay root — not in
/// system `Local`s, whose lifetime is the system's: caches that outlived
/// the between-games despawn/respawn shipped a never-repainted fresh tree
/// (TADA 64) and then needed an `Added<TouchOverlay>` repair query. A
/// component dies and re-defaults with the tree it describes, so the whole
/// staleness class is gone (`shown: None` can never match a real state).
#[derive(Component, Default)]
pub(super) struct LastPainted {
    size: Vec2,
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
                ..default()
            },
            BackgroundColor(hidden_tint(ui.panel_bg)),
            BorderColor(hidden_tint(ui.accent)),
            BorderRadius::all(radius),
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
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(6.0),
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
                // Above the pause board's tier 30 (`subs::spawn_board`):
                // the button stays a live resume target while Paused (see
                // `paused_pause_region_taps`), so it must draw over the
                // board's full-screen dim — an invisible-but-live control
                // is the exact class the shared visibility predicate
                // exists to prevent. Safe under the wasm z-index rule:
                // this node is painted with real colors on show, not a
                // transparent never-repainted keep-alive root.
                GlobalZIndex(31),
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
    (fill, stroke): (Color, Color),
) {
    node.left = Val::Px(rect.min.x);
    node.top = Val::Px(rect.min.y);
    node.width = Val::Px(rect.width());
    node.height = Val::Px(rect.height());
    // Plain writes, not `set_if_neq`: the caller's `&mut` reborrow of the
    // query's `Mut` already marked these components changed, so an inner
    // guard could not spare a tick — unlike the settings painter, which
    // calls `set_if_neq` on the un-dereferenced `Mut` itself.
    bg.0 = fill;
    border.0 = stroke;
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
    gestures: Res<crate::game::touch::TouchGestures>,
    mut last: Query<&mut LastPainted, With<TouchOverlay>>,
    mut regions: Query<(
        &TouchChrome,
        &mut Node,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
    mut labels: Query<(&TouchLabel, &mut Text)>,
) {
    let Ok(window) = windows.get_single() else {
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
    let batting = touch_team == Some(score.batting_team());
    let scheme = settings.touch_scheme;
    // Exactly while the translator claims the pad/button regions — the
    // shared predicate, so drawn controls whose touches would do something
    // else entirely can never stay on screen. The translator only runs in
    // `Playing`, so while Paused (this system runs there too — the pause
    // button resumes) the gameplay chrome hides: an active-looking pad
    // over the pause board would be dead to the touch.
    let playing = *state.get() == crate::game::GameState::Playing;
    let show_zone_pad = playing
        && touch_team.is_some()
        && crate::game::touch::zone_pad_active(scheme, batting, play.phase);
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
    let Ok(mut last) = last.get_single_mut() else {
        return;
    };
    // A freshly spawned tree's default key (`shown: None`) can never match
    // a real state, so its first paint always fires — no respawn repair
    // query needed (see [`LastPainted`]).
    if !(last.size != size
        || last.shown != Some((show_zone_pad, show_pause))
        || last.line != line
        || theme.is_changed())
    {
        return;
    }
    *last = LastPainted {
        size,
        shown: Some((show_zone_pad, show_pause)),
        line,
    };
    let ui = &theme.ui;
    let hidden = (hidden_tint(ui.panel_bg), hidden_tint(ui.accent));

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
