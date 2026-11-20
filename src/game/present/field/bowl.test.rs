//! Unit tests for [`super`] — the stadium bowl.

use super::*;
use crate::game::quality::QualityTier;
use crate::game::variant::FieldSpec;

fn stadium() -> FieldSpec {
    crate::game::variant::VariantId::Standard.field()
}

#[test]
fn the_front_row_sits_behind_the_wall_and_off_the_foul_lines() {
    let field = stadium();
    assert_eq!(field.scenery, Scenery::Stadium);
    // Fair territory: just past the home-run fence.
    for deg in [-40.0_f32, -20.0, 0.0, 20.0, 40.0] {
        let t = deg.to_radians();
        let fence = rules::fence_at(dir(t) * 10.0, &field);
        assert!(
            (inner_radius(t, &field) - fence - WALL_GAP).abs() < 1e-3,
            "{deg}°"
        );
    }
    // Foul territory: every front-row point is FOUL_GAP or more from both
    // foul lines (the lines run along ±45° from home).
    for deg in (50..=310).step_by(5) {
        let t = (deg as f32).to_radians();
        let p = dir(t) * inner_radius(t, &field);
        for line in [field.fair_half_angle, -field.fair_half_angle] {
            let along = dir(line);
            let off = (p - along * p.dot(along)).length();
            let behind = p.dot(along) < 0.0;
            assert!(
                off >= FOUL_GAP - 0.01 || behind || p.dot(along) > field.fence_line,
                "{deg}°: {off} m from the line"
            );
        }
    }
    // Behind the plate the stands stay clear of the broadcast camera
    // (eye at z ≈ -12).
    assert!(inner_radius(PI, &field) > 20.0);
}

#[test]
fn the_bowl_rises_away_from_the_field() {
    let field = stadium();
    for deg in [0.0_f32, 60.0, 180.0, 300.0] {
        let t = deg.to_radians().max(EYE_HALF);
        let front = seat_point(t, 0.0, &field);
        let back = seat_point(t, BOWL_DEPTH, &field);
        assert!(back.y > front.y + 15.0);
        assert!(back.with_y(0.0).length() > front.with_y(0.0).length());
    }
}

#[test]
fn every_tier_fits_in_the_bowl_and_fills_low_rows_first() {
    let all = seats(&stadium());
    for tier in [QualityTier::Low, QualityTier::Medium, QualityTier::High] {
        let want = tier.budget().crowd_instances as usize;
        assert!(all.len() > want, "{tier:?}: {} seats for {want}", all.len());
    }
    let few = fill(&all, 1500);
    assert_eq!(few.len(), 1500);
    let mean_row = few.iter().map(|s| s.row as f32).sum::<f32>() / few.len() as f32;
    assert!(mean_row < ROWS as f32 * 0.4, "mean row {mean_row}");
    // Scattered round the whole bowl, not bunched in one sector.
    let mut per_sector = [0usize; SECTORS];
    for s in &few {
        per_sector[sector_of(s.theta)] += 1;
    }
    assert!(per_sector.iter().all(|&n| n > 0), "{per_sector:?}");
}

#[test]
fn no_seats_in_the_batters_eye() {
    for s in seats(&stadium()) {
        assert!(wrap(s.theta).abs() >= EYE_HALF - 1e-3, "{}", s.theta);
    }
}

#[test]
fn quads_face_the_way_they_are_told() {
    let mut m = QuadMesh::default();
    let corners = [Vec3::ZERO, Vec3::X, Vec3::X + Vec3::Y, Vec3::Y];
    m.quad(corners, [[0.0; 2]; 4], Vec3::Z, WHITE);
    m.quad(corners, [[0.0; 2]; 4], -Vec3::Z, WHITE);
    for q in 0..2 {
        let tri = &m.index[q * 6..q * 6 + 3];
        let p = |i: u32| Vec3::from(m.pos[i as usize]);
        let n = (p(tri[1]) - p(tri[0])).cross(p(tri[2]) - p(tri[0]));
        let want = if q == 0 { Vec3::Z } else { -Vec3::Z };
        // Counter-clockwise from the front: the winding normal faces out.
        assert!(n.dot(want) > 0.0, "quad {q}");
        assert!(Vec3::from(m.normal[q * 4]).dot(want) > 0.99);
    }
}

#[test]
fn the_scoreboard_shows_the_score() {
    let a = scoreboard_image(&ScoreBoard::default());
    let b = scoreboard_image(&ScoreBoard {
        home_runs: 3,
        ..default()
    });
    assert_ne!(a.data, b.data);
}
