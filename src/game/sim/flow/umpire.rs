//! The umpire: every place a rule result becomes a scoreboard change and an
//! announcement.
//!
//! These calls all need the same four things — what the call changes
//! ([`ScoreBoard`], [`Bases`]), the [`Ruleset`] it is judged against, and the
//! [`PlayBanner`] writer it is announced through. Threading that quartet
//! through each call by hand meant a six- and seven-parameter signature per
//! rule result, with the *interesting* argument (which call was made) buried
//! among four that never vary within a system. [`Umpire`] borrows the quartet
//! once, so each method takes only what actually distinguishes it.
//!
//! Every scoring path in `flow` runs through here, which is also what keeps
//! the layer invariant honest: `fx`, `fielding`, and `runner` report, and
//! only `flow` applies rules (see `CLAUDE.md`).

use bevy::prelude::*;

use crate::game::ScoreBoard;
use crate::game::rules::{self, BallCall, Bases, OutKind, Outcome, StealResult, StrikeCall};
use crate::game::variant::Ruleset;

use super::{BannerTone, Play, PlayBanner};

/// A borrow of everything a rule result needs to be applied and announced.
///
/// Built per system from its resources and passed down; the lifetimes are the
/// borrow of the caller's `ResMut`s (`'a`) and the event writer's own world
/// borrow (`'w`).
pub(super) struct Umpire<'a, 'w> {
    score: &'a mut ScoreBoard,
    bases: &'a mut Bases,
    rules: &'a Ruleset,
    banner: &'a mut EventWriter<'w, PlayBanner>,
}

impl<'a, 'w> Umpire<'a, 'w> {
    pub(super) fn new(
        score: &'a mut ScoreBoard,
        bases: &'a mut Bases,
        rules: &'a Ruleset,
        banner: &'a mut EventWriter<'w, PlayBanner>,
    ) -> Self {
        Self {
            score,
            bases,
            rules,
            banner,
        }
    }

    /// Announce a call. Private so every banner this module sends is one of
    /// the rule results below — a call is never announced without the
    /// scoreboard change that earned it.
    fn announce(&mut self, text: impl Into<String>, tone: BannerTone) {
        self.banner.send(PlayBanner::new(text, tone));
    }

    /// Whether `base` is occupied. The umpire holds the only borrow of
    /// [`Bases`] while it is making calls, so reads that inform a call go
    /// through here rather than forcing the caller to keep a second one.
    pub(super) fn bases_occupied(&self, base: usize) -> bool {
        self.bases.is_occupied(base)
    }

    /// Records a taken ball. Returns whether it was ball four (a dead-ball
    /// walk, which pre-empts any steal attempt).
    pub(super) fn add_ball(&mut self) -> bool {
        match rules::call_ball(self.score, self.bases, self.rules) {
            BallCall::Walk { .. } => {
                // Good, not Epic: Epic is the home-run tier (gold banner + the
                // triumphant stinger) and a free pass was reading identical to
                // a ball over the fence (TODO 68).
                self.announce("WALK", BannerTone::Good);
                true
            }
            BallCall::Ball => {
                self.announce("BALL", BannerTone::Info);
                false
            }
        }
    }

    pub(super) fn add_strike(&mut self, swinging: bool, dropped_third: bool) -> StrikeCall {
        let call = rules::call_strike(self.score, self.bases, self.rules, dropped_third);
        match call {
            StrikeCall::DroppedThird => self.announce("DROPPED 3RD STRIKE!", BannerTone::Good),
            StrikeCall::Strikeout => self.announce("STRIKEOUT!", BannerTone::Bad),
            StrikeCall::Strike if swinging => self.announce("SWING & MISS", BannerTone::Info),
            StrikeCall::Strike => self.announce("STRIKE", BannerTone::Info),
        }
        call
    }

    /// Resolves a sent runner once the catcher has the ball: the jump beats
    /// the throw on off-speed pitches, a fastball cuts the runner down.
    pub(super) fn resolve_steal(&mut self, play: &Play) {
        let off_speed = play.pitch.kind != Some(rules::PitchKind::Fastball);
        match rules::attempt_steal(
            self.score,
            self.bases,
            self.rules,
            off_speed,
            play.duel.big_jump,
        ) {
            StealResult::Stolen { .. } => self.announce("STOLEN BASE!", BannerTone::Good),
            StealResult::Caught => self.announce("CAUGHT STEALING", BannerTone::Bad),
            StealResult::NoRunner => {}
        }
    }

    /// A batted ball that reached base: `hit_bases` bases for the batter,
    /// runners advancing (`jump` = they were already going).
    fn hit(&mut self, hit_bases: u32, label: &str, tone: BannerTone, jump: bool) {
        let runs = rules::apply_hit(self.score, self.bases, hit_bases, jump);
        let text = if runs > 0 {
            format!("{label}  +{runs}")
        } else {
            label.to_string()
        };
        self.announce(text, tone);
    }

    /// Applies a decided [`Outcome`] and announces it.
    pub(super) fn resolve_contact(&mut self, outcome: Outcome, runners_going: bool) {
        match outcome {
            Outcome::Foul => {
                rules::foul(self.score, self.rules);
                self.announce("FOUL", BannerTone::Info);
            }
            Outcome::Out(kind) => {
                let play = rules::apply_batted_out(
                    self.score,
                    self.bases,
                    self.rules,
                    kind,
                    runners_going,
                );
                let base_text = if play.doubled_off {
                    "DOUBLED OFF!"
                } else if play.runs > 0 && matches!(kind, OutKind::Fly { .. }) {
                    "SAC FLY"
                } else {
                    match kind {
                        OutKind::Ground => "GROUND OUT",
                        OutKind::Fly { .. } => "FLY OUT",
                        OutKind::Pop => "POP OUT",
                        OutKind::FoulPop => "FOUL POP OUT",
                        OutKind::Pegged => "PEGGED!",
                        OutKind::Stretching { .. } => "OUT STRETCHING!",
                    }
                };
                let text = if play.runs > 0 {
                    format!("{base_text}  +{}", play.runs)
                } else {
                    base_text.to_string()
                };
                self.announce(text, BannerTone::Bad);
            }
            Outcome::DoublePlay => {
                let play = rules::apply_double_play(self.score, self.bases, self.rules);
                let text = if play.runs > 0 {
                    format!("DOUBLE PLAY!  +{}", play.runs)
                } else {
                    "DOUBLE PLAY!".to_string()
                };
                self.announce(text, BannerTone::Bad);
            }
            Outcome::FieldersChoice { out_base } => {
                rules::apply_fielders_choice(self.score, self.bases, self.rules, out_base);
                self.announce("FIELDER'S CHOICE", BannerTone::Bad);
            }
            Outcome::Hit(n) => {
                let label = match n {
                    1 => "SINGLE".to_string(),
                    2 => "DOUBLE".to_string(),
                    3 => "TRIPLE".to_string(),
                    n => format!("{n} BASES!"),
                };
                self.hit(n, &label, BannerTone::Good, runners_going);
            }
            // A home run is worth one more base than the field has.
            Outcome::HomeRun => {
                let bases_worth = self.bases.count() as u32 + 1;
                self.hit(bases_worth, "HOME RUN!", BannerTone::Epic, runners_going);
            }
        }
    }

    /// Dead ball: the batter takes first and forced runners move. Returns
    /// whether any run scored (an Epic banner, not merely Good).
    pub(super) fn hit_by_pitch(&mut self) {
        let runs = rules::hit_by_pitch(self.score, self.bases);
        let tone = if runs > 0 {
            BannerTone::Epic
        } else {
            BannerTone::Good
        };
        self.announce("HIT BY PITCH", tone);
    }

    /// A foul tip: a strike (never the third — see `rules::foul`). The ball is
    /// dead, so runners hold and the at-bat continues.
    pub(super) fn foul_tip(&mut self) {
        rules::foul(self.score, self.rules);
        self.announce("FOUL", BannerTone::Info);
    }
}
