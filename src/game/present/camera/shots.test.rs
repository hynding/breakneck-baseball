//! Tests for [`super`]: the shot list (TODO 102), the play cams, and
//! moving between shots.

use super::super::DUEL_REFERENCE_ASPECT;
use super::super::framing::{framed_ndc_x, framed_ndc_y};
use super::*;
use crate::game::variant::VariantId;

fn cues(phase: Phase) -> ShotCues {
    ShotCues {
        phase,
        since_contact: 10.0,
        home_run: false,
        walk_off: false,
        pitch_gloved: false,
        view: DuelView::BattingZoom,
        walkup: false,
        cutaway: false,
        strikeout: false,
        thrown_base: None,
        grounder: false,
        runner_cam: None,
        descending_to: None,
        duel_throw_base: None,
        sequence: None,
    }
}

/// A pickoff in the duel and the catcher's throw in the result pause both
/// cut to the bag the ball is going to — over the duel, the gloved-pitch
/// hold, even strike three's reaction cam (the throw is the play).
#[test]
fn a_running_game_throw_takes_the_base_cam() {
    let mut duel = cues(Phase::PrePitch);
    duel.duel_throw_base = Some(0);
    assert_eq!(pick_shot(&duel, None), Shot::BaseCam(0));
    let mut steal = cues(Phase::Result);
    steal.pitch_gloved = true;
    steal.strikeout = true;
    steal.duel_throw_base = Some(1);
    assert_eq!(pick_shot(&steal, None), Shot::BaseCam(1));
    steal.duel_throw_base = None;
    assert_eq!(pick_shot(&steal, None), Shot::ReactionCam);
}

/// Strike three's pause takes the reaction cam even though the pitch was
/// gloved; any other gloved pitch stays at the plate.
#[test]
fn a_strikeout_takes_the_reaction_cam() {
    let mut c = cues(Phase::Result);
    c.pitch_gloved = true;
    assert_eq!(pick_shot(&c, None), Shot::Duel(DuelView::BattingZoom));
    c.strikeout = true;
    assert_eq!(pick_shot(&c, None), Shot::ReactionCam);
}

/// The reaction cam keeps the batter, the catcher's spot, and the plate
/// umpire's spot all in frame.
#[test]
fn reaction_cam_frames_batter_catcher_and_umpire() {
    use super::super::framing::{framed_ndc_x, framed_ndc_y};
    use crate::game::player::{BATTER_STAND_X, RIG_HEIGHT_M};
    let feet = Vec3::new(BATTER_STAND_X, 0.0, 0.0);
    let head = feet + Vec3::Y * RIG_HEIGHT_M;
    let catcher = Vec3::new(0.0, 0.8, -1.5);
    let umpire = Vec3::new(0.0, 1.4, -3.0);
    for p in [feet, head, catcher, umpire] {
        let x = framed_ndc_x(
            REACTION_EYE,
            REACTION_TARGET,
            REACTION_FOV,
            DUEL_REFERENCE_ASPECT,
            p,
        );
        let y = framed_ndc_y(REACTION_EYE, REACTION_TARGET, REACTION_FOV, p);
        assert!(
            x.abs() <= 0.9 && y.abs() <= 0.95,
            "{p} out of the reaction frame at ({x:.2}, {y:.2})"
        );
    }
}

/// The walk-up hold takes its own shot, and only in PrePitch.
#[test]
fn the_walk_up_hold_takes_the_walk_up_shot() {
    let mut c = cues(Phase::PrePitch);
    c.walkup = true;
    assert_eq!(pick_shot(&c, None), Shot::WalkUp);
    c.walkup = false;
    assert_eq!(pick_shot(&c, None), Shot::Duel(DuelView::BattingZoom));
}

/// The walk-up shot frames the whole batter in his box and keeps the
/// catcher's spot in frame behind him — it is the one plate shot where the
/// plate rigs are meant to be seen.
#[test]
fn walk_up_shot_frames_the_batter_and_the_catcher() {
    use super::super::framing::{framed_ndc_x, framed_ndc_y};
    use crate::game::player::{BATTER_STAND_X, RIG_HEIGHT_M};
    let feet = Vec3::new(BATTER_STAND_X, 0.0, 0.0);
    let head = feet + Vec3::Y * RIG_HEIGHT_M;
    let catcher = Vec3::new(0.0, 0.8, -1.5);
    for p in [feet, head, catcher] {
        let x = framed_ndc_x(
            WALKUP_EYE,
            WALKUP_TARGET,
            WALKUP_FOV,
            DUEL_REFERENCE_ASPECT,
            p,
        );
        let y = framed_ndc_y(WALKUP_EYE, WALKUP_TARGET, WALKUP_FOV, p);
        assert!(
            x.abs() <= 0.9 && y.abs() <= 0.95,
            "{p} out of the walk-up frame at ({x:.2}, {y:.2})"
        );
    }
}

/// The duel phases always want the duel framing of the active view — and a
/// different view is a different shot (a V press cuts, never glides).
#[test]
fn duel_phases_want_the_active_view() {
    for phase in [Phase::PrePitch, Phase::WindUp, Phase::Pitch] {
        let mut c = cues(phase);
        assert_eq!(pick_shot(&c, None), Shot::Duel(DuelView::BattingZoom));
        c.view = DuelView::CatcherPov;
        assert_eq!(pick_shot(&c, None), Shot::Duel(DuelView::CatcherPov));
    }
}

/// Fresh contact holds the plate for `BALL_FOLLOW_DELAY`, then cuts to the
/// ball; a chaser under a descending fly takes the fielder cam; a throw
/// takes the base cam over everything else.
#[test]
fn in_play_cuts_plate_then_ball_then_fielder_then_base() {
    let mut c = cues(Phase::InPlay);
    c.since_contact = BALL_FOLLOW_DELAY * 0.5;
    assert_eq!(pick_shot(&c, None), Shot::Duel(DuelView::BattingZoom));
    c.since_contact = BALL_FOLLOW_DELAY + 0.01;
    assert_eq!(pick_shot(&c, None), Shot::BallFollow);
    let fielder = Entity::from_raw_u32(7).unwrap();
    c.descending_to = Some(fielder);
    assert_eq!(pick_shot(&c, None), Shot::FielderCam(fielder));
    c.thrown_base = Some(1);
    assert_eq!(pick_shot(&c, None), Shot::BaseCam(1));
}

/// A throw in from the outfield takes the runner cam ahead of the base cam
/// while it is wanted; the plate hold and a home run still come first.
#[test]
fn the_runner_cam_comes_before_the_base_cam() {
    let runner = Entity::from_raw_u32(11).unwrap();
    let mut c = cues(Phase::InPlay);
    c.thrown_base = Some(1);
    c.runner_cam = Some(runner);
    assert_eq!(pick_shot(&c, None), Shot::RunnerCam(runner));
    c.runner_cam = None;
    assert_eq!(
        pick_shot(&c, Some(Shot::RunnerCam(runner))),
        Shot::BaseCam(1)
    );
    c.runner_cam = Some(runner);
    c.since_contact = BALL_FOLLOW_DELAY * 0.5;
    assert!(matches!(pick_shot(&c, None), Shot::Duel(_)));
    c.since_contact = 10.0;
    c.home_run = true;
    assert_eq!(pick_shot(&c, None), Shot::BallFollow);
}

/// The call never lands on the runner cam: a result reached while it is up
/// cuts to the bag the throw went to.
#[test]
fn a_call_on_the_runner_cam_lands_at_the_bag() {
    let runner = Entity::from_raw_u32(11).unwrap();
    let mut c = cues(Phase::Result);
    c.thrown_base = Some(2);
    assert_eq!(
        pick_shot(&c, Some(Shot::RunnerCam(runner))),
        Shot::BaseCam(2)
    );
    c.thrown_base = None;
    assert_eq!(
        pick_shot(&c, Some(Shot::RunnerCam(runner))),
        Shot::BallFollow
    );
    // Any other shot is still kept for the call.
    c.thrown_base = Some(2);
    assert_eq!(pick_shot(&c, Some(Shot::BaseCam(2))), Shot::BaseCam(2));
}

/// The runner cam's subject: the runner bound for the thrown-to bag beats
/// a nearer one bound elsewhere; with nobody bound there, the nearest.
#[test]
fn the_runner_cam_picks_the_runner_bound_for_the_bag() {
    let field = VariantId::Standard.field();
    let (first, second, third) = (
        field.base_positions[0],
        field.base_positions[1],
        field.base_positions[2],
    );
    let batter = Entity::from_raw_u32(1).unwrap();
    let lead = Entity::from_raw_u32(2).unwrap();
    // The batter, just past first, heads for second; the lead runner,
    // nearer third than the batter is to it, is bound home.
    let racing = [
        (batter, first.lerp(second, 0.2), second),
        (lead, third * 0.9, Vec3::ZERO),
    ];
    assert_eq!(
        pick_runner_subject(racing, third, 1.0),
        Some((lead, Vec3::ZERO)),
        "nobody bound for third: the nearest"
    );
    assert_eq!(
        pick_runner_subject(racing, second, 1.0),
        Some((batter, second)),
        "the batter is the one bound for second"
    );
    assert_eq!(pick_runner_subject([], second, 1.0), None);
    // A bag at the runner's feet within `arrived` still counts as his.
    let near = [(lead, third, third + Vec3::X * 0.5), (batter, third, first)];
    assert_eq!(
        pick_runner_subject(near, third, 1.0),
        Some((lead, third + Vec3::X * 0.5))
    );
}

/// Seconds left on a throw: distance over flat speed, a vertical or
/// stopped ball never arrives.
#[test]
fn throw_secs_left_reads_flat_speed() {
    let bag = Vec3::new(0.0, 0.0, 30.0);
    let t = throw_secs_left(bag, Vec3::new(0.0, 3.0, 0.0), Vec3::new(0.0, -9.0, 15.0));
    assert!((t - 2.0).abs() < 1e-5, "{t}");
    assert!(throw_secs_left(bag, Vec3::ZERO, Vec3::Y * 5.0).is_infinite());
    const { assert!(RUNNER_CAM_MIN_START_SECS > RUNNER_CAM_HANDOFF_SECS) };
}

/// Only a throw from the outfield takes the runner cam, only while the
/// ball is still well out, and never back once the base cam is up.
#[test]
fn the_runner_cam_hands_over_to_the_bag() {
    let field = VariantId::Standard.field();
    let second = field.base_positions[1];
    let fence = field.fence_line;
    let from = Vec3::new(-25.0, 1.8, 80.0);
    // A deep throw flies 1.2 s (`fielding::lob_velocity`).
    let vel = (second - from).with_y(0.0) / 1.2;
    assert!(runner_cam_wanted(from, second, from, vel, fence, false));
    // Halfway in, it is under the hand-over time: the bag's shot now.
    let half = second.lerp(from, 0.45);
    assert!(!runner_cam_wanted(from, second, half, vel, fence, false));
    let early = second.lerp(from, 0.6);
    assert!(runner_cam_wanted(from, second, early, vel, fence, false));
    // A ball that died short of the bag is no throw in flight.
    assert!(!runner_cam_wanted(
        from,
        second,
        early,
        vel * 0.05,
        fence,
        false
    ));
    // Once the base cam is up it keeps the play.
    assert!(!runner_cam_wanted(from, second, from, vel, fence, true));
    // A shortstop ranging deep into the hole, or an outfielder gathering
    // a single in shallow, is not a ball run down in the gap.
    for short in [Vec3::new(12.0, 1.8, 52.0), Vec3::new(20.0, 1.8, 62.0)] {
        assert!(!runner_cam_wanted(
            short,
            field.base_positions[0],
            short,
            Vec3::new(-20.0, 0.0, -40.0),
            fence,
            false
        ));
    }
    // The front lawn: the across-the-street fielder's throw qualifies, a
    // sidewalk fielder's never does.
    let lawn = VariantId::FrontYard.field();
    let to = lawn.base_positions[0];
    for (spot, want) in [
        (lawn.fielder_positions[2], true),
        (lawn.fielder_positions[0], false),
        (lawn.fielder_positions[1], false),
    ] {
        let from = spot + Vec3::Y * 1.5;
        assert_eq!(
            runner_cam_wanted(from, to, from, (to - from) / 1.0, lawn.fence_line, false),
            want,
            "{spot}"
        );
    }
}

/// Degenerate races still give a finite shot above the ground: a runner on
/// his bag, a ball right over the race, a throw straight down his line.
#[test]
fn the_runner_cam_survives_degenerate_races() {
    let field = VariantId::Standard.field();
    let second = field.base_positions[1];
    let first = field.base_positions[0];
    for (runner, bag, ball) in [
        (second, second, Vec3::new(0.0, 2.0, 80.0)),
        (first, second, (first + second) / 2.0),
        (first, second, first + (first - second) * 2.0 + Vec3::Y),
        (Vec3::ZERO, Vec3::ZERO, Vec3::ZERO),
    ] {
        let r = RunnerFrame {
            runner,
            bound_for: bag,
            call_bag: bag,
        };
        let (eye, target, fov) = runner_cam(r, ball, 16.0 / 9.0);
        assert!(
            eye.is_finite() && target.is_finite() && fov.is_finite(),
            "{eye} {target} {fov}"
        );
        assert!(eye.y > 1.0);
        assert!((RUNNER_CAM_MIN_FOV..=MAX_LEAD_FOV + 1e-4).contains(&fov));
    }
}

/// The runner cam stands off the runner's race on the side away from the
/// throw, above the ground, and keeps the runner, the bag and the incoming
/// ball in the middle of the frame.
#[test]
fn the_runner_cam_frames_runner_bag_and_ball() {
    use super::super::framing::{framed_ndc_x, framed_ndc_y};
    let field = VariantId::Standard.field();
    let aspect = 16.0 / 9.0;
    for (from, to, ball) in [
        // A double: rounding first for second, throw from left-centre.
        (0usize, 1usize, Vec3::new(10.0, 3.0, 70.0)),
        // A triple: rounding second for third, throw from the right-field corner.
        (1, 2, Vec3::new(-55.0, 3.5, 60.0)),
        // A double into the right-centre gap: the throw comes from behind him.
        (0, 1, Vec3::new(-30.0, 2.5, 75.0)),
        // A triple down the left-field line: the throw comes from ahead.
        (1, 2, Vec3::new(45.0, 3.0, 55.0)),
    ] {
        let bag = field.base_positions[to];
        let runner = field.base_positions[from].lerp(bag, 0.3);
        // On a double the throw can go in behind him, to first: that bag
        // (where the call lands) stays in frame too.
        let call_bag = if to == 1 {
            field.base_positions[0]
        } else {
            bag
        };
        let r = RunnerFrame {
            runner,
            bound_for: bag,
            call_bag,
        };
        let (eye, target, fov) = runner_cam(r, ball, aspect);
        assert!(eye.y > 2.0, "{eye}");
        // The ball comes in from beyond the race, never from behind the lens.
        assert!(
            eye.distance(ball) > eye.distance(bag) && eye.distance(ball) > eye.distance(runner)
        );
        assert!((RUNNER_CAM_MIN_FOV..=MAX_LEAD_FOV + 1e-4).contains(&fov));
        for p in [
            runner + Vec3::Y,
            bag + Vec3::Y * 0.5,
            call_bag + Vec3::Y * 0.5,
            ball,
        ] {
            let x = framed_ndc_x(eye, target, fov, aspect, p);
            let y = framed_ndc_y(eye, target, fov, p);
            assert!(
                x.abs() <= LEAD_MARGIN + 0.02 && y.abs() <= LEAD_MARGIN + 0.02,
                "{p} at ({x:.2}, {y:.2}) for {from}->{to}"
            );
        }
    }
}

/// A home run in flight is followed, never fielder- or base-cammed, and its
/// result pause is the trot orbit.
#[test]
fn a_home_run_is_followed_then_orbited() {
    let mut c = cues(Phase::InPlay);
    c.home_run = true;
    c.descending_to = Some(Entity::from_raw_u32(3).unwrap());
    c.thrown_base = Some(0);
    assert_eq!(pick_shot(&c, None), Shot::BallFollow);
    c.phase = Phase::Result;
    assert_eq!(pick_shot(&c, Some(Shot::BallFollow)), Shot::TrotOrbit);
}

/// A walk-off that stayed in the park still gets the celebration orbit.
#[test]
fn a_walk_off_is_orbited() {
    let mut c = cues(Phase::Result);
    c.walk_off = true;
    assert_eq!(pick_shot(&c, Some(Shot::BaseCam(3))), Shot::TrotOrbit);
}

/// The call lands in the shot the play ended on: a result pause keeps the
/// current shot — except a gloved pitch, which stays at the plate.
#[test]
fn a_result_keeps_the_shot_it_ended_on() {
    let c = cues(Phase::Result);
    assert_eq!(pick_shot(&c, Some(Shot::BaseCam(0))), Shot::BaseCam(0));
    let f = Entity::from_raw_u32(9).unwrap();
    assert_eq!(
        pick_shot(&c, Some(Shot::FielderCam(f))),
        Shot::FielderCam(f)
    );
    assert_eq!(pick_shot(&c, None), Shot::Duel(DuelView::BattingZoom));
    let mut gloved = cues(Phase::Result);
    gloved.pitch_gloved = true;
    assert_eq!(
        pick_shot(&gloved, Some(Shot::BaseCam(0))),
        Shot::Duel(DuelView::BattingZoom)
    );
}

/// The base cam sits low and just outside the diamond at every bag (home
/// included), looking at the bag from a standoff, in both parks.
#[test]
fn base_cam_frames_every_bag_from_outside_the_diamond() {
    for id in [VariantId::Standard, VariantId::FrontYard] {
        let f = id.field();
        let centre = f.base_positions.iter().copied().sum::<Vec3>() / (f.base_count() as f32 + 1.0);
        for base in 0..=f.base_count() {
            let bag = if base == f.base_count() {
                Vec3::ZERO
            } else {
                f.base_positions[base]
            };
            let (eye, target) = base_cam(&f, base);
            assert!(
                (target - bag).length() < 1.0,
                "{id:?} base {base}: target off the bag"
            );
            let standoff = Vec2::new(eye.x - bag.x, eye.z - bag.z).length();
            assert!(
                (standoff - BASE_CAM_BACK).abs() < 1e-3,
                "{id:?} base {base}: standoff {standoff}"
            );
            assert!((eye.y - BASE_CAM_HEIGHT).abs() < 1e-3);
            // Outside the diamond: further from the centre than the bag is.
            let flat = |v: Vec3| Vec2::new(v.x - centre.x, v.z - centre.z).length();
            assert!(
                flat(eye) > flat(bag),
                "{id:?} base {base}: eye inside the diamond"
            );
        }
    }
}

/// The fielder cam stands behind the fielder on the far side from the
/// ball and looks back across him at the ball.
#[test]
fn fielder_cam_looks_across_the_fielder_at_the_ball() {
    let fielder = Vec3::new(10.0, 0.0, 60.0);
    let ball = Vec3::new(4.0, 8.0, 52.0);
    let (eye, target) = fielder_cam(fielder, ball);
    assert!(
        eye.z > fielder.z && eye.x > fielder.x,
        "eye must be beyond the fielder"
    );
    assert!((eye.y - FIELDER_CAM_HEIGHT).abs() < 1e-3);
    let to_ball = (ball - eye).normalize();
    let to_target = (target - eye).normalize();
    assert!(
        to_ball.dot(to_target) > 0.9,
        "the ball must be near the centre of frame"
    );
}

// ── ShotSpec and transitions ──────────────────────────────────────────────────

fn inputs(field: &FieldSpec) -> ShotInputs<'_> {
    ShotInputs {
        field,
        aspect: DUEL_REFERENCE_ASPECT,
        ball: None,
        fielder: None,
        runner: None,
        wind: Vec3::ZERO,
        home_run: false,
        trot_secs: 0.0,
        duel_fov_nudge: 0.0,
        still: false,
        exit: None,
    }
}

/// The catalog frames each shot as the rig did before it existed: the duel
/// views, the fixed plate shots and the play cams, with the lenses pinned
/// to their literal values.
#[test]
fn every_shot_keeps_its_framing() {
    let field = VariantId::Standard.field();
    let inp = inputs(&field);
    for view in [
        DuelView::BattingZoom,
        DuelView::CatcherPov,
        DuelView::BehindPitcher,
        DuelView::BroadcastPlate,
    ] {
        let (eye, target, fov) = view.framing(&field, DUEL_REFERENCE_ASPECT);
        assert_eq!(Shot::Duel(view).spec(&inp), ShotSpec::new(eye, target, fov));
    }
    let walk = Shot::WalkUp.spec(&inp);
    assert_eq!((walk.eye, walk.target), (WALKUP_EYE, WALKUP_TARGET));
    assert!((walk.fov.to_degrees() - 42.0).abs() < 1e-3);
    let react = Shot::ReactionCam.spec(&inp);
    assert_eq!((react.eye, react.target), (REACTION_EYE, REACTION_TARGET));
    assert!((react.fov.to_degrees() - 40.0).abs() < 1e-3);
    assert!((PLAY_CAM_FOV.to_degrees() - 45.0).abs() < 1e-3);
    assert_eq!(FOLLOW_RATE, 5.0);
    let (eye, target) = base_cam(&field, 2);
    let base = Shot::BaseCam(2).spec(&inp);
    assert_eq!(
        (base.eye, base.target, base.fov),
        (eye, target, PLAY_CAM_FOV)
    );
    // No ball: the play cams fall back to the wide broadcast shot.
    let wide = Shot::BallFollow.spec(&inp);
    assert_eq!(
        (wide.eye, wide.target),
        (field.broadcast_eye, field.broadcast_target)
    );
    let f = Entity::from_raw_u32(4).unwrap();
    assert_eq!(Shot::FielderCam(f).spec(&inp), wide);
}

/// The fielder cam frames the fielder it was given, or the ball if the
/// fielder's rig is gone.
#[test]
fn the_fielder_cam_reads_its_fielder() {
    let field = VariantId::Standard.field();
    let ball = BallState {
        pos: Vec3::new(4.0, 8.0, 52.0),
        vel: Vec3::new(0.0, -5.0, 0.0),
        spin: Vec3::ZERO,
    };
    let fielder = Vec3::new(10.0, 0.0, 60.0);
    let mut inp = inputs(&field);
    inp.ball = Some(ball);
    inp.fielder = Some(fielder);
    let shot = Shot::FielderCam(Entity::from_raw_u32(4).unwrap());
    let (eye, target) = fielder_cam(fielder, ball.pos);
    let spec = shot.spec(&inp);
    assert_eq!((spec.eye, spec.target), (eye, target));
    inp.fielder = None;
    let (eye, _) = fielder_cam(ball.pos, ball.pos);
    assert_eq!(shot.spec(&inp).eye, eye);
}

/// A home run in flight sweeps with the ball; anything else frames where
/// it will come down.
#[test]
fn the_ball_follow_splits_on_the_home_run() {
    let field = VariantId::Standard.field();
    let ball = BallState {
        pos: Vec3::new(5.0, 20.0, 60.0),
        vel: Vec3::new(2.0, 10.0, 30.0),
        spin: Vec3::ZERO,
    };
    let mut inp = inputs(&field);
    inp.ball = Some(ball);
    let fly = Shot::BallFollow.spec(&inp);
    let (eye, target, _) = landing_zone_cam(ball, Vec3::ZERO, -field.broadcast_eye.z);
    assert_eq!((fly.eye, fly.target), (eye, target));
    inp.home_run = true;
    let hr = Shot::BallFollow.spec(&inp);
    assert_eq!((hr.eye, hr.target), home_run_follow(&field, ball.pos));
}

/// The trot orbit sweeps the behind-home arc and then holds its far end.
#[test]
fn the_trot_orbit_sweeps_then_holds() {
    let field = VariantId::Standard.field();
    let at = |secs| {
        let mut inp = inputs(&field);
        inp.trot_secs = secs;
        Shot::TrotOrbit.spec(&inp).eye
    };
    assert_ne!(at(0.0), at(1.0));
    assert_eq!(at(100.0), at(200.0));
}

/// Every shot still cuts in: the catalog changes no transitions.
#[test]
fn every_shot_cuts_in_today() {
    let f = Entity::from_raw_u32(1).unwrap();
    for shot in [
        Shot::Duel(DuelView::BattingZoom),
        Shot::BallFollow,
        Shot::FielderCam(f),
        Shot::BaseCam(0),
        Shot::TrotOrbit,
        Shot::WalkUp,
        Shot::ReactionCam,
    ] {
        assert_eq!(
            shot.transition_from(Some(Shot::BallFollow)),
            Transition::Cut
        );
    }
}

fn spec(eye: Vec3, target: Vec3, fov: f32) -> ShotSpec {
    ShotSpec::new(eye, target, fov)
}

/// A cut lands on the new framing the same frame.
#[test]
fn a_cut_snaps() {
    let mut m = RigMotion::new(Vec3::ZERO, Vec3::Z, 1.0);
    let to = spec(Vec3::new(10.0, 5.0, 0.0), Vec3::new(0.0, 0.0, 50.0), 0.8);
    m.start(Transition::Cut, to, 1.0 / 60.0);
    assert_eq!((m.eye, m.target, m.fov), (to.eye, to.target, to.fov));
    assert!(!m.blending());
}

/// Within a shot the lens follows its subject at the shot's rate, never
/// overshooting.
#[test]
fn within_a_shot_the_lens_follows() {
    let mut m = RigMotion::new(Vec3::ZERO, Vec3::Z, 1.0);
    let to = spec(Vec3::new(10.0, 0.0, 0.0), Vec3::Z, 1.0);
    m.step(to, 1.0 / 60.0);
    let k = 1.0 - (-FOLLOW_RATE / 60.0_f32).exp();
    assert!((m.eye.x - 10.0 * k).abs() < 1e-4, "{}", m.eye.x);
    for _ in 0..600 {
        m.step(to, 1.0 / 60.0);
        assert!(m.eye.x <= 10.0 + 1e-4);
    }
    assert!((m.eye.x - 10.0).abs() < 1e-3);
}

/// An ease glides from where the lens was, passes the midpoint at half
/// time, tracks a moving subject, and lands on it exactly at the end.
#[test]
fn an_ease_glides_and_lands() {
    let mut m = RigMotion::new(Vec3::ZERO, Vec3::Z, 1.0);
    let to = spec(Vec3::new(10.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 20.0), 0.5);
    m.start(Transition::Ease { secs: 1.0 }, to, 0.0);
    assert_eq!(m.eye, Vec3::ZERO, "an ease starts where the lens is");
    for _ in 0..5 {
        m.step(to, 0.1);
    }
    assert!((m.eye.x - 5.0).abs() < 1e-3, "midpoint {}", m.eye.x);
    assert!((m.fov - 0.75).abs() < 1e-4);
    // The subject moves on: the ease follows it, then lands on it.
    let moved = spec(Vec3::new(12.0, 0.0, 0.0), to.target, to.fov);
    m.step(moved, 0.1);
    let s = smoothstep(0.6);
    assert!((m.eye.x - 12.0 * s).abs() < 1e-3, "mid-blend {}", m.eye.x);
    for _ in 0..4 {
        m.step(moved, 0.1);
    }
    m.step(moved, 0.01);
    assert!(!m.blending());
    assert_eq!(m.eye, moved.eye);
}

/// A whip pan holds the eye and swings the look-at, then cuts the eye.
#[test]
fn a_whip_pan_swings_then_cuts() {
    let mut m = RigMotion::new(Vec3::ZERO, Vec3::Z, 1.0);
    let to = spec(Vec3::new(10.0, 0.0, 0.0), Vec3::new(30.0, 0.0, 0.0), 0.5);
    m.start(Transition::WhipPan { secs: 0.3 }, to, 0.05);
    m.step(to, 0.1);
    assert_eq!(m.eye, Vec3::ZERO, "the eye holds during the pan");
    assert!(
        (m.target - Vec3::new(15.0, 0.0, 0.5)).length() < 1e-3,
        "{}",
        m.target
    );
    m.step(to, 0.2);
    assert_eq!((m.eye, m.target, m.fov), (to.eye, to.target, to.fov));
}

/// A transition with no usable length is a cut: zero, negative, NaN or
/// infinite seconds can never leave the lens stuck mid-blend.
#[test]
fn a_degenerate_transition_is_a_cut() {
    for secs in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        for kind in [Transition::Ease { secs }, Transition::WhipPan { secs }] {
            let mut m = RigMotion::new(Vec3::ZERO, Vec3::Z, 1.0);
            let to = spec(Vec3::X, Vec3::Y, 0.5);
            m.start(kind, to, 1.0 / 60.0);
            assert_eq!(m.eye, Vec3::X, "{kind:?}");
            assert!(!m.blending());
        }
    }
}

/// A blend's first step uses the frame it starts on.
#[test]
fn a_blend_moves_on_its_first_frame() {
    let mut m = RigMotion::new(Vec3::ZERO, Vec3::Z, 1.0);
    let to = spec(Vec3::new(10.0, 0.0, 0.0), Vec3::Z, 1.0);
    m.start(Transition::Ease { secs: 1.0 }, to, 0.1);
    assert!(m.eye.x > 0.0);
}

// ── Predictive framing (task 7) ───────────────────────────────────────────────

/// A batted ball off the bat: launch angle and spray (degrees, spray > 0
/// toward third, +X) and exit speed (m/s), with the bat's spin
/// (`rules::hit_spin`).
fn launch(angle: f32, spray: f32, speed: f32) -> BallState {
    let (a, s) = (angle.to_radians(), spray.to_radians());
    let vel = Vec3::new(s.sin() * a.cos(), a.sin(), s.cos() * a.cos()) * speed;
    BallState {
        pos: Vec3::new(0.0, 1.0, 0.0),
        vel,
        spin: rules::hit_spin(vel),
    }
}

/// One 60 Hz step of the live ball's flight model (as
/// `rules::predict_landing_from` integrates it), then rolling on the
/// grass: the ground slab has no restitution, and Rapier combines the
/// ball's `Min` rule with it to a dead ball (`sim/ball.rs`), so it never
/// hops on the grass.
fn fly_step(b: &mut BallState, dt: f32) {
    use crate::game::ball::{BALL_DRAG_FACTOR, MAGNUS_FACTOR};
    let speed = b.vel.length();
    b.vel += -BALL_DRAG_FACTOR * speed * b.vel * dt;
    b.vel += MAGNUS_FACTOR * b.spin.cross(b.vel) * dt;
    b.spin /= 1.0 + dt * rules::SPIN_DAMPING;
    b.vel.y -= rules::GRAVITY * dt;
    b.pos += b.vel * dt;
    if b.pos.y < 0.0 {
        b.pos.y = 0.0;
        b.vel.y = 0.0;
        b.vel.x *= 0.8;
        b.vel.z *= 0.8;
        b.spin = Vec3::ZERO;
    }
}

/// Fielding's first-bounce mark (`ActivePlay::landed_at`): the first time
/// the ball is down low and not climbing, on the contact clock.
fn mark_landing(b: &BallState, t: f32, landed_at: &mut Option<f32>) {
    if landed_at.is_none() && b.pos.y < 0.25 && b.vel.y <= 0.0 {
        *landed_at = Some(t);
    }
}

/// Plays a batted ball through the broadcast rig's shot choice and lens
/// motion at 60 fps and returns the worst |NDC| the ball reached on either
/// axis once the plate hold ended, with the shot it happened in. The
/// fielder waits under the predicted landing spot for a fly, and meets a
/// grounder 1.2 s down its path.
fn worst_ball_ndc(field: &FieldSpec, start: BallState, aspect: f32) -> (f32, Shot) {
    use super::super::framing::{framed_ndc_x, framed_ndc_y};
    let dt = 1.0 / 60.0;
    let fielder_e = Entity::from_raw_u32(5).unwrap();
    let (landing, _) = rules::predict_landing_from(
        start.pos,
        start.vel,
        start.spin,
        crate::game::ball::BALL_DRAG_FACTOR,
        crate::game::ball::MAGNUS_FACTOR,
        Vec3::ZERO,
    );
    let mut probe = start;
    for _ in 0..72 {
        fly_step(&mut probe, dt);
    }
    let meet = probe.pos.with_y(0.0);
    let mut ball = start;
    let mut motion = RigMotion::new(field.broadcast_eye, field.broadcast_target, BROADCAST_FOV);
    let mut shot = None;
    let mut read = PlayRead::default();
    let mut landed_at = None;
    let mut worst = (0.0_f32, Shot::BallFollow);
    let mut t = 0.0;
    let mut landed = false;
    while t < 4.0 && !landed {
        fly_step(&mut ball, dt);
        t += dt;
        mark_landing(&ball, t, &mut landed_at);
        read.observe(
            t >= BALL_FOLLOW_DELAY,
            Some(ball),
            landed_at,
            false,
            Vec3::ZERO,
        );
        let grounder = read.grounder() == Some(true);
        let descending =
            ball.pos.y > FIELDER_CAM_MIN_HEIGHT && ball.vel.y < 0.0 && !grounder && !read.landed();
        let mut c = cues(Phase::InPlay);
        c.since_contact = t;
        c.grounder = grounder;
        c.descending_to = descending.then_some(fielder_e);
        let next = pick_shot(&c, shot);
        let mut inp = inputs(field);
        inp.aspect = aspect;
        inp.ball = Some(ball);
        inp.fielder = Some(if grounder { meet } else { landing });
        let want = next.spec(&inp);
        if shot != Some(next) {
            motion.start(next.transition_from(shot), want, dt);
            shot = Some(next);
        } else {
            motion.step(want, dt);
        }
        if t >= BALL_FOLLOW_DELAY {
            let x = framed_ndc_x(motion.eye, motion.target, motion.fov, aspect, ball.pos);
            let y = framed_ndc_y(motion.eye, motion.target, motion.fov, ball.pos);
            let m = x.abs().max(y.abs());
            if m > worst.0 {
                worst = (m, next);
            }
        }
        // A fly is over once it comes down; a grounder once it reaches the
        // fielder.
        landed = if grounder {
            t > 1.2
        } else {
            t > BALL_FOLLOW_DELAY && ball.pos.y <= 0.0
        };
    }
    worst
}

/// The ball never leaves the middle 80% of the frame, for a pop-up, a
/// liner, a deep fly and a grounder through each hole, at 16:9 and on a
/// narrower 4:3 window, and in the front yard (whose eye sits closer).
#[test]
fn the_ball_stays_in_the_middle_of_the_frame() {
    let field = VariantId::Standard.field();
    let hits = [
        ("pop-up", launch(72.0, 5.0, 30.0)),
        ("liner", launch(14.0, -10.0, 38.0)),
        ("deep fly", launch(30.0, 15.0, 40.0)),
        ("gapper", launch(22.0, -28.0, 39.0)),
        ("grounder, 3B hole", launch(-6.0, 30.0, 36.0)),
        ("grounder, SS hole", launch(-6.0, 12.0, 36.0)),
        ("grounder, 2B hole", launch(-6.0, -12.0, 36.0)),
        ("grounder, 1B hole", launch(-6.0, -30.0, 36.0)),
    ];
    for (aspect, field) in [
        (16.0 / 9.0, field.clone()),
        (4.0 / 3.0, field),
        (16.0 / 9.0, VariantId::FrontYard.field()),
    ] {
        for (name, ball) in hits {
            let (worst, shot) = worst_ball_ndc(&field, ball, aspect);
            assert!(
                worst <= LEAD_MARGIN + 0.02,
                "{name} at {aspect:.2}: ball at |NDC| {worst:.2} in {shot:?}"
            );
        }
    }
}

/// Grounders take the ground-ball shot; balls in the air never do, not
/// even in their last half-metre, nor once a fielder holds the ball still
/// at his glove.
#[test]
fn only_grounders_take_the_ground_ball_shot() {
    let dt = 1.0 / 60.0;
    for (ball, want) in [
        (launch(-6.0, 12.0, 36.0), true),
        (launch(-15.0, -20.0, 30.0), true),
        // A hard chopper: down well inside the plate hold.
        (launch(-20.0, 0.0, 40.0), true),
        (launch(-35.0, 25.0, 45.0), true),
        (launch(14.0, -10.0, 38.0), false),
        (launch(72.0, 5.0, 30.0), false),
        (launch(30.0, 15.0, 40.0), false),
    ] {
        let mut b = ball;
        let mut read = PlayRead::default();
        let mut landed_at = None;
        let mut t = 0.0;
        // Run past the landing and on through a held ball.
        while t < 8.0 {
            fly_step(&mut b, dt);
            t += dt;
            mark_landing(&b, t, &mut landed_at);
            read.observe(
                t >= BALL_FOLLOW_DELAY,
                Some(b),
                landed_at,
                false,
                Vec3::ZERO,
            );
            let mut c = cues(Phase::InPlay);
            c.since_contact = t;
            c.grounder = read.grounder() == Some(true);
            if t < BALL_FOLLOW_DELAY {
                assert_eq!(read.grounder(), None, "decided during the plate hold");
                continue;
            }
            assert_eq!(
                pick_shot(&c, None) == Shot::GroundBall,
                want,
                "{:?} at {t:.2}",
                ball.vel
            );
        }
        // Later frames — a fielder holding the ball chest-high, still —
        // never reopen the call.
        read.observe(
            true,
            Some(BallState {
                pos: Vec3::new(5.0, 1.2, 60.0),
                vel: Vec3::ZERO,
                spin: Vec3::ZERO,
            }),
            landed_at,
            false,
            Vec3::ZERO,
        );
        assert_eq!(read.grounder(), Some(want), "the call never changes");
        assert!(read.landed(), "every ball here came down");
    }
    // Nothing is decided before the plate hold ends, or without a ball.
    let mut read = PlayRead::default();
    read.observe(
        false,
        Some(launch(-6.0, 12.0, 36.0)),
        None,
        false,
        Vec3::ZERO,
    );
    read.observe(true, None, None, false, Vec3::ZERO);
    assert_eq!(read.grounder(), None);
}

/// A ball already down during the plate hold is a grounder whatever it is
/// doing at the call; a touchdown after the hold is not.
#[test]
fn a_touchdown_in_the_hold_makes_a_grounder() {
    let fly = launch(30.0, 0.0, 40.0);
    let mut read = PlayRead::default();
    read.observe(
        true,
        Some(fly),
        Some(BALL_FOLLOW_DELAY * 0.4),
        false,
        Vec3::ZERO,
    );
    assert_eq!(read.grounder(), Some(true));
    let mut read = PlayRead::default();
    read.observe(true, Some(fly), None, false, Vec3::ZERO);
    assert_eq!(read.grounder(), Some(false));
    assert!(!read.landed());
    // Called late (the broadcast view switched on mid-play) after a
    // touchdown past the hold: a fly that came down, even rolling now.
    let mut read = PlayRead::default();
    let rolling = BallState {
        pos: Vec3::new(0.0, 0.04, 70.0),
        vel: Vec3::new(0.0, 0.0, 5.0),
        spin: Vec3::ZERO,
    };
    read.observe(true, Some(rolling), Some(3.0), false, Vec3::ZERO);
    assert_eq!(read.grounder(), Some(false));
}

/// The last play's touchdown mark, still set on the contact frame, never
/// marks the new ball landed: only marks seen after the call count.
#[test]
fn a_stale_mark_before_the_call_is_ignored() {
    let mut read = PlayRead::default();
    read.observe(
        false,
        Some(launch(30.0, 0.0, 40.0)),
        Some(3.0),
        false,
        Vec3::ZERO,
    );
    assert!(!read.landed());
    read.observe(true, Some(launch(30.0, 0.0, 40.0)), None, false, Vec3::ZERO);
    assert_eq!(read.grounder(), Some(false));
    assert!(!read.landed());
}

/// A fly is done with the fielder cam once it has come down, or banged off
/// the wall.
#[test]
fn a_fly_is_landed_after_touchdown_or_the_wall() {
    let dt = 1.0 / 60.0;
    let mut b = launch(35.0, 0.0, 34.0);
    let mut read = PlayRead::default();
    let mut landed_at = None;
    let mut t = 0.0;
    while landed_at.is_none() {
        fly_step(&mut b, dt);
        t += dt;
        mark_landing(&b, t, &mut landed_at);
        if landed_at.is_none() {
            read.observe(t >= BALL_FOLLOW_DELAY, Some(b), None, false, Vec3::ZERO);
            assert!(!read.landed(), "landed in the air at {t:.2}");
        }
    }
    read.observe(true, Some(b), landed_at, false, Vec3::ZERO);
    assert!(read.landed());

    let mut read = PlayRead::default();
    read.observe(true, Some(launch(35.0, 0.0, 34.0)), None, false, Vec3::ZERO);
    read.observe(true, Some(launch(35.0, 0.0, 34.0)), None, true, Vec3::ZERO);
    assert!(read.landed());
}

/// The ball-follow's lens is wider while the ball is high than as it drops
/// to its landing spot, never tighter than the broadcast
/// lens or wider than the cap.
#[test]
fn the_lens_widens_on_the_climb() {
    let field = VariantId::Standard.field();
    let mut b = launch(72.0, 5.0, 30.0);
    let mut fovs = vec![];
    for _ in 0..240 {
        fly_step(&mut b, 1.0 / 60.0);
        let mut inp = inputs(&field);
        inp.ball = Some(b);
        fovs.push(Shot::BallFollow.spec(&inp).fov);
        if b.pos.y <= 0.0 {
            break;
        }
    }
    // Wider at the top of the flight than as it comes down to the spot.
    let apex = fovs.len() / 2;
    assert!(fovs[apex] > *fovs.last().unwrap(), "{fovs:?}");
    assert!(
        fovs.iter()
            .all(|f| (BROADCAST_FOV - 1e-4..=MAX_LEAD_FOV + 1e-4).contains(f))
    );
}

/// The ground-ball shot frames the ball, the fielder and first base.
#[test]
fn the_ground_ball_shot_holds_ball_fielder_and_first() {
    use super::super::framing::{framed_ndc_x, framed_ndc_y};
    let field = VariantId::Standard.field();
    let first = field.base_positions[0];
    let ball = Vec3::new(6.0, 0.2, 18.0);
    let fielder = Vec3::new(9.0, 0.0, 26.0);
    let (eye, target, fov) = ground_ball_cam(ball, Some(fielder), first, 16.0 / 9.0);
    for p in [ball, fielder + Vec3::Y, first + Vec3::Y * 0.5] {
        let x = framed_ndc_x(eye, target, fov, 16.0 / 9.0, p);
        let y = framed_ndc_y(eye, target, fov, p);
        assert!(x.abs() <= 0.81 && y.abs() <= 0.81, "{p}: ({x:.2}, {y:.2})");
    }
}

/// `fit_vfov` is the inverse of the framing math: points it fits land on
/// the margin, never past it.
#[test]
fn fit_vfov_puts_the_widest_point_on_the_margin() {
    use super::super::framing::{fit_vfov, framed_ndc_x, framed_ndc_y};
    let eye = Vec3::new(0.0, 10.0, -20.0);
    let target = Vec3::new(0.0, 2.0, 30.0);
    let pts = [Vec3::new(20.0, 0.0, 40.0), Vec3::new(-5.0, 25.0, 50.0)];
    let fov = fit_vfov(eye, target, &pts, 1.5, 0.8);
    let m = pts
        .iter()
        .map(|&p| {
            framed_ndc_x(eye, target, fov, 1.5, p)
                .abs()
                .max(framed_ndc_y(eye, target, fov, p).abs())
        })
        .fold(0.0, f32::max);
    assert!((m - 0.8).abs() < 1e-3, "{m}");
    // Behind the lens: nothing fits.
    assert_eq!(
        fit_vfov(eye, target, &[eye - Vec3::Z], 1.5, 0.8),
        std::f32::consts::PI
    );
}

/// A big moment's sequence owns the result pause while it runs, ahead of
/// the trot orbit and the kept shot.
#[test]
fn a_sequence_shot_takes_the_result_pause() {
    let mut c = cues(Phase::Result);
    c.home_run = true;
    c.sequence = Some(Shot::CrowdCam);
    assert_eq!(pick_shot(&c, Some(Shot::BallFollow)), Shot::CrowdCam);
    c.sequence = None;
    assert_eq!(pick_shot(&c, Some(Shot::CrowdCam)), Shot::TrotOrbit);
    // A dive's shot that ended hands the pause back to the play: the bag
    // the throw went to, else the ball.
    let f = Entity::from_raw_u32(3).unwrap();
    let mut dive = cues(Phase::Result);
    assert_eq!(pick_shot(&dive, Some(Shot::DiveCam(f))), Shot::BallFollow);
    dive.thrown_base = Some(3);
    assert_eq!(pick_shot(&dive, Some(Shot::DiveCam(f))), Shot::BaseCam(3));
    // Outside the result pause it is ignored.
    let mut live = cues(Phase::InPlay);
    live.since_contact = 1.0;
    live.sequence = Some(Shot::PileUp);
    assert_eq!(pick_shot(&live, None), Shot::BallFollow);
}

fn in_frame(eye: Vec3, target: Vec3, fov: f32, p: Vec3) -> bool {
    let ahead = (p - eye).dot(target - eye) > 0.0;
    ahead
        && framed_ndc_x(eye, target, fov, 16.0 / 9.0, p).abs() < 0.9
        && framed_ndc_y(eye, target, fov, p).abs() < 0.9
}

/// The crowd shot stands on the grass inside the fence and looks up into
/// the seats on the line the ball left by, never at the batter's eye.
#[test]
fn the_crowd_shot_looks_up_into_the_seats() {
    let field = VariantId::Standard.field();
    let mut inp = inputs(&field);
    for exit in [
        Vec3::new(-90.0, 5.0, 80.0),
        Vec3::new(70.0, 8.0, 95.0),
        Vec3::new(0.0, 10.0, 130.0),
    ] {
        inp.exit = Some(exit);
        let s = Shot::CrowdCam.spec(&inp);
        let fence = rules::fence_at(s.target, &field);
        let flat = |v: Vec3| Vec2::new(v.x, v.z).length();
        assert!(flat(s.eye) < fence, "eye inside the park: {s:?}");
        assert!(flat(s.target) > fence, "looking past the fence: {s:?}");
        assert!(s.target.y > s.eye.y, "looking up at the stands: {s:?}");
        let angle = s.target.x.atan2(s.target.z).abs();
        assert!(angle > 10.0_f32.to_radians(), "off the batter's eye");
        // On the side the ball went out.
        if exit.x.abs() > 1.0 {
            assert_eq!(s.target.x.signum(), exit.x.signum());
        }
    }
}

/// The dive cam is low and close, the fielder in frame, wherever he dove.
#[test]
fn the_dive_cam_frames_the_fielder_low() {
    let field = VariantId::Standard.field();
    let mut inp = inputs(&field);
    let f = Entity::from_raw_u32(9).unwrap();
    for fielder in [
        Vec3::new(-12.0, 0.0, 28.0),
        Vec3::new(30.0, 0.0, 80.0),
        Vec3::new(0.0, 0.0, 95.0),
        Vec3::ZERO,
    ] {
        inp.fielder = Some(fielder);
        let s = Shot::DiveCam(f).spec(&inp);
        assert!(s.eye.is_finite() && s.target.is_finite());
        assert!(s.eye.y < 1.0, "low: {s:?}");
        assert!(s.eye.distance(fielder) < 5.0, "close: {s:?}");
        for p in [fielder + Vec3::Y * 0.2, fielder + Vec3::Y * 1.0] {
            assert!(in_frame(s.eye, s.target, s.fov, p), "{p} out of {s:?}");
        }
    }
    inp.fielder = None;
    assert_eq!(
        Shot::DiveCam(f).spec(&inp).eye,
        field.broadcast_eye,
        "no fielder: the wide shot"
    );
}

/// The pile-up frames the plate and the ground around it.
#[test]
fn the_pile_up_frames_the_plate() {
    let field = VariantId::Standard.field();
    let s = Shot::PileUp.spec(&inputs(&field));
    for p in [
        Vec3::ZERO,
        Vec3::new(0.0, 1.8, 0.0),
        Vec3::new(1.5, 0.0, -1.0),
        Vec3::new(-1.5, 1.0, 0.5),
    ] {
        assert!(in_frame(s.eye, s.target, s.fov, p), "{p} out of {s:?}");
    }
}

// ── Camera settings and reduce motion ─────────────────────────────────────────

#[test]
fn the_duel_fov_nudge_widens_and_tightens_only_the_duel() {
    let field = VariantId::Standard.field();
    let mut inp = inputs(&field);
    let base = Shot::Duel(DuelView::default()).spec(&inp).fov;
    let wide = Shot::BallFollow.spec(&inp).fov;
    inp.duel_fov_nudge = 10.0_f32.to_radians();
    let wider = Shot::Duel(DuelView::default()).spec(&inp).fov;
    assert!((wider - base - 10.0_f32.to_radians()).abs() < 1e-5);
    assert_eq!(
        Shot::BallFollow.spec(&inp).fov,
        wide,
        "other shots keep their lens"
    );
    inp.duel_fov_nudge = (-5.0_f32).to_radians();
    assert!(Shot::Duel(DuelView::default()).spec(&inp).fov < base);
}

#[test]
fn reduce_motion_cuts_every_transition() {
    for kind in [
        Transition::Cut,
        Transition::Ease { secs: 0.6 },
        Transition::WhipPan { secs: 0.3 },
    ] {
        assert_eq!(motion_safe(kind, true), Transition::Cut, "{kind:?}");
        assert_eq!(motion_safe(kind, false), kind);
    }
}

#[test]
fn reduce_motion_holds_the_trot_orbit_still() {
    let field = VariantId::Standard.field();
    let mut inp = inputs(&field);
    inp.home_run = true;
    inp.still = true;
    let opening = Shot::TrotOrbit.spec(&inp);
    for secs in [0.5, 2.0, 6.0] {
        inp.trot_secs = secs;
        let s = Shot::TrotOrbit.spec(&inp);
        assert_eq!(s.eye, opening.eye, "{secs} s");
        assert_eq!(s.target, opening.target);
    }
    inp.still = false;
    assert_ne!(
        Shot::TrotOrbit.spec(&inp).eye,
        opening.eye,
        "it sweeps otherwise"
    );
}

/// The first walk-up of each half opens on the cutaway for its hold, then
/// the walk-up shot takes over; the game's first half never gets one (the
/// intro just showed the park), a later walk-up in the same half doesn't
/// either, and nor does a half that opens without a walk-up.
#[test]
fn each_new_half_opens_on_the_cutaway() {
    // Top of the first: marked seen, no cutaway.
    let (cut, up) = half_inning_cutaway(true, true, (1, true), 0.0, None);
    assert!(!up);
    // The half flips during the third out's result, before any pre-pitch
    // frame: nothing is marked yet.
    let (cut, up) = half_inning_cutaway(false, false, (1, false), 99.0, cut);
    assert!(!up);
    // Bottom of the first: the first walk-up's hold starts it.
    let (cut, up) = half_inning_cutaway(true, true, (1, false), 100.0, cut);
    assert!(up);
    let (cut, up) = half_inning_cutaway(true, true, (1, false), 100.0 + CUTAWAY_SECS * 0.9, cut);
    assert!(up, "held through its seconds");
    let (cut, up) = half_inning_cutaway(true, true, (1, false), 100.0 + CUTAWAY_SECS + 0.01, cut);
    assert!(!up, "then the walk-up shot");
    // Between pitches and at the half's next walk-up: no second cutaway.
    let (cut, up) = half_inning_cutaway(false, false, (1, false), 130.0, cut);
    assert!(!up);
    let (cut, up) = half_inning_cutaway(true, true, (1, false), 160.0, cut);
    assert!(!up, "only the half's first walk-up");
    // Top of the second: a fresh one.
    let (cut, up) = half_inning_cutaway(true, true, (2, true), 300.0, cut);
    assert!(up);
    // The bottom of the second opens with no walk-up (the top ended on a
    // caught stealing): seen, and its second batter's walk-up stays plain.
    let (cut, up) = half_inning_cutaway(true, false, (2, false), 400.0, cut);
    assert!(!up);
    let (_, up) = half_inning_cutaway(true, true, (2, false), 420.0, cut);
    assert!(!up, "no cutaway mid-half");

    let mut c = cues(Phase::PrePitch);
    c.walkup = true;
    c.cutaway = true;
    assert_eq!(pick_shot(&c, None), Shot::HalfInning);
    c.cutaway = false;
    assert_eq!(pick_shot(&c, None), Shot::WalkUp);
}

/// The cutaway frames the whole scoreboard in a stadium (all four corners
/// inside the frame, the board filling a good share of it), and the yard
/// from high behind the plate elsewhere.
#[test]
fn the_cutaway_frames_the_scoreboard() {
    let field = VariantId::Standard.field();
    for aspect in [16.0 / 9.0, 4.0 / 3.0, 390.0 / 664.0] {
        let (eye, target, fov) = half_inning_cam(&field, aspect);
        let board = crate::game::field::scoreboard_centre(&field);
        assert_eq!(target, board);
        let half = crate::game::field::SCOREBOARD_SIZE / 2.0;
        let across = Vec3::Y.cross((-board).with_y(0.0).normalize()).normalize();
        let mut widest: f32 = 0.0;
        for (sx, sy) in [(1.0, 1.0), (-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0)] {
            let p = board + across * half.x * sx + Vec3::Y * half.y * sy;
            let x = framed_ndc_x(eye, target, fov, aspect, p);
            let y = framed_ndc_y(eye, target, fov, p);
            assert!(x.abs() <= 1.0 && y.abs() <= 1.0, "corner out at {aspect}");
            widest = widest.max(x.abs()).max(y.abs());
        }
        assert!(widest > 0.6, "the board is too small at {aspect}: {widest}");
        assert!(
            eye.length() < board.length(),
            "the eye is in front of the board"
        );
    }
    let yard = VariantId::FrontYard.field();
    let (eye, target, _) = half_inning_cam(&yard, 16.0 / 9.0);
    assert!(eye.y > 10.0 && eye.z < 0.0 && target.z > 0.0);
}

/// The nudge lands on the authored lens before the narrow-aspect
/// correction: on a portrait phone and a 4:3 tablet, +5 widens and -5
/// tightens every duel view, never clamping the corrected shot narrower.
#[test]
fn the_duel_fov_nudge_holds_on_narrow_screens() {
    let field = VariantId::Standard.field();
    for aspect in [390.0 / 664.0, 4.0 / 3.0, DUEL_REFERENCE_ASPECT] {
        for view in [
            DuelView::BattingZoom,
            DuelView::CatcherPov,
            DuelView::BehindPitcher,
            DuelView::BroadcastPlate,
        ] {
            let mut inp = inputs(&field);
            inp.aspect = aspect;
            let base = Shot::Duel(view).spec(&inp).fov;
            inp.duel_fov_nudge = 5.0_f32.to_radians();
            let wider = Shot::Duel(view).spec(&inp).fov;
            inp.duel_fov_nudge = (-5.0_f32).to_radians();
            let tighter = Shot::Duel(view).spec(&inp).fov;
            assert!(
                wider > base,
                "{view:?} at {aspect}: +5 gave {wider} vs {base}"
            );
            assert!(
                tighter < base,
                "{view:?} at {aspect}: -5 gave {tighter} vs {base}"
            );
        }
    }
}
