//! Where one headless inning spends its time, system by system (TODO 29's
//! "headless sim ~72% slower since Bevy 0.15" probe). Not a test of the game:
//! `#[ignore]`d, and compiled only with the `profile` feature, which turns on
//! Bevy's `trace` spans. Run it with:
//!
//! ```sh
//! cargo nextest run --features profile --test e2e sim_profile:: \
//!     --run-ignored only --no-capture
//! ```
//!
//! The layer below hangs off `LogPlugin::custom_layer` and sums every span's
//! wall time by label (`system:<fn path>`, `schedule:<label>`). Bevy's
//! system spans are roots (`parent: None`), so system rows don't nest under
//! schedule rows; the schedule rows are the inclusive per-frame budget, the
//! system rows say who spends it. Spans cost time themselves, so compare runs
//! of this probe with each other, never with an untraced run.
#![cfg(feature = "profile")]

use crate::common;

use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use bevy::log::BoxedLayer;
use bevy::log::tracing::Subscriber;
use bevy::log::tracing::field::{Field, Visit};
use bevy::log::tracing::span::{Attributes, Id};
use bevy::log::tracing_subscriber::Layer;
use bevy::log::tracing_subscriber::layer::Context;
use bevy::log::tracing_subscriber::registry::LookupSpan;
use bevy::prelude::*;
use breakneck_baseball::game::input::{Controllers, InputSource};
use breakneck_baseball::game::{GameState, ScoreBoard};
use common::{deterministic_headless_app_with_log_layer, run_until, start_game, tap_key};

/// Same step as `tests/balance_sim.rs`: this probe measures the sim the
/// balance harness runs.
const SIM_DT: f64 = 1.0 / 100.0;
const MAX_FRAMES: u64 = 400_000;

/// Inclusive wall time and enter count per span label.
static TOTALS: LazyLock<Mutex<HashMap<String, (Duration, u64)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

struct Label(String);
struct Entered(Instant);

struct ProfileLayer;

impl<S> Layer<S> for ProfileLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let mut name = NameField(None);
        attrs.record(&mut name);
        let label = match name.0 {
            Some(n) => format!("{}:{}", attrs.metadata().name(), n),
            None => attrs.metadata().name().to_string(),
        };
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(Label(label));
        }
    }

    fn on_enter(&self, id: &Id, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(Entered(Instant::now()));
        }
    }

    fn on_exit(&self, id: &Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let label = {
            let mut ext = span.extensions_mut();
            let Some(Entered(start)) = ext.remove::<Entered>() else {
                return;
            };
            let Some(Label(label)) = ext.get_mut::<Label>() else {
                return;
            };
            (label.clone(), start.elapsed())
        };
        let mut totals = TOTALS.lock().expect("profile totals");
        let entry = totals.entry(label.0).or_insert((Duration::ZERO, 0));
        entry.0 += label.1;
        entry.1 += 1;
    }
}

/// Pulls the `name` field Bevy puts on `system` and `schedule` spans.
struct NameField(Option<String>);

impl Visit for NameField {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "name" {
            self.0 = Some(value.to_string());
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        if field.name() == "name" && self.0.is_none() {
            self.0 = Some(format!("{value:?}"));
        }
    }
}

fn profile_layer(_: &mut App) -> Option<BoxedLayer> {
    Some(Box::new(ProfileLayer))
}

#[test]
#[ignore = "profiling probe, not a gate — see the module doc for the command"]
fn where_one_cpu_inning_spends_its_time() {
    let mut app = deterministic_headless_app_with_log_layer(profile_layer);
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        Duration::from_secs_f64(SIM_DT),
    ));
    // 9 -> 1 inning (the menu cycle wraps), one-player mode, then both slots
    // to the CPU — exactly `balance_sim::play_one_game`.
    tap_key(&mut app, KeyCode::KeyI);
    start_game(&mut app, KeyCode::Digit1);
    *app.world_mut().resource_mut::<Controllers>() = Controllers {
        home: InputSource::Cpu,
        away: InputSource::Cpu,
        ..Controllers::default()
    };

    // Boot and menu frames are not the sim: start counting at first pitch.
    TOTALS.lock().expect("profile totals").clear();
    let wall = Instant::now();
    let frames = run_until(&mut app, MAX_FRAMES, |app| {
        let over = *app.world().resource::<State<GameState>>().get() == GameState::GameOver;
        over || app.world().resource::<ScoreBoard>().inning >= 2
    })
    .expect("one CPU-vs-CPU inning never finished");
    let wall = wall.elapsed();

    let totals = TOTALS.lock().expect("profile totals");
    let main_total = totals.get("schedule:Main").map(|(d, _)| *d).unwrap_or(wall);
    let mut rows: Vec<_> = totals.iter().collect();
    rows.sort_by_key(|(_, (total, _))| std::cmp::Reverse(*total));

    let score = app.world().resource::<ScoreBoard>();
    println!(
        "\nsim profile — one CPU-vs-CPU inning: {frames} frames in {:.1} s wall \
         ({:.2} ms/frame; schedule:Main {:.2} ms/frame); final {}-{}\n",
        wall.as_secs_f64(),
        wall.as_secs_f64() * 1000.0 / frames as f64,
        main_total.as_secs_f64() * 1000.0 / frames as f64,
        score.away_runs,
        score.home_runs,
    );
    // How much of the frame is inside any system at all. The rest is the
    // executor: run conditions, command application, change-tick bookkeeping,
    // per-system fixed cost — which scales with how many systems run.
    let (system_runs, in_systems) = totals
        .iter()
        .filter(|(label, _)| label.starts_with("system:"))
        .fold((0u64, Duration::ZERO), |(n, d), (_, (total, enters))| {
            (n + enters, d + *total)
        });
    let distinct = totals.keys().filter(|l| l.starts_with("system:")).count();
    println!(
        "systems: {distinct} distinct, {:.0} runs/frame, {:.0}% of schedule:Main inside system \
         spans (the rest is executor overhead)\n",
        system_runs as f64 / frames as f64,
        100.0 * in_systems.as_secs_f64() / main_total.as_secs_f64(),
    );

    // Who spends it, by crate: every `system:` row keyed on the path's first
    // segment. Engine crates vs `breakneck_baseball` is the headline.
    let mut by_crate: HashMap<&str, Duration> = HashMap::new();
    for (label, (total, _)) in totals.iter() {
        if let Some(path) = label.strip_prefix("system:") {
            let krate = path.split("::").next().unwrap_or(path);
            *by_crate.entry(krate).or_default() += *total;
        }
    }
    let mut by_crate: Vec<_> = by_crate.into_iter().collect();
    by_crate.sort_by_key(|&(_, total)| std::cmp::Reverse(total));
    println!(
        "{:>9} {:>9} {:>6}  systems by crate",
        "total ms", "us/frame", "%Main"
    );
    for (krate, total) in &by_crate {
        println!(
            "{:>9.1} {:>9.1} {:>5.1}%  {krate}",
            total.as_secs_f64() * 1000.0,
            total.as_secs_f64() * 1e6 / frames as f64,
            100.0 * total.as_secs_f64() / main_total.as_secs_f64(),
        );
    }
    println!();

    println!(
        "{:>9} {:>9} {:>6} {:>7}  span",
        "total ms", "us/frame", "%Main", "enters"
    );
    for (label, (total, enters)) in rows.iter().take(60) {
        println!(
            "{:>9.1} {:>9.1} {:>5.1}% {:>7}  {}",
            total.as_secs_f64() * 1000.0,
            total.as_secs_f64() * 1e6 / frames as f64,
            100.0 * total.as_secs_f64() / main_total.as_secs_f64(),
            enters,
            label.replace("breakneck_baseball::game::", ""),
        );
    }
}
