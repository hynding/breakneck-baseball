//! Tests for [`super`] — the shot list (TODO 102) and the play cams.

use super::*;
use crate::game::variant::VariantId;

fn cues(phase: Phase) -> ShotCues {
    ShotCues {
        phase,
        since_contact: 10.0,
        home_run: false,
        pitch_gloved: false,
        view: DuelView::BattingZoom,
        walkup: false,
        strikeout: false,
        thrown_base: None,
        descending_to: None,
    }
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
    let fielder = Entity::from_raw(7);
    c.descending_to = Some(fielder);
    assert_eq!(pick_shot(&c, None), Shot::FielderCam(fielder));
    c.thrown_base = Some(1);
    assert_eq!(pick_shot(&c, None), Shot::BaseCam(1));
}

/// A home run in flight is followed, never fielder- or base-cammed, and its
/// result pause is the trot orbit.
#[test]
fn a_home_run_is_followed_then_orbited() {
    let mut c = cues(Phase::InPlay);
    c.home_run = true;
    c.descending_to = Some(Entity::from_raw(3));
    c.thrown_base = Some(0);
    assert_eq!(pick_shot(&c, None), Shot::BallFollow);
    c.phase = Phase::Result;
    assert_eq!(pick_shot(&c, Some(Shot::BallFollow)), Shot::TrotOrbit);
}

/// The call lands in the shot the play ended on: a result pause keeps the
/// current shot — except a gloved pitch, which stays at the plate.
#[test]
fn a_result_keeps_the_shot_it_ended_on() {
    let c = cues(Phase::Result);
    assert_eq!(pick_shot(&c, Some(Shot::BaseCam(0))), Shot::BaseCam(0));
    let f = Entity::from_raw(9);
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
