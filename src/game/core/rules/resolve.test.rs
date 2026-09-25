//! Unit tests for [`super`] — the resolve module.

use super::super::best_catcher;
use super::super::test_support::*;
use super::*;
use crate::game::variant::VariantId;

#[test]
fn force_chain_extends_from_home() {
    // Batter on first is always forced; the chain reaches only as far as
    // the runners are contiguous from first.
    assert!(is_forced(&with(&[0]), 0));
    assert!(is_forced(&with(&[0, 1]), 1));
    assert!(!is_forced(&with(&[1]), 1)); // gap at first: runner on second free
    assert!(!is_forced(&with(&[0, 2]), 2)); // gap at second: runner on third free
    assert!(is_forced(&loaded(), 2)); // bases loaded: everyone forced
}

// ── Live-play races ───────────────────────────────────────────────────────

#[test]
fn routine_fly_gets_run_down() {
    // A can-of-corn to shallow centre hangs ~3 s; the middle infield
    // reaches it with time to spare.
    let f = std_field();
    assert!(
        best_catcher(
            &f.fielder_positions,
            Vec3::new(0.0, 0.0, 44.0),
            3.0,
            &PaceTuning::default()
        )
        .is_some()
    );
}

#[test]
fn sinking_liner_falls_in() {
    // A liner dying at 55 m hangs ~1.5 s: nobody can get there.
    let f = std_field();
    assert!(
        best_catcher(
            &f.fielder_positions,
            Vec3::new(0.0, 0.0, 55.0),
            1.5,
            &PaceTuning::default()
        )
        .is_none()
    );
}

#[test]
fn catches_map_to_pop_fly_and_foul_pop() {
    let f = std_field();
    assert_eq!(resolve_catch(Vec3::new(0.0, 0.0, 12.0), &f), OutKind::Pop);
    assert_eq!(
        resolve_catch(Vec3::new(0.0, 0.0, 50.0), &f),
        OutKind::Fly { deep: false }
    );
    assert_eq!(
        resolve_catch(Vec3::new(0.0, 0.0, 80.0), &f),
        OutKind::Fly { deep: true }
    );
    assert_eq!(
        resolve_catch(Vec3::new(-30.0, 0.0, 10.0), &f),
        OutKind::FoulPop
    );
}

#[test]
fn quick_infield_gather_beats_the_batter() {
    assert_eq!(
        resolve_gathered(Vec3::new(0.0, 0.0, 7.0), 1.2, &std_field(), &std_rules()),
        Outcome::Out(OutKind::Ground)
    );
}

#[test]
fn slow_infield_gather_is_an_infield_single() {
    assert_eq!(
        resolve_gathered(Vec3::new(0.0, 0.0, 26.0), 3.0, &std_field(), &std_rules()),
        Outcome::Hit(1)
    );
}

#[test]
fn shallow_outfield_gather_concedes_a_single() {
    assert_eq!(
        resolve_gathered(Vec3::new(0.0, 0.0, 35.0), 2.6, &std_field(), &std_rules()),
        Outcome::Hit(1)
    );
}

#[test]
fn deep_gap_gather_is_a_double() {
    assert_eq!(
        resolve_gathered(Vec3::new(50.0, 0.0, 95.0), 5.8, &std_field(), &std_rules()),
        Outcome::Hit(2)
    );
}

#[test]
fn ball_to_the_wall_is_a_triple() {
    assert_eq!(
        resolve_gathered(Vec3::new(0.0, 0.0, 120.0), 7.5, &std_field(), &std_rules()),
        Outcome::Hit(3)
    );
}

// ── Throw-target selection ────────────────────────────────────────────────

#[test]
fn bases_empty_throws_to_first() {
    assert_eq!(
        throw_target(
            Vec3::new(0.0, 0.0, 7.0),
            1.2,
            &empty(),
            false,
            &std_field(),
            &pace()
        ),
        0
    );
}

#[test]
fn runner_on_first_takes_the_force_at_second() {
    // Gathered near second with a runner on first: the lead force is on
    // and the short throw beats the runner.
    assert_eq!(
        throw_target(
            Vec3::new(0.0, 0.0, 30.0),
            1.2,
            &with(&[0]),
            false,
            &std_field(),
            &pace()
        ),
        1
    );
}

#[test]
fn runner_on_second_is_not_forced() {
    // No runner on first, so second base is not a force — take first.
    assert_eq!(
        throw_target(
            Vec3::new(0.0, 0.0, 30.0),
            1.2,
            &with(&[1]),
            false,
            &std_field(),
            &pace()
        ),
        0
    );
}

#[test]
fn bases_loaded_forces_the_play_at_home() {
    let field = std_field();
    assert_eq!(
        throw_target(
            Vec3::new(-5.0, 0.0, 10.0),
            0.8,
            &loaded(),
            false,
            &field,
            &pace()
        ),
        field.base_count()
    );
}

#[test]
fn late_gather_falls_back_to_first() {
    // Gathered so late that no throw beats any runner: still play to
    // first — the conventional, "most reasonable" attempt.
    assert_eq!(
        throw_target(
            Vec3::new(0.0, 0.0, 60.0),
            6.0,
            &with(&[0]),
            false,
            &std_field(),
            &pace()
        ),
        0
    );
}

#[test]
fn outfield_double_draws_the_throw_to_second() {
    // A clean gap double: no force is winnable, so the throw goes ahead
    // of the batter to the bag he's stretching for.
    assert_eq!(
        throw_target(
            Vec3::new(0.0, 0.0, 110.0),
            6.5,
            &empty(),
            false,
            &std_field(),
            &pace()
        ),
        1
    );
}

#[test]
fn hit_and_run_jump_takes_the_force_off_the_table() {
    // A mid-infield gather that forces the standing-start runner at
    // second — but with the windup jump the throw can't win there, so
    // the smart throw goes to first instead.
    let pos = Vec3::new(0.0, 0.0, 20.0);
    assert_eq!(
        throw_target(pos, 1.2, &with(&[0]), false, &std_field(), &pace()),
        1
    );
    assert_eq!(
        throw_target(pos, 1.2, &with(&[0]), true, &std_field(), &pace()),
        0
    );
}

// ── Thrown-ball resolution ────────────────────────────────────────────────

fn neutral(pos: Vec3, t: f32, target: usize, bases: &Bases, f: &FieldSpec, r: &Ruleset) -> Outcome {
    resolve_thrown(pos, t, target, bases, false, RunnerCall::Neutral, f, r)
}

#[test]
fn prompt_throw_to_first_matches_resolve_gathered() {
    let (f, r) = (std_field(), std_rules());
    for (pos, t) in [
        (Vec3::new(0.0, 0.0, 7.0), 1.2),
        (Vec3::new(0.0, 0.0, 26.0), 3.0),
        (Vec3::new(0.0, 0.0, 35.0), 2.6),
        (Vec3::new(50.0, 0.0, 95.0), 5.8),
    ] {
        assert_eq!(
            neutral(pos, t, 0, &empty(), &f, &r),
            resolve_gathered(pos, t, &f, &r),
            "at {pos:?} t={t}"
        );
    }
}

#[test]
fn quick_force_at_second_turns_two() {
    // Sharp play near the bag: the force arrives early and the relay to
    // first still beats the batter — the classic double play.
    assert_eq!(
        neutral(
            Vec3::new(0.0, 0.0, 28.0),
            1.2,
            1,
            &with(&[0]),
            &std_field(),
            &std_rules()
        ),
        Outcome::DoublePlay
    );
}

#[test]
fn slow_force_at_second_is_a_fielders_choice() {
    // A weak roller near the plate: the force barely beats the runner,
    // and the long relay cannot double the batter.
    assert_eq!(
        neutral(
            Vec3::new(0.0, 0.0, 5.0),
            1.8,
            1,
            &with(&[0]),
            &std_field(),
            &std_rules()
        ),
        Outcome::FieldersChoice { out_base: 1 }
    );
}

#[test]
fn throw_behind_the_play_concedes_the_single() {
    // Third base is not a force with only a runner on first: the throw
    // there gets nobody, and the batter has the single.
    assert_eq!(
        neutral(
            Vec3::new(0.0, 0.0, 28.0),
            1.2,
            2,
            &with(&[0]),
            &std_field(),
            &std_rules()
        ),
        Outcome::Hit(1)
    );
}

#[test]
fn bases_loaded_quick_throw_home_turns_two() {
    // The 2-3 special: force at the plate, relay to first in time.
    let field = std_field();
    assert_eq!(
        neutral(
            Vec3::new(-5.0, 0.0, 10.0),
            0.8,
            field.base_count(),
            &loaded(),
            &field,
            &std_rules()
        ),
        Outcome::DoublePlay
    );
}

#[test]
fn outfield_gather_cannot_force_anyone() {
    // Even aimed at a live force, a deep gather concedes: the out at any
    // bag is only contested from infield range.
    assert_eq!(
        neutral(
            Vec3::new(0.0, 0.0, 60.0),
            3.5,
            1,
            &with(&[0]),
            &std_field(),
            &std_rules()
        ),
        Outcome::Hit(1)
    );
}

#[test]
fn hit_and_run_beats_the_force_at_second() {
    // The jump the runner got at the windup makes the force unwinnable;
    // the play falls through to a plain single.
    assert_eq!(
        resolve_thrown(
            Vec3::new(0.0, 0.0, 28.0),
            1.2,
            1,
            &with(&[0]),
            true,
            RunnerCall::Neutral,
            &std_field(),
            &std_rules()
        ),
        Outcome::Hit(1)
    );
}

#[test]
fn sent_batter_is_cut_down_stretching() {
    // A shallow-outfield single with the batter sent: the extra base is
    // not there, and the batter is out on the bases with the single's
    // advancement preserved for the other runners.
    assert_eq!(
        resolve_thrown(
            Vec3::new(0.0, 0.0, 60.0),
            3.5,
            0,
            &empty(),
            false,
            RunnerCall::Send,
            &std_field(),
            &std_rules()
        ),
        Outcome::Out(OutKind::Stretching { advanced: 1 })
    );
}

#[test]
fn sent_batter_stretches_a_double_into_a_triple() {
    // Deep in the gap the softer stretch race is winnable.
    assert_eq!(
        resolve_thrown(
            Vec3::new(0.0, 0.0, 110.0),
            6.5,
            0,
            &empty(),
            false,
            RunnerCall::Send,
            &std_field(),
            &std_rules()
        ),
        Outcome::Hit(3)
    );
}

#[test]
fn held_batter_banks_the_single() {
    // The same deep ball played safe stops a base short of the walk.
    let neutral_bases = match resolve_thrown(
        Vec3::new(0.0, 0.0, 110.0),
        6.5,
        0,
        &empty(),
        false,
        RunnerCall::Neutral,
        &std_field(),
        &std_rules(),
    ) {
        Outcome::Hit(n) => n,
        other => panic!("expected a hit, got {other:?}"),
    };
    assert_eq!(
        resolve_thrown(
            Vec3::new(0.0, 0.0, 110.0),
            6.5,
            0,
            &empty(),
            false,
            RunnerCall::Hold,
            &std_field(),
            &std_rules()
        ),
        Outcome::Hit((neutral_bases - 1).max(1))
    );
}

// ── Aimed-base selection ──────────────────────────────────────────────────

#[test]
fn aim_maps_the_diamond_to_the_stick() {
    let f = std_field();
    // Screen right = first, up = second, left = third, down = home.
    assert_eq!(aimed_base(Vec2::new(1.0, 0.0), &f), Some(0));
    assert_eq!(aimed_base(Vec2::new(0.0, 1.0), &f), Some(1));
    assert_eq!(aimed_base(Vec2::new(-1.0, 0.0), &f), Some(2));
    assert_eq!(aimed_base(Vec2::new(0.0, -1.0), &f), Some(f.base_count()));
}

#[test]
fn centred_stick_selects_nothing() {
    assert_eq!(aimed_base(Vec2::new(0.2, 0.1), &std_field()), None);
}

// ── Front-yard live play ──────────────────────────────────────────────────

fn yard() -> (FieldSpec, Ruleset) {
    (VariantId::FrontYard.field(), VariantId::FrontYard.rules())
}

#[test]
fn front_yard_infield_out_is_a_peg() {
    let (f, r) = yard();
    assert_eq!(
        resolve_gathered(Vec3::new(0.0, 0.0, 4.0), 0.4, &f, &r),
        Outcome::Out(OutKind::Pegged)
    );
}
