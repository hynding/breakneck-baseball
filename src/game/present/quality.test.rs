//! Unit tests for [`super`] — graphics quality resolution and application.

use super::*;

#[test]
fn auto_picks_by_platform() {
    use GpuClass::*;
    let cases = [
        (Platform::Desktop(Some(Discrete)), QualityTier::High),
        (Platform::Desktop(None), QualityTier::High),
        (Platform::Desktop(Some(Shared)), QualityTier::Medium),
        (Platform::Desktop(Some(Software)), QualityTier::Low),
        (Platform::WebDesktop, QualityTier::Medium),
        (Platform::Handheld, QualityTier::Low),
    ];
    for (platform, want) in cases {
        assert_eq!(
            resolve(GraphicsQuality::Auto, platform),
            want,
            "{platform:?}"
        );
    }
}

#[test]
fn gpu_class_from_adapter_device_type() {
    use wgpu_types::DeviceType;
    assert_eq!(
        GpuClass::from_device_type(DeviceType::DiscreteGpu),
        GpuClass::Discrete
    );
    assert_eq!(
        GpuClass::from_device_type(DeviceType::IntegratedGpu),
        GpuClass::Shared
    );
    assert_eq!(
        GpuClass::from_device_type(DeviceType::VirtualGpu),
        GpuClass::Shared
    );
    assert_eq!(
        GpuClass::from_device_type(DeviceType::Other),
        GpuClass::Shared
    );
    assert_eq!(
        GpuClass::from_device_type(DeviceType::Cpu),
        GpuClass::Software
    );
}

#[test]
fn fixed_settings_ignore_the_platform() {
    for platform in [
        Platform::Handheld,
        Platform::Desktop(Some(GpuClass::Discrete)),
    ] {
        assert_eq!(resolve(GraphicsQuality::Low, platform), QualityTier::Low);
        assert_eq!(
            resolve(GraphicsQuality::Medium, platform),
            QualityTier::Medium
        );
        assert_eq!(resolve(GraphicsQuality::High, platform), QualityTier::High);
    }
}

/// The web build's old hardcoded budget (2x MSAA, 1024 shadows) is exactly
/// Medium, so Auto on a desktop browser keeps today's look; native's old
/// budget (4x, 2048) is High.
#[test]
fn tiers_keep_the_previous_per_target_budgets() {
    assert_eq!(QualityTier::Medium.msaa(), Msaa::Sample2);
    assert_eq!(QualityTier::Medium.shadow_map_size(), 1024);
    assert_eq!(QualityTier::High.msaa(), Msaa::Sample4);
    assert_eq!(QualityTier::High.shadow_map_size(), 2048);
    assert_eq!(QualityTier::Low.msaa(), Msaa::Off);
    assert!(QualityTier::Low.shadow_map_size() < 1024);
}

#[test]
fn render_scale_caps_density_per_tier() {
    let close = |a: f32, b: f32| (a - b).abs() < 1e-4;
    // High never scales.
    for sf in [1.0, 2.0, 3.0] {
        assert!(close(QualityTier::High.render_scale(sf), 1.0));
    }
    // Medium: full res on a 1x screen, 1.5 px per logical px above it.
    assert!(close(QualityTier::Medium.render_scale(1.0), 1.0));
    assert!(close(QualityTier::Medium.render_scale(2.0), 0.75));
    assert!(close(QualityTier::Medium.render_scale(3.0), 0.5));
    // Low: 75% on a 1x screen, 1 px per logical px on dense ones.
    assert!(close(QualityTier::Low.render_scale(1.0), 0.75));
    assert!(close(QualityTier::Low.render_scale(2.0), 0.5));
    assert!(close(QualityTier::Low.render_scale(3.0), 1.0 / 3.0));
    // Absurd densities hit the floor; a bogus zero factor can't divide by 0.
    assert!(close(QualityTier::Low.render_scale(10.0), MIN_RENDER_SCALE));
    assert!(close(QualityTier::Low.render_scale(0.0), 0.75));
}

#[test]
fn describe_names_the_choice_and_its_effect() {
    let auto = ActiveQuality {
        setting: GraphicsQuality::Auto,
        tier: QualityTier::Medium,
    };
    assert_eq!(auto.describe(2.0), "Auto: Medium, 2x AA, 75% res");
    let low = ActiveQuality {
        setting: GraphicsQuality::Low,
        tier: QualityTier::Low,
    };
    assert_eq!(low.describe(1.0), "Low, no AA, 75% res");
}

/// A settings edit re-resolves the tier and lands on the camera's MSAA and
/// the shadow-map resource the same frame.
#[test]
fn settings_change_applies_msaa_and_shadow_map() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(Settings::default())
        .insert_resource(Platform::Desktop(Some(GpuClass::Discrete)))
        .insert_resource(ActiveQuality {
            setting: GraphicsQuality::Auto,
            tier: QualityTier::High,
        })
        .insert_resource(DirectionalLightShadowMap { size: 2048 })
        .add_systems(
            Update,
            (resolve_quality, (apply_msaa, apply_shadow_map)).chain(),
        );
    let camera = app.world_mut().spawn((Camera3d::default(), Msaa::Off)).id();
    app.update();
    assert_eq!(*app.world().get::<Msaa>(camera).unwrap(), Msaa::Sample4);

    app.world_mut().resource_mut::<Settings>().graphics = GraphicsQuality::Low;
    app.update();
    assert_eq!(
        app.world().resource::<ActiveQuality>().tier,
        QualityTier::Low
    );
    assert_eq!(*app.world().get::<Msaa>(camera).unwrap(), Msaa::Off);
    assert_eq!(
        app.world().resource::<DirectionalLightShadowMap>().size,
        512
    );
}
