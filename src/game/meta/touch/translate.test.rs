//! Unit tests for [`super`] — the translate module.

use super::*;

const SIZE: Vec2 = Vec2::new(1280.0, 720.0);

/// A minimal app that feeds *real* `TouchInput` events through Bevy's
/// input plugin into [`read_touch`] — the gesture state machine itself
/// (claiming, re-anchoring, re-arming, stickiness), not just the pure
/// helpers, under test.
fn translator_app(scheme: TouchScheme, home_bats: bool) -> (App, Entity) {
    use crate::game::flow::Play;
    use bevy::window::PrimaryWindow;

    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::input::InputPlugin));
    let window = app
        .world_mut()
        .spawn((
            Window {
                resolution: (SIZE.x, SIZE.y).into(),
                ..Default::default()
            },
            PrimaryWindow,
        ))
        .id();
    let mut play = Play::default();
    play.phase = Phase::Pitch;
    let score = crate::game::ScoreBoard {
        top_of_inning: !home_bats,
        ..Default::default()
    };
    let controllers = Controllers {
        touch_team: Some(Team::Home),
        ..Controllers::default()
    };
    app.insert_resource(play)
        .insert_resource(score)
        .insert_resource(controllers)
        .insert_resource(Settings {
            touch_scheme: scheme,
            ..Settings::default()
        })
        .init_resource::<TouchGestures>()
        .init_resource::<TouchIntent>()
        .init_resource::<crate::game::subs::PauseTapped>()
        .add_systems(PreUpdate, read_touch.after(bevy::input::InputSystem));
    (app, window)
}

fn send_touch(
    app: &mut App,
    window: Entity,
    phase: bevy::input::touch::TouchPhase,
    id: u64,
    pos: Vec2,
) {
    app.world_mut().send_event(bevy::input::touch::TouchInput {
        phase,
        position: pos,
        window,
        force: None,
        id,
    });
}

#[test]
fn translator_tap_scheme_swings_from_a_real_touch_event() {
    use bevy::input::touch::TouchPhase;
    let (mut app, window) = translator_app(TouchScheme::Tap, true);
    send_touch(
        &mut app,
        window,
        TouchPhase::Started,
        7,
        Vec2::new(1100.0, 150.0),
    );
    app.update();
    let out = app.world().resource::<TouchIntent>().0;
    assert!(out.action, "a tap during the pitch is the swing");
    assert!(
        out.aim.x > 0.0 && out.aim.y > 0.0,
        "upper-right tap aims upper-right, got {:?}",
        out.aim
    );
    assert!(
        app.world().resource::<TouchGestures>().touch_seen(),
        "first contact must set the device-detection bit"
    );
}

/// One flick gesture: a downward reset move (its own frame — down never
/// fires) followed by a large upward move. The 450 px sweep keeps the
/// measured speed above `FLICK_TRIGGER_HPS` even when a loaded machine
/// stretches the real dt between updates to ~0.5 s — a 100 px sweep
/// flaked at >126 ms frame gaps under parallel test load.
fn flick_up(app: &mut App, window: Entity, id: u64, x: f32) {
    use bevy::input::touch::TouchPhase;
    send_touch(app, window, TouchPhase::Moved, id, Vec2::new(x, 650.0));
    app.update();
    send_touch(app, window, TouchPhase::Moved, id, Vec2::new(x, 200.0));
    app.update();
}

#[test]
fn translator_flick_rearms_every_pitch_for_a_resting_thumb() {
    use crate::game::flow::Play;
    use bevy::input::touch::TouchPhase;
    let (mut app, window) = translator_app(TouchScheme::Flick, true);
    // Rest the thumb, then flick up.
    send_touch(
        &mut app,
        window,
        TouchPhase::Started,
        7,
        Vec2::new(640.0, 650.0),
    );
    app.update();
    flick_up(&mut app, window, 7, 640.0);
    assert!(
        app.world().resource::<TouchIntent>().0.action,
        "first flick fires"
    );
    // Another upward sweep in the same pitch: spent.
    flick_up(&mut app, window, 7, 640.0);
    assert!(
        !app.world().resource::<TouchIntent>().0.action,
        "one swing per pitch"
    );
    // The pitch resolves and a new one arrives; the thumb never lifted.
    app.world_mut().resource_mut::<Play>().phase = Phase::Result;
    app.update();
    app.world_mut().resource_mut::<Play>().phase = Phase::Pitch;
    app.update();
    flick_up(&mut app, window, 7, 640.0);
    assert!(
        app.world().resource::<TouchIntent>().0.action,
        "entering a new pitch must re-arm the flick for a resting thumb"
    );
}

#[test]
fn translator_zone_pad_claims_sticks_and_swings() {
    use bevy::input::touch::TouchPhase;
    let (mut app, window) = translator_app(TouchScheme::ZonePad, true);
    let pad_spot = pad_rect(SIZE).center();
    send_touch(&mut app, window, TouchPhase::Started, 1, pad_spot);
    app.update();
    let cursor = app.world().resource::<TouchIntent>().0.cursor;
    let resting = crate::game::batting::PciState::center();
    assert!(
        cursor.is_some_and(|c| (c - resting).length() < 1e-3),
        "pad-center touch aims the zone center, got {cursor:?}"
    );
    // Lift the finger: the cursor sticks at the last aimed spot.
    send_touch(&mut app, window, TouchPhase::Ended, 1, pad_spot);
    app.update();
    assert!(
        app.world().resource::<TouchIntent>().0.cursor.is_some(),
        "the absolute cursor is sticky after the finger lifts"
    );
    // The SWING button fires the press edge.
    send_touch(
        &mut app,
        window,
        TouchPhase::Started,
        2,
        swing_button_rect(SIZE).center(),
    );
    app.update();
    assert!(
        app.world().resource::<TouchIntent>().0.action,
        "a SWING button press is the swing edge"
    );
}

#[test]
fn translator_single_frame_tap_still_acts_and_reveals_the_device() {
    use bevy::input::touch::TouchPhase;
    // A tap that presses AND releases between two frames (a fast real
    // tap, or a synthetic browser tap) must still fire the bare-tap
    // action and flip the device-detection bit — it only ever appears
    // in `just_pressed`, never among the held touches.
    let (mut app, window) = translator_app(TouchScheme::Off, false);
    let pos = Vec2::new(400.0, 360.0);
    send_touch(&mut app, window, TouchPhase::Started, 9, pos);
    send_touch(&mut app, window, TouchPhase::Ended, 9, pos);
    app.update();
    assert!(
        app.world().resource::<TouchGestures>().touch_seen(),
        "an instantaneous tap must still count as a seen touch"
    );
    assert!(
        app.world().resource::<TouchIntent>().0.action,
        "an instantaneous tap is still the bare-tap action"
    );
}

#[test]
fn translator_defense_action_tap_keeps_the_drag_aim() {
    use bevy::input::touch::TouchPhase;
    // Tap scheme on DEFENSE: the second-finger action tap must not eat
    // the first finger's drag aim on the release frame — flow samples
    // the pitch/throw aim at exactly that action edge.
    let (mut app, window) = translator_app(TouchScheme::Tap, false);
    send_touch(
        &mut app,
        window,
        TouchPhase::Started,
        1,
        Vec2::new(600.0, 300.0),
    );
    app.update();
    // Full-deflection downward drag.
    send_touch(
        &mut app,
        window,
        TouchPhase::Moved,
        1,
        Vec2::new(600.0, 450.0),
    );
    app.update();
    send_touch(
        &mut app,
        window,
        TouchPhase::Started,
        2,
        Vec2::new(400.0, 300.0),
    );
    app.update();
    let out = app.world().resource::<TouchIntent>().0;
    assert!(out.action, "the second-finger tap is the action press");
    assert!(
        out.aim.y < -0.9,
        "the drag aim must survive the action frame, got {:?}",
        out.aim
    );
}

#[test]
fn translator_windup_tap_swings_without_dropping_the_send_drag() {
    use crate::game::flow::Play;
    use bevy::input::touch::TouchPhase;
    // Tap scheme, batting, runner-send drag held through the windup: an
    // anticipatory swing tap must not clobber the send's Down aim even
    // for one frame (flow re-reads the send from aim every frame).
    let (mut app, window) = translator_app(TouchScheme::Tap, true);
    app.world_mut().resource_mut::<Play>().phase = Phase::WindUp;
    send_touch(
        &mut app,
        window,
        TouchPhase::Started,
        1,
        Vec2::new(600.0, 300.0),
    );
    app.update();
    send_touch(
        &mut app,
        window,
        TouchPhase::Moved,
        1,
        Vec2::new(600.0, 450.0),
    );
    app.update();
    send_touch(
        &mut app,
        window,
        TouchPhase::Started,
        2,
        Vec2::new(640.0, 100.0),
    );
    app.update();
    let out = app.world().resource::<TouchIntent>().0;
    assert!(out.action, "the windup tap still emits the swing edge");
    assert!(
        out.aim.y < -0.9,
        "the held send-drag owns the aim, got {:?}",
        out.aim
    );
}

#[test]
fn translator_flick_second_thumb_cannot_double_swing_the_same_pitch() {
    use crate::game::flow::Play;
    use bevy::input::touch::TouchPhase;
    let (mut app, window) = translator_app(TouchScheme::Flick, true);
    // Two resting thumbs; A flicks and lifts, B is adopted mid-pitch.
    // A touches down a frame before B: two same-frame touchdowns reach
    // the claim loop in `Touches`' random HashMap order, and this test
    // needs A to be the tracked stick.
    send_touch(
        &mut app,
        window,
        TouchPhase::Started,
        1,
        Vec2::new(500.0, 650.0),
    );
    app.update();
    send_touch(
        &mut app,
        window,
        TouchPhase::Started,
        2,
        Vec2::new(800.0, 650.0),
    );
    app.update();
    flick_up(&mut app, window, 1, 500.0);
    assert!(
        app.world().resource::<TouchIntent>().0.action,
        "thumb A's flick fires"
    );
    send_touch(
        &mut app,
        window,
        TouchPhase::Ended,
        1,
        Vec2::new(500.0, 200.0),
    );
    app.update();
    // Thumb B (adopted) flicks in the same pitch: spent.
    flick_up(&mut app, window, 2, 800.0);
    assert!(
        !app.world().resource::<TouchIntent>().0.action,
        "one flick swing per pitch, whichever finger"
    );
    // Next pitch: the adopted thumb is re-armed like any other.
    app.world_mut().resource_mut::<Play>().phase = Phase::Result;
    app.update();
    app.world_mut().resource_mut::<Play>().phase = Phase::Pitch;
    app.update();
    flick_up(&mut app, window, 2, 800.0);
    assert!(
        app.world().resource::<TouchIntent>().0.action,
        "a new pitch re-arms the adopted thumb"
    );
}

#[test]
fn translator_zone_pad_swing_hold_survives_the_between_pitch_release() {
    use crate::game::flow::Play;
    use bevy::input::touch::TouchPhase;
    let (mut app, window) = translator_app(TouchScheme::ZonePad, true);
    // Press SWING during the pitch (the edge fires)…
    send_touch(
        &mut app,
        window,
        TouchPhase::Started,
        1,
        swing_button_rect(SIZE).center(),
    );
    app.update();
    assert!(app.world().resource::<TouchIntent>().0.action);
    // …hold it through the Result (the regions release between pitches)
    // and the next PrePitch (the handoff must re-acquire the button)…
    app.world_mut().resource_mut::<Play>().phase = Phase::Result;
    app.update();
    app.world_mut().resource_mut::<Play>().phase = Phase::PrePitch;
    app.update();
    // …then the next delivery fires the anticipatory press: the held
    // button must not be dead for the whole pitch.
    app.world_mut().resource_mut::<Play>().phase = Phase::Pitch;
    app.update();
    assert!(
        app.world().resource::<TouchIntent>().0.action,
        "a SWING hold kept across the between-pitch release fires at the delivery"
    );
}

#[test]
fn resolver_requires_a_seen_touchscreen() {
    // Selecting a scheme on the menu must not override a desktop
    // keyboard player's configured batting style with zero touches
    // ever seen — ownership needs `TouchGestures::seen`.
    let mut world = bevy::ecs::world::World::new();
    world.init_resource::<TouchGestures>();
    world.init_resource::<Touches>();
    world.insert_resource(Controllers::default());
    let mut system = bevy::ecs::system::IntoSystem::into_system(resolve_touch_owner);
    system.initialize(&mut world);
    system.run((), &mut world);
    assert_eq!(
        world.resource::<Controllers>().touch_team,
        None,
        "no touch seen: no owner"
    );
    world.resource_mut::<TouchGestures>().seen = true;
    system.run((), &mut world);
    assert_eq!(
        world.resource::<Controllers>().touch_team,
        Some(Team::Home),
        "a seen touchscreen grants the human Home slot ownership"
    );
}

#[test]
fn translator_zone_pad_leaves_the_cursor_to_the_keyboard_when_touch_free() {
    use bevy::input::touch::TouchPhase;
    // A touchscreen was seen once (the session bit), but no finger is
    // down and nothing aimed the pad this at-bat: the translator must
    // emit NO absolute cursor, or keyboard/stick PCI steering would be
    // locked out for the rest of the session by one incidental brush.
    let (mut app, window) = translator_app(TouchScheme::ZonePad, true);
    app.world_mut().resource_mut::<TouchGestures>().seen = true;
    app.update();
    assert_eq!(
        app.world().resource::<TouchIntent>().0.cursor,
        None,
        "touch-free frame: the velocity path must stay live"
    );
    // A finger resting off-pad center-pins (a stray finger's stick aim
    // must not velocity-integrate the swing cursor)…
    send_touch(
        &mut app,
        window,
        TouchPhase::Started,
        3,
        Vec2::new(900.0, 200.0),
    );
    app.update();
    assert!(
        app.world().resource::<TouchIntent>().0.cursor.is_some(),
        "a finger on the glass pins the cursor absolutely"
    );
    // …and lifting it hands steering back to the keyboard.
    send_touch(
        &mut app,
        window,
        TouchPhase::Ended,
        3,
        Vec2::new(900.0, 200.0),
    );
    app.update();
    app.update();
    assert_eq!(
        app.world().resource::<TouchIntent>().0.cursor,
        None,
        "all fingers up and no pad aim: keyboard steering returns"
    );
}

#[test]
fn translator_zone_pad_region_resting_fingers_do_not_center_pin() {
    use bevy::input::touch::TouchPhase;
    // The cycle-15 SWING-holder fix, one finger later: the pin filter
    // excludes region OWNERS by id, and must exclude roleless fingers
    // RESTING in a region by position too — the claim loop consumed
    // that brush ("it becomes no other input"), so it emits no stick
    // aim and cannot justify locking a keyboard steerer to center.
    let (mut app, window) = translator_app(TouchScheme::ZonePad, true);
    app.world_mut().resource_mut::<TouchGestures>().seen = true;
    let button = swing_button_rect(SIZE).center();
    // Separate frames: same-frame Started events reach the claim loop
    // in `Touches`' arbitrary HashMap order.
    send_touch(&mut app, window, TouchPhase::Started, 7, button);
    app.update();
    send_touch(&mut app, window, TouchPhase::Started, 8, button);
    app.update();
    let intent = &app.world().resource::<TouchIntent>().0;
    assert!(
        intent.action_held,
        "the first finger owns SWING and holds it"
    );
    assert_eq!(
        intent.cursor, None,
        "a roleless thumb resting on the owned button must not pin the cursor"
    );
}

#[test]
fn translator_zone_pad_wandered_stick_still_pins_the_cursor() {
    use bevy::input::touch::TouchPhase;
    // The carve-out above is for ROLELESS region-resters only: the
    // tracked stick keeps emitting aim inside a region (the wander
    // rule), so it must keep forcing the absolute snap there — carved
    // out, its drag velocity-integrated the swing cursor, the exact
    // drift the pin is documented to stop.
    let (mut app, window) = translator_app(TouchScheme::ZonePad, true);
    app.world_mut().resource_mut::<TouchGestures>().seen = true;
    // Touch down OFF every region (tracked as the stick)…
    send_touch(
        &mut app,
        window,
        TouchPhase::Started,
        9,
        Vec2::new(900.0, 200.0),
    );
    app.update();
    // …then wander INTO the pad rect while still held.
    send_touch(
        &mut app,
        window,
        TouchPhase::Moved,
        9,
        pad_rect(SIZE).center(),
    );
    app.update();
    assert!(
        app.world().resource::<TouchIntent>().0.cursor.is_some(),
        "the tracked stick inside a region must still force the absolute snap"
    );
}
