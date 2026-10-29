//! Measurement-only harness (not committed): plays full CPU-vs-CPU games and
//! logs every observable event as JSON lines for offline analysis.
//! Env: BB_GAMES (default 4), BB_START (game index offset, default 0),
//! BB_INNINGS (1 or 9, default 9), BB_OUT (path, default realism.jsonl).

mod common;

use std::io::Write;

use bevy::prelude::*;

use breakneck_baseball::game::ball::{HitEvent, PitchEvent, WallBangEvent};
use breakneck_baseball::game::coach::CoachFindingEvent;
use breakneck_baseball::game::flow::{
    BallInPlayEvent, ContactEvent, LiveBallEvent, Play, PlayBanner,
};
use breakneck_baseball::game::input::{Controllers, InputSource};
use breakneck_baseball::game::rules::{Bases, BattingOrder};
use breakneck_baseball::game::{GameState, ScoreBoard, Team};

use common::{DriveGame, HeadlessConfig, headless_app_with, run_until, start_game, tap_key};

const SIM_DT: f64 = 1.0 / 100.0;

#[derive(Resource, Default)]
struct Log {
    game: u32,
    lines: Vec<String>,
    prev_phase: String,
    prev_slots: Option<(u32, u32)>,
    prev_score: (u32, u32),
    prev_half: (u32, bool),
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[allow(clippy::too_many_arguments)]
fn log_events(
    time: Res<Time<Virtual>>,
    score: Res<ScoreBoard>,
    bases: Res<Bases>,
    order: Res<BattingOrder>,
    play: Res<Play>,
    mut banners: MessageReader<PlayBanner>,
    mut pitches: MessageReader<PitchEvent>,
    mut hits: MessageReader<HitEvent>,
    mut walls: MessageReader<WallBangEvent>,
    mut contacts: MessageReader<ContactEvent>,
    mut bip: MessageReader<BallInPlayEvent>,
    mut live: MessageReader<LiveBallEvent>,
    mut coach: MessageReader<CoachFindingEvent>,
    mut log: ResMut<Log>,
) {
    let t = time.elapsed_secs_f64();
    let b = (0..3)
        .map(|i| if bases.is_occupied(i) { '1' } else { '0' })
        .collect::<String>();
    let head = format!(
        "\"g\":{},\"t\":{:.3},\"inn\":{},\"top\":{},\"b\":{},\"s\":{},\"o\":{},\"bases\":\"{}\",\"h\":{},\"a\":{}",
        log.game,
        t,
        score.inning,
        score.top_of_inning,
        score.balls,
        score.strikes,
        score.outs,
        b,
        score.home_runs,
        score.away_runs
    );
    let push = |log: &mut Log, kind: &str, extra: String| {
        log.lines
            .push(format!("{{{head},\"k\":\"{kind}\"{extra}}}"));
    };
    let phase = format!("{:?}", play.phase);
    if phase != log.prev_phase {
        push(&mut log, "phase", format!(",\"p\":\"{phase}\""));
        log.prev_phase = phase;
    }
    let cur = (order.current(Team::Home), order.current(Team::Away));
    if let Some(prev) = log.prev_slots {
        if cur.0 != prev.0 {
            push(&mut log, "pa_end", ",\"team\":\"home\"".into());
        }
        if cur.1 != prev.1 {
            push(&mut log, "pa_end", ",\"team\":\"away\"".into());
        }
    }
    log.prev_slots = Some(cur);
    let sc = (score.home_runs, score.away_runs);
    if sc != log.prev_score {
        let d = (sc.0 - log.prev_score.0, sc.1 - log.prev_score.1);
        push(&mut log, "score", format!(",\"dh\":{},\"da\":{}", d.0, d.1));
        log.prev_score = sc;
    }
    let half = (score.inning, score.top_of_inning);
    if half != log.prev_half {
        push(&mut log, "half", String::new());
        log.prev_half = half;
    }
    for e in banners.read() {
        let f = e.follow_up.as_deref().unwrap_or("");
        push(
            &mut log,
            "banner",
            format!(",\"text\":\"{}\",\"fu\":\"{}\"", esc(&e.text), esc(f)),
        );
    }
    for e in pitches.read() {
        let v = e.velocity;
        push(
            &mut log,
            "pitch",
            format!(
                ",\"v\":[{:.3},{:.3},{:.3}],\"spin\":[{:.2},{:.2},{:.2}]",
                v.x, v.y, v.z, e.spin.x, e.spin.y, e.spin.z
            ),
        );
    }
    for e in hits.read() {
        let v = e.velocity;
        push(
            &mut log,
            "hit",
            format!(",\"v\":[{:.3},{:.3},{:.3}]", v.x, v.y, v.z),
        );
    }
    for e in walls.read() {
        push(
            &mut log,
            "wall",
            format!(",\"pos\":[{:.2},{:.2},{:.2}]", e.pos.x, e.pos.y, e.pos.z),
        );
    }
    for e in contacts.read() {
        push(
            &mut log,
            "contact",
            format!(
                ",\"q\":\"{:?}\",\"dt\":{:.1},\"team\":\"{:?}\"",
                e.quality, e.dt_ms, e.batting_team
            ),
        );
    }
    for e in bip.read() {
        push(
            &mut log,
            "bip",
            format!(
                ",\"kind\":\"{:?}\",\"class\":\"{:?}\",\"land\":[{:.2},{:.2},{:.2}]",
                e.kind, e.contact_class, e.landing.x, e.landing.y, e.landing.z
            ),
        );
    }
    for e in live.read() {
        let s = match *e {
            LiveBallEvent::Caught { pos } => format!(
                ",\"ev\":\"caught\",\"pos\":[{:.2},{:.2},{:.2}]",
                pos.x, pos.y, pos.z
            ),
            LiveBallEvent::Landed { pos } => format!(
                ",\"ev\":\"landed\",\"pos\":[{:.2},{:.2},{:.2}]",
                pos.x, pos.y, pos.z
            ),
            LiveBallEvent::Thrown {
                pos,
                base,
                race_time,
            } => format!(
                ",\"ev\":\"thrown\",\"pos\":[{:.2},{:.2},{:.2}],\"base\":{base},\"race\":{race_time:.2}",
                pos.x, pos.y, pos.z
            ),
            LiveBallEvent::Settled => ",\"ev\":\"settled\"".to_string(),
        };
        push(&mut log, "live", s);
    }
    for e in coach.read() {
        let f = &e.0;
        push(
            &mut log,
            "coach",
            format!(
                ",\"check\":\"{:?}\",\"sev\":\"{:?}\",\"subj\":\"{}\",\"exp\":\"{}\",\"obs\":\"{}\"",
                f.check,
                f.severity,
                esc(&f.subject),
                esc(&f.expected),
                esc(&f.observed)
            ),
        );
    }
}

fn play_one(game_index: u32, innings: u32) -> Vec<String> {
    let mut app = headless_app_with(HeadlessConfig {
        single_threaded: true,
        skip_skeletal_sampling: true,
        ..Default::default()
    });
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::from_secs_f64(SIM_DT),
    ));
    app.insert_resource(Log {
        game: game_index,
        ..Default::default()
    });
    app.add_systems(DriveGame, log_events);
    for _ in 0..game_index * 37 {
        app.update();
    }
    if innings == 1 {
        tap_key(&mut app, KeyCode::KeyI);
    }
    start_game(&mut app, KeyCode::Digit1);
    *app.world_mut().resource_mut::<Controllers>() = Controllers {
        home: InputSource::Cpu,
        away: InputSource::Cpu,
        ..Controllers::default()
    };
    let frames = run_until(&mut app, 4_000_000, |app| {
        *app.world().resource::<State<GameState>>().get() == GameState::GameOver
            || app.world().resource::<ScoreBoard>().inning > innings + 6
    });
    for _ in 0..3 {
        app.update();
    }
    let s = app.world().resource::<ScoreBoard>();
    let s = (s.inning, s.top_of_inning, s.home_runs, s.away_runs);
    let t = app.world().resource::<Time<Virtual>>().elapsed_secs_f64();
    let mut lines = std::mem::take(&mut app.world_mut().resource_mut::<Log>().lines);
    lines.push(format!(
        "{{\"g\":{game_index},\"k\":\"game_end\",\"frames\":{},\"t\":{t:.2},\"inn\":{},\"top\":{},\"h\":{},\"a\":{},\"over\":{}}}",
        frames.unwrap_or(0), s.0, s.1, s.2, s.3, frames.is_some()
    ));
    lines
}

#[test]
#[ignore]
fn realism_sim() {
    let env = |k: &str, d: u32| {
        std::env::var(k)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(d)
    };
    let n = env("BB_GAMES", 4);
    let start = env("BB_START", 0);
    let innings = env("BB_INNINGS", 9);
    let out = std::env::var("BB_OUT").unwrap_or_else(|_| "realism.jsonl".into());
    let mut f = std::fs::File::create(&out).unwrap();
    for g in start..start + n {
        let t0 = std::time::Instant::now();
        let lines = play_one(g, innings);
        for l in &lines {
            writeln!(f, "{l}").unwrap();
        }
        f.flush().unwrap();
        eprintln!(
            "game {g} done in {:.1}s: {}",
            t0.elapsed().as_secs_f64(),
            lines.last().unwrap()
        );
    }
}
