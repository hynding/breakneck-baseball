//! The mode matrix: every control configuration × every batting style, a
//! short director-driven game each, with the Coach asserting throughout.
//!
//! {1P vs CPU, 2P} × {Classic, Swing Meter, PCI} plus one CPU-vs-CPU cell —
//! the CPU always bats Classic regardless of settings (asserted below), so
//! style does not multiply the attract-mode cell. Every slot drives through
//! the Director's `Intents` seam, which is what makes a future control
//! mechanism covered automatically.

mod common;

use bevy::app::App;
use breakneck_baseball::game::batting::style_for;
use breakneck_baseball::game::coach::CoachReport;
use breakneck_baseball::game::flow::{Phase, Play};
use breakneck_baseball::game::input::Controllers;
use breakneck_baseball::game::settings::{BattingStyle, Settings, TouchScheme};
use breakneck_baseball::game::{ScoreBoard, Team};

use common::{MatrixMode, headless_app, run_until, start_matrix_game};

/// Plays a matrix cell until `plays` plate outcomes have resolved (Result
/// entries), then asserts the game progressed, at least one swing was
/// judged when a scripted batter is at work, and the Coach saw zero
/// violation-grade findings.
fn run_cell(mode: MatrixMode, style: BattingStyle, plays: u32) {
    let mut app = headless_app();
    start_matrix_game(&mut app, mode, style, "balanced");
    drive_and_assert(
        &mut app,
        &format!("{mode:?}/{style:?}"),
        plays,
        !matches!(mode, MatrixMode::CpuVsCpu),
    );
}

/// The shared convergence loop + assertions (split from [`run_cell`] so the
/// touch-scheme cell can reuse it after its own settings setup).
fn drive_and_assert(app: &mut App, label: &str, plays: u32, expect_swing: bool) {
    let mut results = 0u32;
    let mut last_phase = Phase::PrePitch;
    let mut saw_swing = false;
    let done = run_until(app, 200_000, |app| {
        let play = app.world().resource::<Play>();
        if play.phase == Phase::Result && last_phase != Phase::Result {
            results += 1;
        }
        saw_swing |= play.last_contact_quality().is_some();
        last_phase = play.phase;
        results >= plays
    });
    assert!(
        done.is_some(),
        "{label}: only {results}/{plays} plays resolved"
    );
    if expect_swing {
        assert!(
            saw_swing,
            "{label}: the scripted batter never got a swing judged \
             — the style adapter is not translating the director's commit"
        );
    }
    let report = app.world().resource::<CoachReport>();
    let violations: Vec<_> = report.violations().collect();
    assert!(
        violations.is_empty(),
        "{label}: Coach violations:\n{violations:#?}"
    );
}

#[test]
fn one_player_classic() {
    run_cell(MatrixMode::OnePlayerVsCpu, BattingStyle::ClassicTiming, 6);
}

#[test]
fn one_player_meter() {
    run_cell(MatrixMode::OnePlayerVsCpu, BattingStyle::SwingMeter, 6);
}

#[test]
fn one_player_pci() {
    run_cell(MatrixMode::OnePlayerVsCpu, BattingStyle::PciCursor, 6);
}

#[test]
fn two_players_classic() {
    run_cell(MatrixMode::TwoPlayers, BattingStyle::ClassicTiming, 6);
}

#[test]
fn two_players_meter() {
    run_cell(MatrixMode::TwoPlayers, BattingStyle::SwingMeter, 6);
}

#[test]
fn two_players_pci() {
    run_cell(MatrixMode::TwoPlayers, BattingStyle::PciCursor, 6);
}

/// The absolute-cursor cell: the `zone-pad` script aims through the
/// `TeamIntent::cursor` channel a Zone Pad finger produces, graded by the
/// PCI adapter, with the Coach asserting throughout — the touch swing
/// path's plumbing minus the fingers. Also pins the ownership rule: a
/// Director-driven slot is never touch-owned (`resolve_touch_owner`), so a
/// persisted touch scheme cannot hijack a scripted slot's configured style.
#[test]
fn one_player_zone_pad_cursor_script() {
    let mut app = headless_app();
    start_matrix_game(
        &mut app,
        MatrixMode::OnePlayerVsCpu,
        BattingStyle::PciCursor,
        "zone-pad",
    );
    // A persisted scheme is present AND a touchscreen has been seen (the
    // test seam — without it the seen filter alone yields no owner and the
    // Director-exclusion assertion below is vacuous), but the scripted
    // slot must still not be touch-owned...
    app.world_mut().resource_mut::<Settings>().touch_scheme = TouchScheme::ZonePad;
    app.world_mut()
        .resource_mut::<breakneck_baseball::game::touch::TouchGestures>()
        .mark_seen();
    for _ in 0..4 {
        app.update();
    }
    {
        let world = app.world();
        let controllers = world.resource::<Controllers>();
        assert_eq!(
            controllers.touch_team, None,
            "a Director-driven slot must have no touch owner"
        );
        // ...so the *configured* PCI style applies, scheme notwithstanding.
        assert_eq!(
            style_for(Team::Home, controllers, world.resource::<Settings>()),
            BattingStyle::PciCursor,
        );
    }
    drive_and_assert(&mut app, "OnePlayerVsCpu/ZonePad-cursor", 6, true);
}

#[test]
fn cpu_vs_cpu_ignores_style() {
    let mut app = headless_app();
    // Deliberately configure a non-Classic style: the CPU must ignore it.
    start_matrix_game(
        &mut app,
        MatrixMode::CpuVsCpu,
        BattingStyle::PciCursor,
        "balanced",
    );
    // One director frame must run before routing applies; step a few.
    for _ in 0..4 {
        app.update();
    }
    {
        let world = app.world();
        let controllers = world.resource::<Controllers>();
        let settings = world.resource::<Settings>();
        for team in [Team::Home, Team::Away] {
            assert_eq!(
                style_for(team, controllers, settings),
                BattingStyle::ClassicTiming,
                "{team:?}: a CPU-driven slot must always bat Classic"
            );
        }
    }
    let mut results = 0u32;
    let mut last_phase = Phase::PrePitch;
    let done = run_until(&mut app, 200_000, |app| {
        let play = app.world().resource::<Play>();
        if play.phase == Phase::Result && last_phase != Phase::Result {
            results += 1;
        }
        last_phase = play.phase;
        results >= 6
    });
    assert!(
        done.is_some(),
        "CPU vs CPU: only {results}/6 plays resolved"
    );
    let score = app.world().resource::<ScoreBoard>();
    assert!(
        score.outs + score.balls + score.strikes > 0 || score.top_of_inning || score.inning >= 1,
        "attract mode never progressed"
    );
    let report = app.world().resource::<CoachReport>();
    let violations: Vec<_> = report.violations().collect();
    assert!(violations.is_empty(), "CPU vs CPU: {violations:#?}");
}
