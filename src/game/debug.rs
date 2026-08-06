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
    pub custom: crate::game::scenario::Scenario,
}

/// Pins every judged swing's grade — deterministic swing-outcome testing.
/// Debug-only: read by `flow`'s swing site through a cfg-gated param.
#[derive(Resource, Default, Clone, Copy)]
pub struct ForcedContact(pub Option<crate::game::rules::ContactQuality>);

pub struct DebugPlugin;

impl Plugin for DebugPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DebugState>()
            .init_resource::<ForcedContact>()
            .add_plugins(EguiPlugin)
            .add_plugins(bevy::diagnostic::FrameTimeDiagnosticsPlugin)
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
                DebugTab::Tune => {
                    bevy_inspector_egui::bevy_inspector::ui_for_resource::<
                        crate::game::variant::Ruleset,
                    >(world, ui);
                    egui::CollapsingHeader::new("Field & Camera").show(ui, |ui| {
                        bevy_inspector_egui::bevy_inspector::ui_for_resource::<
                            crate::game::variant::FieldSpec,
                        >(world, ui);
                    });
                    if ui.button("Dump diff → stdout + clipboard").clicked() {
                        let variant = world.resource::<crate::game::GameConfig>().variant;
                        let text = world
                            .resource::<crate::game::variant::Ruleset>()
                            .diff_literal(variant);
                        println!("{text}");
                        ui.ctx().copy_text(text);
                    }
                }
                DebugTab::Scenario => {
                    for s in crate::game::scenario::presets() {
                        if ui.button(s.name).clicked() {
                            let r = crate::game::scenario::apply_to_world(world, &s);
                            world.resource_mut::<DebugState>().last_error = r.err();
                        }
                    }
                    ui.separator();
                    ui.label("Custom");
                    let base_count = world
                        .resource::<crate::game::variant::FieldSpec>()
                        .base_count();
                    let mut state = world.resource_mut::<DebugState>();
                    state.custom.bases.resize(base_count, false);
                    let mut custom = state.custom.clone();
                    ui.horizontal(|ui| {
                        for (i, occ) in custom.bases.iter_mut().enumerate() {
                            ui.checkbox(occ, format!("{}B", i + 1));
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut custom.balls)
                                .range(0..=3)
                                .prefix("B "),
                        );
                        ui.add(
                            egui::DragValue::new(&mut custom.strikes)
                                .range(0..=2)
                                .prefix("S "),
                        );
                        ui.add(
                            egui::DragValue::new(&mut custom.outs)
                                .range(0..=2)
                                .prefix("O "),
                        );
                        ui.add(
                            egui::DragValue::new(&mut custom.inning)
                                .range(1..=99)
                                .prefix("Inn "),
                        );
                        ui.checkbox(&mut custom.top, "top");
                    });
                    egui::ComboBox::from_label("next CPU pitch")
                        .selected_text(format!("{:?}", custom.next_cpu_pitch))
                        .show_ui(ui, |ui| {
                            use crate::game::rules::PitchKind::*;
                            ui.selectable_value(&mut custom.next_cpu_pitch, None, "None");
                            for k in [Fastball, Curveball, Changeup, Slider, Sinker] {
                                ui.selectable_value(
                                    &mut custom.next_cpu_pitch,
                                    Some(k),
                                    format!("{k:?}"),
                                );
                            }
                        });
                    world.resource_mut::<DebugState>().custom = custom.clone();
                    if ui.button("Apply custom").clicked() {
                        let r = crate::game::scenario::apply_to_world(world, &custom);
                        world.resource_mut::<DebugState>().last_error = r.err();
                    }
                    ui.separator();
                    let mut forced = world.resource::<ForcedContact>().0;
                    egui::ComboBox::from_label("force contact")
                        .selected_text(format!("{forced:?}"))
                        .show_ui(ui, |ui| {
                            use crate::game::rules::ContactQuality::*;
                            ui.selectable_value(&mut forced, None, "Off");
                            for q in [Whiff, FoulTip, Weak, Solid, Perfect] {
                                ui.selectable_value(&mut forced, Some(q), format!("{q:?}"));
                            }
                        });
                    world.resource_mut::<ForcedContact>().0 = forced;
                    if let Some(err) = world.resource::<DebugState>().last_error {
                        ui.colored_label(egui::Color32::YELLOW, err);
                    }
                }
                DebugTab::State => {
                    let play = world.resource::<crate::game::flow::Play>();
                    ui.monospace(format!("phase: {:?}", play.phase));
                    ui.monospace(format!("pending_call: {:?}", play.pending_call()));
                    ui.monospace(format!("last swing: {:?}", play.last_contact_quality()));
                    ui.monospace(format!(
                        "steal window: {:.2}s (lead extended: {})",
                        play.steal_window_remaining(),
                        world.resource::<crate::game::flow::LeadState>().extended
                    ));
                    ui.monospace(format!(
                        "runners settled: {}",
                        world.resource::<crate::game::runner::RunnersSettled>().0
                    ));
                    let mut q = world.query_filtered::<(
                        &Transform,
                        &bevy_rapier3d::prelude::Velocity,
                    ), With<crate::game::ball::Baseball>>();
                    if let Ok((tf, vel)) = q.get_single(world) {
                        ui.monospace(format!(
                            "ball: h {:.1} m, v {:.1} m/s",
                            tf.translation.y,
                            vel.linvel.length()
                        ));
                    }
                    if let Some(fps) = world
                        .resource::<bevy::diagnostic::DiagnosticsStore>()
                        .get(&bevy::diagnostic::FrameTimeDiagnosticsPlugin::FPS)
                        .and_then(|d| d.smoothed())
                    {
                        ui.monospace(format!("fps: {fps:.0}"));
                    }
                }
                DebugTab::Gizmos => {
                    ui.label("Gizmos — Task 9");
                }
                DebugTab::Time => {
                    ui.label("Time — Task 10");
                }
            }
        });
}
