//! The selector + tabs + revert egui side panel.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_inspector_egui::bevy_egui::EguiContext;
use bevy_inspector_egui::egui;

use crate::game::Team;
use crate::game::appearance::{
    Arms, CelebrationId, Eyewear, FidgetId, Headwear, PlayerDef, SkinTone, StanceId,
};
use crate::game::rules::LINEUP_SIZE;

use super::persist::save_working;
use super::randomize::randomize_player;
use super::{CreatorState, CreatorTab, selected_def};

/// The selector + tabs + revert egui side panel. Exclusive (needs `&mut
/// World`, same as `debug::debug_panel`) purely to reach the `EguiContext`
/// the same way that system does.
///
/// egui redraws every frame the panel is visible, so a plain `&mut
/// CreatorState` handed to [`render_creator_panel`] would flag
/// `Changed<CreatorState>` on *every* frame via `Mut::deref_mut` — not just
/// frames with a real edit — defeating `apply_creator_edits`'s
/// `is_changed()` gate (it would rebuild `RosterDefs`/`Rosters` and
/// re-insert `PlayerIdentity` continuously while the Creator is simply
/// sitting open). Fixed the same way `debug.rs`'s Tune tab avoids the same
/// trap: render against a `bypass_change_detection()` borrow (no implicit
/// flag), have the render fns report back whether a widget actually
/// changed something, and only call `set_changed()` when that's true — a
/// real edit still flags exactly like a normal `DerefMut` would.
///
/// Tolerates a missing egui context — a headless test app has no
/// `PrimaryWindow` entity, so this no-ops instead of panicking; the apply
/// path it feeds (`apply_creator_edits`) is a separate system precisely so
/// the headless e2e can drive it with no panel, and therefore no egui
/// context, in the loop at all — and that path mutates `CreatorState` via
/// ordinary `ResMut`/field-assignment, whose normal change detection is
/// untouched by this bypass (it only ever applies to the panel's own
/// render-time borrow).
pub(super) fn creator_panel(world: &mut World) {
    let Ok(ctx) = world
        .query_filtered::<&mut EguiContext, With<PrimaryWindow>>()
        .get_single_mut(world)
        .map(|mut c| c.get_mut().clone())
    else {
        return;
    };
    let mut cs = world.resource_mut::<CreatorState>();
    let changed = {
        let cs = cs.bypass_change_detection();
        egui::SidePanel::left("creator_panel")
            .default_width(320.0)
            .resizable(true)
            .show(&ctx, |ui| render_creator_panel(ui, cs))
            .inner
    };
    if changed {
        cs.set_changed();
    }
}

/// Team toggle + scrollable 13-name roster list, tab strip, the active
/// tab's fields, and Revert — all against `cs.working` (the panel never
/// touches `RosterDefs`/`Rosters`/the preview rig directly; that's
/// `apply_creator_edits`'s job). Returns whether any widget actually
/// changed a value this frame — [`creator_panel`] uses that to decide
/// whether to flag `CreatorState` changed at all (see its doc comment).
fn render_creator_panel(ui: &mut egui::Ui, cs: &mut CreatorState) -> bool {
    let mut changed = false;
    ui.heading("Player Creator");
    ui.separator();

    ui.horizontal(|ui| {
        changed |= ui
            .selectable_value(&mut cs.team, Team::Home, "Home")
            .changed();
        changed |= ui
            .selectable_value(&mut cs.team, Team::Away, "Away")
            .changed();
    });
    let team = cs.team;
    let pool_len = match team {
        Team::Home => cs.working.home.len(),
        Team::Away => cs.working.away.len(),
    };
    egui::ScrollArea::vertical()
        .id_salt("creator_roster_list")
        .max_height(180.0)
        .show(ui, |ui| {
            let lineup_size = LINEUP_SIZE as usize;
            for i in 0..pool_len {
                let def = match team {
                    Team::Home => &cs.working.home[i],
                    Team::Away => &cs.working.away[i],
                };
                let label = if i < lineup_size {
                    format!("{:>2}. #{:<2} {}", i + 1, def.number, def.name)
                } else {
                    format!("B{}. #{:<2} {}", i - lineup_size + 1, def.number, def.name)
                };
                if ui.selectable_label(cs.index == i, label).clicked() && cs.index != i {
                    cs.index = i;
                    changed = true;
                }
            }
        });
    ui.separator();

    ui.horizontal(|ui| {
        for (label, t) in [
            ("Identity", CreatorTab::Identity),
            ("Gear", CreatorTab::Gear),
            ("Colors", CreatorTab::Colors),
            ("Animations", CreatorTab::Animations),
        ] {
            changed |= ui.selectable_value(&mut cs.tab, t, label).changed();
        }
    });
    ui.separator();

    let tab = cs.tab;
    let (team, index) = (cs.team, cs.index);
    {
        let def = selected_def(&mut cs.working, team, index);
        changed |= match tab {
            CreatorTab::Identity => render_identity_tab(ui, def),
            CreatorTab::Gear => render_gear_tab(ui, def),
            CreatorTab::Colors => render_colors_tab(ui, def),
            CreatorTab::Animations => render_animations_tab(ui, def),
        };
    }

    ui.separator();
    ui.horizontal(|ui| {
        if ui.button("Revert").clicked() {
            // `apply_creator_edits` picks this up next frame — it reapplies
            // on any `CreatorState` change, a revert included.
            cs.working = cs.snapshot.clone();
            cs.status = "reverted to last entry".to_string();
            changed = true;
        }
        if ui.button("Save").clicked() {
            // Not `apply_creator_edits`'s job (that path never touches
            // disk) — the panel calls the pure save fn directly and routes
            // the result into `cs.status`/`cs.snapshot` itself.
            match save_working(&cs.working) {
                Ok(()) => {
                    cs.snapshot = cs.working.clone();
                    cs.status = "saved to data/players.ron".to_string();
                }
                Err(e) => cs.status = format!("save failed: {e}"),
            }
            changed = true;
        }
        if ui.button("Randomize").clicked() {
            cs.randomize_seed = cs.randomize_seed.wrapping_add(1);
            let seed = cs.randomize_seed;
            randomize_player(selected_def(&mut cs.working, team, index), seed);
            changed = true;
        }
        // Disabled rather than wired up: the portrait harness
        // (`portraits.rs`) is a `PortraitRun` phase machine that starts at
        // `Phase::WaitForMenu` and drives the *menu* into the Creator itself
        // (`start_next_shot`/`advance_after_capture`) — invoking it from
        // inside an already-open Creator would need a phase tweak to skip
        // that leg. Not required for this task; the real entry point is the
        // CLI flag below, which the tooltip now points at directly.
        ui.add_enabled(false, egui::Button::new("Portraits"))
            .on_disabled_hover_text(
            "run from the command line:\ncargo run --features \"dev debug\" -- --portraits <dir>",
        );
    });
    ui.label(cs.status.as_str());
    changed
}

fn render_identity_tab(ui: &mut egui::Ui, def: &mut PlayerDef) -> bool {
    let mut changed = false;
    ui.label("Name (A-Z, up to 8 characters — jersey font alphabet)");
    let mut name = def.name.clone();
    if ui.text_edit_singleline(&mut name).changed() {
        def.name = name
            .chars()
            .filter(|c| c.is_ascii_alphabetic())
            .map(|c| c.to_ascii_uppercase())
            .take(8)
            .collect();
        changed = true;
    }
    ui.horizontal(|ui| {
        ui.label("Number");
        changed |= ui
            .add(egui::DragValue::new(&mut def.number).range(0..=99))
            .changed();
    });
    changed
}

/// One horizontally-wrapped row of selectable buttons, one per `T::VARIANTS`
/// entry labelled from `T::NAMES` (declaration order pinned equal by
/// `appearance_enum!`, see `tests::variants_len_matches_names_for_every_appearance_enum`).
/// Returns whether the click actually changed `value` (a re-click on the
/// already-selected variant reports `false`, same as every other widget
/// here).
fn radio_grid<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    variants: &[T],
    names: &[&str],
    value: &mut T,
) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        for (variant, name) in variants.iter().zip(names.iter()) {
            changed |= ui.selectable_value(value, *variant, *name).changed();
        }
    });
    changed
}

fn render_gear_tab(ui: &mut egui::Ui, def: &mut PlayerDef) -> bool {
    let mut changed = false;
    ui.label("Headwear");
    changed |= radio_grid(
        ui,
        Headwear::VARIANTS,
        Headwear::NAMES,
        &mut def.appearance.headwear,
    );
    ui.separator();
    ui.label("Eyewear");
    changed |= radio_grid(
        ui,
        Eyewear::VARIANTS,
        Eyewear::NAMES,
        &mut def.appearance.eyewear,
    );
    ui.separator();
    ui.label("Arms");
    changed |= radio_grid(ui, Arms::VARIANTS, Arms::NAMES, &mut def.appearance.arms);
    ui.separator();
    changed |= ui.checkbox(&mut def.appearance.chain, "Chain").changed();
    changed
}

fn render_colors_tab(ui: &mut egui::Ui, def: &mut PlayerDef) -> bool {
    let mut changed = false;
    ui.label("Skin tone");
    ui.horizontal_wrapped(|ui| {
        for (variant, name) in SkinTone::VARIANTS.iter().zip(SkinTone::NAMES.iter()) {
            let rgba = variant.color().to_srgba().to_f32_array();
            let swatch = egui::Color32::from_rgb(
                (rgba[0] * 255.0) as u8,
                (rgba[1] * 255.0) as u8,
                (rgba[2] * 255.0) as u8,
            );
            let selected = def.appearance.skin == *variant;
            let button = egui::Button::new("")
                .fill(swatch)
                .min_size(egui::vec2(28.0, 28.0))
                .stroke(if selected {
                    egui::Stroke::new(2.0_f32, egui::Color32::WHITE)
                } else {
                    egui::Stroke::NONE
                });
            if ui.add(button).on_hover_text(*name).clicked() && def.appearance.skin != *variant {
                def.appearance.skin = *variant;
                changed = true;
            }
        }
    });
    changed
}

fn render_animations_tab(ui: &mut egui::Ui, def: &mut PlayerDef) -> bool {
    let mut changed = false;
    ui.label("Stance");
    changed |= radio_grid(
        ui,
        StanceId::VARIANTS,
        StanceId::NAMES,
        &mut def.appearance.style.stance,
    );
    ui.separator();
    ui.label("Fidget");
    ui.horizontal_wrapped(|ui| {
        changed |= ui
            .selectable_value(&mut def.appearance.style.fidget, None, "None")
            .changed();
        for (variant, name) in FidgetId::VARIANTS.iter().zip(FidgetId::NAMES.iter()) {
            changed |= ui
                .selectable_value(&mut def.appearance.style.fidget, Some(*variant), *name)
                .changed();
        }
    });
    ui.separator();
    ui.label("Celebration");
    changed |= radio_grid(
        ui,
        CelebrationId::VARIANTS,
        CelebrationId::NAMES,
        &mut def.appearance.style.celebration,
    );
    changed
}
