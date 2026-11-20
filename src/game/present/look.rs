//! The look pass (task 3 of the graphics and camera plan): colour grading,
//! distance fog toward the horizon, a gradient skybox over the flat clear
//! colour, and HDR bloom on High. All cosmetic and all on the main camera
//! — nothing here touches gameplay.
//!
//! Each piece follows the tier's [`RenderBudget`]: grading and fog are on at
//! every tier (both are a few shader instructions), the sky from Medium,
//! and bloom only on High. The camera's look is re-applied whenever the
//! tier, the sky (theme or weather) or the camera itself changes.
//!
//! Bloom renders through the same offscreen route as every other tier
//! (`quality::apply_render_scale`): the main camera's HDR target is
//! tonemapped into the scaled image, which the upscale camera stretches.

use bevy::camera::Exposure;
use bevy::core_pipeline::Skybox;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::view::{ColorGrading, ColorGradingGlobal, ColorGradingSection, Hdr};

use crate::game::camera::MainCamera;
use crate::game::quality::{ActiveQuality, RenderBudget};
use crate::game::settings::Settings;

/// Fog distances (m) on a clear day: nothing on the field fogs (the wall is
/// ~130 m from the plate); the stands and the light towers soften.
pub const FOG_CLEAR: (f32, f32) = (170.0, 800.0);
/// Fog distances in full overcast: the stands past the wall soften. Fog
/// is measured from the camera, which sits behind the plate, so starting
/// past the ~130 m wall keeps the outfield itself clear.
pub const FOG_OVERCAST: (f32, f32) = (140.0, 520.0);

/// Bloom on High: gentler than Bevy's `NATURAL` so the white chalk and the
/// sunlit uniforms don't haze; emissive balls, fireworks and night lights
/// still glow.
const BLOOM_INTENSITY: f32 = 0.08;

/// The look the main camera should wear for a sky colour, an overcast
/// fraction and a tier: pure, so the tier and weather rules are unit-tested.
#[derive(Clone, Debug)]
pub struct CameraLook {
    pub fog: Option<(Color, f32, f32)>,
    pub grading: ColorGrading,
    pub sky: bool,
    pub bloom: bool,
}

/// Relative luminance of an sRGB colour (0 = black, 1 = white).
fn luminance(c: Color) -> f32 {
    let s = c.to_srgba();
    0.2126 * s.red + 0.7152 * s.green + 0.0722 * s.blue
}

/// True for a night sky (Midnight Neon's near-black, or any dark theme).
pub fn is_night(sky: Color) -> bool {
    luminance(sky) < 0.15
}

/// The horizon haze colour: the sky lifted toward a pale grey by day (air
/// scatters light near the horizon), and only slightly lifted at night.
pub fn horizon_haze(sky: Color) -> Color {
    let s = sky.to_srgba();
    let (target, t) = if is_night(sky) {
        (Srgba::new(0.10, 0.12, 0.20, 1.0), 0.5)
    } else {
        (Srgba::new(0.86, 0.89, 0.92, 1.0), 0.45)
    };
    Srgba::new(
        s.red + (target.red - s.red) * t,
        s.green + (target.green - s.green) * t,
        s.blue + (target.blue - s.blue) * t,
        1.0,
    )
    .into()
}

/// The zenith colour: a deeper, more saturated take on the sky.
pub fn zenith(sky: Color) -> Color {
    let s = sky.to_srgba();
    Srgba::new(s.red * 0.72, s.green * 0.80, s.blue * 0.95, 1.0).into()
}

impl CameraLook {
    pub fn for_sky(sky: Color, overcast: f32, budget: &RenderBudget) -> Self {
        let o = overcast.clamp(0.0, 1.0);
        let lerp = |a: f32, b: f32| a + (b - a) * o;
        let fog = budget.fog.then(|| {
            (
                // Fade toward what is behind the far geometry: the skybox's
                // horizon haze from Medium, the flat sky colour on Low.
                if budget.sky { horizon_haze(sky) } else { sky },
                lerp(FOG_CLEAR.0, FOG_OVERCAST.0),
                lerp(FOG_CLEAR.1, FOG_OVERCAST.1),
            )
        });
        let night = is_night(sky);
        let grading = ColorGrading {
            global: ColorGradingGlobal {
                // Day: a touch warm and rich. Night: cool, a little
                // desaturated, lifted shadows so dark uniforms still read.
                temperature: if night { -0.08 } else { 0.04 },
                post_saturation: if night { 0.95 } else { 1.0 + 0.08 * (1.0 - o) },
                ..default()
            },
            shadows: ColorGradingSection {
                lift: if night { 0.02 } else { 0.0 },
                ..default()
            },
            midtones: ColorGradingSection {
                contrast: if night { 1.0 } else { 1.06 - 0.04 * o },
                ..default()
            },
            highlights: ColorGradingSection::default(),
        };
        CameraLook {
            fog,
            grading,
            sky: budget.sky,
            bloom: budget.bloom,
        }
    }
}

pub struct LookPlugin;

impl Plugin for LookPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, init_sky_texture)
            .add_systems(Update, (repaint_sky, apply_camera_look).chain())
            // After the camera rigs have moved the eye this frame.
            .add_systems(PostUpdate, fog_follows_camera);
    }
}

/// The weather's overcast fraction (0 with no settings, e.g. a harness).
fn overcast(settings: Option<&Settings>) -> f32 {
    settings.map_or(0.0, |s| s.weather.spec().overcast)
}

/// The weather's fog distances for a camera at home plate, before
/// [`fog_range`] pushes them out for a camera that has backed off.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct FogBase(pub f32, pub f32);

/// How far behind the plate the batting views sit (m): no push-back there.
const FOG_HOME_SLACK: f32 = 15.0;

/// Fog distances for a camera at `eye`. Fog is measured from the camera,
/// so a wide shot (the home-run orbit, a high fly from deep) would fog the
/// field itself if the distances stayed fixed. Every metre the camera sits
/// beyond the batting views pushes the fog a metre further out, so it
/// always starts past the wall as seen from the plate.
pub fn fog_range(base: FogBase, eye: Vec3) -> (f32, f32) {
    let push = (eye.length() - FOG_HOME_SLACK).max(0.0);
    (base.0 + push, base.1 + push)
}

fn fog_follows_camera(
    mut cameras: Query<(&Transform, &FogBase, &mut DistanceFog), With<MainCamera>>,
) {
    for (eye, base, mut fog) in &mut cameras {
        let (start, end) = fog_range(*base, eye.translation);
        let want = FogFalloff::Linear { start, end };
        if !matches!(fog.falloff, FogFalloff::Linear { start: s, end: e } if s == start && e == end)
        {
            fog.falloff = want;
        }
    }
}

#[allow(clippy::type_complexity)]
fn apply_camera_look(
    mut commands: Commands,
    active: Option<Res<ActiveQuality>>,
    clear: Res<ClearColor>,
    settings: Option<Res<Settings>>,
    sky: Res<SkyTexture>,
    cameras: Query<(Entity, Ref<MainCamera>), With<Camera3d>>,
    exposure: Query<&Exposure>,
) {
    let Some(active) = active else {
        return;
    };
    let settings_changed = settings.as_ref().is_some_and(|s| s.is_changed());
    for (camera, marker) in &cameras {
        if !(active.is_changed() || clear.is_changed() || settings_changed || marker.is_added()) {
            continue;
        }
        let look = CameraLook::for_sky(clear.0, overcast(settings.as_deref()), &active.budget());
        let mut e = commands.entity(camera);
        e.insert((Tonemapping::TonyMcMapface, look.grading));
        match look.fog {
            Some((color, start, end)) => {
                e.insert((
                    FogBase(start, end),
                    DistanceFog {
                        color,
                        falloff: FogFalloff::Linear { start, end },
                        ..default()
                    },
                ));
            }
            None => {
                e.remove::<(FogBase, DistanceFog)>();
            }
        }
        if look.sky {
            // The skybox pass runs after the opaque pass and only shades
            // pixels nothing else covered, so the sky costs one pass over
            // the visible sky, not a full-screen mesh. Its brightness is in
            // cd/m², scaled by the camera's exposure: cancel that so the
            // texture shows the sky colour as authored, as the clear colour
            // under it does.
            // Read once per re-apply: nothing changes a camera's
            // `Exposure` today; a writer would need to re-run this.
            let exposure = exposure.get(camera).copied().unwrap_or_default();
            e.insert(Skybox {
                image: sky.0.clone(),
                brightness: exposure.exposure().recip(),
                rotation: Quat::IDENTITY,
            });
        } else {
            e.remove::<Skybox>();
        }
        if look.bloom {
            e.insert((
                Hdr,
                Bloom {
                    intensity: BLOOM_INTENSITY,
                    ..Bloom::NATURAL
                },
            ));
        } else {
            e.remove::<(Bloom, Hdr)>();
        }
    }
}

/// Texels along one cube face edge. The sky only varies with elevation, so
/// 64 rows per face keep the gradient smooth under linear filtering.
const SKY_FACE: u32 = 64;

/// How far toward the horizon haze the sky is at elevation `e` (the sine of
/// the angle above the horizon, -1..=1): 0 straight up, 1 at the horizon
/// and everywhere below it.
pub fn haze_at(e: f32) -> f32 {
    // Polar angle from the zenith, 0..1 with the horizon at 0.5.
    let v = e.clamp(-1.0, 1.0).acos() / std::f32::consts::PI;
    (v / 0.5).clamp(0.0, 1.0).powf(1.6)
}

/// The gradient sky as a cubemap: zenith overhead, fading to the horizon
/// haze at eye level and below (the fog's colour, so the far stands and the
/// sky meet without a seam).
fn sky_cubemap(sky: Color) -> Image {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{
        Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
    };
    let top = zenith(sky).to_srgba();
    let haze = horizon_haze(sky).to_srgba();
    let n = SKY_FACE as usize;
    let mut data = Vec::with_capacity(n * n * 6 * 4);
    // Faces in wgpu order: +X, -X, +Y, -Y, +Z, -Z. Only the direction's
    // height matters: the side faces run top (t = -1) to bottom (t = 1).
    for face in 0..6 {
        for row in 0..n {
            let t = (row as f32 + 0.5) / n as f32 * 2.0 - 1.0;
            for col in 0..n {
                let s = (col as f32 + 0.5) / n as f32 * 2.0 - 1.0;
                let dir = match face {
                    2 => Vec3::new(s, 1.0, t),
                    3 => Vec3::new(s, -1.0, -t),
                    _ => Vec3::new(1.0, -t, s),
                };
                let k = haze_at(dir.normalize().y);
                let c = Srgba::new(
                    top.red + (haze.red - top.red) * k,
                    top.green + (haze.green - top.green) * k,
                    top.blue + (haze.blue - top.blue) * k,
                    1.0,
                );
                data.extend_from_slice(&c.to_u8_array());
            }
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: SKY_FACE,
            height: SKY_FACE,
            depth_or_array_layers: 6,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..default()
    });
    image
}

/// The gradient sky's cubemap, shared by every main camera that wears it
/// and repainted in place when the sky changes.
#[derive(Resource)]
pub struct SkyTexture(pub Handle<Image>);

fn init_sky_texture(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    clear: Res<ClearColor>,
) {
    commands.insert_resource(SkyTexture(images.add(sky_cubemap(clear.0))));
}

/// `spawn_field` sets each game's [`ClearColor`] (theme and weather), so
/// repaint the sky whenever it changes.
fn repaint_sky(clear: Res<ClearColor>, sky: Res<SkyTexture>, mut images: ResMut<Assets<Image>>) {
    if !clear.is_changed() {
        return;
    }
    if let Some(image) = images.get_mut(&sky.0) {
        *image = sky_cubemap(clear.0);
    }
}

#[cfg(test)]
#[path = "look.test.rs"]
mod tests;
