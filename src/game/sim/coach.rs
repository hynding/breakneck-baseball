//! Coach observer — samples the live world at a fixed cadence, fills the
//! pure [`crate::game::core::coach`] snapshot, runs the checks, and reports.
//!
//! **Pure observer.** This module never mutates `ScoreBoard`, `Bases`,
//! `Play`, rigs, physics, or any other gameplay state — it reads, samples,
//! and emits [`CoachFindingEvent`]s / accumulates a [`CoachReport`]. It is
//! fully inert unless the [`CoachEnabled`] resource is present (inserted by
//! default in `--features debug` builds and by the headless test harness;
//! absent in release).
//!
//! The cheap per-frame half of the system tracks play facts that would be
//! missed between samples (contact, the bounce, the plate crossing, the
//! glove-arrival height, banner-announced dead balls); the checks themselves
//! run at ~30 Hz of virtual time — every frame would buy nothing, since
//! every check carries a ≥150 ms tolerance.

use std::collections::{HashMap, VecDeque};

use bevy::prelude::*;
use bevy_rapier3d::prelude::Velocity;

use crate::game::animation::MoveIntent;
use crate::game::ball::{BALL_DRAG_FACTOR, Baseball, InFlight, MAGNUS_FACTOR};
use crate::game::coach::{
    BallFacts, CheckId, Coach, CoachFinding, CoachPhase, CoachSnapshot, ContactFacts, FielderFacts,
    RunnerFacts, Severity,
};
use crate::game::fielding::ActivePlay;
use crate::game::flow::{BallInPlayEvent, LiveBallEvent, Phase, Play};
use crate::game::player::{CatcherRole, Fielder};
use crate::game::rules::{self, Bases, ContactKind};
use crate::game::runner::{Runner, RunnersSettled};
use crate::game::variant::{FieldSpec, Ruleset};
use crate::game::{GameState, ScoreBoard};

/// Seconds between check runs (~30 Hz of virtual time).
const SAMPLE_SECS: f32 = 1.0 / 30.0;
/// Recent findings kept for the debug tab / reports.
const RECENT_CAP: usize = 64;

/// Marker: the Coach is watching. Absent in release builds by default —
/// insert it (debug tab, test harness, autoplay) to turn the observer on.
#[derive(Resource, Default)]
pub struct CoachEnabled;

/// Per-check switchboard, driven by the debug tab. A disabled check's
/// findings are dropped at emission; the sampling itself keeps running.
#[derive(Resource, Default)]
pub struct CoachConfig {
    pub disabled: Vec<CheckId>,
}

/// One finding, as a Bevy event, for anything that wants to react live
/// (the debug tab feed, the autoplay JSON logger).
#[derive(Event, Clone, Debug)]
pub struct CoachFindingEvent(pub CoachFinding);

/// Accumulated findings: counts by check/severity plus a ring buffer of the
/// most recent, for reports and the debug tab.
#[derive(Resource, Default)]
pub struct CoachReport {
    counts: HashMap<(CheckId, Severity), u32>,
    pub recent: VecDeque<CoachFinding>,
    pub samples: u64,
}

impl CoachReport {
    fn record(&mut self, finding: CoachFinding) {
        *self
            .counts
            .entry((finding.check, finding.severity))
            .or_insert(0) += 1;
        if self.recent.len() == RECENT_CAP {
            self.recent.pop_front();
        }
        self.recent.push_back(finding);
    }

    pub fn count(&self, check: CheckId, severity: Severity) -> u32 {
        self.counts.get(&(check, severity)).copied().unwrap_or(0)
    }

    /// Total findings of `severity` across every check.
    pub fn total(&self, severity: Severity) -> u32 {
        CheckId::ALL.iter().map(|&c| self.count(c, severity)).sum()
    }

    /// Every violation-grade finding currently in the ring buffer.
    pub fn violations(&self) -> impl Iterator<Item = &CoachFinding> {
        self.recent
            .iter()
            .filter(|f| f.severity == Severity::Violation)
    }
}

/// The observer's own memory: the pure check machine plus the per-play facts
/// the 30 Hz sampler would miss between samples.
#[derive(Resource)]
pub struct CoachState {
    coach: Coach,
    sample: Timer,
    contact: Option<ContactFacts>,
    /// Any ball was put in play this play (fair, foul, or home run).
    contacted: bool,
    /// The live ball has touched grass (LiveBallEvent::Landed seen).
    bounced: bool,
    /// Plate-crossing (x, y), mirrored from the same z-gate flow uses.
    crossing: Option<Vec2>,
    /// Ball height on first reaching the catcher's glove line — the exact
    /// gate `catcher_receives` uses for its dirt/sailed exemption.
    glove_y: Option<f32>,
    /// Ball height on the frame the take was judged (Result began with the
    /// pitch still in flight) — the exact input `catcher_receives` feeds its
    /// official dirt/sailed exemption, which runs a few metres *behind* the
    /// glove line (the widened `late_swing_z` window). Either observation
    /// point reading "dirt" exempts the mitt expectation.
    result_y: Option<f32>,
    last_phase: Phase,
}

impl CoachState {
    /// The current play's contact facts — read by the debug overlays to draw
    /// expected runner breaks and uncovered bags. Observer-read only.
    pub fn contact_facts(&self) -> Option<&ContactFacts> {
        self.contact.as_ref()
    }
}

impl Default for CoachState {
    fn default() -> Self {
        Self {
            coach: Coach::default(),
            sample: Timer::from_seconds(SAMPLE_SECS, TimerMode::Repeating),
            contact: None,
            contacted: false,
            bounced: false,
            crossing: None,
            glove_y: None,
            result_y: None,
            last_phase: Phase::PrePitch,
        }
    }
}

fn reset_coach(mut state: ResMut<CoachState>, mut report: ResMut<CoachReport>) {
    *state = CoachState::default();
    *report = CoachReport::default();
}

/// The gameplay state the observer reads (never writes) each sample.
#[derive(bevy::ecs::system::SystemParam)]
struct WorldFacts<'w> {
    play: Res<'w, Play>,
    score: Res<'w, ScoreBoard>,
    bases: Res<'w, Bases>,
    ruleset: Res<'w, Ruleset>,
    field: Res<'w, FieldSpec>,
    active: Res<'w, ActivePlay>,
    settled: Res<'w, RunnersSettled>,
}

/// The play-by-play reports the per-frame tracker consumes.
#[derive(bevy::ecs::system::SystemParam)]
struct PlayReports<'w, 's> {
    in_play: EventReader<'w, 's, BallInPlayEvent>,
    live: EventReader<'w, 's, LiveBallEvent>,
}

/// The rigs and the ball, as the sampler sees them.
#[derive(bevy::ecs::system::SystemParam)]
struct WorldRigs<'w, 's> {
    ball: Query<
        'w,
        's,
        (
            &'static Transform,
            &'static Velocity,
            Option<&'static InFlight>,
        ),
        With<Baseball>,
    >,
    catcher: Query<'w, 's, &'static Transform, (With<CatcherRole>, Without<Baseball>)>,
    fielders: Query<
        'w,
        's,
        (&'static Fielder, &'static Transform, &'static MoveIntent),
        Without<Baseball>,
    >,
    runners:
        Query<'w, 's, (&'static Runner, &'static Transform, &'static MoveIntent), Without<Fielder>>,
}

/// Per-frame fact tracking: the events and phase edges the ~30 Hz sampler
/// would miss between its 33 ms samples. Cheap, so it runs every frame.
fn track_frame_facts(
    state: &mut CoachState,
    facts: &WorldFacts,
    reports: &mut PlayReports,
    rigs: &WorldRigs,
    now: f32,
) {
    // ── Per-frame fact tracking (cheap; events and edges the sampler would
    // miss between its 33 ms samples) ────────────────────────────────────────
    for ev in reports.in_play.read() {
        state.contacted = true;
        if matches!(ev.kind, ContactKind::Live { fair: true }) {
            state.contact = Some(ContactFacts {
                at: now,
                class: ev.contact_class,
                outs_at_contact: facts.score.outs,
                bases_at_contact: (0..facts.bases.count())
                    .map(|b| facts.bases.is_occupied(b))
                    .collect(),
                steal_armed: facts.play.runners_going(),
            });
        }
    }
    for ev in reports.live.read() {
        if matches!(ev, LiveBallEvent::Landed { .. }) {
            state.bounced = true;
        }
    }
    let ball = rigs.ball.get_single().ok();
    if facts.play.phase == Phase::Pitch {
        if let Some((tf, _, _)) = ball {
            // Mirror flow's plate-crossing record (same z-gate).
            if state.crossing.is_none() && tf.translation.z <= 0.1 {
                state.crossing = Some(Vec2::new(tf.translation.x, tf.translation.y));
            }
        }
    }
    if let (Some((tf, vel, in_flight)), Ok(catcher_tf)) = (ball, rigs.catcher.get_single()) {
        // First arrival at the glove line, exactly as `catcher_receives`
        // gates its catch: this is what exempts dirt balls and sailed
        // pitches from the mitt expectation.
        if state.glove_y.is_none()
            && in_flight.is_some()
            && tf.translation.z <= catcher_tf.translation.z + 0.6
            && vel.linvel.z < 0.0
        {
            state.glove_y = Some(tf.translation.y);
        }
    }
    if facts.play.phase == Phase::Result && state.last_phase != Phase::Result {
        if let Some((tf, _, Some(_))) = ball {
            state.result_y = Some(tf.translation.y);
        }
    }
    // A fresh at-bat: clear the per-facts.play facts. (The pure Coach resets its
    // own facts.play memory on the same PrePitch edge.)
    if facts.play.phase == Phase::PrePitch && state.last_phase != Phase::PrePitch {
        state.contact = None;
        state.contacted = false;
        state.bounced = false;
        state.crossing = None;
        state.glove_y = None;
        state.result_y = None;
    }
    state.last_phase = facts.play.phase;
}

/// Assembles the snapshot the pure [`Coach`] grades. Read-only over the world:
/// everything it needs was either tracked above or is readable this frame.
fn build_snapshot(
    state: &CoachState,
    facts: &WorldFacts,
    rigs: &WorldRigs,
    now: f32,
) -> CoachSnapshot {
    let phase = match facts.play.phase {
        Phase::PrePitch => CoachPhase::PrePitch,
        Phase::WindUp => CoachPhase::WindUp,
        Phase::Pitch => CoachPhase::Pitch,
        Phase::InPlay => CoachPhase::InPlay,
        Phase::Result => CoachPhase::Result,
    };

    let hbp = state.crossing.is_some_and(rules::hits_batter);
    let out_of_band = |y: f32| !(0.12..=2.4).contains(&y);
    let dirt = state.glove_y.is_some_and(out_of_band) || state.result_y.is_some_and(out_of_band);
    // A dropped third's untouched pitch legitimately never reaches the mitt —
    // read straight off the umpire's decision (`Play::last_strike_call`,
    // cleared at the next PrePitch), not the banner announcement.
    let dropped_third = facts.play.last_strike_call() == Some(rules::StrikeCall::DroppedThird);
    let untouched_pitch_result = phase == CoachPhase::Result
        && state.crossing.is_some()
        && !state.contacted
        && !dropped_third
        && !hbp
        && !dirt;

    // Map the chaser / cover entities to fielder indices.
    let index_of = |entity: Entity| {
        rigs.fielders
            .get(entity)
            .ok()
            .map(|(fielder, _, _)| fielder.index)
    };
    let chaser = facts.active.chaser().and_then(index_of);
    let covers = facts
        .active
        .covers()
        .iter()
        .filter_map(|&(base, entity)| Some((base, index_of(entity)?)))
        .collect();

    let ball = rigs.ball.get_single().ok();
    let ball_facts = ball.map(|(tf, vel, in_flight)| BallFacts {
        pos: tf.translation,
        vel: vel.linvel,
        predicted_landing: (state.contact.is_some() && !state.bounced && in_flight.is_some()).then(
            || {
                rules::predict_landing_from(
                    tf.translation,
                    vel.linvel,
                    vel.angvel,
                    BALL_DRAG_FACTOR,
                    MAGNUS_FACTOR,
                )
                .0
            },
        ),
    });

    CoachSnapshot {
        time: now,
        phase,
        in_steal_window: facts.play.in_steal_window(),
        pending_call: facts.play.pending_call().is_some(),
        runners_settled: facts.settled.0,
        untouched_pitch_result,
        result_secs: facts.ruleset.pace.result_secs,
        auto_throw_delay_secs: facts.ruleset.pace.auto_throw_delay_secs,
        catcher_pos: rigs.catcher.get_single().ok().map(|tf| tf.translation),
        ball: ball_facts,
        contact: state.contact.clone(),
        chaser,
        covers,
        holding_since: facts.active.holding_since(),
        fielders: rigs
            .fielders
            .iter()
            .map(|(fielder, tf, intent)| FielderFacts {
                index: fielder.index,
                pos: tf.translation,
                move_target: intent.target,
            })
            .collect(),
        fielder_spots: facts.field.fielder_positions.clone(),
        base_positions: facts.field.base_positions.clone(),
        runners: rigs
            .runners
            .iter()
            .map(|(runner, tf, intent)| RunnerFacts {
                base: runner.base,
                pos: tf.translation,
                moving: intent.target.is_some(),
            })
            .collect(),
    }
}

/// Where a finding goes: filtered by config, tallied in the report, and
/// broadcast. Bundled like the input side above so `observe` stays inside
/// the argument limit without an `allow`.
#[derive(bevy::ecs::system::SystemParam)]
struct CoachOutput<'w> {
    config: Res<'w, CoachConfig>,
    report: ResMut<'w, CoachReport>,
    findings: EventWriter<'w, CoachFindingEvent>,
}

/// The one observer system: track this frame's facts, then — at the sample
/// rate — build a snapshot and hand it to the pure Coach.
fn observe(
    time: Res<Time>,
    mut state: ResMut<CoachState>,
    facts: WorldFacts,
    mut reports: PlayReports,
    rigs: WorldRigs,
    mut out: CoachOutput,
) {
    let now = time.elapsed_secs();
    track_frame_facts(&mut state, &facts, &mut reports, &rigs, now);

    if !state.sample.tick(time.delta()).just_finished() {
        return;
    }

    let snapshot = build_snapshot(&state, &facts, &rigs, now);
    out.report.samples += 1;
    for finding in state.coach.observe(&snapshot) {
        if out.config.disabled.contains(&finding.check) {
            continue;
        }
        out.report.record(finding.clone());
        out.findings.send(CoachFindingEvent(finding));
    }
}

pub struct CoachPlugin;

impl Plugin for CoachPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CoachConfig>()
            .init_resource::<CoachReport>()
            .init_resource::<CoachState>()
            .add_event::<CoachFindingEvent>()
            .add_systems(crate::game::game_start(), reset_coach)
            .add_systems(
                Update,
                observe
                    .after(crate::game::flow::PhaseSet)
                    .run_if(in_state(GameState::Playing).and(resource_exists::<CoachEnabled>)),
            );
        // Default-on where a watcher exists: debug builds (the F1 tab shows
        // the feed) — headless tests insert it from the harness; release
        // builds leave the observer fully inert.
        #[cfg(feature = "debug")]
        app.init_resource::<CoachEnabled>();
    }
}
