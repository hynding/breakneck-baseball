//! Base-runner rigs — pure visualization of the `Bases` truth in rules.rs.
//! Runners never decide anything; they mirror occupancy after each play.

use bevy::prelude::*;

use crate::game::animation::{AnimClip, MoveIntent, Playing};
use crate::game::flow::{BallInPlayEvent, LeadState, LiveBallEvent, Phase, Play};
use crate::game::player::{spawn_rig, Batter, RigModel, RigUnit, TeamPalette};
use crate::game::rules::{self, Bases, ContactKind, RunnerBreak};
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

/// A rig standing on (or running to) 0-indexed base `base`. Public so headless
/// tests can read rig positions against the bags; the field itself is private.
#[derive(Component)]
pub struct Runner {
    pub base: usize,
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

/// A freshly spawned run-out rig waiting (hidden) at the plate while the
/// real batter finishes the swing follow-through — or admires the home run.
/// When the delay expires the batter rig hides and this one takes over, so
/// the swing is actually seen instead of the batter vanishing at contact.
#[derive(Component)]
struct RunDelay(Timer);

/// Seconds the batter holds the box after fair contact before the run-out rig
/// breaks for first. Kept small (≤ 0.2 s) so the batter is running almost the
/// instant he makes contact — just long enough to see the swing follow-through
/// and bat drop before the seamless swap to the run-out rig. Purely visual: it
/// does not feed the race math (the umpire charges its own reaction delay).
const RUN_OUT_DELAY: f32 = 0.15;
/// A home run earns a longer look before the trot starts.
const TROT_DELAY: f32 = 0.9;

/// The batter running out a live ball whose call hasn't come yet. If the
/// resolution puts the batter on base, [`sync_runners`] adopts this rig's
/// position so the runner doesn't teleport back to the plate.
#[derive(Component)]
struct BatterGhost;

/// How a runner aboard is currently breaking off contact, before the umpire's
/// call arrives (see [`rules::runner_break`]). A runner *without* this
/// component is holding his bag (a tag-up, or nobody in a position to run) —
/// [`take_leadoffs`] parks him there. The runner's `base` is left untouched
/// while breaking, so resolution ([`run_out_pending_call`] / [`sync_runners`])
/// re-paths from his true origin bag.
#[derive(Component, Clone, Copy, PartialEq)]
enum Breaking {
    /// Running for the next bag on contact.
    GoNow,
    /// Halfway off, reading the fly.
    Halfway,
    /// Read a catch — retreating to the origin bag.
    Retreat,
}

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
/// Rigs still serving their [`RunDelay`] hold at the plate.
#[allow(clippy::type_complexity)]
fn advance_paths(
    mut movers: Query<
        (
            Entity,
            &mut BasePath,
            &mut MoveIntent,
            Option<&DespawnAtPathEnd>,
        ),
        Without<RunDelay>,
    >,
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
    mut runners: Query<
        (&Runner, &Transform, &mut MoveIntent),
        (Without<BasePath>, Without<Breaking>),
    >,
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
    rig_model: Option<Res<RigModel>>,
    palette: Option<Res<TeamPalette>>,
    mut runners: Query<(Entity, &mut Runner)>,
    ghosts: Query<(Entity, &Transform), With<BatterGhost>>,
    mut commands: Commands,
) {
    if !bases.is_changed() {
        return;
    }
    let (Some(rig_model), Some(palette)) = (rig_model, palette) else {
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
        let entity = spawn_rig(&mut commands, &rig_model, RigUnit::Batter, mats, start, 1.0);
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
/// run-out rig waits hidden through a short [`RunDelay`] so the real batter
/// is seen finishing the swing before the swap; fouls leave him in the box.
fn batter_runs(
    mut events: EventReader<BallInPlayEvent>,
    field: Res<FieldSpec>,
    score: Res<ScoreBoard>,
    rig_model: Option<Res<RigModel>>,
    palette: Option<Res<TeamPalette>>,
    mut commands: Commands,
) {
    for ev in events.read() {
        let (Some(rig_model), Some(palette)) = (&rig_model, &palette) else {
            return;
        };

        let (waypoints, ghost, delay) = match ev.kind {
            // The trot: every base, then home — after a look at the ball.
            ContactKind::HomeRun => {
                let mut wp: Vec<Vec3> = (0..field.base_count())
                    .map(|b| base_pos(&field, b))
                    .collect();
                wp.push(Vec3::new(0.0, RIG_Y, 0.0));
                (wp, false, TROT_DELAY)
            }
            // A live fair ball: run it out — nobody knows the call yet.
            ContactKind::Live { fair: true } => {
                (path_between(&field, None, 0), true, RUN_OUT_DELAY)
            }
            ContactKind::Live { fair: false } => continue,
        };

        let mats = palette.for_team(score.batting_team());
        let entity = spawn_rig(
            &mut commands,
            rig_model,
            RigUnit::Batter,
            mats,
            PLATE_START,
            1.0,
        );
        commands.entity(entity).insert((
            BasePath { waypoints, next: 0 },
            DespawnAtPathEnd,
            RunDelay(Timer::from_seconds(delay, TimerMode::Once)),
            Visibility::Hidden,
        ));
        if ghost {
            commands.entity(entity).insert(BatterGhost);
        }
    }
}

/// Counts down each [`RunDelay`]; on expiry the stand-in rig appears, the
/// real batter rig hides, and the run begins — a seamless handoff at the
/// plate that lets the swing follow-through actually be seen.
#[allow(clippy::type_complexity)]
fn tick_run_delays(
    time: Res<Time>,
    mut delayed: Query<(Entity, &mut RunDelay, &mut Visibility), Without<Batter>>,
    mut batter_q: Query<&mut Visibility, (With<Batter>, Without<RunDelay>)>,
    mut commands: Commands,
) {
    for (entity, mut delay, mut visibility) in &mut delayed {
        if delay.0.tick(time.delta()).finished() {
            commands.entity(entity).remove::<RunDelay>();
            *visibility = Visibility::Inherited;
            for mut batter_visibility in &mut batter_q {
                *batter_visibility = Visibility::Hidden;
            }
        }
    }
}

/// A hit has been decided but the throw is still in the air
/// ([`Play::pending_hit`]): the play is visually alive, so everyone breaks
/// for the bases the call will give them *now* — the batter ghost becomes a
/// real runner rounding first while the outfielder's throw comes in, and the
/// runners aboard sprint (or score) ahead of the announcement. When the call
/// applies, [`sync_runners`] finds every rig already on (or heading to) its
/// base and has nothing left to move.
#[allow(clippy::type_complexity)]
fn run_out_pending_call(
    play: Res<Play>,
    field: Res<FieldSpec>,
    mut handled: Local<bool>,
    ghosts: Query<Entity, With<BatterGhost>>,
    mut runners: Query<(Entity, &mut Runner), Without<BatterGhost>>,
    mut commands: Commands,
) {
    let Some(hit_bases) = play.pending_hit() else {
        *handled = false;
        return;
    };
    if *handled {
        return;
    }
    *handled = true;
    let count = field.base_count();
    let jump = play.runners_going() as usize;

    // Runners aboard take the bases the decided call gives them.
    for (entity, mut runner) in &mut runners {
        let dest = runner.base + hit_bases as usize + jump;
        if dest >= count {
            commands.entity(entity).insert((
                BasePath {
                    waypoints: path_home(&field, runner.base),
                    next: 0,
                },
                DespawnAtPathEnd,
            ));
            commands.entity(entity).remove::<Runner>();
        } else {
            commands.entity(entity).insert(BasePath {
                waypoints: path_between(&field, Some(runner.base), dest),
                next: 0,
            });
            runner.base = dest;
        }
    }

    // The run-out ghost becomes the real runner and keeps going for the
    // extra bases while the ball is in the air. `hit_bases` is provably >= 1
    // here (`pending_hit()` only ever surfaces `Outcome::Hit(n)` with n >= 1
    // — see rules.rs's `resolve_catch`/`resolve_thrown` construction sites),
    // but saturate rather than trust that invariant across future callers.
    let batter_dest = (hit_bases as usize).min(count).saturating_sub(1);
    if let Some(ghost) = ghosts.iter().next() {
        commands
            .entity(ghost)
            .remove::<BatterGhost>()
            .remove::<DespawnAtPathEnd>()
            .insert(Runner { base: batter_dest });
        if batter_dest > 0 {
            // Queued behind the leg to first that's already running.
            commands.entity(ghost).insert(BasePath {
                waypoints: path_between(&field, Some(0), batter_dest),
                next: 0,
            });
        }
    }
}

/// World position of the bag one past `base` — home plate once the runner is
/// rounding the last base.
fn next_bag_pos(field: &FieldSpec, base: usize) -> Vec3 {
    if base + 1 >= field.base_count() {
        Vec3::new(0.0, RIG_Y, 0.0)
    } else {
        base_pos(field, base + 1)
    }
}

/// On fair contact, each runner aboard breaks off the bat per the real-baseball
/// read ([`rules::runner_break`]): a forced grounder or any two-out ball is a
/// break for the next bag; a catchable fly (or an unforced grounder) edges
/// halfway to read the play; a deep fly holds to tag up. The runner's `base`
/// is left as-is — this only starts the rig moving; the umpire's call still
/// comes from the live-play races and is reconciled at resolution. Home runs
/// (already resolved) and fouls are left to the trot / reset paths.
fn break_runners(
    mut events: EventReader<BallInPlayEvent>,
    score: Res<ScoreBoard>,
    bases: Res<Bases>,
    mut runners: Query<(Entity, &Runner)>,
    mut commands: Commands,
) {
    for ev in events.read() {
        if !matches!(ev.kind, ContactKind::Live { fair: true }) {
            continue;
        }
        for (entity, runner) in &mut runners {
            let forced = rules::is_forced(&bases, runner.base);
            match rules::runner_break(score.outs, forced, ev.contact_class) {
                RunnerBreak::GoNow => {
                    commands.entity(entity).insert(Breaking::GoNow);
                }
                RunnerBreak::Halfway => {
                    commands.entity(entity).insert(Breaking::Halfway);
                }
                // Tag-ups hold the bag; take_leadoffs parks them there.
                RunnerBreak::TagUp => {
                    commands.entity(entity).remove::<Breaking>();
                }
            }
        }
    }
}

/// Halfway runners read the fly: a catch turns them around (`Retreat` to the
/// bag); a fair landing commits them (`GoNow`) only once it's actually
/// through the infield (`rules::landed_past_infield`) — a fair ball that
/// drops but stays in the infield is the same "it got down but I can't make
/// it" read as a catch, so it also sends the runner back (`Retreat`). Only
/// active while the ball is live and uncalled.
fn read_break_reads(
    mut events: EventReader<LiveBallEvent>,
    play: Res<Play>,
    field: Res<FieldSpec>,
    mut breaking: Query<&mut Breaking>,
) {
    if play.phase != Phase::InPlay {
        events.clear();
        return;
    }
    for ev in events.read() {
        let commit = match *ev {
            LiveBallEvent::Caught { .. } => Some(Breaking::Retreat),
            LiveBallEvent::Landed { pos } if rules::is_fair(pos, &field) => {
                Some(if rules::landed_past_infield(pos, &field) {
                    Breaking::GoNow
                } else {
                    Breaking::Retreat
                })
            }
            _ => None,
        };
        let Some(commit) = commit else { continue };
        for mut b in &mut breaking {
            if *b == Breaking::Halfway {
                *b = commit;
            }
        }
    }
}

/// Drives the rigs of runners currently breaking off contact toward the target
/// their read implies — the next bag (GoNow), the midpoint (Halfway), or back
/// to the origin bag (Retreat). All motion is a [`MoveIntent`]; the runner's
/// `base` stays put so resolution can re-path from the true origin. Runners
/// already handed an authoritative [`BasePath`] (a decided call) are left to it.
#[allow(clippy::type_complexity)]
fn drive_breaks(
    field: Res<FieldSpec>,
    mut runners: Query<(&Runner, &Breaking, &Transform, &mut MoveIntent), Without<BasePath>>,
) {
    for (runner, breaking, tf, mut intent) in &mut runners {
        let bag = base_pos(&field, runner.base);
        let next = next_bag_pos(&field, runner.base);
        let target = match breaking {
            Breaking::GoNow => next,
            Breaking::Halfway => bag.lerp(next, 0.5),
            Breaking::Retreat => bag,
        };
        if (tf.translation - target).length() > 0.25 {
            intent.target = Some(target);
            intent.speed = RUN_SPEED;
        }
    }
}

/// Clears the breaking state once it no longer applies: the play left `InPlay`
/// (reset for the next at-bat), or the runner was handed a real [`BasePath`] by
/// the resolution — from there the normal path/leadoff machinery takes over.
#[allow(clippy::type_complexity)]
fn clear_breaks(
    play: Res<Play>,
    breaking: Query<Entity, With<Breaking>>,
    pathed: Query<Entity, (With<Breaking>, With<BasePath>)>,
    mut commands: Commands,
) {
    if play.phase != Phase::InPlay {
        for entity in &breaking {
            commands.entity(entity).remove::<Breaking>();
        }
        return;
    }
    for entity in &pathed {
        commands.entity(entity).remove::<Breaking>();
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
                break_runners,
                tick_run_delays,
                run_out_pending_call,
                sync_runners,
                clear_breaks,
                read_break_reads,
                drive_breaks,
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
