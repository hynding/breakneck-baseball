//! Touchscreen input translator.
//!
//! Raw [`Touches`] become one normalized [`TouchIntent`] per frame, which
//! `input::gather_intents` merges into P1's [`TeamIntent`](crate::game::input::TeamIntent)
//! — touch is a *device* feeding the same `Intents` seam as the keyboard and
//! gamepad, so the Director, every script, and the mode matrix cover
//! touch-driven play automatically (the covering rule in `game::director`).
//!
//! While P1 is batting and the pitch is live, the selected
//! [`TouchScheme`](crate::game::settings::TouchScheme) decides how gestures
//! become a swing (see the scheme docs in `settings`). Everywhere else — on
//! defense, between pitches, while running the bases — every scheme shares
//! one generic mapping: the first finger is a virtual stick (drag from the
//! touchdown point = aim), a second finger is the action button, and a quick
//! bare tap is an action press.
//!
//! Screen conventions match the keyboard: screen-right = `aim.x` +1 (world
//! −X, the first-base side, per the pitch/hit mappings' negation), screen-up
//! = `aim.y` +1. The Zone Pad's absolute cursor is emitted in zone-plane
//! coordinates (world x / height), the same space as `batting::PciState`.

use bevy::input::InputSystem;
use bevy::input::touch::Touch;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::game::flow::{Phase, Play};
use crate::game::input::{Controllers, TeamIntent};
use crate::game::rules;
use crate::game::settings::{Settings, TouchScheme};
use crate::game::{GameState, ScoreBoard, Team};

// ── Tunables (fractions of the window's smaller dimension unless noted) ──────

mod translate;

pub use translate::{paused_pause_region_taps, read_touch};

/// Virtual-stick throw: full deflection at this fraction of the smaller
/// window dimension.
const STICK_RADIUS_FRAC: f32 = 0.12;
/// Flick trigger: upward speed in smaller-window-dimensions per second.
const FLICK_TRIGGER_HPS: f32 = 1.1;
/// Displacement ceiling on the flick trigger: once a frame is long enough
/// that the speed test would ask for more than this fraction of the smaller
/// window dimension, the bar stops rising (see [`flick_fire`]). A sharp
/// flick whose whole displacement batches into one stretched frame would
/// otherwise divide by the stretched dt and measure UNDER the trigger — the
/// sharpest flicks dropped exactly when frames stretch, the same flake the
/// unit tests' 450 px sweeps dodged. Kept well clear of a resting thumb's
/// slow DRIFT across such a frame, which must not fire.
const FLICK_HITCH_MIN_FRAC: f32 = 0.2;
/// A bare tap must end within this many seconds…
const TAP_MAX_SECS: f32 = 0.25;
/// …and move less than this fraction of the smaller window dimension.
const TAP_MAX_MOVE_FRAC: f32 = 0.02;

// ── Output ────────────────────────────────────────────────────────────────────

/// Re-export: the resource lives with the seam that consumes it (see
/// `input::TouchIntent`); this module is its writer.
pub use crate::game::input::TouchIntent;

// ── Gesture state ─────────────────────────────────────────────────────────────

/// The tracked first finger (virtual stick / flick / hold).
#[derive(Clone, Copy, Debug)]
struct PrimaryTouch {
    id: u64,
    /// `Time<Real>::elapsed_secs` at touchdown, for the bare-tap test.
    pressed_at: f32,
    /// Where the virtual stick is anchored. Starts at the touchdown point;
    /// re-anchored at the pitch window's edges (see [`read_touch`]) so swing
    /// gestures and runner steering never contaminate each other.
    anchor: Vec2,
    /// The finger's position last frame. [`Touch::delta`] can't serve here:
    /// it spans only the *latest* move event, so a 120 Hz touch stream under
    /// a 60 fps frame would halve measured flick speed on exactly the
    /// devices this ships for.
    last_pos: Vec2,
    /// Tracked from its own touchdown (claim loop), not inherited mid-hold
    /// (orphan adoption). Only fresh fingers may fire the bare-tap action —
    /// lifting two long-held fingers must not read as a tap.
    fresh: bool,
    /// Path length travelled so far, accumulated per frame — the bare-tap
    /// test bounds this, not the net displacement (`Touch::distance`): an
    /// aborted out-and-back drag ends near its start yet was never a tap.
    travel: f32,
}

impl PrimaryTouch {
    /// A finger tracked from `pos` at `now` — the one constructor for the
    /// claim loop and orphan adoption, so a new field (this struct grew
    /// twice already: `fresh`, then `travel`) is initialized once, not
    /// hand-copied at two literals that differ only in `fresh`.
    fn tracked_from(id: u64, pos: Vec2, now: f32, fresh: bool) -> Self {
        Self {
            id,
            pressed_at: now,
            anchor: pos,
            last_pos: pos,
            fresh,
            travel: 0.0,
        }
    }
}

/// Cross-frame finger bookkeeping. Reset on leaving `Playing` (which
/// includes pausing) so a stale finger id can never ghost-drive a play.
#[derive(Resource, Default, Debug)]
pub struct TouchGestures {
    primary: Option<PrimaryTouch>,
    /// Zone Pad: the finger owning the pad region.
    pad: Option<u64>,
    /// Zone Pad: the finger holding the SWING button.
    button: Option<u64>,
    /// Whether last frame was a Zone Pad at-bat, for the pad handoff's
    /// rising-edge test.
    zone_pad_was: bool,
    /// Zone Pad: the last aimed cursor, held after the finger lifts — for
    /// the rest of the CURRENT pitch (the regions release at `InPlay`, and
    /// this clears with them, so every pitch opens re-centered exactly
    /// like keyboard PCI does). Emitting it every frame while set also
    /// keeps the PCI adapter snapping instead of velocity-integrating
    /// stray stick aim (runner steering) into the swing cursor.
    zone_cursor: Option<Vec2>,
    /// Last frame's play phase, to detect the end of the pitch window.
    last_phase: Option<Phase>,
    /// A flick swing already fired this pitch. Pitch-level, not per-finger:
    /// tying it to the tracked finger let a lift-and-swap (or orphan
    /// adoption of a second resting thumb) fire a second swing while the
    /// same ball was still in flight. Re-armed on entering `Pitch`.
    flick_spent: bool,
    /// Any touch contact seen this session. Deliberately preserved across
    /// [`reset_touch`] (see its doc) — it reveals touch chrome (the pause
    /// button), and hiding that mid-session would strand a touch player.
    seen: bool,
    /// This frame's pause-chrome liveness, decided once in [`read_touch`]
    /// from *pre-frame* `seen` and shared with `ui::touch::tap_pause` — so
    /// the revealing touch itself can never press the invisible button.
    pause_live: bool,
    /// EVERY finger that touched down on live pause chrome, remembered by
    /// id: roles are fixed at touchdown, so a chrome finger that lingers
    /// and drifts off the small rect must never be adopted as the stick or
    /// counted as an action hold — and a SECOND chrome finger needs the
    /// same protection (binding only the first left it adoptable and
    /// counted toward the cursor pin once it drifted). Bounded by the
    /// touch count; released as each finger lifts.
    pause_fingers: Vec<u64>,
}

// ── Pure mapping helpers (unit-tested without ECS) ────────────────────────────

/// The screen→aim convention in one place: screen-right stays +x, screen-up
/// (window y decreasing) becomes aim +y. Every gesture mapping goes through
/// this, so the module-doc convention has exactly one encoding.
fn screen_to_aim(v: Vec2) -> Vec2 {
    Vec2::new(v.x, -v.y)
}

/// Which Zone Pad region a position hits, encoding the tie-break once: the
/// SWING button wins over the pad (a swing press must win any hit-test
/// tie). Shared by the touchdown claim and the at-bat-start handoff so the
/// priority can never drift between the two loops.
#[derive(Clone, Copy, PartialEq)]
enum ZoneRegion {
    Button,
    Pad,
}

fn zone_region_at(pos: Vec2, size: Vec2) -> Option<ZoneRegion> {
    let button = swing_button_rect(size);
    // The drawn button is the circle inscribed in its rect (the overlay
    // paints `BorderRadius` 50%) — hit-test the same circle, or a tap in a
    // visual corner the player sees as a miss (~21% of the square) still
    // fires a swing.
    if pos.distance(button.center()) <= button.width() * 0.5 {
        Some(ZoneRegion::Button)
    } else if pad_rect(size).contains(pos) {
        Some(ZoneRegion::Pad)
    } else {
        None
    }
}

/// Virtual stick: pixel offset from the anchor → aim.
fn stick_aim(offset_px: Vec2, scale: f32) -> Vec2 {
    let radius = (STICK_RADIUS_FRAC * scale).max(1.0);
    (screen_to_aim(offset_px) / radius).clamp_length_max(1.0)
}

/// Tap scheme: tap position relative to the screen center → aim.
fn tap_aim(pos: Vec2, size: Vec2) -> Vec2 {
    let half = (size * 0.5).max(Vec2::ONE);
    screen_to_aim((pos - half) / half).clamp_length_max(1.0)
}

/// Flick scheme: fires (returning the aim) the frame the upward speed
/// crosses the trigger. `delta_px` is this frame's finger movement.
fn flick_fire(delta_px: Vec2, dt: f32, scale: f32) -> Option<Vec2> {
    if dt <= 0.0 || scale <= 0.0 {
        return None;
    }
    let up_frac = -delta_px.y / scale;
    // The bar is the speed trigger's displacement over THIS frame, capped
    // at the flick-scale floor. One continuous expression on purpose: a
    // two-arm version (speed below a dt cutoff, floor above it) is
    // STRICTER than the speed test in the band just past the cutoff — at
    // 0.15 s it demanded 0.2 where speed asked 0.165, dropping exactly the
    // mild-hitch flicks the hitch handling exists to save. Capping can
    // only ever loosen, so a sharp flick batched into one stretched frame
    // still fires while a slow drift across it still does not.
    let bar = (FLICK_TRIGGER_HPS * dt).min(FLICK_HITCH_MIN_FRAC);
    (up_frac > bar).then(|| screen_to_aim(delta_px).normalize_or_zero())
}

/// The Zone Pad's screen rectangle: a square hugging the lower-left corner.
/// Sized from the window's *smaller* dimension so the pad and the SWING
/// button never collide on portrait phones (height-fraction sizing put ~96%
/// of the button inside the pad at 390×844).
pub fn pad_rect(size: Vec2) -> Rect {
    let s = size.min_element();
    let side = 0.40 * s;
    let min = Vec2::new(0.03 * size.x, size.y - 0.05 * s - side);
    Rect::from_corners(min, min + Vec2::splat(side))
}

/// The SWING button's screen rectangle, lower-right corner (same
/// smaller-dimension sizing as [`pad_rect`]).
pub fn swing_button_rect(size: Vec2) -> Rect {
    let s = size.min_element();
    let r = 0.10 * s;
    let center = Vec2::new(size.x - 0.14 * s, size.y - 0.18 * s);
    Rect::from_center_half_size(center, Vec2::splat(r))
}

/// The pause button's screen rectangle, right edge — shared by the overlay
/// (which positions the UI Button here) and [`read_touch`]'s claim loop
/// (which must never read a touchdown here as game input). Same
/// smaller-dimension sizing as its sibling regions, so it scales with the
/// device like they do.
pub fn pause_button_rect(size: Vec2) -> Rect {
    let s = size.min_element();
    let side = 0.12 * s;
    let min = Vec2::new(size.x - side - 0.025 * s, 0.42 * size.y);
    Rect::from_corners(min, min + Vec2::splat(side))
}

/// Whether the Zone Pad's regions are active: the scheme's at-bat, in the
/// pre-contact phases (once the ball is live the regions release to runner
/// steering). One home for the *when*, shared by [`read_touch`]'s claiming
/// and the overlay's painting — like the rects share the *where* — so the
/// drawn pad and the claimed region can never disagree.
pub fn zone_pad_active(scheme: TouchScheme, batting: bool, phase: Phase) -> bool {
    batting && scheme == TouchScheme::ZonePad && phase.pre_contact()
}

/// Whether the pause chrome is live: touch owns a slot and a touch has been
/// seen. One home for the predicate — the overlay paints by it and
/// `ui::touch::tap_pause` accepts presses by it, so an invisible button can
/// never be a live target. No scheme arm: ownership already implies
/// seen-or-contact (see `resolve_touch_owner`), and the old `scheme != Off`
/// short-circuit let the first-ever contact of a session claim a button
/// that had never been painted — claims must pass PRE-frame `seen`, so the
/// revealing touch can never press what it reveals, whatever the scheme.
pub fn pause_chrome_visible(owner: Option<Team>, seen: bool) -> bool {
    owner.is_some() && seen
}

/// Absolute pad→zone mapping: finger position inside the pad becomes a
/// zone-plane cursor. Normalized through [`screen_to_aim`] (the module's one
/// screen→aim encoding), then aim → zone plane: aim +x = world −X (the
/// first-base side, the pitch/hit mappings' negation — the same flip the PCI
/// adapter's velocity path applies), aim +y = the top of the called zone.
fn pad_zone_cursor(pos: Vec2, pad: Rect) -> Vec2 {
    let f = ((pos - pad.min) / pad.size().max(Vec2::ONE)).clamp(Vec2::ZERO, Vec2::ONE);
    let aim = screen_to_aim(f * 2.0 - Vec2::ONE);
    Vec2::new(
        rules::aim_to_world_x(aim.x) * rules::ZONE_HALF_WIDTH,
        rules::aim_to_zone_y(aim.y),
    )
}

/// The *candidate* touch team: Home whenever Home is human (it always is in
/// menu-started modes — see `assign_controllers`), else nobody. The actual
/// per-frame owner is `Controllers::touch_team`, resolved by
/// [`resolve_touch_owner`], which also excludes Director-driven slots.
/// Any touch contact this frame — held fingers OR an instantaneous tap that
/// pressed and released between two frames: such a tap appears only in
/// `just_pressed`, never among the held touches (found live in a browser;
/// fast real taps and synthetic ones both do it). One encoding for device
/// detection, ownership resolution, and the Zone Pad's cursor pinning.
fn any_contact(touches: &Touches) -> bool {
    any_contact_matching(touches, |_| true)
}

/// [`any_contact`] with a predicate — the held-OR-just-pressed union has
/// ONE encoding (forgetting the `just_pressed` half is how instantaneous
/// taps got dropped, twice, before being found live in a browser).
fn any_contact_matching(touches: &Touches, pred: impl Fn(&Touch) -> bool) -> bool {
    touches.iter().any(&pred) || touches.iter_just_pressed().any(&pred)
}

/// The ONE chrome-binding body (the method on [`TouchGestures`] delegates
/// here; the pause claim, holding only the `Vec`, calls it directly) — a
/// binding-semantics change can't reach one chrome surface and miss
/// another.
fn bind_chrome_finger(pause_fingers: &mut Vec<u64>, id: u64) {
    if !pause_fingers.contains(&id) {
        pause_fingers.push(id);
    }
}

/// This finger belongs to gameplay, not chrome: not bound by id, and not
/// currently inside live chrome. The two-term core every gameplay filter
/// (cursor pin, action holds, orphan adoption) starts from — three cycles
/// each patched ONE site missing one half; naming it ends the class. Sites
/// add only their own extra terms.
fn chrome_free(pause_fingers: &[u64], live_chrome: Option<Rect>, t: &Touch) -> bool {
    !pause_fingers.contains(&t.id()) && !live_chrome.is_some_and(|r| r.contains(t.position()))
}

fn touch_team(controllers: &Controllers) -> Option<Team> {
    (controllers.player_index(Team::Home).is_some()).then_some(Team::Home)
}

/// Resolves `Controllers::touch_team` each frame: the candidate slot, minus
/// two exclusions. A directed pseudo-human slot (Scripted/Cpu routing makes
/// it Keyboard-sourced) is never touch-owned, so its *configured* batting
/// style holds and scripts, autoplay, and attract modes behave identically
/// in every build — a runtime fact checked at runtime (a `cfg!(autoplay)`
/// gate here used to make `cargo test --features autoplay` behave
/// differently from plain `cargo test`). And ownership needs a real
/// touchscreen (`TouchGestures::seen`): merely selecting a scheme on the
/// menu must not override a desktop keyboard player's configured batting
/// style with zero touches ever seen. Runs in the menu too — the settings
/// screen displays the override state. Guarded write: the device model's
/// change tick stays honest for its other readers.
pub fn resolve_touch_owner(
    touches: Res<Touches>,
    mut controllers: ResMut<Controllers>,
    gestures: Res<TouchGestures>,
    director: Option<Res<crate::game::director::Director>>,
) {
    use crate::game::director::Policy;
    // `any_contact` alongside the persistent bit: on a pad-started session
    // (no menu tap ever), the first gameplay touch must be owned the same
    // frame it lands — `seen` alone flips one system later (`read_touch`),
    // which dropped an instantaneous first tap entirely.
    // One positive filter chain: keep the candidate only while a touch is
    // real and no Director owns the slot — a future exclusion is another
    // `.filter`, not another bool-and-branch.
    let next = touch_team(&controllers)
        .filter(|_| gestures.seen || any_contact(&touches))
        .filter(|&team| {
            director
                .as_ref()
                .is_none_or(|d| matches!(d.policy(team), Policy::Human))
        });
    if controllers.touch_team != next {
        controllers.touch_team = next;
    }
}

impl TouchGestures {
    /// Whether any touch contact has ever been seen this session — the
    /// device-detection bit the overlay uses to show touch chrome (the
    /// pause button) even under `TouchScheme::Off`.
    pub fn touch_seen(&self) -> bool {
        self.seen
    }

    /// Binds a finger to touch chrome by id: for as long as it stays down
    /// it is excluded from stick adoption, action holds, and the Zone
    /// Pad's cursor pin — "roles are fixed at touchdown". The pause
    /// button's claim binds its own fingers; OTHER chrome (the pause
    /// board's quit row) calls this from its raw hit-test, so a still-held
    /// chrome finger crossing a resume can't become gameplay input.
    /// Released as the finger lifts (`release_lifted_chrome_fingers`).
    pub fn bind_chrome_finger(&mut self, id: u64) {
        bind_chrome_finger(&mut self.pause_fingers, id);
    }

    /// Whether a finger is currently bound as chrome — the read side of
    /// [`Self::bind_chrome_finger`], for surfaces that must yield to an
    /// earlier claimant (the quit row skips fingers the pause button's
    /// claim bound first, since the button draws above the board).
    pub fn is_chrome_finger(&self, id: u64) -> bool {
        self.pause_fingers.contains(&id)
    }

    /// Test seam: simulates a touchscreen having been seen. Headless runs
    /// never receive real `Touches`, and ownership requires the bit — a
    /// test pinning the Director exclusion in `resolve_touch_owner` is
    /// vacuous without it (the seen filter alone would yield `None`).
    pub fn mark_seen(&mut self) {
        self.seen = true;
    }

    /// Whether the pause chrome accepts presses this frame (see the
    /// `pause_live` field).
    pub fn pause_chrome_live(&self) -> bool {
        self.pause_live
    }
}

/// Neutral everything on leaving `Playing` (fires on pause too — a finger
/// held across a pause must not ghost-drive the resumed play). The `seen`
/// device bit survives: wiping it would hide the pause button after the
/// very tap that opened the pause board.
pub fn reset_touch(mut gestures: ResMut<TouchGestures>, mut out: ResMut<TouchIntent>) {
    *gestures = TouchGestures {
        seen: gestures.seen,
        // Preserved so the pause button keeps accepting the resume tap
        // while `read_touch` (Playing-gated) isn't re-deciding it.
        pause_live: gestures.pause_live,
        // Chrome roles are bound to physical fingers, and a pause
        // doesn't lift them: wiping these let a still-held chrome finger
        // drift off the rect while paused and be adopted as the stick on
        // resume. Released as each lifts, in either state.
        pause_fingers: std::mem::take(&mut gestures.pause_fingers),
        ..TouchGestures::default()
    };
    *out = TouchIntent::default();
}

/// Sets the device-detection bit outside `Playing` — menu and game-over
/// taps are touches too, so a phone player's ownership is already resolved
/// by the time a game starts instead of costing them their first gameplay
/// touch. Inside `Playing`, `read_touch` owns the bit with its pre-frame
/// discipline (the revealing touch must never press the still-invisible
/// pause button), which running this there would preempt.
pub fn detect_touchscreen(touches: Res<Touches>, mut gestures: ResMut<TouchGestures>) {
    if !gestures.seen && any_contact(&touches) {
        gestures.seen = true;
    }
}

/// `run_if` for the touch chrome painters and tap consumers (and, via
/// `not()`, for `detect_touchscreen`): nothing shows or accepts presses
/// before a touch is ever seen — the one named gate, so the semantics of
/// `seen` have a single home to change.
pub fn touchscreen_seen(gestures: Res<TouchGestures>) -> bool {
    gestures.seen
}

/// `run_if` for [`read_touch`]: skip the never-seen, ownerless, contactless
/// frames — the desktop majority. All three terms are load-bearing: `seen`
/// keeps the `pause_live` upkeep running once chrome could exist,
/// ownership keeps the translator live for its owner, and live contact
/// lets the very first touch of a pad-started session be processed the
/// frame it lands (the seen-flip happens inside `read_touch` in `Playing`).
pub fn touch_relevant(
    controllers: Res<Controllers>,
    gestures: Res<TouchGestures>,
    touches: Res<Touches>,
) -> bool {
    controllers.touch_team.is_some() || gestures.seen || any_contact(&touches)
}

/// New game (the MainMenu → Playing transition): the previous game's chrome
/// liveness must not leak into frame one — the state-gated PreUpdate
/// systems can't re-decide anything until frame two (the transition applies
/// after PreUpdate), and a stale `true` would leave the invisible pause
/// button a live click target for exactly that frame. Ownership re-resolves
/// here too (chained after this), so frame one's `style_for` and reticle
/// consumers already see the right owner instead of the `None` a fresh
/// `assign_controllers` just wrote.
pub fn arm_touch_for_new_game(mut gestures: ResMut<TouchGestures>) {
    gestures.pause_live = false;
    // A chrome binding is meaningless in a fresh game (the button wasn't
    // there on the menu), and neither release path runs in GameOver or
    // MainMenu — a recycled id carried in from last game would strand the
    // menu-start finger still resting on the glass.
    gestures.pause_fingers.clear();
}

// ── Plugin ────────────────────────────────────────────────────────────────────

pub struct TouchPlugin;

impl Plugin for TouchPlugin {
    fn build(&self, app: &mut App) {
        // `TouchIntent` is init'd by `InputPlugin` (its owner/consumer).
        app.init_resource::<TouchGestures>()
            .add_systems(
                PreUpdate,
                detect_touchscreen
                    // After the input plugin's touch ingestion and before
                    // the resolver — without both edges the first-ever
                    // tap's frame reads stale `Touches` or resolves
                    // ownership nondeterministically (this writes the bit
                    // the resolver reads).
                    .after(InputSystem)
                    .before(resolve_touch_owner)
                    // (Skips forever once the one-way bit sets.)
                    .run_if(not(in_state(GameState::Playing)).and(not(touchscreen_seen))),
            )
            .add_systems(
                crate::game::game_start(),
                (arm_touch_for_new_game, resolve_touch_owner).chain(),
            )
            .add_systems(
                PreUpdate,
                resolve_touch_owner
                    // After touch ingestion: the same-frame grant reads
                    // this frame's `Touches` (caught by the pipeline e2e —
                    // without the edge the grant was schedule-ambiguous).
                    .after(InputSystem)
                    .before(crate::game::input::gather_intents)
                    // The menu too: the settings screen displays whether
                    // the touch scheme owns P1's style, and it must agree
                    // with the ownership `style_for` will actually apply.
                    .run_if(
                        in_state(GameState::MainMenu)
                            .or(in_state(GameState::Playing))
                            .or(in_state(GameState::Paused))
                            // The sibling gate (`read_touch` uses it too):
                            // no owner to clear, no seen device, no live
                            // contact — the resolver can only recompute
                            // `None`, so skip it on the desktop majority.
                            .and(touch_relevant),
                    ),
            )
            .add_systems(
                Update,
                paused_pause_region_taps
                    .before(crate::game::subs::PauseInputSet)
                    // After the device-model writer (gamepad hotplug), so
                    // the `Controllers` read here is never order-ambiguous
                    // against it (the ambiguity audit gates this). Skips
                    // touch-irrelevant frames like every sibling — a
                    // keyboard-only pause otherwise dirtied `TouchGestures`
                    // 60×/s for a touchscreen that was never seen (the
                    // mid-pause first touch still enters via `any_contact`).
                    .after(crate::game::GameplayOrder::Input)
                    .run_if(in_state(GameState::Paused).and(touch_relevant)),
            )
            .add_systems(
                PreUpdate,
                read_touch
                    .after(InputSystem)
                    .after(resolve_touch_owner)
                    .before(crate::game::input::gather_intents)
                    .run_if(in_state(GameState::Playing).and(touch_relevant)),
            )
            .add_systems(OnExit(GameState::Playing), reset_touch);
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: Vec2 = Vec2::new(1280.0, 720.0);

    #[test]
    fn stick_aim_maps_drag_up_to_positive_y_and_clamps() {
        // Drag straight up (screen y decreases) by the full radius.
        let up = stick_aim(Vec2::new(0.0, -STICK_RADIUS_FRAC * SIZE.y), SIZE.y);
        assert!((up - Vec2::Y).length() < 1e-5);
        // A huge drag clamps to the unit circle.
        let big = stick_aim(Vec2::new(4000.0, 4000.0), SIZE.y);
        assert!((big.length() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn tap_aim_center_is_neutral_and_right_is_positive_x() {
        assert!(tap_aim(SIZE * 0.5, SIZE).length() < 1e-5);
        let right = tap_aim(Vec2::new(SIZE.x, SIZE.y * 0.5), SIZE);
        assert!(right.x > 0.99 && right.y.abs() < 1e-5);
        let top = tap_aim(Vec2::new(SIZE.x * 0.5, 0.0), SIZE);
        assert!(top.y > 0.99);
    }

    #[test]
    fn flick_fires_only_past_the_upward_speed_trigger() {
        let dt = 1.0 / 60.0;
        // Slow upward drift: no fire.
        let slow = Vec2::new(0.0, -0.5 * FLICK_TRIGGER_HPS * SIZE.y * dt);
        assert!(flick_fire(slow, dt, SIZE.y).is_none());
        // Fast upward flick: fires, aimed up.
        let fast = Vec2::new(0.0, -2.0 * FLICK_TRIGGER_HPS * SIZE.y * dt);
        let aim = flick_fire(fast, dt, SIZE.y).expect("flick should fire");
        assert!(aim.y > 0.99);
        // A fast *downward* swipe never fires.
        assert!(flick_fire(-fast, dt, SIZE.y).is_none());
        // Angled flick keeps its horizontal component's sign.
        let angled = Vec2::new(fast.y.abs() * 0.5, fast.y);
        let aim = flick_fire(angled, dt, SIZE.y).expect("angled flick fires");
        assert!(aim.x > 0.0 && aim.y > 0.0);
        // Hitch frames cap the bar at the displacement ceiling — pinned in
        // BOTH directions: a sharp flick batched into one stretched frame
        // still fires, while a slow resting-thumb drift across the same
        // frame (whose true speed is far under the trigger) must NOT fire
        // and burn the pitch's one flick.
        let hitch_dt = 0.5;
        let batched = Vec2::new(0.0, -1.5 * FLICK_HITCH_MIN_FRAC * SIZE.y);
        assert!(
            flick_fire(batched, hitch_dt, SIZE.y).is_some(),
            "a hitch frame must not swallow a sharp flick"
        );
        let drift = Vec2::new(0.0, -0.5 * FLICK_HITCH_MIN_FRAC * SIZE.y);
        assert!(
            flick_fire(drift, hitch_dt, SIZE.y).is_none(),
            "a slow drift across a hitch frame must not fire a swing"
        );
        // The bar may only ever LOOSEN as frames stretch: anything that
        // clears the plain speed trigger must fire at every dt. A two-arm
        // version (speed, then a floor past a cutoff) failed exactly here,
        // demanding MORE displacement just past its cutoff than the speed
        // test it replaced.
        for &mild_dt in &[0.05, 0.099, 0.101, 0.15, 0.18, 0.25, 1.0] {
            let just_over_speed = Vec2::new(0.0, -1.01 * FLICK_TRIGGER_HPS * mild_dt * SIZE.y);
            assert!(
                flick_fire(just_over_speed, mild_dt, SIZE.y).is_some(),
                "a gesture past the speed trigger must fire at dt={mild_dt}"
            );
        }
    }

    #[test]
    fn pad_corners_map_to_zone_corners_with_screen_x_negated() {
        let pad = pad_rect(SIZE);
        // Pad top-left: screen-left = world +X (third-base side), zone top.
        let tl = pad_zone_cursor(pad.min, pad);
        assert!((tl.x - rules::ZONE_HALF_WIDTH).abs() < 1e-5);
        assert!((tl.y - rules::ZONE_HIGH).abs() < 1e-5);
        // Pad bottom-right: world −X, zone bottom.
        let br = pad_zone_cursor(pad.max, pad);
        assert!((br.x + rules::ZONE_HALF_WIDTH).abs() < 1e-5);
        assert!((br.y - rules::ZONE_LOW).abs() < 1e-5);
        // Center maps to the adapter's own resting spot — one definition.
        let c = pad_zone_cursor(pad.center(), pad);
        let resting = crate::game::batting::PciState::center();
        assert!((c - resting).length() < 1e-4);
    }

    #[test]
    fn pad_and_swing_button_stay_disjoint_across_aspect_ratios() {
        // Landscape desktop, portrait phone (the shipped failure case at
        // height-fraction sizing), tablet-ish square, and a small landscape.
        for size in [
            SIZE,
            Vec2::new(390.0, 844.0),
            Vec2::new(800.0, 800.0),
            Vec2::new(568.0, 320.0),
        ] {
            let regions = [
                pad_rect(size),
                swing_button_rect(size),
                pause_button_rect(size),
            ];
            for (i, a) in regions.iter().enumerate() {
                for b in regions.iter().skip(i + 1) {
                    assert!(
                        a.intersect(*b).is_empty(),
                        "{a:?} overlaps {b:?} at {size:?}"
                    );
                }
                // Fully on screen.
                assert!(
                    a.min.x >= 0.0 && a.min.y >= 0.0,
                    "{a:?} off-screen at {size:?}"
                );
                assert!(
                    a.max.x <= size.x && a.max.y <= size.y,
                    "{a:?} off-screen at {size:?}"
                );
            }
        }
    }
}
