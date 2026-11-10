//! Team match: other players (humans and bots) as animated operatives in the world, with
//! their held weapon, flashlight, footsteps and hit boxes.
use bevy::pbr::VolumetricLight;
use bevy::prelude::*;
use std::f32::consts::FRAC_PI_2;

use crate::audio::*;
use crate::collision::*;
use crate::game::*;
use crate::items::*;
use crate::level::*;
use crate::materials::*;
use crate::npc::{spawn_humanoid, HandMount, HumanStyle, Rig};
use crate::util::*;
use crate::world::{LightInfo, LightReg};

pub const FLASHLIGHT_LM: f32 = 170_000.0;

#[derive(Component)]
pub struct Avatar {
    pub id: u8,
    pub team: u8,
    /// weapon model currently attached (255 = none)
    pub weapon: u8,
    pub light: Entity,
    pub speed: f32,
    pub step: f32,
    pub crouch: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub snap: bool,
}

pub struct AvatarPlugin;

impl Plugin for AvatarPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, avatar_bodies.after(crate::world::rebuild_bodies).in_set(GameSet::Input).run_if(in_game).run_if(teams_mode))
            .add_systems(Update, avatar_lights.after(crate::npc::npc_lights).in_set(GameSet::Input).run_if(in_game).run_if(teams_mode))
            .add_systems(Update, (sync_avatars, animate_avatars).chain().in_set(GameSet::Present).run_if(in_game).run_if(teams_mode));
    }
}

/// Shootable boxes for everyone except the local player (uses the smoothed, drawn position
/// so what you see is what you hit).
fn avatar_bodies(mut phys: ResMut<Physics>, players: Res<Players>, avatars: Query<(&Avatar, &Transform)>) {
    for (a, t) in avatars.iter() {
        let Some(p) = players.map.get(&a.id) else { continue };
        if p.net.has(pflags::DOWNED) {
            continue;
        }
        let h = if p.net.has(pflags::CROUCH) { 1.15 } else { 1.8 };
        let feet = t.translation;
        phys.targets.push((Aabb::centered(feet + Vec3::Y * (h * 0.42), Vec3::new(0.28, h * 0.42, 0.28)), Body::Player(a.id)));
        phys.targets.push((Aabb::centered(feet + Vec3::Y * (h - 0.15), Vec3::splat(0.15)), Body::PlayerHead(a.id)));
    }
}

/// Other players' flashlights light you up (and make you visible to bots), like guards' do.
fn avatar_lights(mut reg: ResMut<LightReg>, players: Res<Players>, avatars: Query<(&Avatar, &Transform)>) {
    for (a, t) in avatars.iter() {
        let Some(p) = players.map.get(&a.id) else { continue };
        if !p.net.has(pflags::LIGHT) || p.net.has(pflags::DOWNED) {
            continue;
        }
        let pos = t.translation + Vec3::Y * (1.45 - 0.4 * a.crouch);
        reg.dynamic.push(LightInfo { pos, dir: look_dir(a.yaw, a.pitch - 0.08), cos_outer: 0.42f32.cos(), range: 18.0, power: 1.3 });
    }
}

#[allow(clippy::too_many_arguments)]
fn sync_avatars(
    mut commands: Commands,
    mut players: ResMut<Players>,
    local: Res<LocalId>,
    mats: Res<Mats>,
    shapes: Res<Shapes>,
    settings: Res<Settings>,
    avatars: Query<(Entity, &Avatar)>,
) {
    for (e, a) in avatars.iter() {
        let keep = players.map.get(&a.id).map(|p| p.avatar == Some(e) && p.team == a.team).unwrap_or(false);
        if !keep {
            commands.entity(e).despawn();
        }
    }
    let hi = matches!(settings.quality, Quality::High | Quality::Ultra);
    for (id, p) in players.map.iter_mut() {
        if *id == local.0 || p.team >= 2 {
            continue;
        }
        if let Some(e) = p.avatar {
            if avatars.get(e).map(|(_, a)| a.team == p.team).unwrap_or(false) {
                continue;
            }
        }
        let pos = p.net.pos();
        let (root, rig) = spawn_humanoid(&mut commands, &HumanStyle::Agent { team: p.team }, &mats, &shapes, Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(p.net.yaw)));
        let mut le = commands.spawn((
            SpotLight {
                intensity: 0.0,
                range: 22.0,
                radius: 0.05,
                color: Color::srgb(1.0, 0.96, 0.88),
                outer_angle: 0.42,
                inner_angle: 0.22,
                shadows_enabled: settings.quality == Quality::Ultra,
                ..default()
            },
            Transform::from_xyz(0.18, 1.45, -0.3),
        ));
        if hi {
            le.insert(VolumetricLight);
        }
        let light = le.id();
        commands.entity(root).add_child(light);
        commands.entity(root).insert((
            Avatar { id: *id, team: p.team, weapon: 255, light, speed: 0.0, step: 0.0, crouch: 0.0, yaw: p.net.yaw, pitch: 0.0, snap: true },
            rig,
        ));
        p.avatar = Some(root);
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn animate_avatars(
    mut commands: Commands,
    time: Res<Time>,
    players: Res<Players>,
    local: Res<LocalId>,
    level: Res<Level>,
    assets: (Res<Mats>, Res<Shapes>),
    mut q: Query<(&mut Avatar, &mut Transform, &mut Rig, &mut Visibility)>,
    mut parts: Query<&mut Transform, Without<Avatar>>,
    mut lights: Query<&mut SpotLight>,
    mounts: Query<Option<&Children>, With<HandMount>>,
    mut snd: EventWriter<SoundEv>,
) {
    let (mats, shapes) = (&assets.0, &assets.1);
    let dt = time.delta_secs().min(0.05);
    let my_team = players.team_of(local.0);
    for (mut a, mut tf, mut rig, mut vis) in q.iter_mut() {
        let Some(p) = players.map.get(&a.id) else { continue };
        // ---- smooth network motion
        let target = p.net.pos();
        let prev = tf.translation;
        if a.snap || prev.distance(target) > 4.0 {
            tf.translation = target;
            a.snap = false;
        } else {
            tf.translation = prev.lerp(target, exp_decay(14.0, dt));
        }
        let moved = (tf.translation - prev).with_y(0.0).length();
        let spd = moved / dt.max(1e-4);
        a.speed += (spd - a.speed) * exp_decay(10.0, dt);
        a.yaw = approach_angle(a.yaw, p.net.yaw, dt * 14.0);
        a.pitch += (p.net.pitch - a.pitch) * exp_decay(12.0, dt);
        tf.rotation = Quat::from_rotation_y(a.yaw);

        let downed = p.net.has(pflags::DOWNED);
        let crouch_target = if p.net.has(pflags::CROUCH) && !downed { 1.0 } else { 0.0 };
        a.crouch += (crouch_target - a.crouch) * exp_decay(10.0, dt);
        rig.down = if downed { (rig.down + dt * 2.2).min(1.0) } else { (rig.down - dt * 2.0).max(0.0) };
        let walk = (a.speed / 1.8).min(1.5);
        rig.phase += a.speed * dt * 3.2;
        let swing = rig.phase.sin() * 0.55 * walk.min(1.0) * (1.0 - a.crouch * 0.5);
        let e = rig.down * rig.down * (3.0 - 2.0 * rig.down);
        let gun = matches!(p.net.weapon, 1..=3);
        let two_handed = matches!(p.net.weapon, 2 | 3);

        if let Ok(mut b) = parts.get_mut(rig.body) {
            b.rotation = Quat::from_rotation_x(e * FRAC_PI_2);
            b.translation = Vec3::new(0.0, 0.13 * e - 0.4 * a.crouch * (1.0 - e), 0.0);
        }
        let bend = a.crouch * 1.05;
        if let Ok(mut l) = parts.get_mut(rig.l_leg) {
            l.rotation = Quat::from_rotation_x(if downed { 0.0 } else { swing + bend });
        }
        if let Ok(mut r) = parts.get_mut(rig.r_leg) {
            r.rotation = Quat::from_rotation_x(if downed { 0.0 } else { -swing + bend });
        }
        if let Ok(mut r) = parts.get_mut(rig.r_arm) {
            r.rotation = if downed {
                Quat::from_rotation_z(0.3)
            } else if gun {
                Quat::from_rotation_x(1.5 + a.pitch * 0.85) * Quat::from_rotation_z(0.1)
            } else {
                Quat::from_rotation_x(swing * 0.5 + 0.25)
            };
        }
        if let Ok(mut l) = parts.get_mut(rig.l_arm) {
            l.rotation = if downed {
                Quat::from_rotation_z(-0.3)
            } else if two_handed {
                Quat::from_rotation_x(1.35 + a.pitch * 0.85) * Quat::from_rotation_z(-0.55)
            } else {
                Quat::from_rotation_x(-swing * 0.5)
            };
        }
        if let Ok(mut h) = parts.get_mut(rig.head) {
            h.rotation = Quat::from_rotation_x((a.pitch * 0.6).clamp(-0.6, 0.6));
        }

        // ---- flashlight
        if let Ok(mut s) = lights.get_mut(a.light) {
            let want = if p.net.has(pflags::LIGHT) && !downed { FLASHLIGHT_LM } else { 0.0 };
            if (s.intensity - want).abs() > 1.0 {
                s.intensity = want;
            }
        }
        if let Ok(mut lt) = parts.get_mut(a.light) {
            lt.translation = Vec3::new(0.18, 1.45 - 0.4 * a.crouch, -0.3);
            lt.rotation = Quat::from_rotation_x(a.pitch - 0.08);
        }

        // ---- cloak: enemies cannot see a cloaked operative at all
        let want_vis = if p.net.has(pflags::CLOAK) && p.team != my_team { Visibility::Hidden } else { Visibility::Inherited };
        if *vis != want_vis {
            *vis = want_vis;
        }

        // ---- footsteps (this is how you hear someone sneaking up on you)
        if !downed && a.speed > 0.6 {
            a.step += moved;
            let sprint = p.net.has(pflags::SPRINT);
            let crouch = p.net.has(pflags::CROUCH);
            let stride = if sprint { 2.3 } else if crouch { 1.3 } else { 1.9 };
            if a.step > stride {
                a.step = 0.0;
                let tile = level.tile_at(tf.translation);
                let metal = matches!(tile, Tile::Vent | Tile::Grate) || tile.floor() == Some(Floor::Metal);
                let vol = if crouch { 0.1 } else if sprint { 0.75 } else { 0.38 };
                snd.write(SoundEv::at(if metal { Sfx::StepMetal } else { Sfx::Step }, tf.translation, vol));
            }
        }

        // ---- held weapon model
        if a.weapon != p.net.weapon {
            a.weapon = p.net.weapon;
            if let Some(hand) = rig.hand {
                if let Ok(Some(children)) = mounts.get(hand) {
                    for c in children.iter() {
                        commands.entity(c).despawn();
                    }
                }
                if let Some(w) = weapon_from_id(p.net.weapon) {
                    let defs = weapon_parts(w, mats, shapes);
                    commands.entity(hand).with_children(|g| spawn_parts(g, &defs));
                }
            }
        }
    }
}
