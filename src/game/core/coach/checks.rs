//! The Coach's individual expectation checks.
//!
//! One method per [`CheckId`], all with the same shape — read the
//! [`CoachSnapshot`], push any [`CoachFinding`] onto `out`, and mutate only
//! the Coach's own memory. [`Coach::observe`] in the parent module is the
//! only caller and decides the order, so adding a check is a method here plus
//! a line there and nothing else in the game can tell the difference.

use super::*;

impl Coach {
    // ── Steal window ─────────────────────────────────────────────────────────

    /// The pre-pitch window gates the pitch: while it is still running the
    /// ball must not be on its way (no WindUp, no live Pitch).
    pub(super) fn check_steal_window(&mut self, s: &CoachSnapshot, out: &mut Vec<CoachFinding>) {
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

    pub(super) fn check_runner_breaks(&mut self, s: &CoachSnapshot, out: &mut Vec<CoachFinding>) {
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

    pub(super) fn check_chaser(&mut self, s: &CoachSnapshot, out: &mut Vec<CoachFinding>) {
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

    pub(super) fn check_coverage(&mut self, s: &CoachSnapshot, out: &mut Vec<CoachFinding>) {
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

    pub(super) fn check_catcher_receives(
        &mut self,
        s: &CoachSnapshot,
        out: &mut Vec<CoachFinding>,
    ) {
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

    pub(super) fn check_throw_discipline(
        &mut self,
        s: &CoachSnapshot,
        out: &mut Vec<CoachFinding>,
    ) {
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

    pub(super) fn check_settlement(&mut self, s: &CoachSnapshot, out: &mut Vec<CoachFinding>) {
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

    pub(super) fn arm_idle_deadlines(&mut self, s: &CoachSnapshot) {
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

    pub(super) fn check_idle(&mut self, s: &CoachSnapshot, out: &mut Vec<CoachFinding>) {
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
    pub(super) fn finalize_live_play(&mut self, s: &CoachSnapshot, out: &mut Vec<CoachFinding>) {
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
