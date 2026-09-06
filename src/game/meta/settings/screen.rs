//! The settings screen: spawn/paint of its UI and the input systems that
//! drive it (opened with **S** on the main menu; see `super`'s
//! [`super::SettingsPlugin`]).

use bevy::prelude::*;

use bevy::color::Alpha;

#[cfg(test)]
use super::BattingStyle;
use super::{Settings, SettingsOpen};
use crate::game::input::Controllers;
use crate::game::theme::Theme;
use crate::game::ui::{KeepAliveUi, hidden_tint, set_color_if_neq, set_text_if_neq};
use crate::game::{Team, batting};

// ── Settings screen ──────────────────────────────────────────────────────────

/// Root of the settings screen (full-screen centered column overlay).
#[derive(Component)]
pub(super) struct SettingsUi;

/// The screen's inner card — the opaque panel that occludes the menu behind
/// it while open (mirrors `subs.rs`'s `SubsUi`/`SubsCard` split: the menu
/// isn't a 3D backdrop like gameplay, so the overlay needs its own solid
/// panel rather than relying on world geometry to read as "in front").
#[derive(Component)]
pub(super) struct SettingsCard;

/// The screen's title line ("SETTINGS" while open).
#[derive(Component)]
pub(super) struct SettingsTitle;

/// Label text for each row — tinted to show the cursor row.
#[derive(Component)]
pub(super) struct SettingsRowLabel;

/// Value text for each row (styles, trail, colour, volume).
#[derive(Component)]
pub(super) struct SettingsRowText;

/// The row an element belongs to — the one index per entity, shared by the
/// label and value markers so paint and tap can never disagree about which
/// row an entity is.
#[derive(Component)]
pub(super) struct SettingsRowIndex(usize);

/// Which row the cursor is on (0..ROW_LABELS.len()).
#[derive(Resource, Default)]
pub struct SettingsCursorRow(usize);

impl SettingsCursorRow {
    /// Rests the cursor on the top row — the ONE parking encoding. Takes
    /// the `ResMut` (the `set_text_if_neq` pattern) so the guard's read
    /// never dirties the change tick the settings paint gates on:
    /// `toggle_settings` parks every frame the screen is closed, and an
    /// unconditional `&mut` there would repaint the (closed) screen per
    /// frame. Callers: `toggle_settings` (every close), `menu_tap`'s
    /// tap-open (the one open path the while-closed park never sees), and
    /// `close_settings_on_exit` (state-exit safety net).
    pub fn park(this: &mut ResMut<Self>) {
        if this.0 != 0 {
            this.0 = 0;
        }
    }
}

/// The ONE settings-open transition (the `press_quit` pattern): parks the
/// cursor, then opens — so a reopen can never start on CLOSE, whatever
/// path opened the screen. `toggle_settings`' while-closed park still
/// covers closes; this covers opens, including any path (like the menu's
/// "S Settings" tap, which runs before the settings chain) the while-closed
/// park never sees.
pub fn open_settings(open: &mut SettingsOpen, cursor: &mut ResMut<SettingsCursorRow>) {
    SettingsCursorRow::park(cursor);
    open.0 = true;
}

// Row indices, named once and used by both `paint_settings_screen` and
// `cycle_row` — inserting a row means touching these consts, not hunting
// magic numbers through two match statements (this file already paid that
// renumbering tax once).
const ROW_P1_STYLE: usize = 0;
const ROW_P2_STYLE: usize = 1;
const ROW_TOUCH: usize = 2;
const ROW_TRAIL: usize = 3;
const ROW_TRAIL_COLOR: usize = 4;
const ROW_REDUCE_MOTION: usize = 5;
const ROW_STRIKE_ZONE: usize = 6;
const ROW_VOLUME: usize = 7;
/// The CLOSE pseudo-row (tap / Left / Right closes the screen — the touch
/// equivalent of Esc). Keep it last; keep the array below in index order.
const ROW_CLOSE: usize = 8;

const ROW_LABELS: [&str; ROW_CLOSE + 1] = [
    "P1 BATTING STYLE",
    "P2 BATTING STYLE",
    "P1 TOUCH SWING",
    "PITCH TRAIL",
    "TRAIL COLOR",
    "REDUCE MOTION",
    "STRIKE ZONE",
    "VOLUME",
    "CLOSE",
];

/// Two-state row value text.
fn on_off(on: bool) -> &'static str {
    if on { "On" } else { "Off" }
}

/// Builds the settings screen once at startup, painted behind
/// [`hidden_tint`] per the wasm UI rule (see `subs.rs`): spawned once, shown
/// and hidden only by mutating the children of this root — never despawned
/// or respawned mid-session.
pub(super) fn spawn_settings_screen(mut commands: Commands, theme: Res<Theme>) {
    let ui = &theme.ui;
    commands
        .spawn((
            SettingsUi,
            KeepAliveUi,
            // Overlay tier 20 — above the menu (10); see TODO 67.
            GlobalZIndex(20),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(hidden_tint(ui.panel_bg)),
        ))
        .with_children(|root| {
            root.spawn((
                SettingsCard,
                KeepAliveUi,
                Node {
                    padding: UiRect::axes(Val::Px(40.0), Val::Px(28.0)),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Px(10.0),
                    border: UiRect::all(Val::Px(1.5)),
                    ..default()
                },
                BackgroundColor(hidden_tint(ui.panel_bg)),
                BorderColor(hidden_tint(ui.panel_border)),
                BorderRadius::all(Val::Px(16.0)),
            ))
            .with_children(|card| {
                card.spawn((
                    SettingsTitle,
                    Text::new(""),
                    TextFont {
                        font_size: 34.0,
                        ..default()
                    },
                    TextColor(ui.accent),
                ));
                // The ONE row-part bundle: label and value are two entities
                // making up one tappable row, so both carry `Button` and
                // the row index (a touch anywhere on the row selects and
                // cycles it). One body, so a component every part needs
                // can't reach one and miss the other — leaving a row half
                // that renders but never taps.
                let row_part = |i: usize, font_size: f32, color: Color| {
                    (
                        Button,
                        SettingsRowIndex(i),
                        Text::new(""),
                        TextFont {
                            font_size,
                            ..default()
                        },
                        TextColor(color),
                    )
                };
                for i in 0..ROW_LABELS.len() {
                    card.spawn((SettingsRowLabel, row_part(i, 20.0, ui.text_primary)));
                    card.spawn((SettingsRowText, row_part(i, 18.0, ui.text_dim)));
                }
            });
        });
}

/// Paints the screen every frame from state: blank/hidden when closed, full
/// content when open (child mutation only — wasm rule, see `subs.rs`). The
/// root dims to a translucent scrim and the card goes opaque while open so
/// the settings content fully occludes the menu behind it (the menu has no
/// 3D backdrop the way gameplay does, so the overlay must paint its own).
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn paint_settings_screen(
    open: Res<SettingsOpen>,
    cursor: Res<SettingsCursorRow>,
    settings: Res<Settings>,
    controllers: Res<Controllers>,
    theme: Res<Theme>,
    mut roots: Query<&mut BackgroundColor, (With<SettingsUi>, Without<SettingsCard>)>,
    mut cards: Query<(&mut BackgroundColor, &mut BorderColor), With<SettingsCard>>,
    mut title: Query<
        (&mut Text, &mut TextColor),
        (
            With<SettingsTitle>,
            Without<SettingsRowLabel>,
            Without<SettingsRowText>,
        ),
    >,
    mut labels: Query<
        (&SettingsRowIndex, &mut Text, &mut TextColor),
        (
            With<SettingsRowLabel>,
            Without<SettingsTitle>,
            Without<SettingsRowText>,
        ),
    >,
    mut values: Query<
        (&SettingsRowIndex, &mut Text),
        (
            With<SettingsRowText>,
            Without<SettingsTitle>,
            Without<SettingsRowLabel>,
        ),
    >,
) {
    let Ok((mut title_text, mut title_color)) = title.get_single_mut() else {
        return;
    };
    // Everything this system writes derives from exactly these five
    // resources (`Controllers` for the resolved touch owner — its write is
    // guarded, so its tick flips only on real ownership changes), so one
    // early-return covers the ~100% common frame; `KeepAliveUi` on the root
    // and card keeps their wasm extraction alive (the 2026-08-27 bisect)
    // without per-frame repaints here.
    if !(open.is_changed()
        || cursor.is_changed()
        || settings.is_changed()
        || controllers.is_changed()
        || theme.is_changed())
    {
        return;
    }
    let ui = &theme.ui;
    if !open.0 {
        for mut bg in &mut roots {
            bg.set_if_neq(BackgroundColor(hidden_tint(ui.panel_bg)));
        }
        for (mut bg, mut border) in &mut cards {
            bg.set_if_neq(BackgroundColor(hidden_tint(ui.panel_bg)));
            border.set_if_neq(BorderColor(hidden_tint(ui.panel_border)));
        }
        // Compare-before-write: an unconditional write would dirty every
        // Text each frame (MainMenu is the boot state), re-shaping ~20
        // strings per tick for a screen that isn't even open.
        set_text_if_neq(&mut title_text, "");
        for (_, mut text, _) in &mut labels {
            set_text_if_neq(&mut text, "");
        }
        for (_, mut text) in &mut values {
            set_text_if_neq(&mut text, "");
        }
        return;
    }
    for mut bg in &mut roots {
        bg.set_if_neq(BackgroundColor(ui.panel_bg.with_alpha(0.9)));
    }
    for (mut bg, mut border) in &mut cards {
        // Theme panel colours carry their own translucency (~0.85 alpha) for
        // layering over the 3D field; here the menu sits directly behind, so
        // the card must be fully opaque or its text collides with the menu's.
        bg.set_if_neq(BackgroundColor(ui.panel_bg.with_alpha(1.0)));
        border.set_if_neq(BorderColor(ui.panel_border));
    }
    set_text_if_neq(&mut title_text, "SETTINGS");
    set_color_if_neq(&mut title_color, ui.accent);
    for (label, mut text, mut color) in &mut labels {
        let marker = if cursor.0 == label.0 { "> " } else { "  " };
        let want = format!("{marker}{}", ROW_LABELS[label.0]);
        set_text_if_neq(&mut text, &want);
        let want_color = if cursor.0 == label.0 {
            ui.accent
        } else {
            ui.text_primary
        };
        set_color_if_neq(&mut color, want_color);
    }
    for (row, mut text) in &mut values {
        let want = match row.0 {
            // The touch scheme owning P1's style — by the SAME predicate
            // `style_for` applies (resolved owner + scheme), so this row
            // can never claim an override the adapter isn't applying
            // (e.g. on a desktop where no touchscreen was ever seen).
            ROW_P1_STYLE => {
                match batting::touch_style_override(Team::Home, &controllers, &settings) {
                    Some(style) => format!("{} [set by TOUCH SWING row]", style.label()),
                    None => settings.batting_style[0].label().to_string(),
                }
            }
            ROW_P2_STYLE => settings.batting_style[1].label().to_string(),
            ROW_TOUCH => settings.touch_scheme.label().to_string(),
            ROW_TRAIL => settings.pitch_trail.label().to_string(),
            ROW_TRAIL_COLOR => settings.trail_color.label().to_string(),
            ROW_REDUCE_MOTION => on_off(settings.reduce_motion).to_string(),
            // The zone overlay was persisted but reachable only via the
            // undiscoverable Z on the pause board (TODO 79).
            ROW_STRIKE_ZONE => on_off(settings.show_strike_zone).to_string(),
            ROW_VOLUME => format!("{:.0}%", settings.volume * 100.0),
            ROW_CLOSE => "tap / Esc".to_string(),
            // Loud, not silent: a newly appended row without a paint arm
            // must fail here, not render CLOSE's hint text.
            row => unreachable!("unpainted settings row {row}"),
        };
        set_text_if_neq(&mut text, &want);
    }
}

/// **S** / gamepad **Select** toggles the screen open/closed; **Esc** /
/// gamepad **East** (B) closes it while open. MainMenu only. `Select` is
/// also `game::camera`'s duel-view toggle, and `East` is `menu.rs`'s
/// innings-cycle key — no clash: those systems run only in `Playing`
/// (`Select`) or are gated `.run_if(settings_closed)` (`East`), so they
/// never fire alongside this system's `MainMenu`-only, open-state-gated
/// handling of the same buttons.
pub(super) fn toggle_settings(
    keyboard: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    mut open: ResMut<SettingsOpen>,
    mut cursor: ResMut<SettingsCursorRow>,
) {
    // The key-path parking site: while the screen is CLOSED — however it
    // closed, this system or the CLOSE row or Esc — the cursor rests at the
    // top, so a reopen can never start on CLOSE, where the first edit press
    // would instantly dismiss the screen. The `!open.0` gate is load-bearing
    // (an unconditional park here re-parks the cursor every OPEN frame too,
    // making every row below the first unreachable). Parking BEFORE the
    // toggle below keeps the invariant airtight for a same-frame S reopen;
    // the ONE open path this system never sees closed is the menu's "S
    // Settings" tap (`menu_tap` runs before this chain), which parks at its
    // own open site.
    if !open.0 {
        SettingsCursorRow::park(&mut cursor);
    }
    let toggle_pressed = keyboard.just_pressed(KeyCode::KeyS)
        || pads.iter().any(|p| p.just_pressed(GamepadButton::Select));
    let close_pressed = keyboard.just_pressed(KeyCode::Escape)
        || pads.iter().any(|p| p.just_pressed(GamepadButton::East));
    if toggle_pressed {
        if open.0 {
            open.0 = false;
        } else {
            open_settings(&mut open, &mut cursor);
        }
    } else if open.0 && close_pressed {
        open.0 = false;
    }
}

/// Cycles one settings row. Shared by the arrow-key path (`wrap_volume`
/// false — Right clamps at full, Left goes down) and the tap path
/// (`wrap_volume` true — taps can only go "right", so full wraps to mute);
/// the CLOSE pseudo-row closes the screen instead of editing anything.
/// `p1_style_overridden` is [`batting::touch_style_override`]'s verdict,
/// threaded in so this pure fn stays resource-free.
fn cycle_row(
    row: usize,
    right: bool,
    wrap_volume: bool,
    p1_style_overridden: bool,
    settings: &mut Settings,
    open: &mut SettingsOpen,
) {
    match row {
        // While the touch scheme owns P1's style (same predicate the
        // adapter applies) the stored value is dormant and the row displays
        // the scheme's effective style — editing the hidden value would
        // cycle state the player can't see.
        ROW_P1_STYLE if p1_style_overridden => {}
        ROW_P1_STYLE | ROW_P2_STYLE => {
            // Row → slot mapped explicitly: indexing by `row` worked only
            // because these consts happen to be 0 and 1 — a row inserted
            // above them would renumber the consts and turn this into an
            // out-of-bounds panic (or a wrong-player edit) with no compile
            // error and no test failure.
            let slot = usize::from(row == ROW_P2_STYLE);
            let s = settings.batting_style[slot];
            settings.batting_style[slot] = if right { s.next() } else { s.prev() };
        }
        ROW_TOUCH => {
            let s = settings.touch_scheme;
            settings.touch_scheme = if right { s.next() } else { s.prev() };
        }
        ROW_TRAIL => {
            let s = settings.pitch_trail;
            settings.pitch_trail = if right { s.next() } else { s.prev() };
        }
        ROW_TRAIL_COLOR => {
            let c = settings.trail_color;
            settings.trail_color = if right { c.next() } else { c.prev() };
        }
        ROW_REDUCE_MOTION => settings.reduce_motion = !settings.reduce_motion,
        ROW_STRIKE_ZONE => settings.show_strike_zone = !settings.show_strike_zone,
        ROW_VOLUME => {
            settings.volume = if right {
                if wrap_volume && settings.volume >= 0.999 {
                    0.0
                } else {
                    (settings.volume + 0.1).min(1.0)
                }
            } else {
                (settings.volume - 0.1).max(0.0)
            };
        }
        ROW_CLOSE => open.0 = false,
        // Loud like the paint arm's `unreachable!`: a newly appended row
        // without a cycle arm must fail here, not ship a row that renders
        // perfectly and silently ignores every tap and keypress.
        row => unreachable!("uncycled settings row {row}"),
    }
}

/// Up/Down (or gamepad DPad Up/Down) move the row cursor; Left/Right (or
/// DPad Left/Right) cycle the row's value or nudge the volume (±0.1,
/// clamped). No-op while the screen is closed.
pub(super) fn edit_settings(
    keyboard: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    controllers: Res<Controllers>,
    mut open: ResMut<SettingsOpen>,
    mut cursor: ResMut<SettingsCursorRow>,
    mut settings: ResMut<Settings>,
) {
    if !open.0 {
        return;
    }
    let up = keyboard.just_pressed(KeyCode::ArrowUp)
        || pads.iter().any(|p| p.just_pressed(GamepadButton::DPadUp));
    let down = keyboard.just_pressed(KeyCode::ArrowDown)
        || pads.iter().any(|p| p.just_pressed(GamepadButton::DPadDown));
    if up {
        cursor.0 = cursor.0.checked_sub(1).unwrap_or(ROW_LABELS.len() - 1);
    }
    if down {
        cursor.0 = (cursor.0 + 1) % ROW_LABELS.len();
    }
    let left = keyboard.just_pressed(KeyCode::ArrowLeft)
        || pads.iter().any(|p| p.just_pressed(GamepadButton::DPadLeft));
    let right = keyboard.just_pressed(KeyCode::ArrowRight)
        || pads
            .iter()
            .any(|p| p.just_pressed(GamepadButton::DPadRight));
    if !(left || right) {
        return;
    }
    cycle_row_tracked(
        cursor.0,
        right,
        false,
        &controllers,
        &mut settings,
        &mut open,
    );
    // A close via the CLOSE row needs no handling here: `toggle_settings`
    // (first in the chain) parks the cursor whenever the screen is closed.
}

/// Runs [`cycle_row`] while keeping change detection honest: the `ResMut`s
/// are only marked changed when the row actually edited them — otherwise a
/// tap on CLOSE (or a Left on already-muted volume) would trigger
/// `persist_settings`' store write for an interaction that changed nothing.
/// Works on plain clones and commits via `set_if_neq`, Bevy's purpose-built
/// change-honest write.
fn cycle_row_tracked(
    row: usize,
    right: bool,
    wrap_volume: bool,
    controllers: &Controllers,
    settings: &mut ResMut<Settings>,
    open: &mut ResMut<SettingsOpen>,
) {
    // The P1-row lock verdict is computed HERE, not by callers: a third
    // edit path that forgot the two-line preamble would pass `false` and
    // silently edit the dormant hidden value the guard exists to protect.
    let overridden = batting::touch_style_override(Team::Home, controllers, settings).is_some();
    let mut next_settings = (**settings).clone();
    let mut next_open = **open;
    cycle_row(
        row,
        right,
        wrap_volume,
        overridden,
        &mut next_settings,
        &mut next_open,
    );
    settings.set_if_neq(next_settings);
    open.set_if_neq(next_open);
}

/// The touch/mouse path: tapping a row selects it and cycles it forward
/// (the CLOSE row closes the screen). Runs only while the screen is open.
#[allow(clippy::too_many_arguments)]
pub(super) fn tap_settings_row(
    rows: Query<(Entity, &SettingsRowIndex, &ComputedNode, &GlobalTransform), With<Button>>,
    interactions: Query<&Interaction, (Changed<Interaction>, With<Button>)>,
    touches: Res<Touches>,
    mouse: Res<ButtonInput<MouseButton>>,
    controllers: Res<Controllers>,
    mut open: ResMut<SettingsOpen>,
    mut cursor: ResMut<SettingsCursorRow>,
    mut settings: ResMut<Settings>,
) {
    if !open.0 {
        return;
    }
    // O(1) early-out on the no-input frame — an open screen is mostly
    // being read, and the 18 per-row lookups below can only act when an
    // edge or a pointer press exists.
    if interactions.is_empty() && !crate::game::input::pointer_pressed(&touches, &mouse) {
        return;
    }
    for (entity, index, node, transform) in &rows {
        if !crate::game::input::button_pressed(
            &interactions,
            entity,
            &touches,
            &mouse,
            node,
            transform,
        ) {
            continue;
        }
        let row = index.0;
        // Guarded like `park`: an unconditional write dirties the tick that
        // `paint_settings_screen`'s gate reads, re-shaping every row's text
        // on a repeat tap of the row the cursor already sits on.
        if cursor.0 != row {
            cursor.0 = row;
        }
        cycle_row_tracked(row, true, true, &controllers, &mut settings, &mut open);
        // Stop — ONE activation per frame, the rule `menu_tap` already
        // states: falling through made a multi-touch frame (two fingers
        // landing in one batch) apply several row edits in query — i.e.
        // spawn — order, so a stray second hit cycled a value nobody chose,
        // or edited a screen that the CLOSE row had just shut.
        // (`toggle_settings` parks the cursor for the close, like every
        // other close path.)
        return;
    }
}

/// Safety-net reset for leaving `MainMenu`: see the `OnExit` registration.
/// Writes conditionally — `SettingsOpen`'s change tick gates the settings
/// paint, and a defensive no-op write on every game start would dirty it
/// for nothing.
pub(super) fn close_settings_on_exit(
    mut open: ResMut<SettingsOpen>,
    mut cursor: ResMut<SettingsCursorRow>,
) {
    if open.0 {
        open.0 = false;
    }
    SettingsCursorRow::park(&mut cursor);
}

#[cfg(test)]
mod tests {
    use super::*;

    // Keyboard path only: the harness has no gamepad-input injection
    // precedent anywhere in the crate (no test presses a `GamepadButton`),
    // so the gamepad half of `toggle_settings`/`edit_settings` isn't
    // exercised in unit tests — the `Query<&Gamepad>` is simply empty here
    // and both systems fall through to their keyboard branch unchanged.
    #[test]
    fn s_key_toggles_and_esc_closes() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<SettingsOpen>()
            .init_resource::<SettingsCursorRow>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, toggle_settings);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyS);
        app.update();
        assert!(app.world().resource::<SettingsOpen>().0);

        let mut kb = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        kb.clear_just_pressed(KeyCode::KeyS);
        kb.press(KeyCode::Escape);
        app.update();
        assert!(!app.world().resource::<SettingsOpen>().0);
    }

    /// The `ROW_*` consts and the positional `ROW_LABELS` array are two
    /// representations of one ordering; a drift between them compiles fine
    /// and ships a screen whose labels name one setting while cycling edits
    /// another. Pin the pairing.
    #[test]
    fn row_consts_agree_with_their_labels() {
        for (row, want) in [
            (ROW_P1_STYLE, "P1 BATTING STYLE"),
            (ROW_P2_STYLE, "P2 BATTING STYLE"),
            (ROW_TOUCH, "P1 TOUCH SWING"),
            (ROW_TRAIL, "PITCH TRAIL"),
            (ROW_TRAIL_COLOR, "TRAIL COLOR"),
            (ROW_REDUCE_MOTION, "REDUCE MOTION"),
            (ROW_STRIKE_ZONE, "STRIKE ZONE"),
            (ROW_VOLUME, "VOLUME"),
            (ROW_CLOSE, "CLOSE"),
        ] {
            assert_eq!(ROW_LABELS[row], want, "row {row} label drifted");
        }
        assert_eq!(ROW_LABELS.len(), ROW_CLOSE + 1, "CLOSE must stay last");
    }

    #[test]
    fn touch_row_cycles_and_close_row_closes() {
        use crate::game::settings::{BattingStyle, TouchScheme};
        let mut settings = Settings::default();
        let mut open = SettingsOpen(true);
        // Row 2 is P1 TOUCH SWING: forward cycle walks the schemes.
        cycle_row(ROW_TOUCH, true, false, false, &mut settings, &mut open);
        assert_eq!(settings.touch_scheme, TouchScheme::Tap);
        assert!(open.0, "editing a value row must not close the screen");
        // While the touch scheme owns P1's style (the caller-threaded
        // verdict), the row is dormant: cycling it must not invisibly
        // rewrite the stored (hidden) value.
        cycle_row(ROW_P1_STYLE, true, false, true, &mut settings, &mut open);
        assert_eq!(settings.batting_style[0], BattingStyle::ClassicTiming);
        // Backward from Off wraps to the last scheme.
        settings.touch_scheme = TouchScheme::Off;
        cycle_row(ROW_TOUCH, false, false, false, &mut settings, &mut open);
        assert_eq!(settings.touch_scheme, TouchScheme::ZonePad);
        // With no override in force the style row edits normally again.
        settings.touch_scheme = TouchScheme::Off;
        cycle_row(ROW_P1_STYLE, true, false, false, &mut settings, &mut open);
        assert_eq!(settings.batting_style[0], BattingStyle::SwingMeter);
        // The tap path (wrap_volume) wraps full volume to mute so touch can
        // reach every level; the keyboard path clamps at full instead.
        settings.volume = 1.0;
        cycle_row(ROW_VOLUME, true, true, false, &mut settings, &mut open);
        assert!(settings.volume.abs() < f32::EPSILON, "tap wraps full→mute");
        cycle_row(ROW_VOLUME, false, true, false, &mut settings, &mut open);
        assert!(settings.volume.abs() < f32::EPSILON, "Left floors at zero");
        settings.volume = 1.0;
        cycle_row(ROW_VOLUME, true, false, false, &mut settings, &mut open);
        assert!(
            (settings.volume - 1.0).abs() < f32::EPSILON,
            "keyboard Right clamps at full, never surprise-mutes"
        );
        assert!(open.0, "volume edits must not close the screen");
        // The CLOSE pseudo-row closes instead of editing.
        cycle_row(ROW_CLOSE, true, false, false, &mut settings, &mut open);
        assert!(!open.0);
    }

    #[test]
    fn edit_settings_cycles_style_and_clamps_volume() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(SettingsOpen(true))
            .init_resource::<SettingsCursorRow>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<Controllers>()
            .insert_resource(Settings {
                volume: 0.95,
                ..Default::default()
            })
            .add_systems(Update, edit_settings);

        // Row 0 (P1 style): Right cycles forward.
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ArrowRight);
        app.update();
        assert_eq!(
            app.world().resource::<Settings>().batting_style[0],
            BattingStyle::SwingMeter
        );

        // Move to the volume row (second-to-last — the CLOSE pseudo-row sits
        // beneath it) and push past the clamp. `press` only re-marks
        // `just_pressed` after a `release` — a held key doesn't repeat — so
        // each tap is release-then-press.
        let mut kb = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        kb.release(KeyCode::ArrowRight);
        for _ in 0..ROW_LABELS.len() - 2 {
            let mut kb = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            kb.press(KeyCode::ArrowDown);
            app.update();
            let mut kb = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            kb.release(KeyCode::ArrowDown);
        }
        assert_eq!(
            app.world().resource::<SettingsCursorRow>().0,
            ROW_LABELS.len() - 2
        );

        let mut kb = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        kb.release(KeyCode::ArrowDown);
        kb.press(KeyCode::ArrowRight);
        app.update();
        assert!((app.world().resource::<Settings>().volume - 1.0).abs() < f32::EPSILON);
    }
}
