//! The curtain: a full-screen node that closes at the end of a result beat
//! and opens on the next pre-pitch — the dip to black the reference footage
//! uses to punctuate a strike or a foul and to hide the reset underneath
//! (TODO 101; docs/agent/SMB3-REFERENCE-NOTES.md §2.3) — and, when the beat
//! ended the plate appearance, the **themed wipe**: a panel in the theme's
//! colours sweeping in from the left with the batting team's name, before
//! the next batter's walk-up (the reference's logo wipe, §2.7).
//!
//! Flow owns *when* it closes and how long that takes (`Play::curtain_progress`,
//! `ResultBeat::curtain_secs` — the phase flips only once the curtain is
//! fully across); this module owns the paint, the sweep, and the opening.
//! It also gives the game its fade-in from black on PLAY BALL for free: the
//! root spawns fully opaque, which is exactly what the wasm UI rule needs —
//! a root painted with a real colour at spawn, shown and hidden by mutation,
//! never alpha 0 (see [`hidden_tint`]).

use bevy::prelude::*;

use crate::game::flow::Play;
use crate::game::theme::Theme;
use crate::game::{GameplayEntity, ScoreBoard};

use super::{KeepAliveUi, hidden_tint, set_text_if_neq};

/// Seconds the curtain takes to open again after a dip (and after the
/// game-start fade-in, which starts from fully closed).
const OPEN_SECS: f32 = 0.35;
/// Seconds the wipe takes to sweep back out after the walk-up opens on it.
const WIPE_OPEN_SECS: f32 = 0.45;

/// The curtain's root node. Public so tests can read its `BackgroundColor`
/// and `Node`.
#[derive(Component)]
pub struct CurtainRoot;

/// The team name painted on the wipe. Public so tests can read it.
#[derive(Component)]
pub struct WipeLabel;

/// The curtain's state: how much of the screen it covers (1 = fully across
/// or fully black) and whether the current close is the themed wipe. Reset
/// to closed at game start so the first frames of play fade in from black.
#[derive(Resource)]
pub(super) struct Curtain {
    coverage: f32,
    wipe: bool,
}

impl Default for Curtain {
    fn default() -> Self {
        Self {
            coverage: 1.0,
            wipe: false,
        }
    }
}

/// Where the wipe panel's left edge sits (percent of the viewport width)
/// for a given `coverage`: sweeping in from the left while `closing`, out
/// to the right while opening. Pure so the sweep is unit-tested.
pub(super) fn wipe_left_percent(coverage: f32, closing: bool) -> f32 {
    let uncovered = 1.0 - coverage.clamp(0.0, 1.0);
    if closing {
        -100.0 * uncovered
    } else {
        100.0 * uncovered
    }
}

pub(super) fn spawn_curtain(
    mut commands: Commands,
    theme: Res<Theme>,
    mut curtain: ResMut<Curtain>,
) {
    *curtain = Curtain::default();
    commands
        .spawn((
            CurtainRoot,
            GameplayEntity,
            KeepAliveUi,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Percent(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::BLACK),
        ))
        .with_children(|root| {
            root.spawn((
                WipeLabel,
                Text::new(""),
                TextFont {
                    font_size: 72.0,
                    ..default()
                },
                TextColor(theme.ui.accent),
            ));
        });
}

/// Follows flow's curtain while it closes, opens it back up on its own
/// afterwards, and paints the root: black at the closing alpha for a dip,
/// an opaque theme panel sliding across for a wipe. Reduce-motion keeps
/// both: a cut to black is less motion than the reset it hides.
#[allow(clippy::type_complexity)]
pub(super) fn drive_curtain(
    time: Res<Time>,
    play: Res<Play>,
    score: Res<ScoreBoard>,
    theme: Res<Theme>,
    mut curtain: ResMut<Curtain>,
    mut roots: Query<(&mut Node, &mut BackgroundColor), With<CurtainRoot>>,
    mut labels: Query<&mut Text, With<WipeLabel>>,
) {
    let closing = play.curtain_progress();
    if closing.is_some() && curtain.coverage <= 0.0 {
        // The close begins: is this one the wipe?
        curtain.wipe = play.curtain_is_wipe();
    }
    let open_secs = if curtain.wipe {
        WIPE_OPEN_SECS
    } else {
        OPEN_SECS
    };
    let coverage = match closing {
        // Closing: track flow's timer so the frame the phase flips is the
        // frame the screen is fully covered.
        Some(progress) => curtain.coverage.max(progress),
        // Opening (or already open): retreat toward zero.
        None => (curtain.coverage - time.delta_secs() / open_secs).max(0.0),
    };
    if coverage == curtain.coverage && coverage == 0.0 {
        return;
    }
    curtain.coverage = coverage;
    let fully_open = coverage <= 0.0;
    if fully_open {
        curtain.wipe = false;
    }

    let label = if curtain.wipe {
        score.batting_team().label()
    } else {
        ""
    };
    for mut text in &mut labels {
        set_text_if_neq(&mut text, label);
    }
    for (mut node, mut bg) in &mut roots {
        if curtain.wipe {
            node.left = Val::Percent(wipe_left_percent(coverage, closing.is_some()));
            bg.0 = theme.ui.panel_bg.with_alpha(1.0);
        } else {
            node.left = Val::Percent(0.0);
            bg.0 = if fully_open {
                hidden_tint(Color::BLACK)
            } else {
                Color::BLACK.with_alpha(coverage)
            };
        }
    }
}

#[cfg(test)]
#[path = "curtain.test.rs"]
mod tests;
