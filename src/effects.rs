//! Visual effects: tracers, sparks, blood, decals, NPC muzzle flashes and rain.
use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;
use std::collections::VecDeque;

use crate::audio::*;
use crate::game::*;
use crate::level::*;
use crate::materials::*;
use crate::util::*;

#[derive(Component)]
pub struct Particle {
    pub vel: Vec3,
    pub life: f32,
    pub max: f32,
    pub grav: f32,
    pub size: Vec3,
}

#[derive(Component)]
pub struct Fade {
    pub life: f32,
    pub max: f32,
    pub light: f32,
}

#[derive(Component)]
pub struct Decal;

#[derive(Component)]
pub struct RainDrop {
    pub speed: f32,
}

#[derive(Resource, Default)]
pub struct Decals(pub VecDeque<Entity>);

pub struct EffectsPlugin;

impl Plugin for EffectsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Decals>()
            .add_systems(OnEnter(AppState::InGame), |mut d: ResMut<Decals>| d.0.clear())
            .add_systems(Update, (handle_fx, update_particles, update_fades, update_rain).in_set(GameSet::Present));
    }
}

pub fn spawn_rain(commands: &mut Commands, shapes: &Shapes, mats: &Mats) {
    let mut rng = Rng::new(1234);
    for _ in 0..1100 {
        let p = Vec3::new(rng.range(-18.0, 18.0), rng.range(0.0, 14.0), rng.range(-18.0, 18.0));
        commands.spawn((
            Mesh3d(shapes.cube.clone()),
            MeshMaterial3d(mats.rain.clone()),
            Transform::from_translation(p + Vec3::new(50.0, 0.0, 60.0)).with_scale(Vec3::new(0.008, 0.5, 0.008)),
            RainDrop { speed: rng.range(10.0, 13.0) },
            NotShadowCaster,
            LevelEntity,
        ));
    }
}

fn update_rain(
    time: Res<Time>,
    cam: Query<&GlobalTransform, With<MainCamera>>,
    level: Option<Res<Level>>,
    mut drops: Query<(&RainDrop, &mut Transform, &mut Visibility)>,
    mut rng: ResMut<GameRng>,
) {
    let (Ok(cam), Some(level)) = (cam.single(), level) else { return };
    let c = cam.translation();
    let dt = time.delta_secs().min(0.05);
    let wind = Vec3::new(0.8, 0.0, 0.4);
    for (d, mut tf, mut vis) in drops.iter_mut() {
        tf.translation += (Vec3::NEG_Y * d.speed + wind) * dt;
        let rel = tf.translation - c;
        let ground = if level.tile_at(tf.translation) == Tile::Water { WATER_Y } else { 0.0 };
        if tf.translation.y < ground || rel.x.abs() > 18.0 || rel.z.abs() > 18.0 {
            // respawn above the camera somewhere outdoors
            for _ in 0..4 {
                let p = Vec3::new(c.x + rng.0.range(-18.0, 18.0), c.y + rng.0.range(4.0, 12.0), c.z + rng.0.range(-18.0, 18.0));
                tf.translation = p;
                if !level.indoor_at(p) && !level.tile_at(p).is_wallish() {
                    break;
                }
            }
        }
        let t = level.tile_at(tf.translation);
        let near_lens = Vec2::new(rel.x, rel.z).length() < 1.3 && rel.y.abs() < 2.5;
        let hidden = near_lens || t.indoor() || (t.is_wallish() && tf.translation.y < ROOF_H + 0.3) || matches!(t, Tile::Stack(_)) && tf.translation.y < 2.6;
        let want = if hidden { Visibility::Hidden } else { Visibility::Inherited };
        if *vis != want {
            *vis = want;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_fx(
    mut commands: Commands,
    mut evs: EventReader<FxEv>,
    mats: Res<Mats>,
    shapes: Res<Shapes>,
    mut rng: ResMut<GameRng>,
    mut decals: ResMut<Decals>,
    mut snd: EventWriter<SoundEv>,
    settings: Res<Settings>,
) {
    for e in evs.read() {
        let Fx::Shot { from, to, weapon, impact: imp, normal } = &e.fx else { continue };
        let from = v3(*from);
        let to = v3(*to);
        let n = v3(*normal).normalize_or_zero();
        let d = to - from;
        let len = d.length();
        // shots relayed from other players: we also need to hear them
        if e.remote {
            let sfx = match *weapon {
                0 => Some((Sfx::Zap, 0.8)),
                1 => Some((Sfx::Pistol, 0.9)),
                2 => Some((Sfx::Crossbow, 0.5)),
                3 => Some((Sfx::Rifle, 0.9)),
                10 | 11 => Some((Sfx::GuardShot, 0.9)),
                12 => Some((Sfx::TurretShot, 0.8)),
                _ => None,
            };
            if let Some((sfx, vol)) = sfx {
                snd.write(SoundEv::at(sfx, from, vol));
            }
        }
        // tracer
        if *weapon != 0 && len > 0.5 {
            let mid = from + d * 0.5;
            let w = if *weapon == 2 { 0.006 } else { 0.012 };
            commands.spawn((
                Mesh3d(shapes.cube.clone()),
                MeshMaterial3d(mats.tracer.clone()),
                Transform::from_translation(mid).looking_at(to, Vec3::Y).with_scale(Vec3::new(w, w, len.min(60.0))),
                Fade { life: 0.05, max: 0.05, light: 0.0 },
                NotShadowCaster,
                LevelEntity,
            ));
        }
        // npc / turret muzzle flash light
        if *weapon >= 10 {
            commands.spawn((
                PointLight { intensity: 160_000.0, range: 9.0, color: Color::srgb(1.0, 0.72, 0.38), shadows_enabled: false, ..default() },
                Transform::from_translation(from),
                Fade { life: 0.06, max: 0.06, light: 160_000.0 },
                LevelEntity,
            ));
            commands.spawn((
                Mesh3d(shapes.sphere_lo.clone()),
                MeshMaterial3d(mats.muzzle.clone()),
                Transform::from_translation(from).with_scale(Vec3::splat(0.12)),
                Fade { life: 0.05, max: 0.05, light: 0.0 },
                NotShadowCaster,
                LevelEntity,
            ));
        }
        // impact
        let count_mul = match settings.quality {
            Quality::Low => 0.5,
            _ => 1.0,
        };
        let mut burst = |mat: &Handle<StandardMaterial>, count: usize, speed: f32, grav: f32, size: f32, life: f32, rng: &mut Rng| {
            for _ in 0..((count as f32 * count_mul) as usize).max(1) {
                let dir = (n * 0.8 + rng.unit_sphere()).normalize_or_zero();
                let s = Vec3::splat(size * rng.range(0.6, 1.3));
                commands.spawn((
                    Mesh3d(shapes.cube.clone()),
                    MeshMaterial3d(mat.clone()),
                    Transform::from_translation(to + n * 0.02).with_scale(s),
                    Particle { vel: dir * speed * rng.range(0.4, 1.2), life, max: life, grav, size: s },
                    NotShadowCaster,
                    LevelEntity,
                ));
            }
        };
        match *imp {
            impact::WALL | impact::METAL => {
                let metal = *imp == impact::METAL;
                burst(&mats.spark, if metal { 10 } else { 5 }, if metal { 5.0 } else { 3.5 }, 9.0, 0.012, 0.35, &mut rng.0);
                snd.write(SoundEv::at(if metal { Sfx::ImpactMetal } else { Sfx::ImpactWall }, to, 0.5));
                if n.length_squared() > 0.5 && *weapon < 10 {
                    let size = rng.0.range(0.06, 0.09);
                    let ent = commands
                        .spawn((
                            Mesh3d(shapes.quad.clone()),
                            MeshMaterial3d(mats.decal.clone()),
                            Transform::from_translation(to + n * 0.006).with_rotation(Quat::from_rotation_arc(Vec3::Y, n)).with_scale(Vec3::new(size, 1.0, size)),
                            Decal,
                            NotShadowCaster,
                            LevelEntity,
                        ))
                        .id();
                    decals.0.push_back(ent);
                    while decals.0.len() > 80 {
                        if let Some(old) = decals.0.pop_front() {
                            commands.entity(old).try_despawn();
                        }
                    }
                }
                commands.spawn((
                    PointLight { intensity: 15_000.0, range: 2.5, color: Color::srgb(1.0, 0.7, 0.35), shadows_enabled: false, ..default() },
                    Transform::from_translation(to + n * 0.1),
                    Fade { life: 0.05, max: 0.05, light: 15_000.0 },
                    LevelEntity,
                ));
            }
            impact::FLESH => {
                if *weapon < 10 {
                    burst(&mats.blood, 9, 2.5, 9.8, 0.025, 0.6, &mut rng.0);
                    snd.write(SoundEv::at(Sfx::ImpactFlesh, to, 0.6));
                }
            }
            impact::GLASS => {
                burst(&mats.glass_shard, 12, 3.0, 9.8, 0.03, 0.8, &mut rng.0);
                burst(&mats.spark, 6, 4.0, 9.8, 0.01, 0.25, &mut rng.0);
                snd.write(SoundEv::at(Sfx::Glass, to, 0.6));
            }
            _ => {}
        }
    }
}

fn update_particles(mut commands: Commands, time: Res<Time>, mut q: Query<(Entity, &mut Particle, &mut Transform)>) {
    let dt = time.delta_secs();
    for (e, mut p, mut tf) in q.iter_mut() {
        p.life -= dt;
        if p.life <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        p.vel.y -= p.grav * dt;
        let v = p.vel;
        tf.translation += v * dt;
        if tf.translation.y < 0.005 && v.y < 0.0 {
            tf.translation.y = 0.005;
            p.vel *= Vec3::new(0.4, -0.2, 0.4);
        }
        let k = (p.life / p.max).clamp(0.0, 1.0);
        tf.scale = p.size * (0.3 + 0.7 * k);
    }
}

fn update_fades(mut commands: Commands, time: Res<Time>, mut q: Query<(Entity, &mut Fade, Option<&mut PointLight>)>) {
    let dt = time.delta_secs();
    for (e, mut f, light) in q.iter_mut() {
        f.life -= dt;
        if f.life <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        if let Some(mut l) = light {
            l.intensity = f.light * (f.life / f.max);
        }
    }
}
