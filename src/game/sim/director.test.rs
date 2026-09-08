//! Unit tests for [`super`] — the director module.

use super::*;

fn ctx() -> Ctx {
    Ctx {
        phase: Phase::Pitch,
        on_offense: true,
        in_steal_window: false,
        phase_elapsed: 0.5,
        dt_ms: Some(-20.0),
        gathered: false,
    }
}

#[test]
fn plate_eta_fires_once_the_error_reaches_the_threshold() {
    let cond = Condition::PlateEta { early_ms: 10.0 };
    // dt −20 ms: the ball is still 20 ms out — a 10 ms trigger waits.
    assert!(!eval(&cond, &ctx()));
    let mut c = ctx();
    c.dt_ms = Some(-10.0);
    assert!(eval(&cond, &c));
    c.dt_ms = Some(5.0);
    assert!(eval(&cond, &c));
    // No live ball: never fires.
    c.dt_ms = None;
    assert!(!eval(&cond, &c));
}

#[test]
fn boolean_combinators_compose() {
    let c = ctx();
    let both = Condition::All(vec![
        Condition::OnOffense,
        Condition::Phase(ScriptPhase::Pitch),
    ]);
    assert!(eval(&both, &c));
    let neither = Condition::All(vec![
        Condition::OnDefense,
        Condition::Phase(ScriptPhase::Pitch),
    ]);
    assert!(!eval(&neither, &c));
    assert!(eval(&Condition::Not(Box::new(neither.clone())), &c));
    assert!(eval(&Condition::Any(vec![neither, both]), &c));
}

#[test]
fn every_builtin_script_parses() {
    for (name, _) in BUILTIN_SCRIPTS {
        let s = script(name).expect("registered");
        assert!(
            !s.rules.is_empty() || !s.steps.is_empty(),
            "{name} is empty"
        );
    }
}

#[test]
fn base_aims_match_the_stick_convention() {
    // Same mapping rules::aimed_base reads back (screen right = first).
    use crate::game::rules::aimed_base;
    use crate::game::variant::VariantId;
    let f = VariantId::Standard.field();
    assert_eq!(aimed_base(BaseSel::First.aim(), &f), Some(0));
    assert_eq!(aimed_base(BaseSel::Second.aim(), &f), Some(1));
    assert_eq!(aimed_base(BaseSel::Third.aim(), &f), Some(2));
    assert_eq!(aimed_base(BaseSel::Home.aim(), &f), Some(f.base_count()));
}
