//! The Coach — pure expectation checks over a sampled world snapshot.
//!
//! The Coach knows what every player *should* be doing at every instant and
//! reports when reality drifts from that. It derives its expectations from
//! the same single sources of truth flow uses ([`rules::runner_break`],
//! [`rules::is_forced`], the steal-window state, the fielding assignment) —
//! never a second copy of baseball logic — and checks the sampled world
//! against them with an explicit per-check time tolerance, so genuinely
//! missed behavior is flagged, not animation latency.
//!
//! Same discipline as `core/rules`: no ECS, no RNG, plain data in →
//! [`CoachFinding`]s out. The Coach holds small per-check watchdogs
//! (deadline timestamps) because "X must happen within 400 ms" and the
//! late-vs-never distinction cannot be judged from a single instant — but it
//! is still fully deterministic: the same snapshot sequence always yields
//! the same findings. The sim-side observer (`sim/coach.rs`) fills the
//! snapshot and never mutates gameplay; the Coach is a pure observer.

use bevy::math::{Vec2, Vec3};

use crate::game::rules::{self, ContactClass, RunnerBreak};

// ── Phases (coach-local mirror) ───────────────────────────────────────────────

/// Mirror of `flow::Phase`, defined here so `core` never imports `sim`
/// (the sim observer maps the real phase across, field for field).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoachPhase {
    PrePitch,
    WindUp,
    Pitch,
    InPlay,
    Result,
}

// ── Findings ──────────────────────────────────────────────────────────────────

/// Which expectation a finding came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CheckId {
    /// Runners aboard break off contact per [`rules::runner_break`].
    RunnerBreaks,
    /// Exactly one assigned chaser whose intercept tracks the live ball.
    ChaserConvergence,
    /// Every force-relevant bag has a coverer (or the chaser) on a live ball.
    BaseCoverage,
    /// An untouched, catchable pitch ends at rest in the catcher's mitt.
    CatcherReceives,
    /// A held gathered ball is thrown by the auto-throw deadline; a decided
    /// pending call is announced inside its settle cap.
    ThrowDiscipline,
    /// No pitch is delivered while the steal window still gates it.
    StealWindow,
    /// The result pause settles every runner rig inside its cap.
    Settlement,
    /// Between plays, fielders return to (and hold) their spots.
    IdleDiscipline,
}

impl CheckId {
    pub const ALL: [CheckId; 8] = [
        CheckId::RunnerBreaks,
        CheckId::ChaserConvergence,
        CheckId::BaseCoverage,
        CheckId::CatcherReceives,
        CheckId::ThrowDiscipline,
        CheckId::StealWindow,
        CheckId::Settlement,
        CheckId::IdleDiscipline,
    ];

    /// Stable short label for reports.
    pub fn label(self) -> &'static str {
        match self {
            CheckId::RunnerBreaks => "runner-breaks",
            CheckId::ChaserConvergence => "chaser-convergence",
            CheckId::BaseCoverage => "base-coverage",
            CheckId::CatcherReceives => "catcher-receives",
            CheckId::ThrowDiscipline => "throw-discipline",
            CheckId::StealWindow => "steal-window",
            CheckId::Settlement => "settlement",
            CheckId::IdleDiscipline => "idle-discipline",
        }
    }
}

/// How badly reality drifted from the expectation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Legal but ugly (e.g. an intercept line lagging the ball).
    Style,
    /// The expected behavior happened, but outside its tolerance.
    Late,
    /// The rules say X must be happening and it is not, beyond tolerance.
    Violation,
}

/// One observed drift between what the rules expect and what the world shows.
#[derive(Clone, Debug)]
pub struct CoachFinding {
    pub check: CheckId,
    pub severity: Severity,
    /// Virtual game time (seconds) at which the finding fired.
    pub game_time: f32,
    /// Who drifted ("runner on 1B", "fielder #3", "catcher").
    pub subject: String,
    pub expected: String,
    pub observed: String,
}

// ── Tolerances ────────────────────────────────────────────────────────────────

/// Every check's explicit grace, in one tunable struct. The time values are
/// the "human reaction" grace between a rule firing and the rig visibly
/// obeying it; distances separate "on his way" from "parked wrong".
#[derive(Clone, Debug)]
pub struct CoachTolerances {
    /// A runner ordered to break must be moving this soon after contact.
    pub runner_break_secs: f32,
    /// A breaking runner still within this of his bag counts as "never left".
    pub break_stall_dist_m: f32,
    /// A tag-up runner farther than this off his bag while the fly is still
    /// airborne has left early (extended leads reach ~4.5 m).
    pub tagup_leave_dist_m: f32,
    /// A live ball must have an assigned chaser this soon after contact.
    pub chaser_assign_secs: f32,
    /// The chaser's intercept target may lag the live prediction by this much.
    pub intercept_slack_m: f32,
    /// ...for at most this long before it counts as "not tracking".
    pub intercept_grace_secs: f32,
    /// Cover assignments must exist this soon after contact.
    pub coverage_secs: f32,
    /// An untouched pitch must be at rest in the mitt this soon after the
    /// take is judged.
    pub catcher_secs: f32,
    /// "In the mitt" = at rest within this of the catcher.
    pub catcher_dist_m: f32,
    /// Grace past the auto-throw deadline before a held ball is flagged.
    pub throw_hold_grace_secs: f32,
    /// A decided call must be announced inside this (flow's 4 s settle cap
    /// plus grace).
    pub pending_announce_cap_secs: f32,
    /// Runners must settle inside the result pause plus this (the home-run
    /// trot legitimately takes ~15 s; flow's hard cap is 20 s).
    pub settle_grace_secs: f32,
    /// The Result phase itself must end inside this (flow's 20 s cap + grace).
    pub result_stuck_cap_secs: f32,
    /// Grace past the computed jog-back arrival before an off-spot fielder
    /// is flagged.
    pub idle_grace_secs: f32,
    /// "On his spot" = within this of the FieldSpec position.
    pub idle_dist_m: f32,
    /// Jog-back speed used to compute the idle arrival deadline (mirrors
    /// fielding's return speed).
    pub return_speed_mps: f32,
    /// A ball slower than this counts as at rest.
    pub rest_speed_mps: f32,
}

impl Default for CoachTolerances {
    fn default() -> Self {
        Self {
            runner_break_secs: 0.4,
            break_stall_dist_m: 5.0,
            tagup_leave_dist_m: 6.0,
            chaser_assign_secs: 0.3,
            intercept_slack_m: 3.0,
            intercept_grace_secs: 0.4,
            coverage_secs: 0.4,
            catcher_secs: 0.4,
            catcher_dist_m: 1.5,
            throw_hold_grace_secs: 0.3,
            pending_announce_cap_secs: 4.5,
            settle_grace_secs: 16.0,
            result_stuck_cap_secs: 21.0,
            idle_grace_secs: 0.4,
            idle_dist_m: 0.6,
            return_speed_mps: 4.0,
            rest_speed_mps: 0.5,
        }
    }
}

// ── Snapshot ──────────────────────────────────────────────────────────────────

/// One fielder as sampled (the pitcher is not a fielder; the catcher is).
#[derive(Clone, Debug)]
pub struct FielderFacts {
    /// Index into `fielder_spots` (the FieldSpec position this rig owns).
    pub index: usize,
    pub pos: Vec3,
    /// Current movement order, if any.
    pub move_target: Option<Vec3>,
}

/// One runner aboard as sampled. `base` is the origin bag while breaking
/// (the sim leaves it untouched until resolution re-paths).
#[derive(Clone, Debug)]
pub struct RunnerFacts {
    pub base: usize,
    pub pos: Vec3,
    /// Holds a live movement order or base path.
    pub moving: bool,
}

/// The live ball as sampled.
#[derive(Clone, Debug)]
pub struct BallFacts {
    pub pos: Vec3,
    pub vel: Vec3,
    /// Predicted landing of a live airborne batted ball (the sim fills it
    /// via `rules::predict_landing_from`); `None` once it has bounced —
    /// from there the chase target is the ball itself.
    pub predicted_landing: Option<Vec3>,
}

/// Facts frozen at contact for the current live play.
#[derive(Clone, Debug)]
pub struct ContactFacts {
    /// Game time of contact.
    pub at: f32,
    pub class: ContactClass,
    pub outs_at_contact: u32,
    /// Base occupancy at contact (runner-break expectations key off this).
    pub bases_at_contact: Vec<bool>,
    /// The runners broke with the windup — every break expectation is moot.
    pub steal_armed: bool,
}

/// The minimal world facts the checks need, sampled by the sim observer.
#[derive(Clone, Debug)]
pub struct CoachSnapshot {
    /// Virtual game time, seconds.
    pub time: f32,
    pub phase: CoachPhase,
    pub in_steal_window: bool,
    /// A decided call is waiting for its announcement (throw in the air).
    pub pending_call: bool,
    pub runners_settled: bool,
    /// This play ended with an untouched, catchable pitch (no contact, no
    /// HBP, not in the dirt) — the catcher must end up with the ball.
    pub untouched_pitch_result: bool,
    /// The active result-pause length (pace.result_secs).
    pub result_secs: f32,
    /// The active auto-throw deadline (pace.auto_throw_delay_secs).
    pub auto_throw_delay_secs: f32,
    pub catcher_pos: Option<Vec3>,
    pub ball: Option<BallFacts>,
    /// Present from fair live contact until the play resolves.
    pub contact: Option<ContactFacts>,
    /// Index into `fielders` of the assigned chaser, while one is chasing.
    pub chaser: Option<usize>,
    /// The fielding assignment: (base index, fielder index); base ==
    /// `base_positions.len()` means home plate.
    pub covers: Vec<(usize, usize)>,
    /// Game time a fielder gathered the ball and began holding it.
    pub holding_since: Option<f32>,
    pub fielders: Vec<FielderFacts>,
    /// FieldSpec fielder spots (same indexing as `FielderFacts::index`).
    pub fielder_spots: Vec<Vec3>,
    pub base_positions: Vec<Vec3>,
    pub runners: Vec<RunnerFacts>,
}

fn flat_dist(a: Vec3, b: Vec3) -> f32 {
    Vec2::new(a.x - b.x, a.z - b.z).length()
}

fn bases_from(occupancy: &[bool]) -> rules::Bases {
    let mut b = rules::Bases::new(occupancy.len());
    for (i, &occ) in occupancy.iter().enumerate() {
        b.set(i, occ);
    }
    b
}

// ── Watchdog state ────────────────────────────────────────────────────────────

/// Deadline tracking for one "must happen by T" expectation. When the
/// deadline passes without the behavior, the subject turns overdue; behavior
/// arriving later downgrades to a [`Severity::Late`] finding, and a play
/// ending with it still absent fires the [`Severity::Violation`].
#[derive(Clone, Copy, Debug, PartialEq)]
enum Watch {
    Waiting {
        deadline: f32,
    },
    Overdue {
        since: f32,
    },
    /// Behavior confirmed (or finding already fired) — nothing left to watch.
    Done,
}

/// Per-play memory, reset when a fresh at-bat begins.
#[derive(Default)]
struct PlayMemory {
    /// Runner-break watchdogs keyed by origin base, with the expected break.
    runner_watch: Vec<(usize, RunnerBreak, Watch)>,
    tagup_flagged: Vec<usize>,
    chaser_watch: Option<Watch>,
    intercept_bad_since: Option<f32>,
    intercept_flagged: bool,
    coverage_checked: bool,
    catcher_watch: Option<Watch>,
    hold_flagged: bool,
    pending_since: Option<f32>,
    pending_flagged: bool,
    steal_flagged: bool,
    result_since: Option<f32>,
    settle_flagged: bool,
    stuck_flagged: bool,
    /// Per-fielder jog-back deadlines for the current PrePitch, plus the
    /// set already flagged.
    idle_deadlines: Vec<(usize, f32)>,
    idle_flagged: Vec<usize>,
}

/// The Coach: feed it snapshots in time order, collect findings.
pub struct Coach {
    pub tol: CoachTolerances,
    last_phase: Option<CoachPhase>,
    play: PlayMemory,
}

impl Default for Coach {
    fn default() -> Self {
        Self::new(CoachTolerances::default())
    }
}

impl Coach {
    pub fn new(tol: CoachTolerances) -> Self {
        Self {
            tol,
            last_phase: None,
            play: PlayMemory::default(),
        }
    }

    /// Observe one sampled instant. Returns every finding that fired at it.
    pub fn observe(&mut self, s: &CoachSnapshot) -> Vec<CoachFinding> {
        let mut findings = Vec::new();
        let entered = self.last_phase != Some(s.phase);

        // Leaving InPlay finalizes the live-play watchdogs: anything still
        // overdue never happened.
        if entered && self.last_phase == Some(CoachPhase::InPlay) {
            self.finalize_live_play(s, &mut findings);
        }

        if entered {
            match s.phase {
                CoachPhase::PrePitch => {
                    // Fresh at-bat: reset play-scoped memory, then arm the
                    // idle jog-back deadlines from where everyone stands now.
                    self.play = PlayMemory::default();
                    self.arm_idle_deadlines(s);
                }
                CoachPhase::Result => self.play.result_since = Some(s.time),
                _ => {}
            }
        }
        self.last_phase = Some(s.phase);

        self.check_steal_window(s, &mut findings);
        if s.phase == CoachPhase::InPlay {
            self.check_runner_breaks(s, &mut findings);
            self.check_chaser(s, &mut findings);
            self.check_coverage(s, &mut findings);
            self.check_throw_discipline(s, &mut findings);
        }
        if s.phase == CoachPhase::Result {
            self.check_catcher_receives(s, &mut findings);
            self.check_settlement(s, &mut findings);
        }
        if s.phase == CoachPhase::PrePitch {
            self.check_idle(s, &mut findings);
        }
        findings
    }

    // ── Steal window ─────────────────────────────────────────────────────────

    /// The pre-pitch window gates the pitch: while it is still running the
    /// ball must not be on its way (no WindUp, no live Pitch).
    fn check_steal_window(&mut self, s: &CoachSnapshot, out: &mut Vec<CoachFinding>) {
        if self.play.steal_flagged {
            return;
        }
        if s.in_steal_window && matches!(s.phase, CoachPhase::WindUp | CoachPhase::Pitch) {
            self.play.steal_flagged = true;
            out.push(CoachFinding {
                check: CheckId::StealWindow,
                severity: Severity::Violation,
                game_time: s.time,
                subject: "pitcher".into(),
                expected: "no delivery while the steal window gates the pitch".into(),
                observed: format!("phase {:?} with the window still open", s.phase),
            });
        }
    }

    // ── Runner breaks ────────────────────────────────────────────────────────

    fn check_runner_breaks(&mut self, s: &CoachSnapshot, out: &mut Vec<CoachFinding>) {
        let Some(contact) = &s.contact else {
            return;
        };
        // Runners already going with the windup broke legitimately before
        // contact; no break expectation applies.
        if contact.steal_armed {
            return;
        }
        // Once the call is decided, resolution re-paths everyone.
        if s.pending_call {
            return;
        }
        // Arm one watchdog per runner aboard at contact, from the same rule
        // flow's choreography reads.
        if self.play.runner_watch.is_empty() {
            let bases = bases_from(&contact.bases_at_contact);
            for (base, &occ) in contact.bases_at_contact.iter().enumerate() {
                if !occ {
                    continue;
                }
                let expected = rules::runner_break(
                    contact.outs_at_contact,
                    rules::is_forced(&bases, base),
                    contact.class,
                );
                self.play.runner_watch.push((
                    base,
                    expected,
                    Watch::Waiting {
                        deadline: contact.at + self.tol.runner_break_secs,
                    },
                ));
            }
            if self.play.runner_watch.is_empty() {
                // Nobody aboard: mark checked so we don't re-derive.
                self.play
                    .runner_watch
                    .push((usize::MAX, RunnerBreak::TagUp, Watch::Done));
            }
        }

        for (base, expected, watch) in &mut self.play.runner_watch {
            let Some(runner) = s.runners_on(*base) else {
                continue;
            };
            let bag = s.base_positions.get(*base).copied().unwrap_or(Vec3::ZERO);
            let off_bag = flat_dist(runner.pos, bag);
            match expected {
                RunnerBreak::GoNow | RunnerBreak::Halfway => {
                    let broke = runner.moving || off_bag > self.tol.break_stall_dist_m;
                    match *watch {
                        Watch::Waiting { deadline } => {
                            if broke {
                                *watch = Watch::Done;
                            } else if s.time > deadline {
                                *watch = Watch::Overdue { since: deadline };
                            }
                        }
                        Watch::Overdue { since } => {
                            if broke {
                                *watch = Watch::Done;
                                out.push(CoachFinding {
                                    check: CheckId::RunnerBreaks,
                                    severity: Severity::Late,
                                    game_time: s.time,
                                    subject: format!("runner on base {base}"),
                                    expected: format!(
                                        "{expected:?} within {:.2}s of contact",
                                        self.tol.runner_break_secs
                                    ),
                                    observed: format!(
                                        "broke {:.2}s past the tolerance",
                                        s.time - since
                                    ),
                                });
                            }
                        }
                        Watch::Done => {}
                    }
                }
                RunnerBreak::TagUp => {
                    // No runner leaves early on a tag-up while the fly is
                    // still airborne.
                    let airborne = s
                        .ball
                        .as_ref()
                        .is_some_and(|b| b.predicted_landing.is_some());
                    if airborne
                        && off_bag > self.tol.tagup_leave_dist_m
                        && !self.play.tagup_flagged.contains(base)
                    {
                        self.play.tagup_flagged.push(*base);
                        out.push(CoachFinding {
                            check: CheckId::RunnerBreaks,
                            severity: Severity::Violation,
                            game_time: s.time,
                            subject: format!("runner on base {base}"),
                            expected: "hold the bag to tag up on a deep fly".into(),
                            observed: format!("{off_bag:.1} m off the bag with the ball up"),
                        });
                    }
                }
            }
        }
    }

    // ── Chaser convergence ───────────────────────────────────────────────────

    fn check_chaser(&mut self, s: &CoachSnapshot, out: &mut Vec<CoachFinding>) {
        let Some(contact) = &s.contact else {
            return;
        };
        // A decided or held ball has no chase to run.
        if s.pending_call || s.holding_since.is_some() {
            self.play.chaser_watch.get_or_insert(Watch::Done);
            return;
        }
        let watch = self.play.chaser_watch.get_or_insert(Watch::Waiting {
            deadline: contact.at + self.tol.chaser_assign_secs,
        });
        match *watch {
            Watch::Waiting { deadline } => {
                if s.chaser.is_some() {
                    *watch = Watch::Done;
                } else if s.time > deadline {
                    *watch = Watch::Overdue { since: deadline };
                }
            }
            Watch::Overdue { since } => {
                if s.chaser.is_some() {
                    *watch = Watch::Done;
                    out.push(CoachFinding {
                        check: CheckId::ChaserConvergence,
                        severity: Severity::Late,
                        game_time: s.time,
                        subject: "defense".into(),
                        expected: format!(
                            "a chaser assigned within {:.2}s of contact",
                            self.tol.chaser_assign_secs
                        ),
                        observed: format!("assigned {:.2}s past the tolerance", s.time - since),
                    });
                }
            }
            Watch::Done => {}
        }

        // The assigned chaser's intercept must track the live ball: the
        // predicted landing while airborne, the ball itself on the ground.
        let (Some(chaser), Some(ball)) = (s.chaser, &s.ball) else {
            return;
        };
        let Some(fielder) = s.fielders.iter().find(|f| f.index == chaser) else {
            return;
        };
        let goal = ball.predicted_landing.unwrap_or(ball.pos);
        let tracking = fielder
            .move_target
            .is_some_and(|t| flat_dist(t, goal) <= self.tol.intercept_slack_m)
            // Standing on the goal with no order left is also converged.
            || flat_dist(fielder.pos, goal) <= self.tol.intercept_slack_m;
        if tracking {
            self.play.intercept_bad_since = None;
        } else if !self.play.intercept_flagged {
            let since = *self.play.intercept_bad_since.get_or_insert(s.time);
            if s.time - since > self.tol.intercept_grace_secs {
                self.play.intercept_flagged = true;
                out.push(CoachFinding {
                    check: CheckId::ChaserConvergence,
                    severity: Severity::Late,
                    game_time: s.time,
                    subject: format!("fielder #{chaser}"),
                    expected: format!(
                        "intercept within {:.1} m of the live ball's goal",
                        self.tol.intercept_slack_m
                    ),
                    observed: format!(
                        "target {:?} vs goal ({:.1}, {:.1}) for over {:.2}s",
                        fielder.move_target.map(|t| (t.x, t.z)),
                        goal.x,
                        goal.z,
                        self.tol.intercept_grace_secs
                    ),
                });
            }
        }
    }

    // ── Base coverage ────────────────────────────────────────────────────────

    fn check_coverage(&mut self, s: &CoachSnapshot, out: &mut Vec<CoachFinding>) {
        let Some(contact) = &s.contact else {
            return;
        };
        if self.play.coverage_checked || s.time < contact.at + self.tol.coverage_secs {
            return;
        }
        self.play.coverage_checked = true;
        // Force-relevant bags: first for the batter, plus the bag ahead of
        // every forced runner (the same chain the throw races).
        let bases = bases_from(&contact.bases_at_contact);
        let mut wanted = vec![0usize];
        for (base, &occ) in contact.bases_at_contact.iter().enumerate() {
            if occ && rules::is_forced(&bases, base) {
                wanted.push(base + 1);
            }
        }
        for bag in wanted {
            let covered = s.covers.iter().any(|&(b, _)| b == bag)
                || s.chaser.is_some_and(|c| {
                    s.fielders.iter().any(|f| {
                        f.index == c
                            && s.base_positions
                                .get(bag)
                                .is_some_and(|p| flat_dist(f.pos, *p) < 2.0)
                    })
                });
            if !covered {
                out.push(CoachFinding {
                    check: CheckId::BaseCoverage,
                    severity: Severity::Violation,
                    game_time: s.time,
                    subject: format!("bag {bag}"),
                    expected: format!(
                        "a coverer assigned within {:.2}s of contact",
                        self.tol.coverage_secs
                    ),
                    observed: "no coverer and the chaser elsewhere".into(),
                });
            }
        }
    }

    // ── Catcher receives ─────────────────────────────────────────────────────

    fn check_catcher_receives(&mut self, s: &CoachSnapshot, out: &mut Vec<CoachFinding>) {
        if !s.untouched_pitch_result {
            return;
        }
        let Some(catcher) = s.catcher_pos else {
            return; // parks without a catcher let the ball fly
        };
        let entry = self.play.result_since.unwrap_or(s.time);
        let watch = self.play.catcher_watch.get_or_insert(Watch::Waiting {
            deadline: entry + self.tol.catcher_secs,
        });
        let Some(ball) = &s.ball else {
            return;
        };
        let received = ball.vel.length() < self.tol.rest_speed_mps
            && ball.pos.distance(catcher) < self.tol.catcher_dist_m;
        if let Watch::Waiting { deadline } = *watch {
            if received {
                *watch = Watch::Done;
            } else if s.time > deadline {
                // The pitch is already past the umpire's decision — a
                // mitt that still doesn't have it is the TADA #1/#13
                // sail-through. Fire now; Result is short.
                *watch = Watch::Done;
                out.push(CoachFinding {
                    check: CheckId::CatcherReceives,
                    severity: Severity::Violation,
                    game_time: s.time,
                    subject: "catcher".into(),
                    expected: format!(
                        "untouched pitch at rest within {:.1} m of the mitt \
                             within {:.2}s of the call",
                        self.tol.catcher_dist_m, self.tol.catcher_secs
                    ),
                    observed: format!(
                        "ball at ({:.1}, {:.1}, {:.1}) moving {:.1} m/s",
                        ball.pos.x,
                        ball.pos.y,
                        ball.pos.z,
                        ball.vel.length()
                    ),
                });
            }
        }
    }

    // ── Throw discipline ─────────────────────────────────────────────────────

    fn check_throw_discipline(&mut self, s: &CoachSnapshot, out: &mut Vec<CoachFinding>) {
        // A held gathered ball auto-throws by the deadline.
        if let Some(held_at) = s.holding_since {
            let deadline = held_at + s.auto_throw_delay_secs + self.tol.throw_hold_grace_secs;
            if s.time > deadline && !self.play.hold_flagged {
                self.play.hold_flagged = true;
                out.push(CoachFinding {
                    check: CheckId::ThrowDiscipline,
                    severity: Severity::Violation,
                    game_time: s.time,
                    subject: "ball holder".into(),
                    expected: format!(
                        "auto-throw within {:.2}s of the gather",
                        s.auto_throw_delay_secs + self.tol.throw_hold_grace_secs
                    ),
                    observed: format!("still holding {:.2}s after gathering", s.time - held_at),
                });
            }
        }
        // A decided call is announced inside its settle cap.
        if s.pending_call {
            let since = *self.play.pending_since.get_or_insert(s.time);
            if s.time - since > self.tol.pending_announce_cap_secs && !self.play.pending_flagged {
                self.play.pending_flagged = true;
                out.push(CoachFinding {
                    check: CheckId::ThrowDiscipline,
                    severity: Severity::Violation,
                    game_time: s.time,
                    subject: "umpire".into(),
                    expected: format!(
                        "pending call announced within {:.1}s",
                        self.tol.pending_announce_cap_secs
                    ),
                    observed: format!("still pending after {:.1}s", s.time - since),
                });
            }
        } else {
            self.play.pending_since = None;
        }
    }

    // ── Settlement ───────────────────────────────────────────────────────────

    fn check_settlement(&mut self, s: &CoachSnapshot, out: &mut Vec<CoachFinding>) {
        let Some(since) = self.play.result_since else {
            return;
        };
        let in_result = s.time - since;
        if !s.runners_settled
            && in_result > s.result_secs + self.tol.settle_grace_secs
            && !self.play.settle_flagged
        {
            self.play.settle_flagged = true;
            out.push(CoachFinding {
                check: CheckId::Settlement,
                severity: Severity::Violation,
                game_time: s.time,
                subject: "runners".into(),
                expected: format!(
                    "every rig settled within {:.1}s of the result",
                    s.result_secs + self.tol.settle_grace_secs
                ),
                observed: format!("a rig still mid-path {in_result:.1}s into the pause"),
            });
        }
        if in_result > s.result_secs + self.tol.result_stuck_cap_secs && !self.play.stuck_flagged {
            self.play.stuck_flagged = true;
            out.push(CoachFinding {
                check: CheckId::Settlement,
                severity: Severity::Violation,
                game_time: s.time,
                subject: "flow".into(),
                expected: "the result pause ends inside its hard cap".into(),
                observed: format!("still in Result after {in_result:.1}s"),
            });
        }
    }

    // ── Idle discipline ──────────────────────────────────────────────────────

    fn arm_idle_deadlines(&mut self, s: &CoachSnapshot) {
        self.play.idle_deadlines = s
            .fielders
            .iter()
            .filter_map(|f| {
                let spot = s.fielder_spots.get(f.index)?;
                let jog = flat_dist(f.pos, *spot) / self.tol.return_speed_mps.max(0.1);
                Some((f.index, s.time + jog + self.tol.idle_grace_secs))
            })
            .collect();
    }

    fn check_idle(&mut self, s: &CoachSnapshot, out: &mut Vec<CoachFinding>) {
        for &(index, deadline) in &self.play.idle_deadlines {
            if s.time <= deadline || self.play.idle_flagged.contains(&index) {
                continue;
            }
            let Some(fielder) = s.fielders.iter().find(|f| f.index == index) else {
                continue;
            };
            let Some(spot) = s.fielder_spots.get(index) else {
                continue;
            };
            let off = flat_dist(fielder.pos, *spot);
            let heading_back = fielder
                .move_target
                .is_some_and(|t| flat_dist(t, *spot) < self.tol.idle_dist_m);
            if off > self.tol.idle_dist_m && !heading_back {
                self.play.idle_flagged.push(index);
                out.push(CoachFinding {
                    check: CheckId::IdleDiscipline,
                    severity: Severity::Violation,
                    game_time: s.time,
                    subject: format!("fielder #{index}"),
                    expected: format!(
                        "back on his spot within the jog plus {:.2}s",
                        self.tol.idle_grace_secs
                    ),
                    observed: format!("{off:.1} m off the spot, not heading back"),
                });
            }
        }
    }

    // ── Play end ─────────────────────────────────────────────────────────────

    /// The live play ended: anything still overdue never happened.
    fn finalize_live_play(&mut self, s: &CoachSnapshot, out: &mut Vec<CoachFinding>) {
        for (base, expected, watch) in &mut self.play.runner_watch {
            if let Watch::Overdue { since } = *watch {
                *watch = Watch::Done;
                out.push(CoachFinding {
                    check: CheckId::RunnerBreaks,
                    severity: Severity::Violation,
                    game_time: s.time,
                    subject: format!("runner on base {base}"),
                    expected: format!("{expected:?} off contact"),
                    observed: format!(
                        "never left the bag (overdue since {since:.2}s) before the play ended"
                    ),
                });
            }
        }
        if let Some(Watch::Overdue { since }) = self.play.chaser_watch {
            self.play.chaser_watch = Some(Watch::Done);
            out.push(CoachFinding {
                check: CheckId::ChaserConvergence,
                severity: Severity::Violation,
                game_time: s.time,
                subject: "defense".into(),
                expected: "a chaser assigned to the live ball".into(),
                observed: format!("no chaser from {since:.2}s until the play ended"),
            });
        }
    }
}

impl CoachSnapshot {
    fn runners_on(&self, base: usize) -> Option<&RunnerFacts> {
        self.runners.iter().find(|r| r.base == base)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(time: f32, phase: CoachPhase) -> CoachSnapshot {
        CoachSnapshot {
            time,
            phase,
            in_steal_window: false,
            pending_call: false,
            runners_settled: true,
            untouched_pitch_result: false,
            result_secs: 1.2,
            auto_throw_delay_secs: 0.6,
            catcher_pos: Some(Vec3::new(0.0, 0.0, -2.0)),
            ball: None,
            contact: None,
            chaser: None,
            covers: Vec::new(),
            holding_since: None,
            fielders: Vec::new(),
            fielder_spots: Vec::new(),
            base_positions: vec![
                Vec3::new(-19.4, 0.0, 19.4),
                Vec3::new(0.0, 0.0, 38.8),
                Vec3::new(19.4, 0.0, 19.4),
            ],
            runners: Vec::new(),
        }
    }

    fn contact(at: f32, class: ContactClass, bases: &[usize]) -> ContactFacts {
        let mut occ = vec![false; 3];
        for &b in bases {
            occ[b] = true;
        }
        ContactFacts {
            at,
            class,
            outs_at_contact: 0,
            bases_at_contact: occ,
            steal_armed: false,
        }
    }

    fn runner(base: usize, pos: Vec3, moving: bool) -> RunnerFacts {
        RunnerFacts { base, pos, moving }
    }

    fn count(findings: &[CoachFinding], check: CheckId, severity: Severity) -> usize {
        findings
            .iter()
            .filter(|f| f.check == check && f.severity == severity)
            .count()
    }

    // ── Steal window ─────────────────────────────────────────────────────────

    #[test]
    fn pitch_during_the_steal_window_is_a_violation() {
        let mut coach = Coach::default();
        let mut s = snap(1.0, CoachPhase::WindUp);
        s.in_steal_window = true;
        let f = coach.observe(&s);
        assert_eq!(count(&f, CheckId::StealWindow, Severity::Violation), 1);
        // Fires once, not every sample.
        let f = coach.observe(&s);
        assert!(f.is_empty());
    }

    #[test]
    fn pitch_after_the_window_closes_is_clean() {
        let mut coach = Coach::default();
        let mut s = snap(1.0, CoachPhase::PrePitch);
        s.in_steal_window = true;
        assert!(coach.observe(&s).is_empty());
        let s = snap(4.0, CoachPhase::WindUp);
        assert!(coach.observe(&s).is_empty());
    }

    // ── Runner breaks ────────────────────────────────────────────────────────

    fn go_now_play(runner_moving: bool) -> (Coach, Vec<CoachFinding>) {
        let mut coach = Coach::default();
        let bag = Vec3::new(-19.4, 0.0, 19.4);
        let mut all = Vec::new();
        // Forced grounder with a runner on first: GoNow expected.
        for i in 0..30 {
            let t = 10.0 + i as f32 * 0.1;
            let mut s = snap(t, CoachPhase::InPlay);
            s.contact = Some(contact(10.0, ContactClass::Grounder, &[0]));
            s.chaser = Some(0);
            s.covers = vec![(0, 1), (1, 2), (2, 3), (3, 4)];
            s.runners = vec![runner(0, bag, runner_moving)];
            all.extend(coach.observe(&s));
        }
        // Play ends.
        let s = snap(13.5, CoachPhase::Result);
        all.extend(coach.observe(&s));
        (coach, all)
    }

    #[test]
    fn forced_grounder_runner_who_never_breaks_is_a_violation() {
        let (_, f) = go_now_play(false);
        assert_eq!(count(&f, CheckId::RunnerBreaks, Severity::Violation), 1);
    }

    #[test]
    fn forced_grounder_runner_who_breaks_is_clean() {
        let (_, f) = go_now_play(true);
        assert_eq!(count(&f, CheckId::RunnerBreaks, Severity::Violation), 0);
        assert_eq!(count(&f, CheckId::RunnerBreaks, Severity::Late), 0);
    }

    #[test]
    fn late_break_is_reported_late_not_violation() {
        let mut coach = Coach::default();
        let bag = Vec3::new(-19.4, 0.0, 19.4);
        let mut all = Vec::new();
        for i in 0..30 {
            let t = 10.0 + i as f32 * 0.1;
            let mut s = snap(t, CoachPhase::InPlay);
            s.contact = Some(contact(10.0, ContactClass::Grounder, &[0]));
            s.chaser = Some(0);
            s.covers = vec![(0, 1), (1, 2), (2, 3), (3, 4)];
            // Breaks only 1.5 s after contact — past the 0.4 s tolerance.
            s.runners = vec![runner(0, bag, t >= 11.5)];
            all.extend(coach.observe(&s));
        }
        assert_eq!(count(&all, CheckId::RunnerBreaks, Severity::Late), 1);
        assert_eq!(count(&all, CheckId::RunnerBreaks, Severity::Violation), 0);
    }

    #[test]
    fn tag_up_runner_leaving_early_is_a_violation() {
        let mut coach = Coach::default();
        let bag = Vec3::new(19.4, 0.0, 19.4); // third base
        let mut s = snap(10.2, CoachPhase::InPlay);
        s.contact = Some(contact(10.0, ContactClass::DeepFly, &[2]));
        s.ball = Some(BallFacts {
            pos: Vec3::new(0.0, 20.0, 60.0),
            vel: Vec3::new(0.0, 5.0, 20.0),
            predicted_landing: Some(Vec3::new(0.0, 0.0, 80.0)),
        });
        s.chaser = Some(0);
        // 8 m off the bag with the ball still up: left early.
        s.runners = vec![runner(2, bag + Vec3::new(-6.0, 0.0, -5.3), true)];
        let f = coach.observe(&s);
        assert_eq!(count(&f, CheckId::RunnerBreaks, Severity::Violation), 1);
    }

    #[test]
    fn tag_up_runner_holding_is_clean() {
        let mut coach = Coach::default();
        let bag = Vec3::new(19.4, 0.0, 19.4);
        let mut s = snap(10.2, CoachPhase::InPlay);
        s.contact = Some(contact(10.0, ContactClass::DeepFly, &[2]));
        s.ball = Some(BallFacts {
            pos: Vec3::new(0.0, 20.0, 60.0),
            vel: Vec3::new(0.0, 5.0, 20.0),
            predicted_landing: Some(Vec3::new(0.0, 0.0, 80.0)),
        });
        s.chaser = Some(0);
        s.runners = vec![runner(2, bag + Vec3::new(-2.0, 0.0, 0.0), false)];
        let f = coach.observe(&s);
        assert_eq!(count(&f, CheckId::RunnerBreaks, Severity::Violation), 0);
    }

    #[test]
    fn steal_armed_contact_suspends_break_expectations() {
        let mut coach = Coach::default();
        let bag = Vec3::new(-19.4, 0.0, 19.4);
        let mut all = Vec::new();
        for i in 0..30 {
            let t = 10.0 + i as f32 * 0.1;
            let mut s = snap(t, CoachPhase::InPlay);
            let mut c = contact(10.0, ContactClass::Grounder, &[0]);
            c.steal_armed = true;
            s.contact = Some(c);
            s.chaser = Some(0);
            s.runners = vec![runner(0, bag, false)];
            all.extend(coach.observe(&s));
        }
        let s = snap(13.5, CoachPhase::Result);
        all.extend(coach.observe(&s));
        assert_eq!(count(&all, CheckId::RunnerBreaks, Severity::Violation), 0);
    }

    // ── Chaser convergence ───────────────────────────────────────────────────

    #[test]
    fn live_ball_with_no_chaser_is_a_violation_at_play_end() {
        let mut coach = Coach::default();
        let mut all = Vec::new();
        for i in 0..20 {
            let t = 10.0 + i as f32 * 0.1;
            let mut s = snap(t, CoachPhase::InPlay);
            s.contact = Some(contact(10.0, ContactClass::CatchableFly, &[]));
            all.extend(coach.observe(&s));
        }
        let s = snap(12.5, CoachPhase::Result);
        all.extend(coach.observe(&s));
        assert_eq!(
            count(&all, CheckId::ChaserConvergence, Severity::Violation),
            1
        );
    }

    #[test]
    fn promptly_assigned_chaser_tracking_the_landing_is_clean() {
        let mut coach = Coach::default();
        let landing = Vec3::new(5.0, 0.0, 60.0);
        let mut all = Vec::new();
        for i in 0..20 {
            let t = 10.0 + i as f32 * 0.1;
            let mut s = snap(t, CoachPhase::InPlay);
            s.contact = Some(contact(10.0, ContactClass::CatchableFly, &[]));
            s.ball = Some(BallFacts {
                pos: Vec3::new(2.0, 15.0, 40.0),
                vel: Vec3::new(1.0, -2.0, 15.0),
                predicted_landing: Some(landing),
            });
            s.chaser = Some(2);
            s.fielders = vec![FielderFacts {
                index: 2,
                pos: Vec3::new(0.0, 0.0, 50.0),
                move_target: Some(landing),
            }];
            all.extend(coach.observe(&s));
        }
        assert_eq!(
            count(&all, CheckId::ChaserConvergence, Severity::Violation),
            0
        );
        assert_eq!(count(&all, CheckId::ChaserConvergence, Severity::Late), 0);
    }

    #[test]
    fn chaser_with_a_stale_intercept_is_flagged() {
        let mut coach = Coach::default();
        let mut all = Vec::new();
        for i in 0..20 {
            let t = 10.0 + i as f32 * 0.1;
            let mut s = snap(t, CoachPhase::InPlay);
            s.contact = Some(contact(10.0, ContactClass::CatchableFly, &[]));
            s.ball = Some(BallFacts {
                pos: Vec3::new(2.0, 10.0, 40.0),
                vel: Vec3::new(1.0, -2.0, 15.0),
                predicted_landing: Some(Vec3::new(5.0, 0.0, 60.0)),
            });
            s.chaser = Some(2);
            s.fielders = vec![FielderFacts {
                index: 2,
                pos: Vec3::new(-20.0, 0.0, 30.0),
                // Parked on a target 20+ m from the live landing.
                move_target: Some(Vec3::new(-25.0, 0.0, 30.0)),
            }];
            all.extend(coach.observe(&s));
        }
        assert_eq!(count(&all, CheckId::ChaserConvergence, Severity::Late), 1);
    }

    // ── Base coverage ────────────────────────────────────────────────────────

    #[test]
    fn uncovered_force_bag_is_a_violation() {
        let mut coach = Coach::default();
        let mut all = Vec::new();
        for i in 0..10 {
            let t = 10.0 + i as f32 * 0.1;
            let mut s = snap(t, CoachPhase::InPlay);
            // Runner on first: bags 0 (batter) and 1 (force) are relevant.
            s.contact = Some(contact(10.0, ContactClass::Grounder, &[0]));
            s.chaser = Some(0);
            s.covers = vec![(0, 1)]; // second base left uncovered
            s.runners = vec![runner(0, Vec3::new(-19.4, 0.0, 19.4), true)];
            all.extend(coach.observe(&s));
        }
        assert_eq!(count(&all, CheckId::BaseCoverage, Severity::Violation), 1);
    }

    #[test]
    fn covered_force_bags_are_clean() {
        let mut coach = Coach::default();
        let mut all = Vec::new();
        for i in 0..10 {
            let t = 10.0 + i as f32 * 0.1;
            let mut s = snap(t, CoachPhase::InPlay);
            s.contact = Some(contact(10.0, ContactClass::Grounder, &[0]));
            s.chaser = Some(0);
            s.covers = vec![(0, 1), (1, 2)];
            s.runners = vec![runner(0, Vec3::new(-19.4, 0.0, 19.4), true)];
            all.extend(coach.observe(&s));
        }
        assert_eq!(count(&all, CheckId::BaseCoverage, Severity::Violation), 0);
    }

    // ── Catcher receives ─────────────────────────────────────────────────────

    #[test]
    fn untouched_pitch_resting_in_the_mitt_is_clean() {
        let mut coach = Coach::default();
        let mut all = Vec::new();
        for i in 0..12 {
            let t = 20.0 + i as f32 * 0.1;
            let mut s = snap(t, CoachPhase::Result);
            s.untouched_pitch_result = true;
            s.ball = Some(BallFacts {
                pos: Vec3::new(0.0, 0.5, -1.6),
                vel: Vec3::ZERO,
                predicted_landing: None,
            });
            all.extend(coach.observe(&s));
        }
        assert_eq!(
            count(&all, CheckId::CatcherReceives, Severity::Violation),
            0
        );
    }

    #[test]
    fn untouched_pitch_sailing_past_the_mitt_is_a_violation() {
        let mut coach = Coach::default();
        let mut all = Vec::new();
        for i in 0..12 {
            let t = 20.0 + i as f32 * 0.1;
            let mut s = snap(t, CoachPhase::Result);
            s.untouched_pitch_result = true;
            s.ball = Some(BallFacts {
                pos: Vec3::new(0.0, 0.8, -6.0 - i as f32),
                vel: Vec3::new(0.0, 0.0, -20.0),
                predicted_landing: None,
            });
            all.extend(coach.observe(&s));
        }
        assert_eq!(
            count(&all, CheckId::CatcherReceives, Severity::Violation),
            1
        );
    }

    // ── Throw discipline ─────────────────────────────────────────────────────

    #[test]
    fn ball_held_past_the_auto_throw_deadline_is_a_violation() {
        let mut coach = Coach::default();
        let mut s = snap(11.5, CoachPhase::InPlay);
        s.contact = Some(contact(10.0, ContactClass::Grounder, &[]));
        s.chaser = None;
        s.holding_since = Some(10.4); // 1.1 s held > 0.6 + 0.3 grace
        let f = coach.observe(&s);
        assert_eq!(count(&f, CheckId::ThrowDiscipline, Severity::Violation), 1);
    }

    #[test]
    fn ball_thrown_inside_the_deadline_is_clean() {
        let mut coach = Coach::default();
        let mut s = snap(10.8, CoachPhase::InPlay);
        s.contact = Some(contact(10.0, ContactClass::Grounder, &[]));
        s.holding_since = Some(10.4); // 0.4 s held < 0.9 deadline
        let f = coach.observe(&s);
        assert_eq!(count(&f, CheckId::ThrowDiscipline, Severity::Violation), 0);
    }

    #[test]
    fn pending_call_never_announced_is_a_violation() {
        let mut coach = Coach::default();
        let mut all = Vec::new();
        for i in 0..60 {
            let t = 10.0 + i as f32 * 0.1;
            let mut s = snap(t, CoachPhase::InPlay);
            s.contact = Some(contact(10.0, ContactClass::Grounder, &[]));
            s.pending_call = true;
            all.extend(coach.observe(&s));
        }
        assert_eq!(
            count(&all, CheckId::ThrowDiscipline, Severity::Violation),
            1
        );
    }

    #[test]
    fn pending_call_announced_inside_the_cap_is_clean() {
        let mut coach = Coach::default();
        let mut all = Vec::new();
        for i in 0..20 {
            let t = 10.0 + i as f32 * 0.1;
            let mut s = snap(t, CoachPhase::InPlay);
            s.contact = Some(contact(10.0, ContactClass::Grounder, &[]));
            s.pending_call = i < 15; // announced at 1.5 s
            all.extend(coach.observe(&s));
        }
        assert_eq!(
            count(&all, CheckId::ThrowDiscipline, Severity::Violation),
            0
        );
    }

    // ── Settlement ───────────────────────────────────────────────────────────

    #[test]
    fn runners_never_settling_is_a_violation() {
        let mut coach = Coach::default();
        let mut all = Vec::new();
        for i in 0..200 {
            let t = 30.0 + i as f32 * 0.1;
            let mut s = snap(t, CoachPhase::Result);
            s.runners_settled = false;
            all.extend(coach.observe(&s));
        }
        assert_eq!(count(&all, CheckId::Settlement, Severity::Violation), 1);
    }

    #[test]
    fn a_normal_result_pause_is_clean() {
        let mut coach = Coach::default();
        let mut all = Vec::new();
        for i in 0..15 {
            let t = 30.0 + i as f32 * 0.1;
            let mut s = snap(t, CoachPhase::Result);
            s.runners_settled = i > 5;
            all.extend(coach.observe(&s));
        }
        assert_eq!(count(&all, CheckId::Settlement, Severity::Violation), 0);
    }

    // ── Idle discipline ──────────────────────────────────────────────────────

    #[test]
    fn fielder_parked_off_his_spot_between_plays_is_a_violation() {
        let mut coach = Coach::default();
        let spot = Vec3::new(0.0, 0.0, 50.0);
        let mut all = Vec::new();
        for i in 0..40 {
            let t = 40.0 + i as f32 * 0.1;
            let mut s = snap(t, CoachPhase::PrePitch);
            s.fielder_spots = vec![spot];
            s.fielders = vec![FielderFacts {
                index: 0,
                // Parked 8 m off the spot, no movement order.
                pos: spot + Vec3::new(8.0, 0.0, 0.0),
                move_target: None,
            }];
            all.extend(coach.observe(&s));
        }
        assert_eq!(count(&all, CheckId::IdleDiscipline, Severity::Violation), 1);
    }

    #[test]
    fn fielder_jogging_back_to_his_spot_is_clean() {
        let mut coach = Coach::default();
        let spot = Vec3::new(0.0, 0.0, 50.0);
        let mut all = Vec::new();
        for i in 0..40 {
            let t = 40.0 + i as f32 * 0.1;
            let mut s = snap(t, CoachPhase::PrePitch);
            s.fielder_spots = vec![spot];
            // Jogs in at the return speed; heading straight for the spot.
            let remaining = (8.0 - (t - 40.0) * 4.0).max(0.0);
            s.fielders = vec![FielderFacts {
                index: 0,
                pos: spot + Vec3::new(remaining, 0.0, 0.0),
                move_target: (remaining > 0.0).then_some(spot),
            }];
            all.extend(coach.observe(&s));
        }
        assert_eq!(count(&all, CheckId::IdleDiscipline, Severity::Violation), 0);
    }

    #[test]
    fn set_fielders_are_clean_forever() {
        let mut coach = Coach::default();
        let spot = Vec3::new(0.0, 0.0, 50.0);
        for i in 0..100 {
            let t = 40.0 + i as f32 * 0.1;
            let mut s = snap(t, CoachPhase::PrePitch);
            s.fielder_spots = vec![spot];
            s.fielders = vec![FielderFacts {
                index: 0,
                pos: spot,
                move_target: None,
            }];
            assert!(coach.observe(&s).is_empty());
        }
    }
}
