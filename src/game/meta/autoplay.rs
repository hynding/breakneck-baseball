//! Autoplay (`--features autoplay`): the game drives itself — menu
//! navigation scripted, every slot handed to the Director, the Coach
//! watching — for demo/attract mode and visual playtest runs, native and
//! wasm alike. Findings stream as JSON lines (stderr natively, the console
//! on the web) and a `coach-report.json` lands at every game end (a file
//! natively; localStorage plus a `COACH_REPORT` console line on the web).
//! Beside the Coach counts the report carries a `game` summary (score,
//! contact grades, balls in play) and a `frames` summary (real frame-time
//! percentiles), so two runs diff as data rather than by eye.
//!
//! The module also carries the always-on wasm **beacon**: two console
//! breadcrumbs (`bb-state …`, `bb-first-pitch`) that browser automation
//! watches to prove the real input path works end to end. The beacon is
//! independent of the autoplay feature — it must fire on a normal build
//! driven by synthetic keyboard events.

#[cfg(feature = "autoplay")]
mod drive {
    use std::collections::BTreeMap;

    use bevy::app::AppExit;
    use bevy::prelude::*;
    use serde_json::json;

    use crate::game::coach::{
        CheckId, CoachEnabled, CoachFinding, CoachFindingEvent, CoachReport, Severity,
    };
    use crate::game::director::{Director, DriveGame, Policy, script};
    use crate::game::flow::{BallInPlayEvent, ContactEvent};
    use crate::game::rules::ContactKind;
    use crate::game::{GameState, ScoreBoard};

    /// Boot grace before the menu key is pressed (asset spawns settle).
    const MENU_GRACE_SECS: f32 = 1.0;

    /// Run configuration, read once at startup. Natively from the
    /// environment: `BREAKNECK_AUTOPLAY_SCRIPT=<name>` scripts the Home
    /// slot (vs CPU) instead of the default CPU-vs-CPU attract mode;
    /// `BREAKNECK_AUTOPLAY_INNINGS=<n>` shortens the game;
    /// `BREAKNECK_AUTOPLAY_ONCE=1` exits after the first game's report;
    /// `BREAKNECK_COACH_REPORT=<path>` moves the report file. On wasm the
    /// same switches ride the page URL's query string —
    /// `?script=<name>&innings=<n>` — so a CI browser run can play one
    /// inning without a rebuild (TODO 60).
    struct AutoplayConfig {
        script: Option<String>,
        innings: Option<u32>,
        once: bool,
        /// Native only: the web build persists to localStorage instead.
        #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
        report_path: String,
    }

    fn config() -> AutoplayConfig {
        #[cfg(not(target_arch = "wasm32"))]
        {
            AutoplayConfig {
                script: std::env::var("BREAKNECK_AUTOPLAY_SCRIPT").ok(),
                innings: std::env::var("BREAKNECK_AUTOPLAY_INNINGS")
                    .ok()
                    .and_then(|v| v.parse().ok()),
                once: std::env::var("BREAKNECK_AUTOPLAY_ONCE").is_ok_and(|v| v == "1"),
                report_path: std::env::var("BREAKNECK_COACH_REPORT")
                    .unwrap_or_else(|_| "coach-report.json".into()),
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let search = web_sys::window()
                .map(|w| w.location())
                .and_then(|l| l.search().ok())
                .unwrap_or_default();
            let param = |key: &str| {
                search
                    .trim_start_matches('?')
                    .split('&')
                    .find_map(|kv| kv.split_once('=').filter(|(k, _)| *k == key))
                    .map(|(_, v)| v.to_string())
            };
            AutoplayConfig {
                script: param("script"),
                innings: param("innings").and_then(|v| v.parse().ok()),
                once: false,
                report_path: String::new(),
            }
        }
    }

    fn setup(mut commands: Commands, mut game_config: ResMut<crate::game::GameConfig>) {
        let cfg = config();
        // Shortening the demo game: equivalent to cycling the menu's I key
        // before starting — GameConfig is the menu's own seam.
        if let Some(n) = cfg.innings {
            game_config.innings = n.max(1);
        }
        // A persisted touch scheme can't hijack the scripted slots' batting
        // styles here: `touch::resolve_touch_owner` excludes Director-driven
        // slots (and touch-free devices), so `Controllers::touch_team` stays
        // `None` and `style_for` reads the configured styles. (NOT the
        // `touch::touch_team` candidate fn — that reports `Some(Home)` for
        // any human-sourced slot, scripted ones included.)
        commands.init_resource::<CoachEnabled>();
        let director = match cfg.script.as_deref().and_then(script) {
            Some(s) => Director {
                home: Policy::Scripted(s),
                away: Policy::Cpu,
            },
            None => Director {
                home: Policy::Cpu,
                away: Policy::Cpu,
            },
        };
        commands.insert_resource(director);
    }

    /// Scripted menu navigation, injected at the same post-clear point every
    /// synthetic input uses (the [`DriveGame`] schedule): **1** starts a game
    /// from the main menu, **Enter** leaves the game-over card — an endless
    /// attract loop.
    fn navigate_menus(
        time: Res<Time<Real>>,
        state: Res<State<GameState>>,
        mut keyboard: ResMut<ButtonInput<KeyCode>>,
    ) {
        match state.get() {
            GameState::MainMenu if time.elapsed_secs() > MENU_GRACE_SECS => {
                keyboard.press(KeyCode::Digit1);
            }
            GameState::GameOver => {
                keyboard.release(KeyCode::Digit1);
                // Alternate press/release: `press` on an already-pressed key
                // produces no new `just_pressed` edge, and the single edge a
                // held Enter produced landed on the first GameOver frame —
                // inside `GameOverGrace`'s 1 s input-tail guard — hanging
                // the attract loop on the final card forever.
                if keyboard.pressed(KeyCode::Enter) {
                    keyboard.release(KeyCode::Enter);
                } else {
                    keyboard.press(KeyCode::Enter);
                }
            }
            _ => {
                keyboard.release(KeyCode::Digit1);
                keyboard.release(KeyCode::Enter);
            }
        }
    }

    /// Frame-time histogram width: one bucket per millisecond, the last
    /// bucket catching every frame at or beyond it.
    const FRAME_BUCKETS: usize = 101;

    /// What one game looked like, beside the Coach's verdicts: the outcome
    /// tallies and real frame times. Observe-only like the Coach — it reads
    /// events and clocks, never gameplay state it could perturb. Reset at
    /// every game start, so an attract loop reports per game.
    #[derive(Resource)]
    struct RunSummary {
        /// `ContactEvent` grades, keyed by the `ContactQuality` name.
        contact: BTreeMap<String, u32>,
        home_runs: u32,
        fair_live: u32,
        foul_live: u32,
        frames: u64,
        /// Real frame time in whole milliseconds, clamped into the last bucket.
        frame_ms: [u32; FRAME_BUCKETS],
        max_frame_ms: f32,
    }

    impl Default for RunSummary {
        fn default() -> Self {
            Self {
                contact: BTreeMap::new(),
                home_runs: 0,
                fair_live: 0,
                foul_live: 0,
                frames: 0,
                frame_ms: [0; FRAME_BUCKETS],
                max_frame_ms: 0.0,
            }
        }
    }

    impl RunSummary {
        /// The smallest whole-millisecond bucket holding fraction `p` of frames.
        fn frame_percentile(&self, p: f64) -> usize {
            let target = (self.frames as f64 * p).ceil() as u64;
            let mut seen = 0u64;
            for (ms, &n) in self.frame_ms.iter().enumerate() {
                seen += u64::from(n);
                if seen >= target.max(1) {
                    return ms;
                }
            }
            FRAME_BUCKETS - 1
        }
    }

    fn reset_summary(mut summary: ResMut<RunSummary>) {
        *summary = RunSummary::default();
    }

    fn tally_summary(
        time: Res<Time<Real>>,
        mut contacts: MessageReader<ContactEvent>,
        mut in_play: MessageReader<BallInPlayEvent>,
        mut summary: ResMut<RunSummary>,
    ) {
        for c in contacts.read() {
            *summary
                .contact
                .entry(format!("{:?}", c.quality))
                .or_insert(0) += 1;
        }
        for b in in_play.read() {
            match b.kind {
                ContactKind::HomeRun => summary.home_runs += 1,
                ContactKind::Live { fair: true } => summary.fair_live += 1,
                ContactKind::Live { fair: false } => summary.foul_live += 1,
            }
        }
        let ms = time.delta_secs() * 1000.0;
        summary.frames += 1;
        summary.frame_ms[(ms as usize).min(FRAME_BUCKETS - 1)] += 1;
        summary.max_frame_ms = summary.max_frame_ms.max(ms);
    }

    fn summary_doc(summary: &RunSummary, score: &ScoreBoard) -> serde_json::Value {
        json!({
            "game": {
                "runs": { "home": score.home_runs, "away": score.away_runs },
                "inning": score.inning,
                "top_of_inning": score.top_of_inning,
                "contact": summary.contact,
                "home_runs_hit": summary.home_runs,
                "fair_live": summary.fair_live,
                "foul_live": summary.foul_live,
            },
            "frames": {
                "count": summary.frames,
                "p50_ms": summary.frame_percentile(0.50),
                "p95_ms": summary.frame_percentile(0.95),
                "p99_ms": summary.frame_percentile(0.99),
                "max_ms": summary.max_frame_ms,
                // The percentiles saturate at the last bucket; this says how
                // many frames sit there (software rendering lands them all).
                "over_100ms": summary.frame_ms[FRAME_BUCKETS - 1],
            },
        })
    }

    fn finding_json(f: &CoachFinding) -> serde_json::Value {
        json!({
            "check": f.check.label(),
            "severity": format!("{:?}", f.severity),
            "game_time": f.game_time,
            "subject": f.subject,
            "expected": f.expected,
            "observed": f.observed,
        })
    }

    fn emit(line: &str) {
        #[cfg(target_arch = "wasm32")]
        web_sys::console::log_1(&line.into());
        #[cfg(not(target_arch = "wasm32"))]
        eprintln!("{line}");
    }

    /// Streams every finding as one JSON line the moment it fires.
    fn log_findings(mut events: MessageReader<CoachFindingEvent>) {
        for CoachFindingEvent(f) in events.read() {
            emit(&format!("COACH_FINDING {}", finding_json(f)));
        }
    }

    fn report_doc(
        report: &CoachReport,
        summary: &RunSummary,
        score: &ScoreBoard,
    ) -> serde_json::Value {
        let counts: Vec<_> = CheckId::ALL
            .iter()
            .flat_map(|&check| {
                [Severity::Violation, Severity::Late, Severity::Style]
                    .into_iter()
                    .map(move |sev| (check, sev, report.count(check, sev)))
            })
            .filter(|&(_, _, n)| n > 0)
            .map(|(check, sev, n)| {
                json!({"check": check.label(), "severity": format!("{sev:?}"), "count": n})
            })
            .collect();
        let mut doc = json!({
            "samples": report.samples,
            "counts": counts,
            "recent": report.recent.iter().map(finding_json).collect::<Vec<_>>(),
        });
        if let (Some(doc), serde_json::Value::Object(extra)) =
            (doc.as_object_mut(), summary_doc(summary, score))
        {
            doc.extend(extra);
        }
        doc
    }

    /// Persists the report where the platform's automation can pull it: a
    /// file natively, the `bb-coach-report` localStorage key on the web.
    fn persist(doc: &serde_json::Value, cfg: &AutoplayConfig) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let text = serde_json::to_string_pretty(doc).unwrap_or_default();
            if let Err(e) = std::fs::write(&cfg.report_path, &text) {
                emit(&format!("COACH_REPORT_WRITE_FAILED {e}"));
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = cfg;
            if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten())
            {
                let _ = storage.set_item("bb-coach-report", &doc.to_string());
            }
        }
    }

    /// Mid-game flush every few seconds, so an inning-length watched run
    /// still yields a pullable report (the report is cumulative; the
    /// game-end write below just makes it final). Quiet: no console line.
    fn flush_report(
        time: Res<Time<Real>>,
        mut last: Local<f32>,
        report: Res<CoachReport>,
        summary: Res<RunSummary>,
        score: Res<ScoreBoard>,
    ) {
        if time.elapsed_secs() - *last < 10.0 {
            return;
        }
        *last = time.elapsed_secs();
        persist(&report_doc(&report, &summary, &score), &config());
    }

    /// The game ended: dump the final report, announce it on the console,
    /// and exit if this is a one-shot run.
    fn write_report(
        report: Res<CoachReport>,
        summary: Res<RunSummary>,
        score: Res<ScoreBoard>,
        mut exit: MessageWriter<AppExit>,
    ) {
        let cfg = config();
        let doc = report_doc(&report, &summary, &score);
        emit(&format!("COACH_REPORT {doc}"));
        persist(&doc, &cfg);
        if cfg.once {
            exit.write(AppExit::Success);
        }
    }

    pub struct AutoplayPlugin;

    impl Plugin for AutoplayPlugin {
        fn build(&self, app: &mut App) {
            // The breadcrumbs the web build always carries, on stderr for a
            // native self-driving run (the web target registers them itself).
            #[cfg(not(target_arch = "wasm32"))]
            app.add_plugins(super::WebBeaconPlugin);
            app.init_resource::<RunSummary>()
                .add_systems(Startup, setup)
                .add_systems(DriveGame, navigate_menus)
                .add_systems(crate::game::game_start(), reset_summary)
                .add_systems(
                    Update,
                    (
                        log_findings,
                        (tally_summary, flush_report)
                            .chain()
                            .run_if(in_state(GameState::Playing)),
                    ),
                )
                .add_systems(
                    OnTransition {
                        exited: GameState::Playing,
                        entered: GameState::GameOver,
                    },
                    write_report,
                );
        }
    }
}

#[cfg(feature = "autoplay")]
pub use drive::AutoplayPlugin;

/// The beacon lives on wasm always, and natively under `autoplay` so a
/// self-driving native run leaves the same breadcrumbs on stderr.
#[cfg(any(target_arch = "wasm32", feature = "autoplay"))]
mod beacon {
    use bevy::prelude::*;

    use crate::game::GameState;
    use crate::game::flow::{Phase, Play};

    #[cfg(target_arch = "wasm32")]
    fn log(msg: &str) {
        web_sys::console::log_1(&msg.into());
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn log(msg: &str) {
        eprintln!("{msg}");
    }

    /// Per plate appearance: the walk-up starting and ending (TODO 103) —
    /// the breadcrumb that shows a game is *progressing*, not just started.
    fn walk_up_beacon(play: Res<Play>, mut was_on: Local<bool>) {
        let on = play.walkup_active();
        if on != *was_on {
            log(if on { "bb-walkup" } else { "bb-duel" });
            *was_on = on;
        }
    }

    fn on_playing() {
        log("bb-state playing");
    }

    fn on_menu() {
        log("bb-state menu");
    }

    fn on_game_over() {
        log("bb-state game-over");
    }

    /// Whether this game's first delivery has already been announced.
    #[derive(Resource, Default)]
    struct FirstPitchFired(bool);

    /// Once per game: the first delivery has begun — the proof that "menu →
    /// first pitch" worked, whatever drove the input.
    fn first_pitch(play: Res<Play>, mut fired: ResMut<FirstPitchFired>) {
        if !fired.0 && play.phase == Phase::WindUp {
            fired.0 = true;
            log("bb-first-pitch");
        }
    }

    fn reset_first_pitch(mut fired: ResMut<FirstPitchFired>) {
        fired.0 = false;
    }

    pub struct WebBeaconPlugin;

    impl Plugin for WebBeaconPlugin {
        fn build(&self, app: &mut App) {
            app.init_resource::<FirstPitchFired>()
                .add_systems(OnEnter(GameState::Playing), on_playing)
                .add_systems(OnEnter(GameState::MainMenu), on_menu)
                .add_systems(OnEnter(GameState::GameOver), on_game_over)
                .add_systems(crate::game::game_start(), reset_first_pitch)
                .add_systems(
                    Update,
                    (first_pitch, walk_up_beacon).run_if(in_state(GameState::Playing)),
                );
        }
    }
}

#[cfg(any(target_arch = "wasm32", feature = "autoplay"))]
pub use beacon::WebBeaconPlugin;
