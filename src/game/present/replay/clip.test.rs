use super::*;

use crate::game::variant::VariantId;

fn e(n: u32) -> Entity {
    Entity::from_raw_u32(n).unwrap()
}

fn sample(t: f32, x: f32, entities: &[u32]) -> Sample {
    Sample {
        t,
        ball: Some(Vec3::new(x, 1.0, 0.0)),
        poses: entities
            .iter()
            .map(|&n| Pose {
                entity: e(n),
                pos: Vec3::new(x, 0.0, n as f32),
                rot: Quat::from_rotation_y(x),
            })
            .collect(),
    }
}

/// The ring keeps only its window and reads back between samples.
#[test]
fn the_ring_keeps_its_window_and_interpolates() {
    let mut rec = Recording::new(2.0);
    for i in 0..=120 {
        let t = i as f32 / SAMPLE_HZ;
        assert!(rec.due(t));
        rec.push(sample(t, t, &[3, 1, 2]));
    }
    let (first, last) = rec.span().unwrap();
    assert!((last - 4.0).abs() < 1e-4);
    assert!(last - first <= 2.0 + 1e-4, "{first}..{last}");
    // Halfway between two samples.
    let t = 3.0 + 0.5 / SAMPLE_HZ;
    let m = rec.at(t).unwrap();
    assert!((m.ball.unwrap().x - t).abs() < 1e-3);
    assert_eq!(m.poses.len(), 3);
    // Sorted by entity for lookups.
    assert!(m.poses.windows(2).all(|w| w[0].entity < w[1].entity));
    let p = m.poses.iter().find(|p| p.entity == e(2)).unwrap();
    assert!((p.pos.x - t).abs() < 1e-3);
    assert!(p.rot.angle_between(Quat::from_rotation_y(t)) < 1e-3);
    // Outside the span clamps to its ends.
    assert!((rec.at(100.0).unwrap().ball.unwrap().x - last).abs() < 1e-4);
    assert!((rec.at(-5.0).unwrap().ball.unwrap().x - first).abs() < 1e-4);
}

/// Not every frame is a sample, and a clock that goes back (a new game)
/// starts the ring over.
#[test]
fn sampling_is_paced_and_a_new_clock_restarts() {
    let mut rec = Recording::new(5.0);
    rec.push(sample(1.0, 0.0, &[1]));
    assert!(!rec.due(1.0 + 0.2 / SAMPLE_HZ));
    assert!(rec.due(1.0 + 1.0 / SAMPLE_HZ));
    rec.push(sample(2.0, 0.0, &[1]));
    assert!(rec.due(0.5));
    rec.push(sample(0.5, 0.0, &[1]));
    assert_eq!(rec.span(), Some((0.5, 0.5)));
}

/// A rig spawned mid-play is absent before it existed; one despawned
/// holds its last pose.
#[test]
fn rigs_that_come_and_go_read_back_sensibly() {
    let mut rec = Recording::new(5.0);
    rec.push(sample(0.0, 0.0, &[1, 2]));
    rec.push(sample(1.0, 1.0, &[1, 3]));
    let m = rec.at(0.5).unwrap();
    let mut ids: Vec<_> = m.poses.iter().map(|p| p.entity.index_u32()).collect();
    ids.sort();
    assert_eq!(ids, vec![1, 2]);
    let gone = m.poses.iter().find(|p| p.entity == e(2)).unwrap();
    assert_eq!(gone.pos.x, 0.0);
    let mut later: Vec<_> = rec
        .at(1.0)
        .unwrap()
        .poses
        .iter()
        .map(|p| p.entity.index_u32())
        .collect();
    later.sort();
    assert_eq!(later, vec![1, 3]);
}

/// The plan plays the play from the dugout, then the second angle — slow
/// around the key moment on High — all inside the recording.
#[test]
fn the_plan_covers_the_play_per_tier() {
    let span = (0.0, 12.0);
    let low = plan(span, 5.0, 8.0, 5.0, 1, false);
    assert_eq!(low.len(), 1);
    assert_eq!(low[0].angle, Angle::Dugout);
    assert!((low[0].from - (5.0 - LEAD_IN)).abs() < 1e-5);
    assert!((low[0].to - (8.0 + TAIL)).abs() < 1e-5);

    let medium = plan(span, 5.0, 8.0, 5.0, 2, false);
    assert_eq!(medium.len(), 2);
    assert_eq!(medium[1].angle, Angle::CentreField);
    assert_eq!(medium[1].speed, 1.0);

    let high = plan(span, 5.0, 8.0, 5.0, 2, true);
    assert_eq!(high[1].speed, SLOW_SPEED);
    assert!(high[1].from >= high[0].from && high[1].to <= high[0].to);
    assert!((high[1].secs() - 2.0 * SLOW_HALF / SLOW_SPEED).abs() < 1e-4);

    // A recording that starts after contact clamps; one too short plays
    // nothing.
    let clamped = plan((6.0, 9.0), 5.0, 8.0, 5.0, 1, false);
    assert_eq!(clamped[0].from, 6.0);
    assert!(plan((8.3, 8.4), 5.0, 8.0, 5.0, 2, true).is_empty());
}

/// Replay cameras stand above the field, look at the ball, and keep a
/// sane lens wherever the ball is.
#[test]
fn replay_cameras_frame_the_ball() {
    for variant in [VariantId::Standard, VariantId::FrontYard] {
        let field = variant.field();
        for angle in [Angle::Dugout, Angle::CentreField] {
            for ball in [
                None,
                Some(Vec3::new(0.0, 1.0, 0.0)),
                Some(Vec3::new(-20.0, 30.0, 70.0)),
                Some(Vec3::new(0.0, 0.0, field.fence_line)),
            ] {
                let (eye, target, fov) = camera(angle, &field, ball);
                assert!(eye.y > 5.0, "{angle:?} {eye}");
                assert!(target.y >= 0.3);
                assert!((MIN_FOV..=MAX_FOV).contains(&fov));
                if let Some(b) = ball {
                    assert!(target.distance(b.with_y(b.y.max(0.3))) < 1e-4);
                }
            }
        }
    }
}
