//! The lineup card (graphics plan task 15): both starting nines, side by
//! side under the matchup, over the second half of the intro flyover
//! ([`crate::game::intro`]). Spawned once at game start, painted with the
//! near-invisible tint and `Visibility::Hidden` while off, and shown by
//! mutating its children: the wasm UI rule (see [`hidden_tint`]). Its
//! painter only reads [`Intro`]; the intro's clock ticks elsewhere (the
//! wasm Timer-and-UI rule).

use bevy::prelude::*;

use crate::game::intro::Intro;
use crate::game::roster::Rosters;
use crate::game::rules::LINEUP_SIZE;
use crate::game::theme::Theme;
use crate::game::{GameplayEntity, Team};

use super::{KeepAliveUi, hidden_tint, set_text_if_neq};

/// The card's root.
#[derive(Component)]
pub(super) struct LineupCard;

/// One line of the card. Public so e2e tests can read the text.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineupLine {
    /// "AWAY AT HOME".
    Matchup,
    /// A team's column heading.
    Team(Team),
    /// A team's batter in a 1-based lineup slot.
    Batter(Team, u32),
}

/// The text on one line of the card. Public so e2e tests can read it.
#[derive(Component)]
pub struct LineupText(pub LineupLine);

pub(super) fn spawn_lineup_card(mut commands: Commands, theme: Res<Theme>) {
    let ui = &theme.ui;
    let line = |kind: LineupLine, size: f32, color: Color| {
        (
            LineupText(kind),
            Text::new(""),
            TextFont {
                font_size: size,
                ..default()
            },
            TextColor(color),
        )
    };
    commands
        .spawn((
            LineupCard,
            GameplayEntity,
            KeepAliveUi,
            Visibility::Hidden,
            Node {
                position_type: PositionType::Absolute,
                // Centred across the top: the duel cards stand down while
                // the intro plays, and a narrow phone has no room beside them.
                top: Val::Percent(14.0),
                left: Val::Percent(50.0),
                padding: UiRect::axes(Val::Px(20.0), Val::Px(16.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(10.0),
                border: UiRect::all(Val::Px(1.5)),
                border_radius: BorderRadius::all(Val::Px(12.0)),
                ..default()
            },
            // Back by half its own width, so it sits centred whatever its
            // text measures.
            UiTransform::from_translation(Val2::percent(-50.0, 0.0)),
            BackgroundColor(hidden_tint(ui.panel_bg)),
            BorderColor::all(hidden_tint(ui.panel_border)),
        ))
        .with_children(|card| {
            card.spawn(line(LineupLine::Matchup, 22.0, ui.accent));
            card.spawn(Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(28.0),
                ..default()
            })
            .with_children(|columns| {
                for team in [Team::Away, Team::Home] {
                    columns
                        .spawn(Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(3.0),
                            ..default()
                        })
                        .with_children(|column| {
                            column.spawn(line(LineupLine::Team(team), 16.0, ui.accent));
                            for slot in 1..=LINEUP_SIZE {
                                column.spawn(line(
                                    LineupLine::Batter(team, slot),
                                    14.0,
                                    ui.text_primary,
                                ));
                            }
                        });
                }
            });
        });
}

/// What a line of the card says.
pub(super) fn lineup_line(line: LineupLine, rosters: &Rosters) -> String {
    match line {
        LineupLine::Matchup => format!("{} AT {}", Team::Away.label(), Team::Home.label()),
        LineupLine::Team(team) => team.label().to_string(),
        LineupLine::Batter(team, slot) => {
            let card = rosters.team(team).batting(slot);
            format!("{slot}  #{:<2} {}", card.number, card.name)
        }
    }
}

/// Shows the card while the intro has it up, blank and hidden otherwise.
pub(super) fn paint_lineup_card(
    intro: Res<Intro>,
    rosters: Res<Rosters>,
    theme: Res<Theme>,
    mut cards: Query<(&mut BackgroundColor, &mut BorderColor, &mut Visibility), With<LineupCard>>,
    mut lines: Query<(&LineupText, &mut Text)>,
) {
    // The intro changes every frame it runs and once more as it ends; past
    // that nothing here moves.
    if !(intro.is_changed() || rosters.is_changed() || theme.is_changed()) {
        return;
    }
    let ui = &theme.ui;
    let on = intro.lineup_up();
    for (mut bg, mut border, mut visibility) in &mut cards {
        let want = if on {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != want {
            *visibility = want;
        }
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
    for (line, mut text) in &mut lines {
        let want = if on {
            lineup_line(line.0, &rosters)
        } else {
            String::new()
        };
        set_text_if_neq(&mut text, &want);
    }
}
