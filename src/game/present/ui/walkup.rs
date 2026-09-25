//! The walk-up card (TODO 103): the incoming batter's name, number, and
//! lineup slot, shown beside the walk-up shot while `Play::walkup_active`.
//! Painted at spawn with the near-invisible tint and shown by mutating its
//! children — the wasm UI rule (see [`hidden_tint`]).

use bevy::prelude::*;

use crate::game::flow::Play;
use crate::game::roster::Rosters;
use crate::game::rules::{BattingOrder, LINEUP_SIZE};
use crate::game::theme::Theme;
use crate::game::{GameplayEntity, ScoreBoard};

use super::{KeepAliveUi, hidden_tint, set_color_if_neq, set_text_if_neq};

/// The card's root.
#[derive(Component)]
pub(super) struct WalkUpCard;

/// One line of the card. Public so e2e tests can read the text.
#[derive(Component)]
pub struct WalkUpText(pub WalkUpLine);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WalkUpLine {
    Title,
    Name,
    Number,
    Slot,
}

pub(super) fn spawn_walkup_card(mut commands: Commands, theme: Res<Theme>) {
    let ui = &theme.ui;
    commands
        .spawn((
            WalkUpCard,
            GameplayEntity,
            KeepAliveUi,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Percent(30.0),
                left: Val::Percent(6.0),
                width: Val::Px(260.0),
                padding: UiRect::axes(Val::Px(18.0), Val::Px(14.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                border: UiRect::all(Val::Px(1.5)),
                ..default()
            },
            BackgroundColor(hidden_tint(ui.panel_bg)),
            BorderColor::all(hidden_tint(ui.panel_border)),
            BorderRadius::all(Val::Px(14.0)),
        ))
        .with_children(|card| {
            for (line, size) in [
                (WalkUpLine::Title, 14.0),
                (WalkUpLine::Name, 30.0),
                (WalkUpLine::Number, 22.0),
                (WalkUpLine::Slot, 14.0),
            ] {
                card.spawn((
                    WalkUpText(line),
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

/// Fills the card while the walk-up is on, blanks it otherwise.
pub(super) fn paint_walkup_card(
    play: Res<Play>,
    score: Res<ScoreBoard>,
    order: Res<BattingOrder>,
    rosters: Res<Rosters>,
    theme: Res<Theme>,
    mut cards: Query<(&mut BackgroundColor, &mut BorderColor), With<WalkUpCard>>,
    mut lines: Query<(&WalkUpText, &mut Text, &mut TextColor)>,
) {
    let ui = &theme.ui;
    let on = play.walkup_active();
    for (mut bg, mut border) in &mut cards {
        let (want_bg, want_border) = if on {
            (ui.panel_bg, ui.panel_border)
        } else {
            (hidden_tint(ui.panel_bg), hidden_tint(ui.panel_border))
        };
        if bg.0 != want_bg {
            bg.0 = want_bg;
        }
        let want_border = BorderColor::all(want_border);
        if *border != want_border {
            *border = want_border;
        }
    }
    let batting = score.batting_team();
    let slot = order.current(batting);
    let card = rosters.team(batting).batting(slot);
    for (line, mut text, mut color) in &mut lines {
        let (value, tint) = if !on {
            (String::new(), ui.text_primary)
        } else {
            match line.0 {
                WalkUpLine::Title => ("UP TO BAT".to_string(), ui.accent),
                WalkUpLine::Name => (card.name.clone(), ui.text_primary),
                WalkUpLine::Number => (format!("#{}", card.number), ui.text_primary),
                WalkUpLine::Slot => (
                    // Plain ASCII: the bundled font has no middle dot, and a glyph
                    // fallback is not worth risking on wasm.
                    format!("{}  -  AB {slot}/{LINEUP_SIZE}", batting.label()),
                    ui.text_dim,
                ),
            }
        };
        set_text_if_neq(&mut text, &value);
        set_color_if_neq(&mut color, tint);
    }
}
