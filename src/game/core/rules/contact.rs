//! Classifying batted-ball contact: fair/foul/home-run, the baserunning
//! read off contact, and swing-timing contact quality.

use bevy::math::{Vec2, Vec3};

use crate::game::variant::{FieldSpec, Ruleset};

use super::{INFIELD_GATHER_RADIUS, TAG_UP_MIN_DIST, fence_at, is_fair};

// ── Live-play resolution ──────────────────────────────────────────────────────

/// What contact alone settles. Everything except a ball over the fence stays
/// live: the fielders' chase and the runner races decide the rest during the
/// play, not at the crack of the bat.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ContactKind {
    HomeRun,
    Live {
        /// Whether the *predicted* landing is fair — cosmetic hint only; the
        /// actual call comes from where the ball really comes down.
        fair: bool,
    },
}

/// Classifies contact from the live-model predicted `landing` point (see
/// [`predict_landing`]).
pub fn classify_contact(landing: Vec3, field: &FieldSpec) -> ContactKind {
    let fair = is_fair(landing, field);
    let dist = Vec2::new(landing.x, landing.z).length();
    if fair && dist > fence_at(landing, field) {
        return ContactKind::HomeRun;
    }
    ContactKind::Live { fair }
}

// ── Baserunning reads after contact ───────────────────────────────────────────
// Pure, deterministic reads that drive *when the runner rigs break* off contact
// (never the call — the outcome still comes from the live-play races). Encodes
// the real-baseball conventions documented in docs/BASEBALL.md
// ("Baserunning after contact").

/// A fly ball hanging at least this long (seconds) is airborne long enough to
/// be a catch read; anything quicker is a grounder / hard liner that will be
/// on the ground before a runner has to commit. Calibrated against
/// [`predict_landing`] hang times (see the `contact_class_*` unit tests).
const GROUNDER_HANG_SECS: f32 = 1.2;

/// The shape of a fair batted ball as the runners read it off the bat — the
/// distinction that decides how each aboard runner breaks. Derived purely from
/// the predicted flight (hang time + landing distance); no RNG.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContactClass {
    /// On the ground (or a liner already down) before the runner must commit.
    Grounder,
    /// A fly that hangs long enough to be caught but is shallow enough that a
    /// tag-up would gain nothing — the "may be a fly out" read.
    CatchableFly,
    /// A deep fly: catchable, but far enough that tagging up can advance a
    /// runner (the sacrifice-fly distance, [`TAG_UP_MIN_DIST`]).
    DeepFly,
}

/// How a base runner breaks off contact. Purely a *choreography* decision —
/// the rigs move on this while the umpire's call is still being raced out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
pub enum RunnerBreak {
    /// Break for the next bag immediately (run on contact).
    GoNow,
    /// Advance about halfway and read the play: continue if the ball drops,
    /// retreat if it is caught.
    Halfway,
    /// Hold the bag to tag up, advancing legally after the catch.
    TagUp,
}

/// Classifies a fair batted ball for the baserunning read, from the predicted
/// `landing` point and `hang_time` (see [`predict_landing`]). Per
/// docs/BASEBALL.md "Baserunning after contact": a quick ball is a grounder,
/// a long hang deep enough for a sacrifice fly is a deep fly, everything else
/// in between is a catchable fly.
pub fn contact_class(landing: Vec3, hang_time: f32, field: &FieldSpec) -> ContactClass {
    if hang_time < GROUNDER_HANG_SECS {
        return ContactClass::Grounder;
    }
    let dist = Vec2::new(landing.x, landing.z).length();
    if dist >= TAG_UP_MIN_DIST * field.hit_scale {
        ContactClass::DeepFly
    } else {
        ContactClass::CatchableFly
    }
}

/// The break a runner takes off contact, per docs/BASEBALL.md "Baserunning
/// after contact":
/// 1. Two outs — run on contact (nothing to lose).
/// 2. Fewer than two outs, ground ball — a forced runner goes immediately; an
///    unforced runner reads whether it gets through (breaks halfway).
/// 3. Fewer than two outs, catchable fly — go halfway and read the catch.
/// 4. Fewer than two outs, deep fly — tag up.
///
/// Deterministic and RNG-free; the actual call still comes from the live-play
/// races, so this only governs *when the rig moves*, never the outcome.
pub fn runner_break(outs: u32, forced: bool, contact: ContactClass) -> RunnerBreak {
    if outs >= 2 {
        return RunnerBreak::GoNow;
    }
    match contact {
        ContactClass::Grounder if forced => RunnerBreak::GoNow,
        ContactClass::Grounder => RunnerBreak::Halfway,
        ContactClass::CatchableFly => RunnerBreak::Halfway,
        ContactClass::DeepFly => RunnerBreak::TagUp,
    }
}

/// Whether a fair ball's first-bounce landing is past the infield — the
/// "does it get through" read a `Halfway` runner (see [`runner_break`]) makes
/// off a ground ball or catchable fly that hits the dirt/grass instead of a
/// glove. Reuses [`INFIELD_GATHER_RADIUS`] (scaled by `field.hit_scale`), the
/// same infield-range radius the live-throw race already treats as "an out at
/// first is only contested on infield balls" — a landing at or beyond it is
/// through the infield for the same reason a gather out there is a lost
/// cause for the defense. Per docs/BASEBALL.md "Baserunning after contact".
pub fn landed_past_infield(landing: Vec3, field: &FieldSpec) -> bool {
    Vec2::new(landing.x, landing.z).length() >= INFIELD_GATHER_RADIUS * field.hit_scale
}

// ── Contact quality ───────────────────────────────────────────────────────────
// The batting-feel spine (docs/superpowers/specs/2026-07-30-batting-feel-design.md
// §2): a swing's outcome is graded by how far off dead-on timing it lands,
// not by contact-or-miss alone.

/// How well-timed a swing was, from a whiff to a dead-on hit. Graded by
/// [`contact_quality`] against the [`Ruleset`] timing windows.
///
/// `Weak` is never produced by [`contact_quality`] — the Classic timing
/// windows below only ever yield the other four variants. It exists so the
/// Plan-C PCI (plate-coverage-indicator) adapter, which grades contact by a
/// shrunk timing window instead, has a quality to report without widening
/// this enum later; keep matches on it exhaustive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
pub enum ContactQuality {
    /// No contact: the swing missed entirely.
    Whiff,
    /// Contact, but late/early enough it only ever fouls off.
    FoulTip,
    /// Weak contact (Plan-C PCI adapter only — see the enum doc comment).
    Weak,
    /// Solidly timed contact.
    Solid,
    /// Dead-on timing.
    Perfect,
}

/// Grades a swing's timing error (`dt_ms`, milliseconds, signed so early
/// swings are negative) into a [`ContactQuality`] using the active
/// [`Ruleset`]'s windows (`perfect_ms`/`solid_ms`/`foul_ms`). Symmetric
/// around zero: an early and a late swing of the same magnitude grade the
/// same. Never yields `ContactQuality::Weak` — see that variant's doc
/// comment.
pub fn contact_quality(dt_ms: f32, rules: &Ruleset) -> ContactQuality {
    let dt = dt_ms.abs();
    if dt <= rules.batting.perfect_ms {
        ContactQuality::Perfect
    } else if dt <= rules.batting.solid_ms {
        ContactQuality::Solid
    } else if dt <= rules.batting.foul_ms {
        ContactQuality::FoulTip
    } else {
        ContactQuality::Whiff
    }
}

/// Shapes a batted-ball velocity by how well the swing was timed: scales the
/// exit speed by the quality's multiplier and rotates the launch toward the
/// pull side by `pull_yaw_per_ms · dt_ms`.
///
/// `base` is the raw vector from [`hit_velocity`] (aim + contact-point launch);
/// `dt_ms` is the signed swing timing (early = negative — see
/// `flow::swing_dt_ms`). The yaw is applied about the vertical (Y) axis in the
/// same sense as [`hit_velocity`]'s `spray` angle, where a *negative* horizontal
/// component is world −X. First base is at −X and `aim.x` is negated in the
/// hit mapping (see CLAUDE.md), so −X is the right-handed batter's pull side:
/// an early (negative `dt_ms`) swing yields a negative yaw that rotates the ball
/// toward −X, i.e. pulls it. Late (positive) contact pushes the other way.
///
/// `Whiff`/`FoulTip` never reach here (they put no ball in play); they return
/// `base` unchanged so the match stays exhaustive.
pub fn apply_contact_quality(
    base: Vec3,
    quality: ContactQuality,
    dt_ms: f32,
    rules: &Ruleset,
) -> Vec3 {
    let exit_mult = match quality {
        ContactQuality::Perfect => rules.batting.exit_perfect,
        ContactQuality::Solid => rules.batting.exit_solid,
        ContactQuality::Weak => rules.batting.exit_weak,
        ContactQuality::Whiff | ContactQuality::FoulTip => return base,
    };
    let scaled = base * exit_mult;
    // Rotate the horizontal (x, z) launch about +Y by the pull yaw. Matching
    // `hit_velocity`'s spray convention (x = h·sin θ, z = h·cos θ), a positive
    // yaw increases θ (toward +X) and a negative yaw decreases it (toward −X).
    let yaw = rules.batting.pull_yaw_per_ms * dt_ms;
    let (s, c) = yaw.sin_cos();
    Vec3::new(
        scaled.x * c + scaled.z * s,
        scaled.y,
        scaled.z * c - scaled.x * s,
    )
}

/// PCI contact grading (spec §3): the timing windows shrink linearly with the
/// cursor's miss distance. `frac = miss/radius`; effective perfect =
/// `perfect_ms·(1−frac)` (0 at the radius), effective solid =
/// `solid_ms·(1−frac/2)` (halved at the radius). Timing inside the FULL solid
/// window but outside the shrunk one is clipped contact → `Weak` (the only
/// source of Weak in the game). Beyond the radius the bat's sweet spot never
/// reaches the ball: best case FoulTip on timing alone.
pub fn pci_contact_quality(dt_ms: f32, miss_m: f32, rules: &Ruleset) -> ContactQuality {
    let dt = dt_ms.abs();
    if dt > rules.batting.foul_ms {
        return ContactQuality::Whiff;
    }
    let frac = (miss_m / rules.batting.pci_radius_m).max(0.0);
    if frac > 1.0 {
        return ContactQuality::FoulTip;
    }
    let perfect_eff = rules.batting.perfect_ms * (1.0 - frac);
    let solid_eff = rules.batting.solid_ms * (1.0 - frac / 2.0);
    if dt <= perfect_eff {
        ContactQuality::Perfect
    } else if dt <= solid_eff {
        ContactQuality::Solid
    } else if dt <= rules.batting.solid_ms {
        ContactQuality::Weak
    } else {
        ContactQuality::FoulTip
    }
}

/// PCI hit direction (spec §3): derived from the contact-point offset, not
/// raw aim. Normalized against the cursor-radius scale so a half-radius miss
/// is a half-strength aim; components clamp to the aim domain. Signs: cursor
/// under the ball lofts (+y); the x component keeps raw-aim's sense (the −X
/// pull negation lives in `hit_velocity`, per CLAUDE.md).
pub fn pci_aim(offset: Vec2) -> Vec2 {
    const PCI_AIM_SCALE_M: f32 = 0.20;
    Vec2::new(
        (offset.x / PCI_AIM_SCALE_M).clamp(-1.0, 1.0),
        (-offset.y / PCI_AIM_SCALE_M).clamp(-1.0, 1.0),
    )
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "contact.test.rs"]
mod tests;
