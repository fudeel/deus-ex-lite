//! Guards, security cameras, turrets, friendly NPCs: models, perception, AI and pathfinding.
use bevy::pbr::{NotShadowCaster, VolumetricLight};
use bevy::prelude::*;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashSet};
use std::f32::consts::{FRAC_PI_2, PI, TAU};

use crate::audio::*;
use crate::collision::*;
use crate::game::*;
use crate::items::*;
use crate::level::*;
use crate::materials::*;
use crate::util::*;
use crate::world::*;

pub const BARKS: [&str; 10] = [
    "Huh? Who's there?",
    "Intruder! Open fire!",
    "Must've been the rain...",
    "Man down! We've got a body here!",
    "Where'd they go?",
    "Argh... can't... keep my eyes...",
    "Nothing here. Back to it.",
    "Heard something over there.",
    "Lights are out. Somebody's here.",
    "Keep your eyes open.",
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GState {
    Patrol,
    Sentry,
    Suspicious,
    Investigate,
    Combat,
    Search,
    Return,
    Unconscious,
    Dead,
}

#[derive(Component)]
pub struct Guard {
    pub id: u16,
    pub rifle: bool,
    pub hp: f32,
    pub stun: f32,
    pub state: GState,
    pub pos: Vec3,
    pub yaw: f32,
    pub route: Vec<Vec3>,
    pub route_i: usize,
    pub path: Vec<Vec3>,
    pub path_i: usize,
    pub goal: Option<Vec3>,
    pub wait: f32,
    pub home_yaw: f32,
    pub suspicion: f32,
    pub target: Option<u8>,
    pub last_seen: Vec3,
    pub state_t: f32,
    pub lost_t: f32,
    pub fire_cd: f32,
    pub burst: u8,
    pub tranq: f32,
    pub flashlight: bool,
    pub bark_cd: f32,
    pub speed: f32,
    pub aiming: bool,
    pub stagger: f32,
    pub search_t: f32,
    pub run: bool,
}

impl Guard {
    pub fn conscious(&self) -> bool {
        !matches!(self.state, GState::Unconscious | GState::Dead)
    }
    pub fn anim(&self) -> u8 {
        match self.state {
            GState::Unconscious => 5,
            GState::Dead => 6,
            _ if self.tranq > 0.0 || self.stagger > 0.0 => 7,
            _ if self.aiming => 3,
            _ if self.speed > 2.6 => 2,
            _ if self.speed > 0.2 => 1,
            _ => 0,
        }
    }
    fn set_state(&mut self, s: GState) {
        if self.state != s {
            self.state = s;
            self.state_t = 0.0;
            self.path.clear();
            self.goal = None;
        }
    }
}

/// Where a team-match avatar holds its weapon (children are rebuilt when the weapon changes).
#[derive(Component)]
pub struct HandMount;

#[derive(Component)]
pub struct Rig {
    pub body: Entity,
    pub head: Entity,
    pub l_leg: Entity,
    pub r_leg: Entity,
    pub l_arm: Entity,
    pub r_arm: Entity,
    pub visor: Option<Entity>,
    pub light: Option<Entity>,
    /// where the held weapon is attached (team-match avatars: an empty `HandMount` that is
    /// filled with the current weapon; guards: their gun)
    pub hand: Option<Entity>,
    pub phase: f32,
    pub down: f32,
    pub visor_on: bool,
}

#[derive(Component)]
pub struct SecCam {
    pub id: u16,
    pub base_yaw: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub status: u8,
    pub hp: f32,
    pub meter: f32,
    pub head: Entity,
    pub led: Entity,
    pub cone: Entity,
    pub pos: Vec3,
    pub cooldown: f32,
    pub phase: f32,
}

#[derive(Component)]
pub struct Turret {
    pub id: u16,
    pub yaw: f32,
    pub pitch: f32,
    pub status: u8,
    pub hp: f32,
    pub always: bool,
    pub head: Entity,
    pub led: Entity,
    pub flash: Entity,
    pub fire_cd: f32,
    pub pos: Vec3,
    pub flash_t: f32,
}

#[derive(Component)]
pub struct Civilian {
    pub id: u16,
    pub pos: Vec3,
    pub yaw: f32,
}

#[derive(Resource, Default)]
pub struct AiShared {
    pub discovered: HashSet<u16>,
    pub heightened: f32,
}

pub struct NpcPlugin;

impl Plugin for NpcPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AiShared>()
            .add_systems(OnEnter(AppState::InGame), |mut s: ResMut<AiShared>| *s = AiShared::default())
            .add_systems(Update, (npc_bodies.after(crate::world::rebuild_bodies), npc_lights.after(crate::world::update_light_registry)).in_set(GameSet::Input))
            .add_systems(
                Update,
                (npc_requests, guard_ai, camera_ai, turret_ai).chain().in_set(GameSet::Authority).run_if(is_authority).run_if(in_game),
            )
            .add_systems(Update, apply_barks.in_set(GameSet::Apply))
            .add_systems(Update, (animate_guards, animate_devices, animate_civilians).in_set(GameSet::Present));
    }
}

// ------------------------------------------------------------------ models

pub enum HumanStyle {
    Guard { rifle: bool },
    Informant,
    /// team-match operative (players and bots)
    Agent { team: u8 },
}

fn part(p: &mut ChildSpawnerCommands, s: &Shapes, m: &Handle<StandardMaterial>, pos: Vec3, size: Vec3) -> Entity {
    p.spawn((Mesh3d(s.cube.clone()), MeshMaterial3d(m.clone()), Transform::from_translation(pos).with_scale(size))).id()
}

use bevy::ecs::hierarchy::ChildSpawnerCommands;

/// Builds an articulated humanoid. Returns (root, rig).
pub fn spawn_humanoid(commands: &mut Commands, style: &HumanStyle, mats: &Mats, s: &Shapes, tf: Transform) -> (Entity, Rig) {
    let (torso, limbs, legs) = match style {
        HumanStyle::Guard { .. } => (mats.guard_armor.clone(), mats.guard_armor.clone(), mats.guard_cloth.clone()),
        HumanStyle::Informant => (mats.npc_jacket.clone(), mats.npc_jacket.clone(), mats.npc_pants.clone()),
        HumanStyle::Agent { .. } => (mats.coat.clone(), mats.coat.clone(), mats.guard_cloth.clone()),
    };
    let is_guard = matches!(style, HumanStyle::Guard { .. });
    let agent_team = match style {
        HumanStyle::Agent { team } => Some((*team).min(1) as usize),
        _ => None,
    };
    let root = commands.spawn((tf, Visibility::default(), LevelEntity)).id();
    let mut head = Entity::PLACEHOLDER;
    let mut l_leg = Entity::PLACEHOLDER;
    let mut r_leg = Entity::PLACEHOLDER;
    let mut l_arm = Entity::PLACEHOLDER;
    let mut r_arm = Entity::PLACEHOLDER;
    let mut visor = None;
    let mut hand = None;
    let body = commands
        .spawn((Transform::default(), Visibility::default()))
        .with_children(|b| {
            part(b, s, &torso, Vec3::new(0.0, 1.2, 0.0), Vec3::new(0.44, 0.56, 0.25));
            if is_guard {
                part(b, s, &mats.gun_dark, Vec3::new(0.0, 1.27, -0.02), Vec3::new(0.47, 0.34, 0.28));
                part(b, s, &mats.coat_accent, Vec3::new(0.0, 1.38, -0.162), Vec3::new(0.2, 0.012, 0.004));
            }
            if let Some(t) = agent_team {
                // trench coat skirt, team-painted chest rig and a dim team stripe
                part(b, s, &mats.coat, Vec3::new(0.0, 0.66, 0.015), Vec3::new(0.46, 0.5, 0.27));
                part(b, s, &mats.team_paint[t], Vec3::new(0.0, 1.25, -0.03), Vec3::new(0.4, 0.26, 0.24));
                part(b, s, &mats.team_glow[t], Vec3::new(0.0, 1.31, -0.152), Vec3::new(0.24, 0.014, 0.004));
                part(b, s, &mats.team_paint[t], Vec3::new(-0.27, 1.42, 0.0), Vec3::new(0.16, 0.1, 0.2));
                part(b, s, &mats.team_paint[t], Vec3::new(0.27, 1.42, 0.0), Vec3::new(0.16, 0.1, 0.2));
            }
            part(b, s, &mats.black, Vec3::new(0.0, 0.93, 0.0), Vec3::new(0.42, 0.07, 0.25));
            part(b, s, &legs, Vec3::new(0.0, 0.85, 0.0), Vec3::new(0.38, 0.16, 0.22));
            b.spawn((Mesh3d(s.cylinder.clone()), MeshMaterial3d(mats.skin.clone()), Transform::from_xyz(0.0, 1.51, 0.0).with_scale(Vec3::new(0.1, 0.1, 0.1))));
            head = b
                .spawn((Transform::from_xyz(0.0, 1.55, 0.0), Visibility::default()))
                .with_children(|h| {
                    h.spawn((Mesh3d(s.sphere.clone()), MeshMaterial3d(mats.skin.clone()), Transform::from_xyz(0.0, 0.11, 0.0).with_scale(Vec3::new(0.22, 0.25, 0.23))));
                    if is_guard {
                        h.spawn((Mesh3d(s.sphere.clone()), MeshMaterial3d(mats.guard_armor.clone()), Transform::from_xyz(0.0, 0.15, 0.01).with_scale(Vec3::new(0.27, 0.23, 0.29))));
                        visor = Some(
                            h.spawn((
                                Mesh3d(s.cube.clone()),
                                MeshMaterial3d(mats.guard_visor.clone()),
                                Transform::from_xyz(0.0, 0.12, -0.112).with_scale(Vec3::new(0.19, 0.045, 0.04)),
                                NotShadowCaster,
                            ))
                            .id(),
                        );
                    } else if agent_team.is_some() {
                        h.spawn((Mesh3d(s.sphere.clone()), MeshMaterial3d(mats.hair.clone()), Transform::from_xyz(0.0, 0.16, 0.015).with_scale(Vec3::new(0.235, 0.17, 0.235))));
                        h.spawn((Mesh3d(s.cube.clone()), MeshMaterial3d(mats.shades.clone()), Transform::from_xyz(0.0, 0.12, -0.11).with_scale(Vec3::new(0.18, 0.035, 0.03))));
                    } else {
                        h.spawn((Mesh3d(s.cube.clone()), MeshMaterial3d(mats.npc_pants.clone()), Transform::from_xyz(0.0, 0.22, 0.0).with_scale(Vec3::new(0.24, 0.07, 0.25))));
                        h.spawn((Mesh3d(s.cube.clone()), MeshMaterial3d(mats.npc_pants.clone()), Transform::from_xyz(0.0, 0.19, -0.13).with_scale(Vec3::new(0.22, 0.02, 0.1))));
                        h.spawn((Mesh3d(s.cube.clone()), MeshMaterial3d(mats.shades.clone()), Transform::from_xyz(0.0, 0.12, -0.11).with_scale(Vec3::new(0.17, 0.035, 0.03))));
                    }
                })
                .id();
            for (side, slot) in [(-1.0f32, 0), (1.0, 1)] {
                let leg = b
                    .spawn((Transform::from_xyz(side * 0.11, 0.88, 0.0), Visibility::default()))
                    .with_children(|l| {
                        part(l, s, &legs, Vec3::new(0.0, -0.42, 0.0), Vec3::new(0.14, 0.84, 0.16));
                        part(l, s, &mats.black, Vec3::new(0.0, -0.84, -0.04), Vec3::new(0.14, 0.1, 0.25));
                    })
                    .id();
                if slot == 0 {
                    l_leg = leg;
                } else {
                    r_leg = leg;
                }
            }
            for (side, slot) in [(-1.0f32, 0), (1.0, 1)] {
                let arm = b
                    .spawn((Transform::from_xyz(side * 0.29, 1.43, 0.0), Visibility::default()))
                    .with_children(|a| {
                        part(a, s, &limbs, Vec3::new(0.0, -0.29, 0.0), Vec3::new(0.11, 0.6, 0.12));
                        part(a, s, &mats.black, Vec3::new(0.0, -0.62, 0.0), Vec3::new(0.08, 0.09, 0.09));
                        if slot == 1 {
                            if let HumanStyle::Guard { rifle } = style {
                                let w = if *rifle { WeaponKind::Rifle } else { WeaponKind::Pistol };
                                let rot = Quat::from_rotation_x(-FRAC_PI_2) * Quat::from_rotation_z(PI);
                                hand = Some(a.spawn((Transform::from_xyz(0.0, -0.64, -0.02).with_rotation(rot), Visibility::default())).with_children(|g| spawn_parts(g, &weapon_parts(w, mats, s))).id());
                            }
                            if agent_team.is_some() {
                                // empty hand mount: the avatar system fills it with the held weapon
                                let rot = Quat::from_rotation_x(-FRAC_PI_2) * Quat::from_rotation_z(PI);
                                hand = Some(a.spawn((Transform::from_xyz(0.0, -0.64, -0.02).with_rotation(rot), Visibility::default(), HandMount)).id());
                            }
                        }
                        if slot == 0 {
                            if let Some(t) = agent_team {
                                part(a, s, &mats.team_glow[t], Vec3::new(0.0, -0.16, 0.0), Vec3::new(0.115, 0.03, 0.125));
                            }
                        }
                    })
                    .id();
                if slot == 0 {
                    l_arm = arm;
                } else {
                    r_arm = arm;
                }
            }
        })
        .id();
    commands.entity(root).add_child(body);
    (root, Rig { body, head, l_leg, r_leg, l_arm, r_arm, visor, light: None, hand, phase: 0.0, down: 0.0, visor_on: true })
}

pub fn spawn_npcs(commands: &mut Commands, lvl: &Level, mats: &Mats, shapes: &Shapes, settings: &Settings, phys: &mut Physics) {
    let hi = matches!(settings.quality, Quality::High | Quality::Ultra);
    for g in &lvl.guards {
        let pos = Level::center(g.x, g.z);
        let (root, mut rig) = spawn_humanoid(commands, &HumanStyle::Guard { rifle: g.rifle }, mats, shapes, Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(g.yaw)));
        if g.flashlight {
            let lp = Vec3::new(0.18, 1.35, -0.25);
            let mut e = commands.spawn((
                SpotLight {
                    intensity: 170_000.0,
                    range: 20.0,
                    radius: 0.05,
                    color: Color::srgb(0.95, 0.97, 1.0),
                    outer_angle: 0.36,
                    inner_angle: 0.2,
                    shadows_enabled: settings.quality == Quality::Ultra,
                    ..default()
                },
                Transform::from_translation(lp).looking_to(Vec3::new(0.0, -0.2, -1.0).normalize(), Vec3::Y),
            ));
            if hi {
                e.insert(VolumetricLight);
            }
            let le = e.id();
            commands.entity(root).add_child(le);
            rig.light = Some(le);
        }
        let route: Vec<Vec3> = g.route.iter().map(|&(x, z)| Level::center(x, z)).collect();
        let patrol = route.len() > 1;
        commands.entity(root).insert((
            Guard {
                id: g.id,
                rifle: g.rifle,
                hp: if g.rifle { 120.0 } else { 80.0 },
                stun: 0.0,
                state: if patrol { GState::Patrol } else { GState::Sentry },
                pos,
                yaw: g.yaw,
                route,
                route_i: 0,
                path: vec![],
                path_i: 0,
                goal: None,
                wait: 1.0 + (g.id as f32 * 0.7) % 2.0,
                home_yaw: g.yaw,
                suspicion: 0.0,
                target: None,
                last_seen: pos,
                state_t: 0.0,
                lost_t: 0.0,
                fire_cd: 0.0,
                burst: 0,
                tranq: 0.0,
                flashlight: g.flashlight,
                bark_cd: 0.0,
                speed: 0.0,
                aiming: false,
                stagger: 0.0,
                search_t: 0.0,
                run: false,
            },
            rig,
            Interactable { kind: Interact::Body(g.id), aabb: Aabb::centered(pos, Vec3::splat(0.4)) },
        ));
    }

    for c in &lvl.cameras {
        let face = yaw_dir(c.yaw);
        let mount = Level::center(c.x, c.z) - face * (CELL * 0.5 - 0.08) + Vec3::Y * 3.25;
        let lens = mount + face * 0.28;
        let mut head = Entity::PLACEHOLDER;
        let mut led = Entity::PLACEHOLDER;
        let mut cone = Entity::PLACEHOLDER;
        let root = commands
            .spawn((Transform::from_translation(mount).with_rotation(Quat::from_rotation_y(c.yaw)), Visibility::default(), LevelEntity))
            .with_children(|p| {
                p.spawn((Mesh3d(shapes.cube.clone()), MeshMaterial3d(mats.gun_dark.clone()), Transform::from_xyz(0.0, 0.0, -0.06).with_scale(Vec3::new(0.06, 0.06, 0.14))));
                head = p
                    .spawn((Transform::from_xyz(0.0, -0.02, -0.16), Visibility::default()))
                    .with_children(|h| {
                        h.spawn((Mesh3d(shapes.cube.clone()), MeshMaterial3d(mats.white.clone()), Transform::from_xyz(0.0, 0.0, -0.1).with_scale(Vec3::new(0.16, 0.14, 0.32))));
                        h.spawn((Mesh3d(shapes.cube.clone()), MeshMaterial3d(mats.gun_dark.clone()), Transform::from_xyz(0.0, 0.08, -0.12).with_scale(Vec3::new(0.2, 0.02, 0.4))));
                        h.spawn((
                            Mesh3d(shapes.cylinder.clone()),
                            MeshMaterial3d(mats.shades.clone()),
                            Transform::from_xyz(0.0, 0.0, -0.27).with_rotation(Quat::from_rotation_x(FRAC_PI_2)).with_scale(Vec3::new(0.09, 0.03, 0.09)),
                        ));
                        led = h
                            .spawn((Mesh3d(shapes.sphere_lo.clone()), MeshMaterial3d(mats.led_green.clone()), Transform::from_xyz(0.05, 0.03, -0.27).with_scale(Vec3::splat(0.022)), NotShadowCaster))
                            .id();
                        let mut ce = h.spawn((
                            SpotLight { intensity: 9_000.0, range: 16.0, color: Color::srgb(0.3, 1.0, 0.4), outer_angle: 0.45, inner_angle: 0.38, shadows_enabled: false, ..default() },
                            Transform::from_xyz(0.0, 0.0, -0.3),
                        ));
                        if hi {
                            ce.insert(VolumetricLight);
                        }
                        cone = ce.id();
                    })
                    .id();
            })
            .id();
        commands.entity(root).insert(SecCam {
            id: c.id,
            base_yaw: c.yaw,
            yaw: c.yaw,
            pitch: -0.38,
            status: 1,
            hp: 25.0,
            meter: 0.0,
            head,
            led,
            cone,
            pos: lens,
            cooldown: 0.0,
            phase: c.id as f32 * 2.1,
        });
    }

    for t in &lvl.turrets {
        let c = Level::center(t.x, t.z);
        let top = Vec3::new(c.x, WALL_H, c.z);
        let mut head = Entity::PLACEHOLDER;
        let mut led = Entity::PLACEHOLDER;
        let mut flash = Entity::PLACEHOLDER;
        let root = commands
            .spawn((Transform::from_translation(top), Visibility::default(), LevelEntity))
            .with_children(|p| {
                p.spawn((Mesh3d(shapes.cylinder.clone()), MeshMaterial3d(mats.gun_dark.clone()), Transform::from_xyz(0.0, -0.15, 0.0).with_scale(Vec3::new(0.12, 0.3, 0.12))));
                p.spawn((Mesh3d(shapes.cylinder.clone()), MeshMaterial3d(mats.gunmetal.clone()), Transform::from_xyz(0.0, -0.02, 0.0).with_scale(Vec3::new(0.5, 0.04, 0.5))));
                head = p
                    .spawn((Transform::from_xyz(0.0, -0.42, 0.0).with_rotation(Quat::from_rotation_y(t.yaw)), Visibility::default()))
                    .with_children(|h| {
                        h.spawn((Mesh3d(shapes.sphere.clone()), MeshMaterial3d(mats.gunmetal.clone()), Transform::from_scale(Vec3::new(0.36, 0.3, 0.4))));
                        for sx in [-0.07f32, 0.07] {
                            h.spawn((
                                Mesh3d(shapes.cylinder.clone()),
                                MeshMaterial3d(mats.gun_dark.clone()),
                                Transform::from_xyz(sx, -0.02, -0.3).with_rotation(Quat::from_rotation_x(FRAC_PI_2)).with_scale(Vec3::new(0.035, 0.38, 0.035)),
                            ));
                        }
                        led = h
                            .spawn((Mesh3d(shapes.sphere_lo.clone()), MeshMaterial3d(mats.led_off.clone()), Transform::from_xyz(0.0, 0.08, -0.17).with_scale(Vec3::splat(0.04)), NotShadowCaster))
                            .id();
                        flash = h
                            .spawn((PointLight { intensity: 0.0, range: 8.0, color: Color::srgb(1.0, 0.75, 0.4), shadows_enabled: false, ..default() }, Transform::from_xyz(0.0, 0.0, -0.55)))
                            .id();
                    })
                    .id();
            })
            .id();
        commands.entity(root).insert(Turret {
            id: t.id,
            yaw: t.yaw,
            pitch: 0.0,
            status: 0,
            hp: 70.0,
            always: t.always,
            head,
            led,
            flash,
            fire_cd: 0.0,
            pos: top - Vec3::Y * 0.42,
            flash_t: 0.0,
        });
    }

    for n in &lvl.npcs {
        let pos = Level::center(n.x, n.z);
        let (root, rig) = spawn_humanoid(commands, &HumanStyle::Informant, mats, shapes, Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(n.yaw)));
        commands.entity(root).insert((
            Civilian { id: n.id, pos, yaw: n.yaw },
            rig,
            Interactable { kind: Interact::Npc(n.id), aabb: Aabb::centered(pos + Vec3::Y * 0.9, Vec3::new(0.4, 0.9, 0.4)) },
        ));
        phys.props.push(Aabb::centered(pos + Vec3::Y * 0.9, Vec3::new(0.25, 0.9, 0.25)));
    }
}

// ------------------------------------------------------------------ pathfinding

fn walkable(level: &Level, closed: &HashSet<(i32, i32)>, x: i32, z: i32) -> bool {
    walkable_ex(level, closed, x, z, false)
}

fn walkable_ex(level: &Level, closed: &HashSet<(i32, i32)>, x: i32, z: i32, vents: bool) -> bool {
    let ok = if vents { level.nav_walkable_vents(x, z) } else { level.nav_walkable(x, z) };
    ok && !closed.contains(&(x, z))
}

fn line_clear(level: &Level, closed: &HashSet<(i32, i32)>, a: Vec3, b: Vec3, vents: bool) -> bool {
    let d = b - a;
    let len = Vec2::new(d.x, d.z).length();
    if len < 0.01 {
        return true;
    }
    let dir = Vec3::new(d.x, 0.0, d.z) / len;
    let side = Vec3::new(-dir.z, 0.0, dir.x) * 0.3;
    let steps = (len / 0.35).ceil() as i32;
    for i in 0..=steps {
        let p = a + d * (i as f32 / steps as f32);
        for o in [Vec3::ZERO, side, -side] {
            let (x, z) = Level::cell_of(p + o);
            if !walkable_ex(level, closed, x, z, vents) {
                return false;
            }
        }
    }
    true
}

pub fn find_path(level: &Level, closed: &HashSet<(i32, i32)>, from: Vec3, to: Vec3) -> Option<Vec<Vec3>> {
    find_path_ex(level, closed, from, to, false)
}

/// A* over the cell grid. `vents`: crawl ducts are walkable too (team-match bots).
pub fn find_path_ex(level: &Level, closed: &HashSet<(i32, i32)>, from: Vec3, to: Vec3, vents: bool) -> Option<Vec<Vec3>> {
    let walkable = |level: &Level, closed: &HashSet<(i32, i32)>, x: i32, z: i32| walkable_ex(level, closed, x, z, vents);
    let (sx, sz) = Level::cell_of(from);
    let (mut gx, mut gz) = Level::cell_of(to);
    let mut exact = true;
    if !walkable(level, closed, gx, gz) {
        exact = false;
        let mut best: Option<(f32, (i32, i32))> = None;
        for dz in -2..=2 {
            for dx in -2..=2 {
                let (x, z) = (gx + dx, gz + dz);
                if walkable(level, closed, x, z) {
                    let d = (Level::center(x, z) - to).length();
                    if best.map(|b| d < b.0).unwrap_or(true) {
                        best = Some((d, (x, z)));
                    }
                }
            }
        }
        let (_, c) = best?;
        gx = c.0;
        gz = c.1;
    }
    if (sx, sz) == (gx, gz) {
        return Some(vec![if exact { to } else { Level::center(gx, gz) }]);
    }
    let w = level.w;
    let n = (level.w * level.h) as usize;
    let mut g = vec![f32::INFINITY; n];
    let mut parent = vec![-1i32; n];
    let mut closed_set = vec![false; n];
    let idx = |x: i32, z: i32| (z * w + x) as usize;
    let hfn = |x: i32, z: i32| ((x - gx) as f32).hypot((z - gz) as f32);
    let mut heap = BinaryHeap::new();
    if !level.inb(sx, sz) {
        return None;
    }
    g[idx(sx, sz)] = 0.0;
    heap.push(Reverse(((hfn(sx, sz) * 1000.0) as u32, sx, sz)));
    let mut found = false;
    while let Some(Reverse((_, x, z))) = heap.pop() {
        let i = idx(x, z);
        if closed_set[i] {
            continue;
        }
        closed_set[i] = true;
        if (x, z) == (gx, gz) {
            found = true;
            break;
        }
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
            let (nx, nz) = (x + dx, z + dz);
            if !walkable(level, closed, nx, nz) {
                continue;
            }
            if dx != 0 && dz != 0 && (!walkable(level, closed, x + dx, z) || !walkable(level, closed, x, z + dz)) {
                continue;
            }
            let ni = idx(nx, nz);
            let cost = g[i] + if dx != 0 && dz != 0 { 1.414 } else { 1.0 };
            if cost < g[ni] {
                g[ni] = cost;
                parent[ni] = i as i32;
                heap.push(Reverse((((cost + hfn(nx, nz)) * 1000.0) as u32, nx, nz)));
            }
        }
    }
    if !found {
        return None;
    }
    let mut cells = vec![];
    let mut cur = idx(gx, gz) as i32;
    while cur >= 0 && cur as usize != idx(sx, sz) {
        let c = cur as usize;
        cells.push(Level::center(c as i32 % w, c as i32 / w));
        cur = parent[c];
    }
    cells.reverse();
    if exact {
        if let Some(last) = cells.last_mut() {
            *last = Vec3::new(to.x, 0.0, to.z);
        }
    }
    // string pulling
    let mut out = vec![];
    let mut anchor = from;
    let mut i = 0;
    while i < cells.len() {
        let mut j = cells.len() - 1;
        while j > i && !line_clear(level, closed, anchor, cells[j], vents) {
            j -= 1;
        }
        out.push(cells[j]);
        anchor = cells[j];
        i = j + 1;
    }
    Some(out)
}

fn closed_doors(doors: &Query<&mut Door>, level: &Level) -> HashSet<(i32, i32)> {
    let mut s = HashSet::new();
    for d in doors.iter() {
        if d.kind == DoorKind::Security && !d.open {
            if let Some(def) = level.doors.get(d.id as usize) {
                s.insert((def.x, def.z));
            }
        }
    }
    s
}

// ------------------------------------------------------------------ physics bodies / lights

fn npc_bodies(
    mut phys: ResMut<Physics>,
    mut guards: Query<(&Guard, &mut Interactable)>,
    cams: Query<&SecCam>,
    turrets: Query<&Turret>,
) {
    for (g, mut it) in guards.iter_mut() {
        if g.conscious() {
            let fwd = yaw_dir(g.yaw);
            let c = g.pos + Vec3::Y * 0.72;
            phys.targets.push((Aabb::centered(c, Vec3::new(0.26, 0.72, 0.26)), Body::Guard(g.id, false)));
            let h = g.pos + Vec3::Y * 1.68 + fwd * 0.02;
            phys.targets.push((Aabb::centered(h, Vec3::new(0.15, 0.17, 0.15)), Body::Guard(g.id, true)));
            it.aabb = Aabb::centered(c, Vec3::new(0.3, 0.8, 0.3));
        } else {
            let back = -yaw_dir(g.yaw);
            let c = g.pos + back * 0.85 + Vec3::Y * 0.2;
            phys.targets.push((Aabb::centered(c, Vec3::new(0.45, 0.2, 0.45)), Body::Guard(g.id, false)));
            it.aabb = Aabb::centered(c, Vec3::new(0.6, 0.35, 0.6));
        }
    }
    for c in cams.iter() {
        if c.status != 4 {
            phys.targets.push((Aabb::centered(c.pos - yaw_dir(c.yaw) * 0.12, Vec3::splat(0.17)), Body::Camera(c.id)));
        }
    }
    for t in turrets.iter() {
        if t.status != 4 {
            phys.targets.push((Aabb::centered(t.pos, Vec3::new(0.25, 0.2, 0.25)), Body::Turret(t.id)));
        }
    }
}

pub fn npc_lights(mut reg: ResMut<LightReg>, guards: Query<&Guard>) {
    reg.dynamic.clear();
    for g in guards.iter() {
        if g.flashlight && g.conscious() {
            let dir = (yaw_dir(g.yaw) + Vec3::new(0.0, -0.2, 0.0)).normalize();
            reg.dynamic.push(LightInfo { pos: g.pos + Vec3::Y * 1.35, dir, cos_outer: 0.36f32.cos(), range: 16.0, power: 1.3 });
        }
    }
}

// ------------------------------------------------------------------ authority: requests

#[allow(clippy::too_many_arguments)]
fn npc_requests(
    mut reqs: EventReader<AuthRequest>,
    mut guards: Query<&mut Guard>,
    mut cams: Query<&mut SecCam>,
    mut turrets: Query<&mut Turret>,
    players: Res<Players>,
    phys: Res<Physics>,
    mut state: ResMut<WorldState>,
    mut out: EventWriter<WorldEv>,
    mut snd: EventWriter<SoundEv>,
    mut rng: ResMut<GameRng>,
) {
    let mut alerts: Vec<(Vec3, f32, Vec3)> = vec![];
    for r in reqs.read() {
        let from_pos = players.map.get(&r.from).map(|p| p.net.pos()).unwrap_or(Vec3::ZERO);
        match &r.req {
            WorldRequest::HitNpc { id, dmg, kind, dir, head } => {
                let Some(mut g) = guards.iter_mut().find(|g| g.id == *id) else { continue };
                if g.state == GState::Dead {
                    continue;
                }
                let was_conscious = g.conscious();
                let unaware = matches!(g.state, GState::Patrol | GState::Sentry | GState::Return | GState::Unconscious) || (g.state == GState::Suspicious && g.suspicion < 0.6);
                let behind = yaw_dir(g.yaw).dot(Vec3::from_array(*dir).with_y(0.0).normalize_or_zero()) > 0.3;
                match kind {
                    DmgKind::Bullet => {
                        g.hp -= dmg * if *head { 3.2 } else { 1.0 };
                        if g.hp <= 0.0 {
                            g.set_state(GState::Dead);
                            state.stats.kills += 1;
                        } else if was_conscious {
                            g.stagger = 0.25;
                        }
                    }
                    DmgKind::Melee => {
                        if !was_conscious {
                            // hitting an unconscious body kills it eventually
                            g.hp -= dmg * 0.5;
                            if g.hp <= 0.0 {
                                g.set_state(GState::Dead);
                            }
                        } else if unaware && behind {
                            g.set_state(GState::Unconscious);
                            state.stats.knockouts += 1;
                        } else {
                            g.stun += dmg;
                            g.stagger = 0.35;
                            if g.stun >= if g.rifle { 90.0 } else { 65.0 } {
                                g.set_state(GState::Unconscious);
                                state.stats.knockouts += 1;
                            }
                        }
                    }
                    DmgKind::Tranq => {
                        if was_conscious {
                            if *head {
                                g.set_state(GState::Unconscious);
                                state.stats.knockouts += 1;
                            } else if g.tranq <= 0.0 {
                                g.tranq = 3.5;
                                out.write(WorldEv { ev: WorldEvent::Bark { npc: g.id, line: 5 }, quiet: false });
                            }
                        }
                    }
                }
                if g.conscious() && g.tranq <= 0.0 && g.state != GState::Combat {
                    g.set_state(GState::Combat);
                    g.target = Some(r.from);
                    g.last_seen = from_pos;
                    g.suspicion = 1.0;
                    g.yaw = dir_yaw(from_pos - g.pos);
                    alerts.push((g.pos, 15.0, from_pos));
                }
            }
            WorldRequest::HitCamera { id, dmg } => {
                if let Some(mut c) = cams.iter_mut().find(|c| c.id == *id) {
                    if c.status != 4 {
                        c.hp -= dmg;
                        if c.hp <= 0.0 {
                            c.status = 4;
                            snd.write(SoundEv::at(Sfx::Spark, c.pos, 0.8));
                        }
                    }
                }
            }
            WorldRequest::HitTurret { id, dmg } => {
                if let Some(mut t) = turrets.iter_mut().find(|t| t.id == *id) {
                    if t.status != 4 {
                        t.hp -= dmg;
                        if t.hp <= 0.0 {
                            t.status = 4;
                            snd.write(SoundEv::at(Sfx::Spark, t.pos, 0.9));
                        }
                    }
                }
            }
            WorldRequest::Noise { pos, radius } => {
                let p = Vec3::from_array(*pos);
                for mut g in guards.iter_mut() {
                    if !g.conscious() || g.state == GState::Combat || g.tranq > 0.0 {
                        continue;
                    }
                    let d = (g.pos - p).length();
                    let eff = if phys.los(g.pos + Vec3::Y * 1.6, p + Vec3::Y * 0.5) { *radius } else { radius * 0.55 };
                    if d > eff {
                        continue;
                    }
                    let loud = *radius >= 20.0;
                    if matches!(g.state, GState::Patrol | GState::Sentry | GState::Return | GState::Suspicious | GState::Search | GState::Investigate) {
                        let first = !matches!(g.state, GState::Investigate | GState::Search);
                        g.set_state(GState::Investigate);
                        g.last_seen = p;
                        g.run = loud;
                        g.suspicion = g.suspicion.max(0.5);
                        if first && g.bark_cd <= 0.0 {
                            g.bark_cd = 5.0;
                            out.write(WorldEv { ev: WorldEvent::Bark { npc: g.id, line: 7 }, quiet: false });
                        }
                    }
                }
            }
            WorldRequest::LootBody { id } => {
                if state.looted.contains(id) {
                    continue;
                }
                if let Some(g) = guards.iter().find(|g| g.id == *id) {
                    if !g.conscious() {
                        let (pistol, rifle) = if g.rifle { (0, 12 + rng.0.below(10) as u16) } else { (5 + rng.0.below(6) as u16, 0) };
                        let darts = if rng.0.chance(0.3) { 1 } else { 0 };
                        out.write(WorldEv { ev: WorldEvent::BodyLooted { id: *id, by: r.from, pistol, rifle, darts }, quiet: false });
                    }
                }
            }
            _ => {}
        }
    }
    apply_alerts(&mut guards, &alerts, &phys);
}

fn apply_alerts(guards: &mut Query<&mut Guard>, alerts: &[(Vec3, f32, Vec3)], phys: &Physics) {
    for (center, radius, target) in alerts {
        for mut g in guards.iter_mut() {
            if !g.conscious() || g.state == GState::Combat || g.tranq > 0.0 {
                continue;
            }
            if (g.pos - *center).length() > *radius {
                continue;
            }
            let sees = phys.los(g.pos + Vec3::Y * 1.6, *target + Vec3::Y * 1.0);
            if sees {
                g.set_state(GState::Combat);
                g.suspicion = 1.0;
            } else {
                g.set_state(GState::Investigate);
                g.run = true;
                g.suspicion = 0.7;
            }
            g.last_seen = *target;
        }
    }
}

// ------------------------------------------------------------------ authority: guard AI

struct Seen {
    id: u8,
    rate: f32,
    chest: Vec3,
    vis: f32,
    moving: bool,
    crouch: bool,
}

#[allow(clippy::too_many_arguments)]
fn guard_ai(
    time: Res<Time>,
    level: Res<Level>,
    phys: Res<Physics>,
    players: Res<Players>,
    local: Res<LocalId>,
    mut guards: Query<&mut Guard>,
    mut doors: Query<&mut Door>,
    mut shared: ResMut<AiShared>,
    lights: Query<&LampLight>,
    mut out: EventWriter<WorldEv>,
    mut dmg: EventWriter<DamageEv>,
    mut fx: EventWriter<FxEv>,
    mut snd: EventWriter<SoundEv>,
    mut rng: ResMut<GameRng>,
    mut state: ResMut<WorldState>,
) {
    let dt = time.delta_secs().min(0.05);
    shared.heightened = (shared.heightened - dt).max(0.0);
    let closed = closed_doors(&doors, &level);
    let bodies: Vec<(u16, Vec3)> = guards.iter().filter(|g| !g.conscious()).map(|g| (g.id, g.pos)).collect();
    let broken_lights: Vec<Vec3> = lights.iter().filter(|l| l.broken || (!l.on && l.kind != LightKind::Alarm)).map(|l| l.pos).collect();
    let mut alerts: Vec<(Vec3, f32, Vec3)> = vec![];
    let mut barks: Vec<(u16, u8)> = vec![];

    for mut g in guards.iter_mut() {
        g.bark_cd -= dt;
        g.stagger = (g.stagger - dt).max(0.0);
        g.state_t += dt;
        g.aiming = false;
        if !g.conscious() {
            g.speed = 0.0;
            continue;
        }
        if g.tranq > 0.0 {
            g.tranq -= dt;
            g.speed = 0.0;
            if g.tranq <= 0.0 {
                g.set_state(GState::Unconscious);
                state.stats.knockouts += 1;
            }
            continue;
        }
        let eye = g.pos + Vec3::Y * 1.62;
        let fwd = yaw_dir(g.yaw);

        // ---- perception
        let mut best: Option<Seen> = None;
        for (pid, p) in players.map.iter() {
            if p.net.has(pflags::DOWNED) || p.net.has(pflags::CLOAK) {
                continue;
            }
            let crouch = p.net.has(pflags::CROUCH);
            let chest = p.net.pos() + Vec3::Y * if crouch { 0.7 } else { 1.15 };
            let to = chest - eye;
            let dist = to.length();
            if dist > 32.0 {
                continue;
            }
            let flat = Vec3::new(to.x, 0.0, to.z).normalize_or_zero();
            let fov_cos = if g.state == GState::Combat { 0.2 } else { 0.55 };
            if flat.dot(fwd) < fov_cos && dist > 1.4 {
                continue;
            }
            if !phys.los(eye, chest) && !phys.los(eye, p.net.pos() + Vec3::Y * if crouch { 0.95 } else { 1.65 }) {
                continue;
            }
            let vis = p.visibility;
            let mut vf = (vis * 1.5).clamp(0.02, 1.0);
            if dist < 2.0 {
                vf = vf.max(0.7);
            }
            let df = (1.25 - dist / 24.0).clamp(0.12, 1.0);
            let mv = if p.net.has(pflags::SPRINT) { 1.6 } else if p.net.has(pflags::MOVING) { 1.25 } else { 1.0 };
            let cr = if crouch { 0.7 } else { 1.0 };
            let alert_k = match g.state {
                GState::Combat => 4.0,
                GState::Search => 2.2,
                GState::Suspicious | GState::Investigate => 1.5,
                _ => 1.0,
            } * if shared.heightened > 0.0 { 1.3 } else { 1.0 };
            let rate = 1.5 * vf * df * mv * cr * alert_k;
            if best.as_ref().map(|b| rate > b.rate).unwrap_or(true) {
                best = Some(Seen { id: *pid, rate, chest, vis, moving: p.net.has(pflags::MOVING), crouch });
            }
        }
        let sees = best.is_some();
        if let Some(s) = &best {
            g.suspicion = (g.suspicion + s.rate * dt).min(1.2);
            g.last_seen = s.chest.with_y(0.0);
            g.target = Some(s.id);
            g.lost_t = 0.0;
        } else {
            let decay = match g.state {
                GState::Search | GState::Combat => 0.06,
                _ => 0.14,
            };
            g.suspicion = (g.suspicion - decay * dt).max(0.0);
            g.lost_t += dt;
        }

        // ---- bodies and dark rooms
        if matches!(g.state, GState::Patrol | GState::Sentry | GState::Return | GState::Search | GState::Investigate) {
            for (bid, bpos) in &bodies {
                if shared.discovered.contains(bid) {
                    continue;
                }
                let to = *bpos + Vec3::Y * 0.3 - eye;
                let d = to.length();
                if d < 14.0 && Vec3::new(to.x, 0.0, to.z).normalize_or_zero().dot(fwd) > 0.4 && phys.los(eye, *bpos + Vec3::Y * 0.3) {
                    shared.discovered.insert(*bid);
                    shared.heightened = 90.0;
                    g.set_state(GState::Investigate);
                    g.last_seen = *bpos;
                    g.run = true;
                    g.suspicion = 0.8;
                    barks.push((g.id, 3));
                    alerts.push((g.pos, 20.0, *bpos));
                    break;
                }
            }
            if g.state == GState::Patrol && g.bark_cd <= 0.0 {
                for lp in &broken_lights {
                    if (lp.with_y(0.0) - g.pos).length() < 5.0 && level.room_at(*lp) == level.room_at(g.pos) && rng.0.chance(dt * 0.3) {
                        g.bark_cd = 30.0;
                        barks.push((g.id, 8));
                        shared.heightened = shared.heightened.max(30.0);
                    }
                }
            }
        }

        // ---- state machine
        let speed_walk = 1.5;
        let speed_run = 3.7;
        let mut move_to: Option<(Vec3, f32)> = None;
        let mut face: Option<Vec3> = None;
        match g.state {
            GState::Patrol | GState::Sentry | GState::Return => {
                if g.suspicion >= 1.0 {
                    g.set_state(GState::Combat);
                    barks.push((g.id, 1));
                    alerts.push((g.pos, 18.0, g.last_seen));
                } else if g.suspicion >= 0.35 {
                    g.set_state(GState::Suspicious);
                    if g.bark_cd <= 0.0 {
                        g.bark_cd = 6.0;
                        barks.push((g.id, 0));
                    }
                } else if g.state == GState::Return {
                    let home = g.route.get(g.route_i).copied().unwrap_or(g.pos);
                    if (g.pos - home).length() < 0.4 {
                        let next = if g.route.len() > 1 { GState::Patrol } else { GState::Sentry };
                        g.set_state(next);
                    } else {
                        move_to = Some((home, speed_walk));
                    }
                } else if g.state == GState::Patrol {
                    let tgt = g.route[g.route_i];
                    if (g.pos - tgt).length() < 0.35 {
                        g.wait -= dt;
                        if g.wait <= 0.0 {
                            g.route_i = (g.route_i + 1) % g.route.len();
                            g.wait = rng.0.range(1.5, 4.0);
                            g.path.clear();
                        } else if g.wait < 1.2 {
                            let t = time.elapsed_secs() + g.id as f32;
                            g.yaw += (t * 1.3).sin() * dt * 0.8;
                        }
                    } else {
                        move_to = Some((tgt, speed_walk));
                    }
                } else {
                    // sentry: slowly scan
                    let home = g.route.first().copied().unwrap_or(g.pos);
                    if (g.pos - home).length() > 0.5 {
                        move_to = Some((home, speed_walk));
                    } else {
                        let t = time.elapsed_secs() * 0.35 + g.id as f32;
                        let want = g.home_yaw + t.sin() * 0.7;
                        g.yaw = approach_angle(g.yaw, want, dt * 0.8);
                    }
                }
            }
            GState::Suspicious => {
                face = Some(g.last_seen);
                if g.suspicion >= 1.0 {
                    g.set_state(GState::Combat);
                    barks.push((g.id, 1));
                    alerts.push((g.pos, 18.0, g.last_seen));
                } else if g.suspicion < 0.12 {
                    g.set_state(GState::Return);
                    if g.bark_cd <= 0.0 {
                        g.bark_cd = 6.0;
                        barks.push((g.id, 2));
                    }
                } else if g.state_t > 2.5 {
                    g.set_state(GState::Investigate);
                    g.run = false;
                }
            }
            GState::Investigate => {
                if g.suspicion >= 1.0 {
                    g.set_state(GState::Combat);
                    barks.push((g.id, 1));
                    alerts.push((g.pos, 18.0, g.last_seen));
                } else if (g.pos - g.last_seen).length() < 1.0 || (g.state_t > 2.0 && g.goal.is_some() && g.path.is_empty()) {
                    g.wait -= dt;
                    let t = time.elapsed_secs() + g.id as f32;
                    g.yaw += (t * 0.9).sin() * dt * 1.5;
                    if g.state_t > 25.0 || g.wait < -4.0 {
                        g.wait = 0.0;
                        g.set_state(GState::Return);
                        barks.push((g.id, 6));
                    }
                } else {
                    let ls = g.last_seen;
                    move_to = Some((ls, if g.run { speed_run } else { 2.2 }));
                    if g.state_t > 30.0 {
                        g.set_state(GState::Return);
                    }
                }
            }
            GState::Combat => {
                if let (true, Some(s)) = (sees, &best) {
                    face = Some(s.chest);
                    g.aiming = true;
                    let dist = (s.chest - eye).length();
                    if dist > 13.0 {
                        move_to = Some((s.chest.with_y(0.0), speed_run));
                    }
                    let to = (s.chest - eye).normalize_or_zero();
                    let facing = yaw_dir(g.yaw).dot(Vec3::new(to.x, 0.0, to.z).normalize_or_zero()) > 0.95;
                    g.fire_cd -= dt;
                    if facing && g.fire_cd <= 0.0 && g.stagger <= 0.0 {
                        let mut acc = (0.78 - dist * 0.013) * (0.5 + 0.5 * (s.vis * 1.6).min(1.0));
                        if s.moving {
                            acc *= 0.75;
                        }
                        if s.crouch {
                            acc *= 0.9;
                        }
                        acc = acc.clamp(0.1, 0.85);
                        let right = Vec3::new(g.yaw.cos(), 0.0, -g.yaw.sin());
                        let muzzle = g.pos + Vec3::Y * 1.4 + yaw_dir(g.yaw) * 0.55 + right * 0.2;
                        let hit = rng.0.chance(acc);
                        let (end, imp) = if hit {
                            (s.chest + rng.0.unit_sphere() * 0.15, impact::FLESH)
                        } else {
                            let aim = (s.chest + rng.0.unit_sphere() * 0.9 - muzzle).normalize_or_zero();
                            match phys.raycast(muzzle, aim, 60.0, false, false, None) {
                                Some(h) => (h.pos, impact::WALL),
                                None => (muzzle + aim * 60.0, impact::NONE),
                            }
                        };
                        if hit && s.id == local.0 {
                            dmg.write(DamageEv { amount: if g.rifle { 7.0 } else { 10.0 }, from: muzzle, by: NO_PLAYER, weapon: if g.rifle { 11 } else { 10 } });
                        }
                        fx.write(FxEv { fx: Fx::Shot { from: a3(muzzle), to: a3(end), weapon: if g.rifle { 11 } else { 10 }, impact: imp, normal: [0.0, 1.0, 0.0] }, remote: false });
                        snd.write(SoundEv::at(Sfx::GuardShot, muzzle, 0.9));
                        alerts.push((g.pos, 22.0, g.last_seen));
                        if g.rifle {
                            g.burst += 1;
                            if g.burst >= 3 {
                                g.burst = 0;
                                g.fire_cd = rng.0.range(0.9, 1.4);
                            } else {
                                g.fire_cd = 0.12;
                            }
                        } else {
                            g.fire_cd = rng.0.range(0.6, 1.0);
                        }
                    }
                } else {
                    g.fire_cd = g.fire_cd.max(0.4);
                    let ls = g.last_seen;
                    if (g.pos - ls).length() > 1.0 && g.lost_t < 12.0 {
                        move_to = Some((ls, speed_run));
                    } else if g.lost_t > 1.5 {
                        g.set_state(GState::Search);
                        g.search_t = 0.0;
                        barks.push((g.id, 4));
                    }
                }
            }
            GState::Search => {
                if g.suspicion >= 0.65 && sees {
                    g.set_state(GState::Combat);
                    barks.push((g.id, 1));
                } else if g.state_t > 22.0 {
                    g.set_state(GState::Return);
                    barks.push((g.id, 6));
                } else {
                    g.search_t -= dt;
                    if g.search_t <= 0.0 || g.goal.is_none() {
                        g.search_t = rng.0.range(3.0, 6.0);
                        let (cx, cz) = Level::cell_of(g.last_seen);
                        for _ in 0..10 {
                            let x = cx + rng.0.below(9) as i32 - 4;
                            let z = cz + rng.0.below(9) as i32 - 4;
                            if walkable(&level, &closed, x, z) {
                                g.goal = Some(Level::center(x, z));
                                g.path.clear();
                                break;
                            }
                        }
                    }
                    if let Some(goal) = g.goal {
                        if (g.pos - goal).length() > 0.5 {
                            move_to = Some((goal, 2.4));
                        }
                    }
                }
            }
            GState::Unconscious | GState::Dead => {}
        }

        // ---- movement along a path
        g.speed = 0.0;
        if let Some((goal, speed)) = move_to {
            let need_path = g.goal.map(|og| (og - goal).length() > 1.0).unwrap_or(true) || g.path_i >= g.path.len();
            if need_path {
                match find_path(&level, &closed, g.pos, goal) {
                    Some(p) => {
                        g.path = p;
                        g.path_i = 0;
                    }
                    None => {
                        g.path.clear();
                        g.path_i = 0;
                    }
                }
                g.goal = Some(goal);
            }
            if g.path_i < g.path.len() {
                let wp = g.path[g.path_i];
                let to = Vec3::new(wp.x - g.pos.x, 0.0, wp.z - g.pos.z);
                let d = to.length();
                // door handling: open closed doors on the way
                let mut blocked = false;
                let ahead = g.pos + to.normalize_or_zero() * 1.2;
                for cell in [Level::cell_of(ahead), Level::cell_of(g.pos)] {
                    if let Some(def) = level.door_at(cell.0, cell.1) {
                        if let Some(mut door) = doors.iter_mut().find(|dd| dd.id == def.id) {
                            if door.kind == DoorKind::Grate {
                                continue;
                            }
                            if !door.open && door.kind != DoorKind::Security {
                                out.write(WorldEv { ev: WorldEvent::Door { id: door.id, open: true, locked: door.locked }, quiet: false });
                                door.npc_timer = 4.0;
                            }
                            if door.t < 0.6 && door.kind != DoorKind::Grate {
                                blocked = !door.open || door.t < 0.6;
                            }
                            if door.open {
                                door.npc_timer = door.npc_timer.max(3.0);
                            }
                        }
                    }
                }
                // don't walk through players
                let near_player = players.map.values().any(|p| !p.net.has(pflags::DOWNED) && (p.net.pos() - (g.pos + to.normalize_or_zero() * 0.6)).length() < 0.5);
                if d < 0.25 {
                    g.path_i += 1;
                } else if !blocked && !near_player {
                    let step = (speed * dt).min(d);
                    let dir = to / d;
                    g.pos += dir * step;
                    g.speed = speed;
                    if face.is_none() {
                        g.yaw = approach_angle(g.yaw, dir_yaw(dir), dt * 6.0);
                    }
                }
            }
        } else {
            g.path.clear();
            g.goal = None;
        }
        if let Some(f) = face {
            let d = f - g.pos;
            if Vec2::new(d.x, d.z).length() > 0.1 {
                g.yaw = approach_angle(g.yaw, dir_yaw(d), dt * 5.0);
            }
        }
    }
    apply_alerts(&mut guards, &alerts, &phys);
    for (id, line) in barks {
        out.write(WorldEv { ev: WorldEvent::Bark { npc: id, line }, quiet: false });
    }
}

// ------------------------------------------------------------------ authority: cameras & turrets

#[allow(clippy::too_many_arguments)]
fn camera_ai(time: Res<Time>, mut cams: Query<&mut SecCam>, players: Res<Players>, phys: Res<Physics>, state: Res<WorldState>, mut req: EventWriter<WorldRequest>) {
    let dt = time.delta_secs();
    let t = time.elapsed_secs();
    for mut c in cams.iter_mut() {
        if c.status == 4 {
            continue;
        }
        if !state.cameras_on {
            c.status = 0;
            c.meter = 0.0;
            continue;
        }
        c.cooldown -= dt;
        let fwd = look_dir(c.yaw, c.pitch);
        let mut seen: Option<(Vec3, f32)> = None;
        for p in players.map.values() {
            if p.net.has(pflags::DOWNED) || p.net.has(pflags::RADAR) {
                continue;
            }
            let chest = p.net.pos() + Vec3::Y * if p.net.has(pflags::CROUCH) { 0.6 } else { 1.1 };
            let to = chest - c.pos;
            let dist = to.length();
            if dist > 22.0 || to.normalize_or_zero().dot(fwd) < 0.82 {
                continue;
            }
            if !phys.los(c.pos + fwd * 0.2, chest) {
                continue;
            }
            let rate = 1.1 * (p.visibility * 1.8).clamp(0.25, 1.0) * (1.2 - dist / 30.0).clamp(0.3, 1.0);
            seen = Some((chest, rate));
        }
        match seen {
            Some((chest, rate)) => {
                c.meter = (c.meter + rate * dt).min(1.2);
                let d = chest - c.pos;
                let want_yaw = dir_yaw(d);
                c.yaw = approach_angle(c.yaw, want_yaw, dt * 1.5);
                if c.meter >= 1.0 {
                    c.status = 3;
                    if c.cooldown <= 0.0 {
                        c.cooldown = 8.0;
                        req.write(WorldRequest::TriggerAlarm { pos: a3(chest) });
                    }
                } else {
                    c.status = 2;
                }
            }
            None => {
                c.meter = (c.meter - 0.35 * dt).max(0.0);
                if c.meter <= 0.0 {
                    c.status = if state.alarm { 3 } else { 1 };
                    let want = c.base_yaw + (t * TAU / 10.0 + c.phase).sin() * 0.85;
                    c.yaw = approach_angle(c.yaw, want, dt * 0.6);
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn turret_ai(
    time: Res<Time>,
    mut turrets: Query<&mut Turret>,
    mut guards: Query<&mut Guard>,
    players: Res<Players>,
    local: Res<LocalId>,
    phys: Res<Physics>,
    state: Res<WorldState>,
    mut rng: ResMut<GameRng>,
    mut dmg: EventWriter<DamageEv>,
    mut fx: EventWriter<FxEv>,
    mut snd: EventWriter<SoundEv>,
) {
    let dt = time.delta_secs();
    for mut t in turrets.iter_mut() {
        t.flash_t -= dt;
        if t.status == 4 {
            continue;
        }
        let active = match state.turret_mode {
            0 => false,
            2 => true,
            _ => state.alarm || t.always,
        };
        if !active {
            t.status = 0;
            continue;
        }
        // pick a target
        let mut target: Option<(Vec3, Option<u8>, Option<u16>)> = None;
        let mut best_d = 26.0;
        if state.turret_mode == 2 {
            for g in guards.iter() {
                if !g.conscious() {
                    continue;
                }
                let c = g.pos + Vec3::Y * 1.1;
                let d = (c - t.pos).length();
                if d < best_d && phys.los(t.pos, c) {
                    best_d = d;
                    target = Some((c, None, Some(g.id)));
                }
            }
        } else {
            for (id, p) in players.map.iter() {
                if p.net.has(pflags::DOWNED) || p.net.has(pflags::RADAR) {
                    continue;
                }
                let c = p.net.pos() + Vec3::Y * if p.net.has(pflags::CROUCH) { 0.6 } else { 1.1 };
                let d = (c - t.pos).length();
                if d < best_d && phys.los(t.pos, c) {
                    best_d = d;
                    target = Some((c, Some(*id), None));
                }
            }
        }
        let Some((c, pid, gid)) = target else {
            t.status = 1;
            t.yaw += dt * 0.6;
            t.pitch *= 1.0 - dt;
            continue;
        };
        let d = c - t.pos;
        let want_yaw = dir_yaw(d);
        let want_pitch = (d.y / Vec2::new(d.x, d.z).length().max(0.01)).atan();
        t.yaw = approach_angle(t.yaw, want_yaw, dt * 2.6);
        t.pitch += (want_pitch - t.pitch) * (dt * 6.0).min(1.0);
        t.status = 3;
        t.fire_cd -= dt;
        if wrap_angle(t.yaw - want_yaw).abs() < 0.12 && t.fire_cd <= 0.0 {
            t.fire_cd = 0.11;
            t.flash_t = 0.05;
            let dir = look_dir(t.yaw, t.pitch);
            let muzzle = t.pos + dir * 0.55;
            let hit = rng.0.chance(0.38);
            let end = if hit {
                if let Some(id) = pid {
                    if id == local.0 {
                        dmg.write(DamageEv { amount: 6.0, from: muzzle, by: NO_PLAYER, weapon: 12 });
                    }
                }
                if let Some(gid) = gid {
                    if let Some(mut g) = guards.iter_mut().find(|g| g.id == gid) {
                        g.hp -= 9.0;
                        if g.hp <= 0.0 {
                            g.set_state(GState::Dead);
                        } else if g.state != GState::Combat {
                            g.set_state(GState::Search);
                            g.last_seen = t.pos.with_y(0.0);
                        }
                    }
                }
                c + rng.0.unit_sphere() * 0.2
            } else {
                let aim = (c + rng.0.unit_sphere() * 1.0 - muzzle).normalize_or_zero();
                phys.raycast(muzzle, aim, 40.0, false, false, None).map(|h| h.pos).unwrap_or(muzzle + aim * 40.0)
            };
            fx.write(FxEv {
                fx: Fx::Shot { from: a3(muzzle), to: a3(end), weapon: 12, impact: if hit { impact::FLESH } else { impact::WALL }, normal: [0.0, 1.0, 0.0] },
                remote: false,
            });
            snd.write(SoundEv::at(Sfx::TurretShot, muzzle, 0.8));
        }
    }
}

fn apply_barks(mut evs: EventReader<WorldEv>, guards: Query<&Guard>, mut subs: EventWriter<Subtitle>, mut snd: EventWriter<SoundEv>) {
    for WorldEv { ev, quiet } in evs.read() {
        if *quiet {
            continue;
        }
        if let WorldEvent::Bark { npc, line } = ev {
            if let Some(g) = guards.iter().find(|g| g.id == *npc) {
                subs.write(Subtitle { who: format!("Kade Security {}", g.id + 1), text: BARKS.get(*line as usize).unwrap_or(&"...").to_string() });
                snd.write(SoundEv::at(Sfx::Radio, g.pos + Vec3::Y * 1.6, 0.5));
            }
        }
    }
}

// ------------------------------------------------------------------ presentation

#[allow(clippy::type_complexity)]
fn animate_guards(
    time: Res<Time>,
    mut q: Query<(&Guard, &mut Transform, &mut Rig)>,
    mut parts: Query<&mut Transform, (Without<Guard>, Without<Civilian>)>,
    mut lights: Query<&mut SpotLight>,
    mut mats_q: Query<&mut MeshMaterial3d<StandardMaterial>>,
    mats: Res<Mats>,
) {
    let dt = time.delta_secs();
    let t = time.elapsed_secs();
    for (g, mut tf, mut rig) in q.iter_mut() {
        tf.translation = g.pos;
        tf.rotation = Quat::from_rotation_y(g.yaw);
        let anim = g.anim();
        let down = matches!(anim, 5 | 6);
        rig.down = if down { (rig.down + dt * 2.2).min(1.0) } else { (rig.down - dt * 2.0).max(0.0) };
        let walk = (g.speed / 1.6).min(1.5);
        rig.phase += g.speed * dt * 3.6;
        let swing = rig.phase.sin() * 0.55 * walk.min(1.0);
        if let Ok(mut b) = parts.get_mut(rig.body) {
            let e = rig.down * rig.down * (3.0 - 2.0 * rig.down);
            let wobble = if anim == 7 { (t * 7.0).sin() * 0.08 } else { 0.0 };
            b.rotation = Quat::from_rotation_x(e * FRAC_PI_2 + wobble);
            b.translation = Vec3::new(0.0, 0.13 * e, 0.0);
        }
        if let Ok(mut l) = parts.get_mut(rig.l_leg) {
            l.rotation = Quat::from_rotation_x(if down { 0.0 } else { swing });
        }
        if let Ok(mut r) = parts.get_mut(rig.r_leg) {
            r.rotation = Quat::from_rotation_x(if down { 0.0 } else { -swing });
        }
        let aiming = anim == 3;
        if let Ok(mut r) = parts.get_mut(rig.r_arm) {
            r.rotation = if aiming {
                Quat::from_rotation_x(1.5) * Quat::from_rotation_z(0.1)
            } else if down {
                Quat::from_rotation_z(0.3)
            } else {
                Quat::from_rotation_x(swing * 0.5 + 0.25)
            };
        }
        if let Ok(mut l) = parts.get_mut(rig.l_arm) {
            l.rotation = if aiming && g.rifle {
                Quat::from_rotation_x(1.35) * Quat::from_rotation_z(-0.55)
            } else if down {
                Quat::from_rotation_z(-0.3)
            } else {
                Quat::from_rotation_x(-swing * 0.5)
            };
        }
        if let Ok(mut h) = parts.get_mut(rig.head) {
            let look = if matches!(g.state, GState::Search | GState::Suspicious) || (g.state == GState::Patrol && g.speed < 0.1) { (t * 0.8 + g.id as f32).sin() * 0.5 } else { 0.0 };
            h.rotation = Quat::from_rotation_y(look);
        }
        if let Some(le) = rig.light {
            if let Ok(mut s) = lights.get_mut(le) {
                let want = if g.flashlight && g.conscious() { 170_000.0 } else { 0.0 };
                if (s.intensity - want).abs() > 1.0 {
                    s.intensity = want;
                }
            }
            if let Ok(mut lt) = parts.get_mut(le) {
                let pitch = if aiming { -0.05 } else { -0.22 };
                lt.rotation = Quat::from_rotation_x(pitch);
            }
        }
        let visor_should = g.conscious();
        if visor_should != rig.visor_on {
            rig.visor_on = visor_should;
            if let Some(v) = rig.visor {
                if let Ok(mut m) = mats_q.get_mut(v) {
                    m.0 = if visor_should { mats.guard_visor.clone() } else { mats.led_off.clone() };
                }
            }
        }
    }
}

#[allow(clippy::type_complexity)]
fn animate_devices(
    cams: Query<&SecCam>,
    turrets: Query<&Turret>,
    mut tfs: Query<&mut Transform, (Without<SecCam>, Without<Turret>)>,
    mut lights: Query<&mut SpotLight>,
    mut points: Query<&mut PointLight>,
    mut mats_q: Query<&mut MeshMaterial3d<StandardMaterial>>,
    mats: Res<Mats>,
    time: Res<Time>,
) {
    let blink = (time.elapsed_secs() * 4.0).sin() > 0.0;
    for c in cams.iter() {
        if let Ok(mut h) = tfs.get_mut(c.head) {
            let rel = wrap_angle(c.yaw - c.base_yaw);
            let broken_droop = if c.status == 4 { 0.6 } else { 0.0 };
            h.rotation = Quat::from_rotation_y(rel) * Quat::from_rotation_x(c.pitch - broken_droop);
        }
        let (mat, color, inten) = match c.status {
            0 | 4 => (mats.led_off.clone(), Color::BLACK, 0.0),
            2 => (mats.led_amber.clone(), Color::srgb(1.0, 0.7, 0.1), 14_000.0),
            3 => (if blink { mats.led_red.clone() } else { mats.led_off.clone() }, Color::srgb(1.0, 0.1, 0.05), 18_000.0),
            _ => (mats.led_green.clone(), Color::srgb(0.3, 1.0, 0.4), 9_000.0),
        };
        if let Ok(mut m) = mats_q.get_mut(c.led) {
            if m.0 != mat {
                m.0 = mat;
            }
        }
        if let Ok(mut s) = lights.get_mut(c.cone) {
            if (s.intensity - inten).abs() > 1.0 {
                s.intensity = inten;
            }
            s.color = color;
        }
    }
    for t in turrets.iter() {
        if let Ok(mut h) = tfs.get_mut(t.head) {
            let droop = if t.status == 4 { 0.7 } else { 0.0 };
            h.rotation = Quat::from_rotation_y(t.yaw) * Quat::from_rotation_x(t.pitch - droop);
        }
        let mat = match t.status {
            0 | 4 => mats.led_off.clone(),
            3 => mats.led_red.clone(),
            _ => mats.led_amber.clone(),
        };
        if let Ok(mut m) = mats_q.get_mut(t.led) {
            if m.0 != mat {
                m.0 = mat;
            }
        }
        if let Ok(mut p) = points.get_mut(t.flash) {
            p.intensity = if t.flash_t > 0.0 { 150_000.0 } else { 0.0 };
        }
    }
}

fn animate_civilians(time: Res<Time>, q: Query<(&Civilian, &Rig)>, mut parts: Query<&mut Transform, Without<Civilian>>) {
    let t = time.elapsed_secs();
    for (c, rig) in q.iter() {
        if let Ok(mut h) = parts.get_mut(rig.head) {
            h.rotation = Quat::from_rotation_y((t * 0.4 + c.id as f32).sin() * 0.6);
        }
        if let Ok(mut b) = parts.get_mut(rig.body) {
            b.translation.y = (t * 1.3).sin() * 0.006;
        }
        let _ = (c.pos, c.yaw);
    }
}
