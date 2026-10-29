//! Graphics quality: resolves the player's [`GraphicsQuality`] setting to a
//! concrete [`QualityTier`] for this device and applies it — MSAA on the 3D
//! cameras, the sun's shadow-map size, and a render scale for the 3D view.
//!
//! **Render scale.** The main camera renders into an offscreen image sized
//! `physical window × scale`, and a 2D camera stretches that image over the
//! window (linear filtering). UI renders on the 2D camera at full
//! resolution, so text stays sharp while the 3D view gets cheaper. The
//! route is the same at 100% (the image is then window-sized): an earlier
//! cut drew straight to the window at 100% and switched routes on a
//! settings change, which moves the UI between cameras — on wasm/WebGL2
//! the whole UI then went blank in Chromium, the same family as the wasm
//! UI rule in CLAUDE.md. One full-screen blit is the price of one route.
//! (A window `scale_factor_override` can't do this: on native the surface
//! is the window's physical size regardless, and on the web winit sizes
//! the canvas backing store from `devicePixelRatio` itself.)
//!
//! **Auto.** Native desktops pick by GPU class (discrete → High,
//! integrated/virtual → Medium, software → Low); a desktop browser gets
//! Medium (the MSAA 2x / 1024 shadow budget the web build always used);
//! phones and tablets — a coarse primary pointer on the web, or a native
//! mobile target — get Low.

use bevy::camera::RenderTarget;
use bevy::camera::visibility::RenderLayers;
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::image::ImageSampler;
use bevy::light::DirectionalLightShadowMap;
use bevy::picking::Pickable;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureFormat};
use bevy::window::PrimaryWindow;

use crate::game::camera::MainCamera;
use crate::game::settings::{GraphicsQuality, Settings};

/// A concrete quality level — what [`GraphicsQuality`] resolves to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QualityTier {
    Low,
    Medium,
    High,
}

/// Floor for [`QualityTier::render_scale`], so a very dense screen can't
/// shrink the 3D view to a smear.
pub const MIN_RENDER_SCALE: f32 = 0.25;

impl QualityTier {
    pub fn label(self) -> &'static str {
        match self {
            QualityTier::Low => "Low",
            QualityTier::Medium => "Medium",
            QualityTier::High => "High",
        }
    }

    /// Anti-aliasing for the 3D cameras. 4x holds on native GPUs; on
    /// WebGL2 the MSAA resolve is a measurable per-frame cost, which is why
    /// the web's Auto lands on Medium's 2x.
    pub fn msaa(self) -> Msaa {
        match self {
            QualityTier::Low => Msaa::Off,
            QualityTier::Medium => Msaa::Sample2,
            QualityTier::High => Msaa::Sample4,
        }
    }

    /// Texels per side of the sun's shadow map (Bevy's default is 2048).
    /// The blocky rigs' soft field shadows read fine at 1024; 512 is the
    /// fill-rate floor for weak mobile GPUs.
    pub fn shadow_map_size(self) -> usize {
        match self {
            QualityTier::Low => 512,
            QualityTier::Medium => 1024,
            QualityTier::High => 2048,
        }
    }

    /// Fraction of the window's physical pixels the 3D view renders at, for
    /// a window with `scale_factor` physical pixels per logical pixel.
    ///
    /// Each tier caps *density* (render pixels per logical pixel) rather
    /// than taking a flat fraction, so a 1x desktop monitor keeps full
    /// resolution at Medium while a 3x phone — where full resolution is ~9x
    /// the pixels of its logical size — is scaled hard. Low additionally
    /// takes a flat 75% so it still saves fill rate on a 1x screen.
    pub fn render_scale(self, scale_factor: f32) -> f32 {
        let (max_fraction, density_cap) = match self {
            QualityTier::Low => (0.75, 1.0),
            QualityTier::Medium => (1.0, 1.5),
            QualityTier::High => (1.0, f32::INFINITY),
        };
        (density_cap / scale_factor.max(f32::EPSILON))
            .min(max_fraction)
            .clamp(MIN_RENDER_SCALE, 1.0)
    }
}

/// The GPU's class, from the render adapter's device type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuClass {
    Discrete,
    /// Integrated, virtual/hosted, or unreported — the cautious middle.
    Shared,
    /// A software rasterizer (e.g. llvmpipe, WARP).
    Software,
}

impl GpuClass {
    pub fn from_device_type(device_type: wgpu_types::DeviceType) -> Self {
        use wgpu_types::DeviceType;
        match device_type {
            DeviceType::DiscreteGpu => GpuClass::Discrete,
            DeviceType::Cpu => GpuClass::Software,
            DeviceType::IntegratedGpu | DeviceType::VirtualGpu | DeviceType::Other => {
                GpuClass::Shared
            }
        }
    }
}

/// What the Auto heuristic knows about the device.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    /// A native desktop build; `None` when no adapter info exists (the
    /// headless test harness).
    Desktop(Option<GpuClass>),
    /// A browser with a fine primary pointer (mouse/trackpad).
    WebDesktop,
    /// A phone or tablet: a coarse primary pointer in the browser, or a
    /// native mobile target.
    Handheld,
}

impl Platform {
    /// The tier [`GraphicsQuality::Auto`] picks here.
    pub fn auto_tier(self) -> QualityTier {
        match self {
            Platform::Desktop(Some(GpuClass::Discrete)) | Platform::Desktop(None) => {
                QualityTier::High
            }
            Platform::Desktop(Some(GpuClass::Shared)) | Platform::WebDesktop => QualityTier::Medium,
            Platform::Desktop(Some(GpuClass::Software)) | Platform::Handheld => QualityTier::Low,
        }
    }

    /// Probes the running device. The adapter info is in the main world by
    /// `Plugin::finish` (the render plugin inserts it once its device is up).
    fn detect(world: &World) -> Self {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = world;
            // `(pointer: coarse)` is the touch-first primary input — phones
            // and tablets, but not a touchscreen laptop with a trackpad.
            let coarse = web_sys::window()
                .and_then(|w| w.match_media("(pointer: coarse)").ok().flatten())
                .is_some_and(|q| q.matches());
            if coarse {
                Platform::Handheld
            } else {
                Platform::WebDesktop
            }
        }
        #[cfg(all(
            not(target_arch = "wasm32"),
            any(target_os = "android", target_os = "ios")
        ))]
        {
            let _ = world;
            Platform::Handheld
        }
        #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
        {
            let gpu = world
                .get_resource::<bevy::render::renderer::RenderAdapterInfo>()
                .map(|info| GpuClass::from_device_type(info.device_type));
            Platform::Desktop(gpu)
        }
    }
}

/// Resolves a setting on a platform.
pub fn resolve(setting: GraphicsQuality, platform: Platform) -> QualityTier {
    match setting {
        GraphicsQuality::Auto => platform.auto_tier(),
        GraphicsQuality::Low => QualityTier::Low,
        GraphicsQuality::Medium => QualityTier::Medium,
        GraphicsQuality::High => QualityTier::High,
    }
}

/// The tier in force, re-resolved whenever [`Settings`] changes.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActiveQuality {
    pub setting: GraphicsQuality,
    pub tier: QualityTier,
}

impl ActiveQuality {
    /// Settings-row text: the choice plus what it means on this screen,
    /// e.g. "Auto: Medium, 2x AA, 75% res".
    pub fn describe(&self, scale_factor: f32) -> String {
        let aa = match self.tier.msaa() {
            Msaa::Off => "no AA".to_string(),
            msaa => format!("{}x AA", msaa.samples()),
        };
        let res = (self.tier.render_scale(scale_factor) * 100.0).round();
        match self.setting {
            GraphicsQuality::Auto => {
                format!("Auto: {}, {aa}, {res:.0}% res", self.tier.label())
            }
            _ => format!("{}, {aa}, {res:.0}% res", self.tier.label()),
        }
    }
}

/// The offscreen image the main camera renders into.
#[derive(Resource)]
struct ScaledView(Handle<Image>);

/// The 2D camera that stretches [`ScaledView`] over the window and carries
/// the UI.
#[derive(Component)]
pub struct UpscaleCamera;

/// The full-window sprite showing [`ScaledView`].
#[derive(Component)]
struct UpscaleSprite;

/// Render layer for the upscale sprite/camera pair, so neither sees (or is
/// seen by) anything else.
const UPSCALE_LAYER: usize = 31;

pub struct QualityPlugin;

impl Plugin for QualityPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_upscale_view).add_systems(
            Update,
            (
                resolve_quality,
                (apply_msaa, apply_shadow_map, apply_render_scale),
            )
                .chain(),
        );
    }

    fn finish(&self, app: &mut App) {
        let platform = Platform::detect(app.world());
        let setting = app
            .world()
            .get_resource::<Settings>()
            .map(|s| s.graphics)
            .unwrap_or_default();
        info!(
            "graphics: platform {platform:?}, auto tier {:?}",
            platform.auto_tier()
        );
        app.insert_resource(platform)
            .insert_resource(ActiveQuality {
                setting,
                tier: resolve(setting, platform),
            });
    }
}

fn resolve_quality(
    settings: Res<Settings>,
    platform: Res<Platform>,
    mut active: ResMut<ActiveQuality>,
) {
    if settings.is_changed() {
        active.set_if_neq(ActiveQuality {
            setting: settings.graphics,
            tier: resolve(settings.graphics, *platform),
        });
    }
}

/// Every 3D camera (the main one, plus the dev Creator's preview) follows
/// the tier's MSAA. Cameras spawned since last frame pick it up here too,
/// before their first extract.
fn apply_msaa(active: Res<ActiveQuality>, mut cameras: Query<&mut Msaa, With<Camera3d>>) {
    let want = active.tier.msaa();
    for mut msaa in &mut cameras {
        msaa.set_if_neq(want);
    }
}

fn apply_shadow_map(
    active: Res<ActiveQuality>,
    shadow_map: Option<ResMut<DirectionalLightShadowMap>>,
) {
    let Some(mut shadow_map) = shadow_map else {
        return;
    };
    let want = active.tier.shadow_map_size();
    if shadow_map.size != want {
        shadow_map.size = want;
    }
}

fn spawn_upscale_view(mut commands: Commands, images: Option<ResMut<Assets<Image>>>) {
    let Some(mut images) = images else {
        return;
    };
    let mut image = Image::new_target_texture(1, 1, TextureFormat::Rgba8UnormSrgb);
    // GPU-only: a render target needs no CPU copy, and `resize` would
    // otherwise drag a zeroed buffer of the full size along every time.
    image.data = None;
    // Nothing worth keeping survives a resize (the next frame redraws it
    // all), and the copy would need COPY_SRC usage the target lacks — wgpu
    // validation panics on the first resize otherwise.
    image.copy_on_resize = false;
    image.sampler = ImageSampler::linear();
    let handle = images.add(image);
    commands.spawn((
        UpscaleCamera,
        Camera2d,
        Camera {
            // Over the dev Creator's preview camera (order 0) when that one
            // owns the window; otherwise this is the window's only camera.
            order: 1,
            // The opaque sprite covers every pixel, and while it's hidden
            // (Creator stage) the UI must draw over the camera beneath.
            clear_color: ClearColorConfig::None,
            ..default()
        },
        // Pinned rather than left to `DefaultUiCamera`'s highest-order pick,
        // so the UI's camera never changes at runtime (see the module docs).
        IsDefaultUiCamera,
        Msaa::Off,
        // The main camera already tonemapped into the image.
        Tonemapping::None,
        DebandDither::Disabled,
        RenderLayers::layer(UPSCALE_LAYER),
    ));
    commands.spawn((
        UpscaleSprite,
        Sprite {
            image: handle.clone(),
            custom_size: Some(Vec2::ONE),
            ..default()
        },
        RenderLayers::layer(UPSCALE_LAYER),
        Pickable::IGNORE,
    ));
    commands.insert_resource(ScaledView(handle));
}

/// Points the main camera at the scaled image, keeps the image sized to the
/// window and the sprite covering it. Every write is guarded so a steady
/// frame touches nothing.
#[allow(clippy::type_complexity)]
fn apply_render_scale(
    active: Res<ActiveQuality>,
    view: Option<Res<ScaledView>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut images: ResMut<Assets<Image>>,
    mut main_cameras: Query<&mut Camera, (With<MainCamera>, Without<UpscaleCamera>)>,
    mut upscale_cameras: Query<&mut Msaa, (With<UpscaleCamera>, Without<MainCamera>)>,
    mut sprites: Query<(&mut Sprite, &mut Visibility), With<UpscaleSprite>>,
) {
    // Headless (no window): nothing renders, leave everything be.
    let (Some(view), Ok(window)) = (view, windows.single()) else {
        return;
    };
    let Ok(mut main) = main_cameras.single_mut() else {
        return;
    };
    if !matches!(&main.target, RenderTarget::Image(t) if t.handle == view.0) {
        main.target = RenderTarget::Image(view.0.clone().into());
    }

    let scale = active.tier.render_scale(window.scale_factor());
    let size = (Vec2::new(
        window.physical_width() as f32,
        window.physical_height() as f32,
    ) * scale)
        .round()
        .max(Vec2::ONE)
        .as_uvec2();
    let extent = Extent3d {
        width: size.x,
        height: size.y,
        depth_or_array_layers: 1,
    };
    if images
        .get(&view.0)
        .is_some_and(|image| image.texture_descriptor.size != extent)
        && let Some(image) = images.get_mut(&view.0)
    {
        image.resize(extent);
    }

    // While the main camera is stood down (the dev Creator stage owns the
    // window) the sprite hides, and this camera matches the 3D cameras'
    // MSAA so the two share main textures and the UI composites over the
    // preview instead of over a stale texture.
    let showing = main.is_active;
    let want_msaa = if showing {
        Msaa::Off
    } else {
        active.tier.msaa()
    };
    for mut msaa in &mut upscale_cameras {
        msaa.set_if_neq(want_msaa);
    }
    for (mut sprite, mut visibility) in &mut sprites {
        let want_size = Some(window.size());
        if sprite.custom_size != want_size {
            sprite.custom_size = want_size;
        }
        visibility.set_if_neq(if showing {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}

#[cfg(test)]
#[path = "quality.test.rs"]
mod tests;
