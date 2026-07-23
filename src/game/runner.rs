//! Base-runner rigs — pure visualization of the `Bases` truth in rules.rs.
//! Runners never decide anything; they mirror occupancy after each play.

use bevy::prelude::*;

use crate::game::animation::{AnimClip, MoveIntent, Playing};
use crate::game::flow::{BallInPlayEvent, LeadState, Phase, Play};
use crate::game::player::{spawn_rig, Batter, RigMeshes, RigUnit, TeamPalette};
use crate::game::rules::{self, Bases, ContactKind};
use crate::game::variant::FieldSpec;
use crate::game::{GameState, ScoreBoard};

/// Matches `rules::RUNNER_SPEED` so the rigs arrive when the umpire says.
const RUN_SPEED: f32 = crate::game::rules::RUNNER_SPEED;
/// Rig-root height above the base pad.
const RIG_Y: f32 = 0.6;
/// Where a new runner starts (the batter's box).
const PLATE_START: Vec3 = Vec3::new(0.7, RIG_Y, 0.0);
/// Leadoff distances off the bag toward the next base (metres): the normal
/// lead every runner takes, and the stretched lead that arms the early break.
const LEAD_NORMAL: f32 = 2.2;
const LEAD_EXTENDED: f32 = 4.5;
/// Shuffle speed for taking / retreating from a lead.
const LEAD_SPEED: f32 = 2.6;
/// Distance from the bag at which an arriving runner drops into the slide.
const SLIDE_RANGE: f32 = 2.2;

/// Whether every runner rig has finished its base path — the flow's gate for
/// the next at-bat (the play isn't over while the trot is still running).
#[derive(Resource)]
pub struct RunnersSettled(pub bool);

impl Default for RunnersSettled {
    fn default() -> Self {
        RunnersSettled(true)
    }
}

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

/// The batter running out a live ball whose call hasn't come yet. If the
/// resolution puts the batter on base, [`sync_runners`] adopts this rig's
/// position so the runner doesn't teleport back to the plate.
#[derive(Component)]
struct BatterGhost;

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
        } else {
            // Path exhausted and arrived: the rig has settled on its base.
            commands.entity(entity).remove::<BasePath>();
        }
    }
}

/// Mirrors whether any rig still has a live base path into
/// [`RunnersSettled`], the flow's end-of-play gate.
fn track_settled(paths: Query<(), With<BasePath>>, mut settled: ResMut<RunnersSettled>) {
    let now_settled = paths.is_empty();
    if settled.0 != now_settled {
        settled.0 = now_settled;
    }
}

/// Leadoffs: during the pre-pitch duel and the delivery, the lead eligible
/// runner shuffles off the bag — a normal lead, stretched while the offense
/// holds Down ([`LeadState`]), and a full sprint for the next bag once the
/// steal is on and the pitch is in the air. Everyone else stays planted, and
/// rigs already running a base path are left alone.
#[allow(clippy::type_complexity)]
fn take_leadoffs(
    play: Res<Play>,
    lead: Res<LeadState>,
    bases: Res<Bases>,
    field: Res<FieldSpec>,
    mut runners: Query<(&Runner, &Transform, &mut MoveIntent), Without<BasePath>>,
) {
    let dueling = matches!(play.phase, Phase::PrePitch | Phase::WindUp | Phase::Pitch);
    let candidate = rules::steal_candidate(&bases);
    for (runner, tf, mut intent) in &mut runners {
        let bag = base_pos(&field, runner.base);
        let target = if dueling && Some(runner.base) == candidate {
            let next = base_pos(&field, runner.base + 1);
            let dir = (next - bag).normalize_or_zero();
            if play.phase == Phase::Pitch && play.runners_going() {
                // He's off with the pitch — the resolution at the catcher
                // will repath him (safe) or send him off (caught).
                intent.target = Some(next);
                intent.speed = RUN_SPEED;
                continue;
            }
            let dist = if lead.extended {
                LEAD_EXTENDED
            } else {
                LEAD_NORMAL
            };
            bag + dir * dist
        } else {
            bag
        };
        if (tf.translation - target).length() > 0.25 {
            intent.target = Some(target);
            intent.speed = LEAD_SPEED;
        }
    }
}

/// Drops an arriving runner into the slide for the last couple of metres of
/// the final leg of his path — pure presentation on top of the same
/// [`MoveIntent`] locomotion.
#[allow(clippy::type_complexity)]
fn slide_into_base(
    movers: Query<(Entity, &BasePath, &MoveIntent, &Transform, Option<&Playing>), With<Runner>>,
    mut commands: Commands,
) {
    for (entity, path, intent, tf, playing) in &movers {
        let Some(target) = intent.target else {
            continue;
        };
        let last_leg = path.next >= path.waypoints.len();
        let close = (tf.translation - target).length() < SLIDE_RANGE;
        let sliding = playing.is_some_and(|p| p.clip == AnimClip::Slide);
        if last_leg && close && !sliding {
            commands
                .entity(entity)
                .insert(Playing::new(AnimClip::Slide));
        }
    }
}

/// Mirrors `Bases` after every change: existing runners advance (greedy,
/// most-advanced first), a new runner appears for the batter's base, and
/// leftovers (scored, or wiped by a half-inning flip) run home and leave.
#[allow(clippy::too_many_arguments)]
fn sync_runners(
    bases: Res<Bases>,
    field: Res<FieldSpec>,
    score: Res<ScoreBoard>,
    rig_meshes: Option<Res<RigMeshes>>,
    palette: Option<Res<TeamPalette>>,
    mut runners: Query<(Entity, &mut Runner)>,
    ghosts: Query<(Entity, &Transform), With<BatterGhost>>,
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
    pool.sort_by_key(|&(_, base)| std::cmp::Reverse(base));

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

    // The batter reaching base: spawn a fresh runner — from wherever the
    // run-out ghost already got to, if one is still on the basepath.
    for target in unmatched {
        let start = ghosts.iter().next().map_or(PLATE_START, |(ghost, tf)| {
            commands.entity(ghost).despawn_recursive();
            tf.translation
        });
        let mats = palette.for_team(score.batting_team());
        let entity = spawn_rig(
            &mut commands,
            &rig_meshes,
            RigUnit::Batter,
            mats,
            start,
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

        let (waypoints, ghost) = match ev.kind {
            // The trot: every base, then home.
            ContactKind::HomeRun => {
                let mut wp: Vec<Vec3> = (0..field.base_count())
                    .map(|b| base_pos(&field, b))
                    .collect();
                wp.push(Vec3::new(0.0, RIG_Y, 0.0));
                (wp, false)
            }
            // A live fair ball: run it out — nobody knows the call yet.
            ContactKind::Live { fair: true } => (path_between(&field, None, 0), true),
            ContactKind::Live { fair: false } => continue,
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
        if ghost {
            commands.entity(entity).insert(BatterGhost);
        }
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
        app.init_resource::<RunnersSettled>().add_systems(
            Update,
            (
                batter_runs,
                sync_runners,
                advance_paths,
                take_leadoffs,
                slide_into_base,
                track_settled,
                batter_returns,
            )
                .chain()
                .run_if(in_state(GameState::Playing)),
        );
    }
}
