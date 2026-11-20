//! Small effects on the game's physical moments (graphics plan task 13):
//! dirt sprays off slides and dives, a dust pop from the mitt on a hard
//! fastball, a chalk puff when a ball lands on a foul line, and confetti
//! with streamers on a walk-off. Each burst is authored at its High count
//! and spawned through [`FxBudget`]; none of it reads back into play.

use bevy::prelude::*;

use crate::game::GameplayEntity;
use crate::game::ai::{hash01, noise};
use crate::game::animation::{AnimClip, Playing};
use crate::game::ball::PitchEvent;
use crate::game::flow::{LiveBallEvent, PitchCaughtEvent, Play};
use crate::game::player::CatcherRole;
use crate::game::variant::FieldSpec;

use super::FxAssets;
use super::budget::FxBudget;
use super::particles::{Flutter, Particle};

/// Dirt motes in a slide's or a dive's spray, at High.
const SPRAY_MOTES: usize = 14;
/// How far into each clip the body meets the dirt (fraction of the clip).
const SLIDE_CONTACT: f32 = 0.15;
const DIVE_CONTACT: f32 = 0.55;

/// Where in `clip` the spray fires (seconds from its start), if it sprays.
pub(super) fn spray_at(clip: AnimClip) -> Option<f32> {
    match clip {
        AnimClip::Slide => Some(SLIDE_CONTACT * clip.duration()),
        AnimClip::Dive => Some(DIVE_CONTACT * clip.duration()),
        _ => None,
    }
}

/// True on the frame a clip's clock crosses `at`: short of it last frame
/// (`before`), at or past it now.
pub(super) fn crossed(at: f32, before: f32, now: f32) -> bool {
    before < at && now >= at
}

/// A slide into a bag or a dive for a ball throws up a spray of dirt where
/// the body hits the ground. Each sliding or diving rig's clip time from
/// the frame before is kept here, so the crossing is read off the clip's
/// own clock whichever side of the animation systems this one runs.
pub(super) fn slide_and_dive_spray(
    rigs: Query<(Entity, &Transform, &Playing)>,
    assets: Option<Res<FxAssets>>,
    time: Res<Time>,
    mut last: Local<bevy::platform::collections::HashMap<Entity, (AnimClip, f32)>>,
    mut budget: FxBudget,
    mut commands: Commands,
) {
    let Some(assets) = assets else { return };
    last.retain(|e, _| rigs.contains(*e));
    for (rig, tf, playing) in &rigs {
        let Some(at) = spray_at(playing.clip) else {
            last.remove(&rig);
            continue;
        };
        let now = playing.timer.elapsed_secs();
        // A new clip (or the same one restarted) starts from before 0.
        let before = match last.insert(rig, (playing.clip, now)) {
            Some((clip, t)) if clip == playing.clip && t <= now => t,
            _ => -1.0,
        };
        if !crossed(at, before, now) {
            continue;
        }
        let ground = tf.translation.with_y(0.08);
        let seed0 = time.elapsed_secs() * 7.7 + tf.translation.x;
        for i in 0..budget.allow(SPRAY_MOTES) {
            let seed = seed0 + i as f32 * 2.399;
            commands.spawn((
                Particle::new(
                    Vec3::new(
                        noise(seed) * 2.4,
                        0.8 + hash01(seed * 1.9) * 1.4,
                        noise(seed * 2.3) * 2.4,
                    ),
                    0.6,
                    3.0,
                    1.8,
                ),
                DirtSpray,
                GameplayEntity,
                Mesh3d(assets.dust_mesh.clone()),
                MeshMaterial3d(assets.dust.clone()),
                Transform::from_translation(ground),
            ));
        }
    }
}

/// A mote of a slide's or dive's dirt spray, tagged for the e2e check.
#[derive(Component)]
pub struct DirtSpray;

/// A mote of the mitt's dust pop, tagged for the e2e check.
#[derive(Component)]
pub struct GlovePop;

/// A pitch at least this fast (flight m/s) pops dust off the mitt: 95% of
/// a fresh fastball's flight speed, so a fastball pops and the sinker,
/// slider, curve and changeup never do. A badly spent arm (about 87
/// pitches past its stamina, `velocity_scale` under 0.95) stops popping.
/// Flight speed runs under the broadcast mph (`rules::broadcast_mph`), so
/// this is not an mph figure.
pub(super) const GLOVE_POP_MPS: f32 = crate::game::rules::PITCH_SPEED * 0.95;
/// Motes in a glove pop, at High.
const GLOVE_POP_MOTES: usize = 8;

/// A hard fastball pops a puff of dust off the catcher's mitt.
#[allow(clippy::too_many_arguments)]
pub(super) fn glove_pop(
    mut pitches: MessageReader<PitchEvent>,
    mut caught: MessageReader<PitchCaughtEvent>,
    mut last_speed: Local<f32>,
    catcher: Query<&Transform, With<CatcherRole>>,
    assets: Option<Res<FxAssets>>,
    time: Res<Time>,
    mut budget: FxBudget,
    mut commands: Commands,
) {
    for pitch in pitches.read() {
        *last_speed = pitch.velocity.length();
    }
    let pops = caught.read().count();
    let Some(assets) = assets else { return };
    if pops == 0 || *last_speed < GLOVE_POP_MPS {
        return;
    }
    let Some(catcher) = catcher.iter().next() else {
        return;
    };
    // Where `flow::pitch::catcher_receives` parks the ball in the mitt.
    let mitt = catcher.translation + Vec3::new(0.0, 0.5, 0.45);
    let seed0 = time.elapsed_secs() * 5.3;
    for i in 0..budget.allow(GLOVE_POP_MOTES) {
        let seed = seed0 + i as f32 * 1.618;
        commands.spawn((
            Particle::new(
                Vec3::new(
                    noise(seed),
                    0.5 + hash01(seed * 1.3),
                    noise(seed * 1.7) + 0.6,
                ) * 1.4,
                0.35,
                0.5,
                1.2,
            ),
            GlovePop,
            GameplayEntity,
            Mesh3d(assets.spark_mesh.clone()),
            MeshMaterial3d(assets.dust.clone()),
            Transform::from_translation(mitt),
        ));
    }
}

/// How near a foul line (m, from the chalk's centre) a landing must be to
/// kick up chalk: the line is about 7.6 cm wide, and the ball's own radius
/// reaches across it.
const CHALK_REACH_M: f32 = 0.12;
/// Chalk motes in a puff, at High.
const CHALK_MOTES: usize = 10;

/// True when a first bounce at `pos` lands on a foul line's chalk (the
/// spans `field::foul_line_spans` paints), past the batter's boxes.
pub(super) fn on_the_chalk(pos: Vec3, field: &FieldSpec) -> bool {
    let p = Vec2::new(pos.x, pos.z);
    if p.length() < 1.0 {
        return false; // in the batter's boxes, not on a line
    }
    crate::game::field::foul_line_spans(field)
        .into_iter()
        .any(|(a, b)| {
            let (a, b) = (Vec2::new(a.x, a.z), Vec2::new(b.x, b.z));
            let ab = b - a;
            let t = ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
            p.distance(a + ab * t) <= CHALK_REACH_M
        })
}

/// A ball that lands on a foul line throws up a puff of white chalk.
pub(super) fn chalk_puff(
    mut live: MessageReader<LiveBallEvent>,
    field: Option<Res<FieldSpec>>,
    assets: Option<Res<FxAssets>>,
    time: Res<Time>,
    mut budget: FxBudget,
    mut commands: Commands,
) {
    let (Some(field), Some(assets)) = (field, assets) else {
        live.clear();
        return;
    };
    for ev in live.read() {
        let LiveBallEvent::Landed { pos } = *ev else {
            continue;
        };
        if !on_the_chalk(pos, &field) {
            continue;
        }
        let seed0 = time.elapsed_secs() * 3.9;
        for i in 0..budget.allow(CHALK_MOTES) {
            let seed = seed0 + i as f32 * 2.1;
            commands.spawn((
                Particle::new(
                    Vec3::new(
                        noise(seed) * 1.2,
                        0.9 + hash01(seed * 1.7),
                        noise(seed * 2.9) * 1.2,
                    ),
                    0.7,
                    0.6,
                    2.2,
                ),
                ChalkPuff,
                GameplayEntity,
                Mesh3d(assets.dust_mesh.clone()),
                MeshMaterial3d(assets.chalk.clone()),
                Transform::from_translation(pos.with_y(0.06)),
            ));
        }
    }
}

/// A chalk mote, tagged for the e2e check.
#[derive(Component)]
pub struct ChalkPuff;

/// Confetti flakes and streamers on a walk-off, at High.
const CONFETTI_FLAKES: usize = 140;
const CONFETTI_STREAMERS: usize = 24;
/// How long the confetti hangs in the air (s).
const CONFETTI_SECS: f32 = 4.5;

/// A confetti flake or streamer, tagged for the e2e check.
#[derive(Component)]
pub struct Confetti;

/// A walk-off fires confetti and streamers from both dugouts over the
/// plate, where the team mobs the winning run.
pub(super) fn walk_off_confetti(
    play: Res<Play>,
    mut seen: Local<bool>,
    assets: Option<Res<FxAssets>>,
    time: Res<Time>,
    mut budget: FxBudget,
    mut commands: Commands,
) {
    let now = play.is_walk_off();
    let fire = now && !*seen;
    *seen = now;
    let Some(assets) = assets else { return };
    if !fire || assets.confetti.is_empty() {
        return;
    }
    let s = time.elapsed_secs();
    let flakes = budget.allow(CONFETTI_FLAKES);
    let streamers = budget.allow(CONFETTI_STREAMERS);
    for i in 0..flakes + streamers {
        let seed = s * 3.3 + i as f32 * 1.618;
        let streamer = i >= flakes;
        // Two cannons, one each side of the plate, arcing in over it.
        let side = if i % 2 == 0 { -1.0 } else { 1.0 };
        let from = Vec3::new(side * 9.0, 1.0, 2.0);
        let vel = Vec3::new(
            -side * (3.0 + hash01(seed) * 3.5),
            9.0 + hash01(seed * 1.3) * 5.0,
            noise(seed * 1.7) * 3.0,
        );
        let material = assets.confetti
            [(hash01(seed * 2.9) * assets.confetti.len() as f32) as usize % assets.confetti.len()]
        .clone();
        let mesh = if streamer {
            assets.streamer_mesh.clone()
        } else {
            assets.flake_mesh.clone()
        };
        commands.spawn((
            Particle::new(vel, CONFETTI_SECS, 5.0, 0.0).with_drag(1.6),
            Flutter(Vec3::new(noise(seed * 3.1), noise(seed * 3.7), noise(seed * 4.3)) * 9.0),
            Confetti,
            GameplayEntity,
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(from),
        ));
    }
}

#[cfg(test)]
#[path = "moments.test.rs"]
mod tests;
