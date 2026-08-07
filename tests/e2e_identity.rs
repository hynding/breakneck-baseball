//! Identity plumbing e2e: rigs know who they are; runners wear jerseys.

mod common;

use bevy::prelude::*;
use breakneck_baseball::game::jersey::JerseyQuad;
use breakneck_baseball::game::player::{Batter, Pitcher};
use breakneck_baseball::game::roster::PlayerIdentity;
use breakneck_baseball::game::runner::Runner;
use breakneck_baseball::game::scenario::{apply_to_world, presets, PRESET_LOADED};
use breakneck_baseball::game::Team;
use common::{headless_app, run_until, start_game};

/// JerseyQuads start as rig-root children and re-parent onto bones once the
/// async glTF wiring lands — either way they stay descendants of the root.
fn count_quads(world: &mut World, root: Entity) -> usize {
    let mut count = 0;
    let mut stack = vec![root];
    while let Some(e) = stack.pop() {
        if world.get::<JerseyQuad>(e).is_some() {
            count += 1;
        }
        if let Some(children) = world.get::<Children>(e) {
            stack.extend(children.iter().copied());
        }
    }
    count
}

#[test]
fn seated_rigs_are_identified_at_kickoff() {
    let mut app = headless_app();
    start_game(&mut app, KeyCode::Digit1);
    // Top 1st: Away bats slot 1, Home pitches.
    let world = app.world_mut();
    let batter_id = *world
        .query_filtered::<&PlayerIdentity, With<Batter>>()
        .single(world);
    assert_eq!(
        batter_id,
        PlayerIdentity {
            team: Team::Away,
            index: 0
        }
    );
    let pitcher_id = *world
        .query_filtered::<&PlayerIdentity, With<Pitcher>>()
        .single(world);
    assert_eq!(
        pitcher_id,
        PlayerIdentity {
            team: Team::Home,
            index: 0
        }
    );
}

#[test]
fn runner_rigs_are_identified_and_wear_jerseys() {
    let mut app = headless_app();
    start_game(&mut app, KeyCode::Digit1);
    let s = presets()
        .into_iter()
        .find(|s| s.name == PRESET_LOADED)
        .unwrap();
    apply_to_world(app.world_mut(), &s).expect("ball is dead at PrePitch");
    let settled = run_until(&mut app, 5_000, |app| {
        let mut q = app.world_mut().query::<&Runner>();
        q.iter(app.world()).count() == 3
    });
    assert!(
        settled.is_some(),
        "three runner rigs must appear for bases loaded"
    );

    // Every runner knows who it is (scenario-manifested runners take the
    // batter-side fallback identity) and carries the four lettered quads.
    let world = app.world_mut();
    let runners: Vec<Entity> = world
        .query_filtered::<Entity, With<Runner>>()
        .iter(world)
        .collect();
    for rig in runners {
        let id = world
            .get::<PlayerIdentity>(rig)
            .expect("runner rig must carry PlayerIdentity");
        assert_eq!(id.team, Team::Away, "runners belong to the batting team");
        assert_eq!(count_quads(world, rig), 4, "runner must wear its jerseys");
    }
}

#[test]
fn skin_tones_dress_the_wired_rigs() {
    let mut app = headless_app();
    start_game(&mut app, KeyCode::Digit1);
    // Wait for glTF wiring + dressing (async asset load).
    let dressed = run_until(&mut app, 5_000, |app| {
        let world = app.world_mut();
        let mut q = world.query::<&breakneck_baseball::game::gear::DressedAs>();
        q.iter(world).count() > 0
    });
    assert!(
        dressed.is_some(),
        "at least one rig must dress after wiring"
    );
    // A dressed rig's skin meshes must not wear the shared base material.
    let world = app.world_mut();
    let base = world
        .resource::<breakneck_baseball::game::model_assets::RigAnimations>()
        .skin_material
        .clone();
    let mut rigs = world.query_filtered::<
        &breakneck_baseball::game::model_assets::RigSkinMeshes,
        With<breakneck_baseball::game::gear::DressedAs>,
    >();
    let skin_meshes: Vec<Entity> = rigs.iter(world).flat_map(|m| m.0.clone()).collect();
    assert!(!skin_meshes.is_empty());
    for mesh in skin_meshes {
        let mat = world
            .get::<MeshMaterial3d<StandardMaterial>>(mesh)
            .expect("skin mesh keeps its material component");
        assert_ne!(
            mat.0, base,
            "dressed skin must be a swatch clone, not the base"
        );
    }
}

#[test]
fn batter_holds_his_personal_stance() {
    use breakneck_baseball::game::animation::Playing;
    let mut app = headless_app();
    start_game(&mut app, KeyCode::Digit1);
    // Away leadoff (STONE) has stance UprightClosed in data/players.ron →
    // his duel hold must be StanceClosed, not the shared BattingStance.
    let held = run_until(&mut app, 5_000, |app| {
        let world = app.world_mut();
        world
            .query_filtered::<&Playing, With<Batter>>()
            .iter(world)
            .next()
            .map(|p| p.clip == breakneck_baseball::game::animation::AnimClip::StanceClosed)
            .unwrap_or(false)
    });
    assert!(held.is_some(), "batter must hold his personal stance clip");
}

#[test]
fn fidgets_fire_between_pitches_when_enabled() {
    use breakneck_baseball::game::animation::{AnimClip, FidgetsDisabled, Playing};
    let mut app = headless_app();
    app.world_mut().remove_resource::<FidgetsDisabled>(); // harness default off
    start_game(&mut app, KeyCode::Digit1);
    // STONE (away leadoff) has NO authored fidget — use the scenario seam to
    // put slot 2 (IBARRA, fidget: Some(BatTap)) at the plate instantly
    // instead of simulating an at-bat (batter_slot is 1-based; slot 2 ==
    // away lineup index 1 == IBARRA, per data/players.ron).
    breakneck_baseball::game::scenario::apply_to_world(
        app.world_mut(),
        &breakneck_baseball::game::scenario::Scenario {
            batter_slot: Some(2),
            ..Default::default()
        },
    )
    .expect("ball is dead at PrePitch");
    let fidgeted = run_until(&mut app, 240 * 12, |app| {
        let world = app.world_mut();
        world
            .query_filtered::<&Playing, With<Batter>>()
            .iter(world)
            .next()
            .map(|p| matches!(p.clip, AnimClip::FidgetBatTap | AnimClip::FidgetHalfSwing))
            .unwrap_or(false)
    });
    assert!(
        fidgeted.is_some(),
        "an authored fidget must fire within ~12 s of PrePitch"
    );
}

/// Top 1st: Away bats (CPU by default), Home pitches — a human key press, so
/// this test scripts it directly (the `e2e_cpu.rs` `drive` pattern) rather
/// than waiting on an idle keyboard.
fn drive_pitch_in_pre_pitch(
    play: Option<Res<breakneck_baseball::game::flow::Play>>,
    mut intents: ResMut<breakneck_baseball::game::input::Intents>,
) {
    let Some(play) = play else { return };
    intents.home = default();
    if play.phase == breakneck_baseball::game::flow::Phase::PrePitch {
        intents.home.action = true;
    }
}

#[test]
fn fidget_is_cut_before_the_windup() {
    use breakneck_baseball::game::animation::{is_fidget, AnimClip, Playing};
    use breakneck_baseball::game::flow::{Phase, Play};
    let mut app = headless_app();
    app.add_systems(common::DriveGame, drive_pitch_in_pre_pitch);
    start_game(&mut app, KeyCode::Digit1);
    // Force a fidget onto the batter directly — bypassing `batter_fidgets`'
    // own hash-noise cadence — so `batter_stance`'s continuation-cut arm is
    // exercised deterministically instead of waiting on the interval draw.
    let batter = app
        .world_mut()
        .query_filtered::<Entity, With<Batter>>()
        .single(app.world());
    app.world_mut()
        .entity_mut(batter)
        .insert(Playing::new(AnimClip::FidgetHalfSwing));
    // The scripted pitcher advances PrePitch -> WindUp; the fidget must
    // already be gone the instant it does (spec §4: fidgets exist only
    // inside PrePitch), which also means `trigger_swing`'s stance-only gate
    // never sees one and blocks a real swing press.
    let past_pre_pitch = run_until(&mut app, 5_000, |app| {
        app.world().resource::<Play>().phase != Phase::PrePitch
    });
    assert!(
        past_pre_pitch.is_some(),
        "the pitcher must eventually pitch"
    );
    let world = app.world_mut();
    let survived_fidget = world
        .get::<Playing>(batter)
        .is_some_and(|p| is_fidget(p.clip));
    assert!(
        !survived_fidget,
        "a fidget must never survive past PrePitch into the windup"
    );
}

#[test]
fn home_run_queues_the_authored_celebration() {
    use breakneck_baseball::game::animation::{AnimClip, Playing};
    use breakneck_baseball::game::flow::BallInPlayEvent;
    use breakneck_baseball::game::rules::{ContactClass, ContactKind};
    let mut app = headless_app();
    start_game(&mut app, KeyCode::Digit1);
    // Stamp FOX (away idx2 — authored celebration BatFlip per
    // data/players.ron; OKAFOR home/1, KANE home/8, MARSH away/10 also
    // qualify) directly onto the batter rig: a synthetic identity swap,
    // acceptable per the task brief instead of driving a batting-order
    // advance to put him up for real.
    let batter = app
        .world_mut()
        .query_filtered::<Entity, With<Batter>>()
        .single(app.world());
    app.world_mut().entity_mut(batter).insert(PlayerIdentity {
        team: Team::Away,
        index: 2,
    });
    // Swing in flight when the ball is declared a homer: the celebration
    // must chain via `Playing.next`, never cutting the swing.
    app.world_mut()
        .entity_mut(batter)
        .insert(Playing::new(AnimClip::BatterSwing));
    app.world_mut().send_event(BallInPlayEvent {
        kind: ContactKind::HomeRun,
        landing: Vec3::new(0.0, 0.0, 120.0),
        contact_class: ContactClass::DeepFly,
    });
    for _ in 0..4 {
        app.update();
    }
    let world = app.world_mut();
    let playing = world.get::<Playing>(batter).expect("swing still in flight");
    assert_eq!(playing.clip, AnimClip::BatterSwing, "swing must not be cut");
    assert_eq!(
        playing.next,
        Some(AnimClip::CelebrateBatFlip),
        "flip chains after"
    );
}

#[test]
fn headwear_hides_the_baked_cap_and_mounts_gear() {
    let mut app = headless_app();
    start_game(&mut app, KeyCode::Digit1);
    // VEGA (home slot 0 → the pitcher in the top 1st) wears a Helmet in
    // data/players.ron: his baked cap must hide and a helmet prop appear.
    // Gate on the PITCHER RIG specifically being dressed — rigs wire
    // asynchronously per-entity, so "any gear exists" would race.
    let done = run_until(&mut app, 5_000, |app| {
        let world = app.world_mut();
        let mut q = world.query_filtered::<
            &breakneck_baseball::game::gear::RigGear,
            With<breakneck_baseball::game::player::Pitcher>,
        >();
        q.iter(world)
            .next()
            .map(|g| !g.0.is_empty())
            .unwrap_or(false)
    });
    assert!(
        done.is_some(),
        "the helmeted pitcher must dress with gear props"
    );

    let world = app.world_mut();
    // Find the pitcher rig (identity Home/0 = VEGA per data/players.ron).
    let mut pitchers = world.query_filtered::<(
        &breakneck_baseball::game::model_assets::RigCapMeshes,
        &breakneck_baseball::game::gear::RigGear,
    ), With<breakneck_baseball::game::player::Pitcher>>();
    let (caps, gear) = pitchers.single(world);
    let cap_entities = caps.0.clone();
    let gear_entities = gear.0.clone();
    assert!(
        !gear_entities.is_empty(),
        "helmet wearer must own gear props"
    );
    for cap in cap_entities {
        assert_eq!(
            world.get::<Visibility>(cap).copied(),
            Some(Visibility::Hidden),
            "baked cap must hide under a helmet"
        );
    }
    // Spec §7: props are parented to the right bone entities — the helmet
    // must be a child of the pitcher rig's Head bone.
    let mut pitcher_bones = world.query_filtered::<
        &breakneck_baseball::game::model_assets::RigBones,
        With<breakneck_baseball::game::player::Pitcher>,
    >();
    let head = pitcher_bones.single(world).head;
    let on_head = gear_entities
        .iter()
        .any(|&p| world.get::<Parent>(p).map(|par| par.get()) == Some(head));
    assert!(on_head, "the helmet prop must hang off the Head bone");
}
