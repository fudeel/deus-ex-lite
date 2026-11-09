//! Belt, firing, reloading, aim spread and the first-person view model.
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;

use crate::audio::*;
use crate::collision::*;
use crate::game::*;
use crate::items::*;
use crate::materials::*;
use crate::player::*;
use crate::util::*;

const VM_SCALE: f32 = 0.5;

#[derive(Resource)]
pub struct WeaponState {
    pub cooldown: f32,
    pub spread: f32,
    pub reload_t: f32,
    pub reload_total: f32,
    pub equip_t: f32,
    pub kick: f32,
    pub swing: f32,
    pub last: Option<u32>,
    pub model_for: Option<Option<WeaponKind>>,
    pub muzzle_t: f32,
    pub sway: Vec2,
}

impl Default for WeaponState {
    fn default() -> Self {
        WeaponState {
            cooldown: 0.0,
            spread: 4.0,
            reload_t: 0.0,
            reload_total: 1.0,
            equip_t: 0.0,
            kick: 0.0,
            swing: 0.0,
            last: None,
            model_for: None,
            muzzle_t: 0.0,
            sway: Vec2::ZERO,
        }
    }
}

#[derive(Component)]
pub struct ViewModelRoot;

#[derive(Component)]
pub struct ViewModelGun;

#[derive(Component)]
pub struct MuzzleFlash;

pub struct WeaponsPlugin;

impl Plugin for WeaponsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WeaponState>()
            .add_systems(OnEnter(AppState::InGame), spawn_viewmodel)
            .add_systems(Update, (belt_input, weapons_fire).chain().in_set(GameSet::Logic).after(crate::player::PlayerLogic).run_if(in_game))
            .add_systems(Update, (rebuild_viewmodel, animate_viewmodel).chain().in_set(GameSet::Present).run_if(in_game));
    }
}

fn muzzle_offset(w: Option<WeaponKind>) -> Vec3 {
    match w {
        Some(WeaponKind::Pistol) => Vec3::new(0.0, 0.035, -0.16),
        Some(WeaponKind::Rifle) => Vec3::new(0.0, 0.01, -0.54),
        Some(WeaponKind::Crossbow) => Vec3::new(0.0, 0.036, -0.24),
        _ => Vec3::new(0.0, 0.0, -0.4),
    }
}

fn vm_base(zoom: bool, w: Option<WeaponKind>) -> Vec3 {
    if zoom {
        match w {
            Some(WeaponKind::Rifle) => Vec3::new(0.0, -0.075, -0.16),
            Some(WeaponKind::Crossbow) => Vec3::new(0.0, -0.07, -0.17),
            _ => Vec3::new(0.0, -0.062, -0.2),
        }
    } else {
        match w {
            Some(WeaponKind::Baton) => Vec3::new(0.15, -0.14, -0.2),
            Some(WeaponKind::Rifle) => Vec3::new(0.12, -0.12, -0.17),
            _ => Vec3::new(0.14, -0.13, -0.24),
        }
    }
}

fn spawn_viewmodel(mut commands: Commands, cam: Query<Entity, With<MainCamera>>, mats: Res<Mats>, shapes: Res<Shapes>, mut ws: ResMut<WeaponState>) {
    *ws = WeaponState::default();
    let Ok(cam) = cam.single() else { return };
    let root = commands
        .spawn((Transform::from_translation(vm_base(false, None)), Visibility::default(), ViewModelRoot, LevelEntity))
        .with_children(|p| {
            p.spawn((Transform::default(), Visibility::default(), ViewModelGun));
            p.spawn((
                Mesh3d(shapes.sphere_lo.clone()),
                MeshMaterial3d(mats.muzzle.clone()),
                Transform::from_translation(Vec3::ZERO).with_scale(Vec3::splat(0.0)),
                NotShadowCaster,
                MuzzleFlash,
            ))
            .with_children(|m| {
                m.spawn((PointLight { intensity: 0.0, range: 9.0, color: Color::srgb(1.0, 0.75, 0.4), shadows_enabled: false, ..default() }, Transform::default()));
            });
        })
        .id();
    commands.entity(cam).add_child(root);
}

fn belt_input(
    keys: Res<ButtonInput<KeyCode>>,
    scroll: Res<AccumulatedMouseScroll>,
    mut me: ResMut<Me>,
    mut inv: ResMut<Inventory>,
    skills: Res<Skills>,
    mut ui_mut: ResMut<UiMode>,
    mut snd: EventWriter<SoundEv>,
    mut msgs: EventWriter<HudMsg>,
) {
    if *ui_mut != UiMode::None || me.dead {
        return;
    }
    let digits = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
        KeyCode::Digit0,
    ];
    for (slot, k) in digits.iter().enumerate() {
        if !keys.just_pressed(*k) {
            continue;
        }
        let Some(uid) = inv.belt[slot] else { continue };
        let Some(kind) = inv.get(uid).map(|i| i.kind) else { continue };
        match kind {
            ItemKind::Weapon(_) => {
                inv.equipped = if inv.equipped == Some(uid) { None } else { Some(uid) };
                snd.write(SoundEv::ui(Sfx::Click, 0.4));
            }
            other => {
                let mut mode = ui_mut.clone();
                use_consumable(other, &mut me, &mut inv, &skills, &mut snd, &mut msgs, &mut mode);
                if mode != *ui_mut {
                    *ui_mut = mode;
                }
            }
        }
    }
    let dy = scroll.delta.y;
    if dy.abs() > 0.01 {
        let weapons: Vec<u32> = inv.belt.iter().flatten().copied().filter(|u| matches!(inv.get(*u).map(|i| i.kind), Some(ItemKind::Weapon(_)))).collect();
        if !weapons.is_empty() {
            let cur = inv.equipped.and_then(|e| weapons.iter().position(|w| *w == e));
            let n = weapons.len() as i32;
            let next = match cur {
                Some(i) => (i as i32 + if dy > 0.0 { -1 } else { 1 }).rem_euclid(n),
                None => 0,
            };
            inv.equipped = Some(weapons[next as usize]);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn weapons_fire(
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    ui: Res<UiMode>,
    mut me: ResMut<Me>,
    mut inv: ResMut<Inventory>,
    skills: Res<Skills>,
    mut ws: ResMut<WeaponState>,
    phys: Res<Physics>,
    pose: Res<CamPose>,
    ids: (Res<LocalId>, Res<Players>),
    mut rng: ResMut<GameRng>,
    mut req: EventWriter<WorldRequest>,
    mut fx: EventWriter<FxEv>,
    mut snd: EventWriter<SoundEv>,
    mut msgs: EventWriter<HudMsg>,
) {
    let (local, players) = (&ids.0, &ids.1);
    let dt = time.delta_secs();
    ws.cooldown -= dt;
    ws.kick *= 1.0 - (dt * 14.0).min(1.0);
    ws.swing = (ws.swing - dt * 3.0).max(0.0);
    ws.muzzle_t -= dt;
    let equipped = inv.equipped_weapon();
    if equipped.map(|e| e.0) != ws.last {
        ws.last = equipped.map(|e| e.0);
        ws.equip_t = 0.0;
        ws.reload_t = 0.0;
    }
    ws.equip_t = (ws.equip_t + dt * 3.5).min(1.0);
    let active = *ui == UiMode::None && !me.dead;
    let Some((uid, w)) = equipped else {
        me.zoom = false;
        return;
    };
    let def = weapon_def(w);
    let firearms = skills.get(Skill::Firearms) as f32;
    me.zoom = active && mouse.pressed(MouseButton::Right) && w != WeaponKind::Baton;

    // aim spread converges while standing still (skill dependent)
    let mut target = def.spread_min * (1.7 - firearms * 0.3);
    if me.moving {
        target += if me.sprinting { 5.0 } else { 2.2 };
    }
    if !me.grounded {
        target += 5.0;
    }
    if me.col_h < STAND_H {
        target *= 0.75;
    }
    if me.zoom {
        target *= 0.5;
    }
    target = target.min(def.spread_max);
    let rate = 1.6 + firearms * 0.7;
    ws.spread += (target - ws.spread) * exp_decay(rate, dt);
    ws.spread = ws.spread.min(def.spread_max * 1.5);

    // reload
    if let Some(ammo) = def.ammo {
        let mag = inv.get(uid).map(|i| i.mag).unwrap_or(0);
        let reserve = inv.ammo[ammo.idx()];
        if active && keys.just_pressed(KeyCode::KeyR) && ws.reload_t <= 0.0 && mag < def.mag && reserve > 0 {
            ws.reload_total = def.reload / (1.0 + firearms * 0.15);
            ws.reload_t = ws.reload_total;
            snd.write(SoundEv::ui(Sfx::Reload, 0.5));
        }
        if ws.reload_t > 0.0 {
            ws.reload_t -= dt;
            if ws.reload_t <= 0.0 {
                let need = def.mag - mag;
                let take = need.min(inv.ammo[ammo.idx()]);
                inv.ammo[ammo.idx()] -= take;
                if let Some(it) = inv.get_mut(uid) {
                    it.mag += take;
                }
            }
        }
    }

    // the click that closed a menu must not also fire the weapon
    if !active || ui.is_changed() {
        return;
    }
    let trigger = if def.auto { mouse.pressed(MouseButton::Left) } else { mouse.just_pressed(MouseButton::Left) };
    if !trigger || ws.cooldown > 0.0 || ws.equip_t < 0.95 || ws.reload_t > 0.0 {
        return;
    }
    let fwd = pose.rot * Vec3::NEG_Z;
    let origin = pose.pos;
    let me_body = Some(Body::Player(local.0));

    if w == WeaponKind::Baton {
        ws.cooldown = def.rate * (1.0 - skills.get(Skill::Melee) as f32 * 0.1);
        ws.swing = 1.0;
        snd.write(SoundEv::at(Sfx::Swing, origin, 0.6));
        let mut best: Option<Hit> = None;
        for off in [-0.12f32, 0.0, 0.12] {
            let right = pose.rot * Vec3::X;
            let d = (fwd + right * off).normalize();
            if let Some(h) = phys.raycast(origin, d, def.range, true, false, me_body) {
                if best.map(|b| h.t < b.t).unwrap_or(true) {
                    best = Some(h);
                }
            }
        }
        if let Some(h) = best {
            match h.body {
                Body::Guard(id, head) => {
                    let dmg = def.dmg * (1.0 + skills.get(Skill::Melee) as f32 * 0.35);
                    req.write(WorldRequest::HitNpc { id, dmg, kind: DmgKind::Melee, dir: a3(fwd), head });
                    snd.write(SoundEv::at(Sfx::Zap, h.pos, 0.8));
                    fx.write(FxEv { fx: Fx::Shot { from: a3(h.pos), to: a3(h.pos), weapon: 0, impact: impact::FLESH, normal: a3(h.normal) }, remote: false });
                }
                Body::Light(id) => {
                    req.write(WorldRequest::BreakLight { id });
                    req.write(WorldRequest::Noise { pos: a3(h.pos), radius: 10.0 });
                }
                Body::Camera(id) => {
                    req.write(WorldRequest::HitCamera { id, dmg: 40.0 });
                }
                Body::Turret(id) => {
                    req.write(WorldRequest::HitTurret { id, dmg: 20.0 });
                }
                Body::Player(id) | Body::PlayerHead(id) => {
                    // no friendly fire in the team match
                    if players.team_of(id) != players.team_of(local.0) {
                        let dmg = def.dmg * (1.0 + skills.get(Skill::Melee) as f32 * 0.35);
                        let head = matches!(h.body, Body::PlayerHead(_));
                        req.write(WorldRequest::HitPlayer { target: id, dmg, kind: DmgKind::Melee, dir: a3(fwd), head, weapon: def.id });
                        snd.write(SoundEv::at(Sfx::Zap, h.pos, 0.8));
                    }
                    fx.write(FxEv { fx: Fx::Shot { from: a3(h.pos), to: a3(h.pos), weapon: 0, impact: impact::FLESH, normal: a3(h.normal) }, remote: false });
                }
                _ => {
                    snd.write(SoundEv::at(Sfx::ImpactWall, h.pos, 0.6));
                    req.write(WorldRequest::Noise { pos: a3(h.pos), radius: def.noise });
                }
            }
        }
        return;
    }

    let mag = inv.get(uid).map(|i| i.mag).unwrap_or(0);
    if mag == 0 {
        snd.write(SoundEv::ui(Sfx::Empty, 0.5));
        ws.cooldown = 0.3;
        if let Some(a) = def.ammo {
            if inv.ammo[a.idx()] > 0 {
                ws.reload_total = def.reload / (1.0 + firearms * 0.15);
                ws.reload_t = ws.reload_total;
                snd.write(SoundEv::ui(Sfx::Reload, 0.5));
            } else {
                msgs.write(HudMsg(format!("Out of {}", a.name())));
            }
        }
        return;
    }
    if let Some(it) = inv.get_mut(uid) {
        it.mag -= 1;
    }
    let dir = cone_dir(fwd, ws.spread.to_radians(), &mut rng.0);
    let hit = phys.raycast(origin, dir, def.range, true, false, me_body);
    let (end, normal, imp) = match hit {
        Some(h) => {
            let imp = match h.body {
                Body::Guard(id, head) => {
                    let dmg = def.dmg * (1.0 + firearms * 0.12);
                    req.write(WorldRequest::HitNpc { id, dmg, kind: def.kind, dir: a3(dir), head });
                    impact::FLESH
                }
                Body::Light(id) => {
                    req.write(WorldRequest::BreakLight { id });
                    req.write(WorldRequest::Noise { pos: a3(h.pos), radius: 12.0 });
                    impact::GLASS
                }
                Body::Camera(id) => {
                    req.write(WorldRequest::HitCamera { id, dmg: def.dmg });
                    impact::METAL
                }
                Body::Turret(id) => {
                    req.write(WorldRequest::HitTurret { id, dmg: def.dmg });
                    impact::METAL
                }
                Body::Player(id) | Body::PlayerHead(id) => {
                    if players.team_of(id) != players.team_of(local.0) {
                        let dmg = def.dmg * (1.0 + firearms * 0.12);
                        let head = matches!(h.body, Body::PlayerHead(_));
                        req.write(WorldRequest::HitPlayer { target: id, dmg, kind: def.kind, dir: a3(dir), head, weapon: def.id });
                    }
                    impact::FLESH
                }
                Body::Glass => impact::GLASS,
                Body::Door(_) => impact::METAL,
                _ => impact::WALL,
            };
            (h.pos, h.normal, imp)
        }
        None => (origin + dir * def.range, Vec3::Y, impact::NONE),
    };
    let muzzle = pose.pos + pose.rot * (vm_base(me.zoom, Some(w)) + muzzle_offset(Some(w)) * VM_SCALE);
    fx.write(FxEv { fx: Fx::Shot { from: a3(muzzle), to: a3(end), weapon: def.id, impact: imp, normal: a3(normal) }, remote: false });
    let sfx = match w {
        WeaponKind::Pistol => Sfx::Pistol,
        WeaponKind::Rifle => Sfx::Rifle,
        _ => Sfx::Crossbow,
    };
    snd.write(SoundEv::at(sfx, origin, if w == WeaponKind::Crossbow { 0.5 } else { 0.9 }));
    req.write(WorldRequest::Noise { pos: a3(origin), radius: def.noise });
    ws.spread += def.kick;
    me.recoil += def.recoil;
    me.pitch = (me.pitch + def.recoil * 0.45).min(1.5);
    ws.kick = 1.0;
    ws.cooldown = def.rate;
    if w != WeaponKind::Crossbow {
        ws.muzzle_t = 0.05;
    }
}

fn rebuild_viewmodel(
    mut commands: Commands,
    inv: Res<Inventory>,
    mut ws: ResMut<WeaponState>,
    gun: Query<(Entity, Option<&Children>), With<ViewModelGun>>,
    mats: Res<Mats>,
    shapes: Res<Shapes>,
) {
    let w = inv.equipped_weapon().map(|e| e.1);
    if ws.model_for == Some(w) {
        return;
    }
    let Ok((e, children)) = gun.single() else { return };
    if let Some(ch) = children {
        for c in ch.iter() {
            commands.entity(c).despawn();
        }
    }
    if let Some(w) = w {
        let parts = weapon_parts(w, &mats, &shapes);
        commands.entity(e).with_children(|p| {
            for part in parts {
                let tf = Transform::from_translation(part.tf.translation * VM_SCALE).with_rotation(part.tf.rotation).with_scale(part.tf.scale * VM_SCALE);
                p.spawn((Mesh3d(part.mesh), MeshMaterial3d(part.mat), tf, NotShadowCaster));
            }
        });
    }
    ws.model_for = Some(w);
}

#[allow(clippy::too_many_arguments)]
fn animate_viewmodel(
    time: Res<Time>,
    me: Res<Me>,
    mut ws: ResMut<WeaponState>,
    inv: Res<Inventory>,
    motion: Res<AccumulatedMouseMotion>,
    ui: Res<UiMode>,
    mut root: Query<(&mut Transform, &mut Visibility), (With<ViewModelRoot>, Without<MuzzleFlash>)>,
    mut flash: Query<(&mut Transform, &Children), (With<MuzzleFlash>, Without<ViewModelRoot>)>,
    mut lights: Query<&mut PointLight>,
    mut rng: ResMut<GameRng>,
) {
    let dt = time.delta_secs();
    let Ok((mut tf, mut vis)) = root.single_mut() else { return };
    let w = inv.equipped_weapon().map(|e| e.1);
    *vis = if w.is_some() && !me.dead { Visibility::Inherited } else { Visibility::Hidden };
    let md = if *ui == UiMode::None { motion.delta } else { Vec2::ZERO };
    let target_sway = Vec2::new(-md.x, md.y) * 0.00035;
    ws.sway = ws.sway.lerp(target_sway.clamp(Vec2::splat(-0.03), Vec2::splat(0.03)), exp_decay(10.0, dt));
    let bob_amp = (me.speed / 6.0).min(1.2) * if me.grounded { 1.0 } else { 0.3 } * if me.zoom { 0.15 } else { 1.0 };
    let bob = Vec3::new(me.bob.sin() * 0.012, (me.bob * 2.0).sin().abs() * -0.012, 0.0) * bob_amp;
    let mut pos = vm_base(me.zoom, w) + bob + Vec3::new(ws.sway.x, ws.sway.y, 0.0);
    pos.y -= (1.0 - ws.equip_t).powi(2) * 0.25;
    pos.z += ws.kick * 0.035;
    let mut rot = Quat::from_rotation_x(ws.kick * 0.12) * Quat::from_rotation_y(ws.sway.x * 2.0);
    if ws.reload_t > 0.0 {
        let p = 1.0 - ws.reload_t / ws.reload_total.max(0.01);
        let k = (p * std::f32::consts::PI).sin();
        pos.y -= k * 0.08;
        rot *= Quat::from_rotation_x(-k * 0.7) * Quat::from_rotation_z(k * 0.4);
    }
    if ws.swing > 0.0 {
        let s = ws.swing;
        let arc = (1.0 - s) * std::f32::consts::PI;
        rot *= Quat::from_rotation_y(arc.sin() * 1.1) * Quat::from_rotation_x(-arc.sin() * 0.9);
        pos.x -= arc.sin() * 0.1;
    }
    if me.sprinting {
        rot *= Quat::from_rotation_y(0.5) * Quat::from_rotation_x(-0.25);
        pos += Vec3::new(-0.03, -0.03, 0.03);
    }
    tf.translation = tf.translation.lerp(pos, exp_decay(18.0, dt));
    tf.rotation = tf.rotation.slerp(rot, exp_decay(18.0, dt));

    if let Ok((mut ftf, children)) = flash.single_mut() {
        let on = ws.muzzle_t > 0.0;
        ftf.translation = muzzle_offset(w) * VM_SCALE;
        ftf.scale = if on { Vec3::splat(rng.0.range(0.04, 0.07)) } else { Vec3::ZERO };
        ftf.rotation = Quat::from_rotation_z(rng.0.range(0.0, 6.28));
        for c in children.iter() {
            if let Ok(mut l) = lights.get_mut(c) {
                l.intensity = if on { 180_000.0 } else { 0.0 };
            }
        }
    }
}
