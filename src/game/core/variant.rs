//! Game variants — the data that makes a game of "baseball" *this* game.
//!
//! A [`Ruleset`] holds every countable threshold the rules engine reads, and a
//! [`FieldSpec`] holds the geometry, personnel, and presentation of the park.
//! Both are plain data, inserted as resources when a game starts, so adding a
//! new variant means adding a new definition here — not new systems. Home
//! plate is at the world origin with +Z toward the field in every variant;
//! all plate-local logic (pitching, swinging, cameras) is shared.

use bevy::math::Vec3;
use bevy::prelude::{Reflect, Resource};

use crate::game::rules::{HALF_DIAGONAL, PITCH_DISTANCE};

/// Countable-rule knobs read by the rules engine and game flow, grouped so
/// the debug inspector renders each group as its own collapsible section.
#[derive(Resource, Clone, Debug, Reflect)]
pub struct Ruleset {
    pub counts: CountRules,
    pub batting: BattingTuning,
    pub pace: PaceTuning,
}

impl Ruleset {
    /// Paste-ready Rust lines for every field differing from `variant`'s
    /// defaults — the debug panel's tuning-session export.
    pub fn diff_literal(&self, variant: VariantId) -> String {
        let d = variant.rules();
        let mut out = String::new();
        macro_rules! diff {
            ($($path:ident).+) => {
                if self.$($path).+ != d.$($path).+ {
                    out.push_str(&format!(
                        concat!(stringify!($($path).+), ": {:?},\n"),
                        self.$($path).+
                    ));
                }
            };
        }
        diff!(counts.balls_per_walk);
        diff!(counts.strikes_per_out);
        diff!(counts.outs_per_half);
        diff!(counts.innings);
        diff!(counts.peg_outs);
        diff!(counts.steal_window_secs);
        diff!(batting.perfect_ms);
        diff!(batting.solid_ms);
        diff!(batting.foul_ms);
        diff!(batting.exit_weak);
        diff!(batting.exit_solid);
        diff!(batting.exit_perfect);
        diff!(batting.pull_yaw_per_ms);
        diff!(batting.cpu_timing_spread_ms);
        diff!(batting.pci_radius_m);
        diff!(pace.pitch_speed_scale);
        diff!(pace.runner_speed);
        diff!(pace.fielder_speed);
        diff!(pace.reaction_secs);
        diff!(pace.throw_speed);
        diff!(pace.throw_transfer_secs);
        diff!(pace.relay_transfer_secs);
        diff!(pace.hit_and_run_jump_secs);
        diff!(pace.stretch_grace_secs);
        diff!(pace.runner_margin_secs);
        diff!(pace.result_secs);
        diff!(pace.pickoff_cooldown_secs);
        diff!(pace.auto_throw_delay_secs);
        if out.is_empty() {
            out
        } else {
            format!("// VariantId::{:?} overrides:\n{}", variant, out)
        }
    }
}

/// Count thresholds and window rules.
#[derive(Clone, Debug, Reflect)]
pub struct CountRules {
    /// Balls that walk the batter.
    pub balls_per_walk: u32,
    /// Strikes that retire the batter.
    pub strikes_per_out: u32,
    /// Outs that end a half-inning.
    pub outs_per_half: u32,
    /// Regulation innings.
    pub innings: u32,
    /// Whether a batted ball landing near a fielder pegs the runner out
    /// (front-yard rules: outs by hitting the runner with the ball).
    pub peg_outs: bool,
    /// Length of the pre-pitch steal window (seconds): with a runner able to
    /// steal, the pitch is held this long while the leadoff/pickoff duel
    /// runs. Zero disables the window.
    pub steal_window_secs: f32,
}

/// Batting-feel timing/contact tuning (see the batting-feel spec §2).
// ── Batting-feel timing/contact tuning (per docs/superpowers/specs/
// 2026-07-30-batting-feel-design.md §2) ────────────────────────────────
// `rules::contact_quality` maps a swing's timing error (milliseconds,
// signed: negative = early) to a `ContactQuality` using these windows —
// data, not code, so each variant can feel different without touching
// `rules.rs`. The B7 balance harness is the tuning arbiter for these
// numbers: they start at the plan's defaults and only that harness
// should move them.
#[derive(Clone, Debug, Reflect)]
pub struct BattingTuning {
    /// |dt| at or under this many ms is a `ContactQuality::Perfect`.
    pub perfect_ms: f32,
    /// |dt| at or under this many ms (and over `perfect_ms`) is a
    /// `ContactQuality::Solid`.
    pub solid_ms: f32,
    /// |dt| at or under this many ms (and over `solid_ms`) is a
    /// `ContactQuality::FoulTip`; beyond it, a `ContactQuality::Whiff`.
    pub foul_ms: f32,
    /// Exit-speed multiplier for `ContactQuality::Weak` contact. Weak is
    /// never produced by these Classic windows — it's the Plan-C PCI
    /// adapter's window-shrink outcome — but the multiplier lives here so
    /// the adapter has a single place to tune it per variant.
    pub exit_weak: f32,
    /// Exit-speed multiplier for `ContactQuality::Solid` contact.
    pub exit_solid: f32,
    /// Exit-speed multiplier for `ContactQuality::Perfect` contact.
    pub exit_perfect: f32,
    /// Pull-side yaw offset (radians) applied per millisecond of signed
    /// timing error on Solid/Perfect contact.
    pub pull_yaw_per_ms: f32,
    /// Standard deviation (ms) of the CPU batter's swing-timing scatter.
    pub cpu_timing_spread_ms: f32,
    /// PCI cursor radius (metres) — where timing windows shrink to zero per spec §3.
    pub pci_radius_m: f32,
}

/// Speeds, delays, and race clocks — the game's pace. Defaults are the
/// long-standing module constants; `tests/balance_sim.rs` arbitrates changes.
#[derive(Clone, Debug, Reflect)]
pub struct PaceTuning {
    /// Scales every `PitchKind::speed()` at release (1.0 = the kind table).
    /// This is how the spec's `PITCH_SPEED` promotion lands: one dial for
    /// all five pitches instead of a fastball-only field.
    pub pitch_speed_scale: f32,
    /// Base-runner sprint speed (m/s) — was `rules::RUNNER_SPEED`.
    pub runner_speed: f32,
    /// Fielder sprint speed (m/s) — was `rules::FIELDER_SPEED`.
    pub fielder_speed: f32,
    /// First-step reaction delay for fielders and runners alike — was
    /// `rules::REACTION`.
    pub reaction_secs: f32,
    /// Throw flight speed (m/s) — was `rules::THROW_FLIGHT_SPEED`.
    pub throw_speed: f32,
    /// Glove-to-hand transfer time for a gather — was `rules::THROW_TRANSFER`.
    pub throw_transfer_secs: f32,
    /// Glove-to-hand transfer time for a relay — was `rules::RELAY_TRANSFER`.
    pub relay_transfer_secs: f32,
    /// Head start a hit-and-run jump gives every forced runner — was
    /// `rules::HIT_AND_RUN_JUMP`.
    pub hit_and_run_jump_secs: f32,
    /// Extra grace a sent batter gets stretching for the next base — was
    /// `rules::STRETCH_GRACE`.
    pub stretch_grace_secs: f32,
    /// Bang-bang margin: ties and near-ties go to the runner — was
    /// `rules::RUNNER_MARGIN`.
    pub runner_margin_secs: f32,
    /// Seconds the result banner lingers before the next pitch — was
    /// `flow::RESULT_SECS`.
    pub result_secs: f32,
    /// Minimum seconds between pickoff throws — was
    /// `flow::PICKOFF_COOLDOWN_SECS`.
    pub pickoff_cooldown_secs: f32,
    /// How long the holder waits for a manual throw before auto-throwing —
    /// was `fielding::AUTO_THROW_DELAY`.
    pub auto_throw_delay_secs: f32,
}

impl Default for PaceTuning {
    /// Sourced straight from the long-standing module constants they promote
    /// — this is the single source of truth those consts now feed, so the
    /// two can never quietly drift apart.
    fn default() -> Self {
        use crate::game::{fielding, flow, rules};
        Self {
            pitch_speed_scale: 1.0,
            runner_speed: rules::RUNNER_SPEED,
            fielder_speed: rules::FIELDER_SPEED,
            reaction_secs: rules::REACTION,
            throw_speed: rules::THROW_FLIGHT_SPEED,
            throw_transfer_secs: rules::THROW_TRANSFER,
            relay_transfer_secs: rules::RELAY_TRANSFER,
            hit_and_run_jump_secs: rules::HIT_AND_RUN_JUMP,
            stretch_grace_secs: rules::STRETCH_GRACE,
            runner_margin_secs: rules::RUNNER_MARGIN,
            result_secs: flow::RESULT_SECS,
            pickoff_cooldown_secs: flow::PICKOFF_COOLDOWN_SECS,
            auto_throw_delay_secs: fielding::AUTO_THROW_DELAY,
        }
    }
}

/// Menu-selectable regulation game lengths.
pub const INNINGS_OPTIONS: [u32; 4] = [1, 3, 6, 9];

/// The next game-length option in the menu cycle (wraps; values not in the
/// list restart it).
pub fn next_innings(current: u32) -> u32 {
    match INNINGS_OPTIONS.iter().position(|&n| n == current) {
        Some(i) => INNINGS_OPTIONS[(i + 1) % INNINGS_OPTIONS.len()],
        None => INNINGS_OPTIONS[0],
    }
}

/// Field geometry and personnel. Home plate is implicitly at the origin.
#[derive(Resource, Clone, Debug, Reflect)]
pub struct FieldSpec {
    /// Bases in running order (first base first); the last base leads home.
    pub base_positions: Vec<Vec3>,
    /// Pitching rubber sits at `(0, h, pitch_distance)`.
    pub pitch_distance: f32,
    /// Fair territory spans this angle (radians) each side of +Z.
    pub fair_half_angle: f32,
    /// Home-run fence distance down the foul lines.
    pub fence_line: f32,
    /// Home-run fence distance to straightaway centre.
    pub fence_center: f32,
    /// Scales the batted-ball outcome distance bands to the park's size.
    pub hit_scale: f32,
    /// Peg-out proximity: a low ball landing this close to a fielder beans the
    /// runner. Only consulted when [`Ruleset::peg_outs`] is set.
    pub peg_radius: f32,
    /// Defensive spawn spots *excluding* the pitcher, who always stands at
    /// the rubber. Length sets the fielder count; the team size is this + 1.
    pub fielder_positions: Vec<Vec3>,
    /// Umpire spawn spots. The first entry (behind the plate, z < 0) is the
    /// plate umpire, who crouches through the duel; the rest work the bases.
    /// Purely presentational — the rules module is the actual umpire.
    pub umpire_positions: Vec<Vec3>,
    /// Ball-reset radius: past this the ball is considered lost.
    pub bounds: f32,
    /// Broadcast-camera eye position for this park's size (wide framing,
    /// used while the ball is in play).
    pub broadcast_eye: Vec3,
    /// Broadcast-camera resting look-at point.
    pub broadcast_target: Vec3,
    /// Tight at-bat framing used during the pitch/swing duel (catcher POV,
    /// the default [`crate::game::camera::DuelView`]).
    pub duel_eye: Vec3,
    pub duel_target: Vec3,
    /// The reference "pitcher cam": behind and above the mound, looking out
    /// at the batter. Far enough from the plate that the catcher/plate
    /// umpire are never auto-hidden here — they're meant to stay in frame.
    pub behind_pitcher_eye: Vec3,
    pub behind_pitcher_target: Vec3,
    /// A tight zoom from behind and beside the batter's box, looking across
    /// the zone toward the pitcher — close enough behind the plate that the
    /// catcher (and the umpire behind him) sit in the sightline and get
    /// auto-hidden.
    pub batting_zoom_eye: Vec3,
    pub batting_zoom_target: Vec3,
    /// Which scenery routine dresses the set.
    pub scenery: Scenery,
}

impl FieldSpec {
    /// Number of bases excluding home.
    pub fn base_count(&self) -> usize {
        self.base_positions.len()
    }
}

/// Which spawn routine builds the surroundings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Reflect)]
pub enum Scenery {
    /// Classic ballpark: infield diamond, mound, foul poles, outfield wall.
    Stadium,
    /// Suburban lot: lawn, street, sidewalks, houses, hedges.
    FrontYard,
}

/// The selectable variants, cycled on the main menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum VariantId {
    #[default]
    Standard,
    FrontYard,
}

impl VariantId {
    /// The next variant in the menu cycle (wraps).
    pub fn next(self) -> VariantId {
        match self {
            VariantId::Standard => VariantId::FrontYard,
            VariantId::FrontYard => VariantId::Standard,
        }
    }

    /// Menu label.
    pub fn label(self) -> &'static str {
        match self {
            VariantId::Standard => "Classic Stadium",
            VariantId::FrontYard => "Front Yard",
        }
    }

    /// The rule thresholds for this variant.
    pub fn rules(self) -> Ruleset {
        match self {
            VariantId::Standard => Ruleset {
                counts: CountRules {
                    balls_per_walk: 4,
                    strikes_per_out: 3,
                    outs_per_half: 3,
                    innings: 9,
                    peg_outs: false,
                    steal_window_secs: 1.5,
                },
                batting: BattingTuning {
                    perfect_ms: 40.0,
                    solid_ms: 90.0,
                    foul_ms: 130.0,
                    exit_weak: 0.65,
                    exit_solid: 0.95,
                    exit_perfect: 1.28,
                    pull_yaw_per_ms: 0.006,
                    cpu_timing_spread_ms: 225.0,
                    pci_radius_m: 0.20,
                },
                pace: PaceTuning::default(),
            },
            // Kid's rules: short games, outs by pegging the runner.
            VariantId::FrontYard => Ruleset {
                counts: CountRules {
                    balls_per_walk: 4,
                    strikes_per_out: 3,
                    outs_per_half: 3,
                    innings: 3,
                    peg_outs: true,
                    steal_window_secs: 1.5,
                },
                batting: BattingTuning {
                    perfect_ms: 40.0,
                    solid_ms: 90.0,
                    foul_ms: 130.0,
                    exit_weak: 0.65,
                    exit_solid: 0.95,
                    exit_perfect: 1.28,
                    pull_yaw_per_ms: 0.006,
                    cpu_timing_spread_ms: 225.0,
                    pci_radius_m: 0.20,
                },
                pace: PaceTuning::default(),
            },
        }
    }

    /// The park definition for this variant.
    pub fn field(self) -> FieldSpec {
        match self {
            VariantId::Standard => FieldSpec {
                // Regulation diamond: 90 ft base paths mean each bag sits
                // HALF_DIAGONAL (27.43/√2 m) off-axis — matching the dirt
                // infield drawn in `field.rs`. The behind-home cameras render
                // world −X on screen-right, so first base lives at −X (the
                // right-field line as the viewer sees it).
                base_positions: vec![
                    Vec3::new(-HALF_DIAGONAL, 0.0, HALF_DIAGONAL),
                    Vec3::new(0.0, 0.0, HALF_DIAGONAL * 2.0),
                    Vec3::new(HALF_DIAGONAL, 0.0, HALF_DIAGONAL),
                ],
                pitch_distance: PITCH_DISTANCE,
                fair_half_angle: std::f32::consts::FRAC_PI_4,
                fence_line: 100.0,
                fence_center: 122.0,
                hit_scale: 1.0,
                peg_radius: 0.0,
                fielder_positions: vec![
                    Vec3::new(0.0, 0.0, -1.5),                           // catcher
                    Vec3::new(-HALF_DIAGONAL, 0.0, HALF_DIAGONAL - 3.0), // first base
                    Vec3::new(-7.0, 0.0, HALF_DIAGONAL * 2.0 - 3.0),     // second base
                    Vec3::new(7.0, 0.0, HALF_DIAGONAL * 2.0 - 3.0),      // shortstop
                    Vec3::new(HALF_DIAGONAL, 0.0, HALF_DIAGONAL - 3.0),  // third base
                    Vec3::new(40.0, 0.0, 85.0), // left field (screen left = +X)
                    Vec3::new(0.0, 0.0, 110.0), // centre field
                    Vec3::new(-40.0, 0.0, 85.0), // right field
                ],
                // A full crew: behind the plate, outside each line at first
                // and third, and behind the keystone for the middle.
                umpire_positions: vec![
                    Vec3::new(0.0, 0.0, -3.0),
                    Vec3::new(-HALF_DIAGONAL - 3.0, 0.0, HALF_DIAGONAL + 2.0),
                    Vec3::new(0.0, 0.0, HALF_DIAGONAL * 2.0 + 4.0),
                    Vec3::new(HALF_DIAGONAL + 3.0, 0.0, HALF_DIAGONAL + 2.0),
                ],
                bounds: 220.0,
                broadcast_eye: Vec3::new(0.0, 13.0, -21.0),
                broadcast_target: Vec3::new(0.0, 1.2, 9.0),
                // The catcher's own point of view, tilted down onto the
                // batter: the lens sits at his crouched eye height (~1.44 m
                // — the rig is authored 1.85 m tall, head centred at
                // Blender Z=1.66 standing, and `CatcherCrouch` translates
                // the Hips chain down 0.22 m), looking down toward the
                // plate so the batter's *entire* body — spikes to helmet —
                // fills 80–90% of the screen height
                // (`camera::catcher_pov_frames_the_full_batter_at_80_to_90_percent`
                // is the arbiter; the low target is the downward tilt that
                // keeps his feet in frame). The eye now sits at z=-1.2,
                // fractionally *inside* the catcher's silhouette (his
                // capsule's forward surface is ~z=-1.1), which is why
                // `camera::hide_occluders` hides the catcher outright in
                // this view for as long as the duel framing holds.
                duel_eye: Vec3::new(0.0, 1.4, -1.2),
                duel_target: Vec3::new(0.0, 0.2, 4.0),
                // Behind and above the mound (rubber at z=`pitch_distance`),
                // looking back at the batter's box — the reference
                // pitcher-cam shot. 3 m of standoff behind the rubber keeps
                // the pitcher's own rig out of the near clip; at that
                // distance the catcher (z=-1.5) and plate umpire (z=-3.0)
                // are ~21-23 m from the eye, far outside the near-eye
                // occlusion cone (`OCCLUSION_NEAR`), so they stay visible —
                // exactly what the reference shot wants.
                behind_pitcher_eye: Vec3::new(0.0, 2.2, PITCH_DISTANCE + 3.0),
                behind_pitcher_target: Vec3::new(0.3, 1.0, 0.0),
                // Behind and beside the batter's box, elevated a touch above
                // and behind the plate umpire — a tight "zone cam" close
                // enough that the catcher (and the umpire behind him) sit
                // right in the sightline down the pipe, so they're the ones
                // auto-hidden here (see `camera::hide_occluders`).
                batting_zoom_eye: Vec3::new(0.8, 1.7, -3.2),
                batting_zoom_target: Vec3::new(0.1, 1.0, 12.0),
                scenery: Scenery::Stadium,
            },
            // A front lawn: four bases across the lawn corners, the defense
            // strung out over the sidewalks and the neighbours' yards, and a
            // home run means clearing the houses across the street.
            VariantId::FrontYard => FieldSpec {
                // Running order sweeps screen-right (−X) to screen-left (+X),
                // mirroring the stadium's first-base-at-−X convention.
                base_positions: vec![
                    Vec3::new(-8.0, 0.0, 6.0),
                    Vec3::new(-10.0, 0.0, 14.0),
                    Vec3::new(10.0, 0.0, 14.0),
                    Vec3::new(8.0, 0.0, 6.0),
                ],
                pitch_distance: 10.0,
                fair_half_angle: 55.0_f32.to_radians(),
                fence_line: 38.0,
                fence_center: 48.0,
                hit_scale: 0.4,
                peg_radius: 4.5,
                fielder_positions: vec![
                    Vec3::new(12.0, 0.0, 20.0),  // right sidewalk
                    Vec3::new(-12.0, 0.0, 20.0), // left sidewalk
                    Vec3::new(0.0, 0.0, 34.0),   // across-the-street yard
                ],
                // A kid's game gets one grown-up calling it from the lawn.
                umpire_positions: vec![Vec3::new(0.0, 0.0, -2.2)],
                bounds: 90.0,
                broadcast_eye: Vec3::new(0.0, 7.0, -12.0),
                broadcast_target: Vec3::new(0.0, 1.0, 5.0),
                // No catcher on the lawn (see `fielder_positions` above —
                // none sits at z<0); the lone plate umpire (z=-2.2, front
                // surface ~z=-1.8) stays behind the eye at z=-1.25, so
                // nothing needs hiding here. Same full-body batter framing
                // contract as Standard's `duel_eye` (80–90% of screen
                // height, camera test is the arbiter), scaled a touch lower
                // for the lawn's cosier geometry.
                duel_eye: Vec3::new(0.0, 1.3, -1.25),
                duel_target: Vec3::new(0.0, 0.3, 4.0),
                // Same reasoning as Standard's `behind_pitcher_eye`, scaled
                // to the shorter lawn pitch distance: 3 m behind the rubber
                // clears the pitcher's own rig, and puts the sole umpire
                // (z=-2.2) well outside the near-eye occlusion cone.
                behind_pitcher_eye: Vec3::new(0.0, 2.0, 10.0 + 3.0),
                behind_pitcher_target: Vec3::new(0.3, 0.9, 0.0),
                // Same reasoning as Standard's `batting_zoom_eye`: behind and
                // beside the batter's box, close enough behind the plate
                // that the lone plate umpire (no catcher on the lawn) sits
                // in the sightline and gets auto-hidden.
                batting_zoom_eye: Vec3::new(0.8, 1.6, -2.6),
                batting_zoom_target: Vec3::new(0.1, 0.9, 8.0),
                scenery: Scenery::FrontYard,
            },
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::field::BASE_DISTANCE;

    #[test]
    fn pace_defaults_match_legacy_constants() {
        let p = PaceTuning::default();
        assert_eq!(p.pitch_speed_scale, 1.0);
        assert_eq!(p.runner_speed, 7.5);
        assert_eq!(p.fielder_speed, 7.0);
        assert_eq!(p.reaction_secs, 0.35);
        assert_eq!(p.throw_speed, 27.0);
        assert_eq!(p.throw_transfer_secs, 0.5);
        assert_eq!(p.relay_transfer_secs, 0.3);
        assert_eq!(p.hit_and_run_jump_secs, 1.6);
        assert_eq!(p.stretch_grace_secs, 0.9);
        assert_eq!(p.runner_margin_secs, 0.35);
        assert_eq!(p.result_secs, 1.2);
        assert_eq!(p.pickoff_cooldown_secs, 0.9);
        assert_eq!(p.auto_throw_delay_secs, 0.6);
    }

    #[test]
    fn standard_matches_regulation_baseball() {
        let (r, f) = (VariantId::Standard.rules(), VariantId::Standard.field());
        assert_eq!(
            (
                r.counts.balls_per_walk,
                r.counts.strikes_per_out,
                r.counts.outs_per_half,
                r.counts.innings
            ),
            (4, 3, 3, 9)
        );
        assert!(!r.counts.peg_outs);
        assert_eq!(f.base_count(), 3);
        assert_eq!(f.pitch_distance, 18.44);
        assert_eq!(f.scenery, Scenery::Stadium);
        // First base is 90 ft (27.43 m) from home, and every base path is 90 ft.
        assert!((f.base_positions[0].length() - BASE_DISTANCE).abs() < 0.01);
        for pair in f.base_positions.windows(2) {
            assert!(((pair[1] - pair[0]).length() - BASE_DISTANCE).abs() < 0.01);
        }
        // Second base straight out along +Z at the full diamond diagonal
        // (127 ft 3 3/8 in ≈ 38.79 m).
        assert!((f.base_positions[1] - Vec3::new(0.0, 0.0, 38.79)).length() < 0.01);
        // Screen convention: the behind-home camera renders −X on screen
        // right, so first base is at −X and third at +X.
        assert!(f.base_positions[0].x < 0.0 && f.base_positions[2].x > 0.0);
    }

    #[test]
    fn front_yard_is_four_bases_with_pegging() {
        let (r, f) = (VariantId::FrontYard.rules(), VariantId::FrontYard.field());
        assert!(r.counts.peg_outs);
        assert_eq!(r.counts.innings, 3);
        assert_eq!(f.base_count(), 4);
        assert_eq!(f.fielder_positions.len(), 3); // + the pitcher = 4-player team
        assert!(f.peg_radius > 0.0);
        assert_eq!(f.scenery, Scenery::FrontYard);
    }

    #[test]
    fn innings_options_cycle_and_wrap() {
        assert_eq!(next_innings(1), 3);
        assert_eq!(next_innings(3), 6);
        assert_eq!(next_innings(6), 9);
        assert_eq!(next_innings(9), 1);
    }

    #[test]
    fn unknown_innings_value_restarts_the_cycle() {
        assert_eq!(next_innings(2), 1);
    }

    #[test]
    fn variant_cycle_visits_all_and_wraps() {
        assert_eq!(VariantId::Standard.next(), VariantId::FrontYard);
        assert_eq!(VariantId::FrontYard.next(), VariantId::Standard);
    }

    #[test]
    fn duel_framing_sits_behind_home_looking_out() {
        for id in [VariantId::Standard, VariantId::FrontYard] {
            let f = id.field();
            assert!(f.duel_eye.z < 0.0 && f.duel_target.z > 0.0);
            assert!(
                f.duel_eye.z > f.broadcast_eye.z,
                "duel eye must be closer to the plate than the wide framing"
            );
            // Catcher's-eye height: the rig crouches to about 1.44 m (see
            // the comment on `duel_eye` above), well below both a standing
            // eye line and the old high broadcast-style duel camera
            // (y=2.3/2.2) — this guards against a regression back to that.
            assert!(
                f.duel_eye.y > 0.9 && f.duel_eye.y < 1.6,
                "duel eye should sit at crouched-catcher eye height, not a standing/overhead one"
            );
        }
    }

    #[test]
    fn behind_pitcher_framing_looks_back_at_the_plate_from_the_mound() {
        for id in [VariantId::Standard, VariantId::FrontYard] {
            let f = id.field();
            // The eye sits out past the rubber, looking back down the pipe
            // toward home — the mirror image of the duel/batting views.
            assert!(
                f.behind_pitcher_eye.z > f.pitch_distance,
                "behind-pitcher eye must stand behind the rubber, not in front of it"
            );
            assert!(
                f.behind_pitcher_target.z <= 0.0,
                "behind-pitcher target must look toward (or at) the plate"
            );
            assert!(f.behind_pitcher_eye.z > f.behind_pitcher_target.z);
        }
    }

    #[test]
    fn diff_literal_is_empty_at_defaults() {
        assert_eq!(
            VariantId::Standard
                .rules()
                .diff_literal(VariantId::Standard),
            ""
        );
    }

    #[test]
    fn diff_literal_lists_only_changed_fields() {
        let mut r = VariantId::Standard.rules();
        r.batting.perfect_ms = 48.0;
        r.pace.runner_speed = 8.0;
        let s = r.diff_literal(VariantId::Standard);
        assert!(s.contains("batting.perfect_ms: 48.0,"));
        assert!(s.contains("pace.runner_speed: 8.0,"));
        assert!(!s.contains("solid_ms"));
        assert!(s.starts_with("// VariantId::Standard overrides:"));
    }

    /// `diff_literal`'s `diff!` field list is hand-maintained and can
    /// silently miss a field added to `Ruleset` (or a sub-struct) in the
    /// future. Guard it with reflection instead of a second hand-maintained
    /// list: flip every leaf field `Ruleset` reflects away from its default,
    /// and require `diff_literal` to emit exactly that many lines. A field
    /// missing a `diff!` arm shows up as a line-count mismatch here.
    #[test]
    fn diff_literal_covers_every_reflected_field() {
        use bevy::reflect::{PartialReflect, ReflectMut, ReflectRef};

        fn count_leaf_fields(value: &dyn PartialReflect) -> usize {
            match value.reflect_ref() {
                ReflectRef::Struct(s) => (0..s.field_len())
                    .map(|i| count_leaf_fields(s.field_at(i).unwrap()))
                    .sum(),
                _ => 1,
            }
        }

        fn perturb_every_field(value: &mut dyn PartialReflect) {
            match value.reflect_mut() {
                ReflectMut::Struct(s) => {
                    for i in 0..s.field_len() {
                        perturb_every_field(s.field_at_mut(i).unwrap());
                    }
                }
                _ => {
                    if let Some(v) = value.try_downcast_mut::<f32>() {
                        *v += 1.0;
                    } else if let Some(v) = value.try_downcast_mut::<u32>() {
                        *v += 1;
                    } else if let Some(v) = value.try_downcast_mut::<bool>() {
                        *v = !*v;
                    } else {
                        panic!(
                            "diff_literal completeness test: unhandled leaf field type on \
                             Ruleset; add a case to perturb_every_field (and a matching \
                             diff! arm in diff_literal)"
                        );
                    }
                }
            }
        }

        let expected = count_leaf_fields(VariantId::Standard.rules().as_partial_reflect());

        let mut all_changed = VariantId::Standard.rules();
        perturb_every_field(all_changed.as_partial_reflect_mut());

        let diff = all_changed.diff_literal(VariantId::Standard);
        let emitted = diff.lines().filter(|l| !l.starts_with("//")).count();

        assert_eq!(
            emitted, expected,
            "diff_literal emitted {emitted} line(s) but Ruleset reflects {expected} leaf \
             field(s) — a field is missing a diff! arm in diff_literal"
        );
    }

    #[test]
    fn batting_zoom_framing_sits_behind_home_looking_toward_the_pitcher() {
        for id in [VariantId::Standard, VariantId::FrontYard] {
            let f = id.field();
            // Same plate-corridor orientation as the duel view: eye behind
            // home (z<0), target out toward the mound (z>0).
            assert!(f.batting_zoom_eye.z < 0.0 && f.batting_zoom_target.z > 0.0);
            // "Beside" the batter's box, not dead centre like the duel/pitcher
            // views — this is what makes it a distinct framing.
            assert!(f.batting_zoom_eye.x.abs() > 0.1);
        }
    }
}
