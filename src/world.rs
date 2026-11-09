//! Level entities (doors, lights, pickups, terminals...), world state and mission flow.
//!
//! Gameplay never mutates shared world state directly: it sends a `WorldRequest`, the
//! authority (today: the local game, later: a co-op host) validates it and emits `WorldEv`s
//! that every peer applies. This keeps the door open for networking.
use bevy::pbr::{CascadeShadowConfigBuilder, FogVolume, NotShadowCaster, VolumetricLight};
use bevy::prelude::*;
use std::collections::HashSet;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

use crate::audio::*;
use crate::collision::*;
use crate::game::*;
use crate::items::*;
use crate::level::*;
use crate::materials::*;
use crate::util::*;

pub const LAMP_LM: f32 = 140_000.0;
pub const STREET_LM: f32 = 320_000.0;
pub const ALARM_LM: f32 = 220_000.0;

#[derive(Component)]
pub struct Door {
    pub id: u16,
    pub kind: DoorKind,
    pub open: bool,
    pub locked: bool,
    pub t: f32,
    pub along_x: bool,
    pub base: Vec3,
    pub size: Vec3,
    pub npc_timer: f32,
}

impl Door {
    pub fn offset(&self) -> Vec3 {
        if self.kind == DoorKind::Grate {
            Vec3::Y * (VENT_H - 0.04) * self.t
        } else if self.along_x {
            Vec3::X * CELL * 0.93 * self.t
        } else {
            Vec3::Z * CELL * 0.93 * self.t
        }
    }
    pub fn aabb(&self) -> Aabb {
        Aabb::centered(self.base + self.offset(), self.size * 0.5)
    }
}

#[derive(Component)]
pub struct DoorLed {
    pub id: u16,
}

#[derive(Component)]
pub struct LampLight {
    pub id: u16,
    pub kind: LightKind,
    pub on: bool,
    pub broken: bool,
    pub base: f32,
    pub room: u16,
    pub fixture: Entity,
    pub pos: Vec3,
    pub phase: f32,
    pub lit_visual: bool,
}

#[derive(Component)]
pub struct Pickup {
    pub id: u16,
    pub item: ItemKind,
    pub count: u16,
}

#[derive(Component)]
pub struct TerminalEnt {
    pub id: u16,
}

#[derive(Component)]
pub struct Spin(pub f32);

#[derive(Component)]
pub struct WaterSurface;

#[derive(Component)]
pub struct ExtractionFx;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Interact {
    Door(u16),
    Pickup(u16),
    Terminal(u16),
    Switch(u16),
    Npc(u16),
    Body(u16),
    /// team match: the stealable prototype
    Objective,
    /// team match: hackable terminal (0..3)
    HackTerm(u8),
}

#[derive(Component, Clone)]
pub struct Interactable {
    pub kind: Interact,
    pub aabb: Aabb,
}

#[derive(Clone, Copy)]
pub struct LightInfo {
    pub pos: Vec3,
    pub dir: Vec3,
    /// -2 = omni
    pub cos_outer: f32,
    pub range: f32,
    pub power: f32,
}

/// Every light that matters for stealth visibility.
#[derive(Resource, Default)]
pub struct LightReg {
    pub statics: Vec<LightInfo>,
    pub dynamic: Vec<LightInfo>,
}

#[derive(Resource, Default)]
pub struct Mission {
    pub t: f32,
    pub sent: HashSet<u16>,
    pub obj_sent: [bool; 4],
    pub alarm_t: f32,
}

#[derive(Resource, Default)]
pub struct InfolinkQueue(pub Vec<u16>);

#[derive(Resource, Default)]
pub struct LevelMeshCache {
    /// hash of the level the meshes were built for
    pub hash: u64,
    pub surfs: Vec<(Surf, Handle<Mesh>)>,
    pub glass: Option<Handle<Mesh>>,
    pub water: Option<Handle<Mesh>>,
}

pub const OBJECTIVES: [&str; 4] = [
    "Infiltrate the Halvorsen-Kade biolab",
    "Retrieve the LAZARUS data core from the lab",
    "Bring the core to the extraction point (north-east alley)",
    "Optional: find out what LAZARUS really is",
];

pub fn infolink(id: u16) -> (&'static str, &'static str) {
    match id {
        0 => (
            "CONTROL",
            "Ghost, this is Control. Halvorsen-Kade moved the LAZARUS core into their Port Meridian biolab tonight. Get in, take the core, get out. Stay in the shadows - if they can't see you, they can't stop you.",
        ),
        1 => ("CONTROL", "You're in the warehouse. Our contact says a maintenance duct runs from here up into the office block. Look for a vent grate."),
        2 => ("CONTROL", "You're inside the lab building. The core will be behind a security door - the security office should control it. Check the terminals for credentials."),
        3 => ("CONTROL", "That's the core. Now get to the extraction point in the north-east alley. Move."),
        4 => ("CONTROL", "They've raised the alarm. Turrets will be live. Find somewhere dark and let things cool down."),
        5 => ("CONTROL", "There's a contact waiting in the shack on the east pier. He's paid to talk - hear him out before you go in."),
        6 => ("CONTROL", "Careful. Kills draw attention. The baton and tranquilizer darts keep things quiet."),
        _ => ("CONTROL", "..."),
    }
}

pub fn light_level(p: Vec3, reg: &LightReg, phys: &Physics, outdoor: bool) -> f32 {
    let mut l = if outdoor { 0.07 } else { 0.025 };
    for li in reg.statics.iter().chain(reg.dynamic.iter()) {
        let d = p - li.pos;
        let dist = d.length();
        if dist > li.range || dist < 0.01 {
            continue;
        }
        let mut f = (1.0 - dist / li.range).powi(2) * li.power;
        if li.cos_outer > -1.5 {
            let c = (d / dist).dot(li.dir);
            if c < li.cos_outer {
                continue;
            }
            f *= smoothstep(li.cos_outer, li.cos_outer + 0.15, c);
        }
        if f < 0.01 || !phys.los(li.pos, p) {
            continue;
        }
        l += f;
    }
    l.min(1.0)
}

// ------------------------------------------------------------------ plugin

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorldState>()
            .init_resource::<Mission>()
            .init_resource::<InfolinkQueue>()
            .init_resource::<LightReg>()
            .init_resource::<LevelMeshCache>()
            .init_resource::<Physics>()
            .add_systems(OnEnter(AppState::Menu), (despawn_level, spawn_level).chain())
            .add_systems(OnEnter(AppState::InGame), (despawn_level, reset_world, spawn_level).chain())
            .add_systems(Update, rebuild_bodies.in_set(GameSet::Input))
            .add_systems(Update, update_light_registry.in_set(GameSet::Input))
            .add_systems(Update, route_requests.in_set(GameSet::Route))
            .add_systems(Update, (authority_world, auto_close_doors).in_set(GameSet::Authority).run_if(is_authority).run_if(in_game))
            .add_systems(Update, mission_authority.in_set(GameSet::Authority).run_if(is_authority).run_if(in_game).run_if(campaign_mode))
            .add_systems(Update, apply_world.in_set(GameSet::Apply))
            .add_systems(Update, (animate_doors, animate_lights, spin, animate_water).in_set(GameSet::Present));
    }
}

pub fn despawn_level(mut commands: Commands, q: Query<Entity, With<LevelEntity>>) {
    for e in q.iter() {
        commands.entity(e).despawn();
    }
}

fn reset_world(mut state: ResMut<WorldState>, mut mission: ResMut<Mission>, mut info: ResMut<InfolinkQueue>) {
    *state = WorldState::default();
    *mission = Mission::default();
    info.0.clear();
}

fn open_dir(level: &Level, x: i32, z: i32) -> Vec3 {
    let mut best = (0, Vec3::Z);
    for (dx, dz) in [(0, 1), (0, -1), (1, 0), (-1, 0)] {
        let mut n = 0;
        for k in 1..5 {
            if matches!(level.tile(x + dx * k, z + dz * k), Tile::Open(_)) {
                n += 1;
            } else {
                break;
            }
        }
        if n > best.0 {
            best = (n, Vec3::new(dx as f32, 0.0, dz as f32));
        }
    }
    best.1
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_level(
    mut commands: Commands,
    level: Res<Level>,
    mats: Res<Mats>,
    shapes: Res<Shapes>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut cache: ResMut<LevelMeshCache>,
    settings: Res<Settings>,
    bank: Option<Res<SoundBank>>,
) {
    let lvl = &*level;
    // ---- static geometry (built once per level, cached)
    if cache.hash != lvl.hash {
        cache.surfs.clear();
        cache.glass = None;
        cache.water = None;
        cache.hash = lvl.hash;
    }
    if cache.surfs.is_empty() {
        let t0 = std::time::Instant::now();
        for (s, m) in build_level_meshes(lvl) {
            cache.surfs.push((s, meshes.add(m)));
        }
        cache.glass = build_glass_mesh(lvl).map(|m| meshes.add(m));
        cache.water = build_water_mesh(lvl).map(|m| meshes.add(m));
        info!("level meshes built in {:.0}ms", t0.elapsed().as_secs_f32() * 1000.0);
    }
    for (s, h) in &cache.surfs {
        commands.spawn((Mesh3d(h.clone()), MeshMaterial3d(mats.surf(*s)), Transform::IDENTITY, LevelEntity));
    }
    if let Some(g) = &cache.glass {
        commands.spawn((Mesh3d(g.clone()), MeshMaterial3d(mats.glass.clone()), Transform::IDENTITY, NotShadowCaster, LevelEntity));
    }
    if let Some(w) = &cache.water {
        commands.spawn((Mesh3d(w.clone()), MeshMaterial3d(mats.water.clone()), Transform::IDENTITY, NotShadowCaster, WaterSurface, LevelEntity));
    }

    let mut phys = Physics::from_level(lvl);
    let ultra = settings.quality == Quality::Ultra;

    // ---- moonlight + fog volume
    let size = Vec3::new(lvl.w as f32 * CELL, 0.0, lvl.h as f32 * CELL);
    commands.spawn((
        DirectionalLight { illuminance: 32.0, color: Color::srgb(0.55, 0.65, 0.95), shadows_enabled: true, ..default() },
        Transform::from_xyz(0.0, 0.0, 0.0).looking_to(Vec3::new(-0.3, -1.0, 0.55).normalize(), Vec3::Y),
        CascadeShadowConfigBuilder { num_cascades: 2, minimum_distance: 0.1, maximum_distance: 70.0, first_cascade_far_bound: 22.0, overlap_proportion: 0.2 }
            .build(),
        LevelEntity,
    ));
    commands.spawn((
        FogVolume {
            fog_color: Color::srgb(0.6, 0.65, 0.75),
            density_factor: 0.016,
            absorption: 0.12,
            scattering: 0.3,
            scattering_asymmetry: 0.55,
            ..default()
        },
        Transform::from_xyz(size.x * 0.5, 5.0, size.z * 0.5).with_scale(Vec3::new(size.x + 20.0, 14.0, size.z + 20.0)),
        LevelEntity,
    ));

    // ---- lights
    for l in &lvl.lights {
        let c = Level::center(l.x, l.z);
        let tile = lvl.tile(l.x, l.z);
        let metal = tile.floor() == Some(Floor::Metal);
        match l.kind {
            LightKind::Ceiling | LightKind::Flicker => {
                let y = WALL_H - 0.03;
                commands.spawn((Mesh3d(shapes.cube.clone()), MeshMaterial3d(mats.gun_dark.clone()), Transform::from_xyz(c.x, y, c.z).with_scale(Vec3::new(1.1, 0.06, 0.34)), LevelEntity));
                let fixture = commands
                    .spawn((
                        Mesh3d(shapes.cube.clone()),
                        MeshMaterial3d(if metal { mats.lamp_warm.clone() } else { mats.lamp_on.clone() }),
                        Transform::from_xyz(c.x, y - 0.035, c.z).with_scale(Vec3::new(0.98, 0.02, 0.26)),
                        NotShadowCaster,
                        LevelEntity,
                    ))
                    .id();
                let color = if metal { Color::srgb(1.0, 0.78, 0.5) } else if tile.floor() == Some(Floor::Carpet) { Color::srgb(1.0, 0.9, 0.75) } else { Color::srgb(0.85, 0.92, 1.0) };
                let lp = Vec3::new(c.x, WALL_H - 0.12, c.z);
                let mut e = commands.spawn((
                    SpotLight {
                        intensity: LAMP_LM,
                        range: 11.0,
                        radius: 0.3,
                        color,
                        shadows_enabled: settings.quality != Quality::Low,
                        outer_angle: 1.15,
                        inner_angle: 0.55,
                        ..default()
                    },
                    Transform::from_translation(lp).looking_at(lp - Vec3::Y, Vec3::Z),
                    LampLight { id: l.id, kind: l.kind, on: true, broken: false, base: LAMP_LM, room: l.room, fixture, pos: lp, phase: l.id as f32 * 1.7, lit_visual: true },
                    LevelEntity,
                ));
                if ultra {
                    e.insert(VolumetricLight);
                }
                phys.targets.push((Aabb::centered(Vec3::new(c.x, y - 0.03, c.z), Vec3::new(0.5, 0.06, 0.17)), Body::Light(l.id)));
            }
            LightKind::Street => {
                let dir = open_dir(lvl, l.x, l.z);
                let pole_h = 5.4;
                commands.spawn((Mesh3d(shapes.cylinder.clone()), MeshMaterial3d(mats.gunmetal.clone()), Transform::from_xyz(c.x, pole_h * 0.5, c.z).with_scale(Vec3::new(0.14, pole_h, 0.14)), LevelEntity));
                let arm_c = c + dir * 0.6 + Vec3::Y * pole_h;
                let arm_scale = if dir.x.abs() > 0.5 { Vec3::new(1.3, 0.07, 0.09) } else { Vec3::new(0.09, 0.07, 1.3) };
                commands.spawn((Mesh3d(shapes.cube.clone()), MeshMaterial3d(mats.gunmetal.clone()), Transform::from_translation(arm_c).with_scale(arm_scale), LevelEntity));
                let head = c + dir * 1.15 + Vec3::Y * (pole_h - 0.08);
                commands.spawn((Mesh3d(shapes.cube.clone()), MeshMaterial3d(mats.gun_dark.clone()), Transform::from_translation(head + Vec3::Y * 0.05).with_scale(Vec3::new(0.55, 0.12, 0.55)), LevelEntity));
                let fixture = commands
                    .spawn((
                        Mesh3d(shapes.cube.clone()),
                        MeshMaterial3d(mats.lamp_warm.clone()),
                        Transform::from_translation(head - Vec3::Y * 0.02).with_scale(Vec3::new(0.45, 0.03, 0.45)),
                        NotShadowCaster,
                        LevelEntity,
                    ))
                    .id();
                let lp = head - Vec3::Y * 0.15;
                let mut e = commands.spawn((
                    SpotLight {
                        intensity: STREET_LM,
                        range: 16.0,
                        radius: 0.25,
                        color: Color::srgb(1.0, 0.68, 0.38),
                        shadows_enabled: true,
                        outer_angle: 1.0,
                        inner_angle: 0.45,
                        ..default()
                    },
                    Transform::from_translation(lp).looking_at(lp - Vec3::Y + dir * 0.25, Vec3::Z),
                    LampLight { id: l.id, kind: l.kind, on: true, broken: false, base: STREET_LM, room: l.room, fixture, pos: lp, phase: 0.0, lit_visual: true },
                    LevelEntity,
                ));
                if settings.quality != Quality::Low {
                    e.insert(VolumetricLight);
                }
                phys.props.push(Aabb::centered(c + Vec3::Y * (pole_h * 0.5), Vec3::new(0.1, pole_h * 0.5, 0.1)));
                phys.targets.push((Aabb::centered(head, Vec3::new(0.3, 0.12, 0.3)), Body::Light(l.id)));
            }
            LightKind::Alarm => {
                let y = WALL_H - 0.12;
                commands.spawn((Mesh3d(shapes.cylinder.clone()), MeshMaterial3d(mats.gun_dark.clone()), Transform::from_xyz(c.x, WALL_H - 0.05, c.z).with_scale(Vec3::new(0.26, 0.1, 0.26)), LevelEntity));
                let fixture = commands
                    .spawn((
                        Mesh3d(shapes.sphere.clone()),
                        MeshMaterial3d(mats.led_off.clone()),
                        Transform::from_xyz(c.x, y, c.z).with_scale(Vec3::splat(0.2)),
                        NotShadowCaster,
                        LevelEntity,
                    ))
                    .id();
                let lp = Vec3::new(c.x, y - 0.05, c.z);
                commands.spawn((
                    SpotLight { intensity: 0.0, range: 14.0, color: Color::srgb(1.0, 0.05, 0.02), outer_angle: 0.5, inner_angle: 0.2, shadows_enabled: false, ..default() },
                    Transform::from_translation(lp).looking_to(Vec3::new(1.0, -0.35, 0.0).normalize(), Vec3::Y),
                    LampLight { id: l.id, kind: l.kind, on: false, broken: false, base: ALARM_LM, room: l.room, fixture, pos: lp, phase: 0.0, lit_visual: false },
                    LevelEntity,
                ));
            }
        }
    }

    // ---- doors
    for d in &lvl.doors {
        let c = Level::center(d.x, d.z);
        let (size, base, mat) = if d.kind == DoorKind::Grate {
            let s = if d.along_x { Vec3::new(CELL, VENT_H, 0.05) } else { Vec3::new(0.05, VENT_H, CELL) };
            (s, c + Vec3::Y * (VENT_H * 0.5), mats.grate.clone())
        } else {
            let s = if d.along_x { Vec3::new(CELL, DOOR_H, 0.14) } else { Vec3::new(0.14, DOOR_H, CELL) };
            let m = if d.kind == DoorKind::Security { mats.door_sec.clone() } else { mats.door.clone() };
            (s, c + Vec3::Y * (DOOR_H * 0.5), m)
        };
        let locked = matches!(d.kind, DoorKind::Locked | DoorKind::Keypad | DoorKind::Security);
        let door = Door { id: d.id, kind: d.kind, open: false, locked, t: 0.0, along_x: d.along_x, base, size, npc_timer: 0.0 };
        let aabb = door.aabb();
        commands.spawn((
            Mesh3d(shapes.cube.clone()),
            MeshMaterial3d(mat),
            Transform::from_translation(base).with_scale(size),
            door,
            Interactable { kind: Interact::Door(d.id), aabb: Aabb::centered(aabb.center(), aabb.max - aabb.center() + Vec3::splat(0.08)) },
            LevelEntity,
        ));
        if d.kind != DoorKind::Grate {
            // status LEDs on both faces of the wall beside the door
            let span = if d.along_x { Vec3::X } else { Vec3::Z };
            let normal = if d.along_x { Vec3::Z } else { Vec3::X };
            for side in [-1.0f32, 1.0] {
                let p = c + span * (CELL * 0.5 + 0.16) + normal * side * (CELL * 0.5 + 0.012) + Vec3::Y * 1.35;
                let scale = if d.along_x { Vec3::new(0.07, 0.11, 0.02) } else { Vec3::new(0.02, 0.11, 0.07) };
                commands.spawn((
                    Mesh3d(shapes.cube.clone()),
                    MeshMaterial3d(if locked { mats.led_red.clone() } else { mats.led_green.clone() }),
                    Transform::from_translation(p).with_scale(scale),
                    DoorLed { id: d.id },
                    NotShadowCaster,
                    LevelEntity,
                ));
            }
        }
    }

    // ---- pickups
    for p in &lvl.pickups {
        let c = Level::center(p.x, p.z);
        let hsh = hash2(p.id as i32, 7, 99);
        let mut pos = c + Vec3::new((hsh - 0.5) * 0.5, 0.0, (hash2(p.id as i32, 8, 99) - 0.5) * 0.5);
        let rot = Quat::from_rotation_y(hsh * TAU);
        if p.item == ItemKind::DataCore {
            pos = c;
            commands.spawn((Mesh3d(shapes.cylinder.clone()), MeshMaterial3d(mats.metal.clone()), Transform::from_translation(c + Vec3::Y * 0.5).with_scale(Vec3::new(0.5, 1.0, 0.5)), LevelEntity));
            commands.spawn((Mesh3d(shapes.cylinder.clone()), MeshMaterial3d(mats.cyan_glow.clone()), Transform::from_translation(c + Vec3::Y * 1.0).with_scale(Vec3::new(0.54, 0.02, 0.54)), LevelEntity));
            phys.props.push(Aabb::centered(c + Vec3::Y * 0.5, Vec3::new(0.25, 0.5, 0.25)));
            pos.y = 1.01;
        }
        let parts = item_parts(p.item, &mats, &shapes);
        let mut e = commands.spawn((
            Transform::from_translation(pos).with_rotation(rot),
            Visibility::default(),
            Pickup { id: p.id, item: p.item, count: p.count },
            Interactable { kind: Interact::Pickup(p.id), aabb: Aabb::centered(pos + Vec3::Y * 0.12, Vec3::new(0.3, 0.2, 0.3)) },
            LevelEntity,
        ));
        e.with_children(|c| spawn_parts(c, &parts));
        if p.item == ItemKind::DataCore {
            e.insert(Spin(0.6));
            e.with_children(|c| {
                c.spawn((PointLight { intensity: 25_000.0, range: 5.0, color: Color::srgb(0.3, 0.85, 1.0), shadows_enabled: false, ..default() }, Transform::from_xyz(0.0, 0.25, 0.0)));
            });
        }
    }

    // ---- terminals
    for t in &lvl.terminals {
        let c = Level::center(t.x, t.z);
        let face = yaw_dir(t.yaw);
        let desk_c = c - face * 0.5;
        let rot = Quat::from_rotation_y(t.yaw);
        let screen = if t.kind == TermKind::Security { mats.screen_sec.clone() } else { mats.screen.clone() };
        let glow = if t.kind == TermKind::Security { Color::srgb(1.0, 0.6, 0.2) } else { Color::srgb(0.3, 0.9, 0.8) };
        let monitors: Vec<f32> = if t.kind == TermKind::Security { vec![-0.34, 0.34] } else { vec![0.0] };
        commands
            .spawn((Transform::from_translation(desk_c).with_rotation(rot), Visibility::default(), TerminalEnt { id: t.id }, LevelEntity))
            .with_children(|p| {
                p.spawn((Mesh3d(shapes.cube.clone()), MeshMaterial3d(mats.surf(Surf::Counter)), Transform::from_xyz(0.0, 0.76, 0.0).with_scale(Vec3::new(1.4, 0.05, 0.62))));
                for sx in [-0.67f32, 0.67] {
                    p.spawn((Mesh3d(shapes.cube.clone()), MeshMaterial3d(mats.gun_dark.clone()), Transform::from_xyz(sx, 0.37, 0.0).with_scale(Vec3::new(0.04, 0.74, 0.58))));
                }
                for mx in &monitors {
                    p.spawn((Mesh3d(shapes.cube.clone()), MeshMaterial3d(mats.plastic.clone()), Transform::from_xyz(*mx, 1.08, 0.16).with_scale(Vec3::new(0.6, 0.4, 0.05))));
                    p.spawn((Mesh3d(shapes.cube.clone()), MeshMaterial3d(screen.clone()), Transform::from_xyz(*mx, 1.08, 0.132).with_scale(Vec3::new(0.54, 0.34, 0.006)), NotShadowCaster));
                    p.spawn((Mesh3d(shapes.cube.clone()), MeshMaterial3d(mats.gun_dark.clone()), Transform::from_xyz(*mx, 0.88, 0.19).with_scale(Vec3::new(0.06, 0.2, 0.06))));
                }
                p.spawn((Mesh3d(shapes.cube.clone()), MeshMaterial3d(mats.plastic.clone()), Transform::from_xyz(0.0, 0.795, -0.12).with_scale(Vec3::new(0.46, 0.02, 0.15))));
                p.spawn((PointLight { intensity: 6_000.0, range: 3.5, color: glow, shadows_enabled: false, ..default() }, Transform::from_xyz(0.0, 1.1, -0.25)));
            });
        let half = if face.z.abs() > 0.5 { Vec3::new(0.7, 0.4, 0.31) } else { Vec3::new(0.31, 0.4, 0.7) };
        phys.props.push(Aabb::centered(desk_c + Vec3::Y * 0.4, half));
        commands.spawn((
            Transform::from_translation(desk_c),
            Interactable { kind: Interact::Terminal(t.id), aabb: Aabb::centered(desk_c + Vec3::Y * 1.0, half + Vec3::new(0.05, 0.25, 0.05)) },
            LevelEntity,
        ));
    }

    // ---- light switches
    for s in &lvl.switches {
        let c = Level::center(s.x, s.z);
        let face = yaw_dir(s.yaw);
        let p = c - face * (CELL * 0.5 - 0.02) + Vec3::Y * 1.3;
        let rot = Quat::from_rotation_y(s.yaw);
        commands
            .spawn((
                Mesh3d(shapes.cube.clone()),
                MeshMaterial3d(mats.white.clone()),
                Transform::from_translation(p).with_rotation(rot).with_scale(Vec3::new(0.09, 0.14, 0.03)),
                Interactable { kind: Interact::Switch(s.id), aabb: Aabb::centered(p, Vec3::splat(0.16)) },
                LevelEntity,
            ))
            .with_children(|c| {
                c.spawn((Mesh3d(shapes.cube.clone()), MeshMaterial3d(mats.led_amber.clone()), Transform::from_xyz(0.0, 0.3, -0.6).with_scale(Vec3::new(0.3, 0.12, 0.4)), NotShadowCaster));
            });
    }

    // ---- extraction zone
    if !lvl.extraction.is_empty() {
        let mut center = Vec3::ZERO;
        for &(x, z) in &lvl.extraction {
            let c = Level::center(x, z);
            center += c;
            commands.spawn((
                Mesh3d(shapes.quad.clone()),
                MeshMaterial3d(mats.holo.clone()),
                Transform::from_xyz(c.x, 0.02, c.z).with_scale(Vec3::new(CELL * 0.96, 1.0, CELL * 0.96)),
                NotShadowCaster,
                ExtractionFx,
                LevelEntity,
            ));
        }
        center /= lvl.extraction.len() as f32;
        commands.spawn((
            Mesh3d(shapes.cylinder.clone()),
            MeshMaterial3d(mats.beam.clone()),
            Transform::from_xyz(center.x, 15.0, center.z).with_scale(Vec3::new(0.5, 30.0, 0.5)),
            NotShadowCaster,
            ExtractionFx,
            LevelEntity,
        ));
        commands.spawn((
            PointLight { intensity: 60_000.0, range: 9.0, color: Color::srgb(0.2, 0.8, 1.0), shadows_enabled: false, ..default() },
            Transform::from_translation(center + Vec3::Y * 1.5),
            LevelEntity,
        ));
    }

    crate::npc::spawn_npcs(&mut commands, lvl, &mats, &shapes, &settings, &mut phys);
    crate::effects::spawn_rain(&mut commands, &shapes, &mats);
    if let Some(bank) = bank {
        spawn_loops(&mut commands, &bank, &settings);
    }
    commands.insert_resource(phys);
}

// ------------------------------------------------------------------ per-frame world systems

pub fn rebuild_bodies(mut phys: ResMut<Physics>, doors: Query<&Door>, lights: Query<&LampLight>, level: Option<Res<Level>>) {
    phys.dynamic.clear();
    phys.targets.retain(|(_, b)| matches!(b, Body::Light(_)));
    for d in doors.iter() {
        if d.t < 0.97 {
            phys.dynamic.push((d.aabb(), Body::Door(d.id)));
        }
    }
    // broken lights stop being targets
    let Some(_level) = level else { return };
    let broken: HashSet<u16> = lights.iter().filter(|l| l.broken).map(|l| l.id).collect();
    if !broken.is_empty() {
        phys.targets.retain(|(_, b)| !matches!(b, Body::Light(id) if broken.contains(id)));
    }
}

pub fn update_light_registry(mut reg: ResMut<LightReg>, lights: Query<(&LampLight, &SpotLight, &GlobalTransform)>) {
    reg.statics.clear();
    for (l, s, gt) in lights.iter() {
        if !l.on || l.broken || s.intensity <= 1.0 {
            continue;
        }
        let power = match l.kind {
            LightKind::Ceiling => 1.0,
            LightKind::Flicker => 0.55,
            LightKind::Street => 1.1,
            LightKind::Alarm => 0.25,
        };
        reg.statics.push(LightInfo { pos: gt.translation(), dir: gt.forward().as_vec3(), cos_outer: s.outer_angle.cos(), range: s.range * 0.8, power });
    }
}

/// Local gameplay requests go to the authority. On a client, `net.rs` forwards them to the host.
fn route_requests(mut reqs: EventReader<WorldRequest>, mut out: EventWriter<AuthRequest>, local: Res<LocalId>) {
    for r in reqs.read() {
        out.write(AuthRequest { from: local.0, req: r.clone() });
    }
}

#[allow(clippy::too_many_arguments)]
fn authority_world(
    mut reqs: EventReader<AuthRequest>,
    doors: Query<&Door>,
    lights: Query<&LampLight>,
    level: Res<Level>,
    mut state: ResMut<WorldState>,
    mut mission: ResMut<Mission>,
    mut out: EventWriter<WorldEv>,
) {
    let mut send = |ev: WorldEvent| {
        out.write(WorldEv { ev, quiet: false });
    };
    for r in reqs.read() {
        match &r.req {
            WorldRequest::SetDoor { id, open } => {
                if let Some(d) = doors.iter().find(|d| d.id == *id) {
                    if !d.locked || !*open {
                        send(WorldEvent::Door { id: *id, open: *open, locked: d.locked });
                    }
                }
            }
            WorldRequest::Unlock { id } => send(WorldEvent::Door { id: *id, open: true, locked: false }),
            WorldRequest::Switch { id } => {
                if let Some(sw) = level.switches.get(*id as usize) {
                    let room: Vec<&LampLight> =
                        lights.iter().filter(|l| l.room == sw.room && !l.broken && matches!(l.kind, LightKind::Ceiling | LightKind::Flicker)).collect();
                    let any_on = room.iter().any(|l| l.on);
                    for l in room {
                        send(WorldEvent::Light { id: l.id, on: !any_on, broken: false });
                    }
                }
            }
            WorldRequest::BreakLight { id } => {
                if lights.iter().any(|l| l.id == *id && !l.broken && l.kind != LightKind::Alarm) {
                    send(WorldEvent::Light { id: *id, on: false, broken: true });
                }
            }
            WorldRequest::Pickup { id } => {
                if !state.taken.contains(id) {
                    send(WorldEvent::PickupTaken { id: *id, by: r.from });
                    if level.pickups.get(*id as usize).map(|p| p.item == ItemKind::DataCore).unwrap_or(false) && !mission.obj_sent[1] {
                        mission.obj_sent[1] = true;
                        send(WorldEvent::Objective { id: 1 });
                        if mission.sent.insert(3) {
                            send(WorldEvent::Infolink { id: 3 });
                        }
                    }
                }
            }
            WorldRequest::ReadNote { id } => {
                if !state.notes.contains(id) {
                    send(WorldEvent::Note { id: *id });
                    if *id == 4 && !mission.obj_sent[3] {
                        mission.obj_sent[3] = true;
                        send(WorldEvent::Objective { id: 3 });
                    }
                }
            }
            WorldRequest::SetCameras { on } => send(WorldEvent::Security { cameras: *on, turrets: state.turret_mode }),
            WorldRequest::SetTurrets { mode } => send(WorldEvent::Security { cameras: state.cameras_on, turrets: *mode }),
            WorldRequest::TriggerAlarm { pos } => {
                mission.alarm_t = 45.0;
                if !state.alarm {
                    state.stats.alarms += 1;
                    send(WorldEvent::Alarm { active: true, pos: *pos });
                    if mission.sent.insert(4) {
                        send(WorldEvent::Infolink { id: 4 });
                    }
                }
            }
            WorldRequest::Revive { target } => send(WorldEvent::Revived { target: *target }),
            _ => {}
        }
    }
}

fn mission_authority(time: Res<Time>, players: Res<Players>, level: Res<Level>, state: Res<WorldState>, mut mission: ResMut<Mission>, mut out: EventWriter<WorldEv>) {
    if state.complete.is_some() {
        return;
    }
    let dt = time.delta_secs();
    mission.t += dt;
    let mut evs = vec![];
    let info = |id: u16, m: &mut Mission, evs: &mut Vec<WorldEvent>| {
        if m.sent.insert(id) {
            evs.push(WorldEvent::Infolink { id });
        }
    };
    if mission.t > 1.5 {
        info(0, &mut mission, &mut evs);
    }
    if mission.t > 35.0 {
        info(5, &mut mission, &mut evs);
    }
    if state.stats.kills >= 2 {
        info(6, &mut mission, &mut evs);
    }
    for p in players.map.values() {
        let pos = p.net.pos();
        match level.tile_at(pos).floor() {
            Some(Floor::Metal) => info(1, &mut mission, &mut evs),
            Some(Floor::Tile | Floor::Carpet) => {
                if !mission.obj_sent[0] {
                    mission.obj_sent[0] = true;
                    evs.push(WorldEvent::Objective { id: 0 });
                    info(2, &mut mission, &mut evs);
                }
            }
            _ => {}
        }
        if p.net.has(pflags::CORE) && level.in_extraction(pos) && !mission.obj_sent[2] {
            mission.obj_sent[2] = true;
            evs.push(WorldEvent::Objective { id: 2 });
            evs.push(WorldEvent::MissionComplete { time: mission.t, kills: state.stats.kills, knockouts: state.stats.knockouts, alarms: state.stats.alarms });
        }
    }
    if state.alarm {
        mission.alarm_t -= dt;
        if mission.alarm_t <= 0.0 {
            evs.push(WorldEvent::Alarm { active: false, pos: [0.0; 3] });
        }
    }
    for ev in evs {
        out.write(WorldEv { ev, quiet: false });
    }
}

/// Doors opened by guards close themselves once the doorway is clear.
fn auto_close_doors(time: Res<Time>, mut doors: Query<&mut Door>, players: Res<Players>, guards: Query<&crate::npc::Guard>, mut out: EventWriter<WorldEv>) {
    for mut d in doors.iter_mut() {
        if d.npc_timer <= 0.0 || !d.open {
            continue;
        }
        d.npc_timer -= time.delta_secs();
        if d.npc_timer > 0.0 {
            continue;
        }
        let zone = Aabb::centered(d.base, Vec3::new(1.6, 2.0, 1.6));
        let busy = players.map.values().any(|p| zone.contains(p.net.pos() + Vec3::Y * 0.5)) || guards.iter().any(|g| zone.contains(g.pos + Vec3::Y * 0.5));
        if busy {
            d.npc_timer = 1.0;
        } else {
            out.write(WorldEv { ev: WorldEvent::Door { id: d.id, open: false, locked: d.locked }, quiet: false });
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_world(
    mut commands: Commands,
    mut evs: EventReader<WorldEv>,
    mut doors: Query<&mut Door>,
    mut lights: Query<&mut LampLight>,
    pickups: Query<(Entity, &Pickup)>,
    mut state: ResMut<WorldState>,
    mut msgs: EventWriter<HudMsg>,
    mut snd: EventWriter<SoundEv>,
    mut info: ResMut<InfolinkQueue>,
    level: Res<Level>,
) {
    for WorldEv { ev, quiet } in evs.read() {
        let quiet = *quiet;
        match ev {
            WorldEvent::Door { id, open, locked } => {
                if let Some(mut d) = doors.iter_mut().find(|d| d.id == *id) {
                    if !quiet && d.open != *open {
                        snd.write(SoundEv::at(if *open { Sfx::DoorOpen } else { Sfx::DoorClose }, d.base, 0.7));
                    }
                    if !quiet && d.locked && !*locked {
                        snd.write(SoundEv::at(Sfx::Unlock, d.base, 0.6));
                    }
                    d.open = *open;
                    d.locked = *locked;
                }
            }
            WorldEvent::Light { id, on, broken } => {
                if let Some(mut l) = lights.iter_mut().find(|l| l.id == *id) {
                    if *broken && !l.broken && !quiet {
                        snd.write(SoundEv::at(Sfx::Shatter, l.pos, 0.9));
                    } else if !quiet && l.on != *on {
                        snd.write(SoundEv::at(Sfx::Click, l.pos, 0.5));
                    }
                    l.on = *on;
                    l.broken = *broken;
                }
            }
            WorldEvent::PickupTaken { id, .. } => {
                state.taken.insert(*id);
                for (e, p) in pickups.iter() {
                    if p.id == *id {
                        commands.entity(e).despawn();
                    }
                }
            }
            WorldEvent::BodyLooted { id, .. } => {
                state.looted.insert(*id);
            }
            WorldEvent::Alarm { active, pos } => {
                state.alarm = *active;
                state.alarm_pos = v3(*pos);
                if !quiet {
                    msgs.write(HudMsg(if *active { "ALARM RAISED".into() } else { "Alarm cancelled".into() }));
                }
            }
            WorldEvent::Security { cameras, turrets } => {
                state.cameras_on = *cameras;
                state.turret_mode = *turrets;
            }
            WorldEvent::Objective { id } => {
                if let Some(o) = state.objectives.get_mut(*id as usize) {
                    if !*o && !quiet {
                        msgs.write(HudMsg(format!("GOAL COMPLETE: {}", OBJECTIVES[*id as usize])));
                        snd.write(SoundEv::ui(Sfx::Granted, 0.6));
                    }
                    *o = true;
                }
            }
            WorldEvent::Note { id } => {
                if !state.notes.contains(id) {
                    state.notes.push(*id);
                    if !quiet {
                        if let Some(n) = level.note(*id) {
                            msgs.write(HudMsg(format!("Note added: {}", n.title)));
                        }
                    }
                }
            }
            WorldEvent::Infolink { id } => {
                if !quiet {
                    info.0.push(*id);
                }
            }
            WorldEvent::MissionComplete { time, kills, knockouts, alarms } => {
                state.complete = Some((*time, *kills, *knockouts, *alarms));
            }
            _ => {}
        }
    }
}

fn animate_doors(time: Res<Time>, mut doors: Query<(&mut Door, &mut Transform)>, mut leds: Query<(&DoorLed, &mut MeshMaterial3d<StandardMaterial>)>, mats: Res<Mats>) {
    let dt = time.delta_secs();
    for (mut d, mut tf) in doors.iter_mut() {
        let target = if d.open { 1.0 } else { 0.0 };
        if (d.t - target).abs() > 1e-3 {
            let speed = if d.kind == DoorKind::Grate { 2.5 } else { 1.7 };
            d.t += (target - d.t).signum() * (speed * dt).min((target - d.t).abs());
            tf.translation = d.base + d.offset();
        }
    }
    for (led, mut m) in leds.iter_mut() {
        if let Some(d) = doors.iter().find(|(d, _)| d.id == led.id).map(|(d, _)| d) {
            let want = if !d.locked {
                &mats.led_green
            } else if d.kind == DoorKind::Keypad {
                &mats.led_amber
            } else {
                &mats.led_red
            };
            if m.0 != *want {
                m.0 = want.clone();
            }
        }
    }
}

fn animate_lights(
    time: Res<Time>,
    state: Res<WorldState>,
    mut lights: Query<(&mut LampLight, &mut SpotLight, &mut Transform)>,
    mut fixtures: Query<&mut MeshMaterial3d<StandardMaterial>>,
    mats: Res<Mats>,
) {
    let t = time.elapsed_secs();
    for (mut l, mut s, mut tf) in lights.iter_mut() {
        let (lit, intensity) = match l.kind {
            LightKind::Alarm => {
                l.on = state.alarm;
                if state.alarm {
                    tf.rotation = Quat::from_rotation_y(t * 4.5 + l.phase) * Quat::from_rotation_x(-0.35) * Quat::from_rotation_y(-FRAC_PI_2);
                }
                (state.alarm, if state.alarm { l.base } else { 0.0 })
            }
            LightKind::Flicker if l.on && !l.broken => {
                let n = value_noise(t * 9.0 + l.phase, 0.0, 1 << 20, 5);
                let off = n > 0.72 || (value_noise(t * 0.7 + l.phase, 3.0, 1 << 20, 6) > 0.8 && n > 0.4);
                (!off, if off { l.base * 0.03 } else { l.base * (0.85 + n * 0.15) })
            }
            _ => {
                let on = l.on && !l.broken;
                (on, if on { l.base } else { 0.0 })
            }
        };
        if (s.intensity - intensity).abs() > 1.0 {
            s.intensity = intensity;
        }
        if lit != l.lit_visual {
            l.lit_visual = lit;
            if let Ok(mut m) = fixtures.get_mut(l.fixture) {
                m.0 = match (l.kind, lit) {
                    (LightKind::Alarm, true) => mats.alarm_on.clone(),
                    (LightKind::Alarm, false) => mats.led_off.clone(),
                    (LightKind::Street, true) => mats.lamp_warm.clone(),
                    (_, true) => mats.lamp_on.clone(),
                    _ => mats.lamp_off.clone(),
                };
            }
        }
    }
}

fn spin(time: Res<Time>, mut q: Query<(&Spin, &mut Transform)>) {
    for (s, mut tf) in q.iter_mut() {
        tf.rotate_y(s.0 * time.delta_secs());
    }
}

fn animate_water(time: Res<Time>, mats: Res<Mats>, mut assets: ResMut<Assets<StandardMaterial>>, mut acc: Local<f32>) {
    *acc += time.delta_secs();
    if *acc < 1.0 / 30.0 {
        return;
    }
    *acc = 0.0;
    let t = time.elapsed_secs();
    if let Some(m) = assets.get_mut(&mats.water) {
        m.uv_transform = bevy::math::Affine2::from_translation(Vec2::new(t * 0.012, (t * 0.31).sin() * 0.02 + t * 0.004));
    }
    let _ = PI;
}
