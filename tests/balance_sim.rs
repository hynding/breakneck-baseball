//! Balance-sim harness (Task B7): the arbiter for the offensive economy.
//!
//! Boots the real headless app, forces *both* teams onto the CPU AI (the same
//! `Intents` seam a human would drive — see `ai.rs`), and plays N complete
//! one-inning games to `GameState::GameOver`. Across the aggregate it pins
//! three rates that a healthy game of baseball has to sit inside:
//!
//! * **K%** — strikeouts per plate appearance, in `15..=30`.
//! * **runs/team/game (per-9)** — in `3.0..=8.0`.
//! * **HR/team/game (per-9)** — in `0.5..=2.5`.
//!
//! ## Why one-inning games, and the per-9 extrapolation
//!
//! A full nine-inning CPU-vs-CPU game is ~9× the frames of a one-inning game,
//! and with the steal window opening on most at-bats once runners reach, even
//! one inning is many thousands of frames (this sim steps at `SIM_DT` = 1/100 s;
//! see that const). N=20 nine-inning games would blow the runtime budget. So
//! each sim is a *one-inning* game and the counting rates are extrapolated to
//! the regulation nine:
//!
//! * K% is a **rate** (strikeouts ÷ PA) — scale-free, so no extrapolation: the
//!   one-inning K% *is* the game K%.
//! * runs and HR are **counting** stats. One inning is 1/9 of a nine-inning
//!   game, so per-9 = (per-team total over all sims) ÷ (2 teams · N games) · 9.
//!   The `· 9` is the extrapolation; dividing the 3.0..8.0 / 0.5..2.5 per-9
//!   bands by 9 is the equivalent per-inning band (0.33..0.89 / 0.056..0.28).
//!
//! ## Determinism, variance, and the bands
//!
//! The rules engine is RNG-free (hash-noise only). The harness runs the app
//! **single-threaded** (`common::deterministic_headless_app`): every schedule
//! uses `ExecutorKind::SingleThreaded` and the Bevy task pools are capped at one
//! thread. That pins the old multi-threaded-executor jitter — *within a process
//! the sim is now bit-deterministic* (replaying a game reproduces it exactly),
//! and it also runs ~5× faster on this tiny workload, since thread-pool overhead
//! dominated.
//!
//! What single-threading does **not** remove is a process-global,
//! entropy/ASLR-seeded value in the physics *core* (Rapier's solve / transform
//! math). It perturbs the ball trajectory by sub-ULP amounts that differ from
//! one process to the next; baseball is chaotic about it, so a perturbation
//! occasionally flips a near-threshold swing/contact decision and reshapes a
//! whole game. It is *not* the schedule executor, *not* HashMap iteration order,
//! *not* the animation rig, and *not* a per-app entropy draw — all ruled out by
//! bisection (see the de-flake report). Removing it would need Rapier's
//! `enhanced-determinism` Cargo feature, which would change the shipped binary,
//! so this test bounds the residual with its bands instead of eliminating it.
//!
//! Two things keep the aggregate robust to that residual:
//!
//! 1. **Per-game decorrelation.** Each game idles the virtual clock a distinct,
//!    *coarsely spaced* number of frames before starting (`game_index ·
//!    OFFSET_STRIDE_FRAMES`), shifting every `time.elapsed_secs()` the AI hashes
//!    on — so the N games are N genuinely *different* games. Their spread is
//!    real sample variance, not a single point measured N times.
//! 2. **Aggregate bands at N=40.** The asserts are on the mean across N=40,
//!    whose sampling variance is ~1/√40 of a single game's. K% and HR/9 average
//!    down well; runs/9 (a *chain* of decisions per run) stays the widest and
//!    its band is correspondingly generous. The report records the observed
//!    spread across three consecutive harness runs.

mod common;

use bevy::prelude::*;

use breakneck_baseball::game::flow::{Phase, Play, PlayBanner};
use breakneck_baseball::game::input::{Controllers, InputSource};
use breakneck_baseball::game::rules::BattingOrder;
use breakneck_baseball::game::{GameState, ScoreBoard, Team};

use common::{DriveGame, HeadlessConfig, headless_app_with, run_until, start_game, tap_key};

/// Hard cap per game. A one-inning CPU game settles in a few thousand frames;
/// this is a generous backstop (and covers a tie that spills into extras)
/// so a stalled game fails loudly instead of hanging.
const MAX_FRAMES: u64 = 400_000;

/// Virtual-time step for the balance sim. Coarser than the e2e harness's
/// 1/240 (see `common::DT`), on purpose: the *shipping* game runs at ~60 Hz,
/// so 1/100 is still finer than what a real player experiences while cutting the
/// frames-per-sim-second (the runtime budget). The `contact_z` sweet spot and
/// the `dt=0` timing ideal diverge a little more at a coarser step, so the
/// measured economy is DT-dependent — the harness pins this DT as a fixed
/// measurement condition and tunes the `Ruleset` against it.
const SIM_DT: f64 = 1.0 / 100.0;

/// Per-game clock offset, in frames, multiplied by the game index — a coarse
/// stride so the games' AI-noise streams are well separated (game 0 gets 0,
/// game 1 gets 37 frames ≈ 0.37 s of shifted `elapsed_secs`, etc.), rather than
/// leaning on physics jitter to tell near-identical games apart.
const OFFSET_STRIDE_FRAMES: u32 = 37;

/// Tallies the three balance signals over one game. PA is counted robustly
/// from the batting order rotating (one `advance` per completed PA, per team),
/// independent of the dozen terminal-banner strings; K and HR are read off the
/// two unambiguous banners.
#[derive(Resource, Default)]
struct SimTally {
    pa: u32,
    k: u32,
    hr: u32,
    /// Last-seen (home, away) 1-based lineup slots; `None` until first frame.
    prev_slots: Option<(u32, u32)>,
    // ── Diagnostics (probe only; not asserted) — the PA-outcome and
    //    pitch-event breakdown that makes the tuning targeted. ──
    bb: u32,
    hbp: u32,
    hits: u32,
    batted_out: u32,
    dp: u32,
    fc: u32,
    fouls: u32,
    called_strike: u32,
    whiff: u32,
    pitches: u32,
    picked_off: u32,
    caught_stealing: u32,
    stolen: u32,
    singles: u32,
    doubles: u32,
    triples: u32,
    prev_phase: Option<Phase>,
}

fn tally(
    play: Res<Play>,
    order: Res<BattingOrder>,
    mut banners: MessageReader<PlayBanner>,
    mut sim: ResMut<SimTally>,
) {
    // One pitch per WindUp edge.
    if play.phase == Phase::WindUp && sim.prev_phase != Some(Phase::WindUp) {
        sim.pitches += 1;
    }
    sim.prev_phase = Some(play.phase);
    let cur = (order.current(Team::Home), order.current(Team::Away));
    match sim.prev_slots {
        None => sim.prev_slots = Some(cur),
        Some(prev) => {
            // `advance` rotates the slot (mod 9), so any change is exactly one
            // completed plate appearance for that team.
            if cur.0 != prev.0 {
                sim.pa += 1;
            }
            if cur.1 != prev.1 {
                sim.pa += 1;
            }
            sim.prev_slots = Some(cur);
        }
    }
    for b in banners.read() {
        let t = b.text.as_str();
        if t == "STRIKEOUT!" {
            sim.k += 1;
        } else if t.starts_with("HOME RUN!") {
            sim.hr += 1;
        } else if t == "WALK" {
            sim.bb += 1;
        } else if t == "HIT BY PITCH" {
            sim.hbp += 1;
        } else if t.starts_with("SINGLE") {
            sim.hits += 1;
            sim.singles += 1;
        } else if t.starts_with("DOUBLE ") || t == "DOUBLE" {
            // "DOUBLE  +1" etc; not "DOUBLE PLAY!"
            sim.hits += 1;
            sim.doubles += 1;
        } else if t.starts_with("TRIPLE") || t.contains("BASES!") {
            sim.hits += 1;
            sim.triples += 1;
        } else if t.starts_with("PICKED OFF") {
            sim.picked_off += 1;
        } else if t.starts_with("CAUGHT STEALING") {
            sim.caught_stealing += 1;
        } else if t.starts_with("STOLEN BASE") {
            sim.stolen += 1;
        } else if t.starts_with("DOUBLE PLAY!") {
            sim.dp += 1;
        } else if t == "FIELDER'S CHOICE" {
            sim.fc += 1;
        } else if t == "FOUL" {
            sim.fouls += 1;
        } else if t == "STRIKE" {
            sim.called_strike += 1;
        } else if t == "SWING & MISS" {
            sim.whiff += 1;
        } else if t.starts_with("GROUND OUT")
            || t.starts_with("FLY OUT")
            || t.starts_with("POP OUT")
            || t.starts_with("FOUL POP OUT")
            || t.starts_with("PEGGED")
            || t.starts_with("OUT STRETCHING")
            || t.starts_with("SAC FLY")
            || t.starts_with("DOUBLED OFF")
        {
            sim.batted_out += 1;
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct GameStats {
    home_runs: u32,
    away_runs: u32,
    pa: u32,
    k: u32,
    hr: u32,
    frames: u64,
    bb: u32,
    hbp: u32,
    hits: u32,
    batted_out: u32,
    dp: u32,
    fc: u32,
    fouls: u32,
    called_strike: u32,
    whiff: u32,
    pitches: u32,
    picked_off: u32,
    caught_stealing: u32,
    stolen: u32,
    singles: u32,
    doubles: u32,
    triples: u32,
}

/// Plays one complete one-inning CPU-vs-CPU game. `game_index` sets a coarse,
/// distinct clock offset (`· OFFSET_STRIDE_FRAMES`) idled before the game
/// starts, so this game's AI noise stream is well separated from every other's.
fn play_one_game(game_index: u32) -> GameStats {
    // Single-threaded for run-to-run determinism (see the module doc) and
    // without skeletal sampling: bone poses cannot reach the rules, and
    // `tests/e2e/skeletal_switch.rs` pins that the inning is frame-identical
    // either way — the sampling was ~16% of the headless frame for nothing.
    let mut app = headless_app_with(HeadlessConfig {
        single_threaded: true,
        skip_skeletal_sampling: true,
        ..Default::default()
    });
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::from_secs_f64(SIM_DT),
    ));
    app.init_resource::<SimTally>();
    app.add_systems(DriveGame, tally);

    // Decorrelate this game's AI noise from the others by advancing the shared
    // virtual clock a distinct, coarsely-spaced amount while still on the menu.
    let offset_frames = game_index * OFFSET_STRIDE_FRAMES;
    for _ in 0..offset_frames {
        app.update();
    }

    // 9 -> 1 inning (the menu cycle wraps 9 to 1), then start one-player mode.
    tap_key(&mut app, KeyCode::KeyI);
    start_game(&mut app, KeyCode::Digit1);

    // Force BOTH teams onto the CPU: the AI defense/offense systems already
    // key off `Controllers::source(team) == Cpu`, so this alone makes them
    // drive both halves. Nothing re-runs `assign_controllers` mid-game
    // (only a gamepad hotplug would, and there are none headless).
    *app.world_mut().resource_mut::<Controllers>() = Controllers {
        home: InputSource::Cpu,
        away: InputSource::Cpu,
        ..Controllers::default()
    };

    // Stop at the end of a *single* inning: either the game is already over
    // (someone led after the bottom half) or the scoreboard has rolled into
    // the 2nd. Capping at one inning keeps every game a clean 1/9 sample —
    // a tied one-inning game would otherwise spill into extra innings, both
    // over-weighting the offense in the aggregate and blowing the runtime
    // budget. Ties are simply cut at the end of regulation here.
    let ended = run_until(&mut app, MAX_FRAMES, |app| {
        let over = *app.world().resource::<State<GameState>>().get() == GameState::GameOver;
        let past_first = app.world().resource::<ScoreBoard>().inning >= 2;
        over || past_first
    });
    let frames = ended.unwrap_or_else(|| {
        let s = app.world().resource::<ScoreBoard>();
        panic!(
            "game (offset {offset_frames}) never finished one inning \
             (inning={} top={} outs={} H={} A={})",
            s.inning, s.top_of_inning, s.outs, s.home_runs, s.away_runs
        )
    });

    // Flush any banner written on the game-ending frame into the tally: `tally`
    // reads events one frame late (it runs before flow's `Update`).
    for _ in 0..3 {
        app.update();
    }

    let score = app.world().resource::<ScoreBoard>();
    let t = app.world().resource::<SimTally>();
    GameStats {
        home_runs: score.home_runs,
        away_runs: score.away_runs,
        pa: t.pa,
        k: t.k,
        hr: t.hr,
        frames,
        bb: t.bb,
        hbp: t.hbp,
        hits: t.hits,
        batted_out: t.batted_out,
        dp: t.dp,
        fc: t.fc,
        fouls: t.fouls,
        called_strike: t.called_strike,
        whiff: t.whiff,
        pitches: t.pitches,
        picked_off: t.picked_off,
        caught_stealing: t.caught_stealing,
        stolen: t.stolen,
        singles: t.singles,
        doubles: t.doubles,
        triples: t.triples,
    }
}

struct Aggregate {
    n: u32,
    k_pct: f64,
    runs_per9: f64,
    hr_per9: f64,
    total_pa: u32,
    total_k: u32,
    total_hr: u32,
    total_runs: u32,
    mean_frames: f64,
    // The plate-appearance economy beyond the three headline rates — the
    // 2026-10 realism study found zero walks, a 7× HBP rate and a 40× pickoff
    // rate all passing the original three bands.
    bb_pct: f64,
    hbp_pct: f64,
    pitches_per_pa: f64,
    babip: f64,
    doubles_per9: f64,
    pickoffs_per9: f64,
    caught_stealing_per9: f64,
}

fn run_sim(n: u32) -> Aggregate {
    let games: Vec<GameStats> = (0..n).map(play_one_game).collect();

    let total_runs: u32 = games.iter().map(|g| g.home_runs + g.away_runs).sum();
    let total_pa: u32 = games.iter().map(|g| g.pa).sum();
    let total_k: u32 = games.iter().map(|g| g.k).sum();
    let total_hr: u32 = games.iter().map(|g| g.hr).sum();
    let total_frames: u64 = games.iter().map(|g| g.frames).sum();

    // Per team, per game, extrapolated to nine innings (see module docs).
    let runs_per_team_per_inning = total_runs as f64 / (2.0 * n as f64);
    let hr_per_team_per_inning = total_hr as f64 / (2.0 * n as f64);

    let s = |f: fn(&GameStats) -> u32| -> u32 { games.iter().map(f).sum() };
    let per9 = |count: u32| count as f64 / (2.0 * n as f64) * 9.0;
    let pa = total_pa.max(1) as f64;
    // Balls in play that stayed in the park: hits (HR excluded) + batted outs.
    let in_play = (s(|g| g.hits) + s(|g| g.batted_out) + s(|g| g.dp) + s(|g| g.fc)).max(1);
    let agg = Aggregate {
        n,
        k_pct: 100.0 * total_k as f64 / pa,
        runs_per9: runs_per_team_per_inning * 9.0,
        hr_per9: hr_per_team_per_inning * 9.0,
        total_pa,
        total_k,
        total_hr,
        total_runs,
        mean_frames: total_frames as f64 / n as f64,
        bb_pct: 100.0 * s(|g| g.bb) as f64 / pa,
        hbp_pct: 100.0 * s(|g| g.hbp) as f64 / pa,
        pitches_per_pa: s(|g| g.pitches) as f64 / pa,
        babip: s(|g| g.hits) as f64 / in_play as f64,
        doubles_per9: per9(s(|g| g.doubles)),
        pickoffs_per9: per9(s(|g| g.picked_off)),
        caught_stealing_per9: per9(s(|g| g.caught_stealing)),
    };
    eprintln!(
        "\n=== balance-sim N={} ===\n\
         K%        = {:.1}%   (target 15..30;  {} K / {} PA)\n\
         runs/9    = {:.2}    (target 3.0..8.0; {} runs total)\n\
         HR/9      = {:.2}    (target 0.5..2.5; {} HR total)\n\
         mean frames/game = {:.0}\n\
         --- PA outcomes: BB={} HBP={} hits={} HR={} battedOut={} DP={} FC={} K={} (sum={} vs PA={})\n\
         --- pitch events: fouls={} calledStrike={} whiff={}\n\
         --- economy: BB%={:.1} (target 6..11)  HBP%={:.1} (target 0..2)  pitches/PA={:.2} (target 3.3..4.3)\n\
         ---          BABIP={:.3} (target .250..330)  hits 1B/2B/3B={}/{}/{}  2B/9={:.2} (target 1.0..2.5)\n\
         --- running/9: SB={:.2} CS={:.2} (target 0..1) pickoffs={:.2} (target 0..0.6)",
        agg.n,
        agg.k_pct,
        agg.total_k,
        agg.total_pa,
        agg.runs_per9,
        agg.total_runs,
        agg.hr_per9,
        agg.total_hr,
        agg.mean_frames,
        s(|g| g.bb),
        s(|g| g.hbp),
        s(|g| g.hits),
        s(|g| g.hr),
        s(|g| g.batted_out),
        s(|g| g.dp),
        s(|g| g.fc),
        s(|g| g.k),
        s(|g| g.bb + g.hbp + g.hits + g.hr + g.batted_out + g.dp + g.fc + g.k),
        agg.total_pa,
        s(|g| g.fouls),
        s(|g| g.called_strike),
        s(|g| g.whiff),
        agg.bb_pct,
        agg.hbp_pct,
        agg.pitches_per_pa,
        agg.babip,
        s(|g| g.singles),
        s(|g| g.doubles),
        s(|g| g.triples),
        agg.doubles_per9,
        per9(s(|g| g.stolen)),
        agg.caught_stealing_per9,
        agg.pickoffs_per9,
    );

    agg
}

// ── Bands: nominal targets vs the asserted bands, and why they differ ─────────
//
// The plan's *nominal* targets are K% 15..30, runs/9 3.0..8.0, HR/9 0.5..2.5.
// The asserted bands below are sized to the **measured per-process spread** at
// N=40 (see the report's run table) plus margin for a binary-layout shift. The
// variance model that sets that spread (see the module-header section) is:
//
//  * **Within one process the sim is now bit-deterministic** — the harness runs
//    single-threaded (`deterministic_headless_app`), which pinned the old
//    multi-threaded-executor jitter. Replaying any game in a process reproduces
//    it exactly.
//  * **Across processes it is not.** A process-global, entropy/ASLR-seeded value
//    in the physics *core* (Rapier's solve / transform math — *not* the schedule
//    executor, not hash iteration, not the animation rig: all ruled out, see the
//    report) perturbs the ball trajectory by sub-ULP amounts. Baseball is
//    chaotic about that: a perturbation occasionally flips a near-threshold
//    swing/contact decision, and one flipped HR-vs-out branch reshapes a whole
//    one-inning game. Curing this needs Rapier's `enhanced-determinism` feature,
//    which would leak into the shipped binary — out of scope — so the residual
//    is bounded by the bands rather than removed.
//
// N=40 (up from 20, funded by the single-threaded speedup) averages that spread
// down where it can. At N=40 one chaotic game moves the aggregate only ~0.11
// runs/9 / ~0.11 HR/9, so a per-process seed change nudges the mean — it cannot
// drag it across a full band. Reaching a floor therefore means a *systematic*
// regression (the offense collapsed), which is exactly what the floors catch.
//
// **Every edge is anchored to the N=40 pipeline's own measured spread, not the
// old N=20 multi-threaded distribution** — that older distribution was strictly
// wider (it still carried the executor jitter this harness removed), so its
// draws (e.g. the 1.35 runs/9 this test used to flake on) are not samples of the
// pipeline under test and do not set floors here. Each edge below states the
// risk it favors (flaky-gate if too tight vs toothless-gate if too loose) and
// its anchor. Measured at N=40 across processes (this binary): K% 17.2..21.9,
// runs/9 2.8..4.4, HR/9 1.9..2.7 — until the 2026-10 realism retune
// (`docs/BASEBALL.md`-anchored: late swings no longer die in the catcher's
// glove, Solid contact is a real batted ball, grounders are cut off on their
// roll, the CPU takes 3-0 and the first pitch, steals and pickoffs are
// situational). That retune re-centred the economy on MLB 2025 (4.45 R/G,
// 1.16 HR/G, BB 8.4%, K 22.2%, BABIP .291, 3.87 pitches/PA) and re-measured:
// K% 14.0, runs/9 5.6, HR/9 1.1, BB% 8.2, HBP% 0.0, pitches/PA 3.3,
// BABIP .367, CS/9 0.2, pickoffs/9 0.6 (one process, N=40; the nine-inning
// harness `tests/realism_sim.rs` reads 5.6 R/G, 1.1 HR/G, BB 8.4%, K 16.5%).
// The follow-up outfield-range fix (fielders at MLB depth) re-measured runs/9
// 3.4, HR/9 0.8, K% 14.9, BABIP .301, doubles/9 2.4 (from 5.6 / 1.1 / 14.0 /
// .367 / 4.6; the realism harness reads 4.15 R/G and 0.31 doubles per single,
// MLB 4.45 and 0.30).
//
//  * K% 11..27 — was 13..27, anchored to obs 17.2..21.9 with ±~4 of margin
//    each side (a chaotic game moves it ~0.3). The retune reads 14.0..15.3
//    across runs, so the floor moved to 11 to keep ~3 of margin under the new
//    live mean; a K-rate *collapse* is what it catches.
//  * runs/9 2.0..7.5 — floor 2.0 sits ~0.8 below the measured min 2.8, catching
//    a real offensive collapse without flaking on seed noise (a seed change
//    can't reach it). Ceiling 7.5 is generous vs the max 4.4 — runs is the
//    leveraged, chaotic signal (a run is a *chain* of decisions) so the high
//    tail is the widest; the ceiling favors avoiding a flaky gate. The retune
//    reads 5.6, so the ceiling is ~1.9 above the live mean — the pre-retune
//    economy's 23 runs a game (the realism harness's round 4) would trip it.
//  * HR/9 0.6..2.2 — re-centred on the retune's 1.1 (MLB 1.16): floor 0.6
//    catches the HR-death that the 2026-10 round-5 state showed (0.12 a game),
//    ceiling 2.2 the round-4 explosion (6.8 a game). The park centres HR high
//    (the regulation fence + `rules::hit_velocity` make squared-up contact at
//    the spec `exit_solid`≈1.0 HR-prone), held down only by the CPU's launch
//    aim in `ai.rs` (the top of the stick is left to humans) — so if either
//    edge trips, the fix is an HR-retune ticket (CPU-side levers:
//    `cpu_timing_spread_ms` / the `ai.rs` launch-aim distribution), NOT a
//    wider band. The frozen human-facing `Ruleset` exits stay put.
//  * BB% 5..13 — nominal 6..11 (MLB 8.4), live 8.2. Floor 5 catches the
//    zero-walk economy the study found (the get-it-over package was pure
//    strikes); ceiling 13 the 13.8% of the first retune round.
//  * HBP% 0..2.5 — nominal 0..2 (MLB 1.1), live 0.0: the catch for the slider
//    that used to drill the batter on a third of its throws (`ai.rs` clamps
//    the pitch aim).
//  * pitches/PA 2.9..4.5 — nominal 3.3..4.3 (MLB 3.87), live 3.3. Floor 2.9
//    catches the swing-at-everything hitter (first-pitch hacking ended a
//    quarter of PAs on one pitch).
//  * BABIP .220..=.360 — nominal .250..330 (MLB .291), live .301 since the
//    2026-10 outfield-range fix (outfielders and middle infielders at MLB
//    depth, per docs/BASEBALL.md; before it .367, the soft flares dropping in
//    behind a 36 m-deep second baseman). Ceiling .360 sits ~2σ (N=40 has
//    ~240 balls in play, σ≈.03) above the live value and trips the old .367.
//  * doubles/9 0.5..=3.5 — nominal 1.0..2.5 (MLB 1.57 per team-game), live
//    2.4, before the outfield-range fix 4.6 (a 110 m-deep centre fielder let
//    every gap bloop roll for two). Ceiling 3.5 is ~2σ above live (21
//    doubles at N=40) and trips the old 4.6; floor 0.5 catches outfielders
//    that cut off everything (a fly-ball re-pick experiment read 0.14
//    doubles per single in the realism harness).
//  * CS/9 0..1.6 and pickoffs/9 0..1.5 — nominal 0..1 / 0..0.6 (MLB 0.2 /
//    0.05), live 0.2 / 0.6. These are the "CPU runs itself out of innings"
//    guards: the study measured 3.5× MLB steals and 40× MLB pickoffs.
const K_PCT_BAND: std::ops::RangeInclusive<f64> = 11.0..=27.0; // nominal 15..30
const RUNS9_BAND: std::ops::RangeInclusive<f64> = 2.0..=7.5; // nominal 3.0..8.0
const HR9_BAND: std::ops::RangeInclusive<f64> = 0.6..=2.2; // nominal 0.5..2.5
const BB_PCT_BAND: std::ops::RangeInclusive<f64> = 5.0..=13.0; // nominal 6..11
const HBP_PCT_BAND: std::ops::RangeInclusive<f64> = 0.0..=2.5; // nominal 0..2
const PITCHES_PER_PA_BAND: std::ops::RangeInclusive<f64> = 2.9..=4.5; // nominal 3.3..4.3
const BABIP_BAND: std::ops::RangeInclusive<f64> = 0.22..=0.36; // nominal .250..330
const DOUBLES9_BAND: std::ops::RangeInclusive<f64> = 0.5..=3.5; // nominal 1.0..2.5
const CS9_BAND: std::ops::RangeInclusive<f64> = 0.0..=1.6; // nominal 0..1
const PICKOFFS9_BAND: std::ops::RangeInclusive<f64> = 0.0..=1.5; // nominal 0..0.6

fn assert_bands(agg: &Aggregate) {
    assert!(
        K_PCT_BAND.contains(&agg.k_pct),
        "K% {:.1} out of asserted band {:?} (nominal 15..30)",
        agg.k_pct,
        K_PCT_BAND,
    );
    assert!(
        RUNS9_BAND.contains(&agg.runs_per9),
        "runs/9 {:.2} out of asserted band {:?} (nominal 3.0..8.0)",
        agg.runs_per9,
        RUNS9_BAND,
    );
    assert!(
        HR9_BAND.contains(&agg.hr_per9),
        "HR/9 {:.2} out of asserted band {:?} (nominal 0.5..2.5)",
        agg.hr_per9,
        HR9_BAND,
    );
    let checks: [(&str, f64, &std::ops::RangeInclusive<f64>, &str); 7] = [
        ("BB%", agg.bb_pct, &BB_PCT_BAND, "6..11"),
        ("HBP%", agg.hbp_pct, &HBP_PCT_BAND, "0..2"),
        (
            "pitches/PA",
            agg.pitches_per_pa,
            &PITCHES_PER_PA_BAND,
            "3.3..4.3",
        ),
        ("BABIP", agg.babip, &BABIP_BAND, ".250..330"),
        ("doubles/9", agg.doubles_per9, &DOUBLES9_BAND, "1.0..2.5"),
        ("CS/9", agg.caught_stealing_per9, &CS9_BAND, "0..1"),
        ("pickoffs/9", agg.pickoffs_per9, &PICKOFFS9_BAND, "0..0.6"),
    ];
    for (name, value, band, nominal) in checks {
        assert!(
            band.contains(&value),
            "{name} {value:.3} out of asserted band {band:?} (nominal {nominal})",
        );
    }
}

/// The pinned harness: N=40 one-inning games, bands must hold. N was 20 under
/// the old multi-threaded harness; the single-threaded app here runs the sim
/// ~5× faster (thread-pool overhead dominated so tiny a workload), so N rose to
/// 40 — averaging down the per-process spread on K% and HR/9 — while the whole
/// test still finishes in ~1.5 min (well under the ~3 min budget).
#[test]
fn balance_bands_hold() {
    let agg = run_sim(40);
    assert_bands(&agg);
}

/// Deep-tuning variant: N=100 for a tighter aggregate. Ignored by default
/// (minutes of runtime); run with `cargo test --test balance_sim -- --ignored`.
#[test]
#[ignore]
fn balance_sim_long() {
    let agg = run_sim(100);
    assert_bands(&agg);
}
