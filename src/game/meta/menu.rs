//! Main menu and game-over screens.
//!
//! The menu gates entry into [`GameState::Playing`]: the player picks a mode
//! (1 player vs CPU, or 2 players), a field variant (**F**), and a theme
//! (**T**). Styling comes entirely from the active [`Theme`]; cycling either
//! option rebuilds the menu so the new look/labels show immediately.
//!
//! Every line is also a row under a focus cursor ([`MenuCursor`]), so a
//! controller drives the menu the console way: D-pad / stick moves, **A**
//! chooses, Left/Right change an option, **Start** plays, **Select** opens
//! settings and **B** backs out of them. The hints and hotkey labels follow
//! the [`ActiveDevice`], so each player reads only their own controls.

use bevy::color::Alpha;
use bevy::prelude::*;

use crate::game::input::{ActiveDevice, Controllers, StickLatch, assign_controllers, menu_nav};
use crate::game::settings::{Settings, SettingsOpen};
use crate::game::theme::Theme;
use crate::game::ui::{OverlayPaint, overlay_card, overlay_root, z};
use crate::game::variant::{self, FieldSpec, Ruleset};
use crate::game::{GameConfig, GameMode, GameState, ScoreBoard};

/// Marker for menu-screen UI so it can be torn down on exit or rebuild.
#[derive(Component)]
struct MenuUi;

/// What tapping (or clicking) a menu line does — every line a hotkey drives
/// is also a [`Button`], so a touchscreen can run the whole menu (the same
/// `Interaction` path serves the mouse for free).
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum MenuAction {
    Start(GameMode),
    Cycle(CycleAction),
    OpenSettings,
}

/// A menu line's position under the focus cursor, stamped in build order —
/// a line added to the menu becomes a cursor stop with no extra wiring.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
struct MenuRow(usize);

/// Which [`MenuRow`] the controller/keyboard cursor rests on. Kept across
/// rebuilds and games, so after a game **A** replays the mode just played.
#[derive(Resource, Default)]
struct MenuCursor(usize);

/// One cyclable menu option. Separate from [`MenuAction`] so
/// [`apply_cycle`] can match exhaustively — adding an option without wiring
/// its cycle arm is a compile error, not a silently dead tap.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CycleAction {
    Field,
    Innings,
    Theme,
    Touch,
    Difficulty,
}

/// Public handle for the menu's `Update` chain so `SettingsPlugin` can order
/// its own hotkey systems after it: with the menu reading first, a gamepad
/// press that changes whether the settings screen is open is read by
/// exactly one of the two screens in a frame (the ambiguous ordering used
/// to decide that per schedule build).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct MenuUpdateSet;

/// The line that shows how many controllers are connected.
#[derive(Component)]
struct ControllerStatus;

/// Marker for game-over UI.
#[derive(Component)]
struct GameOverUi;

/// The score card itself — the pointer-dismiss target ([`game_over_restart`]
/// hit-tests it): a tap or click must land ON the card, not anywhere on
/// screen, or a routine refocus click (alt-tab back, click the canvas)
/// dismissed the very card the player came back to read.
#[derive(Component)]
struct GameOverCard;

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        // No OnEnter spawn: `rebuild_menu_on_change` builds the missing tree
        // on the first Update frame — one build path, not two (an OnEnter
        // spawn was immediately torn down by the change-tick catch-up
        // rebuild on the same entry).
        app.init_resource::<MenuCursor>()
            .add_systems(OnExit(GameState::MainMenu), despawn::<MenuUi>)
            .add_systems(
                Update,
                (
                    // S must not fight T/F/I or the mode-select keys while
                    // the settings screen is open — and taps must not reach
                    // the menu through the settings overlay. The chain makes
                    // same-frame hotkey+tap deterministic and lets the one
                    // rebuild system run after every mutator.
                    cycle_options.run_if(crate::game::settings::settings_closed),
                    menu_select.run_if(crate::game::settings::settings_closed),
                    menu_navigate.run_if(crate::game::settings::settings_closed),
                    // NOT run_if-gated: a skipped system's change ticks
                    // freeze, so a press held through the settings overlay
                    // would fire the frame the screen closed. Running every
                    // frame keeps `Changed<Interaction>` current; the
                    // in-body open check discards overlay-covered presses.
                    menu_tap,
                    // Gated closed: rebuild-per-edit under the covering
                    // overlay was invisible work, and a skipped system's
                    // frozen ticks make the catch-up fire exactly once on
                    // the close frame — the deferred rebuild for free.
                    rebuild_menu_on_change.run_if(crate::game::settings::settings_closed),
                    // AFTER the rebuild (commands flush between chained
                    // systems), so a freshly built tree's status line is
                    // painted the same frame instead of rendering blank
                    // once — the guarantee the old OnEnter spawn provided.
                    update_controller_status,
                )
                    .chain()
                    .in_set(MenuUpdateSet)
                    .run_if(in_state(GameState::MainMenu)),
            )
            .add_systems(OnEnter(GameState::GameOver), spawn_game_over)
            .add_systems(OnExit(GameState::GameOver), despawn::<GameOverUi>)
            .add_systems(
                Update,
                game_over_restart.run_if(in_state(GameState::GameOver)),
            );
    }
}

// ── Main menu ─────────────────────────────────────────────────────────────────

/// Builds the full menu tree. Called by [`rebuild_menu_on_change`] when the
/// tree is missing (menu entry) and whenever a displayed value changes (the
/// old tree is despawned first).
fn build_menu(
    commands: &mut Commands,
    config: &GameConfig,
    theme: &Theme,
    settings: &Settings,
    cursor: usize,
    device: ActiveDevice,
) {
    let ui = &theme.ui;
    // Rows are numbered in spawn order; `focused` marks the cursor's row.
    let mut rows = (0..).map(MenuRow);
    let focused = |row: MenuRow| device != ActiveDevice::Touch && row.0 == cursor;

    commands
        .spawn((
            MenuUi,
            // Rebuilt on every press, so it exists only while visible —
            // `Opaque` chrome (see `ui::OverlayPaint`).
            overlay_root(z::MENU, OverlayPaint::Opaque, ui),
        ))
        .with_children(|screen| {
            screen
                .spawn(overlay_card(
                    OverlayPaint::Opaque,
                    Vec2::new(44.0, 30.0),
                    13.0,
                    ui,
                ))
                .with_children(|card| {
                    card.spawn((
                        Text::new("BREAKNECK BASEBALL"),
                        TextFont {
                            font_size: 48.0,
                            ..default()
                        },
                        TextColor(ui.accent),
                    ));
                    card.spawn((
                        Text::new("backyard arcade baseball"),
                        TextFont {
                            font_size: 15.0,
                            ..default()
                        },
                        TextColor(ui.text_dim),
                        Node {
                            margin: UiRect::bottom(Val::Px(10.0)),
                            ..default()
                        },
                    ));

                    for (line, action) in [
                        (
                            "1   One Player  (vs CPU)",
                            MenuAction::Start(GameMode::OnePlayer),
                        ),
                        ("2   Two Players", MenuAction::Start(GameMode::TwoPlayers)),
                    ] {
                        let row = rows.next().expect("unbounded row counter");
                        card.spawn((
                            Button,
                            action,
                            row,
                            Text::new(row_label(line, focused(row), device)),
                            TextFont {
                                font_size: 23.0,
                                ..default()
                            },
                            TextColor(if focused(row) {
                                ui.accent
                            } else {
                                ui.text_primary
                            }),
                        ));
                    }

                    // Option lines: dim key/label, accent value. Each is a
                    // Button so touch (and mouse) can cycle it directly.
                    for (label, value, action) in [
                        (
                            "F   Field",
                            config.variant.label().to_string(),
                            MenuAction::Cycle(CycleAction::Field),
                        ),
                        (
                            "I   Innings",
                            config.innings.to_string(),
                            MenuAction::Cycle(CycleAction::Innings),
                        ),
                        (
                            "T   Theme",
                            config.theme.label().to_string(),
                            MenuAction::Cycle(CycleAction::Theme),
                        ),
                        (
                            "D   CPU difficulty",
                            settings.difficulty.label().to_string(),
                            MenuAction::Cycle(CycleAction::Difficulty),
                        ),
                        (
                            "G   Touch swing",
                            settings.touch_scheme.label().to_string(),
                            MenuAction::Cycle(CycleAction::Touch),
                        ),
                        ("S   Settings", "open".to_string(), MenuAction::OpenSettings),
                    ] {
                        let row = rows.next().expect("unbounded row counter");
                        card.spawn((
                            Button,
                            action,
                            row,
                            Text::new(format!("{}   ", row_label(label, focused(row), device))),
                            TextFont {
                                font_size: 19.0,
                                ..default()
                            },
                            TextColor(if focused(row) {
                                ui.text_primary
                            } else {
                                ui.text_dim
                            }),
                        ))
                        .with_child((
                            TextSpan::new(value),
                            TextFont {
                                font_size: 19.0,
                                ..default()
                            },
                            TextColor(ui.accent),
                        ));
                    }

                    card.spawn((
                        ControllerStatus,
                        Text::new(""),
                        TextFont {
                            font_size: 15.0,
                            ..default()
                        },
                        TextColor(ui.count_ball),
                        Node {
                            margin: UiRect::top(Val::Px(10.0)),
                            ..default()
                        },
                    ));
                    card.spawn((
                        Text::new(controls_hint(device)),
                        TextFont {
                            // 15 px primary, not 13 px dim: this block is
                            // the only place the controls are documented
                            // before play (TODO 75).
                            font_size: 15.0,
                            ..default()
                        },
                        TextColor(ui.text_primary),
                        TextLayout::new_with_justify(Justify::Center),
                    ));
                });

            // Build identifier so bug reports can name a version.
            screen.spawn((
                Text::new(concat!("v", env!("CARGO_PKG_VERSION"))),
                TextFont {
                    font_size: 12.0,
                    ..default()
                },
                TextColor(ui.text_dim),
                Node {
                    position_type: PositionType::Absolute,
                    right: Val::Px(10.0),
                    bottom: Val::Px(8.0),
                    ..default()
                },
            ));
        });
}

/// A row's label for the active device: the leading hotkey column ("F   ")
/// is a keyboard instruction, so other devices drop it; a cursor marker
/// pads every row when a cursor is shown (the default font is monospace,
/// so the marker never shifts the column).
fn row_label(label: &str, focused: bool, device: ActiveDevice) -> String {
    let text = match device {
        ActiveDevice::Keyboard => label,
        _ => label.split_once("   ").map_or(label, |(_, rest)| rest),
    };
    match (device, focused) {
        (ActiveDevice::Touch, _) => text.to_string(),
        (_, true) => format!("> {text}"),
        (_, false) => format!("  {text}"),
    }
}

/// The control summary for the active device only — this block is the one
/// place the controls are documented before play (TODO 75).
fn controls_hint(device: ActiveDevice) -> String {
    const RUNNER: &str = "Batting: hold Down through the windup to send the runner";
    match device {
        ActiveDevice::Gamepad => format!(
            "D-pad / stick: move   A: choose   Left/Right: change\nStart: play   Select: settings   B: back\nIn play: stick to aim, A to pitch or swing\n{RUNNER}"
        ),
        ActiveDevice::Keyboard => format!(
            "1 / 2: play   F I T D G: change options   S: settings\nArrows + Enter also move and choose\nIn play: WASD + Space (P1), Arrows + Right-Ctrl (P2)\n{RUNNER}{}",
            creator_hint()
        ),
        ActiveDevice::Touch => {
            "Tap a line to choose it or change its value\nTouch swing picks how you swing on screen"
                .to_string()
        }
    }
}

/// Dev-only hint line for the Creator stage (`--features debug`), appended to
/// the menu's control summary.
#[cfg(feature = "debug")]
fn creator_hint() -> &'static str {
    "\nC — player creator (dev)"
}

#[cfg(not(feature = "debug"))]
fn creator_hint() -> &'static str {
    ""
}

fn update_controller_status(
    pads: Query<(), With<Gamepad>>,
    mut query: Query<&mut Text, With<ControllerStatus>>,
) {
    // The two common counts are static strings, so menu frames stay
    // allocation-free and `set_text_if_neq` guards the write (glyph
    // re-shaping is the expensive part); only n >= 2 formats. Derived per
    // frame, no caches: a `Local` count silently survived menu rebuilds,
    // and its `Added`-repair query shipped a B0001 on first authoring —
    // the derived form has nothing to invalidate.
    let msg: std::borrow::Cow<'static, str> = match pads.iter().count() {
        0 => "No controllers detected - keyboard fallback active".into(),
        1 => "1 controller connected".into(),
        n => format!("{n} controllers connected").into(),
    };
    for mut text in &mut query {
        crate::game::ui::set_text_if_neq(&mut text, &msg);
    }
}

/// Applies one option cycle. Shared by the hotkey, cursor and tap systems
/// so the input paths can never drift apart.
fn apply_cycle(
    action: CycleAction,
    config: &mut ResMut<GameConfig>,
    theme: &mut ResMut<Theme>,
    settings: &mut ResMut<Settings>,
) {
    // Taking the `ResMut`s (not `&mut T`) keeps change detection honest:
    // only the resource the matched arm actually dereferences is marked
    // changed — a Field press must not persist the settings store or retint
    // the theme's materials.
    match action {
        CycleAction::Field => {
            config.variant = config.variant.next();
            // A new park brings its own regulation length; I re-cycles from there.
            config.innings = config.variant.rules().counts.innings;
        }
        CycleAction::Innings => config.innings = variant::next_innings(config.innings),
        CycleAction::Theme => {
            config.theme = config.theme.next();
            **theme = config.theme.build();
        }
        CycleAction::Touch => settings.touch_scheme = settings.touch_scheme.next(),
        CycleAction::Difficulty => settings.difficulty = settings.difficulty.next(),
    }
}

/// The one build/rebuild path: builds the tree when it's missing (menu
/// entry — there is no OnEnter spawn), and tears it down and rebuilds it
/// whenever anything the menu displays changed — straight off the displayed
/// resources' own change ticks, so a new mutator (a settings-screen row, a
/// future hotkey) can never forget to flag a hand-rolled dirty bit. The
/// missing-tree trigger, not tick catch-up, is what makes re-entry correct:
/// this system's ticks advance every frame it runs, so an untouched game
/// leaves nothing "changed" by the next menu. Chained last (see the
/// plugin), so a hotkey and a tap landing on the same frame still trigger
/// exactly one rebuild. Settings edits made while the overlay covers the
/// menu rebuild as they happen — invisible, and the user-action cost class
/// the menu already accepts per press.
#[allow(clippy::too_many_arguments)]
fn rebuild_menu_on_change(
    menu_q: Query<Entity, With<MenuUi>>,
    next_state: Res<NextState<GameState>>,
    config: Res<GameConfig>,
    theme: Res<Theme>,
    settings: Res<Settings>,
    cursor: Res<MenuCursor>,
    device: Res<ActiveDevice>,
    mut commands: Commands,
) {
    // The missing-tree build is unconditional — even a pending transition
    // must not suppress it (a bailed/re-decided launch would otherwise
    // leave a blank screen with ticks already advanced past the changes,
    // so nothing would ever rebuild). The pending bail only skips the
    // pointless *re*build on a launch frame (`StartGame::start` writes
    // `config.mode`, which reads as a display change while the whole tree
    // is about to be torn down by OnExit).
    let missing = menu_q.is_empty();
    if !missing
        && (crate::game::transition_pending(&next_state)
            || !(config.is_changed()
                || theme.is_changed()
                || settings.is_changed()
                || cursor.is_changed()
                || device.is_changed()))
    {
        return;
    }
    crate::game::despawn_all(&mut commands, &menu_q);
    build_menu(&mut commands, &config, &theme, &settings, cursor.0, *device);
}

/// Everything launching a game mutates, bundled so `menu_select` and
/// `menu_tap` share one signature and a new launch dependency touches one
/// struct instead of five loose parameters at three sites.
#[derive(bevy::ecs::system::SystemParam)]
struct StartGame<'w, 's> {
    config: ResMut<'w, GameConfig>,
    controllers: ResMut<'w, Controllers>,
    rules: ResMut<'w, Ruleset>,
    field: ResMut<'w, FieldSpec>,
    next_state: ResMut<'w, NextState<GameState>>,
    pads: Query<'w, 's, Entity, With<Gamepad>>,
}

impl StartGame<'_, '_> {
    /// Locks in a mode and launches the game. Shared by key/pad select and
    /// taps — including how pads are enumerated, so the two start paths can
    /// never assign controllers differently.
    fn start(&mut self, mode: GameMode) {
        // Two launchers share this seam (keys and taps run in one chain): a
        // second same-frame call must not override the mode the first
        // chose, reassigning controllers to a game nobody selected.
        if crate::game::transition_pending(&self.next_state) {
            return;
        }
        let pad_entities: Vec<Entity> = self.pads.iter().collect();
        self.config.mode = mode;
        *self.controllers = assign_controllers(mode, &pad_entities);
        // Materialize the chosen variant so every gameplay system reads this
        // game's rules and park; the menu's game-length choice overrides the
        // variant's default.
        *self.rules = self.config.variant.rules();
        self.rules.counts.innings = self.config.innings;
        *self.field = self.config.variant.field();
        self.next_state.set(GameState::Playing);
    }
}

/// Cycles the field (**F**), innings (**I**), theme (**T**), CPU
/// difficulty (**D**) and touch swing scheme (**G**); the rebuild system picks the edit up off the
/// resource's change tick. Keyboard only: a pad changes options through the
/// cursor ([`menu_navigate`]), so its face buttons keep their conventional
/// roles instead of each owning an option.
fn cycle_options(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut config: ResMut<GameConfig>,
    mut theme: ResMut<Theme>,
    mut settings: ResMut<Settings>,
) {
    // One row per hotkey: keyboard key, the cycle.
    const HOTKEYS: [(KeyCode, CycleAction); 5] = [
        (KeyCode::KeyF, CycleAction::Field),
        (KeyCode::KeyI, CycleAction::Innings),
        (KeyCode::KeyT, CycleAction::Theme),
        (KeyCode::KeyD, CycleAction::Difficulty),
        (KeyCode::KeyG, CycleAction::Touch),
    ];
    for (key, action) in HOTKEYS {
        if keyboard.just_pressed(key) {
            apply_cycle(action, &mut config, &mut theme, &mut settings);
        }
    }
}

/// The keyboard's direct launch keys (**1** / **2**). Pads launch through
/// the cursor ([`menu_navigate`]).
fn menu_select(keyboard: Res<ButtonInput<KeyCode>>, mut launch: StartGame) {
    let want_one =
        keyboard.just_pressed(KeyCode::Digit1) || keyboard.just_pressed(KeyCode::Numpad1);
    let want_two =
        keyboard.just_pressed(KeyCode::Digit2) || keyboard.just_pressed(KeyCode::Numpad2);

    let mode = if want_two {
        GameMode::TwoPlayers
    } else if want_one {
        GameMode::OnePlayer
    } else {
        return;
    };

    launch.start(mode);
}

/// The cursor path (gamepad, plus keyboard arrows/Enter): Up/Down move the
/// focus over the [`MenuRow`]s, wrapping; **A** / Enter performs the
/// focused row's [`MenuAction`]; Left/Right step a focused option (every
/// option only cycles forward, so both directions step it); **Start** plays
/// — the focused mode, or One Player from an option row. **B** has nothing
/// to back out of here; it closes the settings screen (`toggle_settings`).
#[allow(clippy::too_many_arguments)]
fn menu_navigate(
    keyboard: Res<ButtonInput<KeyCode>>,
    pads: Query<(Entity, &Gamepad)>,
    rows: Query<(&MenuRow, &MenuAction)>,
    mut latch: Local<StickLatch>,
    mut cursor: ResMut<MenuCursor>,
    mut theme: ResMut<Theme>,
    mut settings: ResMut<Settings>,
    mut open: ResMut<SettingsOpen>,
    mut settings_cursor: ResMut<crate::game::settings::SettingsCursorRow>,
    mut launch: StartGame,
) {
    // Read every frame (before any early-out) so the stick latch tracks.
    let nav = menu_nav(&keyboard, &pads, &mut latch);
    let start = pads
        .iter()
        .any(|(_, p)| p.just_pressed(GamepadButton::Start));
    // A launch already decided this frame (`menu_select`, earlier in the
    // chain) must not be followed by an option edit — see `menu_tap`.
    if !(nav.any() || start) || crate::game::transition_pending(&launch.next_state) {
        return;
    }
    let count = rows.iter().count();
    if count == 0 {
        return;
    }
    let mut row = cursor.0.min(count - 1);
    if nav.up {
        row = row.checked_sub(1).unwrap_or(count - 1);
    }
    if nav.down {
        row = (row + 1) % count;
    }
    // Guarded: the cursor's change tick triggers a menu rebuild.
    if cursor.0 != row {
        cursor.0 = row;
    }
    let Some(action) = rows.iter().find_map(|(r, a)| (r.0 == row).then_some(*a)) else {
        return;
    };
    if start {
        let mode = match action {
            MenuAction::Start(mode) => mode,
            _ => GameMode::OnePlayer,
        };
        launch.start(mode);
        return;
    }
    match action {
        MenuAction::Start(mode) if nav.confirm => launch.start(mode),
        MenuAction::OpenSettings if nav.confirm => {
            crate::game::settings::open_settings(&mut open, &mut settings_cursor);
        }
        MenuAction::Cycle(cycle) if nav.confirm || nav.left || nav.right => {
            apply_cycle(cycle, &mut launch.config, &mut theme, &mut settings);
        }
        _ => {}
    }
}

/// The touch/mouse path: a pressed menu line performs its [`MenuAction`].
/// Suppressed while the settings screen is open (see the plugin) so taps
/// can't reach the menu through the overlay.
#[allow(clippy::too_many_arguments)]
fn menu_tap(
    buttons: Query<(Entity, &MenuAction, &ComputedNode, &GlobalTransform), With<Button>>,
    interactions: Query<&Interaction, (Changed<Interaction>, With<Button>)>,
    touches: Res<Touches>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut theme: ResMut<Theme>,
    mut settings: ResMut<Settings>,
    mut open: ResMut<SettingsOpen>,
    mut cursor: ResMut<crate::game::settings::SettingsCursorRow>,
    mut launch: StartGame,
) {
    if open.0 {
        // The settings overlay is up: observe (and thereby consume) this
        // frame's interaction changes, but act on none of them.
        return;
    }
    // A launch already decided this frame (`menu_select` runs earlier in
    // the chain): a same-frame option tap must not mutate `GameConfig`
    // AFTER `StartGame::start` materialized the Ruleset/FieldSpec from it —
    // the game would play settings the menu no longer displays.
    if crate::game::transition_pending(&launch.next_state) {
        return;
    }
    // O(1) early-out on the no-input frame (in-body, so `Changed` ticks
    // stay fresh): the per-button lookups and rect math below can only act
    // when an edge or a pointer press exists.
    if interactions.is_empty() && !crate::game::input::pointer_pressed(&touches, &mouse) {
        return;
    }
    for (entity, action, node, transform) in &buttons {
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
        match *action {
            MenuAction::Start(mode) => {
                launch.start(mode);
                return;
            }
            MenuAction::OpenSettings => {
                // The one open transition (parks, then opens) — this path
                // runs BEFORE the settings chain, so `toggle_settings`'
                // while-closed park never sees a tap-open frame.
                crate::game::settings::open_settings(&mut open, &mut cursor);
                // Stop — a second same-frame press (two fingers) must not
                // cycle an option or start a game under the overlay that
                // just opened (`tap_settings_row` stops on close the same
                // way).
                return;
            }
            MenuAction::Cycle(cycle) => {
                apply_cycle(cycle, &mut launch.config, &mut theme, &mut settings);
                // Stop like the arms above: ONE activation per frame is
                // this system's rule (`Start` and `OpenSettings` already
                // said so), and falling through made a multi-touch frame
                // apply several actions in query — i.e. spawn — order.
                // Cycling Field also rewrites innings from the new park,
                // so a stray second hit changed a value nobody chose.
                return;
            }
        }
    }
}

// ── Game over ─────────────────────────────────────────────────────────────────

fn spawn_game_over(
    mut commands: Commands,
    time: Res<Time<Real>>,
    score: Res<ScoreBoard>,
    theme: Res<Theme>,
    device: Res<ActiveDevice>,
) {
    // Armed alongside the card it guards, so the two can't drift apart.
    commands.insert_resource(GameOverGrace(time.elapsed_secs()));
    let ui = &theme.ui;
    let (winner, color) = if score.home_runs > score.away_runs {
        ("HOME WINS", theme.home.jersey)
    } else if score.away_runs > score.home_runs {
        ("AWAY WINS", theme.away.jersey)
    } else {
        ("TIE GAME", ui.text_primary)
    };

    commands
        .spawn((
            GameOverUi,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(ui.panel_bg.with_alpha(0.92)),
        ))
        .with_children(|screen| {
            screen
                .spawn((
                    GameOverCard,
                    Node {
                        padding: UiRect::axes(Val::Px(50.0), Val::Px(34.0)),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        row_gap: Val::Px(14.0),
                        border: UiRect::all(Val::Px(1.5)),
                        ..default()
                    },
                    BackgroundColor(ui.panel_bg),
                    BorderColor::all(ui.panel_border),
                    BorderRadius::all(Val::Px(16.0)),
                ))
                .with_children(|card| {
                    card.spawn((
                        Text::new(winner),
                        TextFont {
                            font_size: 52.0,
                            ..default()
                        },
                        TextColor(color),
                    ));
                    card.spawn((
                        Text::new(format!(
                            "Final   AWAY {}  -  HOME {}",
                            score.away_runs, score.home_runs
                        )),
                        TextFont {
                            font_size: 28.0,
                            ..default()
                        },
                        TextColor(ui.text_primary),
                    ));
                    card.spawn((
                        Text::new(match *device {
                            ActiveDevice::Keyboard => "Enter  -  back to the menu",
                            ActiveDevice::Gamepad => "A  -  back to the menu",
                            ActiveDevice::Touch => "Tap the card  -  back to the menu",
                        }),
                        TextFont {
                            font_size: 17.0,
                            ..default()
                        },
                        TextColor(ui.text_dim),
                    ));
                });
        });
}

/// Guards the final score card against being dismissed by the tail of an
/// input mash: the swing button (Space, pad A, and every tap under the Tap
/// scheme) is also a restart key, so the last pitch's inputs would otherwise
/// skip the card before it could be read. A timestamp (game-over instant),
/// not a timer: nothing to tick, so nothing to dirty. Stamped and compared
/// on the REAL clock, so no juice writer that leaks `relative_speed` past
/// `OnExit(Playing)` can ever stretch the lockout.
#[derive(Resource)]
struct GameOverGrace(f32);

/// How long the card ignores the input tail, in seconds.
const GAME_OVER_GRACE_SECS: f32 = 1.0;

#[allow(clippy::too_many_arguments)]
fn game_over_restart(
    time: Res<Time<Real>>,
    grace: Res<GameOverGrace>,
    keyboard: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    touches: Res<Touches>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    card: Query<(&ComputedNode, &GlobalTransform), With<GameOverCard>>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    if time.elapsed_secs() - grace.0 < GAME_OVER_GRACE_SECS {
        return;
    }
    // Pointer dismissal must land ON the card (see [`GameOverCard`]).
    // Lazy (a closure, short-circuited by the keyboard/pad arms), and
    // `pointer_on_node` alone: it already requires a just-pressed touch or
    // click, so a separate `pointer_pressed` pre-check would be a second
    // spelling of the same press condition — two encodings that can drift.
    let pointer_on_card = || {
        card.single().is_ok_and(|(node, transform)| {
            crate::game::input::pointer_on_node(
                &touches,
                &mouse,
                windows.single().ok(),
                node,
                transform,
            )
        })
    };
    let confirm = keyboard.just_pressed(KeyCode::Enter)
        || keyboard.just_pressed(KeyCode::Space)
        || pads.iter().any(|p| p.just_pressed(GamepadButton::South))
        || pointer_on_card();
    if confirm {
        next_state.set(GameState::MainMenu);
    }
}

// ── Shared ────────────────────────────────────────────────────────────────────

/// Generic despawn-by-marker used on state exit.
fn despawn<T: Component>(mut commands: Commands, query: Query<Entity, With<T>>) {
    crate::game::despawn_all(&mut commands, &query);
}

#[cfg(test)]
#[path = "menu.test.rs"]
mod tests;
