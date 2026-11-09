//! Items, inventory grid, belt, weapon stats, skills and augmentations.
use bevy::ecs::hierarchy::ChildSpawnerCommands;
use bevy::prelude::*;
use std::f32::consts::FRAC_PI_2;

use crate::game::*;
use crate::level::Level;
use crate::materials::{Mats, Shapes};

pub const GRID_W: u8 = 6;
pub const GRID_H: u8 = 4;

// ------------------------------------------------------------------ weapons

pub struct WeaponDef {
    pub name: &'static str,
    pub mag: u16,
    pub ammo: Option<AmmoKind>,
    pub dmg: f32,
    pub rate: f32,
    pub spread_min: f32,
    pub spread_max: f32,
    pub kick: f32,
    pub recoil: f32,
    pub noise: f32,
    pub kind: DmgKind,
    pub auto: bool,
    pub range: f32,
    pub reload: f32,
    pub id: u8,
}

pub fn weapon_def(w: WeaponKind) -> WeaponDef {
    match w {
        WeaponKind::Baton => WeaponDef {
            name: "Stun Baton",
            mag: 0,
            ammo: None,
            dmg: 34.0,
            rate: 0.55,
            spread_min: 0.0,
            spread_max: 0.0,
            kick: 0.0,
            recoil: 0.0,
            noise: 3.0,
            kind: DmgKind::Melee,
            auto: false,
            range: 2.2,
            reload: 0.0,
            id: 0,
        },
        WeaponKind::Pistol => WeaponDef {
            name: "Pistol",
            mag: 10,
            ammo: Some(AmmoKind::Pistol),
            dmg: 26.0,
            rate: 0.26,
            spread_min: 0.5,
            spread_max: 7.0,
            kick: 2.4,
            recoil: 0.028,
            noise: 30.0,
            kind: DmgKind::Bullet,
            auto: false,
            range: 120.0,
            reload: 1.5,
            id: 1,
        },
        WeaponKind::Crossbow => WeaponDef {
            name: "Tranq Crossbow",
            mag: 4,
            ammo: Some(AmmoKind::Dart),
            dmg: 8.0,
            rate: 0.9,
            spread_min: 0.25,
            spread_max: 4.0,
            kick: 1.0,
            recoil: 0.012,
            noise: 2.5,
            kind: DmgKind::Tranq,
            auto: false,
            range: 60.0,
            reload: 2.0,
            id: 2,
        },
        WeaponKind::Rifle => WeaponDef {
            name: "Assault Rifle",
            mag: 30,
            ammo: Some(AmmoKind::Rifle),
            dmg: 19.0,
            rate: 0.095,
            spread_min: 1.0,
            spread_max: 9.0,
            kick: 1.2,
            recoil: 0.013,
            noise: 40.0,
            kind: DmgKind::Bullet,
            auto: true,
            range: 150.0,
            reload: 2.3,
            id: 3,
        },
    }
}

pub fn weapon_from_id(id: u8) -> Option<WeaponKind> {
    match id {
        0 => Some(WeaponKind::Baton),
        1 => Some(WeaponKind::Pistol),
        2 => Some(WeaponKind::Crossbow),
        3 => Some(WeaponKind::Rifle),
        _ => None,
    }
}

// ------------------------------------------------------------------ item info

pub fn item_size(k: ItemKind) -> (u8, u8) {
    match k {
        ItemKind::Weapon(WeaponKind::Rifle) => (3, 1),
        ItemKind::Weapon(WeaponKind::Crossbow) => (2, 1),
        ItemKind::Weapon(WeaponKind::Baton) => (1, 2),
        _ => (1, 1),
    }
}

pub fn max_stack(k: ItemKind) -> u16 {
    match k {
        ItemKind::Medkit => 10,
        ItemKind::Biocell => 15,
        ItemKind::Lockpick | ItemKind::Multitool => 20,
        _ => 1,
    }
}

pub fn item_name(k: ItemKind, lvl: &Level) -> String {
    match k {
        ItemKind::Weapon(w) => weapon_def(w).name.to_string(),
        ItemKind::Ammo(a) => a.name().to_string(),
        ItemKind::Medkit => "Medkit".into(),
        ItemKind::Biocell => "Biocell".into(),
        ItemKind::Lockpick => "Lockpick".into(),
        ItemKind::Multitool => "Multitool".into(),
        ItemKind::Key(id) => lvl.key_name(id),
        ItemKind::Datacube(_) => "Datacube".into(),
        ItemKind::DataCore => "LAZARUS Data Core".into(),
        ItemKind::AugCanister => "Augmentation Canister".into(),
    }
}

pub fn item_short(k: ItemKind) -> &'static str {
    match k {
        ItemKind::Weapon(WeaponKind::Baton) => "BATON",
        ItemKind::Weapon(WeaponKind::Pistol) => "PISTOL",
        ItemKind::Weapon(WeaponKind::Crossbow) => "XBOW",
        ItemKind::Weapon(WeaponKind::Rifle) => "RIFLE",
        ItemKind::Medkit => "MEDKIT",
        ItemKind::Biocell => "BIOCELL",
        ItemKind::Lockpick => "PICK",
        ItemKind::Multitool => "MULTI",
        ItemKind::DataCore => "CORE",
        ItemKind::AugCanister => "AUG",
        _ => "ITEM",
    }
}

pub fn item_desc(k: ItemKind) -> &'static str {
    match k {
        ItemKind::Weapon(WeaponKind::Baton) => "Non-lethal. Knocks targets out. A strike from behind on an unaware target drops them instantly.",
        ItemKind::Weapon(WeaponKind::Pistol) => "Reliable 10mm sidearm. Loud. Headshots are lethal.",
        ItemKind::Weapon(WeaponKind::Crossbow) => "Silent. Tranquilizer darts put a target to sleep after a few seconds (instantly on a headshot).",
        ItemKind::Weapon(WeaponKind::Rifle) => "Fully automatic. Devastating, but every guard in the district will hear it.",
        ItemKind::Medkit => "Restores health. Medicine skill increases the effect.",
        ItemKind::Biocell => "Recharges bioelectric energy for augmentations.",
        ItemKind::Lockpick => "Used automatically on mechanical locks. Lockpicking skill reduces how many are needed.",
        ItemKind::Multitool => "Bypasses electronic keypads. Electronics skill reduces how many are needed.",
        ItemKind::DataCore => "The LAZARUS research core. Bring it to the extraction point.",
        ItemKind::AugCanister => "Nano-augmentation canister. Use it to install a new augmentation.",
        _ => "",
    }
}

pub fn weapon_ammo_bonus(w: WeaponKind) -> u16 {
    match w {
        WeaponKind::Pistol => 10,
        WeaponKind::Rifle => 30,
        WeaponKind::Crossbow => 4,
        WeaponKind::Baton => 0,
    }
}

// ------------------------------------------------------------------ inventory

#[derive(Clone, Debug)]
pub struct InvItem {
    pub uid: u32,
    pub kind: ItemKind,
    pub count: u16,
    pub mag: u16,
    pub gx: u8,
    pub gy: u8,
}

#[derive(Resource, Clone)]
pub struct Inventory {
    pub items: Vec<InvItem>,
    pub ammo: [u16; 3],
    pub keys: Vec<u8>,
    pub belt: [Option<u32>; 10],
    pub equipped: Option<u32>,
    pub next_uid: u32,
}

impl Default for Inventory {
    fn default() -> Self {
        let mut inv = Inventory { items: vec![], ammo: [0; 3], keys: vec![], belt: [None; 10], equipped: None, next_uid: 1 };
        inv.add(ItemKind::Weapon(WeaponKind::Baton), 1);
        inv.add(ItemKind::Weapon(WeaponKind::Pistol), 1);
        inv.ammo[AmmoKind::Pistol.idx()] = 20;
        inv.add(ItemKind::Lockpick, 2);
        inv.add(ItemKind::Multitool, 1);
        inv.add(ItemKind::Medkit, 1);
        inv.add(ItemKind::Biocell, 1);
        inv.equipped = inv.belt[1];
        inv
    }
}

impl Inventory {
    fn grid(&self, ignore: Option<u32>) -> [[bool; GRID_W as usize]; GRID_H as usize] {
        let mut g = [[false; GRID_W as usize]; GRID_H as usize];
        for it in &self.items {
            if Some(it.uid) == ignore {
                continue;
            }
            let (w, h) = item_size(it.kind);
            for y in it.gy..(it.gy + h).min(GRID_H) {
                for x in it.gx..(it.gx + w).min(GRID_W) {
                    g[y as usize][x as usize] = true;
                }
            }
        }
        g
    }

    pub fn find_space(&self, w: u8, h: u8) -> Option<(u8, u8)> {
        let g = self.grid(None);
        for y in 0..=GRID_H.saturating_sub(h) {
            for x in 0..=GRID_W.saturating_sub(w) {
                let mut ok = true;
                'c: for yy in y..y + h {
                    for xx in x..x + w {
                        if g[yy as usize][xx as usize] {
                            ok = false;
                            break 'c;
                        }
                    }
                }
                if ok {
                    return Some((x, y));
                }
            }
        }
        None
    }

    pub fn has_weapon(&self, w: WeaponKind) -> bool {
        self.items.iter().any(|i| i.kind == ItemKind::Weapon(w))
    }

    pub fn can_take(&self, k: ItemKind) -> bool {
        match k {
            ItemKind::Ammo(_) | ItemKind::Key(_) | ItemKind::Datacube(_) => true,
            ItemKind::Weapon(w) if self.has_weapon(w) => true,
            _ => {
                let max = max_stack(k);
                if max > 1 && self.items.iter().any(|i| i.kind == k && i.count < max) {
                    return true;
                }
                let (w, h) = item_size(k);
                self.find_space(w, h).is_some()
            }
        }
    }

    /// Returns false if there was no room.
    pub fn add(&mut self, k: ItemKind, count: u16) -> bool {
        match k {
            ItemKind::Ammo(a) => {
                self.ammo[a.idx()] = (self.ammo[a.idx()] + count).min(999);
                true
            }
            ItemKind::Key(id) => {
                if !self.keys.contains(&id) {
                    self.keys.push(id);
                }
                true
            }
            ItemKind::Datacube(_) => true,
            _ => {
                if let ItemKind::Weapon(w) = k {
                    if self.has_weapon(w) {
                        if let Some(a) = weapon_def(w).ammo {
                            self.ammo[a.idx()] += weapon_ammo_bonus(w);
                        }
                        return true;
                    }
                }
                let max = max_stack(k);
                if max > 1 {
                    if let Some(it) = self.items.iter_mut().find(|i| i.kind == k && i.count < max) {
                        it.count = (it.count + count).min(max);
                        return true;
                    }
                }
                let (w, h) = item_size(k);
                let Some((gx, gy)) = self.find_space(w, h) else { return false };
                let uid = self.next_uid;
                self.next_uid += 1;
                let mag = match k {
                    ItemKind::Weapon(w) => weapon_def(w).mag,
                    _ => 0,
                };
                self.items.push(InvItem { uid, kind: k, count: count.max(1), mag, gx, gy });
                let beltable = matches!(k, ItemKind::Weapon(_) | ItemKind::Medkit | ItemKind::Biocell | ItemKind::AugCanister);
                if beltable {
                    if let Some(slot) = self.belt.iter().position(|b| b.is_none()) {
                        self.belt[slot] = Some(uid);
                    }
                }
                true
            }
        }
    }

    pub fn count(&self, k: ItemKind) -> u16 {
        self.items.iter().filter(|i| i.kind == k).map(|i| i.count).sum()
    }

    pub fn consume(&mut self, k: ItemKind, n: u16) -> bool {
        if self.count(k) < n {
            return false;
        }
        let mut left = n;
        for it in self.items.iter_mut().filter(|i| i.kind == k) {
            let take = left.min(it.count);
            it.count -= take;
            left -= take;
            if left == 0 {
                break;
            }
        }
        let gone: Vec<u32> = self.items.iter().filter(|i| i.count == 0).map(|i| i.uid).collect();
        for uid in gone {
            self.remove_uid(uid);
        }
        true
    }

    pub fn remove_uid(&mut self, uid: u32) {
        self.items.retain(|i| i.uid != uid);
        for b in self.belt.iter_mut() {
            if *b == Some(uid) {
                *b = None;
            }
        }
        if self.equipped == Some(uid) {
            self.equipped = None;
        }
    }

    pub fn get(&self, uid: u32) -> Option<&InvItem> {
        self.items.iter().find(|i| i.uid == uid)
    }

    pub fn get_mut(&mut self, uid: u32) -> Option<&mut InvItem> {
        self.items.iter_mut().find(|i| i.uid == uid)
    }

    pub fn equipped_weapon(&self) -> Option<(u32, WeaponKind)> {
        let uid = self.equipped?;
        match self.get(uid)?.kind {
            ItemKind::Weapon(w) => Some((uid, w)),
            _ => None,
        }
    }

    pub fn has_core(&self) -> bool {
        self.items.iter().any(|i| i.kind == ItemKind::DataCore)
    }

    pub fn move_item(&mut self, uid: u32, gx: u8, gy: u8) -> bool {
        let Some(it) = self.get(uid) else { return false };
        let (w, h) = item_size(it.kind);
        if gx + w > GRID_W || gy + h > GRID_H {
            return false;
        }
        let g = self.grid(Some(uid));
        for y in gy..gy + h {
            for x in gx..gx + w {
                if g[y as usize][x as usize] {
                    return false;
                }
            }
        }
        if let Some(it) = self.get_mut(uid) {
            it.gx = gx;
            it.gy = gy;
        }
        true
    }
}

// ------------------------------------------------------------------ skills

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Skill {
    Firearms,
    Melee,
    Computer,
    Electronics,
    Lockpicking,
    Medicine,
    Athletics,
}

pub const SKILLS: [Skill; 7] = [
    Skill::Firearms,
    Skill::Melee,
    Skill::Computer,
    Skill::Electronics,
    Skill::Lockpicking,
    Skill::Medicine,
    Skill::Athletics,
];

pub const SKILL_LEVELS: [&str; 4] = ["UNTRAINED", "TRAINED", "ADVANCED", "MASTER"];

impl Skill {
    pub fn name(self) -> &'static str {
        match self {
            Skill::Firearms => "Firearms",
            Skill::Melee => "Melee",
            Skill::Computer => "Computer",
            Skill::Electronics => "Electronics",
            Skill::Lockpicking => "Lockpicking",
            Skill::Medicine => "Medicine",
            Skill::Athletics => "Athletics",
        }
    }
    pub fn desc(self) -> &'static str {
        match self {
            Skill::Firearms => "Accuracy, aim recovery and reload speed with all guns.",
            Skill::Melee => "Damage and speed with the stun baton.",
            Skill::Computer => "Hacking speed and how long you stay inside a hacked system.",
            Skill::Electronics => "Fewer multitools needed to bypass keypads.",
            Skill::Lockpicking => "Fewer lockpicks needed, faster picking.",
            Skill::Medicine => "Medkits heal more.",
            Skill::Athletics => "Run faster, jump higher, recover stamina.",
        }
    }
    pub fn cost(self, to_level: u8) -> u32 {
        let base = match self {
            Skill::Firearms | Skill::Computer => 1200,
            _ => 1000,
        };
        base * match to_level {
            1 => 1,
            2 => 2,
            _ => 3,
        }
    }
}

#[derive(Resource, Clone)]
pub struct Skills {
    pub points: u32,
    pub level: [u8; 7],
}

impl Default for Skills {
    fn default() -> Self {
        let mut level = [0u8; 7];
        level[Skill::Firearms as usize] = 1;
        Skills { points: 3000, level }
    }
}

impl Skills {
    pub fn get(&self, s: Skill) -> u8 {
        self.level[s as usize]
    }
    pub fn try_upgrade(&mut self, s: Skill) -> bool {
        let l = self.get(s);
        if l >= 3 {
            return false;
        }
        let c = s.cost(l + 1);
        if self.points < c {
            return false;
        }
        self.points -= c;
        self.level[s as usize] += 1;
        true
    }
}

// ------------------------------------------------------------------ augmentations

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Aug {
    Vision,
    Cloak,
    Speed,
    Ballistic,
    Regen,
    Radar,
}

pub const AUGS: [Aug; 6] = [Aug::Vision, Aug::Cloak, Aug::Speed, Aug::Ballistic, Aug::Regen, Aug::Radar];

impl Aug {
    pub fn name(self) -> &'static str {
        match self {
            Aug::Vision => "Vision Enhancement",
            Aug::Cloak => "Cloak",
            Aug::Speed => "Speed Enhancement",
            Aug::Ballistic => "Ballistic Protection",
            Aug::Regen => "Regeneration",
            Aug::Radar => "Radar Transparency",
        }
    }
    pub fn short(self) -> &'static str {
        match self {
            Aug::Vision => "VIS",
            Aug::Cloak => "CLK",
            Aug::Speed => "SPD",
            Aug::Ballistic => "BAL",
            Aug::Regen => "REG",
            Aug::Radar => "RAD",
        }
    }
    pub fn desc(self) -> &'static str {
        match self {
            Aug::Vision => "Light-amplifying optics. See in the dark.",
            Aug::Cloak => "Bends light around you. Guards cannot see you - cameras and turrets still can.",
            Aug::Speed => "Muscle actuators: faster movement, higher jumps.",
            Aug::Ballistic => "Subdermal plating absorbs half of incoming damage.",
            Aug::Regen => "Nanites rebuild tissue rapidly. Very power hungry.",
            Aug::Radar => "Makes you invisible to cameras and turrets.",
        }
    }
    pub fn drain(self) -> f32 {
        match self {
            Aug::Vision => 0.5,
            Aug::Cloak => 4.0,
            Aug::Speed => 1.8,
            Aug::Ballistic => 1.2,
            Aug::Regen => 4.5,
            Aug::Radar => 1.8,
        }
    }
}

#[derive(Resource, Clone)]
pub struct Augs {
    pub installed: [bool; 6],
    pub active: [bool; 6],
}

impl Default for Augs {
    fn default() -> Self {
        let mut installed = [false; 6];
        installed[Aug::Vision as usize] = true;
        installed[Aug::Cloak as usize] = true;
        installed[Aug::Speed as usize] = true;
        Augs { installed, active: [false; 6] }
    }
}

impl Augs {
    pub fn on(&self, a: Aug) -> bool {
        self.active[a as usize]
    }
}

// ------------------------------------------------------------------ models

#[derive(Clone)]
pub struct PartDef {
    pub mesh: Handle<Mesh>,
    pub mat: Handle<StandardMaterial>,
    pub tf: Transform,
}

pub fn spawn_parts(p: &mut ChildSpawnerCommands, parts: &[PartDef]) {
    for part in parts {
        p.spawn((Mesh3d(part.mesh.clone()), MeshMaterial3d(part.mat.clone()), part.tf));
    }
}

fn bx(s: &Shapes, m: &Handle<StandardMaterial>, pos: Vec3, size: Vec3) -> PartDef {
    PartDef { mesh: s.cube.clone(), mat: m.clone(), tf: Transform::from_translation(pos).with_scale(size) }
}

fn bxr(s: &Shapes, m: &Handle<StandardMaterial>, pos: Vec3, size: Vec3, rot: Quat) -> PartDef {
    PartDef { mesh: s.cube.clone(), mat: m.clone(), tf: Transform::from_translation(pos).with_rotation(rot).with_scale(size) }
}

/// cylinder along Z
fn cylz(s: &Shapes, m: &Handle<StandardMaterial>, pos: Vec3, r: f32, len: f32) -> PartDef {
    PartDef {
        mesh: s.cylinder.clone(),
        mat: m.clone(),
        tf: Transform::from_translation(pos).with_rotation(Quat::from_rotation_x(FRAC_PI_2)).with_scale(Vec3::new(r * 2.0, len, r * 2.0)),
    }
}

/// cylinder along Y
fn cyly(s: &Shapes, m: &Handle<StandardMaterial>, pos: Vec3, r: f32, len: f32) -> PartDef {
    PartDef { mesh: s.cylinder.clone(), mat: m.clone(), tf: Transform::from_translation(pos).with_scale(Vec3::new(r * 2.0, len, r * 2.0)) }
}

fn sph(s: &Shapes, m: &Handle<StandardMaterial>, pos: Vec3, r: f32) -> PartDef {
    PartDef { mesh: s.sphere_lo.clone(), mat: m.clone(), tf: Transform::from_translation(pos).with_scale(Vec3::splat(r * 2.0)) }
}

/// Weapon model, barrel pointing -Z, origin at the grip. Real-world scale (meters).
pub fn weapon_parts(w: WeaponKind, m: &Mats, s: &Shapes) -> Vec<PartDef> {
    match w {
        WeaponKind::Pistol => vec![
            bx(s, &m.gunmetal, Vec3::new(0.0, 0.035, -0.04), Vec3::new(0.034, 0.034, 0.19)),
            bx(s, &m.gun_dark, Vec3::new(0.0, 0.006, -0.03), Vec3::new(0.03, 0.026, 0.16)),
            bxr(s, &m.plastic, Vec3::new(0.0, -0.05, 0.03), Vec3::new(0.03, 0.11, 0.046), Quat::from_rotation_x(0.25)),
            bx(s, &m.gun_dark, Vec3::new(0.0, -0.012, -0.015), Vec3::new(0.006, 0.026, 0.045)),
            cylz(s, &m.black, Vec3::new(0.0, 0.035, -0.14), 0.007, 0.012),
            bx(s, &m.gun_dark, Vec3::new(0.0, 0.056, -0.125), Vec3::new(0.006, 0.009, 0.006)),
            bx(s, &m.gun_dark, Vec3::new(0.0, 0.056, 0.045), Vec3::new(0.016, 0.009, 0.006)),
            sph(s, &m.cyan_glow, Vec3::new(0.0, 0.0605, -0.125), 0.0009),
            sph(s, &m.cyan_glow, Vec3::new(-0.005, 0.0605, 0.045), 0.0009),
            sph(s, &m.cyan_glow, Vec3::new(0.005, 0.0605, 0.045), 0.0009),
        ],
        WeaponKind::Rifle => vec![
            bx(s, &m.gunmetal, Vec3::new(0.0, 0.0, -0.05), Vec3::new(0.05, 0.07, 0.34)),
            cylz(s, &m.gun_dark, Vec3::new(0.0, 0.01, -0.38), 0.011, 0.3),
            bx(s, &m.plastic, Vec3::new(0.0, 0.005, -0.3), Vec3::new(0.056, 0.058, 0.2)),
            bxr(s, &m.gun_dark, Vec3::new(0.0, -0.085, -0.09), Vec3::new(0.03, 0.12, 0.06), Quat::from_rotation_x(-0.18)),
            bx(s, &m.plastic, Vec3::new(0.0, -0.015, 0.21), Vec3::new(0.04, 0.085, 0.2)),
            bxr(s, &m.plastic, Vec3::new(0.0, -0.07, 0.06), Vec3::new(0.03, 0.09, 0.04), Quat::from_rotation_x(0.25)),
            cylz(s, &m.gun_dark, Vec3::new(0.0, 0.07, -0.06), 0.018, 0.15),
            sph(s, &m.led_red, Vec3::new(0.0, 0.07, -0.135), 0.004),
            bx(s, &m.gun_dark, Vec3::new(0.0, 0.045, -0.06), Vec3::new(0.012, 0.02, 0.05)),
        ],
        WeaponKind::Crossbow => vec![
            bx(s, &m.wood, Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.04, 0.05, 0.42)),
            bx(s, &m.gunmetal, Vec3::new(0.0, 0.02, -0.2), Vec3::new(0.44, 0.016, 0.03)),
            bx(s, &m.metal, Vec3::new(0.0, 0.02, -0.15), Vec3::new(0.42, 0.003, 0.003)),
            bx(s, &m.yellow, Vec3::new(0.0, 0.036, -0.1), Vec3::new(0.006, 0.006, 0.25)),
            bxr(s, &m.plastic, Vec3::new(0.0, -0.06, 0.05), Vec3::new(0.03, 0.09, 0.04), Quat::from_rotation_x(0.25)),
            cylz(s, &m.gun_dark, Vec3::new(0.0, 0.065, -0.02), 0.012, 0.1),
            sph(s, &m.led_green, Vec3::new(0.0, 0.065, -0.072), 0.003),
        ],
        WeaponKind::Baton => vec![
            cylz(s, &m.gun_dark, Vec3::new(0.0, 0.0, -0.17), 0.017, 0.46),
            cylz(s, &m.plastic, Vec3::new(0.0, 0.0, 0.1), 0.022, 0.15),
            cylz(s, &m.cyan_glow, Vec3::new(0.0, 0.0, -0.4), 0.02, 0.025),
            cylz(s, &m.metal, Vec3::new(0.0, 0.0, -0.425), 0.012, 0.03),
        ],
    }
}

/// World pickup model (sits on y=0).
pub fn item_parts(k: ItemKind, m: &Mats, s: &Shapes) -> Vec<PartDef> {
    match k {
        ItemKind::Weapon(w) => {
            let rot = Quat::from_rotation_z(FRAC_PI_2) * Quat::from_rotation_y(0.6);
            weapon_parts(w, m, s)
                .into_iter()
                .map(|mut p| {
                    p.tf = Transform::from_translation(Vec3::new(0.0, 0.03, 0.0) + rot * p.tf.translation)
                        .with_rotation(rot * p.tf.rotation)
                        .with_scale(p.tf.scale);
                    p
                })
                .collect()
        }
        ItemKind::Medkit => vec![
            bx(s, &m.white, Vec3::new(0.0, 0.05, 0.0), Vec3::new(0.28, 0.1, 0.2)),
            bx(s, &m.red, Vec3::new(0.0, 0.101, 0.0), Vec3::new(0.12, 0.004, 0.035)),
            bx(s, &m.red, Vec3::new(0.0, 0.101, 0.0), Vec3::new(0.035, 0.004, 0.12)),
        ],
        ItemKind::Biocell => vec![
            cyly(s, &m.metal, Vec3::new(0.0, 0.07, 0.0), 0.035, 0.14),
            cyly(s, &m.cyan_glow, Vec3::new(0.0, 0.07, 0.0), 0.037, 0.03),
        ],
        ItemKind::Lockpick => vec![
            bx(s, &m.black, Vec3::new(0.0, 0.0125, 0.0), Vec3::new(0.14, 0.025, 0.05)),
            bx(s, &m.metal, Vec3::new(0.09, 0.012, 0.0), Vec3::new(0.08, 0.004, 0.008)),
        ],
        ItemKind::Multitool => vec![
            bx(s, &m.yellow, Vec3::new(0.0, 0.02, 0.0), Vec3::new(0.15, 0.04, 0.06)),
            bx(s, &m.black, Vec3::new(0.08, 0.02, 0.0), Vec3::new(0.03, 0.042, 0.062)),
            sph(s, &m.led_green, Vec3::new(-0.02, 0.041, 0.0), 0.006),
        ],
        ItemKind::Ammo(AmmoKind::Pistol) => vec![
            bx(s, &m.gun_dark, Vec3::new(0.0, 0.03, 0.0), Vec3::new(0.12, 0.06, 0.08)),
            bx(s, &m.white, Vec3::new(0.0, 0.03, 0.041), Vec3::new(0.08, 0.02, 0.002)),
        ],
        ItemKind::Ammo(AmmoKind::Rifle) => vec![
            bx(s, &m.gun_dark, Vec3::new(0.0, 0.04, 0.0), Vec3::new(0.22, 0.08, 0.1)),
            bx(s, &m.yellow, Vec3::new(0.0, 0.04, 0.051), Vec3::new(0.15, 0.02, 0.002)),
        ],
        ItemKind::Ammo(AmmoKind::Dart) => vec![bx(s, &m.yellow, Vec3::new(0.0, 0.015, 0.0), Vec3::new(0.18, 0.03, 0.05))],
        ItemKind::Key(_) => vec![
            bx(s, &m.white, Vec3::new(0.0, 0.003, 0.0), Vec3::new(0.085, 0.004, 0.055)),
            bx(s, &m.led_amber, Vec3::new(0.0, 0.0055, 0.015), Vec3::new(0.085, 0.002, 0.01)),
        ],
        ItemKind::Datacube(_) => vec![
            bx(s, &m.black, Vec3::new(0.0, 0.018, 0.0), Vec3::new(0.11, 0.035, 0.08)),
            bx(s, &m.screen, Vec3::new(0.0, 0.0365, 0.0), Vec3::new(0.08, 0.002, 0.055)),
        ],
        ItemKind::DataCore => vec![
            cyly(s, &m.core_glow, Vec3::new(0.0, 0.18, 0.0), 0.06, 0.3),
            cyly(s, &m.gunmetal, Vec3::new(0.0, 0.03, 0.0), 0.085, 0.05),
            cyly(s, &m.gunmetal, Vec3::new(0.0, 0.34, 0.0), 0.085, 0.04),
            bx(s, &m.metal, Vec3::new(0.07, 0.18, 0.0), Vec3::new(0.012, 0.3, 0.012)),
            bx(s, &m.metal, Vec3::new(-0.07, 0.18, 0.0), Vec3::new(0.012, 0.3, 0.012)),
        ],
        ItemKind::AugCanister => vec![
            cyly(s, &m.metal, Vec3::new(0.0, 0.1, 0.0), 0.05, 0.2),
            cyly(s, &m.orange_glow, Vec3::new(0.0, 0.12, 0.0), 0.052, 0.04),
            cyly(s, &m.gun_dark, Vec3::new(0.0, 0.205, 0.0), 0.035, 0.02),
        ],
    }
}
