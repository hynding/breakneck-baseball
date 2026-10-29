//! Live-play resolution: catches, gathers, throws, and the runner/aim calls
//! that steer them.

use bevy::math::{Vec2, Vec3};

use crate::game::variant::{FieldSpec, PaceTuning, Ruleset};

use super::{
    AIM_DEADZONE, Bases, INFIELD_GATHER_RADIUS, OutKind, Outcome, POP_RADIUS, RunnerCall,
    TAG_UP_MIN_DIST, TagUp, is_fair,
};

/// Whether the runner on (0-indexed) `base` is *forced* to advance: every base
/// behind him back toward home is occupied, so the batter reaching first
/// pushes the whole chain. The runner on first is always forced. Per
/// docs/BASEBALL.md.
pub fn is_forced(bases: &Bases, base: usize) -> bool {
    (0..base).all(|b| bases.is_occupied(b))
}

/// The out recorded when the ball is caught on the fly at `pos`, with the
/// runner on third's tag-up read against the throw home ([`read_tag_up`]).
/// `outs` is the count before the catch; `runners_going` (the hit-and-run
/// flag) means the runners are doubled off instead — nobody tags.
pub fn resolve_catch(
    pos: Vec3,
    bases: &Bases,
    outs: u32,
    runners_going: bool,
    field: &FieldSpec,
    rules: &Ruleset,
) -> OutKind {
    if !is_fair(pos, field) {
        return OutKind::FoulPop;
    }
    let dist = Vec2::new(pos.x, pos.z).length();
    let s = field.hit_scale;
    if dist < POP_RADIUS * s {
        return OutKind::Pop;
    }
    let tag = if runners_going {
        TagUp::Hold
    } else {
        read_tag_up(pos, bases, outs, field, rules)
    };
    OutKind::Fly {
        deep: dist >= TAG_UP_MIN_DIST * s,
        tag,
    }
}

/// The runner on third's tag-up on a fly caught at `pos`. He retouches the
/// bag timed to the catch (a running start, no reaction) and races the
/// outfielder's throw home, which still needs the tag put on. He goes when
/// the race is no worse than the send gamble (`send_aggression_secs`) —
/// scoring if he really beats it, thrown out in the gamble's band. With two
/// out the catch ends the inning, so nobody tags. Per docs/BASEBALL.md
/// ("Fewer than two outs, deep fly — tag up").
pub fn read_tag_up(
    pos: Vec3,
    bases: &Bases,
    outs: u32,
    field: &FieldSpec,
    rules: &Ruleset,
) -> TagUp {
    let n = field.base_count();
    if n == 0 || !bases.is_occupied(n - 1) || outs + 1 >= rules.counts.outs_per_half {
        return TagUp::Hold;
    }
    let pace = &rules.pace;
    let leg = field.base_positions.first().map_or(27.43, |b| b.length());
    let runner_at = leg / pace.runner_speed;
    let throw_at = pace.throw_transfer_secs
        + Vec2::new(pos.x, pos.z).length() / pace.throw_speed
        + pace.tag_apply_secs;
    if runner_at <= throw_at + pace.runner_margin_secs {
        TagUp::Scores
    } else if runner_at <= throw_at + pace.runner_margin_secs + pace.send_aggression_secs {
        TagUp::ThrownOut
    } else {
        TagUp::Hold
    }
}

/// One runner aboard reading a hit for a base more than it gives him.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExtraBaseRead {
    /// His origin base (0-indexed).
    pub from: usize,
    /// The extra bag he reads for (0-indexed; `base_count()` = home).
    pub to: usize,
    /// He goes for it (the race is no worse than his send gamble).
    pub go: bool,
    /// He beats a throw to that bag.
    pub beats_throw: bool,
    /// A throw to that bag arrives in time to be worth making.
    pub contested: bool,
}

/// The lead runner's read on a `hit_bases` hit gathered at `pos`,
/// `gather_time` seconds after contact: the most advanced runner the hit
/// doesn't already score reads whether one more base (second to home on a
/// single, first to third, first to home on a double) beats the throw there.
/// He starts from his secondary lead, breaking at the bat with two out and a
/// reaction later otherwise; the throw needs its transfer, flight and the
/// tag. `None` with nobody to read, or on a hit-and-run, whose jump already
/// bought the runners their extra base. The real-game benchmark is MLB's
/// ~41% extra bases taken (docs/BASEBALL.md "Taking the extra base").
#[allow(clippy::too_many_arguments)]
pub fn read_extra_base(
    pos: Vec3,
    gather_time: f32,
    bases: &Bases,
    hit_bases: u32,
    outs: u32,
    runners_going: bool,
    field: &FieldSpec,
    pace: &PaceTuning,
) -> Option<ExtraBaseRead> {
    if runners_going {
        return None;
    }
    let n = field.base_count();
    let hit = hit_bases as usize;
    let from = (0..n)
        .rev()
        .find(|&b| bases.is_occupied(b) && b + hit < n)?;
    let to = from + hit + 1;
    let leg = field.base_positions.first().map_or(27.43, |b| b.length());
    let start = if outs >= 2 { 0.0 } else { pace.reaction_secs };
    let runner_at = start + leg * ((hit + 1) as f32 - pace.secondary_lead_frac) / pace.runner_speed;
    let bag = home_or_base(to, field);
    let throw_at = gather_time
        + pace.throw_transfer_secs
        + Vec2::new(bag.x - pos.x, bag.z - pos.z).length() / pace.throw_speed
        + pace.tag_apply_secs;
    let beats_throw = runner_at <= throw_at + pace.runner_margin_secs;
    Some(ExtraBaseRead {
        from,
        to,
        go: runner_at <= throw_at + pace.runner_margin_secs + pace.send_aggression_secs,
        beats_throw,
        contested: throw_at <= runner_at + pace.throw_contest_secs,
    })
}

/// The batter-vs-throw race once a fair ball is gathered at `pos`,
/// `gather_time` seconds after contact. Infield gathers contest the out at
/// first; deeper gathers concede it (nobody throws a clean outfield single to
/// first) and the batter stretches for every extra base the throw can't beat.
pub fn resolve_gathered(
    pos: Vec3,
    gather_time: f32,
    field: &FieldSpec,
    rules: &Ruleset,
) -> Outcome {
    let leg = field.base_positions.first().map_or(27.43, |b| b.length());
    let runner_at =
        |base: usize| rules.pace.reaction_secs + leg * base as f32 / rules.pace.runner_speed;
    let throw_at = |target: Vec3| {
        gather_time
            + rules.pace.throw_transfer_secs
            + Vec2::new(target.x - pos.x, target.z - pos.z).length() / rules.pace.throw_speed
    };
    let safe = |base: usize| {
        field
            .base_positions
            .get(base - 1)
            .is_some_and(|bp| runner_at(base) <= throw_at(*bp) + rules.pace.runner_margin_secs)
    };

    let from_home = Vec2::new(pos.x, pos.z).length();
    if from_home < INFIELD_GATHER_RADIUS * field.hit_scale && !safe(1) {
        // Beaten to the bag (or, on the front lawn, beaned on the way).
        return Outcome::Out(if rules.counts.peg_outs {
            OutKind::Pegged
        } else {
            OutKind::Ground
        });
    }
    let mut bases = 1;
    while bases < field.base_count() && safe(bases + 1) {
        bases += 1;
    }
    Outcome::Hit(bases as u32)
}

/// The base the defense throws to for the most reasonable out once a fair
/// ball is gathered at `pos`, `gather_time` seconds after contact: the lead
/// *force* out the throw can still beat. When no out is winnable anywhere,
/// the throw goes ahead of the batter to the bag he will end on — first on a
/// single (the conventional play), second on a stand-up double, and so on —
/// so the defense visibly makes the attempt even on a conceded hit.
/// When the lead runner reads the hit for an extra base ([`read_extra_base`])
/// and the throw there is close enough to contest, the throw goes to his bag
/// instead — the play at the plate. `runners_going` marks a hit-and-run
/// jump, which takes the runner forces off the table; `outs` is the count
/// before the play. 0-indexed into `base_positions`; `base_count()` means
/// home plate (bases loaded, force at the plate). Pure choreography
/// guidance — the call itself comes from [`resolve_thrown`].
#[allow(clippy::too_many_arguments)]
pub fn throw_target(
    pos: Vec3,
    gather_time: f32,
    bases: &Bases,
    runners_going: bool,
    outs: u32,
    field: &FieldSpec,
    pace: &PaceTuning,
) -> usize {
    let leg = field.base_positions.first().map_or(27.43, |b| b.length());
    // Every forced runner (batter included) sprints exactly one base from a
    // standing start at contact, so one clock covers them all — minus the
    // jump when the runners broke with the windup.
    let runner_at = forced_runner_at(leg, runners_going, pace);
    let throw_at = |target: Vec3| {
        gather_time
            + pace.throw_transfer_secs
            + Vec2::new(target.x - pos.x, target.z - pos.z).length() / pace.throw_speed
    };
    let base_pos = |b: usize| home_or_base(b, field);

    // Take the biggest force out the throw still beats — only ever an
    // infield play, as in [`resolve_thrown`]. The batter never has the
    // jump, so first base races on the standing-start clock.
    let infield = Vec2::new(pos.x, pos.z).length() < INFIELD_GATHER_RADIUS * field.hit_scale;
    let batter_at = forced_runner_at(leg, false, pace);
    let mut b = lead_force(bases, field);
    loop {
        let clock = if b == 0 { batter_at } else { runner_at };
        if infield && clock > throw_at(base_pos(b)) + pace.runner_margin_secs {
            return b;
        }
        if b == 0 {
            // No out is winnable anywhere: throw ahead of the batter, to
            // the bag he'll finish on (the same walk [`resolve_thrown`]
            // concedes) — the play the crowd expects to see attempted.
            let batter_reaches =
                |base: usize| pace.reaction_secs + leg * base as f32 / pace.runner_speed;
            let safe = |base: usize| {
                field.base_positions.get(base - 1).is_some_and(|bp| {
                    batter_reaches(base) <= throw_at(*bp) + pace.runner_margin_secs
                })
            };
            let mut n = 1;
            while n < field.base_count() && safe(n + 1) {
                n += 1;
            }
            // A runner sent for the extra base draws the throw when it
            // can still get him — the close play at the plate.
            let read = read_extra_base(
                pos,
                gather_time,
                bases,
                n as u32,
                outs,
                runners_going,
                field,
                pace,
            );
            if let Some(read) = read.filter(|r| r.go && r.contested) {
                return read.to;
            }
            return n - 1;
        }
        b -= 1;
    }
}

/// One forced runner's time to the next bag: a standing start at contact,
/// minus the hit-and-run head start when the runners broke with the windup.
fn forced_runner_at(leg: f32, going: bool, pace: &PaceTuning) -> f32 {
    pace.reaction_secs + leg / pace.runner_speed
        - if going {
            pace.hit_and_run_jump_secs
        } else {
            0.0
        }
}

/// World position of base `b`, where `b == base_count()` means home plate.
fn home_or_base(b: usize, field: &FieldSpec) -> Vec3 {
    if b == field.base_count() {
        Vec3::ZERO
    } else {
        field.base_positions[b]
    }
}

/// The lead base of the force chain: the batter forces first, and each
/// consecutively occupied base extends the chain one further (bases loaded
/// forces the runner at the plate, expressed as `base_count()`).
fn lead_force(bases: &Bases, field: &FieldSpec) -> usize {
    let mut lead = 0;
    for b in 0..field.base_count() {
        if lead == b && bases.is_occupied(b) {
            lead = b + 1;
        } else {
            break;
        }
    }
    lead
}

/// The race once the ball-holder throws to `target` (0-indexed;
/// `base_count()` = home plate), `throw_time` seconds after contact.
///
/// An out needs a live force at the target, an infield-range gather, and the
/// throw beating the forced runner's one-base sprint (minus the jump on a
/// hit-and-run). A force out at first retires the batter plainly; at any
/// later bag the relay to first races the batter — beat him and it's the
/// classic [`Outcome::DoublePlay`], lose and it's a
/// [`Outcome::FieldersChoice`]. No out at all concedes, and the batter takes
/// every base the throw can't beat — the same walk as [`resolve_gathered`],
/// whose behaviour this reproduces exactly for a prompt neutral throw to
/// first with empty bases. `runner_call` is the batting side's say: `Send`
/// stretches for one extra base against a softer race (cut down trying if it
/// fails), `Hold` pulls up a base early. On a clean hit the lead runner
/// aboard reads for one base more ([`read_extra_base`]); a throw to that bag
/// races him ([`Outcome::ExtraBase`]), a throw anywhere else concedes it.
/// `outs` is the count before the play.
#[allow(clippy::too_many_arguments)]
pub fn resolve_thrown(
    pos: Vec3,
    throw_time: f32,
    target: usize,
    bases: &Bases,
    runners_going: bool,
    outs: u32,
    runner_call: RunnerCall,
    field: &FieldSpec,
    rules: &Ruleset,
) -> Outcome {
    let leg = field.base_positions.first().map_or(27.43, |b| b.length());
    let throw_at = |p: Vec3| {
        throw_time
            + rules.pace.throw_transfer_secs
            + Vec2::new(p.x - pos.x, p.z - pos.z).length() / rules.pace.throw_speed
    };
    let base_pos = |b: usize| home_or_base(b, field);
    let flat_dist = |a: Vec3, b: Vec3| Vec2::new(a.x - b.x, a.z - b.z).length();

    let from_home = Vec2::new(pos.x, pos.z).length();
    let infield = from_home < INFIELD_GATHER_RADIUS * field.hit_scale;
    let runner_clock = if target == 0 {
        // The batter is the forced runner at first and never has the jump.
        forced_runner_at(leg, false, &rules.pace)
    } else {
        forced_runner_at(leg, runners_going, &rules.pace)
    };
    if target <= lead_force(bases, field)
        && infield
        && runner_clock > throw_at(base_pos(target)) + rules.pace.runner_margin_secs
    {
        if target == 0 {
            // The sure out at first: just the batter.
            return Outcome::Out(if rules.counts.peg_outs {
                OutKind::Pegged
            } else {
                OutKind::Ground
            });
        }
        // Forced runner retired; the relay to first races the batter.
        let relay_arrival = throw_at(base_pos(target))
            + rules.pace.relay_transfer_secs
            + flat_dist(base_pos(target), base_pos(0)) / rules.pace.throw_speed;
        if forced_runner_at(leg, false, &rules.pace) > relay_arrival + rules.pace.runner_margin_secs
        {
            return Outcome::DoublePlay;
        }
        return Outcome::FieldersChoice { out_base: target };
    }

    // No out on the throw: the batter takes every base it can't beat.
    let batter_at =
        |base: usize| rules.pace.reaction_secs + leg * base as f32 / rules.pace.runner_speed;
    let safe = |base: usize| {
        field
            .base_positions
            .get(base - 1)
            .is_some_and(|bp| batter_at(base) <= throw_at(*bp) + rules.pace.runner_margin_secs)
    };
    let mut n = 1;
    while n < field.base_count() && safe(n + 1) {
        n += 1;
    }
    let hit = match runner_call {
        // Sent for one more: a softer race (the ball is usually elsewhere),
        // but getting it wrong is an out on the bases.
        RunnerCall::Send if n < field.base_count() => {
            let stretch_to = field.base_positions[n];
            if batter_at(n + 1)
                <= throw_at(stretch_to)
                    + rules.pace.runner_margin_secs
                    + rules.pace.stretch_grace_secs
            {
                n as u32 + 1
            } else {
                return Outcome::Out(OutKind::Stretching { advanced: n as u32 });
            }
        }
        // Held up: bank the sure bases (never less than the single).
        RunnerCall::Hold => (n as u32 - 1).max(1),
        _ => n as u32,
    };
    // The lead runner's read: he goes on what he sees at the throw, and
    // only a throw to his bag can get him.
    let read = read_extra_base(
        pos,
        throw_time,
        bases,
        hit,
        outs,
        runners_going,
        field,
        &rules.pace,
    );
    match read.filter(|r| r.go) {
        Some(read) => Outcome::ExtraBase {
            hit,
            runner: read.from,
            safe: target != read.to || read.beats_throw,
        },
        None => Outcome::Hit(hit),
    }
}

/// The batting side's runner call from its held aim during a live ball:
/// stick down sends the batter for the extra base (matching the send-the-
/// runner steal convention), stick up holds him a base early.
pub fn runner_call_from_aim(aim: Vec2) -> RunnerCall {
    if aim.y < -0.7 {
        RunnerCall::Send
    } else if aim.y > 0.7 {
        RunnerCall::Hold
    } else {
        RunnerCall::Neutral
    }
}

/// The base a held defensive aim selects for a manual throw — the base
/// (home = `base_count()`) whose on-screen direction from the plate best
/// matches the stick. Screen right is world −X and screen up is +Z (the
/// behind-home camera), so the diamond reads naturally: right = first,
/// up = second on a three-base diamond, left = third, down = home. `None`
/// when the stick is too centred or points nowhere near a base.
pub fn aimed_base(aim: Vec2, field: &FieldSpec) -> Option<usize> {
    if aim.length() < AIM_DEADZONE {
        return None;
    }
    let dir = Vec2::new(super::aim_to_world_x(aim.x), aim.y).normalize(); // aim → world (x, z)
    let mut best: Option<(usize, f32)> = None;
    let mut consider = |b: usize, world: Vec2| {
        let Some(bd) = world.try_normalize() else {
            return;
        };
        let dot = dir.dot(bd);
        if dot > best.map_or(0.3, |(_, d)| d) {
            best = Some((b, dot));
        }
    };
    for (b, p) in field.base_positions.iter().enumerate() {
        consider(b, Vec2::new(p.x, p.z));
    }
    consider(field.base_count(), Vec2::new(0.0, -1.0)); // home reads as "down"
    best.map(|(b, _)| b)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "resolve.test.rs"]
mod tests;
