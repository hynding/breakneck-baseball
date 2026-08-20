//! Duel-phase animation behaviors: the batter's stance/fidgets, the
//! catcher's crouch, the swing trigger, and the home-run celebration chain.

use bevy::prelude::*;

use crate::game::ScoreBoard;
use crate::game::animation;
use crate::game::animation::{AnimClip, Playing};
use crate::game::flow::{BallInPlayEvent, Phase, Play};
use crate::game::input::Intents;
use crate::game::roster::{PlayerIdentity, Rosters};
use crate::game::rules::{BattingOrder, ContactKind};

use super::{BatPivot, Batter, CatcherRole, PlateUmpire};

/// Holds the plate batter (the `Batter`-marker rig) in his personal batting
/// stance through the duel — resolved from `PlayerIdentity` via
/// `Rosters::team(..).card(..).appearance.style.stance` and
/// `animation::stance_clip`, falling back to the shared `AnimClip::BattingStance`
/// when the batter has no identity yet — the mirror of [`catcher_crouch`] on
/// the other side of the plate — and releases it the moment the ball is in
/// play, so the swap to the run-out rig (or a return to `Idle` between
/// at-bats) can take over. Runs before [`trigger_swing`] (via `.chain()`) so
/// a swing pressed on the very first duel frame still lands: even if this
/// system's insert reaches the batter first, `trigger_swing`'s widened gate
/// immediately replaces the held stance with `BatterSwing` the same frame.
/// Registered `.after(IdentitySet)` so it never reads a stale identity on the
/// exact frame a new batter steps up (the established `dress_jerseys`
/// pattern in jersey.rs).
///
/// Also owns cutting a fidget short: `batter_fidgets` only *starts* one in
/// `Phase::PrePitch`, but an 0.8–0.9 s fidget clip started late in PrePitch
/// can still be mid-flight when the duel moves to `WindUp`/`Pitch` — and a
/// fidget clip left in place there would both violate "fidgets exist only in
/// PrePitch" and (via `trigger_swing`'s stance-only gate) eat a real swing
/// press for up to 0.9 s. So whenever the batter is dueling past PrePitch, or
/// not dueling at all, any fidget in flight is force-replaced with the
/// resolved stance (dueling) or dropped outright (`!dueling`), mirroring the
/// stance-removal arm below.
pub(super) fn batter_stance(
    play: Res<Play>,
    identities: Query<&PlayerIdentity>,
    rosters: Res<Rosters>,
    batters: Query<(Entity, Option<&Playing>), With<Batter>>,
    mut commands: Commands,
) {
    let dueling = matches!(play.phase, Phase::PrePitch | Phase::WindUp | Phase::Pitch);
    let past_pre_pitch = matches!(play.phase, Phase::WindUp | Phase::Pitch);
    for (entity, playing) in &batters {
        let resolved = identities
            .get(entity)
            .map(|id| rosters.team(id.team).card(id.index).appearance.style.stance)
            .map(animation::stance_clip)
            .unwrap_or(AnimClip::BattingStance);
        match playing {
            None if dueling => {
                commands.entity(entity).insert(Playing::new(resolved));
            }
            // Identity changed mid-duel (a new at-bat that never passed
            // through a non-dueling frame): swap the stale stance for the
            // new batter's own.
            Some(playing)
                if dueling && animation::is_stance(playing.clip) && playing.clip != resolved =>
            {
                commands.entity(entity).insert(Playing::new(resolved));
            }
            Some(playing) if !dueling && animation::is_stance(playing.clip) => {
                commands.entity(entity).remove::<Playing>();
            }
            // Continuation cut: a fidget still playing once the windup
            // starts settles straight into the stance it was chained back
            // to anyway.
            Some(playing) if past_pre_pitch && animation::is_fidget(playing.clip) => {
                commands.entity(entity).insert(Playing::new(resolved));
            }
            // Continuation cut, dead-ball side: a fidget surviving into a
            // non-dueling frame (e.g. the ball going live mid-fidget) is
            // just dropped, same as a held stance would be.
            Some(playing) if !dueling && animation::is_fidget(playing.clip) => {
                commands.entity(entity).remove::<Playing>();
            }
            _ => {}
        }
    }
}

/// [`batter_fidgets`]'s dead-ball accumulator. A `Resource` rather than the
/// system's own `Local`s specifically so [`reset_batter_fidget_timer`] can
/// clear it from the `game_start()` transition schedule: a `Local` would
/// otherwise survive untouched across a same-process replay (`GameOver` ->
/// `MainMenu` -> `Playing` again without restarting the app), and the
/// leadoff batter's `PlayerIdentity` at kickoff is often *identical* between
/// two games with the same rosters (`BattingOrder::default()` always starts
/// both teams at slot 1) — so the system's own "batter changed" reset
/// wouldn't catch it either, letting time banked in a previous, finished
/// game silently carry into the next one's first at-bat.
#[derive(Resource, Default)]
pub(super) struct BatterFidgetTimer {
    since_stance: f32,
    current_batter: Option<PlayerIdentity>,
}

/// Fresh accumulator whenever a game (re)starts — see [`BatterFidgetTimer`]'s
/// doc comment for why a `game_start()` clear is needed at all.
pub(super) fn reset_batter_fidget_timer(mut timer: ResMut<BatterFidgetTimer>) {
    *timer = BatterFidgetTimer::default();
}

/// Between pitches a batter with an authored fidget occasionally breaks his
/// stance hold — helmet tap, practice half-swing — then settles back into it
/// (`Playing::then`). Dead-ball only: accumulates on qualifying frames —
/// `Phase::PrePitch`, never during the steal window (the duel there is
/// gameplay-legible timing), and only while he's actually holding a stance —
/// but a non-qualifying frame (windup started, steal window opened, no
/// stance yet) simply pauses accumulation instead of zeroing it. Against the
/// CPU pitcher a single `PrePitch` stretch only lasts ~0.7-1.2 s (`ai.rs`),
/// well under the 4-9 s interval, so the accumulator must survive across
/// stretches within the same at-bat or fidgets would never fire — it resets
/// only on the three events that actually invalidate the count: the batter
/// at the plate changes (tracked via [`BatterFidgetTimer::current_batter`],
/// compared each frame), a fidget actually fires, or fidgets are disabled.
/// `batter_stance`'s continuation-cut arm still owns getting any in-flight
/// fidget back out before the windup, so this system never has to worry
/// about one surviving past `PrePitch`. Cadence is deterministic hash noise
/// (`ai::hash01`, the ai.rs convention): the seed mixes the inning, the
/// batter's 1-based lineup slot, his roster index, and the current out
/// count (outs climb across an inning even when the same slot bats twice,
/// so consecutive at-bats still draw different intervals) — balls/strikes
/// are deliberately left out since they change mid-at-bat and would make
/// the target interval drift under the still-accumulating counter. The
/// drawn interval — 4..9 s per at-bat — varies between at-bats but never
/// between runs (replays and tests stay reproducible). `FidgetsDisabled`
/// (the scripted e2e harness default) suppresses it outright. Registered
/// `.after(IdentitySet)` — it reads `PlayerIdentity`.
#[allow(clippy::too_many_arguments)]
pub(super) fn batter_fidgets(
    play: Res<Play>,
    score: Res<ScoreBoard>,
    order: Res<BattingOrder>,
    rosters: Res<Rosters>,
    time: Res<Time>,
    disabled: Option<Res<animation::FidgetsDisabled>>,
    mut timer: ResMut<BatterFidgetTimer>,
    batters: Query<(Entity, &PlayerIdentity, Option<&Playing>), With<Batter>>,
    mut commands: Commands,
) {
    if disabled.is_some() {
        timer.since_stance = 0.0;
        timer.current_batter = None;
        return;
    }
    let Ok((entity, id, playing)) = batters.get_single() else {
        return;
    };
    if timer.current_batter != Some(*id) {
        // New batter at the plate (substitution or the next at-bat): any
        // accumulated dead-ball time belonged to someone else.
        timer.current_batter = Some(*id);
        timer.since_stance = 0.0;
    }
    let qualifies = play.phase == Phase::PrePitch
        && !play.in_steal_window()
        && playing.is_some_and(|playing| animation::is_stance(playing.clip));
    if !qualifies {
        // Pause, don't reset — the next qualifying frame (which may be in a
        // later PrePitch stretch of the same at-bat) picks up where this
        // left off.
        return;
    }
    let card = rosters.team(id.team).card(id.index);
    let Some(fidget) = card.appearance.style.fidget else {
        return;
    };
    timer.since_stance += time.delta_secs();
    // Deterministic per-at-bat interval in [4, 9): hash the inning, the
    // batter's lineup slot, his roster index, and the out count.
    let seed = score.inning as f32 * 31.0
        + order.current(id.team) as f32 * 7.0
        + id.index as f32 * 13.0
        + score.outs as f32 * 17.0;
    let interval = 4.0 + 5.0 * crate::game::ai::hash01(seed);
    if timer.since_stance >= interval {
        timer.since_stance = 0.0;
        commands.entity(entity).insert(Playing::then(
            animation::fidget_clip(fidget),
            animation::stance_clip(card.appearance.style.stance),
        ));
    }
}

/// Holds the catcher (and the plate umpire peering over him) in the
/// receiving crouch through the duel, and releases the stance the moment the
/// ball is in play so coverage can take over.
#[allow(clippy::type_complexity)]
pub(super) fn catcher_crouch(
    play: Res<Play>,
    catchers: Query<(Entity, Option<&Playing>), Or<(With<CatcherRole>, With<PlateUmpire>)>>,
    mut commands: Commands,
) {
    let dueling = matches!(play.phase, Phase::PrePitch | Phase::WindUp | Phase::Pitch);
    for (entity, playing) in &catchers {
        match playing {
            None if dueling => {
                commands
                    .entity(entity)
                    .insert(Playing::new(AnimClip::CatcherCrouch));
            }
            Some(playing) if !dueling && playing.clip == AnimClip::CatcherCrouch => {
                commands.entity(entity).remove::<Playing>();
            }
            _ => {}
        }
    }
}

/// Starts a swing when the batting side presses action during the duel —
/// humans and the CPU share the same `Intents`, so both animate. The bat
/// pivot sweeps and the batter's arms drive through with it, so the swing
/// reads on the whole body, not just the bat.
#[allow(clippy::type_complexity)]
pub(super) fn trigger_swing(
    intents: Res<Intents>,
    score: Res<ScoreBoard>,
    play: Res<Play>,
    pivots: Query<(Entity, Option<&Playing>), With<BatPivot>>,
    batters: Query<(Entity, Option<&Playing>), (With<Batter>, Without<BatPivot>)>,
    mut commands: Commands,
) {
    if !matches!(play.phase, Phase::PrePitch | Phase::WindUp | Phase::Pitch) {
        return;
    }
    if !intents.get(score.batting_team()).action {
        return;
    }
    for (entity, playing) in &pivots {
        if playing.is_none() {
            commands
                .entity(entity)
                .insert(Playing::then(AnimClip::SwingBat, AnimClip::RecoverSwing));
        }
    }
    for (entity, playing) in &batters {
        // A stance loop (shared or personal) always plays through the duel
        // now, so "nothing playing" is no longer the only swingable state:
        // replacing any held stance is fine (the driver's current-mismatch
        // restart cross-fades it away), but a `BatterSwing` already in
        // flight must never be interrupted by a second press.
        let swingable = match playing.map(|p| p.clip) {
            None => true,
            Some(c) => animation::is_stance(c),
        };
        if swingable {
            commands
                .entity(entity)
                .insert(Playing::new(AnimClip::BatterSwing));
        }
    }
}

/// A homer with an authored celebration chains it after the swing
/// follow-through (`Playing.next`), so the flip rides the same rig the
/// camera is holding on. The trot rig takes over at `RunDelay` expiry
/// (`TROT_DELAY` 0.9 s) — the flip's 0.85 s mostly fits; the handoff
/// truncating the last frames on slow swings is an accepted arcade trade
/// (the HR orbit camera is already moving by then). The `next.is_none()`
/// guard keeps re-entrancy safe if the event ever double-fires, and never
/// touches a batter without `Playing::clip == BatterSwing` — no in-flight
/// swing, no flip.
pub(super) fn celebrate_home_run(
    mut events: EventReader<BallInPlayEvent>,
    rosters: Res<Rosters>,
    mut batters: Query<(&PlayerIdentity, &mut Playing), With<Batter>>,
) {
    for ev in events.read() {
        if !matches!(ev.kind, ContactKind::HomeRun) {
            continue;
        }
        for (id, mut playing) in &mut batters {
            let card = rosters.team(id.team).card(id.index);
            if let Some(clip) = animation::celebration_clip(card.appearance.style.celebration) {
                if playing.clip == AnimClip::BatterSwing && playing.next.is_none() {
                    playing.next = Some(clip);
                }
            }
        }
    }
}
