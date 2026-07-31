//! Batting input adapters (spec §3): each style is a front end that turns raw
//! [`Intents`] into the same [`SwingInput`]; `flow::pitch_live` consumes the
//! command and never sees the style. The CPU always routes Classic.

use bevy::prelude::*;
use bevy_rapier3d::prelude::Velocity;

use crate::game::ball::Baseball;
use crate::game::flow::{Phase, Play};
use crate::game::input::{Controllers, Intents};
use crate::game::rules;
use crate::game::settings::{BattingStyle, Settings};
use crate::game::variant::Ruleset;
use crate::game::{ScoreBoard, Team};

/// One swing, decided this frame. The swing instant is implicit (the frame
/// the command exists); `pci_offset` is the PCI cursor's zone-plane position
/// at the press (world x / height y), `None` for Classic and Meter.
pub struct SwingInput {
    pub aim: Vec2,
    pub pci_offset: Option<Vec2>,
}

/// How long the Swing Meter takes to fill from a fresh press to a full load,
/// in seconds. The load fraction (`0..1`) drives presentation only — the swing
/// is graded by *when* the batter releases, exactly like Classic timing.
const METER_FULL_SECS: f32 = 1.0;

/// The Swing Meter's per-team hold state: the `Time::elapsed_secs` at which the
/// current load began, `None` when the team is not holding. Only the human
/// meter arm of [`adapt_swings`] touches it; every other style leaves it
/// `default` so [`MeterState::load_frac`] reads a flat 0.
#[derive(Resource, Default)]
pub struct MeterState {
    home: Option<f32>,
    away: Option<f32>,
}

impl MeterState {
    fn slot(&self, team: Team) -> Option<f32> {
        match team {
            Team::Home => self.home,
            Team::Away => self.away,
        }
    }

    /// Whether `team` is mid-load (has an open hold).
    pub fn loading(&self, team: Team) -> bool {
        self.slot(team).is_some()
    }

    /// Opens a fresh load for `team` at `now` (`Time::elapsed_secs`).
    pub fn start(&mut self, team: Team, now: f32) {
        match team {
            Team::Home => self.home = Some(now),
            Team::Away => self.away = Some(now),
        }
    }

    /// Clears `team`'s load (release, forced swing, or phase change).
    pub fn clear(&mut self, team: Team) {
        match team {
            Team::Home => self.home = None,
            Team::Away => self.away = None,
        }
    }

    /// `team`'s current load fraction (`0..1`) at `now`; 0 when not loading.
    pub fn load_frac(&self, team: Team, now: f32) -> f32 {
        match self.slot(team) {
            Some(t0) => ((now - t0) / METER_FULL_SECS).clamp(0.0, 1.0),
            None => 0.0,
        }
    }
}

/// The *batting* team's current meter load fraction (`0..1`), republished every
/// frame for presentation — the animation stance-sink and the UI meter bar read
/// it. Always 0 for Classic/PCI batters and whenever no load is open.
#[derive(Resource, Default)]
pub struct MeterLoad(pub f32);

/// The PCI cursor's per-team position on the zone plane, in zone coordinates
/// (`x` = world x in meters, `y` = height in meters). Only the human PCI arm of
/// [`adapt_swings`] moves it; every other style leaves it at rest. Reset to the
/// zone center between pitches (see [`adapt_swings`]'s early-return path); the
/// field-side marker reads it via [`PciState::cursor`].
#[derive(Resource)]
pub struct PciState {
    home: Vec2,
    away: Vec2,
}

impl PciState {
    /// The zone-center resting position: horizontally centered, vertically at
    /// the midpoint of the called zone.
    fn center() -> Vec2 {
        Vec2::new(0.0, (rules::ZONE_LOW + rules::ZONE_HIGH) / 2.0)
    }

    /// `team`'s current cursor position (zone coordinates).
    pub fn cursor(&self, team: Team) -> Vec2 {
        match team {
            Team::Home => self.home,
            Team::Away => self.away,
        }
    }

    fn cursor_mut(&mut self, team: Team) -> &mut Vec2 {
        match team {
            Team::Home => &mut self.home,
            Team::Away => &mut self.away,
        }
    }
}

impl Default for PciState {
    fn default() -> Self {
        let center = Self::center();
        Self {
            home: center,
            away: center,
        }
    }
}

/// What the Swing Meter arm does this frame, given `held` (the action button
/// held now), `was_loading` (a load was already open), and `ball_past` (the
/// ball has travelled beyond the late swing edge). Returns
/// `(now_loading, fire_swing)`. Pure so the state machine tests without ECS.
pub(crate) fn meter_step(held: bool, was_loading: bool, ball_past: bool) -> (bool, bool) {
    match (held, was_loading, ball_past) {
        (true, false, false) => (true, false), // press: start loading
        (true, true, false) => (true, false),  // keep loading
        (false, true, _) => (false, true),     // release: swing NOW
        (true, _, true) => (false, true),      // held too long: forced swing → whiff
        _ => (false, false),
    }
}

/// This frame's swing command per team, produced by [`adapt_swings`] and
/// consumed (once) by `flow::pitch_live`. Commands are single-frame: cleared
/// at the top of every `adapt_swings` run.
#[derive(Resource, Default)]
pub struct SwingCommands {
    home: Option<SwingInput>,
    away: Option<SwingInput>,
}

impl SwingCommands {
    /// Peek at the pending command for `team`, if any.
    pub fn get(&self, team: Team) -> Option<&SwingInput> {
        match team {
            Team::Home => self.home.as_ref(),
            Team::Away => self.away.as_ref(),
        }
    }

    /// Consume the pending command for `team`, if any.
    pub fn take(&mut self, team: Team) -> Option<SwingInput> {
        match team {
            Team::Home => self.home.take(),
            Team::Away => self.away.take(),
        }
    }

    /// Set the pending command for `team`.
    pub fn set(&mut self, team: Team, cmd: SwingInput) {
        match team {
            Team::Home => self.home = Some(cmd),
            Team::Away => self.away = Some(cmd),
        }
    }
}

/// Which batting style drives `team`'s swing this at-bat. The CPU always
/// routes Classic (spec §3) regardless of the settings screen's per-player
/// choices — those only apply to a human-controlled slot.
pub fn style_for(team: Team, controllers: &Controllers, settings: &Settings) -> BattingStyle {
    match controllers.player_index(team) {
        None => BattingStyle::ClassicTiming, // CPU: always Classic (spec §3)
        Some(i) => settings.batting_style[i],
    }
}

/// The adapter: runs after `cpu_offense` (so CPU edges are visible) and
/// before `pre_pitch`/`pitch_live` (so a command lands the same frame).
#[allow(clippy::too_many_arguments)]
pub fn adapt_swings(
    time: Res<Time>,
    intents: Res<Intents>,
    controllers: Res<Controllers>,
    settings: Res<Settings>,
    rules: Res<Ruleset>,
    score: Res<ScoreBoard>,
    play: Res<Play>,
    ball_q: Query<(&Transform, &Velocity), With<Baseball>>,
    mut commands: ResMut<SwingCommands>,
    mut meter: ResMut<MeterState>,
    mut load: ResMut<MeterLoad>,
    mut pci: ResMut<PciState>,
) {
    let now = time.elapsed_secs();
    let team = score.batting_team();
    let intent = intents.get(team);
    // Commands are single-frame: clear both slots first.
    *commands = SwingCommands::default();
    if play.phase != Phase::Pitch {
        // Between pitches the meter is idle: forget any dangling hold so the
        // next at-bat starts from an empty bar (and presentation reads 0). The
        // PCI cursor likewise re-centers so every at-bat opens from the middle
        // of the zone.
        *meter = MeterState::default();
        *pci = PciState::default();
        load.0 = 0.0;
        return;
    }
    match style_for(team, &controllers, &settings) {
        BattingStyle::ClassicTiming => {
            if intent.action {
                commands.set(
                    team,
                    SwingInput {
                        aim: intent.aim,
                        pci_offset: None,
                    },
                );
            }
        }
        // Hold the action to load the meter, release to swing. A release
        // fires this frame's command; still holding once the ball crosses the
        // late swing edge forces a swing that `pitch_live` grades a Whiff (the
        // ball is already beyond `late_swing_z`, so the reachability gate
        // catches it) — the spec's "held past the FoulTip window = a swinging
        // whiff", with no new flow logic.
        BattingStyle::SwingMeter => {
            let ball_past = ball_q.get_single().is_ok_and(|(tf, vel)| {
                tf.translation.z < crate::game::flow::late_swing_z(vel.linvel.z, rules.foul_ms)
            });
            let was = meter.loading(team);
            let (now_loading, fire) = meter_step(intent.action_held, was, ball_past);
            if now_loading && !was {
                meter.start(team, now);
            }
            if !now_loading {
                meter.clear(team);
            }
            if fire {
                commands.set(
                    team,
                    SwingInput {
                        aim: intent.aim,
                        pci_offset: None,
                    },
                );
            }
        }
        // Move the aiming cursor with the stick (a *velocity*, so it is
        // keyboard-playable — a held direction glides it), press to swing at
        // the barrel's current spot. The swing's direction comes from where the
        // cursor sat relative to the ball (`pitch_live` reads `pci_offset`), not
        // from raw aim.
        BattingStyle::PciCursor => {
            // Glide: aim is a velocity, not a position. Stick-right moves the
            // cursor toward screen-right; from the behind-home camera that is
            // world −X (first base side), matching the pitch-aim mapping's
            // negation (CLAUDE.md). Note the stick does double duty this frame:
            // the Down-hold runner-send read (`wants_send` in flow.rs) also
            // watches aim, so steering the cursor and sending a runner mid-pitch
            // share the stick by design — but the leadoff send decision
            // (`steal_armed`) is committed pre-delivery, so the real conflict
            // window is small.
            const PCI_SPEED_MPS: f32 = 1.6;
            let c = pci.cursor_mut(team);
            c.x -= intent.aim.x * PCI_SPEED_MPS * time.delta_secs();
            c.y += intent.aim.y * PCI_SPEED_MPS * time.delta_secs();
            c.x = c.x.clamp(-rules::ZONE_HALF_WIDTH, rules::ZONE_HALF_WIDTH);
            c.y = c.y.clamp(rules::ZONE_LOW, rules::ZONE_HIGH);
            if intent.action {
                commands.set(
                    team,
                    SwingInput {
                        aim: intent.aim,
                        pci_offset: Some(*c),
                    },
                );
            }
        }
    }
    // Republish the batting team's load for presentation (0 for any style that
    // never opened a hold, and 0 the frame a swing fires and clears it).
    load.0 = meter.load_frac(team, now);
}

/// Registers [`SwingCommands`]; the [`adapt_swings`] system itself is chained
/// explicitly by `FlowPlugin` so ordering relative to `cpu_offense`/
/// `pre_pitch` is visible in one place.
pub struct BattingPlugin;

impl Plugin for BattingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SwingCommands>()
            .init_resource::<MeterState>()
            .init_resource::<MeterLoad>()
            .init_resource::<PciState>();
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::input::assign_controllers;
    use crate::game::GameMode;

    #[test]
    fn meter_release_fires_the_swing() {
        assert_eq!(meter_step(false, true, false), (false, true));
    }

    #[test]
    fn meter_holding_past_the_window_is_a_swinging_whiff() {
        assert_eq!(meter_step(true, true, true), (false, true));
    }

    #[test]
    fn meter_press_starts_loading_without_swinging() {
        assert_eq!(meter_step(true, false, false), (true, false));
    }

    #[test]
    fn meter_idle_does_nothing() {
        // No hold, nothing loaded: neither loads nor fires.
        assert_eq!(meter_step(false, false, false), (false, false));
    }

    #[test]
    fn meter_load_frac_ramps_and_clamps() {
        let mut meter = MeterState::default();
        assert_eq!(meter.load_frac(Team::Away, 5.0), 0.0);
        meter.start(Team::Away, 5.0);
        assert!(meter.loading(Team::Away));
        assert!((meter.load_frac(Team::Away, 5.5) - 0.5).abs() < 1e-6);
        assert_eq!(meter.load_frac(Team::Away, 7.0), 1.0); // clamped at full
        meter.clear(Team::Away);
        assert!(!meter.loading(Team::Away));
        assert_eq!(meter.load_frac(Team::Away, 7.0), 0.0);
    }

    #[test]
    fn cpu_always_routes_classic() {
        let controllers = assign_controllers(GameMode::OnePlayer, &[]);
        let settings = Settings {
            batting_style: [BattingStyle::PciCursor, BattingStyle::SwingMeter],
            ..Settings::default()
        };
        assert_eq!(
            style_for(Team::Away, &controllers, &settings),
            BattingStyle::ClassicTiming
        );
        assert_eq!(
            style_for(Team::Home, &controllers, &settings),
            BattingStyle::PciCursor
        );
    }
}
