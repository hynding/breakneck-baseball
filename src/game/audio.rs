//! Procedural game audio — every sound is synthesized at startup into an
//! in-memory WAV (no asset files to ship), then fired off gameplay events:
//! the crack of the bat, a glove pop on a catch, the wall thud on a carom,
//! and a little stinger for the epic banners. Purely cosmetic: nothing here
//! reads or writes game state.
//!
//! The synthesis uses the same deterministic hash noise as the CPU AI, so
//! the waveforms are identical on every run and both targets. On the web,
//! browsers gate audio behind a user gesture — the menu keypress that starts
//! a game satisfies it.

use bevy::audio::{AudioPlayer, AudioSource, PlaybackSettings, Volume};
use bevy::prelude::*;

use crate::game::ai::hash01;
use crate::game::ball::{HitEvent, WallBangEvent};
use crate::game::flow::{BannerTone, LiveBallEvent, PlayBanner};
use crate::game::{GameState, GameplayEntity};

/// Mono synthesis rate — plenty for percussive game sounds, tiny in memory.
const SAMPLE_RATE: u32 = 22_050;

/// Handles to every synthesized sound.
#[derive(Resource)]
struct SoundBank {
    crack: Handle<AudioSource>,
    glove: Handle<AudioSource>,
    wall: Handle<AudioSource>,
    stinger: Handle<AudioSource>,
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
/// nothing clicks at the end.
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

/// Builds the bank once at startup.
fn build_sound_bank(mut commands: Commands, mut sources: ResMut<Assets<AudioSource>>) {
    // Bat crack: a sharp noise burst with a bright ping riding on it.
    let crack = synth(0.12, |t, noise| {
        noise * (-45.0 * t).exp() * 0.9 + sine(1_700.0, t) * (-35.0 * t).exp() * 0.35
    });
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

    commands.insert_resource(SoundBank {
        crack: sources.add(wav_from_samples(&crack)),
        glove: sources.add(wav_from_samples(&glove)),
        wall: sources.add(wav_from_samples(&wall)),
        stinger: sources.add(wav_from_samples(&stinger)),
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

/// Fires the bank off gameplay events.
fn play_event_sounds(
    bank: Option<Res<SoundBank>>,
    mut hits: EventReader<HitEvent>,
    mut bangs: EventReader<WallBangEvent>,
    mut live: EventReader<LiveBallEvent>,
    mut banners: EventReader<PlayBanner>,
    mut commands: Commands,
) {
    let Some(bank) = bank else { return };
    for _ in hits.read() {
        play(&mut commands, &bank.crack, 0.8);
    }
    for _ in bangs.read() {
        play(&mut commands, &bank.wall, 0.9);
    }
    for event in live.read() {
        if matches!(event, LiveBallEvent::Caught { .. }) {
            play(&mut commands, &bank.glove, 0.7);
        }
    }
    for banner in banners.read() {
        if banner.tone == BannerTone::Epic {
            play(&mut commands, &bank.stinger, 0.6);
        }
    }
}

pub struct SoundPlugin;

impl Plugin for SoundPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, build_sound_bank).add_systems(
            Update,
            play_event_sounds.run_if(in_state(GameState::Playing)),
        );
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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
}
