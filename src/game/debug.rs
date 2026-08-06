//! Debug mode (`--features debug`): a tabbed egui panel plus gizmo overlays.
//! A privileged reader/writer of existing resources — it must never own
//! gameplay state beyond its own toggles.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_inspector_egui::bevy_egui::{EguiContext, EguiPlugin};
use bevy_inspector_egui::egui;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum DebugTab {
    #[default]
    Tune,
    Scenario,
    State,
    Gizmos,
    Time,
}

#[derive(Clone, Copy, Default)]
pub struct GizmoToggles {
    pub zone: bool,
    pub trajectory: bool,
    pub intercept: bool,
    pub pci: bool,
    pub runner_targets: bool,
    pub colliders: bool,
}

#[derive(Resource, Default)]
pub struct DebugState {
    pub open: bool,
    pub tab: DebugTab,
    pub gizmos: GizmoToggles,
    pub last_error: Option<&'static str>,
}

pub struct DebugPlugin;

impl Plugin for DebugPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DebugState>()
            .add_plugins(EguiPlugin)
            .add_systems(Update, toggle_panel)
            .add_systems(Update, debug_panel.run_if(panel_open));
    }
}

fn panel_open(state: Res<DebugState>) -> bool {
    state.open
}

/// F1 opens/closes; number keys 1–5 switch tabs while the panel is open.
fn toggle_panel(keys: Res<ButtonInput<KeyCode>>, mut state: ResMut<DebugState>) {
    if keys.just_pressed(KeyCode::F1) {
        state.open = !state.open;
    }
    if !state.open {
        return;
    }
    for (key, tab) in [
        (KeyCode::Digit1, DebugTab::Tune),
        (KeyCode::Digit2, DebugTab::Scenario),
        (KeyCode::Digit3, DebugTab::State),
        (KeyCode::Digit4, DebugTab::Gizmos),
        (KeyCode::Digit5, DebugTab::Time),
    ] {
        if keys.just_pressed(key) {
            state.tab = tab;
        }
    }
}

/// Exclusive: the inspector widgets need `&mut World` alongside the egui ctx.
fn debug_panel(world: &mut World) {
    let Ok(ctx) = world
        .query_filtered::<&mut EguiContext, With<PrimaryWindow>>()
        .get_single_mut(world)
        .map(|mut c| c.get_mut().clone())
    else {
        return;
    };
    egui::Window::new("Debug")
        .default_width(340.0)
        .show(&ctx, |ui| {
            let mut tab = world.resource::<DebugState>().tab;
            ui.horizontal(|ui| {
                for (label, t) in [
                    ("Tune", DebugTab::Tune),
                    ("Scenario", DebugTab::Scenario),
                    ("State", DebugTab::State),
                    ("Gizmos", DebugTab::Gizmos),
                    ("Time", DebugTab::Time),
                ] {
                    ui.selectable_value(&mut tab, t, label);
                }
            });
            world.resource_mut::<DebugState>().tab = tab;
            ui.separator();
            match tab {
                DebugTab::Tune => ui.label("Tune — Task 4"),
                DebugTab::Scenario => ui.label("Scenario — Task 6"),
                DebugTab::State => ui.label("State — Task 8"),
                DebugTab::Gizmos => ui.label("Gizmos — Task 9"),
                DebugTab::Time => ui.label("Time — Task 10"),
            };
        });
}
