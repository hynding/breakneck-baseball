//! Unit tests for [`super`] — the look pass.

use super::*;
use crate::game::quality::QualityTier;

const DAY: Color = Color::srgb(0.48, 0.67, 0.88);
const NIGHT: Color = Color::srgb(0.02, 0.03, 0.08);

#[test]
fn day_and_night_skies_are_told_apart() {
    assert!(!is_night(DAY));
    assert!(is_night(NIGHT));
}

#[test]
fn fog_never_reaches_the_field() {
    // The wall is ~130 m out; on the worst day fog must start past it.
    for o in [0.0, 0.5, 1.0] {
        let look = CameraLook::for_sky(DAY, o, &QualityTier::Low.budget());
        let (_, start, end) = look.fog.expect("fog is on at Low");
        assert!(start > 105.0 && end > start, "overcast {o}: {start}..{end}");
    }
}

#[test]
fn overcast_pulls_the_fog_in() {
    let budget = QualityTier::Medium.budget();
    let clear = CameraLook::for_sky(DAY, 0.0, &budget).fog.unwrap();
    let grey = CameraLook::for_sky(DAY, 1.0, &budget).fog.unwrap();
    assert!(grey.1 < clear.1 && grey.2 < clear.2);
}

#[test]
fn bloom_only_on_high() {
    for (tier, want) in [
        (QualityTier::Low, false),
        (QualityTier::Medium, false),
        (QualityTier::High, true),
    ] {
        assert_eq!(
            CameraLook::for_sky(DAY, 0.0, &tier.budget()).bloom,
            want,
            "{tier:?}"
        );
    }
}

#[test]
fn the_sky_hazes_toward_the_horizon() {
    assert_eq!(haze_at(1.0), 0.0, "zenith");
    assert!((haze_at(0.0) - 1.0).abs() < 1e-5, "horizon");
    assert_eq!(haze_at(-0.5), 1.0, "below the horizon");
    assert!(haze_at(0.7) < haze_at(0.3), "monotone toward the horizon");
}

#[test]
fn the_sky_cubemap_is_a_cube() {
    let image = sky_cubemap(DAY);
    assert_eq!(image.texture_descriptor.size.depth_or_array_layers, 6);
    let view = image.texture_view_descriptor.as_ref().unwrap();
    assert_eq!(
        view.dimension,
        Some(bevy::render::render_resource::TextureViewDimension::Cube)
    );
    // The +Y face's centre looks straight up; the whole -Y face is haze.
    let face = (SKY_FACE * SKY_FACE * 4) as usize;
    let data = image.data.as_ref().unwrap();
    let centre = 2 * face + ((SKY_FACE / 2 * SKY_FACE + SKY_FACE / 2) * 4) as usize;
    let up = &data[centre..centre + 3];
    let want = zenith(DAY).to_srgba().to_u8_array();
    assert!(
        up.iter().zip(want).all(|(a, b)| a.abs_diff(b) <= 2),
        "{up:?} vs {want:?}"
    );
    assert_eq!(
        &data[3 * face..3 * face + 4],
        &horizon_haze(DAY).to_srgba().to_u8_array()
    );
}

#[test]
fn sky_from_medium_up() {
    for (tier, want) in [
        (QualityTier::Low, false),
        (QualityTier::Medium, true),
        (QualityTier::High, true),
    ] {
        assert_eq!(
            CameraLook::for_sky(DAY, 0.0, &tier.budget()).sky,
            want,
            "{tier:?}"
        );
    }
}

#[test]
fn haze_is_lighter_than_the_zenith_by_day() {
    assert!(luminance(horizon_haze(DAY)) > luminance(zenith(DAY)));
}
