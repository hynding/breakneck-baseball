//! Game variants — the data that makes a game of "baseball" *this* game.
//!
//! A [`Ruleset`] holds every countable threshold the rules engine reads, and a
//! [`FieldSpec`] holds the geometry, personnel, and presentation of the park.
//! Both are plain data, inserted as resources when a game starts, so adding a
//! new variant means adding a new definition here — not new systems. Home
//! plate is at the world origin with +Z toward the field in every variant;
//! all plate-local logic (pitching, swinging, cameras) is shared.

use bevy::math::Vec3;
use bevy::prelude::Resource;

use crate::game::field::{HALF_DIAGONAL, PITCH_DISTANCE};

/// Countable-rule knobs read by the rules engine and game flow.
#[derive(Resource, Clone, Debug)]
pub struct Ruleset {
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
#[derive(Resource, Clone, Debug)]
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
    /// Ball-reset radius: past this the ball is considered lost.
    pub bounds: f32,
    /// Broadcast-camera eye position for this park's size (wide framing,
    /// used while the ball is in play).
    pub broadcast_eye: Vec3,
    /// Broadcast-camera resting look-at point.
    pub broadcast_target: Vec3,
    /// Tight at-bat framing used during the pitch/swing duel.
    pub duel_eye: Vec3,
    pub duel_target: Vec3,
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
                balls_per_walk: 4,
                strikes_per_out: 3,
                outs_per_half: 3,
                innings: 9,
                peg_outs: false,
            },
            // Kid's rules: short games, outs by pegging the runner.
            VariantId::FrontYard => Ruleset {
                balls_per_walk: 4,
                strikes_per_out: 3,
                outs_per_half: 3,
                innings: 3,
                peg_outs: true,
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
                bounds: 220.0,
                broadcast_eye: Vec3::new(0.0, 13.0, -21.0),
                broadcast_target: Vec3::new(0.0, 1.2, 9.0),
                // Catcher's point of view: centred just behind and above the
                // catcher, looking out at the pitcher — batter and zone at
                // the bottom of frame, the delivery coming straight in.
                duel_eye: Vec3::new(0.0, 3.7, -5.9),
                duel_target: Vec3::new(0.0, 0.55, 14.8),
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
                bounds: 90.0,
                broadcast_eye: Vec3::new(0.0, 7.0, -12.0),
                broadcast_target: Vec3::new(0.0, 1.0, 5.0),
                // Same catcher's-eye framing, scaled to the short lawn duel.
                duel_eye: Vec3::new(0.0, 3.3, -5.0),
                duel_target: Vec3::new(0.0, 0.55, 8.0),
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
    fn standard_matches_regulation_baseball() {
        let (r, f) = (VariantId::Standard.rules(), VariantId::Standard.field());
        assert_eq!(
            (
                r.balls_per_walk,
                r.strikes_per_out,
                r.outs_per_half,
                r.innings
            ),
            (4, 3, 3, 9)
        );
        assert!(!r.peg_outs);
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
        assert!(r.peg_outs);
        assert_eq!(r.innings, 3);
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
        }
    }
}
