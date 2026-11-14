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
        stepped: false,
    };
    assert_eq!(auto.describe(2.0), "Auto: Medium, 2x AA, 75% res");
    let low = ActiveQuality {
        setting: GraphicsQuality::Low,
        tier: QualityTier::Low,
        stepped: false,
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
            stepped: false,
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

// ── Adaptive fallback ────────────────────────────────────────────────────────

/// Feeds `secs` of frames at `dt` each; returns how many step-downs fired.
fn run_frames(budget: &mut FrameBudget, tier: &mut QualityTier, dt: f32, secs: f32) -> u32 {
    let mut fired = 0;
    for _ in 0..(secs / dt).round() as u32 {
        if budget.observe(dt, *tier) {
            *tier = tier.step_down();
            fired += 1;
        }
    }
    fired
}

#[test]
fn a_smooth_run_never_steps_down() {
    let mut budget = FrameBudget::new();
    let mut tier = QualityTier::High;
    // 60 fps and 45 fps are both fine.
    assert_eq!(run_frames(&mut budget, &mut tier, 1.0 / 60.0, 60.0), 0);
    assert_eq!(run_frames(&mut budget, &mut tier, 1.0 / 45.0, 60.0), 0);
    assert_eq!(tier, QualityTier::High);
    assert_eq!(budget.steps, 0);
}

#[test]
fn sustained_slow_frames_step_down_one_tier_at_a_time_to_low() {
    let mut budget = FrameBudget::new();
    let mut tier = QualityTier::High;
    let dt = 1.0 / 25.0;
    // Settle, then two slow windows: the first step lands after ~14 s.
    let first = SETTLE_SECS + FRAME_WINDOW_SECS * SLOW_WINDOWS_TO_STEP as f32 + 0.5;
    assert_eq!(run_frames(&mut budget, &mut tier, dt, first), 1);
    assert_eq!(tier, QualityTier::Medium);
    // It settles again before judging Medium, then steps to Low...
    assert_eq!(run_frames(&mut budget, &mut tier, dt, first), 1);
    assert_eq!(tier, QualityTier::Low);
    // ...and Low is the floor.
    assert_eq!(run_frames(&mut budget, &mut tier, dt, 120.0), 0);
    assert_eq!(budget.steps, 2);
}

#[test]
fn boot_stutter_and_hitches_do_not_count() {
    let mut budget = FrameBudget::new();
    let mut tier = QualityTier::Medium;
    // Shader compiles at boot: awful frames, but inside the settle time.
    assert_eq!(
        run_frames(&mut budget, &mut tier, 0.1, SETTLE_SECS - 0.5),
        0
    );
    // A smooth run with a 2-second tab-switch hitch every few seconds.
    for _ in 0..10 {
        assert!(!budget.observe(2.0, tier));
        assert_eq!(run_frames(&mut budget, &mut tier, 1.0 / 60.0, 3.0), 0);
    }
    assert_eq!(tier, QualityTier::Medium);
}

/// A device so slow every frame is a "hitch" still steps down: the first
/// headless SwiftShader run sat at 2.5 fps on Medium and never fell back
/// while hitches were dropped from the mean instead of capped.
#[test]
fn a_crawling_device_steps_down_too() {
    let mut budget = FrameBudget::new();
    let mut tier = QualityTier::Medium;
    run_frames(
        &mut budget,
        &mut tier,
        0.4,
        SETTLE_SECS + 3.0 * FRAME_WINDOW_SECS,
    );
    assert_eq!(tier, QualityTier::Low);
}

#[test]
fn one_slow_window_alone_is_forgiven() {
    let mut budget = FrameBudget::new();
    let mut tier = QualityTier::High;
    run_frames(&mut budget, &mut tier, 1.0 / 60.0, SETTLE_SECS + 0.1);
    // Slow, fast, slow, fast: never two in a row.
    for _ in 0..6 {
        run_frames(&mut budget, &mut tier, 1.0 / 25.0, FRAME_WINDOW_SECS);
        run_frames(&mut budget, &mut tier, 1.0 / 60.0, FRAME_WINDOW_SECS);
    }
    assert_eq!(tier, QualityTier::High);
}

#[test]
fn stepped_auto_resolves_below_the_platform_pick_and_fixed_settings_ignore_steps() {
    let desktop = Platform::Desktop(Some(GpuClass::Discrete));
    assert_eq!(
        resolve_stepped(GraphicsQuality::Auto, desktop, 1),
        QualityTier::Medium
    );
    assert_eq!(
        resolve_stepped(GraphicsQuality::Auto, desktop, 5),
        QualityTier::Low
    );
    assert_eq!(
        resolve_stepped(GraphicsQuality::High, desktop, 2),
        QualityTier::High
    );
    let stepped = ActiveQuality {
        setting: GraphicsQuality::Auto,
        tier: QualityTier::Low,
        stepped: true,
    };
    assert_eq!(
        stepped.describe(1.0),
        "Auto: Low (slow GPU), no AA, 75% res"
    );
}

/// The step survives an edit to another settings row, and clears when the
/// player picks a tier.
#[test]
fn a_new_graphics_choice_clears_the_step() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(Settings::default())
        .insert_resource(Platform::Desktop(Some(GpuClass::Discrete)))
        .insert_resource(FrameBudget {
            steps: 1,
            ..FrameBudget::new()
        })
        .insert_resource(ActiveQuality {
            setting: GraphicsQuality::Auto,
            tier: QualityTier::Medium,
            stepped: true,
        })
        .add_systems(Update, resolve_quality);
    app.update();
    app.world_mut().resource_mut::<Settings>().volume = 0.3;
    app.update();
    assert_eq!(
        app.world().resource::<ActiveQuality>().tier,
        QualityTier::Medium,
        "a volume edit keeps the stepped tier"
    );
    app.world_mut().resource_mut::<Settings>().graphics = GraphicsQuality::High;
    app.update();
    app.world_mut().resource_mut::<Settings>().graphics = GraphicsQuality::Auto;
    app.update();
    let active = *app.world().resource::<ActiveQuality>();
    assert_eq!(active.tier, QualityTier::High);
    assert!(!active.stepped);
    assert_eq!(app.world().resource::<FrameBudget>().steps, 0);
}
