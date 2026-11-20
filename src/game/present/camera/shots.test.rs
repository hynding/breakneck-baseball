//! Tests for [`super`]: the shot list (TODO 102), the play cams, and
//! moving between shots.

use super::super::DUEL_REFERENCE_ASPECT;
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
        strikeout: false,
        thrown_base: None,
        grounder: false,
        descending_to: None,
        duel_throw_base: None,
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
        wind: Vec3::ZERO,
        home_run: false,
        trot_secs: 0.0,
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
/// `rules::predict_landing_from` integrates it), bouncing and rolling on
/// the grass.
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
        b.vel.y = -b.vel.y * 0.55; // ball.rs restitution
        b.vel.x *= 0.8;
        b.vel.z *= 0.8;
        b.spin = Vec3::ZERO;
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
    let mut worst = (0.0_f32, Shot::BallFollow);
    let mut t = 0.0;
    let mut landed = false;
    while t < 4.0 && !landed {
        fly_step(&mut ball, dt);
        t += dt;
        read.observe(t >= BALL_FOLLOW_DELAY, Some(ball), Vec3::ZERO);
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
        // A hard chopper: down at once, then up on a high first hop at the
        // decision frame — still a grounder.
        (launch(-20.0, 0.0, 40.0), true),
        (launch(-35.0, 25.0, 45.0), true),
        (launch(14.0, -10.0, 38.0), false),
        (launch(72.0, 5.0, 30.0), false),
        (launch(30.0, 15.0, 40.0), false),
    ] {
        let mut b = ball;
        let mut read = PlayRead::default();
        let mut t = 0.0;
        // Run past the landing and on through a held ball.
        while t < 8.0 {
            fly_step(&mut b, dt);
            t += dt;
            read.observe(t >= BALL_FOLLOW_DELAY, Some(b), Vec3::ZERO);
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
        // A fielder holding the ball chest-high, still, doesn't reopen the
        // call (it is made once).
        read.observe(
            true,
            Some(BallState {
                pos: Vec3::new(5.0, 1.2, 60.0),
                vel: Vec3::ZERO,
                spin: Vec3::ZERO,
            }),
            Vec3::ZERO,
        );
        assert_eq!(read.grounder(), Some(want), "the call never changes");
        assert!(read.landed(), "every ball here came down");
    }
    // Nothing is decided before the plate hold ends, or without a ball.
    let mut read = PlayRead::default();
    read.observe(false, Some(launch(-6.0, 12.0, 36.0)), Vec3::ZERO);
    read.observe(true, None, Vec3::ZERO);
    assert_eq!(read.grounder(), None);
}

/// The swing itself is never read as a bounce: a pitch coming down into
/// the zone and leaving on a fly turns upward well above the bounce
/// height.
#[test]
fn the_swing_is_not_a_bounce() {
    let mut read = PlayRead::default();
    let pitch = BallState {
        pos: Vec3::new(0.0, 0.5, 0.3),
        vel: Vec3::new(0.0, -3.0, -38.0),
        spin: Vec3::ZERO,
    };
    read.observe(false, Some(pitch), Vec3::ZERO);
    let mut fly = launch(30.0, 0.0, 40.0);
    fly.pos.y = 0.5;
    read.observe(true, Some(fly), Vec3::ZERO);
    assert_eq!(read.grounder(), Some(false));
    assert!(!read.landed());
}

/// A fly that comes down and takes a high hop is done with the fielder
/// cam: the read marks it landed at the first touchdown.
#[test]
fn a_fly_is_landed_after_its_first_touchdown() {
    let dt = 1.0 / 60.0;
    let mut b = launch(35.0, 0.0, 34.0);
    let mut read = PlayRead::default();
    let mut t = 0.0;
    let mut was_up = false;
    while b.pos.y > 0.0 || t < 0.1 {
        fly_step(&mut b, dt);
        t += dt;
        read.observe(t >= BALL_FOLLOW_DELAY, Some(b), Vec3::ZERO);
        if b.pos.y > 2.0 {
            was_up = true;
            assert!(!read.landed(), "landed in the air at {t:.2}");
        }
    }
    assert!(was_up);
    fly_step(&mut b, dt);
    read.observe(true, Some(b), Vec3::ZERO);
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
