//! The per-frame gesture state machine: what each finger on the glass means
//! this frame.
//!
//! [`read_touch`] derives a [`Frame`] of shared per-frame facts, then runs
//! four passes over the live touches in the order a finger moves through
//! them — claimed by a control, read by whichever control owns it, and
//! inherited when that owner lifts. The whole read commits to
//! [`TouchIntent`] exactly once, at the single exit.
//!
//! The surface those passes read about — the rects, the aim mapping, the
//! ownership rules, and the [`TouchGestures`] resource itself — lives in the
//! parent module.

use super::*;

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
        let batting = score.batting_team() == team;

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

        // Everything the passes below share, derived once. Swing mapping is
        // live only while a swing is possible: the Zone Pad's regions are
        // claimed through the pre-contact phases (see `zone_pad_active` — the
        // overlay paints by the same predicate) so a finger resting on the pad
        // can pre-aim without reading as a runner-send drag — but NOT once the
        // ball is in play: a thumb landing in the lower-left then must steer
        // runners, not feed a dead pad.
        //
        // Tap's swing claim tolerates the WindUp→Pitch flip landing one frame
        // after this PreUpdate read (the delivery frame itself): the tap edge
        // is emitted through the windup too — the adapter ignores it outside
        // `Pitch`, so only a flip-frame tap gains anything — and the finger
        // still claims the stick, keeping the send-drag alive.
        //
        // `generic_action` is live outside the swing window, and under `Off`
        // during it too (that press IS the swing) — ONE spelling for its two
        // halves, the second-finger claim and the bare-tap release, which must
        // always agree.
        let swing_phase = batting && play.phase == Phase::Pitch;
        let pause_rect = pause_button_rect(size);
        let frame = Frame {
            scheme,
            size,
            scale: size.min_element(),
            now: time.elapsed_secs(),
            dt: time.delta_secs(),
            pad: pad_rect(size),
            pause: pause_rect,
            live_chrome: pause_live.then_some(pause_rect),
            // Decided above the early returns (see there).
            zone_pad_at_bat,
            zone_pad_started,
            swing_phase,
            tap_phase: batting && play.phase.in_delivery(),
            generic_action: !swing_phase || scheme == TouchScheme::Off,
        };

        // Pause-chrome touchdowns claimed first (see the shared helper): a
        // chrome finger must never double as a swing, pitch, or stick claim —
        // every pass below skips it by position. While hidden, the region is
        // ordinary screen.
        claim_pause_region_taps(
            &touches,
            frame.pause,
            pause_live,
            &mut gestures.pause_fingers,
            &mut pause_tap,
        );

        // The four passes, in the order a finger moves through them: claimed,
        // then read by whichever control owns it, then inherited when its
        // owner lifts.
        let tap_aimed = claim_new_fingers(&frame, &touches, &mut gestures, &mut next);
        steer_zone_pad(&frame, &touches, &mut gestures, &mut next);
        drive_primary(&frame, &touches, &mut gestures, &mut next, tap_aimed);
        adopt_orphan_stick(&frame, &touches, &mut gestures);

        next
    };
    out.set_if_neq(TouchIntent(next));
}

/// Everything this frame's passes read and none of them change: the window
/// geometry, the clock, and the handful of predicates decided once at the
/// top of [`read_touch`].
///
/// These used to be ten locals threaded implicitly through four inline
/// sections, which is what made the single-exit block hard to read — a pass
/// could reach for any of them and nothing said which it depended on. Naming
/// the set makes each pass's inputs its signature.
struct Frame {
    scheme: TouchScheme,
    size: Vec2,
    /// Gestures scale with the smaller window dimension (like the pad/button
    /// rects): height-scaling would make the same physical flick ~2x harder
    /// in portrait than landscape on a phone.
    scale: f32,
    now: f32,
    dt: f32,
    /// The Zone Pad's aiming rect, derived once per frame.
    pad: Rect,
    /// The pause button's rect, live or not — [`Frame::live_chrome`] pairs it
    /// with this frame's liveness.
    pause: Rect,
    /// `Some` only while the pause chrome is visible: hidden, its region is
    /// ordinary screen.
    live_chrome: Option<Rect>,
    zone_pad_at_bat: bool,
    /// Rising edge of `zone_pad_at_bat` — the resting-thumb handoff frame.
    zone_pad_started: bool,
    swing_phase: bool,
    tap_phase: bool,
    generic_action: bool,
}

impl Frame {
    /// One "the chrome owns this position" predicate for every claim site — a
    /// new piece of touch chrome registers here, not in three loops.
    fn chrome_claims(&self, pos: Vec2) -> bool {
        self.live_chrome.is_some_and(|r| r.contains(pos))
    }

    /// Whether `pos` sits in an *active* Zone Pad region. Fingers there were
    /// consumed by the claim pass even when they took no role ("the brush
    /// becomes no other input"), so the steer and adopt passes must carve
    /// them out by position, not by id.
    fn in_zone_region(&self, pos: Vec2) -> bool {
        self.zone_pad_at_bat && zone_region_at(pos, self.size).is_some()
    }
}

/// Claims fingers that touched down this frame: pause chrome first, then the
/// Zone Pad's regions, the Tap scheme's swing, and finally the virtual stick.
///
/// Returns whether a tap wrote a *position* aim. The stick pass must key on
/// that, not on `next.action`: on defense a second-finger action tap also
/// sets `action`, and eating the drag aim on that exact frame would turn
/// every aimed touch pitch/throw into an unaimed one — flow samples aim at
/// the action edge.
fn claim_new_fingers(
    f: &Frame,
    touches: &Touches,
    gestures: &mut TouchGestures,
    next: &mut TeamIntent,
) -> bool {
    let mut tap_aimed = false;
    // ── Claim new fingers ────────────────────────────────────────────────
    for touch in touches.iter_just_pressed() {
        if f.chrome_claims(touch.position()) {
            continue;
        }
        if f.zone_pad_at_bat {
            match zone_region_at(touch.position(), f.size) {
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
                        gestures.zone_cursor = Some(pad_zone_cursor(touch.position(), f.pad));
                    }
                    continue;
                }
                None => {}
            }
        }
        if f.tap_phase && f.scheme == TouchScheme::Tap {
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
            let dragging = !f.swing_phase
                && gestures.primary.is_some_and(|p| {
                    touches.get_pressed(p.id).is_some_and(|t| {
                        stick_aim(t.position() - p.anchor, f.scale).length() >= rules::AIM_DEADZONE
                    })
                });
            if !dragging {
                next.aim = tap_aim(touch.position(), f.size);
                tap_aimed = true;
            }
        }
        if gestures.primary.is_none() {
            // A Tap-f.scheme touchdown already fired its action edge in the
            // arm above — marking it fresh would let its quick release fire
            // the generic bare-tap arm too, one physical tap delivering two
            // presses (real in WindUp, where the release still lands
            // outside the swing window).
            let fresh = !(f.tap_phase && f.scheme == TouchScheme::Tap);
            gestures.primary = Some(PrimaryTouch::tracked_from(
                touch.id(),
                touch.position(),
                f.now,
                fresh,
            ));
        } else if f.generic_action {
            // Generic mapping: a second finger is the action button. Under
            // `Off` (no swing f.scheme) it stays live during the pitch too —
            // that press IS the swing, keeping a f.scheme-less touch device
            // fully playable instead of soft-locked.
            next.action = true;
        }
    }

    tap_aimed
}

/// The Zone Pad's continuous state: region handoff on the at-bat's rising
/// edge, the sticky aiming cursor, and the SWING button's hold.
fn steer_zone_pad(
    f: &Frame,
    touches: &Touches,
    gestures: &mut TouchGestures,
    next: &mut TeamIntent,
) {
    // ── Zone Pad continuous state ────────────────────────────────────────
    if f.zone_pad_at_bat {
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
        if f.zone_pad_started {
            for touch in touches.iter() {
                let id = touch.id();
                if gestures.pause_fingers.contains(&id) {
                    continue;
                }
                let claimed = match zone_region_at(touch.position(), f.size) {
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
                    gestures.zone_cursor = Some(pad_zone_cursor(touch.position(), f.pad));
                }
                None => {
                    gestures.pad = None;
                    // Promote a roleless thumb already resting on the
                    // pad (see `resting_roleless_in`), updating the
                    // sticky cursor from ITS position — without this,
                    // the lifted owner's stale aim kept emitting while
                    // the visible replacement thumb aimed elsewhere.
                    if let Some((id, pos)) =
                        resting_roleless_in(touches, f.size, ZoneRegion::Pad, gestures)
                    {
                        gestures.pad = Some(id);
                        gestures.zone_cursor = Some(pad_zone_cursor(pos, f.pad));
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
            chrome_free(&gestures.pause_fingers, f.live_chrome, t)
                && Some(id) != gestures.button
                && Some(id) != gestures.pad
                && (gestures.primary.is_some_and(|p| p.id == id)
                    || !(f.in_zone_region(t.position())))
        };
        // Inline, so the `||` short-circuits: `zone_cursor` is sticky
        // for the whole pitch once the thumb has aimed, and the scan
        // it would skip walks every finger twice, rebuilding both
        // region rects per finger per pass.
        if gestures.zone_cursor.is_some() || any_contact_matching(touches, gameplay) {
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
                    resting_roleless_in(touches, f.size, ZoneRegion::Button, gestures)
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
}

/// The tracked "primary" finger — the virtual stick, the flick, and the
/// hold/release — plus the bare-tap action its release can fire.
fn drive_primary(
    f: &Frame,
    touches: &Touches,
    gestures: &mut TouchGestures,
    next: &mut TeamIntent,
    tap_aimed: bool,
) {
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
            match f.scheme {
                TouchScheme::Flick if f.swing_phase => {
                    next.action_held = true;
                    try_flick(flick_spent, next, frame_delta, f.dt, f.scale);
                }
                TouchScheme::Tap if f.swing_phase => {
                    // A resting finger stays silent during the pitch: taps
                    // aim (claim loop), and letting the stick speak here
                    // would clobber a same-frame tap's aim with the resting
                    // finger's drag.
                }
                TouchScheme::HoldRelease if f.swing_phase => {
                    // Hold loads the meter; the drag offset aims.
                    next.action_held = true;
                    next.aim = stick_aim(offset, f.scale);
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
                    let stick = stick_aim(offset, f.scale);
                    if stick != Vec2::ZERO && !tap_aimed {
                        next.aim = stick;
                    }
                    // Fingers on the live pause chrome belong to it — they
                    // are neither an action hold here nor (see the claim
                    // loop / adoption) any other game input.
                    next.action_held |= !f.zone_pad_at_bat
                        && touches.iter().any(|t| {
                            t.id() != primary.id && chrome_free(pause_fingers, f.live_chrome, t)
                        });
                }
            }
        } else {
            // Finger lifted this frame (or vanished).
            if let Some(touch) = touches.iter_just_released().find(|t| t.id() == primary.id) {
                let offset = touch.position() - primary.anchor;
                let quick = f.now - primary.pressed_at <= TAP_MAX_SECS;
                // The bare-tap test bounds the PATH travelled (accumulated
                // per frame plus this frame's tail), not the net
                // displacement: an aborted out-and-back drag ends near its
                // start yet was never a tap, and firing an unaimed
                // pitch/throw from a gesture the player cancelled is the
                // exact wrong answer.
                let travel = primary.travel + (touch.position() - primary.last_pos).length();
                let small = travel <= TAP_MAX_MOVE_FRAC * f.scale;
                match f.scheme {
                    TouchScheme::HoldRelease if f.swing_phase => {
                        // Release fires the meter swing; aim where dragged.
                        next.aim = stick_aim(offset, f.scale);
                    }
                    TouchScheme::Flick if f.swing_phase => {
                        // A flick whose movement and lift land in one frame
                        // batch (a browser hitch, or simply a sharp flick)
                        // never reaches the held-finger arm — measure the
                        // final frame's travel here, or the sharpest flicks
                        // are exactly the dropped ones.
                        try_flick(
                            flick_spent,
                            next,
                            touch.position() - primary.last_pos,
                            f.dt,
                            f.scale,
                        );
                    }
                    _ if f.generic_action && primary.fresh && quick && small => {
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
}

/// A surviving finger inherits the stick when the tracked one lifts.
fn adopt_orphan_stick(f: &Frame, touches: &Touches, gestures: &mut TouchGestures) {
    // ── Orphan adoption ──────────────────────────────────────────────────
    // A surviving finger inherits the stick when the tracked one lifts —
    // otherwise touch goes dead (aim zero, holds dropped) while a finger is
    // visibly still on the screen, until everything is lifted and
    // re-pressed. The anchor starts at the adoption point, so inheriting is
    // aim-neutral until the finger actually moves. Not during a Hold+Release
    // pitch: adopting there would read the resting finger as a fresh hold
    // and re-load the meter for a swing already spent.
    if gestures.primary.is_none() && !(f.swing_phase && f.scheme == TouchScheme::HoldRelease) {
        if let Some(touch) = touches.iter().find(|t| {
            Some(t.id()) != gestures.pad
            && Some(t.id()) != gestures.button
            && chrome_free(&gestures.pause_fingers, f.live_chrome, t)
            // A finger inside an active Zone Pad region was consumed by
            // the claim loop even when it took no role (a second thumb
            // brushing an owned pad) — "the brush becomes no other
            // input" must hold here too, or that thumb is adopted as
            // the stick and its slide inside the pad steers runners.
            && !(f.in_zone_region(t.position()))
        }) {
            gestures.primary = Some(PrimaryTouch::tracked_from(
                touch.id(),
                touch.position(),
                f.now,
                false,
            ));
        }
    }
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

#[cfg(test)]
#[path = "translate.test.rs"]
mod tests;
