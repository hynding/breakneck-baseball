//! Unit tests for [`super`] — the scenario module.

use super::*;
use crate::game::flow::Phase;
use crate::game::variant::VariantId;

#[test]
fn canonical_aim_round_trips_every_pitch() {
    use crate::game::rules::PitchKind::*;
    for kind in [Fastball, Curveball, Changeup, Slider, Sinker] {
        assert_eq!(PitchKind::from_aim(kind.canonical_aim()), kind);
    }
}

#[test]
fn presets_are_legal_for_standard_rules() {
    for s in presets() {
        assert!(s.balls < 4 && s.strikes < 3 && s.outs < 3, "{}", s.name);
        assert!(s.bases.len() <= 4, "{}", s.name);
        assert!(s.inning >= 1, "{}", s.name);
    }
}

#[test]
fn apply_rewrites_the_world_and_fires_the_event() {
    let mut world = test_world(); // helper below
    let s = presets()
        .into_iter()
        .find(|s| s.name == PRESET_LOADED)
        .unwrap();
    apply_to_world(&mut world, &s).unwrap();
    let score = world.resource::<ScoreBoard>();
    assert_eq!((score.balls, score.strikes, score.outs), (3, 2, 2));
    let bases = world.resource::<Bases>();
    assert!(bases.is_occupied(0) && bases.is_occupied(1) && bases.is_occupied(2));
    assert!(!world.resource::<Events<ScenarioAppliedEvent>>().is_empty());
}

#[test]
fn apply_is_refused_while_the_ball_is_live() {
    let mut world = test_world();
    world
        .resource_mut::<Play>()
        .force_phase_for_test(Phase::InPlay);
    let s = &presets()[0];
    assert!(apply_to_world(&mut world, s).is_err());
}

fn test_world() -> World {
    let mut world = World::new();
    world.insert_resource(ScoreBoard {
        inning: 1,
        top_of_inning: true,
        ..Default::default()
    });
    world.insert_resource(Bases::default());
    world.insert_resource(BattingOrder::default());
    world.insert_resource(Play::default());
    world.insert_resource(VariantId::Standard.rules());
    world.insert_resource(VariantId::Standard.field());
    world.init_resource::<PitchOverride>();
    world.init_resource::<Events<ScenarioAppliedEvent>>();
    world
}
