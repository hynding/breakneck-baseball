//! End-to-end: the effects pass (graphics plan task 13). Each moment effect
//! fires in a running game when its cue arrives, the rain splashes and the
//! wind flags follow the tier, and the motes alive never pass the tier's
//! particle cap. (The walk-off confetti is checked in `home_run_moment`,
//! which plays a real walk-off.)

use crate::common;

use bevy::prelude::*;

use breakneck_baseball::game::animation::{AnimClip, Playing};
use breakneck_baseball::game::ball::PitchEvent;
use breakneck_baseball::game::field::{WindFlag, foul_line_spans};
use breakneck_baseball::game::flow::{LiveBallEvent, PitchCaughtEvent};
use breakneck_baseball::game::fx::{
    ChalkPuff, DirtSpray, GlovePop, LiveParticles, Particle, RainSplash,
};
use breakneck_baseball::game::player::CatcherRole;
use breakneck_baseball::game::quality::{ActiveQuality, QualityTier};
use breakneck_baseball::game::rules::{PITCH_DISTANCE, PitchKind, pitch_velocity_kind};
use breakneck_baseball::game::settings::{GraphicsQuality, Settings};
use breakneck_baseball::game::variant::FieldSpec;
use breakneck_baseball::game::weather::Weather;

use common::{headless_app, start_game};

fn set_tier(app: &mut App, tier: QualityTier) {
    app.insert_resource(ActiveQuality {
        setting: GraphicsQuality::Auto,
        tier,
        stepped: false,
    });
}

fn count<T: Component>(app: &mut App) -> usize {
    let world = app.world_mut();
    world.query_filtered::<(), With<T>>().iter(world).count()
}

/// Runs up to `frames` updates and says whether any `T` mote was seen.
fn saw<T: Component>(app: &mut App, frames: usize) -> bool {
    (0..frames).any(|_| {
        app.update();
        count::<T>(app) > 0
    })
}

fn game(weather: Weather, tier: QualityTier) -> App {
    let mut app = headless_app();
    app.world_mut().resource_mut::<Settings>().weather = weather;
    start_game(&mut app, KeyCode::Digit1);
    set_tier(&mut app, tier);
    // Half a second (the harness steps 1/240 s): motes spawned before the
    // tier was set have retired.
    for _ in 0..120 {
        app.update();
    }
    app
}

#[test]
fn a_slide_and_a_dive_kick_up_dirt() {
    for clip in [AnimClip::Slide, AnimClip::Dive] {
        let mut app = game(Weather::Fair, QualityTier::Low);
        let world = app.world_mut();
        let rig = world
            .query_filtered::<Entity, With<CatcherRole>>()
            .single(world)
            .expect("a catcher");
        world.entity_mut(rig).insert(Playing::new(clip));
        assert!(
            saw::<DirtSpray>(&mut app, 240),
            "no dirt spray off the {clip:?}"
        );
    }
}

#[test]
fn a_hard_fastball_pops_the_mitt_and_a_soft_one_does_not() {
    let mut app = game(Weather::Fair, QualityTier::Low);
    // Real pitches, as the pitcher throws them.
    let pitch = |app: &mut App, kind: PitchKind| {
        let velocity = pitch_velocity_kind(kind, Vec2::ZERO, PITCH_DISTANCE, 1.0);
        app.world_mut().write_message(PitchEvent {
            velocity,
            spin: Vec3::ZERO,
        });
        app.update();
        app.world_mut().write_message(PitchCaughtEvent);
        saw::<GlovePop>(app, 3)
    };
    assert!(
        !pitch(&mut app, PitchKind::Changeup),
        "a changeup popped the mitt"
    );
    assert!(
        !pitch(&mut app, PitchKind::Sinker),
        "a sinker popped the mitt"
    );
    assert!(
        pitch(&mut app, PitchKind::Fastball),
        "a fastball didn't pop the mitt"
    );
}

#[test]
fn a_ball_on_the_line_kicks_up_chalk() {
    let mut app = game(Weather::Fair, QualityTier::Low);
    let field = app.world().resource::<FieldSpec>().clone();
    // On the first-base line's chalk, 50 m out.
    let (a, b) = foul_line_spans(&field)[0];
    let along = (b - a).normalize();
    let outward = Vec3::new(along.z, 0.0, -along.x) * along.x.signum();
    let on_line = a + along * 50.0;
    app.world_mut().write_message(LiveBallEvent::Landed {
        pos: on_line + outward * 3.0,
    });
    assert!(
        !saw::<ChalkPuff>(&mut app, 3),
        "chalk off a ball in foul ground"
    );
    app.world_mut()
        .write_message(LiveBallEvent::Landed { pos: on_line });
    assert!(
        saw::<ChalkPuff>(&mut app, 3),
        "no chalk off a ball on the line"
    );
}

#[test]
fn rain_splashes_from_medium_up() {
    let mut low = game(Weather::Rain, QualityTier::Low);
    assert!(!saw::<RainSplash>(&mut low, 60), "Low splashed");
    let mut medium = game(Weather::Rain, QualityTier::Medium);
    assert!(saw::<RainSplash>(&mut medium, 60), "Medium never splashed");
    let mut dry = game(Weather::Fair, QualityTier::High);
    assert!(!saw::<RainSplash>(&mut dry, 60), "a dry day splashed");
}

#[test]
fn the_wind_flags_follow_the_tier() {
    let mut app = game(Weather::Windy, QualityTier::Medium);
    for (tier, shown) in [
        (QualityTier::Medium, true),
        (QualityTier::Low, false),
        (QualityTier::High, true),
    ] {
        set_tier(&mut app, tier);
        app.update();
        let world = app.world_mut();
        let seen: Vec<bool> = world
            .query_filtered::<&Visibility, With<WindFlag>>()
            .iter(world)
            .map(|v| *v != Visibility::Hidden)
            .collect();
        assert_eq!(seen.len(), 3, "two foul poles and the scoreboard");
        assert!(seen.iter().all(|&s| s == shown), "{tier:?}: {seen:?}");
    }
}

/// A rainy game at each tier: the ledger matches the motes on the field,
/// and they never pass the tier's cap.
#[test]
fn the_particle_cap_holds() {
    for tier in [QualityTier::Low, QualityTier::Medium, QualityTier::High] {
        let mut app = game(Weather::Rain, tier);
        let cap = tier.budget().particle_cap;
        // Rain plus a fastball popping the mitt every frame: on Low and
        // Medium that asks for more motes than the cap holds.
        app.world_mut().write_message(PitchEvent {
            velocity: pitch_velocity_kind(PitchKind::Fastball, Vec2::ZERO, PITCH_DISTANCE, 1.0),
            spin: Vec3::ZERO,
        });
        let mut peak = 0;
        for _ in 0..240 {
            app.world_mut().write_message(PitchCaughtEvent);
            app.update();
            let motes = count::<Particle>(&mut app);
            let ledger = app.world().resource::<LiveParticles>().0;
            assert_eq!(motes, ledger, "{tier:?}: ledger drifted");
            assert!(motes <= cap, "{tier:?}: {motes} over {cap}");
            peak = peak.max(motes);
        }
        if tier == QualityTier::High {
            assert!(peak > 0, "{tier:?}: nothing to count");
        } else {
            assert_eq!(peak, cap, "{tier:?}: the bursts never reached the cap");
        }
    }
}
