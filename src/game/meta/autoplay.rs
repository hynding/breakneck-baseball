//! Autoplay (`--features autoplay`): the game drives itself — menu
//! navigation scripted, every slot handed to the Director, the Coach
//! watching — for demo/attract mode and visual playtest runs, native and
//! wasm alike. Findings stream as JSON lines (stderr natively, the console
//! on the web) and a `coach-report.json` lands at every game end (a file
//! natively; localStorage plus a `COACH_REPORT` console line on the web).
//!
//! The module also carries the always-on wasm **beacon**: two console
//! breadcrumbs (`bb-state …`, `bb-first-pitch`) that browser automation
//! watches to prove the real input path works end to end. The beacon is
//! independent of the autoplay feature — it must fire on a normal build
//! driven by synthetic keyboard events.

#[cfg(feature = "autoplay")]
mod drive {
    use bevy::app::AppExit;
    use bevy::prelude::*;
    use serde_json::json;

    use crate::game::GameState;
    use crate::game::coach::{
        CheckId, CoachEnabled, CoachFinding, CoachFindingEvent, CoachReport, Severity,
    };
    use crate::game::director::{Director, DriveGame, Policy, script};

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
                keyboard.press(KeyCode::Enter);
            }
            _ => {
                keyboard.release(KeyCode::Digit1);
                keyboard.release(KeyCode::Enter);
            }
        }
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
    fn log_findings(mut events: EventReader<CoachFindingEvent>) {
        for CoachFindingEvent(f) in events.read() {
            emit(&format!("COACH_FINDING {}", finding_json(f)));
        }
    }

    fn report_doc(report: &CoachReport) -> serde_json::Value {
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
        json!({
            "samples": report.samples,
            "counts": counts,
            "recent": report.recent.iter().map(finding_json).collect::<Vec<_>>(),
        })
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
    fn flush_report(time: Res<Time<Real>>, mut last: Local<f32>, report: Res<CoachReport>) {
        if time.elapsed_secs() - *last < 10.0 {
            return;
        }
        *last = time.elapsed_secs();
        persist(&report_doc(&report), &config());
    }

    /// The game ended: dump the final report, announce it on the console,
    /// and exit if this is a one-shot run.
    fn write_report(report: Res<CoachReport>, mut exit: EventWriter<AppExit>) {
        let cfg = config();
        let doc = report_doc(&report);
        emit(&format!("COACH_REPORT {doc}"));
        persist(&doc, &cfg);
        if cfg.once {
            exit.send(AppExit::Success);
        }
    }

    pub struct AutoplayPlugin;

    impl Plugin for AutoplayPlugin {
        fn build(&self, app: &mut App) {
            app.add_systems(Startup, setup)
                .add_systems(DriveGame, navigate_menus)
                .add_systems(
                    Update,
                    (
                        log_findings,
                        flush_report.run_if(in_state(GameState::Playing)),
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

#[cfg(target_arch = "wasm32")]
mod beacon {
    use bevy::prelude::*;

    use crate::game::GameState;
    use crate::game::flow::{Phase, Play};

    fn log(msg: &str) {
        web_sys::console::log_1(&msg.into());
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
                .add_systems(Update, first_pitch.run_if(in_state(GameState::Playing)));
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub use beacon::WebBeaconPlugin;
