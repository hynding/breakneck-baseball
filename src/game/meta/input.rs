//! Input abstraction.
//!
//! Every gameplay system reads intent from a single normalized [`Intents`]
//! resource instead of touching keyboards or gamepads directly. Each team's
//! [`TeamIntent`] is refreshed every frame from whatever [`InputSource`] is
//! assigned to that team (a game controller, a keyboard scheme, or the CPU).
//!
//! This is what lets pitching/batting code run identically for a human and the
//! AI: the CPU systems (see `game::flow`/`game::player`) simply write into the same
//! [`TeamIntent`] the human input would have produced.

use bevy::input::gamepad::GamepadConnectionEvent;
use bevy::prelude::*;

use crate::game::Team;

// ── Per-team intent ───────────────────────────────────────────────────────────

/// Normalized input for one team for the current frame.
///
/// Meaning depends on whether the team is on offense or defense:
/// - **Defense (pitching):** `aim` steers where the pitch crosses the plate,
///   `action` releases the pitch.
/// - **Offense (batting):** `aim` steers the swing direction (pull/center/oppo),
///   `action` swings.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct TeamIntent {
    /// Directional aim, components in −1.0..=1.0.
    pub aim: Vec2,
    /// Primary button was pressed this frame (pitch release / swing).
    pub action: bool,
    /// Primary button is currently held (the Swing Meter's load state —
    /// `action` is the edge, this is the level).
    pub action_held: bool,
    /// Absolute zone-plane cursor (world x / height, meters), for devices
    /// that aim by position rather than velocity — today the Zone Pad touch
    /// scheme. `None` for sticks/keys; the PCI adapter snaps to it when set.
    pub cursor: Option<Vec2>,
}

impl TeamIntent {
    /// Folds a preferred device's contribution over this one: its aim wins
    /// when nonzero (a deflected stick or a deliberate finger beats resting
    /// keys), edges and holds OR together, and its cursor wins when set.
    /// The one merge policy — used for pad-over-keyboard and touch-over-
    /// keyboard alike, so the two layerings can never drift.
    pub fn overlay(&mut self, preferred: &TeamIntent) {
        if preferred.aim != Vec2::ZERO {
            self.aim = preferred.aim;
        }
        self.action |= preferred.action;
        self.action_held |= preferred.action_held;
        self.cursor = preferred.cursor.or(self.cursor);
    }
}

/// This frame's normalized touch contribution — a [`TeamIntent`] (the same
/// channels every device produces; a new channel added there reaches touch
/// automatically), written by `touch::read_touch` and merged into the
/// touch-owned slot by [`gather_intents`] via [`TeamIntent::overlay`]. Owned
/// here (the seam that consumes it) so the input plugin stands alone; the
/// touch module re-exports it. Neutral when no window exists, no slot is
/// touch-owned, or the game isn't in `Playing` — but NOT under
/// `TouchScheme::Off`, whose generic mapping (drag = aim, tap = action)
/// stays live so a scheme-less touch device is always playable.
#[derive(Resource, Default, Clone, Copy, Debug, PartialEq)]
pub struct TouchIntent(pub TeamIntent);

/// Normalized intent for both teams, rebuilt every frame.
#[derive(Resource, Default, Debug)]
pub struct Intents {
    pub home: TeamIntent,
    pub away: TeamIntent,
}

impl Intents {
    /// Intent for the given team.
    pub fn get(&self, team: Team) -> TeamIntent {
        match team {
            Team::Home => self.home,
            Team::Away => self.away,
        }
    }

    /// Mutable intent for the given team (used by CPU systems to inject input).
    pub fn get_mut(&mut self, team: Team) -> &mut TeamIntent {
        match team {
            Team::Home => &mut self.home,
            Team::Away => &mut self.away,
        }
    }
}

// ── Input sources ─────────────────────────────────────────────────────────────

/// Two keyboard layouts so two people can share one keyboard when controllers
/// are unavailable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyScheme {
    /// WASD + Space.
    Primary,
    /// Arrow keys + Right-Control.
    Secondary,
}

impl KeyScheme {
    /// The team's half of a shared keyboard: Home = Primary (WASD + Space),
    /// Away = Secondary (arrows + Right-Ctrl). The ONE team→scheme mapping
    /// — the pad-owned slot's keyboard fill, hotplug re-assignment, the
    /// Director's pseudo-human routing, and `assign_controllers` all
    /// consult it, so a future swap-sides option can't give a slot the
    /// other player's keys at one forgotten copy.
    pub fn for_team(team: Team) -> Self {
        match team {
            Team::Home => KeyScheme::Primary,
            Team::Away => KeyScheme::Secondary,
        }
    }
}

/// Where a team's input comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputSource {
    /// A connected game controller (the gamepad entity).
    Gamepad(Entity),
    /// A keyboard layout.
    Keyboard(KeyScheme),
    /// Driven by the AI. Intent is written by CPU systems, not this module.
    Cpu,
}

/// Which input source drives each team. Chosen at game start from the selected
/// mode and the set of connected controllers, and updated on hotplug.
#[derive(Resource, Debug)]
pub struct Controllers {
    pub home: InputSource,
    pub away: InputSource,
    /// Which team the touchscreen currently drives — resolved each frame by
    /// `touch::resolve_touch_owner` from the slot layout *and* whether a
    /// Director owns the slot at runtime (a directed pseudo-human slot is
    /// never touch-owned: its configured batting style must hold, in every
    /// build). `None` when no slot is touch-driven. Living on the device
    /// model means `style_for`, `gather_intents`, and the overlay all read
    /// ownership from the struct they already hold — nothing threads it.
    pub touch_team: Option<Team>,
}

impl Default for Controllers {
    fn default() -> Self {
        // Sensible default before a mode is chosen: single keyboard vs CPU.
        Self {
            home: InputSource::Keyboard(KeyScheme::for_team(Team::Home)),
            away: InputSource::Cpu,
            touch_team: None,
        }
    }
}

impl Controllers {
    pub fn source(&self, team: Team) -> InputSource {
        match team {
            Team::Home => self.home,
            Team::Away => self.away,
        }
    }

    /// Which settings slot (P1 = 0, P2 = 1) drives this team, or `None` for
    /// the CPU. Home is always P1's team when human; Away is P2's (or the
    /// solo player's opponent).
    pub fn player_index(&self, team: Team) -> Option<usize> {
        match (team, self.source(team)) {
            (_, InputSource::Cpu) => None,
            (Team::Home, _) => Some(0),
            (Team::Away, _) => Some(1),
        }
    }
}

// ── Plugin ────────────────────────────────────────────────────────────────────

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Intents>()
            .init_resource::<Controllers>()
            // Owned and initialized here (the seam that consumes it); the
            // touch module is its writer.
            .init_resource::<TouchIntent>()
            // Rebuild intents early each frame so all gameplay systems see them.
            .add_systems(PreUpdate, gather_intents)
            // Hotplug rewrites `Controllers`, which the AI and fielding read
            // — pinned first in the gameplay pipeline so the trajectory never
            // depends on an ambiguity tie-break (see `GameplayOrder`).
            .add_systems(
                Update,
                handle_gamepad_hotplug.in_set(crate::game::GameplayOrder::Input),
            );
    }
}

// ── Systems ───────────────────────────────────────────────────────────────────

/// Rebuilds [`Intents`] for both human-driven teams. CPU teams are left at their
/// current value so CPU systems (which run later) can populate them.
///
/// The touchscreen ([`TouchIntent`], produced just before this system)
/// rides the touch-owned slot (`Controllers::touch_team`, resolved by
/// `touch::resolve_touch_owner`) the way the keyboard rides a pad-owned
/// slot: merged in, winning the aim only when it says something.
pub(crate) fn gather_intents(
    controllers: Res<Controllers>,
    keyboard: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    touch: Res<TouchIntent>,
    mut intents: ResMut<Intents>,
) {
    let touch_team = controllers.touch_team;
    for team in [Team::Home, Team::Away] {
        let (mut intent, pad_intent) = match controllers.source(team) {
            // CPU intents are written by AI systems; don't clobber them here.
            InputSource::Cpu => continue,
            InputSource::Keyboard(scheme) => (keyboard_intent(&keyboard, scheme), None),
            InputSource::Gamepad(entity) => {
                // The keyboard always stays live under a pad-owned slot
                // (TODO 72 — a plugged-in-but-idle controller used to make
                // the advertised keys completely dead, and a vanished pad
                // must not either): keyboard base, pad layered when present.
                let scheme = KeyScheme::for_team(team);
                (
                    keyboard_intent(&keyboard, scheme),
                    gamepads.get(entity).ok().map(gamepad_intent),
                )
            }
        };
        // One overlay sequence for every human source: touch rides the
        // touch-owned slot, then the pad (when speaking) wins over both.
        if touch_team == Some(team) {
            intent.overlay(&touch.0);
        }
        if let Some(pad_intent) = pad_intent {
            intent.overlay(&pad_intent);
        }
        *intents.get_mut(team) = intent;
    }
}

/// Whether any of this frame's touchdowns landed inside a laid-out UI
/// node's rect — the raw sibling of the `Interaction` path, for targets
/// that must accept instantaneous taps (down+up in one frame batch never
/// becomes `Pressed`) and second-finger taps (bevy_ui attributes
/// multi-touch presses to the FIRST held finger). One encoding of the
/// physical-layout→logical-touch math, so a DPI or Bevy-upgrade fix can't
/// reach one hit-test site and miss the other.
pub(crate) fn touched_node(
    touches: &Touches,
    node: &ComputedNode,
    transform: &GlobalTransform,
) -> bool {
    touched_node_ids(touches, node, transform).next().is_some()
}

/// [`touched_node`]'s id-yielding form, for callers that must also act on
/// the fingers (the quit row binds them into the chrome registry) —
/// layered on [`touched_rect_ids`] so the containment scan has ONE body.
pub(crate) fn touched_node_ids<'a>(
    touches: &'a Touches,
    node: &ComputedNode,
    transform: &GlobalTransform,
) -> impl Iterator<Item = u64> + 'a {
    touched_rect_ids(touches, node_screen_rect(node, transform))
}

/// The containment scan's rect-space core — ONE body, so a DPI or upgrade
/// fix to `Touch::position` handling reaches every hit-test site: the
/// node-based tests above AND the translator's hand-computed chrome rect
/// (`touch::claim_pause_region_taps`), whose drawn and claimed regions
/// must never disagree with the node surfaces'.
pub(crate) fn touched_rect_ids(touches: &Touches, rect: Rect) -> impl Iterator<Item = u64> + '_ {
    touches
        .iter_just_pressed()
        .filter(move |t| rect.contains(t.position()))
        .map(|t| t.id())
}

/// A laid-out UI node's on-screen rect in LOGICAL pixels (the space touch
/// positions and the window cursor report in) — the one encoding of the
/// physical-layout conversion, shared by the touch and mouse hit-tests.
pub(crate) fn node_screen_rect(node: &ComputedNode, transform: &GlobalTransform) -> Rect {
    let scale = node.inverse_scale_factor();
    Rect::from_center_size(
        transform.translation().truncate() * scale,
        node.size() * scale,
    )
}

/// Whether EITHER pointing device pressed inside a laid-out node this
/// frame: a raw touchdown ([`touched_node`]) or a mouse click at the
/// window cursor — the one home for the mouse half of the containment
/// math, beside its touch sibling (the game-over card consumes it).
pub(crate) fn pointer_on_node(
    touches: &Touches,
    mouse: &ButtonInput<MouseButton>,
    window: Option<&Window>,
    node: &ComputedNode,
    transform: &GlobalTransform,
) -> bool {
    touched_node(touches, node, transform)
        || (mouse.just_pressed(MouseButton::Left)
            && window
                .and_then(Window::cursor_position)
                .is_some_and(|p| node_screen_rect(node, transform).contains(p)))
}

/// One Button's activation this frame: the bevy_ui `Interaction` edge for
/// the MOUSE ([`mouse_pressed_on`]), or a raw touchdown inside the
/// laid-out rect for TOUCH — one bool, so a press tripping both halves
/// still activates once.
pub(crate) fn button_pressed(
    interactions: &Query<&Interaction, (Changed<Interaction>, With<Button>)>,
    entity: Entity,
    touches: &Touches,
    mouse: &ButtonInput<MouseButton>,
    node: &ComputedNode,
    transform: &GlobalTransform,
) -> bool {
    mouse_pressed_on(mouse, interactions.get(entity).ok().into_iter())
        || touched_node(touches, node, transform)
}

/// The MOUSE half of a Button activation: a real mouse press this frame
/// AND a `Pressed` edge among the given `Interaction` changes. The mouse
/// gate is the multi-touch attribution policy, and this is its ONE body:
/// bevy_ui attributes a press edge to whichever pointer position it
/// sampled, so with one finger resting on surface A and a second finger
/// tapping surface B, the `Pressed` edge can land on A — ungated, one
/// physical tap activated two rows (or read a resume tap as a quit).
/// Touch activation therefore never rides `Interaction`: every touch
/// surface pairs this with its own raw hit-test, which cannot
/// misattribute (and an instantaneous tap never becomes `Pressed` anyway).
pub(crate) fn mouse_pressed_on<'a>(
    mouse: &ButtonInput<MouseButton>,
    mut interactions: impl Iterator<Item = &'a Interaction>,
) -> bool {
    mouse.just_pressed(MouseButton::Left) && interactions.any(|i| *i == Interaction::Pressed)
}

/// "Any pointing device pressed this frame" — the pair every screen-serving
/// `Button` must treat together (touch and mouse ride the same
/// `Interaction` path). An armed-quit cancel bug came from enumerating one
/// and not the other; pointer pre-checks call this, and pointer-on-a-node
/// tests call [`pointer_on_node`].
pub(crate) fn pointer_pressed(touches: &Touches, mouse: &ButtonInput<MouseButton>) -> bool {
    touches.any_just_pressed() || mouse.just_pressed(MouseButton::Left)
}

fn gamepad_intent(pad: &Gamepad) -> TeamIntent {
    // Prefer the analog stick; fall back to the d-pad for aim. Below the
    // dead-zone the stick reads as exactly zero — without it, resting-stick
    // drift integrated into the PCI cursor all at-bat (TODO 73).
    let mut aim = pad.left_stick();
    if aim.length() < 0.2 {
        let dpad = pad.dpad();
        aim = if dpad.length() > 0.0 {
            dpad
        } else {
            Vec2::ZERO
        };
    }
    TeamIntent {
        aim,
        action: pad.just_pressed(GamepadButton::South),
        action_held: pad.pressed(GamepadButton::South),
        // Struct-update: channels a physical device can never produce (the
        // absolute cursor) stay at their defaults without a per-channel
        // edit at every constructor.
        ..default()
    }
}

fn keyboard_intent(keyboard: &ButtonInput<KeyCode>, scheme: KeyScheme) -> TeamIntent {
    let (up, down, left, right, action) = match scheme {
        KeyScheme::Primary => (
            KeyCode::KeyW,
            KeyCode::KeyS,
            KeyCode::KeyA,
            KeyCode::KeyD,
            KeyCode::Space,
        ),
        KeyScheme::Secondary => (
            KeyCode::ArrowUp,
            KeyCode::ArrowDown,
            KeyCode::ArrowLeft,
            KeyCode::ArrowRight,
            KeyCode::ControlRight,
        ),
    };

    let mut aim = Vec2::ZERO;
    if keyboard.pressed(up) {
        aim.y += 1.0;
    }
    if keyboard.pressed(down) {
        aim.y -= 1.0;
    }
    if keyboard.pressed(left) {
        aim.x -= 1.0;
    }
    if keyboard.pressed(right) {
        aim.x += 1.0;
    }

    TeamIntent {
        // Clamped to the stick's unit circle: raw per-axis sums gave a
        // diagonal |aim| of 1.41 — a wider pitch envelope and a ~41% faster
        // diagonal PCI cursor than any pad player could reach (TODO 73).
        aim: aim.clamp_length_max(1.0),
        action: keyboard.just_pressed(action),
        action_held: keyboard.pressed(action),
        ..default()
    }
}

/// Keeps [`Controllers`] valid across gamepad hotplug, both directions
/// (TODO 72): a disconnected pad falls back to keyboard so the game keeps
/// running, and a (re)connected pad reclaims the first human slot that is
/// stuck on keyboard — with a banner on each edge so the player knows what
/// their team is listening to. CPU slots are never touched.
fn handle_gamepad_hotplug(
    mut events: EventReader<GamepadConnectionEvent>,
    mut controllers: ResMut<Controllers>,
    mut banner: EventWriter<crate::game::flow::PlayBanner>,
) {
    for event in events.read() {
        if event.disconnected() {
            for (team, label) in [(Team::Home, "P1"), (Team::Away, "P2")] {
                let scheme = KeyScheme::for_team(team);
                if controllers.source(team) == InputSource::Gamepad(event.gamepad) {
                    let slot = match team {
                        Team::Home => &mut controllers.home,
                        Team::Away => &mut controllers.away,
                    };
                    *slot = InputSource::Keyboard(scheme);
                    banner.send(crate::game::flow::PlayBanner::new(
                        format!("PAD LOST - {label} ON KEYBOARD"),
                        crate::game::flow::BannerTone::Info,
                    ));
                }
            }
        } else if event.connected() {
            for (team, label) in [(Team::Home, "P1"), (Team::Away, "P2")] {
                if matches!(controllers.source(team), InputSource::Keyboard(_)) {
                    let slot = match team {
                        Team::Home => &mut controllers.home,
                        Team::Away => &mut controllers.away,
                    };
                    *slot = InputSource::Gamepad(event.gamepad);
                    banner.send(crate::game::flow::PlayBanner::new(
                        format!("PAD CONNECTED - {label}"),
                        crate::game::flow::BannerTone::Info,
                    ));
                    break;
                }
            }
        }
    }
}

/// Assigns input sources to teams given the chosen mode and the currently
/// connected controllers. Used by the menu when a game starts.
pub fn assign_controllers(mode: crate::game::GameMode, pads: &[Entity]) -> Controllers {
    use crate::game::GameMode;
    match mode {
        GameMode::OnePlayer => Controllers {
            home: pads
                .first()
                .copied()
                .map(InputSource::Gamepad)
                .unwrap_or(InputSource::Keyboard(KeyScheme::for_team(Team::Home))),
            away: InputSource::Cpu,
            touch_team: None,
        },
        GameMode::TwoPlayers => Controllers {
            home: pads
                .first()
                .copied()
                .map(InputSource::Gamepad)
                .unwrap_or(InputSource::Keyboard(KeyScheme::for_team(Team::Home))),
            away: pads
                .get(1)
                .copied()
                .map(InputSource::Gamepad)
                .unwrap_or(InputSource::Keyboard(KeyScheme::for_team(Team::Away))),
            touch_team: None,
        },
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::GameMode;

    fn pad(index: u32) -> Entity {
        Entity::from_raw(index)
    }

    #[test]
    fn one_player_no_pads_is_keyboard_vs_cpu() {
        let c = assign_controllers(GameMode::OnePlayer, &[]);
        assert_eq!(c.home, InputSource::Keyboard(KeyScheme::Primary));
        assert_eq!(c.away, InputSource::Cpu);
    }

    #[test]
    fn one_player_with_pad_uses_it_for_the_human() {
        let c = assign_controllers(GameMode::OnePlayer, &[pad(0)]);
        assert_eq!(c.home, InputSource::Gamepad(pad(0)));
        assert_eq!(c.away, InputSource::Cpu);
    }

    #[test]
    fn two_players_no_pads_split_the_keyboard() {
        let c = assign_controllers(GameMode::TwoPlayers, &[]);
        assert_eq!(c.home, InputSource::Keyboard(KeyScheme::Primary));
        assert_eq!(c.away, InputSource::Keyboard(KeyScheme::Secondary));
    }

    #[test]
    fn two_players_one_pad_gives_p2_the_keyboard() {
        let c = assign_controllers(GameMode::TwoPlayers, &[pad(0)]);
        assert_eq!(c.home, InputSource::Gamepad(pad(0)));
        assert_eq!(c.away, InputSource::Keyboard(KeyScheme::Secondary));
    }

    #[test]
    fn two_players_two_pads_assigns_in_order() {
        let c = assign_controllers(GameMode::TwoPlayers, &[pad(0), pad(1)]);
        assert_eq!(c.home, InputSource::Gamepad(pad(0)));
        assert_eq!(c.away, InputSource::Gamepad(pad(1)));
    }

    #[test]
    fn player_index_maps_p1_p2_and_cpu() {
        let one = assign_controllers(GameMode::OnePlayer, &[]);
        assert_eq!(one.player_index(Team::Home), Some(0));
        assert_eq!(one.player_index(Team::Away), None);
        let two = assign_controllers(GameMode::TwoPlayers, &[]);
        assert_eq!(two.player_index(Team::Away), Some(1));
    }
}
