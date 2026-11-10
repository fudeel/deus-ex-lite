//! Camera, post-processing stack, graphics quality presets and augmentation visuals.
use bevy::core_pipeline::bloom::Bloom;
use bevy::core_pipeline::experimental::taa::TemporalAntiAliasing;
use bevy::core_pipeline::fxaa::Fxaa;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::pbr::{
    DirectionalLightShadowMap, DistanceFog, FogFalloff, PointLightShadowMap, ScreenSpaceAmbientOcclusion, ScreenSpaceAmbientOcclusionQualityLevel,
    VolumetricFog,
};
use bevy::asset::AssetId;
use bevy::prelude::*;
use bevy::render::camera::Exposure;
use bevy::render::render_resource::Shader;
use bevy::render::view::ColorGrading;

use crate::game::*;
use crate::items::*;
use crate::level::*;
use crate::player::*;

/// Base camera exposure. Lower = brighter image.
pub const BASE_EV100: f32 = 5.9;

/// Asset id of Bevy 0.16.1's internal volumetric fog shader (`VOLUMETRIC_FOG_HANDLE`, which is
/// private to bevy_pbr).
const VOLUMETRIC_FOG_SHADER_UUID: u128 = 0x481f474c_2024_44bb_8f79_f7c05ced95ea;

/// Replace Bevy's volumetric fog shader with our patched copy (`src/shaders/volumetric_fog.wgsl`).
///
/// Bevy 0.16.1 picks the clustered-light list once per pixel at the depth where the fog ray
/// starts. The camera is always inside our fog volume, so that is the nearest depth slice, and
/// a street lamp's shaft only showed up in screen tiles whose first few metres touched its
/// cone: tile-shaped holes in the light shafts that flickered while moving. The patched shader
/// looks the cluster up at every ray-march step. Must run after `DefaultPlugins` (it does: the
/// plugin is added later), which register the original shader.
fn override_volumetric_fog_shader(app: &mut App) {
    let id: AssetId<Shader> = AssetId::Uuid { uuid: bevy::asset::uuid::Uuid::from_u128(VOLUMETRIC_FOG_SHADER_UUID) };
    match app.world_mut().get_resource_mut::<Assets<Shader>>() {
        Some(mut shaders) => {
            if !shaders.contains(id) {
                warn!("volumetric fog shader not found under the expected id - is this still Bevy 0.16?");
            }
            shaders.insert(id, Shader::from_wgsl(include_str!("shaders/volumetric_fog.wgsl"), "deusmobile/shaders/volumetric_fog.wgsl"));
        }
        None => warn!("no shader assets: volumetric fog fix not applied"),
    }
}

pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        override_volumetric_fog_shader(app);
        app.insert_resource(ClearColor(Color::srgb(0.004, 0.006, 0.011)))
            .insert_resource(AmbientLight { color: Color::srgb(0.45, 0.55, 0.8), brightness: 15.0, affects_lightmapped_meshes: true })
            .insert_resource(DirectionalLightShadowMap { size: 2048 })
            .insert_resource(PointLightShadowMap { size: 1024 })
            .add_systems(Startup, spawn_camera)
            .add_systems(Update, (apply_quality, aug_grading).in_set(GameSet::Present))
            .add_systems(Update, menu_camera.run_if(in_state(AppState::Menu)));
    }
}

fn spawn_camera(mut commands: Commands) {
    commands
        .spawn((
            Camera3d::default(),
            Camera { hdr: true, ..default() },
            Projection::Perspective(PerspectiveProjection { fov: 80f32.to_radians(), near: 0.03, far: 500.0, ..default() }),
            Tonemapping::TonyMcMapface,
            Bloom { intensity: 0.22, low_frequency_boost: 0.65, ..Bloom::NATURAL },
            Exposure { ev100: BASE_EV100 },
            ColorGrading::default(),
            DistanceFog { color: Color::srgb(0.006, 0.008, 0.014), falloff: FogFalloff::Exponential { density: 0.018 }, ..default() },
            Msaa::Off,
            MainCamera,
            SpatialListener::new(0.3),
            Transform::from_xyz(40.0, 4.0, 72.0).looking_at(Vec3::new(40.0, 2.0, 40.0), Vec3::Y),
        ))
        .with_children(|c| {
            c.spawn((
                SpotLight {
                    intensity: 0.0,
                    range: 26.0,
                    radius: 0.03,
                    color: Color::srgb(1.0, 0.95, 0.85),
                    outer_angle: 0.42,
                    inner_angle: 0.22,
                    shadows_enabled: true,
                    ..default()
                },
                // left of and slightly ahead of the view model so the gun isn't blown out;
                // not volumetric: a beam starting at the eye would flood the whole view with haze
                Transform::from_xyz(-0.12, -0.02, -0.26),
                Flashlight,
            ));
        });
}

/// Insert / remove expensive passes when the quality setting changes.
fn apply_quality(mut commands: Commands, settings: Res<Settings>, cam: Query<Entity, With<MainCamera>>, mut applied: Local<Option<Quality>>) {
    if *applied == Some(settings.quality) {
        return;
    }
    let Ok(e) = cam.single() else { return };
    *applied = Some(settings.quality);
    let mut ec = commands.entity(e);
    ec.remove::<(TemporalAntiAliasing, Fxaa, ScreenSpaceAmbientOcclusion, VolumetricFog)>();
    match settings.quality {
        Quality::Low => {
            ec.insert(Fxaa::default());
        }
        q => {
            let (ssao, steps) = match q {
                Quality::Medium => (ScreenSpaceAmbientOcclusionQualityLevel::Low, 24),
                Quality::High => (ScreenSpaceAmbientOcclusionQualityLevel::Medium, 48),
                _ => (ScreenSpaceAmbientOcclusionQualityLevel::High, 64),
            };
            ec.insert((
                TemporalAntiAliasing::default(),
                ScreenSpaceAmbientOcclusion { quality_level: ssao, constant_object_thickness: 0.3 },
                VolumetricFog { ambient_color: Color::srgb(0.4, 0.5, 0.7), ambient_intensity: 0.0, jitter: 0.5, step_count: steps },
            ));
        }
    }
    info!("graphics quality: {}", settings.quality.name());
}

fn aug_grading(
    time: Res<Time>,
    state: Res<State<AppState>>,
    augs: Res<Augs>,
    me: Res<Me>,
    settings: Res<Settings>,
    mut cam: Query<(&mut Exposure, &mut ColorGrading), With<MainCamera>>,
    mut cur: Local<(f32, f32)>,
) {
    let Ok((mut exp, mut cg)) = cam.single_mut() else { return };
    let in_game = *state.get() == AppState::InGame;
    let vision = in_game && augs.on(Aug::Vision);
    let cloak = in_game && augs.on(Aug::Cloak);
    let dt = time.delta_secs();
    let target_ev = BASE_EV100 - settings.brightness - if vision { 3.4 } else { 0.0 };
    cur.0 += (target_ev - cur.0) * (dt * 6.0).min(1.0);
    if cur.0 == 0.0 {
        cur.0 = target_ev;
    }
    exp.ev100 = cur.0;
    let target_sat = if vision { 0.05 } else if cloak { 0.55 } else if me.dead && in_game { 0.2 } else { 0.92 };
    cur.1 += (target_sat - cur.1) * (dt * 6.0).min(1.0);
    cg.global.post_saturation = cur.1;
    cg.global.temperature = if cloak { -0.35 } else { -0.08 };
    cg.shadows.lift = if vision { 0.02 } else { 0.0 };
    cg.midtones.contrast = 1.06;
    cg.shadows.saturation = 0.85;
}

/// Slow cinematic drift along the docks while the main menu is open.
fn menu_camera(time: Res<Time>, mut cam: Query<&mut Transform, With<MainCamera>>, mut proj: Query<&mut Projection, With<MainCamera>>) {
    let Ok(mut tf) = cam.single_mut() else { return };
    let t = time.elapsed_secs() * 0.035;
    let x = 22.0 + (t.sin() * 0.5 + 0.5) * 60.0;
    let pos = Vec3::new(x, 3.2 + (t * 2.3).sin() * 0.4, 66.5);
    let look = Vec3::new(x + 6.0 + (t * 1.7).cos() * 6.0, 1.6, 50.0);
    *tf = Transform::from_translation(pos).looking_at(look, Vec3::Y);
    if let Ok(mut p) = proj.single_mut() {
        if let Projection::Perspective(pp) = p.as_mut() {
            pp.fov = 70f32.to_radians();
        }
    }
    let _ = CELL;
}
