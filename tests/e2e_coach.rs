//! End-to-end Coach gate: the observer watches full scripted and CPU-driven
//! innings and the suite asserts **zero violation-grade findings** — the
//! Coach's checks are the always-on referee for fielder/runner choreography.
//!
//! Checks with substantive, known-broken behavior are downgraded to
//! warn-only via [`KNOWN_ISSUES`]; the allowlist is printed loudly on every
//! run and each entry must link its TODO.md item.

mod common;

use bevy::prelude::*;

use breakneck_baseball::game::ball::Baseball;
use breakneck_baseball::game::coach::{CheckId, CoachReport, Severity};
use breakneck_baseball::game::flow::{Phase, Play};
use breakneck_baseball::game::input::{Controllers, InputSource, Intents};
use breakneck_baseball::game::variant::Ruleset;
use breakneck_baseball::game::{GameState, ScoreBoard, Team};

use common::{DriveGame, headless_app, run_until, start_game, tap_key};

/// Checks whose violations are currently warn-only. Every entry must name
/// its TODO.md item; an empty list means the whole board is green.
const KNOWN_ISSUES: &[(CheckId, &str)] = &[];

/// Prints the allowlist (loudly) and asserts that no violation outside it
/// fired. Late/Style findings are reported but never fail the gate.
fn assert_coach_clean(app: &App, context: &str) {
    let report = app.world().resource::<CoachReport>();
    eprintln!("── Coach report ({context}) ── {} samples", report.samples);
    for check in CheckId::ALL {
        let (v, l, s) = (
            report.count(check, Severity::Violation),
            report.count(check, Severity::Late),
            report.count(check, Severity::Style),
        );
        if v + l + s > 0 {
            eprintln!("  {:<20} violations={v} late={l} style={s}", check.label());
        }
    }
    if !KNOWN_ISSUES.is_empty() {
        eprintln!("!! KNOWN_ISSUES allowlist in effect — these checks are warn-only:");
        for (check, todo) in KNOWN_ISSUES {
            eprintln!(
                "!!   {:<20} ({} violations) — {todo}",
                check.label(),
                report.count(*check, Severity::Violation)
            );
        }
    }
    let bad: Vec<_> = report
        .violations()
        .filter(|f| !KNOWN_ISSUES.iter().any(|(c, _)| *c == f.check))
        .collect();
    assert!(
        bad.is_empty(),
        "{context}: {} violation-grade Coach finding(s):\n{:#?}",
        bad.len(),
        bad
    );
}

/// The scripted mixed game from `e2e_full_game`: Away takes three
/// strikeouts (catcher-receives exercised on every pitch), Home hits a
/// deterministic walk-off homer (settlement through the full trot).
fn scripted_drive(
    state: Res<State<GameState>>,
    mut intents: ResMut<Intents>,
    play: Option<Res<Play>>,
    score: Option<Res<ScoreBoard>>,
    ball: Query<&Transform, With<Baseball>>,
) {
    if *state.get() != GameState::Playing {
        return;
    }
    let (Some(play), Some(score)) = (play, score) else {
        return;
    };
    intents.home = default();
    intents.away = default();
    match play.phase {
        Phase::PrePitch => {
            intents.get_mut(score.fielding_team()).action = true;
        }
        Phase::Pitch if score.batting_team() == Team::Home => {
            if let Ok(t) = ball.get_single() {
                intents.home.aim = Vec2::new(0.0, 1.0);
                if t.translation.z <= 0.45 && t.translation.z >= 0.0 {
                    intents.home.action = true;
                }
            }
        }
        _ => {}
    }
}

fn pin_classic_contact_windows(app: &mut App) {
    let mut r = app.world_mut().resource_mut::<Ruleset>();
    r.batting.perfect_ms = 40.0;
    r.batting.solid_ms = 90.0;
    r.batting.foul_ms = 140.0;
    r.batting.exit_solid = 1.0;
    r.batting.exit_perfect = 1.25;
}

#[test]
fn scripted_game_runs_clean_under_the_coach() {
    let mut app = headless_app();
    app.add_systems(DriveGame, scripted_drive);
    tap_key(&mut app, KeyCode::KeyI); // 9 innings -> 1
    start_game(&mut app, KeyCode::Digit2);
    pin_classic_contact_windows(&mut app);

    let finished = run_until(&mut app, 100_000, |app| {
        *app.world().resource::<State<GameState>>().get() == GameState::GameOver
    });
    assert!(finished.is_some(), "scripted game never finished");
    assert_coach_clean(&app, "scripted takes + walk-off HR");
}

/// A full CPU half-inning: both slots CPU-driven, so grounders, flies,
/// steals, throws, and every fielding assignment run under the Coach's eye.
#[test]
fn cpu_inning_runs_clean_under_the_coach() {
    let mut app = headless_app();
    start_game(&mut app, KeyCode::Digit1);
    *app.world_mut().resource_mut::<Controllers>() = Controllers {
        home: InputSource::Cpu,
        away: InputSource::Cpu,
        ..Controllers::default()
    };

    // Run to the bottom half: a complete CPU half-inning of live defense.
    let flipped = run_until(&mut app, 400_000, |app| {
        let s = app.world().resource::<ScoreBoard>();
        !s.top_of_inning || s.inning > 1
    });
    assert!(flipped.is_some(), "CPU half-inning never completed");
    assert_coach_clean(&app, "CPU vs CPU half-inning");
}
