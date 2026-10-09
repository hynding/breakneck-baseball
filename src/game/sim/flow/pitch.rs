//! PrePitch through Pitch: the leadoff duel, the delivery, and the
//! swing/take judgment.

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::game::ScoreBoard;
use crate::game::animation::{AnimClip, Playing};
use crate::game::ball::{Baseball, HitEvent, InFlight, PitchEvent};
use crate::game::input::Intents;
use crate::game::player::{CatcherRole, Pitcher};
use crate::game::rules::{self, Bases, BattingOrder, Outcome, StrikeCall};
use crate::game::variant::{FieldSpec, Ruleset};

use super::live::wants_send;
use super::result::end_pitch;
use super::umpire::Umpire;
use super::{
    BallInPlayEvent, BannerTone, CURTAIN_SECS, ContactEvent, LeadState, Phase, PitchCaughtEvent,
    Play, PlayBanner, ResultBeat,
};

// ── Tuning constants ──────────────────────────────────────────────────────────

/// Z of home plate (the batter stands here).
const PLATE_Z: f32 = 0.0;

/// A swing connects while the ball is within this Z band of the plate. The
/// early edge is a fixed distance (a swing started this far out never
/// connects, whatever the timing model); the late edge is *not* fixed — see
/// [`late_swing_z`].
const SWING_EARLY_Z: f32 = 3.2; // ball this far in front = earliest contact
/// Maximum horizontal miss the batter can still reach.
const SWING_REACH_X: f32 = 1.8;

/// Home-run trot window: hang time plus a little air, clamped.
const INPLAY_BUFFER: f32 = 1.2;
const INPLAY_MIN: f32 = 2.2;
const INPLAY_MAX: f32 = 6.5;
/// Hard cap on an unresolved live play: hang time plus chase-and-throw room.
/// If nothing has resolved by then, the play is called from the current ball
/// state so the game can never stall.
const LIVE_PLAY_BUFFER: f32 = 5.0;
const LIVE_PLAY_MIN: f32 = 4.0;
const LIVE_PLAY_MAX: f32 = 11.0;

/// Signed swing-timing error, in milliseconds, at the instant the bat comes
/// through the zone: how far the ball is from the plate converted to time by
/// its own z-speed, signed so an **early** swing (ball still out in front,
/// `z > PLATE_Z`) is **negative** and a **late** swing (ball already past,
/// `z < PLATE_Z`) is positive. `vel_z` is the ball's z-velocity — negative,
/// since it travels toward the plate at −Z — and is clamped away from zero so
/// a stalled ball can't divide by zero. This is the seam
/// [`rules::contact_quality`] grades. Callers timing a *press* (the CPU, the
/// Director, the Meter's forced swing) want [`swing_dt_at_contact_ms`],
/// which looks the swing's startup ahead.
pub(crate) fn swing_dt_ms(ball_z: f32, vel_z: f32) -> f32 {
    1000.0 * (ball_z - PLATE_Z) / vel_z.min(-f32::EPSILON)
}

/// Seconds from the swing press to the bat coming through the zone — the
/// swing's startup. A press *starts* the swing; the ball is judged (and
/// leaves the bat, or is missed) only once the bat arrives, so the swing is
/// seen before its consequence, the way the reference footage plays it
/// (docs/agent/SMB3-REFERENCE-NOTES.md §2.3/§2.5). Pinned to the authored
/// `BatterSwing` clip: its arms whip through the zone at ~35% of 0.42 s
/// (`animation::BATTER_SWING_CONTACT_FRACTION`, tested against this value),
/// so the ball leaves the bat on the frame the bat visibly gets there.
pub const SWING_CONTACT_SECS: f32 = 0.15;

/// Where the ball will be when a bat pressed *now* comes through the zone:
/// the press-timing helper every synthetic batter (CPU, Director scripts,
/// tests) uses to aim a press so the *bat*, not the button, meets the ball
/// at the wanted spot. Straight-line extrapolation: over the startup the
/// z-speed drifts by a couple of ms of timing at most, inside every window.
pub fn bat_arrival_z(ball_z: f32, vel_z: f32) -> f32 {
    ball_z + vel_z * SWING_CONTACT_SECS
}

/// [`swing_dt_ms`] as it will read when a swing pressed now is judged — the
/// timing error the bat arrives with.
pub fn swing_dt_at_contact_ms(ball_z: f32, vel_z: f32) -> f32 {
    swing_dt_ms(bat_arrival_z(ball_z, vel_z), vel_z)
}

/// A swing the batter has committed to, waiting for the bat to come through
/// the zone (`Play::pitch.swing`).
#[derive(Clone, Copy, Debug)]
pub(super) struct PendingSwing {
    input: crate::game::batting::SwingInput,
    /// `Time::elapsed_secs` at which the bat arrives and the swing is judged.
    at: f32,
}

/// The Z at which a swing's timing error would read exactly `foul_ms` late —
/// i.e. `swing_dt_ms(late_swing_z(vel_z, foul_ms), vel_z) == foul_ms` — solved
/// by inverting [`swing_dt_ms`] at `dt_ms == foul_ms`. This *replaces* a fixed
/// distance-past-the-plate constant as the swing window's late edge (docs/
/// superpowers/specs/2026-07-30-batting-feel-design.md §2): a fixed cutoff
/// can't track the tuned foul window or the live pitch speed, so at typical
/// game speeds a constant like −1.2 m only ever reaches ~40 ms of lateness —
/// no `cpu_timing_spread_ms` (or a genuinely late human press) could ever
/// reach a timing-driven `FoulTip`/`Whiff`, because the window closed (and
/// the pitch got judged a take) long before the timing math got there. The
/// spatial band is now the geometric shadow of the timing model's own foul
/// window, so it stays open exactly as long as a swing can still be graded
/// by `contact_quality` — no longer, no shorter.
pub(crate) fn late_swing_z(vel_z: f32, foul_ms: f32) -> f32 {
    foul_ms * vel_z.min(-f32::EPSILON) / 1000.0
}

/// The steal window a fresh at-bat opens: the ruleset's duel length whenever
/// a runner is actually in a position to steal ([`rules::steal_candidate`]),
/// nothing otherwise — a runner parked on third alone gates no pitch.
pub(super) fn steal_window_for(bases: &Bases, rules: &Ruleset) -> Timer {
    let secs = if rules::steal_candidate(bases).is_some() {
        rules.counts.steal_window_secs
    } else {
        0.0
    };
    Timer::from_seconds(secs, TimerMode::Once)
}

/// Fresh play + base state whenever a game (re)starts. The base count follows
/// the chosen field.
pub(super) fn reset_flow(
    mut play: ResMut<Play>,
    mut bases: ResMut<Bases>,
    mut order: ResMut<BattingOrder>,
    mut lead: ResMut<LeadState>,
    field: Res<FieldSpec>,
) {
    *play = Play::default();
    bases.reset_for(field.base_count());
    *order = BattingOrder::default();
    lead.extended = false;
}

// ── PrePitch: the leadoff duel, then the defense aims and releases ────────────

#[allow(clippy::too_many_arguments)]
pub(super) fn pre_pitch(
    time: Res<Time>,
    mut play: ResMut<Play>,
    intents: Res<Intents>,
    mut score: ResMut<ScoreBoard>,
    mut bases: ResMut<Bases>,
    rules_res: Res<Ruleset>,
    mut lead: ResMut<LeadState>,
    mut banner: MessageWriter<PlayBanner>,
    pitcher_q: Query<Entity, With<Pitcher>>,
    mut commands: Commands,
) {
    if play.phase != Phase::PrePitch {
        return;
    }
    if walk_up(&mut play, &intents, &score, time.delta()) {
        return;
    }
    play.duel.hold.tick(time.delta());
    play.duel.pickoff_cooldown.tick(time.delta());

    // The offense works the lead: holding Down stretches the lead runner off
    // the bag — the guaranteed steal jump, bought at pickoff risk.
    let offense = intents.get(score.batting_team());
    lead.extended = wants_send(offense.aim) && rules::steal_candidate(&bases).is_some();

    let intent = intents.get(score.fielding_team());
    if play.in_steal_window() {
        // Only a stretch *held through* the window (while the pickoff threat
        // is live) earns the guaranteed jump at delivery — retreating to the
        // bag forfeits it, so a one-frame pulse can't bank a risk-free jump.
        play.duel.window_lead = lead.extended;
        // The duel window: the ball is held. A defensive action here is a
        // pickoff throw at the leading runner, not a pitch — one throw per
        // reload, so a held button can't spam the bag.
        if intent.action && play.duel.pickoff_cooldown.is_finished() {
            play.duel.pickoff_cooldown =
                Timer::from_seconds(rules_res.pace.pickoff_cooldown_secs, TimerMode::Once);
            let outs_before = score.outs;
            match rules::attempt_pickoff(&mut score, &mut bases, &rules_res, lead.extended) {
                rules::PickoffResult::PickedOff { .. } => {
                    // The out count follows the call (the rules zero it as
                    // they change sides, so a third out reads as the side).
                    let outs = if score.outs > outs_before {
                        score.outs
                    } else {
                        rules_res.counts.outs_per_half
                    };
                    banner.write(
                        PlayBanner::new("PICKED OFF!", BannerTone::Bad)
                            .with_follow_up(format!("OUT {outs}")),
                    );
                    // A pickoff out is a play: it takes the same result
                    // pause as any other out (banner linger + runners
                    // settling) before the next window can open.
                    end_pitch(&mut play, &rules_res.pace, ResultBeat::Pickoff);
                }
                rules::PickoffResult::SafeBack => {
                    banner.write(PlayBanner::new("BACK IN TIME", BannerTone::Info));
                }
                rules::PickoffResult::NoRunner => {}
            }
        }
        return;
    }

    if intent.action {
        play.pitch.pending = Some((intent.aim, rules::PitchKind::from_aim(intent.aim)));
        play.phase = Phase::WindUp;
        play.timer = Timer::from_seconds(AnimClip::WindUp.duration(), TimerMode::Once);
        play.pitch.crossing = None;
        play.resolved = false;
        play.pitch.taken = false;
        play.pitch.presentational_catch = false;
        // A lead still stretched at first movement sends the runner with the
        // delivery. It's only the no-throw-beats-it jump when the stretch
        // was made during the window — that's the extension that paid the
        // pickoff risk; stretching only after the window is a late break.
        if lead.extended {
            play.duel.armed = true;
            play.duel.big_jump = play.duel.window_lead;
        }
        for pitcher in &pitcher_q {
            commands
                .entity(pitcher)
                .insert(Playing::then(AnimClip::WindUp, AnimClip::ThrowRelease));
        }
    }
}

/// The walk-up hold (TODO 103): while the incoming batter is introduced the
/// ball is held — no steal window ticks, no pitch is accepted. The batting
/// side's action press ends it, or it expires on its own; either way it
/// leaves through the curtain, so the cut to the duel framing lands on a
/// black screen. Returns whether the pitch is still held.
fn walk_up(
    play: &mut Play,
    intents: &Intents,
    score: &ScoreBoard,
    dt: std::time::Duration,
) -> bool {
    let Some(walkup) = play.walkup.as_mut() else {
        return false;
    };
    walkup.tick(dt);
    let dismissed = intents.get(score.batting_team()).action;
    if play.curtain.is_some() || walkup.is_finished() || dismissed {
        let curtain = play
            .curtain
            .get_or_insert_with(|| Timer::from_seconds(CURTAIN_SECS, TimerMode::Once));
        if curtain.tick(dt).is_finished() {
            play.walkup = None;
            play.curtain = None;
        }
    }
    true
}

// ── WindUp: the delivery plays out, then the ball leaves the hand ─────────────

#[allow(clippy::too_many_arguments)]
pub(super) fn wind_up(
    time: Res<Time>,
    mut play: ResMut<Play>,
    field: Res<FieldSpec>,
    rules: Res<Ruleset>,
    intents: Res<Intents>,
    score: Res<ScoreBoard>,
    bases: Res<Bases>,
    mut lead: ResMut<LeadState>,
    mut pitch_ev: MessageWriter<PitchEvent>,
) {
    if play.phase != Phase::WindUp {
        // Guarded like every reset in this diff — tick hygiene, not cost.
        if play.duel.send_prev {
            play.duel.send_prev = false;
        }
        return;
    }
    // Holding the stick down through the delivery sends the lead runner (the
    // late break: a classic race against the catcher, no guaranteed jump).
    // Nobody in a position to steal means nobody is going. HELD is literal —
    // evidence from more than one instant: a touch tap's position aim (a
    // low swing tap) lands for exactly one frame, and latching off it sent
    // runners nobody called. Either a prior wind-up frame held the send
    // (`windup_send_prev`) or the pre-pitch lead was stretched coming in
    // (`lead.extended` — the same held-Down, so a one-frame wind-up on a
    // hitching tab still honors a hold that predates it). Real holds span
    // both; a one-frame blip has neither.
    let send_now = wants_send(intents.get(score.batting_team()).aim)
        && rules::steal_candidate(&bases).is_some();
    if send_now && (play.duel.send_prev || lead.extended) {
        play.duel.armed = true;
        lead.extended = true;
    }
    play.duel.send_prev = send_now;
    if play.timer.tick(time.delta()).is_finished() {
        let (aim, kind) = play
            .pitch
            .pending
            .take()
            .unwrap_or((Vec2::ZERO, rules::PitchKind::Changeup));
        pitch_ev.write(PitchEvent {
            velocity: rules::pitch_velocity_kind(
                kind,
                aim,
                field.pitch_distance,
                rules.pace.pitch_speed_scale,
            ),
            spin: kind.spin(),
        });
        play.pitch.kind = Some(kind);
        play.phase = Phase::Pitch;
    }
}

// ── Pitch: batter may swing; otherwise judge the take ─────────────────────────

#[allow(clippy::too_many_arguments)]
pub(super) fn pitch_live(
    time: Res<Time>,
    mut play: ResMut<Play>,
    mut swing_commands: ResMut<crate::game::batting::SwingCommands>,
    rules: Res<Ruleset>,
    field: Res<FieldSpec>,
    mut score: ResMut<ScoreBoard>,
    mut bases: ResMut<Bases>,
    ball_q: Query<(&Transform, &Velocity), With<Baseball>>,
    mut hit_ev: MessageWriter<HitEvent>,
    mut in_play_ev: MessageWriter<BallInPlayEvent>,
    mut contact_ev: MessageWriter<ContactEvent>,
    mut banner: MessageWriter<PlayBanner>,
    mut order: ResMut<BattingOrder>,
    #[cfg(feature = "debug")] forced: Res<crate::game::debug::ForcedContact>,
) {
    if play.phase != Phase::Pitch || play.resolved {
        return;
    }
    let Ok((ball, ball_vel)) = ball_q.single() else {
        return;
    };
    let pos = ball.translation;

    // Record the plate-crossing location once.
    if play.pitch.crossing.is_none() && pos.z <= PLATE_Z + 0.1 {
        play.pitch.crossing = Some(Vec2::new(pos.x, pos.y));
    }

    // Captured before any resolution can flip the half-inning: the batting
    // order advances for the team whose batter just finished, not whoever
    // bats next.
    let batter = score.batting_team();

    // The late edge of the swing window is the geometric shadow of the foul
    // window itself (see `late_swing_z`), recomputed off the ball's live
    // z-speed every frame since it isn't a fixed distance.
    let late_exit_z = late_swing_z(ball_vel.linvel.z, rules.batting.foul_ms);

    // NOTE: the `Umpire` is built inside each branch that actually makes a
    // call, never hoisted above them. `&mut` on a `ResMut` goes through
    // `DerefMut`, which calls `set_changed()` — a borrow taken once at the top
    // would mark `ScoreBoard` and `Bases` changed on every frame of the pitch
    // flight, defeating the `is_changed()` guards in the HUD, the jersey and
    // team-colour painters, and `runner::sync_runners` (TODO 78).

    // A press starts the swing; the bat comes through the zone
    // `SWING_CONTACT_SECS` later, and that is when it is judged. A second
    // press mid-swing is nothing (the rig ignores it too: `trigger_swing`).
    if let Some(input) = swing_commands.take(batter) {
        if play.pitch.swing.is_none() {
            play.pitch.swing = Some(PendingSwing {
                input,
                at: time.elapsed_secs() + SWING_CONTACT_SECS,
            });
        }
    }

    if let Some(PendingSwing { input: swing, .. }) =
        play.pitch.swing.filter(|s| time.elapsed_secs() >= s.at)
    {
        play.pitch.swing = None;
        // The spatial band is the OUTER eligibility gate: a ball out of the
        // batter's reach is a whiff regardless of timing. Within the band,
        // `contact_quality` grades the swing off its timing error — so a Whiff
        // now covers a band-miss AND a badly-mistimed in-band swing uniformly.
        let reachable =
            pos.z >= late_exit_z && pos.z <= SWING_EARLY_Z && pos.x.abs() <= SWING_REACH_X;
        let dt_ms = swing_dt_ms(pos.z, ball_vel.linvel.z);
        // Classic/Meter grade on timing alone; PCI also folds in how far the
        // aiming cursor sat from the ball at the contact point (spec §3).
        #[allow(unused_mut)]
        let mut quality = if !reachable {
            rules::ContactQuality::Whiff
        } else if let Some(cursor) = swing.pci_offset {
            let miss = cursor.distance(Vec2::new(pos.x, pos.y));
            rules::pci_contact_quality(dt_ms, miss, &rules)
        } else {
            rules::contact_quality(dt_ms, &rules)
        };
        #[cfg(feature = "debug")]
        if let Some(f) = forced.0 {
            quality = f;
        }
        // Direction: PCI derives it from the contact-point offset (spec §3);
        // Classic/Meter use the raw aim held at the swing.
        let aim = match swing.pci_offset {
            Some(cursor) => rules::pci_aim(cursor - Vec2::new(pos.x, pos.y)),
            None => swing.aim,
        };
        // Fired on every judged swing for later presentation systems; the
        // rules/physics consequence follows below. A whiff's report waits
        // for its call (see the `Whiff` arm) so the timing stamp lands
        // with the strike text, not ahead of it.
        if quality != rules::ContactQuality::Whiff {
            contact_ev.write(ContactEvent {
                quality,
                batting_team: batter,
                dt_ms,
            });
            // Remember this swing's grade for presentation — the home-run
            // fireworks scale up off a dead-on Perfect (see `game::fx`).
            play.live.last_contact_quality = Some(quality);
        }
        match quality {
            // A ball in play, shaped by the quality's exit multiplier and the
            // timing-driven pull yaw. (`Weak` never comes from the Classic
            // windows — it's the Plan-C PCI adapter's outcome — but it belongs
            // on this arm so the match stays exhaustive and Plan C is a no-op.)
            rules::ContactQuality::Perfect
            | rules::ContactQuality::Solid
            | rules::ContactQuality::Weak => {
                let base = rules::hit_velocity(pos.z, aim);
                let velocity = rules::apply_contact_quality(base, quality, dt_ms, &rules);
                hit_ev.write(HitEvent { velocity });
                let (landing, hang_time) = rules::predict_landing(
                    velocity,
                    rules::hit_spin(velocity),
                    crate::game::ball::BALL_DRAG_FACTOR,
                    crate::game::ball::MAGNUS_FACTOR,
                );
                let kind = rules::classify_contact(landing, &field);
                let contact_class = rules::contact_class(landing, hang_time, &field);
                in_play_ev.write(BallInPlayEvent {
                    kind,
                    landing,
                    contact_class,
                });
                play.live.contact_at = time.elapsed_secs();
                play.phase = Phase::InPlay;
                let mut ump = Umpire::new(&mut score, &mut bases, &rules, &mut banner);
                settle_batted_ball(kind, hang_time, &mut play, &mut ump, &mut order, batter);
            }
            // A foul tip: a strike (never the third — see `rules::foul`). The
            // ball is dead, so runners hold (no steal is resolved) and the
            // at-bat continues. The pull-side sign rides along on the
            // ContactEvent for later presentation.
            rules::ContactQuality::FoulTip => {
                Umpire::new(&mut score, &mut bases, &rules, &mut banner).foul_tip();
                play.pitch.taken = true; // the catcher gloves the tipped ball
                end_pitch(&mut play, &rules.pace, ResultBeat::Foul);
            }
            // A swing and miss: the bat has come through and the ball is
            // still flying. The call waits for the ball to cross the plate
            // (the take trigger below) so the miss is seen before it is
            // announced — the reference's bat-passes → ball-in-the-mitt →
            // STRIKE beat. Nothing else about the pitch changes: it flies
            // on untouched and `catcher_receives` gloves it as a take.
            rules::ContactQuality::Whiff => {
                play.pitch.whiff = Some(dt_ms);
            }
        }
        if play.pitch.whiff.is_none() {
            return;
        }
    }

    // A swing still on its way through can't be judged yet, whatever the
    // ball does meanwhile (it will grade a whiff when the bat arrives).
    if play.pitch.swing.is_some() {
        return;
    }

    // Once the ball is past the foul window's own late edge (a swing here
    // couldn't grade as anything but a take anyway), judge it: the stashed
    // swing-through if the bat already came and went, else the take.
    if pos.z < late_exit_z {
        let mut ump = Umpire::new(&mut score, &mut bases, &rules, &mut banner);
        let beat = if let Some(dt_ms) = play.pitch.whiff.take() {
            // The miss's report lands with its call: whoosh, EARLY/LATE
            // stamp and the strike text on one frame (audio pairs the two
            // to hear a *swinging* strikeout).
            contact_ev.write(ContactEvent {
                quality: rules::ContactQuality::Whiff,
                batting_team: batter,
                dt_ms,
            });
            play.live.last_contact_quality = Some(rules::ContactQuality::Whiff);
            // Swinging through a curveball in the dirt with first base
            // open: the catcher can't hold strike three and the batter
            // runs.
            judge_whiff(&mut play, &mut ump, &mut order, batter)
        } else {
            let cross = play.pitch.crossing.unwrap_or(Vec2::new(pos.x, pos.y));
            judge_take(cross, &mut play, &mut ump, &mut order, batter)
        };
        end_pitch(&mut play, &rules.pace, beat);
    }
}

/// A pitch nobody offered at: a plunking, a called strike, or a ball.
///
/// `cross` is the plate-crossing point the call is judged from — recorded as
/// the ball passed the plate, not sampled here, so a late judgment still
/// grades the pitch where the umpire saw it.
fn judge_take(
    cross: Vec2,
    play: &mut Play,
    ump: &mut Umpire,
    order: &mut BattingOrder,
    batter: crate::game::Team,
) -> ResultBeat {
    if rules::hits_batter(cross) {
        // Dead ball: the batter takes first, forced runners move.
        ump.hit_by_pitch();
        order.advance(batter);
        return ResultBeat::Walk;
    }
    play.pitch.taken = true;
    let (pa_over, walked, beat) = if rules::is_in_zone(cross) {
        let call = ump.add_strike(false, false);
        play.pitch.last_strike_call = Some(call);
        (
            call != StrikeCall::Strike,
            false,
            ResultBeat::for_strike(call),
        )
    } else {
        let walked = ump.add_ball();
        let beat = if walked {
            ResultBeat::Walk
        } else {
            ResultBeat::Ball
        };
        (walked, walked, beat)
    };
    if pa_over {
        order.advance(batter);
    }
    // A walk is a dead ball (runners advance freely); otherwise a sent runner
    // has to beat the catcher's throw.
    if play.duel.armed && !walked {
        ump.resolve_steal(play);
    }
    beat
}

/// A swing and miss. The at-bat may end here (a strikeout, or a dropped third
/// the batter runs out); the caller ends the pitch either way.
fn judge_whiff(
    play: &mut Play,
    ump: &mut Umpire,
    order: &mut BattingOrder,
    batter: crate::game::Team,
) -> ResultBeat {
    // Swinging through a curveball in the dirt with first base open: the
    // catcher can't hold strike three and the batter runs.
    let dropped = play.pitch.kind == Some(rules::PitchKind::Curveball) && !ump.bases_occupied(0);
    let call = ump.add_strike(true, dropped);
    play.pitch.last_strike_call = Some(call);
    // The catcher gloves everything except the strike three that got away
    // (that one is in the dirt by definition).
    play.pitch.taken = call != StrikeCall::DroppedThird;
    if call != StrikeCall::Strike {
        order.advance(batter);
    }
    // The catcher has the ball: a sent runner must survive the throw.
    if play.duel.armed {
        ump.resolve_steal(play);
    }
    ResultBeat::for_strike(call)
}

/// Where a ball that left the bat goes next: only a ball over the fence is
/// settled at contact. Everything else stays live — the fielders' chase and
/// the runner races decide the call in `resolve_live_play` — and the timer is
/// the backstop, sized off the predicted hang time.
fn settle_batted_ball(
    kind: rules::ContactKind,
    hang_time: f32,
    play: &mut Play,
    ump: &mut Umpire,
    order: &mut BattingOrder,
    batter: crate::game::Team,
) {
    match kind {
        rules::ContactKind::HomeRun => {
            play.live.home_run = true;
            ump.resolve_contact(Outcome::HomeRun, play.duel.armed);
            order.advance(batter);
            play.timer = Timer::from_seconds(
                (hang_time + INPLAY_BUFFER).clamp(INPLAY_MIN, INPLAY_MAX),
                TimerMode::Once,
            );
            play.resolved = true;
        }
        rules::ContactKind::Live { .. } => {
            play.timer = Timer::from_seconds(
                (hang_time + LIVE_PLAY_BUFFER).clamp(LIVE_PLAY_MIN, LIVE_PLAY_MAX),
                TimerMode::Once,
            );
            play.resolved = false;
        }
    }
}

// ── The catcher receives ──────────────────────────────────────────────────────

/// Stops an untouched pitch in the catcher's mitt instead of letting it sail
/// past. Parks without a catcher (the front yard) let the ball fly as before.
///
/// The take/swing-through judgment and the *visual* catch are deliberately
/// on two different clocks now (review fix for the widened `late_swing_z`
/// window): the timing dial needs the pitch to keep flying — untouched, at
/// its true position — for a genuinely late press to still grade through
/// `Solid`/`FoulTip` (see `late_swing_z`'s doc comment), but nobody should
/// *see* it sail through the catcher and plate umpire while that's settled.
/// So the ball is hidden **presentationally** the instant it reaches the
/// glove's proximity, whichever phase the pitch is logically in — the catch
/// pop plays right then — while the real flight (position, velocity) is left
/// completely untouched underneath, so a still-possible late swing keeps
/// reading the honest trajectory. Only once the pitch is *officially* judged
/// (a take, or a swing that grades a whiff/foul tip) does the ball actually
/// stop and park at the glove — invisibly, since it was already hidden, so
/// there's no visible jump. A legitimately late hit (or a dropped third, or
/// an HBP) un-hides it immediately: it was never really caught.
#[allow(clippy::type_complexity)]
pub(super) fn catcher_receives(
    mut play: ResMut<Play>,
    catchers: Query<(Entity, &Transform), (With<CatcherRole>, Without<Baseball>)>,
    mut ball_q: Query<
        (Entity, &mut Transform, &mut Velocity, &mut Visibility),
        (With<Baseball>, With<InFlight>),
    >,
    mut caught: MessageWriter<PitchCaughtEvent>,
    mut commands: Commands,
) {
    let Some((catcher, catcher_tf)) = catchers.iter().next() else {
        return;
    };
    let Ok((ball, mut ball_tf, mut vel, mut vis)) = ball_q.single_mut() else {
        return;
    };
    let pos = ball_tf.translation;
    let approaching_glove = pos.z <= catcher_tf.translation.z + 0.6 && vel.linvel.z < 0.0;
    let catchable_height = (0.12..=2.4).contains(&pos.y); // not in the dirt or sailing high

    if play.phase == Phase::Result && play.pitch.taken {
        // Officially judged: freeze it at the glove for real. If it was
        // hidden already this is invisible — the transform simply now
        // matches where the glove already showed it.
        play.pitch.taken = false;
        if !catchable_height {
            *vis = Visibility::Inherited; // shouldn't have been hidden; make sure
            return; // in the dirt or over everything: play it off the backstop
        }
        ball_tf.translation = catcher_tf.translation + Vec3::new(0.0, 0.5, 0.45);
        vel.linvel = Vec3::ZERO;
        vel.angvel = Vec3::ZERO;
        *vis = Visibility::Inherited;
        // Officially in the mitt (whether or not the presentational pop
        // already played) — the camera holds the at-bat framing on it.
        play.pitch.gloved = true;
        commands.entity(ball).remove::<InFlight>();
        if !play.pitch.presentational_catch {
            // No earlier presentational pop (the decision landed before the
            // ball reached the glove) — this is the first and only catch.
            commands
                .entity(catcher)
                .insert(Playing::new(AnimClip::GloveUp));
            caught.write(PitchCaughtEvent);
        }
        play.pitch.presentational_catch = false;
        return;
    }

    if play.phase == Phase::Pitch {
        if play.pitch.presentational_catch {
            return; // already hidden; still waiting on the official judgment
        }
        // Not judged yet — a swing could still land (the timing dial can
        // reach well past the catcher). Hide it here, purely for show, the
        // instant it's in glove range, so it never visibly sails through.
        if !approaching_glove || !catchable_height {
            return;
        }
        let cross = play.pitch.crossing.unwrap_or(Vec2::new(pos.x, pos.y));
        if rules::hits_batter(cross) {
            return; // headed for an HBP call: stays visible, plays through
        }
        *vis = Visibility::Hidden;
        play.pitch.presentational_catch = true;
        play.pitch.gloved = true;
        commands
            .entity(catcher)
            .insert(Playing::new(AnimClip::GloveUp));
        caught.write(PitchCaughtEvent);
        return;
    }

    // Anything else with the ball still tagged `InFlight` (a live hit, a
    // dropped third, an HBP that already fired) is not a catch after all —
    // never leave a presentational hide stuck on a ball that's actually
    // still live, and never leave the camera holding a "gloved" framing on
    // a ball that got away.
    if play.pitch.presentational_catch {
        *vis = Visibility::Inherited;
        play.pitch.presentational_catch = false;
        play.pitch.gloved = false;
    }
}

#[cfg(test)]
#[path = "pitch.test.rs"]
mod tests;
