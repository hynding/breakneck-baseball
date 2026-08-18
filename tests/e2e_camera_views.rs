//! End-to-end: cycling the at-bat camera view with **V** actually changes the
//! resource, and the occluder auto-hide really flips the catcher's root
//! `Visibility` component in the live scene — not just in the pure predicate
//! unit tests in `camera.rs`. Standard variant only: it's the only one with
//! a catcher (front yard has none, see `variant.rs`).

mod common;

use bevy::prelude::*;

use breakneck_baseball::game::camera::{CameraMode, DuelView};
use breakneck_baseball::game::flow::Phase;
use breakneck_baseball::game::player::CatcherRole;

use common::{headless_app, run_until, start_game, tap_key};

const MAX_FRAMES: u64 = 20_000;

fn catcher_visibility(app: &mut App) -> Visibility {
    *app.world_mut()
        .query_filtered::<&Visibility, With<CatcherRole>>()
        .single(app.world())
}

#[test]
fn cycling_v_changes_view_and_toggles_the_catchers_visibility() {
    let mut app = headless_app();
    start_game(&mut app, KeyCode::Digit2);

    // Wait for a dead ball in the duel (pre-pitch): this is when the view
    // framing — and its occlusion check — applies.
    let ready = run_until(&mut app, MAX_FRAMES, |app| {
        app.world()
            .resource::<breakneck_baseball::game::flow::Play>()
            .phase
            == Phase::PrePitch
    });
    assert!(ready.is_some(), "never reached a PrePitch dead ball");

    // Default view: catcher POV. The duel eye sits fractionally *inside*
    // the catcher's silhouette (see `FieldSpec::duel_eye`), so the
    // dedicated catcher-POV arm of `hide_occluders` hides him outright
    // while the duel framing holds.
    assert_eq!(*app.world().resource::<DuelView>(), DuelView::CatcherPov);
    assert_eq!(
        catcher_visibility(&mut app),
        Visibility::Hidden,
        "catcher POV must hide the catcher (the eye sits inside his silhouette)"
    );

    // V: behind-pitcher — the reference shot that must keep him visible.
    tap_key(&mut app, KeyCode::KeyV);
    assert_eq!(*app.world().resource::<DuelView>(), DuelView::BehindPitcher);
    assert_eq!(
        catcher_visibility(&mut app),
        Visibility::Inherited,
        "behind-pitcher must keep the catcher visible per the reference shot"
    );

    // V: batting zoom — close enough behind the plate that the catcher
    // blocks the sightline and must be auto-hidden.
    tap_key(&mut app, KeyCode::KeyV);
    assert_eq!(*app.world().resource::<DuelView>(), DuelView::BattingZoom);
    assert_eq!(
        catcher_visibility(&mut app),
        Visibility::Hidden,
        "batting zoom must hide the catcher blocking the sightline"
    );

    // V: broadcast plate — far enough away that nothing needs hiding.
    tap_key(&mut app, KeyCode::KeyV);
    assert_eq!(
        *app.world().resource::<DuelView>(),
        DuelView::BroadcastPlate
    );
    assert_eq!(catcher_visibility(&mut app), Visibility::Inherited);

    // V wraps back to catcher POV.
    tap_key(&mut app, KeyCode::KeyV);
    assert_eq!(*app.world().resource::<DuelView>(), DuelView::CatcherPov);
}

/// A rig hidden for a Broadcast-mode occluding view must not stay hidden
/// once the player switches to Orbit: Orbit looks through a completely
/// different eye/target, so the occlusion decision must be re-evaluated
/// (and cleared) the moment the camera mode changes, not just when the
/// `DuelView` changes.
#[test]
fn switching_to_orbit_restores_a_view_hidden_catcher() {
    let mut app = headless_app();
    start_game(&mut app, KeyCode::Digit2);

    let ready = run_until(&mut app, MAX_FRAMES, |app| {
        app.world()
            .resource::<breakneck_baseball::game::flow::Play>()
            .phase
            == Phase::PrePitch
    });
    assert!(ready.is_some(), "never reached a PrePitch dead ball");

    // Cycle to BattingZoom (CatcherPov -> BehindPitcher -> BattingZoom).
    tap_key(&mut app, KeyCode::KeyV);
    tap_key(&mut app, KeyCode::KeyV);
    assert_eq!(*app.world().resource::<DuelView>(), DuelView::BattingZoom);
    assert_eq!(
        catcher_visibility(&mut app),
        Visibility::Hidden,
        "batting zoom should have hidden the catcher before the mode switch"
    );

    // C: Broadcast -> Orbit. The DuelView resource is untouched (still
    // BattingZoom), but the active camera is no longer looking through its
    // axis, so the catcher must come back.
    tap_key(&mut app, KeyCode::KeyC);
    assert_eq!(*app.world().resource::<CameraMode>(), CameraMode::Orbit);
    assert_eq!(*app.world().resource::<DuelView>(), DuelView::BattingZoom);
    assert_eq!(
        catcher_visibility(&mut app),
        Visibility::Inherited,
        "orbit must restore real rig visibility even with an occluding DuelView still selected"
    );
}
