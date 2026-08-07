//! Per-player visual dressing beyond jerseys: skin tones, headwear, and
//! gear props. Everything here is driven by [`PlayerIdentity`] →
//! [`PlayerAppearance`]; the systems only mutate materials, visibility,
//! and prop children — never rules, never `ScoreBoard`.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::game::appearance::{PlayerAppearance, SkinTone};
use crate::game::model_assets::{RigAnimations, RigSkinMeshes};
use crate::game::roster::{PlayerIdentity, Rosters};
use crate::game::{GameState, Team};

/// Lazy cache of tinted skin materials, one per swatch — bounded by the
/// palette, not the roster (the `JerseyCache` precedent).
#[derive(Resource, Default)]
pub struct SkinMaterials(HashMap<SkinTone, Handle<StandardMaterial>>);

impl SkinMaterials {
    fn get(
        &mut self,
        tone: SkinTone,
        base: &Handle<StandardMaterial>,
        materials: &mut Assets<StandardMaterial>,
    ) -> Handle<StandardMaterial> {
        self.0
            .entry(tone)
            .or_insert_with(|| {
                let mut m = materials.get(base).cloned().unwrap_or_default();
                m.base_color = tone.color();
                materials.add(m)
            })
            .clone()
    }
}

/// What a rig is currently dressed as. Skipping unchanged re-stamps here
/// keeps the per-pitch identity refresh from churning materials/props.
#[derive(Component, PartialEq, Clone, Copy)]
pub struct DressedAs {
    team: Team,
    appearance: PlayerAppearance,
}

impl DressedAs {
    pub fn team(&self) -> Team {
        self.team
    }
}

/// Applies per-player skin (this task; headwear/gear arms land in Task 3)
/// whenever a rig is freshly wired or its identity's look actually changed.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn dress_rigs(
    mut commands: Commands,
    rosters: Res<Rosters>,
    anims: Option<Res<RigAnimations>>,
    mut skins: ResMut<SkinMaterials>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    rigs: Query<
        (Entity, &PlayerIdentity, &RigSkinMeshes, Option<&DressedAs>),
        Or<(Changed<PlayerIdentity>, Added<RigSkinMeshes>)>,
    >,
    mut mesh_mats: Query<&mut MeshMaterial3d<StandardMaterial>>,
) {
    let Some(anims) = anims else { return };
    for (rig, id, skin_meshes, dressed) in &rigs {
        let card = rosters.team(id.team).card(id.index);
        let target = DressedAs {
            team: id.team,
            appearance: card.appearance,
        };
        if dressed.copied() == Some(target) {
            continue; // same look — per-pitch re-stamp, nothing to do
        }
        let skin = skins.get(card.appearance.skin, &anims.skin_material, &mut materials);
        for &mesh in &skin_meshes.0 {
            if let Ok(mut mat) = mesh_mats.get_mut(mesh) {
                mat.0 = skin.clone();
            }
        }
        commands.entity(rig).insert(target);
    }
}

pub struct GearPlugin;

impl Plugin for GearPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SkinMaterials>().add_systems(
            Update,
            dress_rigs
                .after(crate::game::player::IdentitySet)
                .run_if(in_state(GameState::Playing)),
        );
    }
}
