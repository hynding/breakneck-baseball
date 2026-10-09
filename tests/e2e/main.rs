//! The headless end-to-end suites, compiled as **one** test binary.
//!
//! Every integration-test file is its own crate that statically links all of
//! Bevy, so 30-odd `tests/e2e_*.rs` files meant 30-odd multi-hundred-MB links
//! after every `src/` edit. As modules of a single crate the suites link once
//! and run in parallel on one shared thread pool. Filter with the module path:
//! `cargo test --test e2e matrix::`.
//!
//! Sharing a process has one rule (verify-change skill): a module here never
//! touches process-global state another suite reads. That rules out
//! `std::env::set_var`/`remove_var`. The harness sets `BREAKNECK_SETTINGS_PATH`
//! once per process and nothing ever removes it. Suites that must break the
//! rule keep their own binary: `tests/e2e_settings.rs` (sets the settings path)
//! and `tests/balance_sim.rs` (pins Bevy's process-global task pools to one
//! thread).

#[path = "../common/mod.rs"]
mod common;

mod advanced_rules;
mod ambiguity_audit;
mod appearance_contract;
mod base_cam;
mod baserunning_breaks;
mod batter_runs;
mod batting_styles;
mod call_beat;
mod camera_views;
mod catcher_crouch;
mod change_detection;
mod coach;
mod contact_stamp;
mod contact_timing;
mod cpu;
mod cpu_timing;
#[cfg(feature = "debug")]
mod creator;
mod dressing;
mod fielder_spots;
mod full_game;
mod gltf_model;
mod gltf_rig;
mod home_run_moment;
mod identity;
mod matrix;
mod model_contract;
mod passive_walks;
mod pause_subs;
mod scenarios;
mod swing_startup;
mod touch_pipeline;
mod walkup;
