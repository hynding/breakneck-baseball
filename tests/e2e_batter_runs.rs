//! The batter runs on contact — every live-ball scenario.
//!
//! docs/BASEBALL.md ("Baserunning after contact", convention 5): *"The
//! batter always runs on contact (fair-ball assumption); the engine resets
//! him on a foul."* Three angles:
//!
//! - `pulled_liner_predicts_foul_forward` (pure): proves a human batter can
//!   produce a **forward, predicted-foul** live ball with the shipped
//!   physics (full-stick pull + late-window timing) — the CPU's Classic
//!   windows never produce one, which is why a long CPU-only probe run
//!   (2026-08-28: 50 fair contacts, 0 predicted-foul) missed the bug where
//!   `batter_runs` skipped `Live { fair: false }` and left the batter rooted.
//! - `batter_runs_on_organic_cpu_contacts`: a CPU-vs-CPU stretch — every
//!   organically produced fair ball has the run-out rig ([`BatterGhost`])
//!   on the basepath within tolerance of contact.
//! - `predicted_foul_forward_ball_is_run_out`: the historically broken
//!   class, staged deterministically at the event seam — the ghost must run
//!   on a forward predicted-foul ball, retire when it lands foul, and the
//!   real batter must be back in the box immediately after.

mod common;

use bevy::prelude::*;
use breakneck_baseball::game::flow::{BallInPlayEvent, LiveBallEvent};
use breakneck_baseball::game::rules::{self, ContactKind};
use breakneck_baseball::game::runner::BatterGhost;
use breakneck_baseball::game::settings::BattingStyle;
use breakneck_baseball::game::variant::{FieldSpec, VariantId};

use common::{MatrixMode, headless_app, run_until, start_matrix_game};

/// Virtual seconds allowed between contact and the ghost being on the
/// basepath: `RUN_OUT_DELAY` (0.15 s) plus scheduling slack.
const RUN_TOLERANCE: f64 = 0.6;

fn ghost_present(app: &mut App) -> bool {
    app.world_mut()
        .query_filtered::<(), With<BatterGhost>>()
        .iter(app.world())
        .next()
        .is_some()
}

/// A full-stick pull with late-window timing sends the predicted landing
/// forward but outside the fair wedge — the exact contact class a human
/// batter produces ripping one down the line. Pinned so the synthetic
/// scenario below stays honest about being reachable in real play.
#[test]
fn pulled_liner_predicts_foul_forward() {
    let field = VariantId::Standard.field();
    let ruleset = VariantId::Standard.rules();
    // Full pull aim with an *early* solid-window swing: the aim's spray and
    // the timing-driven pull yaw point the same way, past the foul line
    // (a late swing cancels the spray instead and centers the ball).
    let aim = Vec2::new(1.0, 0.0);
    let dt_ms = -ruleset.batting.solid_ms;
    let base = rules::hit_velocity(0.4, aim);
    let velocity =
        rules::apply_contact_quality(base, rules::ContactQuality::Solid, dt_ms, &ruleset);
    let (landing, _hang) = rules::predict_landing(
        velocity,
        rules::hit_spin(velocity),
        breakneck_baseball::game::ball::BALL_DRAG_FACTOR,
        breakneck_baseball::game::ball::MAGNUS_FACTOR,
    );
    let kind = rules::classify_contact(landing, &field);
    assert_eq!(
        kind,
        ContactKind::Live { fair: false },
        "full pull + late solid timing should predict foul, got {kind:?} at {landing:?}"
    );
    assert!(
        landing.z > 1.0,
        "the predicted-foul ball must still be a *forward* ball, landed z {}",
        landing.z
    );
}

#[test]
fn batter_runs_on_organic_cpu_contacts() {
    let mut app = headless_app();
    start_matrix_game(
        &mut app,
        MatrixMode::CpuVsCpu,
        BattingStyle::ClassicTiming,
        "balanced",
    );

    let mut cursor = bevy::ecs::event::EventCursor::<BallInPlayEvent>::default();
    let mut fair = 0u32;
    let mut failures: Vec<String> = Vec::new();
    let mut pending: Option<(f64, String)> = None;

    run_until(&mut app, 120_000, |app| {
        let now = app.world().resource::<Time<Virtual>>().elapsed_secs_f64();
        let ghost = ghost_present(app);

        let contacts: Vec<(ContactKind, Vec3)> = {
            let events = app.world().resource::<Events<BallInPlayEvent>>();
            cursor
                .read(events)
                .map(|ev| (ev.kind, ev.landing))
                .collect()
        };
        for (kind, landing) in contacts {
            if let ContactKind::Live { fair: true } = kind {
                fair += 1;
                pending = Some((
                    now + RUN_TOLERANCE,
                    format!("fair ball to ({:.1}, {:.1})", landing.x, landing.z),
                ));
            }
        }

        if let Some((deadline, desc)) = &pending {
            if ghost {
                pending = None;
            } else if now > *deadline {
                failures.push(format!("batter never ran: {desc}"));
                pending = None;
            }
        }

        fair >= 8
    });

    println!("organic stretch: fair contacts observed = {fair}");
    assert!(fair >= 8, "thin sample — only {fair} fair contacts");
    assert!(
        failures.is_empty(),
        "batter run-out convention violated:\n{failures:#?}"
    );
}

#[test]
fn predicted_foul_forward_ball_is_run_out() {
    let mut app = headless_app();
    start_matrix_game(
        &mut app,
        MatrixMode::CpuVsCpu,
        BattingStyle::ClassicTiming,
        "balanced",
    );
    // Let the game settle into its first PrePitch with rigs spawned.
    run_until(&mut app, 5_000, |app| {
        app.world()
            .resource::<breakneck_baseball::game::flow::Play>()
            .phase
            == breakneck_baseball::game::flow::Phase::PrePitch
    })
    .expect("game never reached PrePitch");

    // The staged contact: the pure-test class — forward, predicted foul.
    // Injected at the event seam `batter_runs` consumes; the fielding/flow
    // race is irrelevant to the choreography under test.
    let field = app.world().resource::<FieldSpec>().clone();
    let landing = Vec3::new(-30.0, 0.0, 20.0); // forward, well outside the wedge
    assert!(!rules::is_fair(landing, &field) && landing.z > 1.0);
    let contact_class = rules::contact_class(landing, 1.6, &field);
    app.world_mut().send_event(BallInPlayEvent {
        kind: ContactKind::Live { fair: false },
        landing,
        contact_class,
    });

    let ran = run_until(&mut app, 300, ghost_present);
    assert!(
        ran.is_some(),
        "a forward predicted-foul ball must be run out — the batter stayed \
         rooted in the box (docs/BASEBALL.md, baserunning convention 5)"
    );

    // It lands foul: the engine resets him — ghost retired, batter back.
    app.world_mut()
        .send_event(LiveBallEvent::Landed { pos: landing });
    let retired = run_until(&mut app, 300, |app| !ghost_present(app));
    assert!(
        retired.is_some(),
        "the ghost must retire once the ball lands foul"
    );
    let batter_visible = app
        .world_mut()
        .query_filtered::<&Visibility, With<breakneck_baseball::game::player::Batter>>()
        .iter(app.world())
        .all(|v| *v != Visibility::Hidden);
    assert!(
        batter_visible,
        "the real batter must be back in the box after the foul reset"
    );
}
