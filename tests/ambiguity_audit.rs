//! Schedule-ambiguity gate for the gameplay systems (TODO 58).
//!
//! The deterministic harness pins one *legal* execution order — but which
//! legal order it picks rides the schedule graph's ambiguity tie-breaks,
//! which shift with the binary layout. A gameplay pair left ambiguous
//! therefore means "the game trajectory can change when unrelated code
//! changes" — the exact mechanism behind the one-off `e2e_fielder_spots`
//! failure. This test builds the real app with ambiguity detection on and
//! asserts that **no two gameplay-relevant systems are ambiguous with each
//! other** in `Update`; presentation-only pairs (UI, fx, cameras, jerseys)
//! are reported but allowed — they cannot feed back into the trajectory.

mod common;

use bevy::ecs::schedule::{LogLevel, ScheduleBuildSettings};
use bevy::prelude::*;

use common::headless_app;

/// Module fragments whose systems drive the gameplay trajectory: everything
/// in the sim layer, plus the animation systems that move rigs the sim reads
/// back (`locomote`), plus the input layer. The Coach is excluded on
/// purpose: it is a pure observer and cannot feed back.
const GAMEPLAY: &[&str] = &[
    "::sim::flow",
    "::sim::fielding",
    "::sim::runner",
    "::sim::ball",
    "::sim::batting",
    "::sim::ai",
    "::sim::director",
    "::meta::input",
    "animation::driver::locomote",
];

fn is_gameplay(name: &str) -> bool {
    GAMEPLAY.iter().any(|frag| name.contains(frag)) && !name.contains("::coach")
}

#[test]
fn no_gameplay_system_ambiguities_in_update() {
    let mut app = headless_app();
    app.edit_schedule(Update, |schedule| {
        schedule.set_build_settings(ScheduleBuildSettings {
            ambiguity_detection: LogLevel::Warn,
            ..Default::default()
        });
    });
    // One update forces the schedule build (which computes the conflicts).
    app.update();

    let world = app.world_mut();
    world.resource_scope(|world, mut schedules: Mut<Schedules>| {
        let schedule = schedules.get_mut(Update).expect("Update schedule");
        // Ensure it is initialized against this world state.
        schedule.initialize(world).expect("schedule initializes");
        // After initialization the systems live in the executable, not the
        // graph nodes — build the NodeId → name map from the iterator.
        let names: std::collections::HashMap<_, _> = schedule
            .systems()
            .expect("initialized schedule lists its systems")
            .map(|(id, system)| (id, system.name().to_string()))
            .collect();
        let graph = schedule.graph();
        let mut gameplay_pairs = Vec::new();
        let mut allowed = 0usize;
        for (a, b, _components) in graph.conflicting_systems() {
            let name_of = |id: &_| names.get(id).cloned().unwrap_or_else(|| format!("{id:?}"));
            let (an, bn) = (name_of(a), name_of(b));
            if is_gameplay(&an) && is_gameplay(&bn) {
                gameplay_pairs.push(format!("  {an}  <->  {bn}"));
            } else {
                allowed += 1;
            }
        }
        eprintln!(
            "ambiguity audit: {} gameplay pair(s), {} presentation pair(s) (allowed)",
            gameplay_pairs.len(),
            allowed
        );
        for p in &gameplay_pairs {
            eprintln!("{p}");
        }
        assert!(
            gameplay_pairs.is_empty(),
            "gameplay systems with ambiguous ordering (trajectory becomes \
             binary-layout-dependent):\n{}",
            gameplay_pairs.join("\n")
        );
    });
}
