//! Local player: first-person movement, camera, stats, augmentations, visibility, damage.
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, PrimaryWindow};

use crate::audio::*;
use crate::collision::*;
use crate::game::*;
use crate::items::*;
use crate::level::*;
use crate::util::*;
use crate::world::*;

pub const STAND_H: f32 = 1.8;
pub const CROUCH_H: f32 = 1.05;
pub const HW: f32 = 0.28;

#[derive(Resource)]
pub struct Me {
    pub feet: Vec3,
    pub vel: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub crouch: bool,
    pub col_h: f32,
    pub eye_h: f32,
    pub grounded: bool,
    pub lean: f32,
    pub health: f32,
    pub energy: f32,
    pub dead: bool,
    pub dead_t: f32,
    pub flashlight: bool,
    pub bob: f32,
    pub step_dist: f32,
    pub land_dip: f32,
    pub visibility: f32,
    pub vis_timer: f32,
    pub moving: bool,
    pub sprinting: bool,
    pub hurt: f32,
    pub recoil: f32,
    pub fall_v: f32,
    pub zoom: bool,
    pub speed: f32,
    pub noise_cd: f32,
}

impl Me {
    pub fn new(feet: Vec3, yaw: f32) -> Self {
        Me {
            feet,
            vel: Vec3::ZERO,
            yaw,
            pitch: 0.0,
            crouch: false,
            col_h: STAND_H,
            eye_h: STAND_H - 0.14,
            grounded: true,
            lean: 0.0,
            health: 100.0,
            energy: 100.0,
            dead: false,
            dead_t: 0.0,
            flashlight: false,
            bob: 0.0,
            step_dist: 0.0,
            land_dip: 0.0,
            visibility: 0.0,
            vis_timer: 0.0,
            moving: false,
            sprinting: false,
            hurt: 0.0,
            recoil: 0.0,
            fall_v: 0.0,
            zoom: false,
            speed: 0.0,
            noise_cd: 0.0,
        }
    }
    pub fn eye(&self) -> Vec3 {
        self.feet + Vec3::Y * self.eye_h
    }
    pub fn forward(&self) -> Vec3 {
        look_dir(self.yaw, self.pitch)
    }
    pub fn right(&self) -> Vec3 {
        Vec3::new(self.yaw.cos(), 0.0, -self.yaw.sin())
    }
}

impl Default for Me {
    fn default() -> Self {
        Me::new(Vec3::ZERO, 0.0)
    }
}

#[derive(Component)]
pub struct Flashlight;

/// The point the camera actually renders from (eye + lean + bob).
#[derive(Resource, Default)]
pub struct CamPose {
    pub pos: Vec3,
    pub rot: Quat,
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlayerLogic;

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Me>()
            .init_resource::<CamPose>()
            .init_resource::<Inventory>()
            .init_resource::<Skills>()
            .init_resource::<Augs>()
            .add_systems(OnEnter(AppState::InGame), reset_player)
            .add_systems(
                Update,
                (player_look, player_move, player_keys, player_stats, player_damage, player_visibility, publish_player)
                    .chain()
                    .in_set(GameSet::Logic)
                    .in_set(PlayerLogic)
                    .run_if(in_game),
            )
            .add_systems(Update, (apply_pickups, apply_revive).in_set(GameSet::Apply).run_if(in_game))
            .add_systems(Update, player_camera.in_set(GameSet::Present).run_if(in_game))
            .add_systems(Update, (cursor_grab, update_flashlight, pause_time));
    }
}

fn reset_player(mut commands: Commands, level: Res<Level>, mut players: ResMut<Players>, settings: Res<Settings>, local: Res<LocalId>) {
    let spawn = level.spawns[local.0 as usize % level.spawns.len()];
    commands.insert_resource(Me::new(spawn, 0.0));
    commands.insert_resource(Inventory::default());
    commands.insert_resource(Skills::default());
    commands.insert_resource(Augs::default());
    players.map.clear();
    players.map.insert(local.0, PlayerEntry { name: settings.name.clone(), net: PlayerNet::default(), avatar: None, visibility: 0.0 });
}

fn player_look(motion: Res<AccumulatedMouseMotion>, ui: Res<UiMode>, settings: Res<Settings>, mut me: ResMut<Me>) {
    if *ui != UiMode::None || me.dead {
        return;
    }
    let zoom_k = if me.zoom { 0.45 } else { 1.0 };
    let k = 0.0022 * settings.sensitivity * zoom_k;
    me.yaw = wrap_angle(me.yaw - motion.delta.x * k);
    me.pitch = (me.pitch - motion.delta.y * k).clamp(-1.52, 1.52);
}

#[allow(clippy::too_many_arguments)]
fn player_move(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    ui: Res<UiMode>,
    phys: Res<Physics>,
    level: Res<Level>,
    augs: Res<Augs>,
    skills: Res<Skills>,
    mut me: ResMut<Me>,
    mut snd: EventWriter<SoundEv>,
    mut req: EventWriter<WorldRequest>,
) {
    let dt = time.delta_secs().min(0.05);
    let input = *ui == UiMode::None && !me.dead;
    let pressed = |k: KeyCode| input && keys.pressed(k);

    // crouch
    if input && keys.just_pressed(KeyCode::KeyC) {
        me.crouch = !me.crouch;
    }
    let want_crouch = me.crouch || pressed(KeyCode::ControlLeft);
    if want_crouch {
        me.col_h = CROUCH_H;
    } else if me.col_h < STAND_H {
        if phys.is_free(me.feet, HW, STAND_H) {
            me.col_h = STAND_H;
        } else {
            me.crouch = true;
        }
    }
    let crouched = me.col_h < STAND_H;
    let eye_target = me.col_h - if crouched { 0.18 } else { 0.14 };
    me.eye_h += (eye_target - me.eye_h) * (dt * 10.0).min(1.0);

    // wish direction
    let fwd = Vec3::new(-me.yaw.sin(), 0.0, -me.yaw.cos());
    let right = me.right();
    let mut wish = Vec3::ZERO;
    if pressed(KeyCode::KeyW) {
        wish += fwd;
    }
    if pressed(KeyCode::KeyS) {
        wish -= fwd;
    }
    if pressed(KeyCode::KeyD) {
        wish += right;
    }
    if pressed(KeyCode::KeyA) {
        wish -= right;
    }
    let wish = wish.normalize_or_zero();
    let ath = skills.get(Skill::Athletics) as f32;
    let speed_aug = if augs.on(Aug::Speed) { 1.55 } else { 1.0 };
    me.sprinting = pressed(KeyCode::ShiftLeft) && !crouched && wish != Vec3::ZERO && !me.zoom;
    let mut speed = if crouched {
        1.9
    } else if me.sprinting {
        6.4
    } else {
        4.1
    };
    if me.zoom {
        speed *= 0.6;
    }
    speed *= speed_aug * (1.0 + ath * 0.07);
    let target = wish * speed;
    let accel = if me.grounded { 12.0 } else { 2.5 };
    let k = exp_decay(accel, dt);
    let hv = Vec3::new(me.vel.x, 0.0, me.vel.z).lerp(target, k);
    me.vel.x = hv.x;
    me.vel.z = hv.z;

    // jump
    if input && keys.just_pressed(KeyCode::Space) && me.grounded {
        me.vel.y = 5.3 * if augs.on(Aug::Speed) { 1.3 } else { 1.0 } * (1.0 + ath * 0.05);
        me.grounded = false;
    }
    me.vel.y -= 14.0 * dt;
    me.vel.y = me.vel.y.max(-30.0);

    let was_grounded = me.grounded;
    let step = if me.grounded { 0.45 } else { 0.35 };
    let out = phys.move_body(me.feet, HW, me.col_h, me.vel * dt, step);
    let moved = out.pos - me.feet;
    me.feet = out.pos;
    if out.grounded {
        if !was_grounded && me.fall_v < -5.5 {
            me.land_dip = (-me.fall_v * 0.012).min(0.12);
            snd.write(SoundEv::at(Sfx::Land, me.feet, (-me.fall_v / 12.0).min(1.0)));
            if me.fall_v < -13.0 {
                let dmg = (-me.fall_v - 13.0) * 9.0;
                me.health -= dmg;
                me.hurt = 0.6;
            }
            if me.fall_v < -7.0 {
                req.write(WorldRequest::Noise { pos: a3(me.feet), radius: 7.0 });
            }
        }
        me.vel.y = me.vel.y.max(0.0);
        me.grounded = true;
    } else {
        me.grounded = false;
    }
    if out.hit_ceiling {
        me.vel.y = me.vel.y.min(0.0);
    }
    me.fall_v = me.vel.y;
    if out.hit_wall {
        // kill velocity into walls so we don't keep sliding
        let actual = moved / dt.max(1e-4);
        me.vel.x = actual.x;
        me.vel.z = actual.z;
    }
    let hspeed = Vec2::new(moved.x, moved.z).length() / dt.max(1e-4);
    me.speed = hspeed;
    me.moving = hspeed > 0.5;

    // footsteps + noise
    if me.grounded && me.moving {
        let d = Vec2::new(moved.x, moved.z).length();
        me.step_dist += d;
        me.bob += d * 1.65;
        let stride = if me.sprinting { 2.3 } else if crouched { 1.3 } else { 1.9 };
        if me.step_dist > stride {
            me.step_dist = 0.0;
            let metal = matches!(level.tile_at(me.feet), Tile::Open(Floor::Metal) | Tile::Vent | Tile::Grate) || level.tile_at(me.feet).floor() == Some(Floor::Metal);
            let vol = if crouched { 0.12 } else if me.sprinting { 0.7 } else { 0.35 };
            snd.write(SoundEv::at(if metal { Sfx::StepMetal } else { Sfx::Step }, me.feet, vol));
            let radius = if crouched {
                0.0
            } else if me.sprinting {
                if metal { 16.0 } else { 12.0 }
            } else if metal {
                6.0
            } else {
                3.5
            };
            if radius > 0.0 && me.noise_cd <= 0.0 {
                req.write(WorldRequest::Noise { pos: a3(me.feet), radius });
                me.noise_cd = 0.4;
            }
        }
    }
    me.noise_cd -= dt;

    // lean
    let mut lean_target = 0.0;
    if pressed(KeyCode::KeyQ) {
        lean_target -= 1.0;
    }
    if pressed(KeyCode::KeyE) {
        lean_target += 1.0;
    }
    me.lean += (lean_target - me.lean) * (dt * 9.0).min(1.0);
    me.land_dip *= 1.0 - (dt * 6.0).min(1.0);
    me.recoil *= 1.0 - (dt * 9.0).min(1.0);
    me.hurt = (me.hurt - dt).max(0.0);
}

fn player_keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut ui: ResMut<UiMode>,
    mut dirty: ResMut<UiDirty>,
    mut me: ResMut<Me>,
    mut augs: ResMut<Augs>,
    mut inv: ResMut<Inventory>,
    skills: Res<Skills>,
    mut snd: EventWriter<SoundEv>,
    mut msgs: EventWriter<HudMsg>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        *ui = match *ui {
            UiMode::None => UiMode::Pause,
            UiMode::Complete => UiMode::Complete,
            _ => UiMode::None,
        };
        dirty.0 = true;
        return;
    }
    if keys.just_pressed(KeyCode::Tab) || keys.just_pressed(KeyCode::KeyI) {
        *ui = match *ui {
            UiMode::None => UiMode::Inventory(0),
            UiMode::Inventory(_) => UiMode::None,
            ref other => other.clone(),
        };
        dirty.0 = true;
        return;
    }
    if *ui != UiMode::None || me.dead {
        return;
    }
    if keys.just_pressed(KeyCode::KeyL) {
        me.flashlight = !me.flashlight;
        snd.write(SoundEv::ui(Sfx::Click, 0.5));
    }
    if keys.just_pressed(KeyCode::KeyH) {
        use_consumable(ItemKind::Medkit, &mut me, &mut inv, &skills, &mut snd, &mut msgs, &mut ui);
    }
    let fkeys = [KeyCode::F1, KeyCode::F2, KeyCode::F3, KeyCode::F4, KeyCode::F5, KeyCode::F6];
    for (i, k) in fkeys.iter().enumerate() {
        if keys.just_pressed(*k) {
            if !augs.installed[i] {
                msgs.write(HudMsg(format!("{} not installed", AUGS[i].name())));
                continue;
            }
            if !augs.active[i] && me.energy <= 0.5 {
                msgs.write(HudMsg("No bioelectric energy".into()));
                snd.write(SoundEv::ui(Sfx::Denied, 0.5));
                continue;
            }
            augs.active[i] = !augs.active[i];
            snd.write(SoundEv::ui(if augs.active[i] { Sfx::AugOn } else { Sfx::AugOff }, 0.5));
        }
    }
}

pub fn use_consumable(
    kind: ItemKind,
    me: &mut Me,
    inv: &mut Inventory,
    skills: &Skills,
    snd: &mut EventWriter<SoundEv>,
    msgs: &mut EventWriter<HudMsg>,
    ui: &mut UiMode,
) -> bool {
    match kind {
        ItemKind::Medkit => {
            if me.health >= 100.0 {
                msgs.write(HudMsg("Health is already full".into()));
                return false;
            }
            if inv.consume(ItemKind::Medkit, 1) {
                let heal = 30.0 + 12.0 * skills.get(Skill::Medicine) as f32;
                me.health = (me.health + heal).min(100.0);
                snd.write(SoundEv::ui(Sfx::Heal, 0.6));
                msgs.write(HudMsg(format!("Medkit used (+{heal:.0})")));
                return true;
            }
            msgs.write(HudMsg("No medkits".into()));
        }
        ItemKind::Biocell => {
            if me.energy >= 100.0 {
                msgs.write(HudMsg("Energy is already full".into()));
                return false;
            }
            if inv.consume(ItemKind::Biocell, 1) {
                me.energy = (me.energy + 30.0).min(100.0);
                snd.write(SoundEv::ui(Sfx::AugOn, 0.5));
                msgs.write(HudMsg("Bioelectric cell used (+30)".into()));
                return true;
            }
        }
        ItemKind::AugCanister => {
            *ui = UiMode::AugChoice;
            return true;
        }
        _ => {}
    }
    false
}

fn player_stats(time: Res<Time>, mut me: ResMut<Me>, mut augs: ResMut<Augs>, level: Res<Level>, mut snd: EventWriter<SoundEv>, mut msgs: EventWriter<HudMsg>) {
    let dt = time.delta_secs();
    if me.dead {
        me.dead_t += dt;
        if me.dead_t > 5.0 {
            let spawn = level.spawns[0];
            let energy = me.energy;
            *me = Me::new(spawn, 0.0);
            me.energy = energy.max(20.0);
            msgs.write(HudMsg("Recovered at the insertion point".into()));
        }
        return;
    }
    let mut drain = 0.0;
    for (i, a) in AUGS.iter().enumerate() {
        if augs.active[i] {
            drain += a.drain();
        }
    }
    if drain > 0.0 {
        me.energy -= drain * dt;
        if me.energy <= 0.0 {
            me.energy = 0.0;
            augs.active = [false; 6];
            snd.write(SoundEv::ui(Sfx::AugOff, 0.6));
            msgs.write(HudMsg("Bioelectric energy depleted".into()));
        }
    }
    if augs.on(Aug::Regen) && me.health < 100.0 {
        me.health = (me.health + 9.0 * dt).min(100.0);
    }
}

fn player_damage(mut evs: EventReader<DamageEv>, mut me: ResMut<Me>, augs: Res<Augs>, mut snd: EventWriter<SoundEv>) {
    for e in evs.read() {
        if me.dead {
            continue;
        }
        let mut dmg = e.amount;
        if augs.on(Aug::Ballistic) {
            dmg *= 0.5;
        }
        me.health -= dmg;
        me.hurt = (me.hurt + dmg / 25.0).min(1.0);
        snd.write(SoundEv::ui(Sfx::Hurt, (dmg / 20.0).clamp(0.3, 1.0)));
    }
    if me.health <= 0.0 && !me.dead {
        me.health = 0.0;
        me.dead = true;
        me.dead_t = 0.0;
        me.flashlight = false;
    }
}

fn player_visibility(time: Res<Time>, mut me: ResMut<Me>, reg: Res<LightReg>, phys: Res<Physics>, level: Res<Level>) {
    me.vis_timer -= time.delta_secs();
    if me.vis_timer > 0.0 {
        return;
    }
    me.vis_timer = 0.1;
    let chest = me.feet + Vec3::Y * (me.col_h * 0.6);
    let mut v = light_level(chest, &reg, &phys, !level.indoor_at(chest));
    if me.flashlight {
        v = (v + 0.45).min(1.0);
    }
    me.visibility += (v - me.visibility) * 0.5;
}

fn publish_player(me: Res<Me>, augs: Res<Augs>, inv: Res<Inventory>, local: Res<LocalId>, mut players: ResMut<Players>) {
    let Some(entry) = players.map.get_mut(&local.0) else { return };
    let mut flags = 0u8;
    if me.col_h < STAND_H {
        flags |= pflags::CROUCH;
    }
    if me.flashlight {
        flags |= pflags::LIGHT;
    }
    if augs.on(Aug::Cloak) {
        flags |= pflags::CLOAK;
    }
    if augs.on(Aug::Radar) {
        flags |= pflags::RADAR;
    }
    if me.dead {
        flags |= pflags::DOWNED;
    }
    if inv.has_core() {
        flags |= pflags::CORE;
    }
    if me.moving {
        flags |= pflags::MOVING;
    }
    if me.sprinting {
        flags |= pflags::SPRINT;
    }
    entry.net = PlayerNet {
        pos: a3(me.feet),
        yaw: me.yaw,
        pitch: me.pitch,
        flags,
        weapon: inv.equipped_weapon().map(|(_, w)| weapon_def(w).id).unwrap_or(255),
        health: me.health.clamp(0.0, 255.0) as u8,
    };
    entry.visibility = me.visibility;
}

fn apply_pickups(
    mut evs: EventReader<WorldEv>,
    local: Res<LocalId>,
    level: Res<Level>,
    mut inv: ResMut<Inventory>,
    mut skills: ResMut<Skills>,
    mut msgs: EventWriter<HudMsg>,
    mut snd: EventWriter<SoundEv>,
) {
    for WorldEv { ev, quiet } in evs.read() {
        match ev {
            WorldEvent::PickupTaken { id, by } if *by == local.0 => {
                let Some(p) = level.pickups.get(*id as usize) else { continue };
                let name = item_name(p.item, &level);
                if inv.add(p.item, p.count) {
                    let what = match p.item {
                        ItemKind::Ammo(_) => format!("{} x{}", name, p.count),
                        ItemKind::Lockpick | ItemKind::Multitool if p.count > 1 => format!("{} x{}", name, p.count),
                        _ => name,
                    };
                    msgs.write(HudMsg(format!("Picked up {what}")));
                    snd.write(SoundEv::ui(Sfx::Pickup, 0.6));
                } else {
                    msgs.write(HudMsg("Inventory full".into()));
                }
            }
            WorldEvent::BodyLooted { by, pistol, rifle, darts, .. } if *by == local.0 => {
                inv.ammo[AmmoKind::Pistol.idx()] += pistol;
                inv.ammo[AmmoKind::Rifle.idx()] += rifle;
                inv.ammo[AmmoKind::Dart.idx()] += darts;
                let mut parts = vec![];
                if *pistol > 0 {
                    parts.push(format!("{pistol} 10mm"));
                }
                if *rifle > 0 {
                    parts.push(format!("{rifle} 7.62mm"));
                }
                if *darts > 0 {
                    parts.push(format!("{darts} darts"));
                }
                msgs.write(HudMsg(if parts.is_empty() { "Found nothing useful".into() } else { format!("Searched body: {}", parts.join(", ")) }));
                snd.write(SoundEv::ui(Sfx::Pickup, 0.5));
            }
            WorldEvent::Objective { .. } if !quiet => {
                skills.points += 750;
                msgs.write(HudMsg("+750 skill points".into()));
            }
            WorldEvent::Note { .. } if !quiet => {
                skills.points += 100;
            }
            _ => {}
        }
    }
}

fn apply_revive(mut evs: EventReader<WorldEv>, local: Res<LocalId>, mut me: ResMut<Me>) {
    for WorldEv { ev, .. } in evs.read() {
        if let WorldEvent::Revived { target } = ev {
            if *target == local.0 && me.dead {
                me.dead = false;
                me.health = 35.0;
            }
        }
    }
}

fn player_camera(
    me: Res<Me>,
    phys: Res<Physics>,
    mut pose: ResMut<CamPose>,
    mut cam: Query<(&mut Transform, &mut Projection), With<MainCamera>>,
    settings: Res<Settings>,
    inv: Res<Inventory>,
    time: Res<Time>,
) {
    let Ok((mut tf, mut proj)) = cam.single_mut() else { return };
    let mut eye = me.eye();
    if me.dead {
        let k = (me.dead_t * 2.0).min(1.0);
        eye = me.feet + Vec3::Y * lerp(me.eye_h, 0.25, k);
    }
    // bob
    let bob_amp = (me.speed / 6.0).min(1.2) * if me.grounded { 1.0 } else { 0.0 };
    let bob_y = (me.bob * 2.0).sin() * 0.035 * bob_amp - me.land_dip;
    let bob_x = me.bob.sin() * 0.02 * bob_amp;
    let right = me.right();
    // lean with wall check
    let mut lean = me.lean;
    if lean.abs() > 0.01 {
        let dir = right * lean.signum();
        let max = phys.raycast(eye, dir, 0.75, false, false, None).map(|h| (h.t - 0.22).max(0.0)).unwrap_or(0.75);
        let want = lean.abs() * 0.6;
        if want > max {
            lean = max / 0.6 * lean.signum();
        }
    }
    let pos = eye + right * (lean * 0.6 + bob_x) + Vec3::Y * (bob_y - lean.abs() * 0.08);
    let roll = if me.dead { 0.5 * (me.dead_t * 2.0).min(1.0) } else { -lean * 0.14 };
    let rot = Quat::from_euler(EulerRot::YXZ, me.yaw, me.pitch + me.recoil, roll);
    tf.translation = pos;
    tf.rotation = rot;
    pose.pos = pos;
    pose.rot = rot;
    // fov / zoom
    let mut fov = settings.fov;
    if me.zoom {
        fov = match inv.equipped_weapon().map(|(_, w)| w) {
            Some(WeaponKind::Rifle) => 32.0,
            Some(WeaponKind::Crossbow) => 36.0,
            _ => 58.0,
        };
    }
    if me.sprinting {
        fov += 4.0;
    }
    if let Projection::Perspective(p) = proj.as_mut() {
        let target = fov.to_radians();
        p.fov += (target - p.fov) * (time.delta_secs() * 12.0).min(1.0);
        p.near = 0.03;
    }
}

fn update_flashlight(me: Res<Me>, state: Res<State<AppState>>, mut q: Query<&mut SpotLight, With<Flashlight>>) {
    let in_game = *state.get() == AppState::InGame;
    for mut s in q.iter_mut() {
        let want = if in_game && me.flashlight && !me.dead { 260_000.0 } else { 0.0 };
        if (s.intensity - want).abs() > 1.0 {
            s.intensity = want;
        }
    }
}

/// Single player: freeze the world while menus that should pause are open.
fn pause_time(ui: Res<UiMode>, state: Res<State<AppState>>, mut time: ResMut<Time<Virtual>>) {
    let pause = *state.get() == AppState::InGame && matches!(*ui, UiMode::Pause | UiMode::Inventory(_) | UiMode::Read(_) | UiMode::AugChoice | UiMode::Complete);
    if pause && !time.is_paused() {
        time.pause();
    } else if !pause && time.is_paused() {
        time.unpause();
    }
}

fn cursor_grab(mut windows: Query<&mut Window, With<PrimaryWindow>>, ui: Res<UiMode>, state: Res<State<AppState>>) {
    let Ok(mut w) = windows.single_mut() else { return };
    let lock = *state.get() == AppState::InGame && *ui == UiMode::None && w.focused;
    // winit cannot lock the cursor on Windows; confining it works and raw mouse motion still arrives
    let grab = if cfg!(target_os = "windows") { CursorGrabMode::Confined } else { CursorGrabMode::Locked };
    let (mode, vis) = if lock { (grab, false) } else { (CursorGrabMode::None, true) };
    if w.cursor_options.grab_mode != mode {
        w.cursor_options.grab_mode = mode;
    }
    if w.cursor_options.visible != vis {
        w.cursor_options.visible = vis;
    }
}
