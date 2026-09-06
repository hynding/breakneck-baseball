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

/// Which Zone Pad region a position hits, encoding the tie-break once: the
/// SWING button wins over the pad (a swing press must win any hit-test
/// tie). Shared by the touchdown claim and the at-bat-start handoff so the
/// priority can never drift between the two loops.
#[derive(Clone, Copy, PartialEq)]
enum ZoneRegion {
    Button,
    Pad,
}

/// A held finger with NO role (not chrome-bound, not a region owner, not
/// the tracked stick) currently resting inside `region` — the promotion
/// candidate when a region's owner lifts mid-at-bat: the claim loop only
/// sees touchdowns and the handoff scan only runs at the at-bat's rising
/// edge, so without this a second thumb already resting on the pad (or
/// SWING) stayed dead until physically re-pressed. The tracked stick is
/// never promoted (the wander rule: an in-flight drag that merely crossed
/// a rect keeps its steer role).
fn resting_roleless_in(
    touches: &Touches,
    size: Vec2,
    region: ZoneRegion,
    gestures: &TouchGestures,
) -> Option<(u64, Vec2)> {
    touches
        .iter()
        .find(|t| {
            let id = t.id();
            // `chrome_free`'s rect half is passed `None` deliberately: a
            // position inside a Zone Pad region cannot be inside the pause
            // rect (the regions are pairwise disjoint — pinned by the
            // aspect-ratio test), so only the id binding can matter here.
            chrome_free(&gestures.pause_fingers, None, t)
                && gestures.pad != Some(id)
                && gestures.button != Some(id)
                && gestures.primary.as_ref().is_none_or(|p| p.id != id)
                && zone_region_at(t.position(), size) == Some(region)
        })
        .map(|t| (t.id(), t.position()))
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

/// The ONE flick commit: tests the trigger and, on fire, emits the swing
/// and burns the pitch's single shot. Called from the held-finger arm and
/// the just-released arm (a flick whose movement and lift land in one
/// frame batch never reaches the held arm) — one body, so what a fired
/// flick emits cannot drift between the two.
fn try_flick(spent: &mut bool, next: &mut TeamIntent, delta_px: Vec2, dt: f32, scale: f32) {
    if *spent {
        return;
    }
    if let Some(aim) = flick_fire(delta_px, dt, scale) {
        next.action = true;
        next.aim = aim;
        *spent = true;
    }
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

/// The raw pause-region claim, shared by [`read_touch`] (Playing) and
/// [`paused_pause_region_taps`] (Paused) so the two states can never drift:
/// while the chrome is live, a touchdown in its rect *is* the pause request
/// — raised here rather than left to the UI `Interaction` path because
/// bevy_ui resolves multi-touch presses at the FIRST held finger's position,
/// so a second-finger tap never reaches the Button (`ui::touch::tap_pause`
/// still serves the mouse). The finger is remembered by id: roles are fixed
/// at touchdown, so a chrome finger that lingers must never later be adopted
/// as the stick or counted as an action hold — in either state.
fn claim_pause_region_taps(
    touches: &Touches,
    pause_rect: Rect,
    live: bool,
    pause_fingers: &mut Vec<u64>,
    tapped: &mut crate::game::subs::PauseTapped,
) {
    if !live {
        return;
    }
    for id in crate::game::input::touched_rect_ids(touches, pause_rect) {
        tapped.0 = true;
        // EVERY chrome touchdown is bound (not just the first): each
        // bound finger keeps its protection from stick adoption and
        // the cursor pin for as long as it stays down, wherever it
        // drifts.
        bind_chrome_finger(pause_fingers, id);
    }
}

/// Whether the bound chrome finger has lifted and its id should be
/// released — checked every frame, in every state that reads touches
/// (INCLUDING ownerless frames, where [`read_touch`] otherwise returns
/// early): browsers and Android recycle touch ids, so a stale binding
/// would strand the next finger dealt the same id. Returns the verdict
/// instead of taking `&mut` so callers only touch the `ResMut` (and its
/// change tick) when there is actually something to clear.
fn release_lifted_chrome_fingers(touches: &Touches, gestures: &mut ResMut<TouchGestures>) {
    // Guarded through the `ResMut` (the `park` pattern): the verdict reads
    // immutably, and the retain — the only `&mut` — happens only when a
    // finger actually lifted, so quiet frames never dirty the change tick.
    if gestures
        .pause_fingers
        .iter()
        .any(|&id| touches.get_pressed(id).is_none())
    {
        gestures
            .pause_fingers
            .retain(|&id| touches.get_pressed(id).is_some());
    }
}

// ── The translator ────────────────────────────────────────────────────────────

/// Turns this frame's [`Touches`] into [`TouchIntent`] under the selected
/// scheme. Runs in `PreUpdate` after Bevy's touch-event processing
/// (`InputSystem` — without that ordering, gestures would read last frame's
/// `Touches` on some builds, a build-dependent ~1-frame swing-timing skew)
/// and before `gather_intents` (which merges the result), only while
/// `Playing` — in every scheme including `Off`, whose generic mapping keeps
/// touch devices playable.
#[allow(clippy::too_many_arguments)]
pub fn read_touch(
    time: Res<Time<Real>>,
    touches: Res<Touches>,
    windows: Query<&Window, With<PrimaryWindow>>,
    score: Res<ScoreBoard>,
    play: Res<Play>,
    controllers: Res<Controllers>,
    settings: Res<Settings>,
    mut pause_tap: ResMut<crate::game::subs::PauseTapped>,
    mut gestures: ResMut<TouchGestures>,
    mut out: ResMut<TouchIntent>,
) {
    // Built locally and committed ONCE at the single exit below (early
    // returns `break` the labeled block instead) — a forgotten commit on a
    // future early return would leave last frame's intent latched in the
    // resource, this module's proven ghost-input class. `set_if_neq`: a
    // touch-free frame (every frame, on a desktop) never dirties it.
    let next: TeamIntent = 'compute: {
        let mut next = TeamIntent::default();
        let scheme = settings.touch_scheme;
        // Pre-frame `seen` decides this frame's chrome liveness: a first-ever
        // touch must act as gameplay input and *reveal* the pause button, not
        // press the still-invisible node it happens to land on.
        let seen_before = gestures.seen;
        if any_contact(&touches) {
            gestures.seen = true;
        }
        // Per-frame hygiene BEFORE every early return below: the chrome-finger
        // release must run on ownerless frames too (a finger lifted while a
        // Director owned the slot left its recycled id stranded), and liveness
        // must be re-decided (exiting with last frame's value latched froze an
        // ownerless game's invisible pause button as a live click target).
        release_lifted_chrome_fingers(&touches, &mut gestures);
        // This frame's pause-chrome liveness (see `seen_before` above) — the
        // claim loop's region carve-out and `ui::touch::tap_pause` both read
        // this one decision, so they can never disagree within a frame.
        // Decided BEFORE the ownership early-return, through the shared
        // predicate: the ownerless path's `false` is exactly
        // `pause_chrome_visible(None, _)`, and spelling it by hand there
        // split one predicate across two sites — a third term added to it
        // would have reached one path and not the other.
        let pause_live = pause_chrome_visible(controllers.touch_team, seen_before);
        gestures.pause_live = pause_live;
        // The Zone Pad's rising edge is per-frame hygiene too, for the same
        // reason: a frame that exits early below (no owner, no window) while
        // `zone_pad_was` stayed latched true makes the NEXT real at-bat's
        // `zone_pad_started` false, so the resting-thumb handoff — the whole
        // reason the edge exists — never runs and thumbs already on the pad
        // and SWING button stay dead for that entire at-bat.
        // (Plain writes throughout this resource: its change tick is already
        // dirtied every held-finger frame by `last_pos`, so nothing can gate on
        // it — `paint_touch_overlay` mirrors the one bit it needs in a Local.)
        let zone_pad_at_bat = zone_pad_active(
            scheme,
            controllers.touch_team == Some(score.batting_team()),
            play.phase,
        );
        let zone_pad_started = zone_pad_at_bat && !gestures.zone_pad_was;
        gestures.zone_pad_was = zone_pad_at_bat;
        let Some(team) = controllers.touch_team else {
            break 'compute next;
        };
        let Ok(window) = windows.get_single() else {
            break 'compute next;
        };
        let size = window.size();
        // Gestures scale with the smaller dimension (like the pad/button rects):
        // height-scaling would make the same physical flick ~2× harder in
        // portrait than landscape on a phone.
        let scale = size.min_element();
        // Hoisted: the chrome rect (and its this-frame liveness pairing) is
        // consulted per touch at several claim and filter sites below — one
        // derivation per frame, one value they all share.
        let pause_rect = pause_button_rect(size);
        let live_chrome = pause_live.then_some(pause_rect);
        let batting = score.batting_team() == team;
        let now = time.elapsed_secs();
        let dt = time.delta_secs();

        // Swing gestures and base steering must not contaminate each other, so
        // the stick re-anchors at both edges of the pitch window: on *entering*
        // `Pitch` (a Down drag held through the windup has done its job — the
        // send is committed pre-delivery — and must not poison a Hold+Release
        // aim measured during the flight) and on *leaving* it (displacement
        // accumulated by the swing — a lingering post-flick finger, a
        // hold-and-release drag — must not read as a runner call on the live
        // ball). Entering also re-arms the flick so a resting thumb can swing
        // at every pitch of the at-bat, not just the first. Other phase changes
        // keep the anchor — a lead stretched in the steal window survives into
        // the windup.
        if gestures.last_phase != Some(play.phase) {
            let crossing_pitch =
                gestures.last_phase == Some(Phase::Pitch) || play.phase == Phase::Pitch;
            if crossing_pitch {
                if let Some(primary) = gestures.primary.as_mut() {
                    if let Some(touch) = touches.get_pressed(primary.id) {
                        primary.anchor = touch.position();
                    }
                }
                // Everything keyed on *entering* the pitch window lives in this
                // one block, so the edge has a single encoding.
                if play.phase == Phase::Pitch {
                    gestures.flick_spent = false;
                    // A SWING press held from before the delivery fires as the
                    // ball leaves the hand: the button's press edge landed in a
                    // phase the batting adapter ignores, so without this an
                    // anticipatory press (which the always-drawn button
                    // invites) reads as a dead button — an early swing graded
                    // Early/Whiff is the honest outcome. Emitted as an
                    // ordinary action edge so it flows through the `Intents`
                    // seam like any press; the sim layer keeps zero device
                    // knowledge.
                    // `zone_pad_active` (Pitch is pre-contact, so this is
                    // "the pad's at-bat" — the predicate's one home, not a
                    // second spelling of its terms).
                    if zone_pad_active(scheme, batting, play.phase)
                        && gestures
                            .button
                            .is_some_and(|id| touches.get_pressed(id).is_some())
                    {
                        next.action = true;
                    }
                }
            }
            gestures.last_phase = Some(play.phase);
        }

        // Scheme-specific swing mapping only while a swing is possible. The
        // Zone Pad's regions are claimed through the pre-contact phases (see
        // `zone_pad_active` — the overlay paints by the same predicate) so a
        // finger resting on the pad can pre-aim without reading as a runner-
        // send drag — but NOT once the ball is in play: a thumb landing in the
        // lower-left then must steer runners, not feed a dead pad.
        let swing_phase = batting && play.phase == Phase::Pitch;
        // `zone_pad_at_bat` / `zone_pad_started` are decided above the early
        // returns (see there).
        // Tap's swing claim tolerates the WindUp→Pitch flip landing one frame
        // after this PreUpdate read (the delivery frame itself): the tap edge
        // is emitted through the windup too — the adapter ignores it outside
        // `Pitch`, so only a flip-frame tap gains anything — and the finger
        // still claims the stick below, keeping the send-drag alive.
        let tap_phase = batting && play.phase.in_delivery();
        // The generic action mapping is live outside the swing window, and
        // under `Off` during it too (that press IS the swing) — ONE spelling
        // for its two halves, the second-finger claim and the bare-tap
        // release, which must always agree.
        let generic_action = !swing_phase || scheme == TouchScheme::Off;

        // One "the chrome owns this position" predicate for every claim site —
        // a new piece of touch chrome registers here, not in three loops.
        let chrome_claims = |pos: Vec2| live_chrome.is_some_and(|r| r.contains(pos));
        // Pause-chrome touchdowns claimed first (see the shared helper): a
        // chrome finger must never double as a swing, pitch, or stick claim —
        // the loop below skips it by position. While hidden, the region is
        // ordinary screen.
        claim_pause_region_taps(
            &touches,
            pause_rect,
            pause_live,
            &mut gestures.pause_fingers,
            &mut pause_tap,
        );
        // Whether a tap wrote a position aim this frame. The stick guard below
        // must key on THIS, not on `next.action`: on defense a second-finger
        // action tap also sets `action`, and eating the drag aim on that exact
        // frame would turn every aimed touch pitch/throw into an unaimed one —
        // flow samples aim at the action edge.
        let mut tap_aimed = false;

        // ── Claim new fingers ────────────────────────────────────────────────
        for touch in touches.iter_just_pressed() {
            if chrome_claims(touch.position()) {
                continue;
            }
            if zone_pad_at_bat {
                match zone_region_at(touch.position(), size) {
                    Some(ZoneRegion::Button) => {
                        // Every press is an edge, but the tracker keeps the
                        // FIRST holder — a second finger tapping the button
                        // must not orphan a still-held anticipatory press.
                        if gestures.button.is_none() {
                            gestures.button = Some(touch.id());
                        }
                        next.action = true; // SWING press edge
                        continue;
                    }
                    Some(ZoneRegion::Pad) => {
                        // First holder keeps the pad: a second finger brushing
                        // the rect must not steal aiming from the thumb that
                        // owns it (the brush is still consumed — it becomes no
                        // other input).
                        if gestures.pad.is_none() {
                            gestures.pad = Some(touch.id());
                            // The touchdown position IS an aim, recorded at the
                            // claim: an instantaneous tap can press and release
                            // between two frames, and the continuous block
                            // below would then clear the pad without ever
                            // reading the finger — grading the swing at
                            // zone-center instead of the corner the player
                            // tapped.
                            gestures.zone_cursor =
                                Some(pad_zone_cursor(touch.position(), pad_rect(size)));
                        }
                        continue;
                    }
                    None => {}
                }
            }
            if tap_phase && scheme == TouchScheme::Tap {
                // Every touchdown is a swing; position aims it. No `continue`:
                // the finger also claims the stick below (aim-neutral during
                // the pitch — the Tap arm silences it — but a windup touchdown
                // must still be able to drag a runner send).
                next.action = true;
                // …unless a deliberately deflected drag owns the aim: flow
                // re-reads the runner send from aim every frame, so letting a
                // windup tap clobber a held send-drag — even for one frame —
                // could silently drop the send at its commit. WindUp only:
                // during the pitch the Tap arm below silences the drag's
                // stick entirely, so deferring to it there would grade the
                // swing at zone-center instead of the tapped placement while
                // protecting an aim that is never emitted.
                let dragging = !swing_phase
                    && gestures.primary.is_some_and(|p| {
                        touches.get_pressed(p.id).is_some_and(|t| {
                            stick_aim(t.position() - p.anchor, scale).length()
                                >= rules::AIM_DEADZONE
                        })
                    });
                if !dragging {
                    next.aim = tap_aim(touch.position(), size);
                    tap_aimed = true;
                }
            }
            if gestures.primary.is_none() {
                // A Tap-scheme touchdown already fired its action edge in the
                // arm above — marking it fresh would let its quick release fire
                // the generic bare-tap arm too, one physical tap delivering two
                // presses (real in WindUp, where the release still lands
                // outside the swing window).
                let fresh = !(tap_phase && scheme == TouchScheme::Tap);
                gestures.primary = Some(PrimaryTouch::tracked_from(
                    touch.id(),
                    touch.position(),
                    now,
                    fresh,
                ));
            } else if generic_action {
                // Generic mapping: a second finger is the action button. Under
                // `Off` (no swing scheme) it stays live during the pitch too —
                // that press IS the swing, keeping a scheme-less touch device
                // fully playable instead of soft-locked.
                next.action = true;
            }
        }

        // ── Zone Pad continuous state ────────────────────────────────────────
        if zone_pad_at_bat {
            // Fingers already down at the moment the at-bat began (resting
            // through the half-inning flip, or holding SWING through the
            // previous pitch's Result — the regions release between pitches)
            // hand off to the region each sits in: the tracked stick would
            // otherwise keep steering runners while the control it's touching
            // looks dead, and an untracked second finger (a thumb that landed
            // during Result — the claim loop stores no id for it) would leave
            // its region dead for the whole at-bat until re-pressed. A
            // re-acquired button is a hold, not a press edge — the Pitch-entry
            // anticipatory emission fires it at the delivery. Rising-edge
            // only: an in-flight drag that merely *wanders* into a rect
            // mid-play must keep its runner-steer role.
            if zone_pad_started {
                for touch in touches.iter() {
                    let id = touch.id();
                    if gestures.pause_fingers.contains(&id) {
                        continue;
                    }
                    let claimed = match zone_region_at(touch.position(), size) {
                        Some(ZoneRegion::Button)
                            if gestures.button.is_none() && gestures.pad != Some(id) =>
                        {
                            gestures.button = Some(id);
                            true
                        }
                        Some(ZoneRegion::Pad)
                            if gestures.pad.is_none() && gestures.button != Some(id) =>
                        {
                            gestures.pad = Some(id);
                            true
                        }
                        _ => false,
                    };
                    // The stick gives up its role when a region takes it.
                    if claimed && gestures.primary.is_some_and(|p| p.id == id) {
                        gestures.primary = None;
                    }
                }
            }
            if let Some(id) = gestures.pad {
                match touches.get_pressed(id) {
                    Some(touch) => {
                        gestures.zone_cursor =
                            Some(pad_zone_cursor(touch.position(), pad_rect(size)));
                    }
                    None => {
                        gestures.pad = None;
                        // Promote a roleless thumb already resting on the
                        // pad (see `resting_roleless_in`), updating the
                        // sticky cursor from ITS position — without this,
                        // the lifted owner's stale aim kept emitting while
                        // the visible replacement thumb aimed elsewhere.
                        if let Some((id, pos)) =
                            resting_roleless_in(&touches, size, ZoneRegion::Pad, &gestures)
                        {
                            gestures.pad = Some(id);
                            gestures.zone_cursor = Some(pad_zone_cursor(pos, pad_rect(size)));
                        }
                    }
                }
            }
            // Absolute only while touch is actually driving: a pad aim this
            // at-bat (sticky after the finger lifts — see the `zone_cursor`
            // docs), or any finger currently on the glass — with fingers
            // around, the adapter must always snap, or a stray off-pad
            // finger's stick aim would velocity-integrate the swing cursor
            // away from anywhere the player pointed. Otherwise emit `None` so
            // keyboard/stick PCI steering keeps the velocity path: keying this
            // on the session-permanent `seen` bit locked steering out for the
            // rest of the session after one incidental brush on a touchscreen
            // laptop.
            // A gameplay finger (`chrome_free`), and not holding a Zone Pad
            // region — the SWING holder's press produces no stick aim, so
            // it cannot corrupt the cursor and must not force the center
            // pin (a keyboard steerer resting an anticipatory thumb on the
            // drawn button was locked to zone center for the whole pitch).
            // NON-PRIMARY region-resting fingers are carved out by position
            // too, like the adoption filter below: a roleless second thumb
            // on an already-owned region was consumed by the claim loop
            // ("the brush becomes no other input") and emits no stick aim
            // either — by id alone it passed this filter and re-shipped the
            // same center-lock one finger later (and pinned the cursor on
            // the very frame owner-lift promoted it). The tracked stick is
            // exempt from the carve-out: the wander rule lets it keep
            // emitting aim INSIDE a region, so it must keep forcing the
            // snap wherever it is, or its drag would velocity-integrate
            // the swing cursor — the exact drift the pin exists to stop.
            let gameplay = |t: &Touch| {
                let id = t.id();
                chrome_free(&gestures.pause_fingers, live_chrome, t)
                    && Some(id) != gestures.button
                    && Some(id) != gestures.pad
                    && (gestures.primary.is_some_and(|p| p.id == id)
                        || !(zone_pad_at_bat && zone_region_at(t.position(), size).is_some()))
            };
            // Inline, so the `||` short-circuits: `zone_cursor` is sticky
            // for the whole pitch once the thumb has aimed, and the scan
            // it would skip walks every finger twice, rebuilding both
            // region rects per finger per pass.
            if gestures.zone_cursor.is_some() || any_contact_matching(&touches, gameplay) {
                next.cursor = Some(
                    gestures
                        .zone_cursor
                        .unwrap_or_else(crate::game::batting::PciState::center),
                );
            }
            if let Some(id) = gestures.button {
                if touches.get_pressed(id).is_some() {
                    next.action_held = true;
                } else {
                    gestures.button = None;
                    // Same promotion for SWING: a second finger resting on
                    // the visibly-pressed button must not go dead when the
                    // holder lifts (its hold feeds the anticipatory press
                    // at the delivery).
                    if let Some((id, _)) =
                        resting_roleless_in(&touches, size, ZoneRegion::Button, &gestures)
                    {
                        gestures.button = Some(id);
                        next.action_held = true;
                    }
                }
            }
        } else {
            gestures.pad = None;
            gestures.button = None;
            gestures.zone_cursor = None;
        }

        // ── Primary finger: stick / flick / hold ────────────────────────────
        // Split borrow: `primary` is held mutably while `pause_fingers` is read
        // and `flick_spent` written — distinct fields, so the destructure costs
        // nothing (a per-frame Vec clone used to dodge this borrow).
        let TouchGestures {
            primary: primary_slot,
            pause_fingers,
            flick_spent,
            ..
        } = &mut *gestures;
        if let Some(primary) = primary_slot.as_mut() {
            if let Some(touch) = touches.get_pressed(primary.id) {
                let offset = touch.position() - primary.anchor;
                let frame_delta = touch.position() - primary.last_pos;
                primary.travel += frame_delta.length();
                primary.last_pos = touch.position();
                match scheme {
                    TouchScheme::Flick if swing_phase => {
                        next.action_held = true;
                        try_flick(flick_spent, &mut next, frame_delta, dt, scale);
                    }
                    TouchScheme::Tap if swing_phase => {
                        // A resting finger stays silent during the pitch: taps
                        // aim (claim loop), and letting the stick speak here
                        // would clobber a same-frame tap's aim with the resting
                        // finger's drag.
                    }
                    TouchScheme::HoldRelease if swing_phase => {
                        // Hold loads the meter; the drag offset aims.
                        next.action_held = true;
                        next.aim = stick_aim(offset, scale);
                    }
                    _ => {
                        // Generic virtual stick (also Tap/Flick outside the
                        // swing window, and Zone Pad fingers off both regions).
                        // A second finger's hold is Zone-Pad's SWING channel, so
                        // it stays off wholesale during Zone-Pad at-bats.
                        // Write only a nonzero deflection (a finger claimed this
                        // very frame has zero offset), and never over a
                        // same-frame tap's position aim (the flip-frame windup
                        // tap this order exists to serve) — but ONLY over a
                        // real one: `tap_aimed`, not `next.action`, which a
                        // defense action tap also sets.
                        let stick = stick_aim(offset, scale);
                        if stick != Vec2::ZERO && !tap_aimed {
                            next.aim = stick;
                        }
                        // Fingers on the live pause chrome belong to it — they
                        // are neither an action hold here nor (see the claim
                        // loop / adoption) any other game input.
                        next.action_held |= !zone_pad_at_bat
                            && touches.iter().any(|t| {
                                t.id() != primary.id && chrome_free(pause_fingers, live_chrome, t)
                            });
                    }
                }
            } else {
                // Finger lifted this frame (or vanished).
                if let Some(touch) = touches.iter_just_released().find(|t| t.id() == primary.id) {
                    let offset = touch.position() - primary.anchor;
                    let quick = now - primary.pressed_at <= TAP_MAX_SECS;
                    // The bare-tap test bounds the PATH travelled (accumulated
                    // per frame plus this frame's tail), not the net
                    // displacement: an aborted out-and-back drag ends near its
                    // start yet was never a tap, and firing an unaimed
                    // pitch/throw from a gesture the player cancelled is the
                    // exact wrong answer.
                    let travel = primary.travel + (touch.position() - primary.last_pos).length();
                    let small = travel <= TAP_MAX_MOVE_FRAC * scale;
                    match scheme {
                        TouchScheme::HoldRelease if swing_phase => {
                            // Release fires the meter swing; aim where dragged.
                            next.aim = stick_aim(offset, scale);
                        }
                        TouchScheme::Flick if swing_phase => {
                            // A flick whose movement and lift land in one frame
                            // batch (a browser hitch, or simply a sharp flick)
                            // never reaches the held-finger arm — measure the
                            // final frame's travel here, or the sharpest flicks
                            // are exactly the dropped ones.
                            try_flick(
                                flick_spent,
                                &mut next,
                                touch.position() - primary.last_pos,
                                dt,
                                scale,
                            );
                        }
                        _ if generic_action && primary.fresh && quick && small => {
                            // Generic bare tap = action press (under `Off` the
                            // pitch window doesn't suppress it — see the claim
                            // loop's second-finger note).
                            next.action = true;
                        }
                        _ => {}
                    }
                }
                *primary_slot = None;
            }
        }

        // ── Orphan adoption ──────────────────────────────────────────────────
        // A surviving finger inherits the stick when the tracked one lifts —
        // otherwise touch goes dead (aim zero, holds dropped) while a finger is
        // visibly still on the screen, until everything is lifted and
        // re-pressed. The anchor starts at the adoption point, so inheriting is
        // aim-neutral until the finger actually moves. Not during a Hold+Release
        // pitch: adopting there would read the resting finger as a fresh hold
        // and re-load the meter for a swing already spent.
        if gestures.primary.is_none() && !(swing_phase && scheme == TouchScheme::HoldRelease) {
            if let Some(touch) = touches.iter().find(|t| {
                Some(t.id()) != gestures.pad
                && Some(t.id()) != gestures.button
                && chrome_free(&gestures.pause_fingers, live_chrome, t)
                // A finger inside an active Zone Pad region was consumed by
                // the claim loop even when it took no role (a second thumb
                // brushing an owned pad) — "the brush becomes no other
                // input" must hold here too, or that thumb is adopted as
                // the stick and its slide inside the pad steers runners.
                && !(zone_pad_at_bat && zone_region_at(t.position(), size).is_some())
            }) {
                gestures.primary = Some(PrimaryTouch::tracked_from(
                    touch.id(),
                    touch.position(),
                    now,
                    false,
                ));
            }
        }

        next
    };
    out.set_if_neq(TouchIntent(next));
}

/// The pause-region tap path for `Paused`, where [`read_touch`] is off —
/// the same shared claim (see [`claim_pause_region_taps`]) so the grip that
/// paused can resume, and so the resume finger's id is remembered: still
/// held on the first resumed frame, it must not be adopted as the stick.
/// Liveness is re-decided here each frame too: `seen` and ownership CAN
/// flip mid-pause (`detect_touchscreen` and `resolve_touch_owner` both run
/// in `Paused`), and claiming against the value frozen at the pause left
/// the freshly painted button drawn-but-dead. Claims still use the
/// PRE-frame value — the same reveal discipline as `read_touch`, so the
/// touch that reveals the chrome can never press it.
pub fn paused_pause_region_taps(
    touches: Res<Touches>,
    windows: Query<&Window, With<PrimaryWindow>>,
    controllers: Res<Controllers>,
    mut gestures: ResMut<TouchGestures>,
    mut tapped: ResMut<crate::game::subs::PauseTapped>,
) {
    release_lifted_chrome_fingers(&touches, &mut gestures);
    let Ok(window) = windows.get_single() else {
        return;
    };
    let live_before = gestures.pause_chrome_live();
    gestures.pause_live = pause_chrome_visible(controllers.touch_team, gestures.seen);
    claim_pause_region_taps(
        &touches,
        pause_button_rect(window.size()),
        live_before,
        &mut gestures.pause_fingers,
        &mut tapped,
    );
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

    /// A minimal app that feeds *real* `TouchInput` events through Bevy's
    /// input plugin into [`read_touch`] — the gesture state machine itself
    /// (claiming, re-anchoring, re-arming, stickiness), not just the pure
    /// helpers, under test.
    fn translator_app(scheme: TouchScheme, home_bats: bool) -> (App, Entity) {
        use crate::game::flow::Play;
        use bevy::window::PrimaryWindow;

        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::input::InputPlugin));
        let window = app
            .world_mut()
            .spawn((
                Window {
                    resolution: (SIZE.x, SIZE.y).into(),
                    ..Default::default()
                },
                PrimaryWindow,
            ))
            .id();
        let mut play = Play::default();
        play.phase = Phase::Pitch;
        let score = crate::game::ScoreBoard {
            top_of_inning: !home_bats,
            ..Default::default()
        };
        let controllers = Controllers {
            touch_team: Some(Team::Home),
            ..Controllers::default()
        };
        app.insert_resource(play)
            .insert_resource(score)
            .insert_resource(controllers)
            .insert_resource(Settings {
                touch_scheme: scheme,
                ..Settings::default()
            })
            .init_resource::<TouchGestures>()
            .init_resource::<TouchIntent>()
            .init_resource::<crate::game::subs::PauseTapped>()
            .add_systems(PreUpdate, read_touch.after(bevy::input::InputSystem));
        (app, window)
    }

    fn send_touch(
        app: &mut App,
        window: Entity,
        phase: bevy::input::touch::TouchPhase,
        id: u64,
        pos: Vec2,
    ) {
        app.world_mut().send_event(bevy::input::touch::TouchInput {
            phase,
            position: pos,
            window,
            force: None,
            id,
        });
    }

    #[test]
    fn translator_tap_scheme_swings_from_a_real_touch_event() {
        use bevy::input::touch::TouchPhase;
        let (mut app, window) = translator_app(TouchScheme::Tap, true);
        send_touch(
            &mut app,
            window,
            TouchPhase::Started,
            7,
            Vec2::new(1100.0, 150.0),
        );
        app.update();
        let out = app.world().resource::<TouchIntent>().0;
        assert!(out.action, "a tap during the pitch is the swing");
        assert!(
            out.aim.x > 0.0 && out.aim.y > 0.0,
            "upper-right tap aims upper-right, got {:?}",
            out.aim
        );
        assert!(
            app.world().resource::<TouchGestures>().touch_seen(),
            "first contact must set the device-detection bit"
        );
    }

    /// One flick gesture: a downward reset move (its own frame — down never
    /// fires) followed by a large upward move. The 450 px sweep keeps the
    /// measured speed above `FLICK_TRIGGER_HPS` even when a loaded machine
    /// stretches the real dt between updates to ~0.5 s — a 100 px sweep
    /// flaked at >126 ms frame gaps under parallel test load.
    fn flick_up(app: &mut App, window: Entity, id: u64, x: f32) {
        use bevy::input::touch::TouchPhase;
        send_touch(app, window, TouchPhase::Moved, id, Vec2::new(x, 650.0));
        app.update();
        send_touch(app, window, TouchPhase::Moved, id, Vec2::new(x, 200.0));
        app.update();
    }

    #[test]
    fn translator_flick_rearms_every_pitch_for_a_resting_thumb() {
        use crate::game::flow::Play;
        use bevy::input::touch::TouchPhase;
        let (mut app, window) = translator_app(TouchScheme::Flick, true);
        // Rest the thumb, then flick up.
        send_touch(
            &mut app,
            window,
            TouchPhase::Started,
            7,
            Vec2::new(640.0, 650.0),
        );
        app.update();
        flick_up(&mut app, window, 7, 640.0);
        assert!(
            app.world().resource::<TouchIntent>().0.action,
            "first flick fires"
        );
        // Another upward sweep in the same pitch: spent.
        flick_up(&mut app, window, 7, 640.0);
        assert!(
            !app.world().resource::<TouchIntent>().0.action,
            "one swing per pitch"
        );
        // The pitch resolves and a new one arrives; the thumb never lifted.
        app.world_mut().resource_mut::<Play>().phase = Phase::Result;
        app.update();
        app.world_mut().resource_mut::<Play>().phase = Phase::Pitch;
        app.update();
        flick_up(&mut app, window, 7, 640.0);
        assert!(
            app.world().resource::<TouchIntent>().0.action,
            "entering a new pitch must re-arm the flick for a resting thumb"
        );
    }

    #[test]
    fn translator_zone_pad_claims_sticks_and_swings() {
        use bevy::input::touch::TouchPhase;
        let (mut app, window) = translator_app(TouchScheme::ZonePad, true);
        let pad_spot = pad_rect(SIZE).center();
        send_touch(&mut app, window, TouchPhase::Started, 1, pad_spot);
        app.update();
        let cursor = app.world().resource::<TouchIntent>().0.cursor;
        let resting = crate::game::batting::PciState::center();
        assert!(
            cursor.is_some_and(|c| (c - resting).length() < 1e-3),
            "pad-center touch aims the zone center, got {cursor:?}"
        );
        // Lift the finger: the cursor sticks at the last aimed spot.
        send_touch(&mut app, window, TouchPhase::Ended, 1, pad_spot);
        app.update();
        assert!(
            app.world().resource::<TouchIntent>().0.cursor.is_some(),
            "the absolute cursor is sticky after the finger lifts"
        );
        // The SWING button fires the press edge.
        send_touch(
            &mut app,
            window,
            TouchPhase::Started,
            2,
            swing_button_rect(SIZE).center(),
        );
        app.update();
        assert!(
            app.world().resource::<TouchIntent>().0.action,
            "a SWING button press is the swing edge"
        );
    }

    #[test]
    fn translator_single_frame_tap_still_acts_and_reveals_the_device() {
        use bevy::input::touch::TouchPhase;
        // A tap that presses AND releases between two frames (a fast real
        // tap, or a synthetic browser tap) must still fire the bare-tap
        // action and flip the device-detection bit — it only ever appears
        // in `just_pressed`, never among the held touches.
        let (mut app, window) = translator_app(TouchScheme::Off, false);
        let pos = Vec2::new(400.0, 360.0);
        send_touch(&mut app, window, TouchPhase::Started, 9, pos);
        send_touch(&mut app, window, TouchPhase::Ended, 9, pos);
        app.update();
        assert!(
            app.world().resource::<TouchGestures>().touch_seen(),
            "an instantaneous tap must still count as a seen touch"
        );
        assert!(
            app.world().resource::<TouchIntent>().0.action,
            "an instantaneous tap is still the bare-tap action"
        );
    }

    #[test]
    fn translator_defense_action_tap_keeps_the_drag_aim() {
        use bevy::input::touch::TouchPhase;
        // Tap scheme on DEFENSE: the second-finger action tap must not eat
        // the first finger's drag aim on the release frame — flow samples
        // the pitch/throw aim at exactly that action edge.
        let (mut app, window) = translator_app(TouchScheme::Tap, false);
        send_touch(
            &mut app,
            window,
            TouchPhase::Started,
            1,
            Vec2::new(600.0, 300.0),
        );
        app.update();
        // Full-deflection downward drag.
        send_touch(
            &mut app,
            window,
            TouchPhase::Moved,
            1,
            Vec2::new(600.0, 450.0),
        );
        app.update();
        send_touch(
            &mut app,
            window,
            TouchPhase::Started,
            2,
            Vec2::new(400.0, 300.0),
        );
        app.update();
        let out = app.world().resource::<TouchIntent>().0;
        assert!(out.action, "the second-finger tap is the action press");
        assert!(
            out.aim.y < -0.9,
            "the drag aim must survive the action frame, got {:?}",
            out.aim
        );
    }

    #[test]
    fn translator_windup_tap_swings_without_dropping_the_send_drag() {
        use crate::game::flow::Play;
        use bevy::input::touch::TouchPhase;
        // Tap scheme, batting, runner-send drag held through the windup: an
        // anticipatory swing tap must not clobber the send's Down aim even
        // for one frame (flow re-reads the send from aim every frame).
        let (mut app, window) = translator_app(TouchScheme::Tap, true);
        app.world_mut().resource_mut::<Play>().phase = Phase::WindUp;
        send_touch(
            &mut app,
            window,
            TouchPhase::Started,
            1,
            Vec2::new(600.0, 300.0),
        );
        app.update();
        send_touch(
            &mut app,
            window,
            TouchPhase::Moved,
            1,
            Vec2::new(600.0, 450.0),
        );
        app.update();
        send_touch(
            &mut app,
            window,
            TouchPhase::Started,
            2,
            Vec2::new(640.0, 100.0),
        );
        app.update();
        let out = app.world().resource::<TouchIntent>().0;
        assert!(out.action, "the windup tap still emits the swing edge");
        assert!(
            out.aim.y < -0.9,
            "the held send-drag owns the aim, got {:?}",
            out.aim
        );
    }

    #[test]
    fn translator_flick_second_thumb_cannot_double_swing_the_same_pitch() {
        use crate::game::flow::Play;
        use bevy::input::touch::TouchPhase;
        let (mut app, window) = translator_app(TouchScheme::Flick, true);
        // Two resting thumbs; A flicks and lifts, B is adopted mid-pitch.
        // A touches down a frame before B: two same-frame touchdowns reach
        // the claim loop in `Touches`' random HashMap order, and this test
        // needs A to be the tracked stick.
        send_touch(
            &mut app,
            window,
            TouchPhase::Started,
            1,
            Vec2::new(500.0, 650.0),
        );
        app.update();
        send_touch(
            &mut app,
            window,
            TouchPhase::Started,
            2,
            Vec2::new(800.0, 650.0),
        );
        app.update();
        flick_up(&mut app, window, 1, 500.0);
        assert!(
            app.world().resource::<TouchIntent>().0.action,
            "thumb A's flick fires"
        );
        send_touch(
            &mut app,
            window,
            TouchPhase::Ended,
            1,
            Vec2::new(500.0, 200.0),
        );
        app.update();
        // Thumb B (adopted) flicks in the same pitch: spent.
        flick_up(&mut app, window, 2, 800.0);
        assert!(
            !app.world().resource::<TouchIntent>().0.action,
            "one flick swing per pitch, whichever finger"
        );
        // Next pitch: the adopted thumb is re-armed like any other.
        app.world_mut().resource_mut::<Play>().phase = Phase::Result;
        app.update();
        app.world_mut().resource_mut::<Play>().phase = Phase::Pitch;
        app.update();
        flick_up(&mut app, window, 2, 800.0);
        assert!(
            app.world().resource::<TouchIntent>().0.action,
            "a new pitch re-arms the adopted thumb"
        );
    }

    #[test]
    fn translator_zone_pad_swing_hold_survives_the_between_pitch_release() {
        use crate::game::flow::Play;
        use bevy::input::touch::TouchPhase;
        let (mut app, window) = translator_app(TouchScheme::ZonePad, true);
        // Press SWING during the pitch (the edge fires)…
        send_touch(
            &mut app,
            window,
            TouchPhase::Started,
            1,
            swing_button_rect(SIZE).center(),
        );
        app.update();
        assert!(app.world().resource::<TouchIntent>().0.action);
        // …hold it through the Result (the regions release between pitches)
        // and the next PrePitch (the handoff must re-acquire the button)…
        app.world_mut().resource_mut::<Play>().phase = Phase::Result;
        app.update();
        app.world_mut().resource_mut::<Play>().phase = Phase::PrePitch;
        app.update();
        // …then the next delivery fires the anticipatory press: the held
        // button must not be dead for the whole pitch.
        app.world_mut().resource_mut::<Play>().phase = Phase::Pitch;
        app.update();
        assert!(
            app.world().resource::<TouchIntent>().0.action,
            "a SWING hold kept across the between-pitch release fires at the delivery"
        );
    }

    #[test]
    fn resolver_requires_a_seen_touchscreen() {
        // Selecting a scheme on the menu must not override a desktop
        // keyboard player's configured batting style with zero touches
        // ever seen — ownership needs `TouchGestures::seen`.
        let mut world = bevy::ecs::world::World::new();
        world.init_resource::<TouchGestures>();
        world.init_resource::<Touches>();
        world.insert_resource(Controllers::default());
        let mut system = bevy::ecs::system::IntoSystem::into_system(resolve_touch_owner);
        system.initialize(&mut world);
        system.run((), &mut world);
        assert_eq!(
            world.resource::<Controllers>().touch_team,
            None,
            "no touch seen: no owner"
        );
        world.resource_mut::<TouchGestures>().seen = true;
        system.run((), &mut world);
        assert_eq!(
            world.resource::<Controllers>().touch_team,
            Some(Team::Home),
            "a seen touchscreen grants the human Home slot ownership"
        );
    }

    #[test]
    fn translator_zone_pad_leaves_the_cursor_to_the_keyboard_when_touch_free() {
        use bevy::input::touch::TouchPhase;
        // A touchscreen was seen once (the session bit), but no finger is
        // down and nothing aimed the pad this at-bat: the translator must
        // emit NO absolute cursor, or keyboard/stick PCI steering would be
        // locked out for the rest of the session by one incidental brush.
        let (mut app, window) = translator_app(TouchScheme::ZonePad, true);
        app.world_mut().resource_mut::<TouchGestures>().seen = true;
        app.update();
        assert_eq!(
            app.world().resource::<TouchIntent>().0.cursor,
            None,
            "touch-free frame: the velocity path must stay live"
        );
        // A finger resting off-pad center-pins (a stray finger's stick aim
        // must not velocity-integrate the swing cursor)…
        send_touch(
            &mut app,
            window,
            TouchPhase::Started,
            3,
            Vec2::new(900.0, 200.0),
        );
        app.update();
        assert!(
            app.world().resource::<TouchIntent>().0.cursor.is_some(),
            "a finger on the glass pins the cursor absolutely"
        );
        // …and lifting it hands steering back to the keyboard.
        send_touch(
            &mut app,
            window,
            TouchPhase::Ended,
            3,
            Vec2::new(900.0, 200.0),
        );
        app.update();
        app.update();
        assert_eq!(
            app.world().resource::<TouchIntent>().0.cursor,
            None,
            "all fingers up and no pad aim: keyboard steering returns"
        );
    }

    #[test]
    fn translator_zone_pad_region_resting_fingers_do_not_center_pin() {
        use bevy::input::touch::TouchPhase;
        // The cycle-15 SWING-holder fix, one finger later: the pin filter
        // excludes region OWNERS by id, and must exclude roleless fingers
        // RESTING in a region by position too — the claim loop consumed
        // that brush ("it becomes no other input"), so it emits no stick
        // aim and cannot justify locking a keyboard steerer to center.
        let (mut app, window) = translator_app(TouchScheme::ZonePad, true);
        app.world_mut().resource_mut::<TouchGestures>().seen = true;
        let button = swing_button_rect(SIZE).center();
        // Separate frames: same-frame Started events reach the claim loop
        // in `Touches`' arbitrary HashMap order.
        send_touch(&mut app, window, TouchPhase::Started, 7, button);
        app.update();
        send_touch(&mut app, window, TouchPhase::Started, 8, button);
        app.update();
        let intent = &app.world().resource::<TouchIntent>().0;
        assert!(
            intent.action_held,
            "the first finger owns SWING and holds it"
        );
        assert_eq!(
            intent.cursor, None,
            "a roleless thumb resting on the owned button must not pin the cursor"
        );
    }

    #[test]
    fn translator_zone_pad_wandered_stick_still_pins_the_cursor() {
        use bevy::input::touch::TouchPhase;
        // The carve-out above is for ROLELESS region-resters only: the
        // tracked stick keeps emitting aim inside a region (the wander
        // rule), so it must keep forcing the absolute snap there — carved
        // out, its drag velocity-integrated the swing cursor, the exact
        // drift the pin is documented to stop.
        let (mut app, window) = translator_app(TouchScheme::ZonePad, true);
        app.world_mut().resource_mut::<TouchGestures>().seen = true;
        // Touch down OFF every region (tracked as the stick)…
        send_touch(
            &mut app,
            window,
            TouchPhase::Started,
            9,
            Vec2::new(900.0, 200.0),
        );
        app.update();
        // …then wander INTO the pad rect while still held.
        send_touch(
            &mut app,
            window,
            TouchPhase::Moved,
            9,
            pad_rect(SIZE).center(),
        );
        app.update();
        assert!(
            app.world().resource::<TouchIntent>().0.cursor.is_some(),
            "the tracked stick inside a region must still force the absolute snap"
        );
    }

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
