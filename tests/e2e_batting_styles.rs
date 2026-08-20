//! End-to-end coverage for the Swing Meter (Task C2) and PCI cursor (Task C4)
//! batting adapters.
//!
//! With `Settings::batting_style` set per human slot, a batter's swings route
//! through the matching arm of `batting::adapt_swings` instead of Classic
//! timing. Staged at-bats prove the routing and grading against the *real* flow
//! spine (`ContactEvent` / `HitEvent`), all pitches and swings driven from the
//! `DriveGame` schedule.
//!
//! Swing Meter (stages 0–2):
//!
//!   * **Routing proof** — a bare `action` edge with the button *not held*
//!     produces NO swing (a Classic batter would have swung on the edge): the
//!     pitch is taken for a called strike, no `ContactEvent`. Proof the
//!     settings row routed away from Classic.
//!   * **Release swing** — hold from delivery, release once the live swing
//!     timing lands in the solid band → a `ContactEvent` graded better than
//!     `FoulTip` and a `HitEvent` (a real ball in play).
//!   * **Hold-through** — hold and never release → the ball crosses the late
//!     swing edge and forces a swing that grades `Whiff` (the spec's "still
//!     holding past the FoulTip window = a swinging whiff").
//!
//! PCI cursor (stages 3–4):
//!
//!   * **Off-center cursor** — hold the stick to glide the cursor well off the
//!     ball for the whole flight, then press with good timing → the grade is
//!     degraded below Solid purely by the cursor-to-ball distance (not timing).
//!   * **Dead-center cursor** — neutral aim keeps the cursor at zone center,
//!     press right at the plate → the full (unshrunk) windows still land a
//!     top-grade Perfect/Solid, proving the shrink is distance-driven.
//!
//! Only the *input timing* (and, for PCI, the cursor steering) is scripted; the
//! graded outcomes fall out of the same pure rules the Classic e2e leans on.

mod common;

use bevy::prelude::*;
use bevy_rapier3d::prelude::Velocity;

use breakneck_baseball::game::ball::{Baseball, HitEvent};
use breakneck_baseball::game::flow::{ContactEvent, Phase, Play};
use breakneck_baseball::game::input::Intents;
use breakneck_baseball::game::rules::ContactQuality;
use breakneck_baseball::game::settings::{BattingStyle, Settings};
use breakneck_baseball::game::{GameState, ScoreBoard};

use common::{DriveGame, headless_app, run_until, start_game};

/// Generous per-stage budget: steal windows can gate a pitch once a runner
/// reaches base, and a mistimed swing may burn an extra pitch or two.
const STAGE_FRAMES: u64 = 15_000;

/// The swing-timing error (ms) the release stage aims for: comfortably inside
/// the tuned solid window (90 ms) so the graded quality beats `FoulTip`. The
/// meter fires the frame the button is released, so the batter releases the
/// first frame the ball's live `dt` reaches this band.
const RELEASE_DT_MS: f32 = -80.0;

/// The swing-timing target (ms) the PCI stages press at: near the plate
/// (`dt ≈ 0`) so timing is *not* the limiting factor — stage 3's degraded grade
/// is then attributable to the cursor distance alone, and stage 4's dead-center
/// swing lands the top grade. Same live-`dt` band read the meter release uses.
const PCI_PRESS_DT_MS: f32 = -6.0;

/// How many staged at-bats run in total (3 meter + 2 PCI).
const STAGE_COUNT: usize = 5;

#[derive(Resource, Default)]
struct Stage(usize);

/// First graded quality and whether a ball went into play, per stage.
#[derive(Resource, Default)]
struct Captured {
    quality: [Option<ContactQuality>; STAGE_COUNT],
    hit: [bool; STAGE_COUNT],
}

/// Pitches every PrePitch (straightaway) and drives the batting side's meter
/// input per stage. All input is written directly here (after `PreUpdate`'s
/// `gather_intents`, before the flow `Update` chain) — the same seam the CPU
/// uses.
fn drive(
    stage: Res<Stage>,
    state: Res<State<GameState>>,
    play: Option<Res<Play>>,
    score: Option<Res<ScoreBoard>>,
    mut intents: ResMut<Intents>,
    ball: Query<(&Transform, &Velocity), With<Baseball>>,
) {
    if *state.get() != GameState::Playing {
        return;
    }
    let (Some(play), Some(score)) = (play, score) else {
        return;
    };
    intents.home = default();
    intents.away = default();
    let fielding = score.fielding_team();
    let batting = score.batting_team();

    match play.phase {
        Phase::PrePitch => {
            intents.get_mut(fielding).action = true;
        }
        Phase::Pitch => match stage.0 {
            // Routing proof: a bare edge, never held. Classic would swing;
            // the meter reads `action_held` (false) and takes the pitch.
            0 => {
                intents.get_mut(batting).action = true;
            }
            // Release swing: hold to load while the ball is still out front,
            // then drop the button the first frame the live timing reaches the
            // solid band — which fires the meter's swing that same frame.
            1 => {
                let in_band = ball.get_single().is_ok_and(|(t, v)| {
                    let vz = v.linvel.z.min(-f32::EPSILON);
                    let dt = 1000.0 * t.translation.z / vz; // == flow::swing_dt_ms
                    dt >= RELEASE_DT_MS
                });
                intents.get_mut(batting).action_held = !in_band;
            }
            // Hold-through: keep loading and never release. The forced swing
            // fires when the ball crosses the late swing edge → a whiff.
            2 => {
                intents.get_mut(batting).action_held = true;
            }
            // PCI off-center: hold the stick UP the whole flight so the cursor
            // glides to the top of the zone, well off a mid-height pitch, and
            // press near the plate. Good timing + a far cursor → the distance
            // shrink degrades the grade below Solid.
            3 => {
                intents.get_mut(batting).aim = Vec2::new(0.0, 1.0);
                if in_press_band(&ball) {
                    intents.get_mut(batting).action = true;
                }
            }
            // PCI dead-center: neutral aim leaves the cursor at zone center;
            // press right at the plate. Full (unshrunk) windows → top grade.
            4 if in_press_band(&ball) => {
                intents.get_mut(batting).action = true;
            }
            _ => {}
        },
        _ => {}
    }
}

/// True the first frame the ball's live swing timing reaches [`PCI_PRESS_DT_MS`]
/// (recomputed here exactly like `flow::swing_dt_ms`), so a PCI press lands near
/// the plate rather than at a fixed z.
fn in_press_band(ball: &Query<(&Transform, &Velocity), With<Baseball>>) -> bool {
    ball.get_single().is_ok_and(|(t, v)| {
        let vz = v.linvel.z.min(-f32::EPSILON);
        let dt = 1000.0 * t.translation.z / vz;
        dt >= PCI_PRESS_DT_MS
    })
}

fn capture(
    stage: Res<Stage>,
    mut contact_ev: EventReader<ContactEvent>,
    mut hit_ev: EventReader<HitEvent>,
    mut cap: ResMut<Captured>,
) {
    let s = stage.0.min(STAGE_COUNT - 1);
    for ev in contact_ev.read() {
        if cap.quality[s].is_none() {
            cap.quality[s] = Some(ev.quality);
        }
    }
    for _ in hit_ev.read() {
        cap.hit[s] = true;
    }
}

fn advance(app: &mut App, stage: usize, what: &str, milestone: impl FnMut(&mut App) -> bool) {
    app.world_mut().resource_mut::<Stage>().0 = stage;
    let reached = run_until(app, STAGE_FRAMES, milestone);
    let s = app.world().resource::<ScoreBoard>();
    assert!(
        reached.is_some(),
        "stage {stage} ({what}) never reached its milestone \
         (inning {} top={} outs={} balls={} strikes={})",
        s.inning,
        s.top_of_inning,
        s.outs,
        s.balls,
        s.strikes
    );
}

#[test]
fn swing_meter_and_pci_cursor_route_and_grade() {
    // Isolate settings persistence to a per-process temp file BEFORE the app
    // boots: this test mutates `Settings`, and `persist_settings` would
    // otherwise write `SwingMeter` into the shared platform config dir and
    // corrupt every other test that boots expecting the default Classic style.
    let dir = std::env::temp_dir().join(format!("bb-e2e-styles-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::set_var("BREAKNECK_SETTINGS_PATH", dir.join("settings.json")) };

    let mut app = headless_app();
    app.init_resource::<Stage>();
    app.init_resource::<Captured>();
    app.add_systems(DriveGame, (drive, capture));
    // Two-player mode leaves both teams keyboard-driven (not CPU), so the
    // batting side routes through the settings row rather than Classic.
    start_game(&mut app, KeyCode::Digit2);
    // Both human slots bat with the meter, so whichever team is up uses it.
    app.world_mut().resource_mut::<Settings>().batting_style = [BattingStyle::SwingMeter; 2];

    // Stage 0: a bare action edge (no hold) takes the pitch for a called
    // strike — a Classic batter would have swung, so no swing here proves the
    // meter routing.
    advance(&mut app, 0, "routing proof: take on a bare edge", |app| {
        let s = app.world().resource::<ScoreBoard>();
        s.strikes >= 1 || s.balls >= 1
    });
    assert!(
        app.world().resource::<Captured>().quality[0].is_none(),
        "a bare `action` edge under the Swing Meter must NOT swing \
         (any ContactEvent here means the settings row still routed to Classic)"
    );

    // Stage 1: hold, release in the solid band → a solid/perfect ball in play.
    advance(&mut app, 1, "release swing lands solid contact", |app| {
        let cap = app.world().resource::<Captured>();
        matches!(
            cap.quality[1],
            Some(ContactQuality::Solid | ContactQuality::Perfect)
        ) && cap.hit[1]
    });

    // Stage 2: hold and never release → forced swinging strike.
    advance(&mut app, 2, "hold-through forces a whiff", |app| {
        app.world().resource::<Captured>().quality[2] == Some(ContactQuality::Whiff)
    });

    let cap = app.world().resource::<Captured>();
    assert!(
        matches!(
            cap.quality[1],
            Some(ContactQuality::Solid | ContactQuality::Perfect)
        ),
        "release-swing quality must beat FoulTip, got {:?}",
        cap.quality[1]
    );
    assert!(
        cap.hit[1],
        "the released solid swing must put a ball in play"
    );
    assert_eq!(
        cap.quality[2],
        Some(ContactQuality::Whiff),
        "holding past the late swing edge must force a swinging whiff"
    );

    // Switch both human slots to the PCI cursor and run the cursor-distance
    // stages. Setting both slots keeps the proof independent of which side is
    // batting after the meter stages advanced the game.
    app.world_mut().resource_mut::<Settings>().batting_style = [BattingStyle::PciCursor; 2];

    // Stage 3: cursor gliding to the top of the zone, well off a mid-height
    // pitch, with good timing → the grade is dragged below Solid by distance.
    advance(
        &mut app,
        3,
        "off-center PCI cursor degrades the grade",
        |app| app.world().resource::<Captured>().quality[3].is_some(),
    );
    let off_center = app.world().resource::<Captured>().quality[3];
    assert!(
        matches!(
            off_center,
            Some(ContactQuality::FoulTip | ContactQuality::Weak | ContactQuality::Whiff)
        ),
        "a far PCI cursor with good timing must grade below Solid \
         (distance feeds grading), got {off_center:?}"
    );

    // Stage 4: cursor parked dead-center, press at the plate → the unshrunk
    // windows still land the top grade, in clear contrast to stage 3.
    advance(
        &mut app,
        4,
        "dead-center PCI cursor lands top grade",
        |app| {
            let cap = app.world().resource::<Captured>();
            matches!(
                cap.quality[4],
                Some(ContactQuality::Perfect | ContactQuality::Solid)
            ) && cap.hit[4]
        },
    );

    let cap = app.world().resource::<Captured>();
    assert!(
        matches!(
            cap.quality[4],
            Some(ContactQuality::Perfect | ContactQuality::Solid)
        ),
        "a dead-center PCI cursor must reach the top grade (Perfect at 0 miss), \
         got {:?}",
        cap.quality[4]
    );
    assert!(
        cap.hit[4],
        "the dead-center PCI swing must put a ball in play"
    );

    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::remove_var("BREAKNECK_SETTINGS_PATH") };
    let _ = std::fs::remove_dir_all(&dir);
}
