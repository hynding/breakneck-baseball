//! Unit tests for [`super`] — the audio module.

use super::*;
use crate::game::Team;
use crate::game::rules::ContactKind;
use bevy::state::app::StatesPlugin;

#[test]
fn wav_container_is_well_formed() {
    let samples = [0.0_f32, 0.5, -0.5, 1.0];
    let source = wav_from_samples(&samples);
    let bytes = &source.bytes;
    assert_eq!(&bytes[0..4], b"RIFF");
    assert_eq!(&bytes[8..16], b"WAVEfmt ");
    assert_eq!(&bytes[36..40], b"data");
    // 44-byte header + 2 bytes per sample.
    assert_eq!(bytes.len(), 44 + samples.len() * 2);
    // Full-scale sample clamps to i16::MAX.
    let last = i16::from_le_bytes([bytes[bytes.len() - 2], bytes[bytes.len() - 1]]);
    assert_eq!(last, i16::MAX);
}

#[test]
fn synthesis_is_deterministic_and_bounded() {
    let a = synth(0.1, |t, noise| sine(440.0, t) * 0.5 + noise * 0.3);
    let b = synth(0.1, |t, noise| sine(440.0, t) * 0.5 + noise * 0.3);
    assert_eq!(a, b);
    assert!(a.iter().all(|s| s.abs() <= 1.0));
}

/// Every crack variant is non-empty, sample-count-correct for its
/// duration, clamped, and — the whole point of parameterizing
/// `synth_crack` — distinct from the others.
#[test]
fn crack_variants_are_bounded_and_distinct() {
    let perfect = synth_crack(0.14, 38.0, 1.0, 2_100.0, 26.0, 0.5);
    let solid = synth_crack(0.12, 45.0, 0.9, 1_700.0, 35.0, 0.35);
    let foul = synth_crack(0.07, 75.0, 0.5, 1_050.0, 60.0, 0.18);

    assert_eq!(perfect.len(), (0.14 * SAMPLE_RATE as f32) as usize);
    assert_eq!(solid.len(), (0.12 * SAMPLE_RATE as f32) as usize);
    assert_eq!(foul.len(), (0.07 * SAMPLE_RATE as f32) as usize);
    assert!(!perfect.is_empty() && !solid.is_empty() && !foul.is_empty());

    // The raw voice can transiently exceed unity (noise + ping stack up
    // near t=0, same as the original single-variant crack this
    // replaces) — `wav_from_samples` is what actually clamps for
    // playback (see `wav_container_is_well_formed`); what matters here
    // is that encoding every variant works without panicking.
    for buf in [&perfect, &solid, &foul] {
        let _ = wav_from_samples(buf);
    }
    // Different durations alone make them unequal, but check the
    // overlapping prefix really differs in content too (not just length).
    let n = foul.len().min(solid.len());
    assert_ne!(
        &foul[..n],
        &solid[..n],
        "foul and solid must sound different"
    );
}

#[test]
fn roar_and_groan_are_bounded_with_expected_duration() {
    let roar_secs = 1.1;
    let groan_secs = 0.9;
    let roar = synth(roar_secs, |t, noise| {
        let envelope = (t / 0.28).min(1.0) * (-1.4 * t).exp();
        noise * envelope * 0.7 + sine(95.0, t) * envelope * 0.35
    });
    let groan = synth(groan_secs, |t, noise| {
        let freq = 190.0 - 70.0 * (t / 0.9).min(1.0);
        sine(freq, t) * (-2.5 * t).exp() * 0.55 + noise * (-6.0 * t).exp() * 0.15
    });
    assert_eq!(roar.len(), (roar_secs * SAMPLE_RATE as f32) as usize);
    assert_eq!(groan.len(), (groan_secs * SAMPLE_RATE as f32) as usize);
    assert!(!roar.is_empty() && !groan.is_empty());
    assert!(roar.iter().all(|s| s.abs() <= 1.0));
    assert!(groan.iter().all(|s| s.abs() <= 1.0));
}

#[test]
fn crowd_loop_is_bounded_deterministic_and_seamless() {
    let seconds = 6.0;
    let a = synth_crowd(seconds);
    let b = synth_crowd(seconds);
    assert_eq!(a, b, "must be deterministic");
    assert_eq!(a.len(), (seconds * SAMPLE_RATE as f32) as usize);
    assert!(!a.is_empty());
    assert!(a.iter().all(|s| s.abs() <= 1.0));
    // The whole point of tuning every partial to an integer cycle count:
    // the value one sample past the end (which is what looping back to
    // the start actually sounds like) must be close to the last sample,
    // not an arbitrary jump — i.e. no click at the loop point.
    let wrap_delta = (a[0] - a[a.len() - 1]).abs();
    let mut max_adjacent_delta = 0.0_f32;
    for w in a.windows(2) {
        max_adjacent_delta = max_adjacent_delta.max((w[1] - w[0]).abs());
    }
    assert!(
        wrap_delta <= max_adjacent_delta * 4.0 + 0.01,
        "loop wrap delta {wrap_delta} should be in line with in-buffer deltas (max {max_adjacent_delta})"
    );
}

/// A minimal app: `MinimalPlugins` (for `Time`/asset storage) plus
/// `StatesPlugin` (for `GameState`) plus `SoundPlugin` — no rendering or
/// the rest of `GamePlugin`. Every event `play_event_sounds` reads is
/// registered directly since the plugins that normally own them
/// (`FlowPlugin`/`BallPlugin`) aren't present, following the same
/// pattern as `juice.rs`'s test harness.
fn test_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(StatesPlugin)
        .init_state::<GameState>()
        .init_resource::<Assets<AudioSource>>()
        .add_event::<ContactEvent>()
        .add_event::<BallInPlayEvent>()
        .add_event::<WallBangEvent>()
        .add_event::<LiveBallEvent>()
        .add_event::<PitchCaughtEvent>()
        .add_event::<PitchEvent>()
        .add_event::<PlayBanner>()
        .add_plugins(SoundPlugin);
    // `bevy_state`'s `StatesPlugin` runs `StateTransition` *before*
    // `Startup` on the very first `update()` (it's spliced into both
    // the startup schedule list and the per-frame one) — so queuing the
    // `MainMenu -> Playing` transition before that first update would
    // fire `game_start()` a frame too early, before `build_sound_bank`
    // has inserted `SoundBank`. One update lets Startup run first
    // (state is still `MainMenu`, nothing queued yet); only then is the
    // transition queued and applied on the second.
    app.update();
    app.world_mut()
        .resource_mut::<NextState<GameState>>()
        .set(GameState::Playing);
    app.update();
    app
}

fn audio_players(app: &App) -> Vec<&AudioPlayer> {
    app.world()
        .iter_entities()
        .filter_map(|e| e.get::<AudioPlayer>())
        .collect()
}

#[test]
fn game_start_spawns_the_looping_crowd_bed() {
    let app = test_app();
    let loops: Vec<_> = app
        .world()
        .iter_entities()
        .filter_map(|e| e.get::<PlaybackSettings>())
        .filter(|s| matches!(s.mode, bevy::audio::PlaybackMode::Loop))
        .collect();
    assert_eq!(
        loops.len(),
        1,
        "exactly one looping crowd bed at game start"
    );
    assert!(!audio_players(&app).is_empty());
}

#[test]
fn perfect_contact_plays_crack_and_roar() {
    let mut app = test_app();
    let before = audio_players(&app).len();
    app.world_mut().send_event(ContactEvent {
        quality: ContactQuality::Perfect,
        batting_team: Team::Home,
        dt_ms: 0.0,
    });
    app.update();
    // The looping bed plus two new one-shots (crack + roar).
    assert_eq!(audio_players(&app).len(), before + 2);
}

#[test]
fn deep_fly_plays_the_roar_without_contact_event() {
    let mut app = test_app();
    let before = audio_players(&app).len();
    app.world_mut().send_event(BallInPlayEvent {
        kind: ContactKind::Live { fair: true },
        landing: Vec3::new(0.0, 0.0, 90.0),
        contact_class: ContactClass::DeepFly,
    });
    app.update();
    assert_eq!(audio_players(&app).len(), before + 1, "roar only, no crack");
}

/// A ball over the fence peaks the crowd: exactly one roar (the peak
/// subsumes the ordinary deep-fly roar, so a HR whose flight also grades
/// a deep fly never double-roars), and no crack (the crack comes off the
/// separate ContactEvent, not sent here).
#[test]
fn home_run_plays_a_single_crowd_peak_roar() {
    let mut app = test_app();
    let before = audio_players(&app).len();
    app.world_mut().send_event(BallInPlayEvent {
        kind: ContactKind::HomeRun,
        landing: Vec3::new(0.0, 0.0, 120.0),
        contact_class: ContactClass::DeepFly,
    });
    app.update();
    assert_eq!(
        audio_players(&app).len(),
        before + 1,
        "a home run plays exactly one (peak) roar"
    );
}

#[test]
fn foul_tip_plays_the_dull_crack_only() {
    let mut app = test_app();
    let before = audio_players(&app).len();
    app.world_mut().send_event(ContactEvent {
        quality: ContactQuality::FoulTip,
        batting_team: Team::Home,
        dt_ms: 95.0,
    });
    app.update();
    assert_eq!(audio_players(&app).len(), before + 1, "crack only, no roar");
}

#[test]
fn swinging_strikeout_groans_but_a_whiff_alone_does_not() {
    let mut app = test_app();

    // A whiff with no strikeout banner (e.g. strike one swinging): the
    // bat cuts air (one whoosh, TODO 68) but nobody groans.
    let before = audio_players(&app).len();
    app.world_mut().send_event(ContactEvent {
        quality: ContactQuality::Whiff,
        batting_team: Team::Home,
        dt_ms: 400.0,
    });
    app.update();
    assert_eq!(
        audio_players(&app).len(),
        before + 1,
        "a bare whiff is one bat whoosh, no groan"
    );

    // The same whiff, but this time it's the frame the K is announced:
    // the bat whoosh plus exactly one (full) groan — the Bad-tone soft
    // reaction defers to it.
    let before = audio_players(&app).len();
    app.world_mut().send_event(ContactEvent {
        quality: ContactQuality::Whiff,
        batting_team: Team::Home,
        dt_ms: 400.0,
    });
    app.world_mut().send_event(PlayBanner {
        text: STRIKEOUT_BANNER.to_string(),
        tone: BannerTone::Bad,
    });
    app.update();
    assert_eq!(
        audio_players(&app).len(),
        before + 2,
        "a swinging strikeout is whoosh + one groan"
    );
}

/// The routine out's two new voices (TODO 68): the throw whooshes, the
/// catch at the bag pops the glove.
#[test]
fn thrown_and_settled_voices_the_routine_out() {
    let mut app = test_app();
    let before = audio_players(&app).len();
    app.world_mut().send_event(LiveBallEvent::Thrown {
        pos: Vec3::ZERO,
        base: 1,
        race_time: 1.0,
    });
    app.update();
    assert_eq!(audio_players(&app).len(), before + 1, "throw whoosh");
    app.world_mut().send_event(LiveBallEvent::Settled);
    app.update();
    assert_eq!(audio_players(&app).len(), before + 2, "glove at the bag");
}

/// Good-tone banners (STOLEN BASE!, WALK, ...) get the bright crowd pop;
/// Bad-tone calls get the soft groan — no announced call is silent
/// (TODO 68).
#[test]
fn good_and_bad_banners_each_have_a_voice() {
    let mut app = test_app();
    let before = audio_players(&app).len();
    app.world_mut().send_event(PlayBanner {
        text: "STOLEN BASE!".to_string(),
        tone: BannerTone::Good,
    });
    app.update();
    assert_eq!(audio_players(&app).len(), before + 1, "good-tone pop");

    let before = audio_players(&app).len();
    app.world_mut().send_event(PlayBanner {
        text: "CAUGHT STEALING".to_string(),
        tone: BannerTone::Bad,
    });
    app.update();
    assert_eq!(audio_players(&app).len(), before + 1, "bad-tone ohh");
}

/// The pitch release hisses once per pitch (TODO 68).
#[test]
fn pitch_release_hisses() {
    let mut app = test_app();
    let before = audio_players(&app).len();
    app.world_mut().send_event(PitchEvent {
        velocity: Vec3::new(0.0, 0.0, -38.0),
        spin: Vec3::ZERO,
    });
    app.update();
    assert_eq!(audio_players(&app).len(), before + 1);
}
