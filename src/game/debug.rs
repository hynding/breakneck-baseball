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
    pub step_pending: bool,
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
            .add_plugins(bevy_rapier3d::render::RapierDebugRenderPlugin::default().disabled())
            .add_systems(Update, toggle_panel)
            .add_systems(Update, debug_panel.run_if(panel_open))
            .add_systems(
                Update,
                (
                    zone_gizmo,
                    trajectory_gizmo,
                    intercept_gizmo,
                    throw_target_gizmo,
                    runner_target_gizmo,
                    pci_gizmo,
                )
                    .run_if(in_state(crate::game::GameState::Playing)),
            )
            .add_systems(Last, finish_step);
    }
}

fn zone_gizmo(state: Res<DebugState>, mut gizmos: Gizmos) {
    if !state.gizmos.zone {
        return;
    }
    use crate::game::rules::{ZONE_HALF_WIDTH, ZONE_HIGH, ZONE_LOW};
    let center = Vec3::new(0.0, (ZONE_LOW + ZONE_HIGH) / 2.0, 0.0);
    gizmos.rect(
        Isometry3d::new(center, Quat::IDENTITY),
        Vec2::new(ZONE_HALF_WIDTH * 2.0, ZONE_HIGH - ZONE_LOW),
        bevy::color::palettes::css::AQUA,
    );
}

fn trajectory_gizmo(
    state: Res<DebugState>,
    ball: Query<(&Transform, &bevy_rapier3d::prelude::Velocity), With<crate::game::ball::InFlight>>,
    mut gizmos: Gizmos,
) {
    if !state.gizmos.trajectory {
        return;
    }
    let Ok((tf, vel)) = ball.get_single() else {
        return;
    };
    use crate::game::ball::{BALL_DRAG_FACTOR, MAGNUS_FACTOR};
    let (landing, _hang) = crate::game::rules::predict_landing_from(
        tf.translation,
        vel.linvel,
        vel.angvel,
        BALL_DRAG_FACTOR,
        MAGNUS_FACTOR,
    );
    // A sightline + landing circle reads the play; exact touchdown already
    // lives in fx.rs's landing ring.
    gizmos.line(
        tf.translation,
        landing + Vec3::Y * 0.02,
        bevy::color::palettes::css::ORANGE,
    );
    gizmos.circle(
        Isometry3d::new(
            landing + Vec3::Y * 0.02,
            Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
        ),
        0.5,
        bevy::color::palettes::css::ORANGE,
    );
}

fn intercept_gizmo(
    state: Res<DebugState>,
    fielders: Query<(&Transform, &crate::game::animation::MoveIntent)>,
    mut gizmos: Gizmos,
) {
    if !state.gizmos.intercept {
        return;
    }
    for (tf, intent) in &fielders {
        if let Some(target) = intent.target {
            gizmos.line(
                tf.translation + Vec3::Y * 0.1,
                target + Vec3::Y * 0.1,
                bevy::color::palettes::css::YELLOW,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn throw_target_gizmo(
    state: Res<DebugState>,
    play: Res<crate::game::flow::Play>,
    bases: Res<crate::game::rules::Bases>,
    ruleset: Res<crate::game::variant::Ruleset>,
    field: Res<crate::game::variant::FieldSpec>,
    time: Res<Time>,
    ball: Query<&Transform, With<crate::game::ball::Baseball>>,
    mut gizmos: Gizmos,
) {
    if !state.gizmos.intercept || play.phase != crate::game::flow::Phase::InPlay {
        return;
    }
    let Ok(tf) = ball.get_single() else {
        return;
    };
    let race = play.since_contact(time.elapsed_secs());
    // Same call fielding.rs makes at the throw (fielding.rs:356).
    let target = crate::game::rules::throw_target(
        tf.translation,
        race,
        &bases,
        play.runners_going(),
        &field,
        &ruleset.pace,
    );
    let pos = field
        .base_positions
        .get(target)
        .copied()
        .unwrap_or(Vec3::ZERO);
    gizmos.circle(
        Isometry3d::new(
            pos + Vec3::Y * 0.05,
            Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
        ),
        1.0,
        bevy::color::palettes::css::YELLOW,
    );
}

fn runner_target_gizmo(
    state: Res<DebugState>,
    field: Res<crate::game::variant::FieldSpec>,
    runners: Query<(&Transform, &crate::game::runner::Runner)>,
    mut gizmos: Gizmos,
) {
    if !state.gizmos.runner_targets {
        return;
    }
    for (tf, runner) in &runners {
        if let Some(bag) = field.base_positions.get(runner.base) {
            gizmos.line(
                tf.translation,
                *bag + Vec3::Y * 0.05,
                bevy::color::palettes::css::LIMEGREEN,
            );
        }
    }
}

fn pci_gizmo(
    state: Res<DebugState>,
    ruleset: Res<crate::game::variant::Ruleset>,
    cursor: Query<&Transform, With<crate::game::field::PciCursorMarker>>,
    mut gizmos: Gizmos,
) {
    if !state.gizmos.pci {
        return;
    }
    let Ok(tf) = cursor.get_single() else {
        return;
    };
    gizmos.circle(
        Isometry3d::new(tf.translation, Quat::IDENTITY),
        ruleset.batting.pci_radius_m,
        bevy::color::palettes::css::MAGENTA,
    );
}

fn panel_open(state: Res<DebugState>) -> bool {
    state.open
}

/// Re-pauses after a single-step frame: `step` in the Time tab unpauses
/// `Time<Virtual>` for exactly one `Update`, and this `Last`-schedule system
/// pauses it back before the next frame starts.
fn finish_step(mut state: ResMut<DebugState>, mut virt: ResMut<Time<Virtual>>) {
    if state.step_pending {
        virt.pause();
        state.step_pending = false;
    }
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
                    let mut gizmos = world.resource::<DebugState>().gizmos;
                    ui.checkbox(&mut gizmos.zone, "Strike zone");
                    ui.checkbox(&mut gizmos.trajectory, "Ball trajectory + landing");
                    ui.checkbox(&mut gizmos.intercept, "Fielder intercepts + throw target");
                    ui.checkbox(&mut gizmos.pci, "PCI radius ring");
                    ui.checkbox(&mut gizmos.runner_targets, "Runner target lines");
                    if ui
                        .checkbox(&mut gizmos.colliders, "Rapier colliders")
                        .changed()
                    {
                        world
                            .resource_mut::<bevy_rapier3d::render::DebugRenderContext>()
                            .enabled = gizmos.colliders;
                    }
                    world.resource_mut::<DebugState>().gizmos = gizmos;
                }
                DebugTab::Time => {
                    ui.horizontal(|ui| {
                        for (label, s) in [("¼×", 0.25f32), ("½×", 0.5), ("1×", 1.0), ("2×", 2.0)]
                        {
                            if ui.button(label).clicked() {
                                world.resource_mut::<crate::game::juice::BaseSpeed>().0 = s;
                                world.resource_mut::<Time<Virtual>>().set_relative_speed(s);
                            }
                        }
                    });
                    let paused = world.resource::<Time<Virtual>>().is_paused();
                    ui.horizontal(|ui| {
                        if ui.button(if paused { "resume" } else { "pause" }).clicked() {
                            let mut virt = world.resource_mut::<Time<Virtual>>();
                            if paused {
                                virt.unpause()
                            } else {
                                virt.pause()
                            }
                        }
                        if ui.button("step").clicked() {
                            world.resource_mut::<Time<Virtual>>().unpause();
                            world.resource_mut::<DebugState>().step_pending = true;
                        }
                    });
                }
            }
        });
}
