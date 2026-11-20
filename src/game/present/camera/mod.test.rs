//! Unit tests for [`super`] — the camera module.

use super::*;

#[test]
fn duel_view_cycles_through_all_four_and_wraps() {
    let v = DuelView::default();
    assert_eq!(
        v,
        DuelView::BattingZoom,
        "the batting view is the default (TODO 100)"
    );
    let v = v.next();
    assert_eq!(v, DuelView::CatcherPov);
    let v = v.next();
    assert_eq!(v, DuelView::BehindPitcher);
    let v = v.next();
    assert_eq!(v, DuelView::BroadcastPlate);
    let v = v.next();
    assert_eq!(v, DuelView::BattingZoom);
}

/// CAMERA SHAKE scales the contact kick, and reduce motion stills it
/// whatever the shake setting says.
#[test]
fn camera_shake_and_reduce_motion_scale_the_contact_kick() {
    use crate::game::ball::HitEvent;
    use crate::game::settings::{CameraShake, Settings};

    let kick_for = |settings: Settings| {
        let mut app = App::new();
        app.add_message::<HitEvent>()
            .init_resource::<CameraKick>()
            .insert_resource(settings)
            .add_systems(Update, kick_on_hit);
        app.world_mut().write_message(HitEvent {
            velocity: Vec3::ONE,
        });
        app.update();
        app.world().resource::<CameraKick>().0
    };
    let with = |camera_shake, reduce_motion| Settings {
        camera_shake,
        reduce_motion,
        ..Default::default()
    };
    assert_eq!(kick_for(with(CameraShake::Full, false)), rigs::HIT_KICK);
    assert!((kick_for(with(CameraShake::Low, false)) - rigs::HIT_KICK * 0.4).length() < 1e-6);
    assert_eq!(kick_for(with(CameraShake::Off, false)), Vec3::ZERO);
    assert_eq!(
        kick_for(with(CameraShake::Full, true)),
        Vec3::ZERO,
        "reduce motion kicked"
    );
}
