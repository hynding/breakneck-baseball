//! Base-runner rigs — pure visualization of the `Bases` truth in rules.rs.
//! Runners never decide anything; they mirror occupancy after each play.

use bevy::prelude::*;

use crate::game::animation::MoveIntent;
use crate::game::flow::{BallInPlayEvent, Phase, Play};
use crate::game::player::{spawn_rig, Batter, RigMeshes, RigUnit, TeamPalette};
use crate::game::rules::{Bases, Outcome};
use crate::game::variant::FieldSpec;
use crate::game::{GameState, ScoreBoard};

const RUN_SPEED: f32 = 6.0;
/// Rig-root height above the base pad.
const RIG_Y: f32 = 0.6;
/// Where a new runner starts (the batter's box).
const PLATE_START: Vec3 = Vec3::new(0.7, RIG_Y, 0.0);

/// A rig standing on (or running to) 0-indexed base `base`.
#[derive(Component)]
struct Runner {
    base: usize,
}

/// Waypoints the rig visits in order (fed one at a time into `MoveIntent`).
#[derive(Component)]
struct BasePath {
    waypoints: Vec<Vec3>,
    next: usize,
}

/// Despawn the rig when its path is exhausted (scored / cleared / ghost run).
#[derive(Component)]
struct DespawnAtPathEnd;

fn base_pos(field: &FieldSpec, base: usize) -> Vec3 {
    field.base_positions[base] + Vec3::Y * RIG_Y
}

/// Waypoints for advancing from `from` (None = home plate) to `to` inclusive.
fn path_between(field: &FieldSpec, from: Option<usize>, to: usize) -> Vec<Vec3> {
    let start = from.map_or(0, |f| f + 1);
    (start..=to).map(|b| base_pos(field, b)).collect()
}

/// Waypoints from base `from` around the remaining bases and home.
fn path_home(field: &FieldSpec, from: usize) -> Vec<Vec3> {
    let mut waypoints: Vec<Vec3> = ((from + 1)..field.base_count())
        .map(|b| base_pos(field, b))
        .collect();
    waypoints.push(Vec3::new(0.0, RIG_Y, 0.0));
    waypoints
}

/// Feeds the next waypoint whenever the rig has arrived at the previous one.
fn advance_paths(
    mut movers: Query<(
        Entity,
        &mut BasePath,
        &mut MoveIntent,
        Option<&DespawnAtPathEnd>,
    )>,
    mut commands: Commands,
) {
    for (entity, mut path, mut intent, despawn) in &mut movers {
        if intent.target.is_some() {
            continue;
        }
        if path.next < path.waypoints.len() {
            intent.target = Some(path.waypoints[path.next]);
            intent.speed = RUN_SPEED;
            path.next += 1;
        } else if despawn.is_some() {
            commands.entity(entity).despawn_recursive();
        }
    }
}

/// Mirrors `Bases` after every change: existing runners advance (greedy,
/// most-advanced first), a new runner appears for the batter's base, and
/// leftovers (scored, or wiped by a half-inning flip) run home and leave.
fn sync_runners(
    bases: Res<Bases>,
    field: Res<FieldSpec>,
    score: Res<ScoreBoard>,
    rig_meshes: Option<Res<RigMeshes>>,
    palette: Option<Res<TeamPalette>>,
    mut runners: Query<(Entity, &mut Runner)>,
    mut commands: Commands,
) {
    if !bases.is_changed() {
        return;
    }
    let (Some(rig_meshes), Some(palette)) = (rig_meshes, palette) else {
        return;
    };

    // Existing runners, most advanced first.
    let mut pool: Vec<(Entity, usize)> = runners.iter().map(|(e, r)| (e, r.base)).collect();
    pool.sort_by(|a, b| b.1.cmp(&a.1));

    let occupied: Vec<usize> = (0..bases.count())
        .filter(|&b| bases.is_occupied(b))
        .collect();
    let mut unmatched: Vec<usize> = Vec::new();

    for &target in occupied.iter().rev() {
        if let Some(i) = pool.iter().position(|&(_, from)| from <= target) {
            let (entity, from) = pool.remove(i);
            if from != target {
                commands.entity(entity).insert(BasePath {
                    waypoints: path_between(&field, Some(from), target),
                    next: 0,
                });
                if let Ok((_, mut runner)) = runners.get_mut(entity) {
                    runner.base = target;
                }
            }
        } else {
            unmatched.push(target);
        }
    }

    // The batter reaching base: spawn a fresh runner at the plate.
    for target in unmatched {
        let mats = palette.for_team(score.batting_team());
        let entity = spawn_rig(
            &mut commands,
            &rig_meshes,
            RigUnit::Batter,
            mats,
            PLATE_START,
            1.0,
        );
        commands.entity(entity).insert((
            Runner { base: target },
            BasePath {
                waypoints: path_between(&field, None, target),
                next: 0,
            },
        ));
    }

    // Leftovers scored or were cleared: run home and leave the field.
    for (entity, from) in pool {
        commands.entity(entity).insert((
            BasePath {
                waypoints: path_home(&field, from),
                next: 0,
            },
            DespawnAtPathEnd,
        ));
        commands.entity(entity).remove::<Runner>();
    }
}

/// On fair contact the batter always runs — like real baseball, even on outs.
/// Hits and walks get their runner from [`sync_runners`]; outs get a ghost
/// run to first and home runs a full trot, both despawning at path end. The
/// real batter rig hides for the duration and "steps back in" at PrePitch.
fn batter_runs(
    mut events: EventReader<BallInPlayEvent>,
    field: Res<FieldSpec>,
    score: Res<ScoreBoard>,
    rig_meshes: Option<Res<RigMeshes>>,
    palette: Option<Res<TeamPalette>>,
    mut batter_q: Query<&mut Visibility, With<Batter>>,
    mut commands: Commands,
) {
    for ev in events.read() {
        let (Some(rig_meshes), Some(palette)) = (&rig_meshes, &palette) else {
            return;
        };

        // The batter leaves the box on every fair ball; on hits the runner
        // spawned by sync_runners is the batter, visually.
        for mut visibility in &mut batter_q {
            *visibility = Visibility::Hidden;
        }

        let waypoints = match ev.outcome {
            // Run through first, then peel off.
            Outcome::Out(_) => path_between(&field, None, 0),
            // The trot: every base, then home.
            Outcome::HomeRun => {
                let mut wp: Vec<Vec3> = (0..field.base_count())
                    .map(|b| base_pos(&field, b))
                    .collect();
                wp.push(Vec3::new(0.0, RIG_Y, 0.0));
                wp
            }
            _ => continue,
        };

        let mats = palette.for_team(score.batting_team());
        let entity = spawn_rig(
            &mut commands,
            rig_meshes,
            RigUnit::Batter,
            mats,
            PLATE_START,
            1.0,
        );
        commands
            .entity(entity)
            .insert((BasePath { waypoints, next: 0 }, DespawnAtPathEnd));
    }
}

/// The next at-bat begins: the batter steps back into the box.
fn batter_returns(play: Res<Play>, mut batter_q: Query<&mut Visibility, With<Batter>>) {
    if play.phase != Phase::PrePitch {
        return;
    }
    for mut visibility in &mut batter_q {
        if *visibility != Visibility::Inherited {
            *visibility = Visibility::Inherited;
        }
    }
}

pub struct RunnerPlugin;

impl Plugin for RunnerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (batter_runs, sync_runners, advance_paths, batter_returns)
                .chain()
                .run_if(in_state(GameState::Playing)),
        );
    }
}
