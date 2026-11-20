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
    // The wall is ~130 m from the plate and the camera sits behind it; on
    // the worst day fog must still start past the wall.
    for o in [0.0, 0.5, 1.0] {
        let look = CameraLook::for_sky(DAY, o, &QualityTier::Low.budget());
        let (_, start, end) = look.fog.expect("fog is on at Low");
        assert!(start > 130.0 && end > start, "overcast {o}: {start}..{end}");
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
    let down = &data[3 * face..3 * face + 3];
    let haze = horizon_haze(DAY).to_srgba().to_u8_array();
    assert!(
        down.iter().zip(haze).all(|(a, b)| a.abs_diff(b) <= 1),
        "{down:?} vs {haze:?}"
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

#[test]
fn low_fogs_toward_the_flat_sky() {
    let (low, _, _) = CameraLook::for_sky(DAY, 0.0, &QualityTier::Low.budget())
        .fog
        .unwrap();
    let (med, _, _) = CameraLook::for_sky(DAY, 0.0, &QualityTier::Medium.budget())
        .fog
        .unwrap();
    assert_eq!(low, DAY);
    assert_eq!(med, horizon_haze(DAY));
}

/// The system path Auto takes when it steps down: the camera gains and
/// sheds the sky, fog and bloom as the tier changes.
#[test]
fn the_camera_follows_the_tier() {
    use crate::game::settings::GraphicsQuality;
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(ClearColor(DAY))
        .insert_resource(SkyTexture(Handle::default()))
        .add_systems(Update, apply_camera_look);
    let camera = app
        .world_mut()
        .spawn((Camera3d::default(), MainCamera))
        .id();
    for (tier, sky, bloom) in [
        (QualityTier::High, true, true),
        (QualityTier::Medium, true, false),
        (QualityTier::Low, false, false),
        (QualityTier::High, true, true),
    ] {
        app.insert_resource(ActiveQuality {
            setting: GraphicsQuality::Auto,
            tier,
            stepped: false,
        });
        app.update();
        let e = app.world().entity(camera);
        assert_eq!(e.contains::<Skybox>(), sky, "{tier:?} sky");
        assert_eq!(e.contains::<Bloom>(), bloom, "{tier:?} bloom");
        assert_eq!(e.contains::<Hdr>(), bloom, "{tier:?} hdr");
        assert!(e.contains::<DistanceFog>(), "{tier:?} fog");
        assert!(e.contains::<ColorGrading>(), "{tier:?} grading");
    }
}
