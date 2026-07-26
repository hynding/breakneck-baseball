//! End-to-end: cycling the at-bat camera view with **V** actually changes the
//! resource, and the occluder auto-hide really flips the catcher's root
//! `Visibility` component in the live scene — not just in the pure predicate
//! unit tests in `camera.rs`. Standard variant only: it's the only one with
//! a catcher (front yard has none, see `variant.rs`).

mod common;

use bevy::prelude::*;

use breakneck_baseball::game::camera::DuelView;
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

    // Default view: catcher POV. His own eye placement already keeps him
    // out of frame, and the occlusion system agrees: Inherited, not Hidden.
    assert_eq!(*app.world().resource::<DuelView>(), DuelView::CatcherPov);
    assert_eq!(
        catcher_visibility(&mut app),
        Visibility::Inherited,
        "catcher POV must not hide the catcher (he's already off-camera by eye placement)"
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
