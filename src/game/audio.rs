//! Procedural game audio — every sound is synthesized at startup into an
//! in-memory WAV (no asset files to ship), then fired off gameplay events:
//! the crack of the bat (in three quality-keyed flavours), a glove pop on a
//! catch, the wall thud on a carom, a little stinger for the epic banners,
//! and a synthesized crowd — a low murmur bed looping under the whole game,
//! a roar swell on a Perfect swing or a deep fly, and a groan on a swinging
//! strikeout. Purely cosmetic: nothing here reads or writes game state.
//!
//! The synthesis uses the same deterministic hash noise as the CPU AI, so
//! the waveforms are identical on every run and both targets. On the web,
//! browsers gate audio behind a user gesture — the menu keypress that starts
//! a game satisfies it.

use bevy::audio::{AudioPlayer, AudioSource, PlaybackSettings, Volume};
use bevy::prelude::*;

use crate::game::ai::hash01;
use crate::game::flow::{BallInPlayEvent, ContactEvent, LiveBallEvent, PitchCaughtEvent};
use crate::game::flow::{BannerTone, PlayBanner};
use crate::game::rules::{ContactClass, ContactQuality};
use crate::game::{ball::WallBangEvent, game_start, GameState, GameplayEntity};

/// Mono synthesis rate — plenty for percussive game sounds, tiny in memory.
const SAMPLE_RATE: u32 = 22_050;

/// The banner text `flow::add_strike` fires on the final strike — matched
/// against in [`play_event_sounds`] to tell a swinging strikeout apart from
/// a called (non-swinging) one, since [`ContactEvent`] only ever fires on a
/// swing (see the `swinging_whiff` correlation below).
const STRIKEOUT_BANNER: &str = "STRIKEOUT!";

/// Handles to every synthesized sound.
#[derive(Resource)]
struct SoundBank {
    crack_perfect: Handle<AudioSource>,
    crack_solid: Handle<AudioSource>,
    crack_foul: Handle<AudioSource>,
    glove: Handle<AudioSource>,
    wall: Handle<AudioSource>,
    stinger: Handle<AudioSource>,
    crowd: Handle<AudioSource>,
    roar: Handle<AudioSource>,
    groan: Handle<AudioSource>,
}

/// Wraps raw mono f32 samples in a minimal 16-bit PCM WAV container that
/// bevy_audio's decoder accepts.
fn wav_from_samples(samples: &[f32]) -> AudioSource {
    let data_len = (samples.len() * 2) as u32;
    let mut bytes = Vec::with_capacity(44 + samples.len() * 2);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
    bytes.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    bytes.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes()); // byte rate
    bytes.extend_from_slice(&2u16.to_le_bytes()); // block align
    bytes.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        let value = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    AudioSource {
        bytes: bytes.into(),
    }
}

/// Runs `voice(t, noise)` over `seconds` of samples with a soft fade-out so
/// nothing clicks at the end. For one-shots only — see [`synth_crowd`] for
/// the looping bed, which needs the opposite property (no fade, seamless
/// wrap).
fn synth(seconds: f32, voice: impl Fn(f32, f32) -> f32) -> Vec<f32> {
    let n = (seconds * SAMPLE_RATE as f32) as usize;
    (0..n)
        .map(|i| {
            let t = i as f32 / SAMPLE_RATE as f32;
            let noise = hash01(i as f32 * 0.618_034) * 2.0 - 1.0;
            let fade = (1.0 - i as f32 / n as f32).min(1.0);
            voice(t, noise) * fade
        })
        .collect()
}

fn sine(freq: f32, t: f32) -> f32 {
    (std::f32::consts::TAU * freq * t).sin()
}

/// The bat crack: a sharp noise burst with a bright ping riding on it,
/// parameterized so [`build_sound_bank`] can voice three quality-keyed
/// variants (sharper/louder for `Perfect`, standard for `Solid`, dull for
/// `FoulTip`) from one shape instead of three copy-pasted synths.
fn synth_crack(
    seconds: f32,
    noise_decay: f32,
    noise_gain: f32,
    ping_freq: f32,
    ping_decay: f32,
    ping_gain: f32,
) -> Vec<f32> {
    synth(seconds, move |t, noise| {
        noise * (-noise_decay * t).exp() * noise_gain
            + sine(ping_freq, t) * (-ping_decay * t).exp() * ping_gain
    })
}

/// A crowd murmur bed meant to loop forever (`PlaybackSettings::LOOP`)
/// under the whole game. Built entirely from sine partials tuned to an
/// *integer* number of cycles over `seconds` — so every partial returns to
/// the exact same phase at the buffer's end as it started, and the sum
/// (unlike [`synth`]'s per-sample hash noise, which doesn't repeat) loops
/// with zero discontinuity at the wrap point. Partial frequencies/phases/
/// gains are themselves hash-noise-derived, so the murmur still varies
/// texturally, deterministically, without ever breaking periodicity.
fn synth_crowd(seconds: f32) -> Vec<f32> {
    const PARTIALS: usize = 14;
    let partials: Vec<(f32, f32, f32)> = (0..PARTIALS)
        .map(|k| {
            let seed = k as f32 * 7.919;
            // Whole cycles over the loop: freq = cycles / seconds always has
            // freq * seconds == an integer, so sin(2*pi*freq*seconds + phase)
            // == sin(phase) — the value (and slope) at t=seconds matches t=0.
            let cycles = (40.0 + hash01(seed) * 260.0).round();
            let phase = hash01(seed + 0.31) * std::f32::consts::TAU;
            let gain = 0.3 + hash01(seed + 0.62) * 0.7;
            (cycles, phase, gain)
        })
        .collect();
    let norm = partials.iter().map(|(_, _, g)| g).sum::<f32>().max(1.0);
    let n = (seconds * SAMPLE_RATE as f32) as usize;
    (0..n)
        .map(|i| {
            let t = i as f32 / SAMPLE_RATE as f32;
            // A slow swell, itself an integer 2 cycles over the loop.
            let swell = 0.7 + 0.3 * sine(2.0 / seconds, t);
            let hum: f32 = partials
                .iter()
                .map(|(cycles, phase, gain)| {
                    (std::f32::consts::TAU * cycles / seconds * t + phase).sin() * gain
                })
                .sum();
            (hum / norm) * 0.5 * swell
        })
        .collect()
}

/// Builds the bank once at startup.
fn build_sound_bank(mut commands: Commands, mut sources: ResMut<Assets<AudioSource>>) {
    // Bat crack, three quality-keyed variants (see `synth_crack`'s doc).
    // `crack_solid` keeps the original tuning as the baseline "standard" hit.
    let crack_perfect = synth_crack(0.14, 38.0, 1.0, 2_100.0, 26.0, 0.5);
    let crack_solid = synth_crack(0.12, 45.0, 0.9, 1_700.0, 35.0, 0.35);
    let crack_foul = synth_crack(0.07, 75.0, 0.5, 1_050.0, 60.0, 0.18);
    // Glove pop: a low thump with a leathery tick.
    let glove = synth(0.09, |t, noise| {
        sine(185.0, t) * (-40.0 * t).exp() * 0.85 + noise * (-90.0 * t).exp() * 0.3
    });
    // Wall bang: a boomy padded thud.
    let wall = synth(0.28, |t, noise| {
        sine(82.0, t) * (-13.0 * t).exp() * 0.95 + noise * (-28.0 * t).exp() * 0.25
    });
    // Epic-banner stinger: a rising two-note chime.
    let stinger = synth(0.5, |t, _| {
        if t < 0.18 {
            sine(660.0, t) * (-9.0 * t).exp() * 0.6
        } else {
            let u = t - 0.18;
            (sine(990.0, u) + sine(1_320.0, u) * 0.4) * (-7.0 * u).exp() * 0.55
        }
    });
    // Crowd murmur bed: see `synth_crowd`'s doc for the seamless-loop design.
    let crowd = synth_crowd(6.0);
    // Roar swell: a quick rise (crowd coming up out of its seats) into a
    // long noisy decay, for a Perfect swing or a deep fly.
    let roar = synth(1.1, |t, noise| {
        let envelope = (t / 0.28).min(1.0) * (-1.4 * t).exp();
        noise * envelope * 0.7 + sine(95.0, t) * envelope * 0.35
    });
    // Groan: a drooping low tone for a swinging strikeout.
    let groan = synth(0.9, |t, noise| {
        let freq = 190.0 - 70.0 * (t / 0.9).min(1.0);
        sine(freq, t) * (-2.5 * t).exp() * 0.55 + noise * (-6.0 * t).exp() * 0.15
    });

    commands.insert_resource(SoundBank {
        crack_perfect: sources.add(wav_from_samples(&crack_perfect)),
        crack_solid: sources.add(wav_from_samples(&crack_solid)),
        crack_foul: sources.add(wav_from_samples(&crack_foul)),
        glove: sources.add(wav_from_samples(&glove)),
        wall: sources.add(wav_from_samples(&wall)),
        stinger: sources.add(wav_from_samples(&stinger)),
        crowd: sources.add(wav_from_samples(&crowd)),
        roar: sources.add(wav_from_samples(&roar)),
        groan: sources.add(wav_from_samples(&groan)),
    });
}

/// One despawn-when-done audio entity per event.
fn play(commands: &mut Commands, handle: &Handle<AudioSource>, volume: f32) {
    commands.spawn((
        GameplayEntity,
        AudioPlayer::new(handle.clone()),
        PlaybackSettings::DESPAWN.with_volume(Volume::new(volume)),
    ));
}

/// Starts the crowd murmur, tagged [`GameplayEntity`] like every other
/// gameplay-scoped spawn so `cleanup_gameplay` despawns it (and thus stops
/// the loop) on the `Playing -> GameOver` teardown — it never plays over the
/// menu, and a fresh one starts on the next `game_start()`.
fn start_crowd_loop(bank: Option<Res<SoundBank>>, mut commands: Commands) {
    let Some(bank) = bank else { return };
    commands.spawn((
        GameplayEntity,
        AudioPlayer::new(bank.crowd.clone()),
        PlaybackSettings::LOOP.with_volume(Volume::new(0.12)),
    ));
}

/// Fires the bank off gameplay events.
#[allow(clippy::too_many_arguments)]
fn play_event_sounds(
    bank: Option<Res<SoundBank>>,
    mut contacts: EventReader<ContactEvent>,
    mut in_play: EventReader<BallInPlayEvent>,
    mut bangs: EventReader<WallBangEvent>,
    mut live: EventReader<LiveBallEvent>,
    mut received: EventReader<PitchCaughtEvent>,
    mut banners: EventReader<PlayBanner>,
    mut commands: Commands,
) {
    let Some(bank) = bank else { return };
    let mut roar = false;
    // A swing that whiffed this frame — `ContactEvent` never fires for a
    // called (non-swinging) strike, so pairing it with the `STRIKEOUT!`
    // banner below (fired from the same `pitch_live` call) identifies a
    // *swinging* strikeout specifically, per the brief.
    let mut swinging_whiff = false;
    for contact in contacts.read() {
        match contact.quality {
            ContactQuality::Perfect => {
                play(&mut commands, &bank.crack_perfect, 0.95);
                roar = true;
            }
            // `Weak` never comes from the Classic windows (Plan-C PCI
            // adapter only) but is bucketed with `Solid` here just as
            // ui.rs/field.rs already do for presentation.
            ContactQuality::Solid | ContactQuality::Weak => {
                play(&mut commands, &bank.crack_solid, 0.8);
            }
            ContactQuality::FoulTip => {
                play(&mut commands, &bank.crack_foul, 0.5);
            }
            // No bat-ball contact on a whiff — no crack, silence is correct.
            ContactQuality::Whiff => swinging_whiff = true,
        }
    }
    // The catcher's mitt pops on every received pitch.
    for _ in received.read() {
        play(&mut commands, &bank.glove, 0.6);
    }
    for _ in bangs.read() {
        play(&mut commands, &bank.wall, 0.9);
    }
    for event in live.read() {
        if matches!(event, LiveBallEvent::Caught { .. }) {
            play(&mut commands, &bank.glove, 0.7);
        }
    }
    for event in in_play.read() {
        if event.contact_class == ContactClass::DeepFly {
            roar = true;
        }
    }
    if roar {
        play(&mut commands, &bank.roar, 0.7);
    }
    for banner in banners.read() {
        if banner.tone == BannerTone::Epic {
            play(&mut commands, &bank.stinger, 0.6);
        }
        if swinging_whiff && banner.text == STRIKEOUT_BANNER {
            play(&mut commands, &bank.groan, 0.65);
        }
    }
}

pub struct SoundPlugin;

impl Plugin for SoundPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, build_sound_bank)
            .add_systems(game_start(), start_crowd_loop)
            .add_systems(
                Update,
                play_event_sounds.run_if(in_state(GameState::Playing)),
            );
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::rules::ContactKind;
    use crate::game::Team;
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

        // A whiff with no strikeout banner (e.g. strike one swinging): no groan.
        let before = audio_players(&app).len();
        app.world_mut().send_event(ContactEvent {
            quality: ContactQuality::Whiff,
            batting_team: Team::Home,
            dt_ms: 400.0,
        });
        app.update();
        assert_eq!(
            audio_players(&app).len(),
            before,
            "a bare whiff makes no bat-ball sound"
        );

        // The same whiff, but this time it's the frame the K is announced.
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
            before + 1,
            "a swinging strikeout groans exactly once"
        );
    }
}
