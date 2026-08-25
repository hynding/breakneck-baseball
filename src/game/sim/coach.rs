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
use crate::game::flow::{BallInPlayEvent, LiveBallEvent, Phase, Play, PlayBanner};
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
    /// Announced dead-ball plays where the mitt is legitimately empty. The
    /// dropped third is recognized by its banner text (flow keeps the
    /// decision internal); HBP is re-derived from the crossing via the same
    /// `rules::hits_batter` flow consults.
    dropped_third: bool,
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
            dropped_third: false,
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
    banners: EventReader<'w, 's, PlayBanner>,
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

/// The one observer system: per-frame fact tracking plus the ~30 Hz sample.
#[allow(clippy::too_many_arguments)]
fn observe(
    time: Res<Time>,
    mut state: ResMut<CoachState>,
    config: Res<CoachConfig>,
    mut report: ResMut<CoachReport>,
    facts: WorldFacts,
    mut reports: PlayReports,
    rigs: WorldRigs,
    mut findings: EventWriter<CoachFindingEvent>,
) {
    let WorldFacts {
        play,
        score,
        bases,
        ruleset,
        field,
        active,
        settled,
    } = facts;
    let (in_play_ev, live_ev, banners) = (
        &mut reports.in_play,
        &mut reports.live,
        &mut reports.banners,
    );
    let (ball_q, catcher_q, fielder_q, runner_q) =
        (&rigs.ball, &rigs.catcher, &rigs.fielders, &rigs.runners);
    let now = time.elapsed_secs();

    // ── Per-frame fact tracking (cheap; events and edges the sampler would
    // miss between its 33 ms samples) ────────────────────────────────────────
    for ev in in_play_ev.read() {
        state.contacted = true;
        if matches!(ev.kind, ContactKind::Live { fair: true }) {
            state.contact = Some(ContactFacts {
                at: now,
                class: ev.contact_class,
                outs_at_contact: score.outs,
                bases_at_contact: (0..bases.count()).map(|b| bases.is_occupied(b)).collect(),
                steal_armed: play.runners_going(),
            });
        }
    }
    for ev in live_ev.read() {
        if matches!(ev, LiveBallEvent::Landed { .. }) {
            state.bounced = true;
        }
    }
    for banner in banners.read() {
        // Flow keeps the dropped-third decision internal; its banner is the
        // announcement, and the one play whose untouched pitch legitimately
        // never reaches the mitt (strike three in the dirt).
        if banner.text.starts_with("DROPPED 3RD") {
            state.dropped_third = true;
        }
    }
    let ball = ball_q.get_single().ok();
    if play.phase == Phase::Pitch {
        if let Some((tf, _, _)) = ball {
            // Mirror flow's plate-crossing record (same z-gate).
            if state.crossing.is_none() && tf.translation.z <= 0.1 {
                state.crossing = Some(Vec2::new(tf.translation.x, tf.translation.y));
            }
        }
    }
    if let (Some((tf, vel, in_flight)), Ok(catcher_tf)) = (ball, catcher_q.get_single()) {
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
    if play.phase == Phase::Result && state.last_phase != Phase::Result {
        if let Some((tf, _, Some(_))) = ball {
            state.result_y = Some(tf.translation.y);
        }
    }
    // A fresh at-bat: clear the per-play facts. (The pure Coach resets its
    // own play memory on the same PrePitch edge.)
    if play.phase == Phase::PrePitch && state.last_phase != Phase::PrePitch {
        state.contact = None;
        state.contacted = false;
        state.bounced = false;
        state.crossing = None;
        state.glove_y = None;
        state.result_y = None;
        state.dropped_third = false;
    }
    state.last_phase = play.phase;

    // ── The ~30 Hz sample ────────────────────────────────────────────────────
    if !state.sample.tick(time.delta()).just_finished() {
        return;
    }

    let phase = match play.phase {
        Phase::PrePitch => CoachPhase::PrePitch,
        Phase::WindUp => CoachPhase::WindUp,
        Phase::Pitch => CoachPhase::Pitch,
        Phase::InPlay => CoachPhase::InPlay,
        Phase::Result => CoachPhase::Result,
    };

    let hbp = state.crossing.is_some_and(rules::hits_batter);
    let out_of_band = |y: f32| !(0.12..=2.4).contains(&y);
    let dirt = state.glove_y.is_some_and(out_of_band) || state.result_y.is_some_and(out_of_band);
    let untouched_pitch_result = phase == CoachPhase::Result
        && state.crossing.is_some()
        && !state.contacted
        && !state.dropped_third
        && !hbp
        && !dirt;

    // Map the chaser / cover entities to fielder indices.
    let index_of = |entity: Entity| {
        fielder_q
            .get(entity)
            .ok()
            .map(|(fielder, _, _)| fielder.index)
    };
    let chaser = active.chaser().and_then(index_of);
    let covers = active
        .covers()
        .iter()
        .filter_map(|&(base, entity)| Some((base, index_of(entity)?)))
        .collect();

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

    let snapshot = CoachSnapshot {
        time: now,
        phase,
        in_steal_window: play.in_steal_window(),
        pending_call: play.pending_call().is_some(),
        runners_settled: settled.0,
        untouched_pitch_result,
        result_secs: ruleset.pace.result_secs,
        auto_throw_delay_secs: ruleset.pace.auto_throw_delay_secs,
        catcher_pos: catcher_q.get_single().ok().map(|tf| tf.translation),
        ball: ball_facts,
        contact: state.contact.clone(),
        chaser,
        covers,
        holding_since: active.holding_since(),
        fielders: fielder_q
            .iter()
            .map(|(fielder, tf, intent)| FielderFacts {
                index: fielder.index,
                pos: tf.translation,
                move_target: intent.target,
            })
            .collect(),
        fielder_spots: field.fielder_positions.clone(),
        base_positions: field.base_positions.clone(),
        runners: runner_q
            .iter()
            .map(|(runner, tf, intent)| RunnerFacts {
                base: runner.base,
                pos: tf.translation,
                moving: intent.target.is_some(),
            })
            .collect(),
    };

    report.samples += 1;
    for finding in state.coach.observe(&snapshot) {
        if config.disabled.contains(&finding.check) {
            continue;
        }
        report.record(finding.clone());
        findings.send(CoachFindingEvent(finding));
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
