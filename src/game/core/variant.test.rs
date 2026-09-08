//! Unit tests for [`super`] — the variant module.

use super::*;
use crate::game::field::BASE_DISTANCE;

#[test]
fn pace_defaults_match_legacy_constants() {
    let p = PaceTuning::default();
    assert_eq!(p.pitch_speed_scale, 1.0);
    assert_eq!(p.runner_speed, 7.5);
    assert_eq!(p.fielder_speed, 7.0);
    assert_eq!(p.reaction_secs, 0.35);
    assert_eq!(p.throw_speed, 27.0);
    assert_eq!(p.throw_transfer_secs, 0.5);
    assert_eq!(p.relay_transfer_secs, 0.3);
    assert_eq!(p.hit_and_run_jump_secs, 1.6);
    assert_eq!(p.stretch_grace_secs, 0.9);
    assert_eq!(p.runner_margin_secs, 0.35);
    assert_eq!(p.result_secs, 1.2);
    assert_eq!(p.pickoff_cooldown_secs, 0.9);
    assert_eq!(p.auto_throw_delay_secs, 0.6);
}

#[test]
fn standard_matches_regulation_baseball() {
    let (r, f) = (VariantId::Standard.rules(), VariantId::Standard.field());
    assert_eq!(
        (
            r.counts.balls_per_walk,
            r.counts.strikes_per_out,
            r.counts.outs_per_half,
            r.counts.innings
        ),
        (4, 3, 3, 9)
    );
    assert!(!r.counts.peg_outs);
    assert_eq!(f.base_count(), 3);
    assert_eq!(f.pitch_distance, 18.44);
    assert_eq!(f.scenery, Scenery::Stadium);
    // First base is 90 ft (27.43 m) from home, and every base path is 90 ft.
    assert!((f.base_positions[0].length() - BASE_DISTANCE).abs() < 0.01);
    for pair in f.base_positions.windows(2) {
        assert!(((pair[1] - pair[0]).length() - BASE_DISTANCE).abs() < 0.01);
    }
    // Second base straight out along +Z at the full diamond diagonal
    // (127 ft 3 3/8 in ≈ 38.79 m).
    assert!((f.base_positions[1] - Vec3::new(0.0, 0.0, 38.79)).length() < 0.01);
    // Screen convention: the behind-home camera renders −X on screen
    // right, so first base is at −X and third at +X.
    assert!(f.base_positions[0].x < 0.0 && f.base_positions[2].x > 0.0);
}

#[test]
fn front_yard_is_four_bases_with_pegging() {
    let (r, f) = (VariantId::FrontYard.rules(), VariantId::FrontYard.field());
    assert!(r.counts.peg_outs);
    assert_eq!(r.counts.innings, 3);
    assert_eq!(f.base_count(), 4);
    assert_eq!(f.fielder_positions.len(), 3); // + the pitcher = 4-player team
    assert!(f.peg_radius > 0.0);
    assert_eq!(f.scenery, Scenery::FrontYard);
}

#[test]
fn innings_options_cycle_and_wrap() {
    assert_eq!(next_innings(1), 3);
    assert_eq!(next_innings(3), 6);
    assert_eq!(next_innings(6), 9);
    assert_eq!(next_innings(9), 1);
}

#[test]
fn unknown_innings_value_restarts_the_cycle() {
    assert_eq!(next_innings(2), 1);
}

#[test]
fn variant_cycle_visits_all_and_wraps() {
    assert_eq!(VariantId::Standard.next(), VariantId::FrontYard);
    assert_eq!(VariantId::FrontYard.next(), VariantId::Standard);
}

#[test]
fn duel_framing_sits_behind_home_looking_out() {
    for id in [VariantId::Standard, VariantId::FrontYard] {
        let f = id.field();
        assert!(f.duel_eye.z < 0.0 && f.duel_target.z > 0.0);
        assert!(
            f.duel_eye.z > f.broadcast_eye.z,
            "duel eye must be closer to the plate than the wide framing"
        );
        // Catcher's-eye height: the rig crouches to about 1.44 m (see
        // the comment on `duel_eye` above), well below both a standing
        // eye line and the old high broadcast-style duel camera
        // (y=2.3/2.2) — this guards against a regression back to that.
        assert!(
            f.duel_eye.y > 0.9 && f.duel_eye.y < 1.6,
            "duel eye should sit at crouched-catcher eye height, not a standing/overhead one"
        );
    }
}

#[test]
fn behind_pitcher_framing_looks_back_at_the_plate_from_the_mound() {
    for id in [VariantId::Standard, VariantId::FrontYard] {
        let f = id.field();
        // The eye sits out past the rubber, looking back down the pipe
        // toward home — the mirror image of the duel/batting views.
        assert!(
            f.behind_pitcher_eye.z > f.pitch_distance,
            "behind-pitcher eye must stand behind the rubber, not in front of it"
        );
        assert!(
            f.behind_pitcher_target.z <= 0.0,
            "behind-pitcher target must look toward (or at) the plate"
        );
        assert!(f.behind_pitcher_eye.z > f.behind_pitcher_target.z);
    }
}

#[test]
fn diff_literal_is_empty_at_defaults() {
    assert_eq!(
        VariantId::Standard
            .rules()
            .diff_literal(VariantId::Standard),
        ""
    );
}

#[test]
fn diff_literal_lists_only_changed_fields() {
    let mut r = VariantId::Standard.rules();
    r.batting.perfect_ms = 48.0;
    r.pace.runner_speed = 8.0;
    let s = r.diff_literal(VariantId::Standard);
    assert!(s.contains("batting.perfect_ms: 48.0,"));
    assert!(s.contains("pace.runner_speed: 8.0,"));
    assert!(!s.contains("solid_ms"));
    assert!(s.starts_with("// VariantId::Standard overrides:"));
}

/// `diff_literal`'s `diff!` field list is hand-maintained and can
/// silently miss a field added to `Ruleset` (or a sub-struct) in the
/// future. Guard it with reflection instead of a second hand-maintained
/// list: flip every leaf field `Ruleset` reflects away from its default,
/// and require `diff_literal` to emit exactly that many lines. A field
/// missing a `diff!` arm shows up as a line-count mismatch here.
#[test]
fn diff_literal_covers_every_reflected_field() {
    use bevy::reflect::{PartialReflect, ReflectMut, ReflectRef};

    fn count_leaf_fields(value: &dyn PartialReflect) -> usize {
        match value.reflect_ref() {
            ReflectRef::Struct(s) => (0..s.field_len())
                .map(|i| count_leaf_fields(s.field_at(i).unwrap()))
                .sum(),
            _ => 1,
        }
    }

    fn perturb_every_field(value: &mut dyn PartialReflect) {
        match value.reflect_mut() {
            ReflectMut::Struct(s) => {
                for i in 0..s.field_len() {
                    perturb_every_field(s.field_at_mut(i).unwrap());
                }
            }
            _ => {
                if let Some(v) = value.try_downcast_mut::<f32>() {
                    *v += 1.0;
                } else if let Some(v) = value.try_downcast_mut::<u32>() {
                    *v += 1;
                } else if let Some(v) = value.try_downcast_mut::<bool>() {
                    *v = !*v;
                } else {
                    panic!(
                        "diff_literal completeness test: unhandled leaf field type on \
                         Ruleset; add a case to perturb_every_field (and a matching \
                         diff! arm in diff_literal)"
                    );
                }
            }
        }
    }

    let expected = count_leaf_fields(VariantId::Standard.rules().as_partial_reflect());

    let mut all_changed = VariantId::Standard.rules();
    perturb_every_field(all_changed.as_partial_reflect_mut());

    let diff = all_changed.diff_literal(VariantId::Standard);
    let emitted = diff.lines().filter(|l| !l.starts_with("//")).count();

    assert_eq!(
        emitted, expected,
        "diff_literal emitted {emitted} line(s) but Ruleset reflects {expected} leaf \
         field(s) — a field is missing a diff! arm in diff_literal"
    );
}

#[test]
fn batting_zoom_framing_sits_behind_home_looking_toward_the_pitcher() {
    for id in [VariantId::Standard, VariantId::FrontYard] {
        let f = id.field();
        // Same plate-corridor orientation as the duel view: eye behind
        // home (z<0), target out toward the mound (z>0).
        assert!(f.batting_zoom_eye.z < 0.0 && f.batting_zoom_target.z > 0.0);
        // "Beside" the batter's box, not dead centre like the duel/pitcher
        // views — this is what makes it a distinct framing.
        assert!(f.batting_zoom_eye.x.abs() > 0.1);
    }
}
