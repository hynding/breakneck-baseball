//! Per-player visual dressing beyond jerseys: skin tones, headwear, and
//! gear props. Everything here is driven by [`PlayerIdentity`] →
//! [`PlayerAppearance`]; the systems only mutate materials, visibility,
//! and prop children — never rules, never `ScoreBoard`.

use std::collections::HashMap;
use std::mem::take;

use bevy::prelude::*;

use crate::game::appearance::{Arms, Eyewear, Headwear, PlayerAppearance, SkinTone};
use crate::game::model_assets::{
    GltfTeamMaterials, RigAnimations, RigBones, RigCapMeshes, RigSkinMeshes,
};
use crate::game::roster::{PlayerIdentity, Rosters};
use crate::game::Team;

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

/// Shared prop meshes + fixed materials, built once at startup. Team-tinted
/// props (helmet, backwards cap) borrow [`GltfTeamMaterials`] at spawn
/// instead of owning their own material here.
#[derive(Resource)]
pub struct GearAssets {
    helmet: Handle<Mesh>,
    cap_crown: Handle<Mesh>,
    cap_brim: Handle<Mesh>,
    lens: Handle<Mesh>,
    visor: Handle<Mesh>,
    eye_black: Handle<Mesh>,
    wristband: Handle<Mesh>,
    chain: Handle<Mesh>,
    dark: Handle<StandardMaterial>,
    white: Handle<StandardMaterial>,
    gold: Handle<StandardMaterial>,
}

fn build_gear_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(GearAssets {
        // Slightly bigger than the baked skin head sphere (radius 0.16, see
        // `spawn_prop`'s call sites below) so the shell doesn't z-fight it.
        helmet: meshes.add(Sphere::new(0.185)),
        cap_crown: meshes.add(Cylinder::new(0.13, 0.08)),
        cap_brim: meshes.add(Cuboid::new(0.20, 0.02, 0.12)),
        lens: meshes.add(Cuboid::new(0.07, 0.05, 0.02)),
        visor: meshes.add(Cuboid::new(0.22, 0.05, 0.02)),
        eye_black: meshes.add(Cuboid::new(0.05, 0.025, 0.005)),
        wristband: meshes.add(Cylinder::new(0.055, 0.05)),
        chain: meshes.add(Torus {
            minor_radius: 0.012,
            major_radius: 0.09,
        }),
        dark: materials.add(StandardMaterial {
            base_color: Color::srgb(0.03, 0.03, 0.04),
            perceptual_roughness: 0.6,
            ..default()
        }),
        white: materials.add(StandardMaterial {
            base_color: Color::srgb(0.95, 0.95, 0.95),
            ..default()
        }),
        gold: materials.add(StandardMaterial {
            base_color: Color::srgb(0.83, 0.68, 0.21),
            metallic: 0.8,
            perceptual_roughness: 0.25,
            ..default()
        }),
    });
}

/// One spawned gear prop (marker; the owning rig tracks them in [`RigGear`]).
#[derive(Component)]
pub struct GearProp;

/// The gear prop entities this rig currently wears — despawned and rebuilt
/// whenever the look changes.
#[derive(Component, Default)]
pub struct RigGear(pub Vec<Entity>);

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

/// Spawns one gear prop parented to `bone` — never per-frame transform
/// copying, the prop rides the bone's own animated transform for free.
fn spawn_prop(
    commands: &mut Commands,
    bone: Entity,
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    transform: Transform,
) -> Entity {
    let prop = commands
        .spawn((GearProp, Mesh3d(mesh), MeshMaterial3d(material), transform))
        .id();
    commands.entity(bone).add_child(prop);
    prop
}

/// Applies per-player skin, headwear, eyewear, arm, and chain gear
/// whenever a rig is freshly wired or its identity's look actually changed.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn dress_rigs(
    mut commands: Commands,
    rosters: Res<Rosters>,
    anims: Option<Res<RigAnimations>>,
    team_mats: Option<Res<GltfTeamMaterials>>,
    gear: Option<Res<GearAssets>>,
    mut skins: ResMut<SkinMaterials>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut rigs: Query<
        (
            Entity,
            &PlayerIdentity,
            &RigSkinMeshes,
            &RigCapMeshes,
            &RigBones,
            Option<&DressedAs>,
            Option<&mut RigGear>,
        ),
        Or<(
            Changed<PlayerIdentity>,
            Added<RigSkinMeshes>,
            Added<RigBones>,
        )>,
    >,
    mut mesh_mats: Query<&mut MeshMaterial3d<StandardMaterial>>,
) {
    let Some(anims) = anims else { return };
    let Some(team_mats) = team_mats else { return };
    let Some(gear) = gear else { return };
    for (rig, id, skin_meshes, cap_meshes, bones, dressed, rig_gear) in &mut rigs {
        let card = rosters.team(id.team).card(id.index);
        let appearance = card.appearance;
        let target = DressedAs {
            team: id.team,
            appearance,
        };
        if dressed.copied() == Some(target) {
            continue; // same look — per-pitch re-stamp, nothing to do
        }
        let skin = skins.get(appearance.skin, &anims.skin_material, &mut materials);
        for &mesh in &skin_meshes.0 {
            if let Ok(mut mat) = mesh_mats.get_mut(mesh) {
                mat.0 = skin.clone();
            }
        }

        // 1. Cap visibility: the baked cap only shows for `Headwear::Cap`.
        let cap_visibility = if appearance.headwear == Headwear::Cap {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        for &mesh in &cap_meshes.0 {
            commands.entity(mesh).insert(cap_visibility);
        }

        // 2. Despawn this rig's old props before rebuilding.
        if let Some(mut rig_gear) = rig_gear {
            for e in take(&mut rig_gear.0) {
                commands.entity(e).despawn_recursive();
            }
        }

        // 3. Spawn fresh props for the new look.
        let team = id.team;
        let mut props = Vec::new();

        // Head-bone-local Y offsets below are pinned to `tools/build_player.py`'s
        // BONES/PARTS tables: the Head bone's own origin sits at the neck
        // (Blender armature-space z=1.50), and the baked skin head sphere is
        // centred 0.16 above that with radius 0.16 (so it spans local Y 0.0
        // .. 0.32) — the baked cap crown sits right on top at Y≈0.32. Gear
        // must sit in that same band, not near the bone origin (Y≈0.06 buries
        // a prop inside the neck/collar, invisible) — confirmed empirically
        // via a web build screenshot before this tune pass.
        match appearance.headwear {
            Headwear::Helmet => {
                props.push(spawn_prop(
                    &mut commands,
                    bones.head,
                    gear.helmet.clone(),
                    team_mats.cap(team),
                    Transform::from_xyz(0.0, 0.16, 0.0),
                ));
            }
            Headwear::CapBackwards => {
                props.push(spawn_prop(
                    &mut commands,
                    bones.head,
                    gear.cap_crown.clone(),
                    team_mats.cap(team),
                    Transform::from_xyz(0.0, 0.31, 0.0),
                ));
                props.push(spawn_prop(
                    &mut commands,
                    bones.head,
                    gear.cap_brim.clone(),
                    team_mats.cap(team),
                    Transform::from_xyz(0.0, 0.29, -0.16),
                ));
            }
            Headwear::Bare | Headwear::Cap => {}
        }

        match appearance.eyewear {
            Eyewear::Glasses => {
                for sign in [-1.0_f32, 1.0] {
                    props.push(spawn_prop(
                        &mut commands,
                        bones.head,
                        gear.lens.clone(),
                        gear.dark.clone(),
                        Transform::from_xyz(sign * 0.055, 0.18, 0.19),
                    ));
                }
            }
            Eyewear::Shades => {
                props.push(spawn_prop(
                    &mut commands,
                    bones.head,
                    gear.visor.clone(),
                    gear.dark.clone(),
                    Transform::from_xyz(0.0, 0.18, 0.19),
                ));
            }
            Eyewear::EyeBlack => {
                for sign in [-1.0_f32, 1.0] {
                    props.push(spawn_prop(
                        &mut commands,
                        bones.head,
                        gear.eye_black.clone(),
                        gear.dark.clone(),
                        Transform::from_xyz(sign * 0.05, 0.12, 0.19),
                    ));
                }
            }
            Eyewear::Bare => {}
        }

        let wristband = |commands: &mut Commands, bone: Entity| {
            spawn_prop(
                commands,
                bone,
                gear.wristband.clone(),
                gear.white.clone(),
                Transform::from_xyz(0.0, -0.18, 0.0),
            )
        };
        match appearance.arms {
            Arms::WristbandL => props.push(wristband(&mut commands, bones.lower_arm_l)),
            Arms::WristbandR => props.push(wristband(&mut commands, bones.lower_arm_r)),
            Arms::WristbandsBoth => {
                props.push(wristband(&mut commands, bones.lower_arm_l));
                props.push(wristband(&mut commands, bones.lower_arm_r));
            }
            Arms::Bare => {}
        }

        if appearance.chain {
            // The baked jersey torso cube (`tools/build_player.py`'s Spine
            // part) reaches to local Z≈0.12 in front of the Spine bone — a
            // chain at Z=0.02 sits inside the mesh and z-fights invisibly
            // (confirmed via a web build screenshot); Z=0.19 clears its front
            // face and drapes over the collar instead.
            props.push(spawn_prop(
                &mut commands,
                bones.spine,
                gear.chain.clone(),
                gear.gold.clone(),
                Transform::from_xyz(0.0, 0.27, 0.19)
                    .with_rotation(Quat::from_rotation_x(85f32.to_radians())),
            ));
        }

        // 4. Record what's spawned and re-stamp the look.
        commands.entity(rig).insert((target, RigGear(props)));
    }
}

pub struct GearPlugin;

impl Plugin for GearPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SkinMaterials>()
            .add_systems(Startup, build_gear_assets)
            .add_systems(
                Update,
                dress_rigs
                    .after(crate::game::player::IdentitySet)
                    .run_if(crate::game::dressing_active),
            );
    }
}
