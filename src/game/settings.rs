//! Player-facing options: the persistent [`Settings`] resource, its storage
//! seam, and the settings screen. Batting styles are stored here and
//! consumed by the batting input adapters (spec §3); until those land the
//! values are inert data.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Which batting input front-end a player uses (spec §3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BattingStyle {
    ClassicTiming,
    SwingMeter,
    PciCursor,
}

impl BattingStyle {
    /// Settings-row value text. PCI carries its input recommendation.
    pub fn label(self) -> &'static str {
        match self {
            BattingStyle::ClassicTiming => "Classic timing",
            BattingStyle::SwingMeter => "Swing meter",
            BattingStyle::PciCursor => "PCI cursor (gamepad recommended)",
        }
    }

    pub fn next(self) -> Self {
        match self {
            BattingStyle::ClassicTiming => BattingStyle::SwingMeter,
            BattingStyle::SwingMeter => BattingStyle::PciCursor,
            BattingStyle::PciCursor => BattingStyle::ClassicTiming,
        }
    }

    pub fn prev(self) -> Self {
        self.next().next()
    }
}

/// Everything the player can configure. Persisted on every change.
#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// Batting style per player slot (index 0 = P1, 1 = P2).
    pub batting_style: [BattingStyle; 2],
    /// Master volume, 0.0..=1.0, applied via [`bevy::audio::GlobalVolume`].
    pub volume: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            batting_style: [BattingStyle::ClassicTiming; 2],
            volume: 0.7,
        }
    }
}

impl Settings {
    /// Returns a copy with every field forced into its legal range —
    /// applied after deserializing untrusted stored data.
    pub fn clamped(&self) -> Self {
        let mut result = self.clone();
        result.volume = result.volume.clamp(0.0, 1.0);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_cycles_through_all_three_and_wraps() {
        let s = BattingStyle::ClassicTiming;
        assert_eq!(s.next(), BattingStyle::SwingMeter);
        assert_eq!(s.next().next(), BattingStyle::PciCursor);
        assert_eq!(s.next().next().next(), BattingStyle::ClassicTiming);
        assert_eq!(s.prev(), BattingStyle::PciCursor);
        assert!(BattingStyle::PciCursor
            .label()
            .contains("gamepad recommended"));
    }

    #[test]
    fn settings_round_trip_and_defaults() {
        let s = Settings::default();
        assert_eq!(s.batting_style, [BattingStyle::ClassicTiming; 2]);
        assert!((s.volume - 0.7).abs() < f32::EPSILON);
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn clamped_bounds_volume() {
        let mut s = Settings::default();
        s.volume = 1.7;
        assert!((s.clamped().volume - 1.0).abs() < f32::EPSILON);
        s.volume = -0.3;
        assert!(s.clamped().volume.abs() < f32::EPSILON);
    }
}
