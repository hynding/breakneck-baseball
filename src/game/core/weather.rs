//! Game-day conditions the player picks before first pitch: the weather
//! (temperature, how hard the wind blows, the sky) and how deep the park's
//! fences sit. Pure data, like [`crate::game::variant`]: `flow` draws the
//! day from a [`Weather`] at game start, the menu applies [`Fences`] to the
//! [`FieldSpec`], and presentation reads [`Conditions`] for the sky and the
//! rain.
//!
//! The defaults ([`Weather::Fair`], [`Fences::Standard`]) are exactly the
//! game before this module: a 70°F day with the light random wind the
//! balance sim was tuned in, on the regulation fences.

use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};

use crate::game::variant::FieldSpec;

/// The temperature carry is neutral at (°F): a mild day, the conditions the
/// balance sim's economy was tuned in.
pub const NEUTRAL_TEMP_F: f32 = 70.0;

/// Exit-speed change per °F away from [`NEUTRAL_TEMP_F`]. Warm air is
/// thinner, so a fly carries further: about 1% of distance per 10°F (≈ 4 ft
/// on a 400 ft fly, per docs/BASEBALL.md, "Weather"). Carry grows about
/// twice as fast as exit speed in the game's flight model (the same
/// coupling `Ratings::power_factor` documents), so half that goes on the
/// exit speed.
const CARRY_PER_DEG_F: f32 = 0.0005;

/// The weather for a game, picked on the menu (**W**) and kept in the
/// settings store.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Weather {
    /// A mild, clear day with a light breeze from anywhere: the game's
    /// long-standing default.
    #[default]
    Fair,
    /// A hot summer afternoon: the ball carries.
    Hot,
    /// An early-April chill under grey skies: the ball dies.
    Cold,
    /// A gusty day: a strong wind from anywhere.
    Windy,
    /// A cool, wet night-like gloom with rain falling.
    Rain,
    /// Still air, as under a roof.
    Calm,
}

/// What a [`Weather`] means for the game: its temperature, the wind's
/// speed range, and how the sky and light change.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeatherSpec {
    pub temp_f: f32,
    /// Wind speed range (mph): a draw `u` in 0..1 maps to
    /// `min + (max − min)·u²`, so most days sit near the light end.
    pub wind_mph: (f32, f32),
    /// How far the sky colour is pulled toward storm grey (0 = the theme's
    /// sky untouched).
    pub overcast: f32,
    /// The sun's strength as a multiple of the theme's.
    pub sun: f32,
    /// Whether rain falls.
    pub rain: bool,
}

impl Weather {
    pub const ALL: [Weather; 6] = [
        Weather::Fair,
        Weather::Hot,
        Weather::Cold,
        Weather::Windy,
        Weather::Rain,
        Weather::Calm,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Weather::Fair => "Fair",
            Weather::Hot => "Hot",
            Weather::Cold => "Cold",
            Weather::Windy => "Windy",
            Weather::Rain => "Rain",
            Weather::Calm => "Calm",
        }
    }

    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|w| *w == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    /// The numbers behind the name (per docs/BASEBALL.md, "Weather" and
    /// "Wind"): MLB game-time temperatures run from the 40s in April to the
    /// 90s in summer, and park-average winds from ~2.5 to ~12 mph, with
    /// gusty days well past that.
    pub fn spec(self) -> WeatherSpec {
        let base = WeatherSpec {
            temp_f: NEUTRAL_TEMP_F,
            // The draw the balance sim was tuned in (`flow::wind_in_range`).
            wind_mph: (2.0, 14.0),
            overcast: 0.0,
            sun: 1.0,
            rain: false,
        };
        match self {
            Weather::Fair => base,
            Weather::Hot => WeatherSpec {
                temp_f: 95.0,
                wind_mph: (2.0, 10.0),
                sun: 1.15,
                ..base
            },
            Weather::Cold => WeatherSpec {
                temp_f: 42.0,
                overcast: 0.55,
                sun: 0.6,
                ..base
            },
            Weather::Windy => WeatherSpec {
                wind_mph: (14.0, 24.0),
                overcast: 0.2,
                ..base
            },
            Weather::Rain => WeatherSpec {
                temp_f: 55.0,
                wind_mph: (4.0, 14.0),
                overcast: 0.8,
                sun: 0.45,
                rain: true,
            },
            Weather::Calm => WeatherSpec {
                wind_mph: (0.0, 0.0),
                ..base
            },
        }
    }
}

/// The exit-speed multiplier the air puts on a batted ball at `temp_f`.
pub fn carry_factor(temp_f: f32) -> f32 {
    1.0 + CARRY_PER_DEG_F * (temp_f - NEUTRAL_TEMP_F)
}

/// This game's conditions, drawn at game start (`flow`) from the chosen
/// [`Weather`]. The wind itself lives in [`crate::game::rules::Wind`],
/// which every flight reads.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct Conditions {
    pub weather: Weather,
    pub temp_f: f32,
}

impl Default for Conditions {
    fn default() -> Self {
        Conditions {
            weather: Weather::Fair,
            temp_f: NEUTRAL_TEMP_F,
        }
    }
}

impl Conditions {
    /// The day's conditions for `weather`.
    pub fn for_weather(weather: Weather) -> Self {
        Conditions {
            weather,
            temp_f: weather.spec().temp_f,
        }
    }

    /// The exit-speed multiplier for today's air ([`carry_factor`]).
    pub fn carry(&self) -> f32 {
        carry_factor(self.temp_f)
    }

    /// The scoreboard's line for the temperature, e.g. "72F" (plain ASCII:
    /// the bundled font has no degree sign).
    pub fn temp_report(&self) -> String {
        format!("{:.0}F", self.temp_f)
    }
}

/// When the game is played, picked on the menu (**N**) and kept in the
/// settings store: a field option, independent of the theme. Purely visual
/// (the sun, the sky and the lights); it never touches the ball.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TimeOfDay {
    /// A low, warm sun and a paler sky.
    Morning,
    /// The game's long-standing look: the theme exactly as authored.
    #[default]
    Afternoon,
    /// A dark sky with the field lit from the light towers.
    Night,
}

impl TimeOfDay {
    pub const ALL: [TimeOfDay; 3] = [TimeOfDay::Morning, TimeOfDay::Afternoon, TimeOfDay::Night];

    pub fn label(self) -> &'static str {
        match self {
            TimeOfDay::Morning => "Morning",
            TimeOfDay::Afternoon => "Afternoon",
            TimeOfDay::Night => "Night",
        }
    }

    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|t| *t == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    /// The key light's angle above the horizon (radians): a low morning
    /// sun for long shadows, the afternoon's 45°, and at night the high
    /// throw of a tower bank.
    pub fn key_elevation(self) -> f32 {
        match self {
            TimeOfDay::Morning => 22f32.to_radians(),
            TimeOfDay::Afternoon => 45f32.to_radians(),
            TimeOfDay::Night => 50f32.to_radians(),
        }
    }
}

/// How deep the outfield fences sit, picked on the menu (**O**) and kept in
/// the settings store. Real parks run from ~310 ft down the lines and ~390
/// ft to centre (cozy) to ~345 / 420 ft (spacious), per docs/BASEBALL.md,
/// "Field dimensions".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Fences {
    /// A bandbox: short lines, a shallow centre field.
    Cozy,
    /// The regulation park every variant is authored with.
    #[default]
    Standard,
    /// A pitcher's park: deep lines, a cavernous centre field.
    Deep,
}

impl Fences {
    pub const ALL: [Fences; 3] = [Fences::Cozy, Fences::Standard, Fences::Deep];

    pub fn label(self) -> &'static str {
        match self {
            Fences::Cozy => "Cozy",
            Fences::Standard => "Standard",
            Fences::Deep => "Deep",
        }
    }

    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|f| *f == self).unwrap_or(1);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    /// The multipliers on the variant's fence distance down the lines and to
    /// centre. On the standard park (100 m / 122 m, 328 / 400 ft) Cozy is
    /// ~310 / 380 ft and Deep ~345 / 420 ft.
    pub fn scale(self) -> (f32, f32) {
        match self {
            Fences::Cozy => (0.945, 0.95),
            Fences::Standard => (1.0, 1.0),
            Fences::Deep => (1.05, 1.05),
        }
    }

    /// `field` with its fences moved to this depth. Everything that knows
    /// the wall (the stadium mesh, the home-run call, the fielders' fence
    /// margin) reads [`crate::game::rules::fence_at`], so this is the only
    /// change a park needs; the ball-loss radius grows with a deep park.
    pub fn apply(self, mut field: FieldSpec) -> FieldSpec {
        let (line, center) = self.scale();
        field.fence_line *= line;
        field.fence_center *= center;
        field.bounds *= center.max(1.0);
        field
    }
}

#[cfg(test)]
#[path = "weather.test.rs"]
mod tests;
