//! The replay tag (plan task 9): a broadcast "REPLAY" bug top left, under
//! the base ring, while an instant replay plays, with "SLOW MOTION" under it on the
//! slowed angle and the skip prompt once a press would skip. Painted at spawn
//! with the near-invisible tint and shown by mutating it — the wasm UI rule
//! (see [`hidden_tint`]) — plus `Visibility::Hidden` while off, so the tint
//! never ghosts over a dark sky.

use bevy::prelude::*;

use crate::game::GameplayEntity;
use crate::game::replay::Replay;
use crate::game::theme::Theme;

use super::{KeepAliveUi, hidden_tint, set_color_if_neq, set_text_if_neq};

/// The tag's root.
#[derive(Component)]
pub(super) struct ReplayTag;

/// One line of the tag. Public so e2e tests can read the text.
#[derive(Component)]
pub struct ReplayTagText(pub ReplayTagLine);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReplayTagLine {
    /// "REPLAY".
    Title,
    /// "SLOW MOTION" on a slowed angle, else blank.
    Speed,
    /// The skip prompt, once a press skips.
    Skip,
}

/// The skip prompt. The action button is the one both sides already press
/// to pitch and swing.
pub const SKIP_PROMPT: &str = "PRESS ACTION TO SKIP";

pub(super) fn spawn_replay_tag(mut commands: Commands, theme: Res<Theme>) {
    let ui = &theme.ui;
    commands
        .spawn((
            ReplayTag,
            GameplayEntity,
            KeepAliveUi,
            Visibility::Hidden,
            Node {
                position_type: PositionType::Absolute,
                // Under the HUD's 96 px base ring (top-left at 14 px).
                top: Val::Px(122.0),
                left: Val::Px(14.0),
                padding: UiRect::axes(Val::Px(14.0), Val::Px(8.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(2.0),
                border: UiRect::all(Val::Px(1.5)),
                border_radius: BorderRadius::all(Val::Px(10.0)),
                ..default()
            },
            BackgroundColor(hidden_tint(ui.panel_bg)),
            BorderColor::all(hidden_tint(ui.accent)),
        ))
        .with_children(|tag| {
            for (line, size) in [
                (ReplayTagLine::Title, 26.0),
                (ReplayTagLine::Speed, 13.0),
                (ReplayTagLine::Skip, 12.0),
            ] {
                tag.spawn((
                    ReplayTagText(line),
                    Text::new(""),
                    TextFont {
                        font_size: size,
                        ..default()
                    },
                    TextColor(ui.text_primary),
                ));
            }
        });
}

/// Shows the tag while a replay plays, blanks it otherwise.
pub(super) fn paint_replay_tag(
    replay: Res<Replay>,
    theme: Res<Theme>,
    mut tags: Query<(&mut BackgroundColor, &mut BorderColor, &mut Visibility), With<ReplayTag>>,
    mut lines: Query<(&ReplayTagText, &mut Text, &mut TextColor)>,
) {
    let ui = &theme.ui;
    let on = replay.is_playing();
    for (mut bg, mut border, mut visibility) in &mut tags {
        let want_visibility = if on {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        visibility.set_if_neq(want_visibility);
        let (want_bg, want_border) = if on {
            (ui.panel_bg, ui.accent)
        } else {
            (hidden_tint(ui.panel_bg), hidden_tint(ui.accent))
        };
        if bg.0 != want_bg {
            bg.0 = want_bg;
        }
        border.set_if_neq(BorderColor::all(want_border));
    }
    for (line, mut text, mut color) in &mut lines {
        let (value, tint) = match line.0 {
            ReplayTagLine::Title if on => ("REPLAY", ui.accent),
            ReplayTagLine::Speed if replay.slow_motion() => ("SLOW MOTION", ui.text_primary),
            ReplayTagLine::Skip if replay.skippable() => (SKIP_PROMPT, ui.text_dim),
            _ => ("", ui.text_primary),
        };
        set_text_if_neq(&mut text, value);
        set_color_if_neq(&mut color, tint);
    }
}
