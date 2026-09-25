//! Change-detection hygiene: every resource a repaint is gated on must stay
//! *clean* while a pitch is in the air.
//!
//! `ScoreBoard` and `Bases` are read behind `is_changed()` guards all over
//! the presentation layer — `hud::update_{score_text,inning_text,count_dots,
//! base_ring}` (TODO 78, added to stop full UI relayouts), the team-colour and
//! identity painters in `player::rig`, `jersey::dress_jerseys` (a `String`
//! clone per jersey quad), and `runner::sync_runners`. A system that merely
//! *borrows* one of them mutably marks it changed: `&mut` on a `ResMut` goes
//! through `DerefMut`, which calls `set_changed()`. So hoisting a `&mut score`
//! above the branch that actually needs it — e.g. building a `flow::Umpire`
//! at the top of `pitch_live` instead of inside each arm that makes a call —
//! silently dirties both resources on every frame of the pitch flight and
//! defeats every one of those guards.
//!
//! Nothing else in the suite can see that: `set_changed()` alters no value, so
//! the outcome of every play is identical either way and all the correctness
//! tests stay green. This probe watches the ticks directly.

mod common;

use bevy::prelude::*;

use breakneck_baseball::game::ScoreBoard;
use breakneck_baseball::game::flow::{Phase, Play};
use breakneck_baseball::game::input::Controllers;
use breakneck_baseball::game::roster::Rosters;
use breakneck_baseball::game::rules::{Bases, BattingOrder};
use breakneck_baseball::game::scenario::{self, Scenario};
use breakneck_baseball::game::settings::Settings;
use breakneck_baseball::game::theme::Theme;

use common::{deterministic_headless_app, run_until, start_game};

/// Long enough to cover many full pitches at 240 Hz without being slow.
const MAX_FRAMES: u64 = 60_000;

/// Mid-flight frames to see before judging. A single pitch spans hundreds of
/// frames at 240 Hz, so this is several complete deliveries.
const MIN_SAMPLES: u32 = 400;

/// Share of in-flight frames allowed to report a dirty resource.
///
/// Measured at 0.0% for all seven today: a pitch legitimately changes the
/// scoreboard *once*, at the judged call, and that lands on a `Result` frame
/// rather than a `Pitch` one. The regression this guards drove `ScoreBoard`
/// and `Bases` to 100%. A quarter is a wide berth that leaves room for a
/// future rule that genuinely writes mid-flight while still failing
/// unmistakably on an every-frame spurious borrow.
const MAX_DIRTY_FRACTION: f64 = 0.25;

/// Every resource that something in `present`/`meta` gates repaint work on.
/// A pitch in flight should leave all of them alone.
const GUARDED: [&str; 7] = [
    "ScoreBoard",
    "Bases",
    "BattingOrder",
    "Rosters",
    "Theme",
    "Settings",
    "Controllers",
];

#[derive(Resource, Default)]
struct DirtyTally {
    in_flight: u32,
    dirty: [u32; GUARDED.len()],
}

/// Samples the change ticks once per frame, after every `Update` system has
/// run. Only frames with a pitch actually in flight count — the whole point
/// is that a delivery in progress applies no rules until it is judged.
#[allow(clippy::too_many_arguments)]
fn tally(
    play: Res<Play>,
    score: Res<ScoreBoard>,
    bases: Res<Bases>,
    order: Res<BattingOrder>,
    rosters: Res<Rosters>,
    theme: Res<Theme>,
    settings: Res<Settings>,
    controllers: Res<Controllers>,
    mut tally: ResMut<DirtyTally>,
) {
    if play.phase != Phase::Pitch {
        return;
    }
    tally.in_flight += 1;
    let seen = [
        score.is_changed(),
        bases.is_changed(),
        order.is_changed(),
        rosters.is_changed(),
        theme.is_changed(),
        settings.is_changed(),
        controllers.is_changed(),
    ];
    for (slot, hit) in tally.dirty.iter_mut().zip(seen) {
        if hit {
            *slot += 1;
        }
    }
}

#[test]
fn a_pitch_in_flight_leaves_every_repaint_guarded_resource_clean() {
    let mut app = deterministic_headless_app();
    app.init_resource::<DirtyTally>();
    app.add_systems(Last, tally);
    start_game(&mut app, KeyCode::Digit1); // human Home vs CPU Away

    // Bottom of the 1st: Home (the silent human) bats and the CPU pitches on
    // its own. In the top half the human is on defence and nothing presses to
    // deliver, so the machine would sit in `PrePitch` forever and the probe
    // would never see a pitch at all.
    let staged = Scenario {
        name: "change-detection bottom 1",
        top: false,
        ..Default::default()
    };
    scenario::apply_to_world(app.world_mut(), &staged).expect("scenario applies");

    let reached = run_until(&mut app, MAX_FRAMES, |app| {
        app.world().resource::<DirtyTally>().in_flight >= MIN_SAMPLES
    });

    let t = app.world().resource::<DirtyTally>();
    assert!(
        reached.is_some(),
        "only saw {} in-flight frames in {MAX_FRAMES}; expected at least {MIN_SAMPLES}",
        t.in_flight,
    );

    let mut offenders = Vec::new();
    for (name, dirty) in GUARDED.iter().zip(t.dirty) {
        let frac = f64::from(dirty) / f64::from(t.in_flight);
        println!(
            "{name:>13}: {dirty:>4}/{} in-flight frames dirty ({:.1}%)",
            t.in_flight,
            frac * 100.0
        );
        if frac > MAX_DIRTY_FRACTION {
            offenders.push(format!("{name} ({:.0}%)", frac * 100.0));
        }
    }
    assert!(
        offenders.is_empty(),
        "these resources were marked changed on in-flight frames: {}. Something is taking \
         a `&mut` borrow it does not write through — `DerefMut` on a `ResMut` calls \
         `set_changed()`, which defeats the repaint guards keyed on them.",
        offenders.join(", "),
    );
}
