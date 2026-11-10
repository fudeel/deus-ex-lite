//! Level loading (ASCII grid + metadata) and static geometry generation.
//!
//! Map legend - one character is one 2m x 2m cell:
//!
//! Tiles
//!   W boundary wall   # building wall   G glass window    ~ water
//!   . lab tile floor  _ carpet floor    = metal floor     , outdoor asphalt   : concrete pier
//!   V vent duct (crouch)   v vent grate (openable)
//!   D door   L locked door   K keypad door   S security door (terminal controlled)
//!   C crate (1m, climbable)   B container / tall stack   h counter / lab bench   R server rack
//!
//! Things (placed on the floor of that cell)
//!   P player spawn   X extraction zone   O objective (data core)   N friendly NPC
//!   g patrolling guard (pistol)   q sentry guard (rifle)   c security camera   t ceiling turret
//!   l ceiling light   f flickering light   a alarm beacon   o street lamp   s light switch
//!   T security terminal   p computer
//!   + medkit   % biocell   * pistol ammo   ! multitool   ? lockpick   0-9 custom items ([meta] "item")
use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use std::collections::{HashMap, HashSet};

use crate::game::*;
use crate::util::*;

pub const CELL: f32 = 2.0;
pub const WALL_H: f32 = 4.0;
pub const ROOF_H: f32 = 4.4;
pub const EXT_H: f32 = 9.0;
pub const VENT_H: f32 = 1.15;
pub const DOOR_H: f32 = 3.0;
pub const GROUND: f32 = -2.0;
pub const WATER_BED: f32 = -1.6;
pub const WATER_Y: f32 = -0.55;
pub const SILL_H: f32 = 0.9;
pub const TRIM_H: f32 = 0.16;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Floor {
    Tile,
    Carpet,
    Metal,
    Asphalt,
    Concrete,
}

impl Floor {
    pub fn outdoor(self) -> bool {
        matches!(self, Floor::Asphalt | Floor::Concrete)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DoorKind {
    Normal,
    Locked,
    Keypad,
    Security,
    Grate,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tile {
    Void,
    Wall,
    ExtWall,
    Glass,
    Open(Floor),
    Door(DoorKind, Floor),
    Vent,
    Grate,
    Crate(Floor),
    Stack(Floor),
    Counter(Floor),
    Rack(Floor),
    Water,
}

impl Tile {
    pub fn floor(&self) -> Option<Floor> {
        match *self {
            Tile::Open(f) | Tile::Door(_, f) | Tile::Crate(f) | Tile::Stack(f) | Tile::Counter(f) | Tile::Rack(f) => Some(f),
            _ => None,
        }
    }
    pub fn is_wallish(&self) -> bool {
        matches!(self, Tile::Void | Tile::Wall | Tile::ExtWall | Tile::Glass)
    }
    pub fn indoor(&self) -> bool {
        match self.floor() {
            Some(f) => !f.outdoor(),
            None => matches!(self, Tile::Vent | Tile::Grate),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Part {
    Ground(Floor),
    Ceiling(Floor),
    Wall,
    ExtWall,
    Lintel,
    VentFloor,
    VentTop,
    Crate,
    Stack(u8),
    Counter,
    Rack,
    WaterBed,
    Invisible,
}

#[derive(Clone, Copy, Debug)]
pub struct Interval {
    pub y0: f32,
    pub y1: f32,
    pub part: Part,
    pub render: bool,
    pub collide: bool,
}

/// Surface materials used by the level geometry.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Surf {
    Concrete,
    Asphalt,
    FloorTile,
    Carpet,
    MetalFloor,
    WallLab,
    WallOffice,
    WallMetal,
    WallExt,
    WallBoundary,
    Ceiling,
    CeilingMetal,
    Roof,
    Vent,
    Crate,
    Container(u8),
    Counter,
    Rack,
    Trim,
    WaterBed,
}

impl Surf {
    /// world meters covered by one texture repeat (u, v)
    pub fn uv_scale(self, top: bool) -> (f32, f32) {
        match self {
            Surf::Asphalt | Surf::WallBoundary | Surf::WaterBed | Surf::Roof => (4.0, 4.0),
            Surf::MetalFloor | Surf::Vent => (1.0, 1.0),
            Surf::Crate if !top => (2.0, 1.0),
            Surf::Counter if !top => (2.0, 1.0),
            Surf::Rack if !top => (2.0, 2.4),
            Surf::Container(_) if !top => (2.0, 2.6),
            Surf::Trim => (1.0, TRIM_H),
            _ => (2.0, 2.0),
        }
    }
}

#[derive(Clone, Debug)]
pub struct DoorDef {
    pub id: u16,
    pub x: i32,
    pub z: i32,
    pub kind: DoorKind,
    /// the door slab spans the X axis (passage runs along Z)
    pub along_x: bool,
    pub strength: u8,
    pub key: Option<u8>,
    pub code: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LightKind {
    Ceiling,
    Flicker,
    Street,
    Alarm,
}

#[derive(Clone, Debug)]
pub struct LightDef {
    pub id: u16,
    pub x: i32,
    pub z: i32,
    pub kind: LightKind,
    pub room: u16,
}

#[derive(Clone, Debug)]
pub struct GuardDef {
    pub id: u16,
    pub x: i32,
    pub z: i32,
    pub rifle: bool,
    pub route: Vec<(i32, i32)>,
    pub flashlight: bool,
    pub yaw: f32,
}

#[derive(Clone, Debug)]
pub struct DeviceDef {
    pub id: u16,
    pub x: i32,
    pub z: i32,
    pub yaw: f32,
    pub always: bool,
}

#[derive(Clone, Debug)]
pub struct PickupDef {
    pub id: u16,
    pub x: i32,
    pub z: i32,
    pub item: ItemKind,
    pub count: u16,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TermKind {
    Security,
    Pc,
}

#[derive(Clone, Debug)]
pub struct TermDef {
    pub id: u16,
    pub x: i32,
    pub z: i32,
    pub kind: TermKind,
    pub login: Option<(String, String)>,
    pub hack: u8,
    pub mails: Vec<u16>,
    pub yaw: f32,
}

#[derive(Clone, Debug)]
pub struct SwitchDef {
    pub id: u16,
    pub x: i32,
    pub z: i32,
    pub room: u16,
    pub yaw: f32,
}

#[derive(Clone, Debug)]
pub struct NpcDef {
    pub id: u16,
    pub x: i32,
    pub z: i32,
    pub kind: String,
    pub yaw: f32,
}

#[derive(Clone, Debug)]
pub struct NoteDef {
    pub id: u16,
    pub title: String,
    pub text: String,
    pub code: Option<String>,
    pub login: Option<(String, String)>,
}

#[derive(Resource)]
pub struct Level {
    /// file name without extension (e.g. "mission01")
    pub name: String,
    pub w: i32,
    pub h: i32,
    pub tiles: Vec<Tile>,
    pub cols: Vec<Vec<Interval>>,
    pub room: Vec<u16>,
    /// cells NPCs must not path through (lamp posts, desks...)
    pub blocked: Vec<bool>,
    pub spawns: Vec<Vec3>,
    pub extraction: Vec<(i32, i32)>,
    pub doors: Vec<DoorDef>,
    pub lights: Vec<LightDef>,
    pub guards: Vec<GuardDef>,
    pub cameras: Vec<DeviceDef>,
    pub turrets: Vec<DeviceDef>,
    pub pickups: Vec<PickupDef>,
    pub terminals: Vec<TermDef>,
    pub switches: Vec<SwitchDef>,
    pub npcs: Vec<NpcDef>,
    pub notes: Vec<NoteDef>,
    pub key_ids: Vec<String>,
    pub key_names: Vec<String>,
    pub glass: Vec<(i32, i32, bool)>,
    pub hash: u64,
    /// team match: respawn points per team (empty on single-player maps)
    pub team_spawns: [Vec<Vec3>; 2],
    /// team match: extraction cells per team (deliver the stolen prototype here)
    pub team_bases: [Vec<(i32, i32)>; 2],
    /// team match: cells where the prototype may appear
    pub steal_spots: Vec<(i32, i32)>,
    /// team match: cells where terminals may appear (x, z, yaw facing away from the wall)
    pub hack_spots: Vec<(i32, i32, f32)>,
}

impl Level {
    pub fn inb(&self, x: i32, z: i32) -> bool {
        x >= 0 && z >= 0 && x < self.w && z < self.h
    }
    pub fn idx(&self, x: i32, z: i32) -> usize {
        (z * self.w + x) as usize
    }
    pub fn tile(&self, x: i32, z: i32) -> Tile {
        if self.inb(x, z) {
            self.tiles[self.idx(x, z)]
        } else {
            Tile::Void
        }
    }
    pub fn center(x: i32, z: i32) -> Vec3 {
        Vec3::new((x as f32 + 0.5) * CELL, 0.0, (z as f32 + 0.5) * CELL)
    }
    pub fn cell_of(p: Vec3) -> (i32, i32) {
        ((p.x / CELL).floor() as i32, (p.z / CELL).floor() as i32)
    }
    pub fn tile_at(&self, p: Vec3) -> Tile {
        let (x, z) = Self::cell_of(p);
        self.tile(x, z)
    }
    pub fn indoor_at(&self, p: Vec3) -> bool {
        self.tile_at(p).indoor()
    }
    pub fn room_at(&self, p: Vec3) -> u16 {
        let (x, z) = Self::cell_of(p);
        if self.inb(x, z) {
            self.room[self.idx(x, z)]
        } else {
            0
        }
    }
    /// Can an NPC stand in / walk through this cell (doors handled by caller).
    pub fn nav_walkable(&self, x: i32, z: i32) -> bool {
        if !self.inb(x, z) || self.blocked[self.idx(x, z)] {
            return false;
        }
        matches!(self.tile(x, z), Tile::Open(_) | Tile::Door(DoorKind::Normal | DoorKind::Locked | DoorKind::Keypad | DoorKind::Security, _))
    }
    pub fn door_at(&self, x: i32, z: i32) -> Option<&DoorDef> {
        self.doors.iter().find(|d| d.x == x && d.z == z)
    }
    pub fn note(&self, id: u16) -> Option<&NoteDef> {
        self.notes.iter().find(|n| n.id == id)
    }
    pub fn key_name(&self, k: u8) -> String {
        self.key_names.get(k as usize).cloned().unwrap_or_else(|| "Key".into())
    }
    pub fn in_extraction(&self, p: Vec3) -> bool {
        let c = Self::cell_of(p);
        self.extraction.contains(&c)
    }
    /// Does this map support the team match?
    pub fn is_team_map(&self) -> bool {
        !self.team_spawns[0].is_empty() && !self.team_spawns[1].is_empty() && !self.steal_spots.is_empty() && self.hack_spots.len() >= 3
    }
    pub fn in_base(&self, team: u8, p: Vec3) -> bool {
        let c = Self::cell_of(p);
        self.team_bases.get(team as usize).map(|b| b.contains(&c)).unwrap_or(false)
    }
    pub fn base_center(&self, team: u8) -> Vec3 {
        let Some(cells) = self.team_bases.get(team as usize).filter(|b| !b.is_empty()) else {
            return self.team_spawns.get(team as usize).and_then(|s| s.first().copied()).unwrap_or(Vec3::ZERO);
        };
        let mut c = Vec3::ZERO;
        for &(x, z) in cells {
            c += Self::center(x, z);
        }
        c / cells.len() as f32
    }
    pub fn steal_pos(&self, spot: u8) -> Vec3 {
        self.steal_spots.get(spot as usize).map(|&(x, z)| Self::center(x, z)).unwrap_or(Vec3::ZERO)
    }
    pub fn hack_pos(&self, spot: u8) -> Vec3 {
        self.hack_spots.get(spot as usize).map(|&(x, z, _)| Self::center(x, z)).unwrap_or(Vec3::ZERO)
    }
    /// Walkable for team-match bots: like `nav_walkable`, plus crawl vents and their grates.
    pub fn nav_walkable_vents(&self, x: i32, z: i32) -> bool {
        self.nav_walkable(x, z) || (self.inb(x, z) && matches!(self.tile(x, z), Tile::Vent | Tile::Grate))
    }
}

fn parse_team(s: Option<&&str>) -> Option<usize> {
    match s.copied() {
        Some("orange") | Some("0") => Some(0),
        Some("blue") | Some("1") => Some(1),
        _ => None,
    }
}

pub const CAMPAIGN_LEVEL: &str = "mission01";
pub const TEAM_LEVEL: &str = "harbor_teams";

pub fn load_level(name: &str) -> Level {
    let path = format!("assets/levels/{name}.txt");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => {
            warn!("could not read {path}, using built-in copy");
            match name {
                TEAM_LEVEL => include_str!("../assets/levels/harbor_teams.txt").to_string(),
                _ => include_str!("../assets/levels/mission01.txt").to_string(),
            }
        }
    };
    let mut lvl = parse_level(&text);
    lvl.name = name.to_string();
    lvl
}

fn fnv(text: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        if b == b'\r' {
            continue;
        }
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

fn parse_xy(s: &str) -> Option<(i32, i32)> {
    let mut it = s.split(',');
    let x = it.next()?.trim().parse().ok()?;
    let z = it.next()?.trim().parse().ok()?;
    Some((x, z))
}

fn kv<'a>(parts: &'a [&'a str], key: &str) -> Option<&'a str> {
    parts.iter().find_map(|p| p.strip_prefix(key).and_then(|r| r.strip_prefix('=')))
}

struct TermMeta {
    kind: TermKind,
    login: Option<(String, String)>,
    hack: u8,
    mails: Vec<u16>,
}

pub fn parse_level(text: &str) -> Level {
    let mut grid: Vec<Vec<char>> = vec![];
    let mut meta: Vec<String> = vec![];
    let mut notes_txt: Vec<String> = vec![];
    let mut section = "";
    for raw in text.lines() {
        let line = raw.trim_end_matches('\r');
        let t = line.trim();
        if t.starts_with('[') && t.ends_with(']') {
            section = match t {
                "[grid]" => "grid",
                "[meta]" => "meta",
                "[notes]" => "notes",
                _ => "",
            };
            continue;
        }
        match section {
            "grid" => {
                if !t.is_empty() {
                    grid.push(line.chars().collect());
                }
            }
            "meta" => {
                if !t.is_empty() && !t.starts_with('#') {
                    meta.push(t.to_string());
                }
            }
            "notes" => {
                if !t.is_empty() && !t.starts_with('#') {
                    notes_txt.push(t.to_string());
                }
            }
            _ => {}
        }
    }
    let h = grid.len() as i32;
    let w = grid.iter().map(|r| r.len()).max().unwrap_or(0) as i32;
    let ch = |x: i32, z: i32| -> char {
        if x < 0 || z < 0 || z >= h {
            return ' ';
        }
        *grid[z as usize].get(x as usize).unwrap_or(&' ')
    };

    // ---- metadata
    let mut locks: HashMap<(i32, i32), (u8, Option<String>)> = HashMap::new();
    let mut keypads: HashMap<(i32, i32), (String, u8)> = HashMap::new();
    let mut items: HashMap<char, (ItemKind, u16)> = HashMap::new();
    let mut terms: HashMap<(i32, i32), TermMeta> = HashMap::new();
    let mut routes: HashMap<(i32, i32), Vec<(i32, i32)>> = HashMap::new();
    let mut flashlights: HashSet<(i32, i32)> = HashSet::new();
    let mut always_turrets: HashSet<(i32, i32)> = HashSet::new();
    let mut npc_kinds: HashMap<(i32, i32), String> = HashMap::new();
    let mut key_ids: Vec<String> = vec![];
    let mut key_names: Vec<String> = vec![];
    let mut team_spawn_cells: [Vec<(i32, i32)>; 2] = [vec![], vec![]];
    let mut team_base_cells: [Vec<(i32, i32)>; 2] = [vec![], vec![]];
    let mut steal_cells: Vec<(i32, i32)> = vec![];
    let mut hack_cells: Vec<(i32, i32)> = vec![];

    for line in &meta {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }
        let at = parts.get(1).and_then(|s| parse_xy(s));
        match parts[0] {
            "lock" => {
                if let Some(c) = at {
                    let s = kv(&parts, "strength").and_then(|v| v.parse().ok()).unwrap_or(2);
                    locks.insert(c, (s, kv(&parts, "key").map(|s| s.to_string())));
                }
            }
            "keypad" => {
                if let Some(c) = at {
                    let code = kv(&parts, "code").unwrap_or("0000").to_string();
                    let s = kv(&parts, "strength").and_then(|v| v.parse().ok()).unwrap_or(2);
                    keypads.insert(c, (code, s));
                }
            }
            "item" => {
                let Some(marker) = parts.get(1).and_then(|s| s.chars().next()) else { continue };
                let kind = parts.get(2).copied().unwrap_or("");
                let arg = parts.get(3).copied();
                let num = |d: u16| arg.and_then(|a| a.parse::<u16>().ok()).unwrap_or(d);
                let item = match kind {
                    "medkit" => Some((ItemKind::Medkit, 1)),
                    "biocell" => Some((ItemKind::Biocell, 1)),
                    "lockpick" => Some((ItemKind::Lockpick, num(1))),
                    "multitool" => Some((ItemKind::Multitool, num(1))),
                    "pistol" => Some((ItemKind::Weapon(WeaponKind::Pistol), 1)),
                    "rifle" => Some((ItemKind::Weapon(WeaponKind::Rifle), 1)),
                    "crossbow" => Some((ItemKind::Weapon(WeaponKind::Crossbow), 1)),
                    "baton" => Some((ItemKind::Weapon(WeaponKind::Baton), 1)),
                    "ammo_pistol" => Some((ItemKind::Ammo(AmmoKind::Pistol), num(12))),
                    "ammo_rifle" => Some((ItemKind::Ammo(AmmoKind::Rifle), num(30))),
                    "darts" => Some((ItemKind::Ammo(AmmoKind::Dart), num(4))),
                    "augcanister" => Some((ItemKind::AugCanister, 1)),
                    "datacore" => Some((ItemKind::DataCore, 1)),
                    "datacube" => Some((ItemKind::Datacube(num(0)), 1)),
                    "key" => {
                        let id = arg.unwrap_or("key").to_string();
                        let name = parts.get(4).map(|s| s.replace('_', " ")).unwrap_or_else(|| "Key".into());
                        key_ids.push(id);
                        key_names.push(name);
                        Some((ItemKind::Key((key_ids.len() - 1) as u8), 1))
                    }
                    _ => None,
                };
                if let Some(it) = item {
                    items.insert(marker, it);
                }
            }
            "terminal" => {
                if let Some(c) = at {
                    let kind = if parts.get(2) == Some(&"security") { TermKind::Security } else { TermKind::Pc };
                    let login = match (kv(&parts, "login"), kv(&parts, "pass")) {
                        (Some(l), Some(p)) => Some((l.to_string(), p.to_string())),
                        _ => None,
                    };
                    let hack = kv(&parts, "hack").and_then(|v| v.parse().ok()).unwrap_or(1);
                    let mails = kv(&parts, "mail")
                        .map(|m| m.split(',').filter_map(|s| s.parse().ok()).collect())
                        .unwrap_or_default();
                    terms.insert(c, TermMeta { kind, login, hack, mails });
                }
            }
            "route" => {
                if let Some(c) = at {
                    let pts: Vec<(i32, i32)> = parts[1..].iter().filter_map(|s| parse_xy(s)).collect();
                    routes.insert(c, pts);
                }
            }
            "flashlight" => {
                if let Some(c) = at {
                    flashlights.insert(c);
                }
            }
            "turret" => {
                if let Some(c) = at {
                    if parts.contains(&"always") {
                        always_turrets.insert(c);
                    }
                }
            }
            "npc" => {
                if let Some(c) = at {
                    npc_kinds.insert(c, parts.get(2).unwrap_or(&"civilian").to_string());
                }
            }
            "team_spawn" | "team_base" => {
                let Some(team) = parse_team(parts.get(1)) else {
                    warn!("level: '{line}' needs a team (orange / blue)");
                    continue;
                };
                let cells = parts.iter().skip(2).filter_map(|s| parse_xy(s));
                if parts[0] == "team_spawn" {
                    team_spawn_cells[team].extend(cells);
                } else {
                    team_base_cells[team].extend(cells);
                }
            }
            "steal_spot" => steal_cells.extend(parts.iter().skip(1).filter_map(|s| parse_xy(s))),
            "hack_spot" => hack_cells.extend(parts.iter().skip(1).filter_map(|s| parse_xy(s))),
            other => warn!("unknown level meta command '{other}'"),
        }
    }

    // ---- notes
    let mut notes = vec![];
    for l in &notes_txt {
        let f: Vec<&str> = l.split('|').collect();
        if f.len() < 3 {
            continue;
        }
        let Ok(id) = f[0].trim().parse::<u16>() else { continue };
        let mut code = None;
        let mut login = None;
        if let Some(extra) = f.get(3) {
            for kvp in extra.split_whitespace() {
                if let Some(c) = kvp.strip_prefix("code=") {
                    code = Some(c.to_string());
                }
                if let Some(c) = kvp.strip_prefix("login=") {
                    let mut s = c.split(':');
                    if let (Some(a), Some(b)) = (s.next(), s.next()) {
                        login = Some((a.to_string(), b.to_string()));
                    }
                }
            }
        }
        notes.push(NoteDef { id, title: f[1].to_string(), text: f[2].replace("\\n", "\n"), code, login });
    }

    // ---- base floor of each char
    let base_floor = |c: char| -> Option<Floor> {
        match c {
            '.' => Some(Floor::Tile),
            '_' => Some(Floor::Carpet),
            '=' => Some(Floor::Metal),
            ',' => Some(Floor::Asphalt),
            ':' => Some(Floor::Concrete),
            _ => None,
        }
    };
    let infer_floor = |x: i32, z: i32| -> Floor {
        for r in 1i32..4 {
            let mut counts: HashMap<Floor, i32> = HashMap::new();
            for dz in -r..=r {
                for dx in -r..=r {
                    if dx.abs().max(dz.abs()) != r {
                        continue;
                    }
                    if r == 1 && dx != 0 && dz != 0 {
                        continue;
                    }
                    if let Some(f) = base_floor(ch(x + dx, z + dz)) {
                        *counts.entry(f).or_default() += if f.outdoor() { 1 } else { 2 };
                    }
                }
            }
            if let Some((f, _)) = counts.into_iter().max_by_key(|(f, c)| (*c, *f as i32)) {
                return f;
            }
        }
        Floor::Tile
    };

    let mut tiles = vec![Tile::Void; (w * h) as usize];
    for z in 0..h {
        for x in 0..w {
            let c = ch(x, z);
            let t = match c {
                ' ' => Tile::Void,
                'W' => Tile::ExtWall,
                '#' => Tile::Wall,
                'G' => Tile::Glass,
                '~' => Tile::Water,
                'V' => Tile::Vent,
                'v' => Tile::Grate,
                _ => {
                    if let Some(f) = base_floor(c) {
                        Tile::Open(f)
                    } else {
                        let f = infer_floor(x, z);
                        match c {
                            'D' => Tile::Door(DoorKind::Normal, f),
                            'L' => Tile::Door(DoorKind::Locked, f),
                            'K' => Tile::Door(DoorKind::Keypad, f),
                            'S' => Tile::Door(DoorKind::Security, f),
                            'C' => Tile::Crate(f),
                            'B' => Tile::Stack(f),
                            'h' => Tile::Counter(f),
                            'R' => Tile::Rack(f),
                            _ => Tile::Open(f),
                        }
                    }
                }
            };
            tiles[(z * w + x) as usize] = t;
        }
    }
    let tile = |x: i32, z: i32| -> Tile {
        if x < 0 || z < 0 || x >= w || z >= h {
            Tile::Void
        } else {
            tiles[(z * w + x) as usize]
        }
    };
    let solid_for_mount = |t: Tile| t.is_wallish() || matches!(t, Tile::Door(..) | Tile::Vent | Tile::Grate);
    // yaw facing away from an adjacent wall + whether one was found
    let wall_facing = |x: i32, z: i32| -> Option<f32> {
        // among adjacent walls, mount on the one that faces the most open space
        let dirs = [(0, -1), (-1, 0), (1, 0), (0, 1)];
        let mut best: Option<(i32, f32)> = None;
        for (dx, dz) in dirs {
            if !solid_for_mount(tile(x + dx, z + dz)) {
                continue;
            }
            let mut run = 0;
            for k in 1..12 {
                if tile(x - dx * k, z - dz * k).is_wallish() {
                    break;
                }
                run += 1;
            }
            if best.map(|b| run > b.0).unwrap_or(true) {
                best = Some((run, dir_yaw(Vec3::new(-dx as f32, 0.0, -dz as f32))));
            }
        }
        best.map(|b| b.1)
    };
    let open_facing = |x: i32, z: i32| -> f32 {
        // face toward the most open direction
        let dirs = [(0, 1), (0, -1), (1, 0), (-1, 0)];
        let mut best = (0, 0.0);
        for (dx, dz) in dirs {
            let mut n = 0;
            for k in 1..6 {
                match tile(x + dx * k, z + dz * k) {
                    Tile::Open(_) | Tile::Door(..) => n += 1,
                    _ => break,
                }
            }
            if n > best.0 {
                best = (n, dir_yaw(Vec3::new(dx as f32, 0.0, dz as f32)));
            }
        }
        best.1
    };

    // ---- rooms (flood fill, doors split rooms)
    let mut room = vec![0u16; (w * h) as usize];
    let mut next_room = 1u16;
    for z in 0..h {
        for x in 0..w {
            let i = (z * w + x) as usize;
            let t = tiles[i];
            let roomish = |t: Tile| matches!(t, Tile::Open(_) | Tile::Crate(_) | Tile::Stack(_) | Tile::Counter(_) | Tile::Rack(_));
            if room[i] != 0 || !roomish(t) {
                continue;
            }
            let mut stack = vec![(x, z)];
            room[i] = next_room;
            while let Some((cx, cz)) = stack.pop() {
                for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let (nx, nz) = (cx + dx, cz + dz);
                    if nx < 0 || nz < 0 || nx >= w || nz >= h {
                        continue;
                    }
                    let ni = (nz * w + nx) as usize;
                    if room[ni] == 0 && roomish(tiles[ni]) {
                        room[ni] = next_room;
                        stack.push((nx, nz));
                    }
                }
            }
            next_room += 1;
        }
    }

    // ---- stack colour groups
    let mut stack_var = vec![0u8; (w * h) as usize];
    {
        let mut seen = vec![false; (w * h) as usize];
        let mut group = 0u8;
        for z in 0..h {
            for x in 0..w {
                let i = (z * w + x) as usize;
                if seen[i] || !matches!(tiles[i], Tile::Stack(_)) {
                    continue;
                }
                let mut st = vec![(x, z)];
                seen[i] = true;
                let indoor = tiles[i].indoor();
                let v = if indoor { 4 } else { group % 4 };
                while let Some((cx, cz)) = st.pop() {
                    stack_var[(cz * w + cx) as usize] = v;
                    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                        let (nx, nz) = (cx + dx, cz + dz);
                        if nx < 0 || nz < 0 || nx >= w || nz >= h {
                            continue;
                        }
                        let ni = (nz * w + nx) as usize;
                        if !seen[ni] && matches!(tiles[ni], Tile::Stack(_)) {
                            seen[ni] = true;
                            st.push((nx, nz));
                        }
                    }
                }
                group = group.wrapping_add(1);
            }
        }
    }

    // ---- entities
    let mut lvl = Level {
        name: String::new(),
        w,
        h,
        tiles: tiles.clone(),
        cols: vec![],
        room,
        blocked: vec![false; (w * h) as usize],
        spawns: vec![],
        extraction: vec![],
        doors: vec![],
        lights: vec![],
        guards: vec![],
        cameras: vec![],
        turrets: vec![],
        pickups: vec![],
        terminals: vec![],
        switches: vec![],
        npcs: vec![],
        notes,
        key_ids,
        key_names,
        glass: vec![],
        hash: fnv(text),
        team_spawns: [
            team_spawn_cells[0].iter().map(|&(x, z)| Level::center(x, z)).collect(),
            team_spawn_cells[1].iter().map(|&(x, z)| Level::center(x, z)).collect(),
        ],
        team_bases: team_base_cells.clone(),
        steal_spots: steal_cells.clone(),
        hack_spots: vec![],
    };
    let key_index = |k: &str, ids: &Vec<String>| ids.iter().position(|s| s == k).map(|p| p as u8);

    for z in 0..h {
        for x in 0..w {
            let c = ch(x, z);
            let i = (z * w + x) as usize;
            let room_id = lvl.room[i];
            match tiles[i] {
                Tile::Door(kind, _) => {
                    let along_x = tile(x - 1, z).is_wallish() || tile(x + 1, z).is_wallish();
                    let mut d = DoorDef {
                        id: lvl.doors.len() as u16,
                        x,
                        z,
                        kind,
                        along_x,
                        strength: 0,
                        key: None,
                        code: None,
                    };
                    match kind {
                        DoorKind::Locked => {
                            let (s, k) = locks.get(&(x, z)).cloned().unwrap_or((2, None));
                            d.strength = s;
                            d.key = k.and_then(|k| key_index(&k, &lvl.key_ids));
                        }
                        DoorKind::Keypad => {
                            let (code, s) = keypads.get(&(x, z)).cloned().unwrap_or(("0000".into(), 2));
                            d.code = Some(code);
                            d.strength = s;
                        }
                        DoorKind::Security => d.strength = 99,
                        _ => {}
                    }
                    lvl.doors.push(d);
                    continue;
                }
                Tile::Grate => {
                    // grate spans X when the duct runs along Z
                    let open = |t: Tile| !t.is_wallish();
                    let along_x = open(tile(x, z - 1)) || open(tile(x, z + 1));
                    lvl.doors.push(DoorDef {
                        id: lvl.doors.len() as u16,
                        x,
                        z,
                        kind: DoorKind::Grate,
                        along_x,
                        strength: 0,
                        key: None,
                        code: None,
                    });
                    continue;
                }
                Tile::Glass => {
                    let along_x = tile(x - 1, z).is_wallish() || tile(x + 1, z).is_wallish();
                    lvl.glass.push((x, z, along_x));
                    continue;
                }
                _ => {}
            }
            let pid = lvl.pickups.len() as u16;
            let pickup = |item: ItemKind, count: u16, lvl: &mut Level| {
                lvl.pickups.push(PickupDef { id: pid, x, z, item, count });
            };
            match c {
                'P' => lvl.spawns.push(Level::center(x, z)),
                'X' => lvl.extraction.push((x, z)),
                'O' => pickup(ItemKind::DataCore, 1, &mut lvl),
                'g' | 'q' => {
                    let mut route = routes.get(&(x, z)).cloned().unwrap_or_default();
                    if route.first() != Some(&(x, z)) {
                        route.insert(0, (x, z));
                    }
                    lvl.guards.push(GuardDef {
                        id: lvl.guards.len() as u16,
                        x,
                        z,
                        rifle: c == 'q',
                        route,
                        flashlight: flashlights.contains(&(x, z)),
                        yaw: open_facing(x, z),
                    });
                }
                'c' => {
                    let yaw = wall_facing(x, z).unwrap_or(0.0);
                    lvl.cameras.push(DeviceDef { id: lvl.cameras.len() as u16, x, z, yaw, always: false });
                }
                't' => lvl.turrets.push(DeviceDef {
                    id: lvl.turrets.len() as u16,
                    x,
                    z,
                    yaw: open_facing(x, z),
                    always: always_turrets.contains(&(x, z)),
                }),
                'l' | 'f' | 'a' | 'o' => {
                    let kind = match c {
                        'l' => LightKind::Ceiling,
                        'f' => LightKind::Flicker,
                        'a' => LightKind::Alarm,
                        _ => LightKind::Street,
                    };
                    if kind == LightKind::Street {
                        lvl.blocked[i] = true;
                    }
                    lvl.lights.push(LightDef { id: lvl.lights.len() as u16, x, z, kind, room: room_id });
                }
                's' => {
                    let yaw = wall_facing(x, z).unwrap_or(0.0);
                    lvl.switches.push(SwitchDef { id: lvl.switches.len() as u16, x, z, room: room_id, yaw });
                }
                'T' | 'p' => {
                    let m = terms.remove(&(x, z)).unwrap_or(TermMeta {
                        kind: if c == 'T' { TermKind::Security } else { TermKind::Pc },
                        login: None,
                        hack: 1,
                        mails: vec![],
                    });
                    lvl.blocked[i] = true;
                    lvl.terminals.push(TermDef {
                        id: lvl.terminals.len() as u16,
                        x,
                        z,
                        kind: m.kind,
                        login: m.login,
                        hack: m.hack,
                        mails: m.mails,
                        yaw: wall_facing(x, z).unwrap_or(0.0),
                    });
                }
                'N' => {
                    lvl.npcs.push(NpcDef {
                        id: lvl.npcs.len() as u16,
                        x,
                        z,
                        kind: npc_kinds.get(&(x, z)).cloned().unwrap_or_else(|| "civilian".into()),
                        yaw: open_facing(x, z),
                    });
                    lvl.blocked[i] = true;
                }
                '+' => pickup(ItemKind::Medkit, 1, &mut lvl),
                '%' => pickup(ItemKind::Biocell, 1, &mut lvl),
                '*' => pickup(ItemKind::Ammo(AmmoKind::Pistol), 12, &mut lvl),
                '!' => pickup(ItemKind::Multitool, 1, &mut lvl),
                '?' => pickup(ItemKind::Lockpick, 2, &mut lvl),
                _ => {
                    if let Some((item, count)) = items.get(&c) {
                        pickup(*item, *count, &mut lvl);
                    }
                }
            }
        }
    }
    if lvl.spawns.is_empty() {
        lvl.spawns.push(lvl.team_spawns[0].first().copied().unwrap_or(Level::center(1, 1)));
    }
    // team-match terminals stand against a wall like the mission terminals
    for &(x, z) in &hack_cells {
        let yaw = wall_facing(x, z).unwrap_or(0.0);
        lvl.hack_spots.push((x, z, yaw));
        if lvl.inb(x, z) {
            let i = lvl.idx(x, z);
            lvl.blocked[i] = true;
        }
    }

    // ---- columns
    lvl.cols = (0..h)
        .flat_map(|z| (0..w).map(move |x| (x, z)))
        .map(|(x, z)| column(tile(x, z), stack_var[(z * w + x) as usize]))
        .collect();
    info!(
        "level parsed: {}x{} cells, {} doors, {} lights, {} guards, {} pickups, team map: {}",
        w,
        h,
        lvl.doors.len(),
        lvl.lights.len(),
        lvl.guards.len(),
        lvl.pickups.len(),
        lvl.is_team_map()
    );
    lvl
}

fn column(t: Tile, var: u8) -> Vec<Interval> {
    let iv = |y0: f32, y1: f32, part: Part| Interval { y0, y1, part, render: true, collide: true };
    let ground = |f: Floor| iv(GROUND, 0.0, Part::Ground(f));
    let mut v = vec![];
    match t {
        Tile::Void | Tile::ExtWall => v.push(iv(GROUND, EXT_H, Part::ExtWall)),
        Tile::Wall => v.push(iv(GROUND, ROOF_H, Part::Wall)),
        Tile::Glass => {
            v.push(iv(GROUND, SILL_H, Part::Wall));
            v.push(Interval { y0: SILL_H, y1: DOOR_H, part: Part::Invisible, render: false, collide: true });
            v.push(iv(DOOR_H, ROOF_H, Part::Lintel));
        }
        Tile::Open(f) => {
            v.push(ground(f));
            if !f.outdoor() {
                v.push(iv(WALL_H, ROOF_H, Part::Ceiling(f)));
            }
        }
        Tile::Door(_, f) => {
            v.push(ground(f));
            v.push(iv(DOOR_H, ROOF_H, Part::Lintel));
        }
        Tile::Vent | Tile::Grate => {
            v.push(iv(GROUND, 0.0, Part::VentFloor));
            v.push(iv(VENT_H, ROOF_H, Part::VentTop));
        }
        Tile::Crate(f) | Tile::Stack(f) | Tile::Counter(f) | Tile::Rack(f) => {
            v.push(ground(f));
            let (top, part) = match t {
                Tile::Crate(_) => (1.0, Part::Crate),
                Tile::Stack(_) => (2.6, Part::Stack(var)),
                Tile::Counter(_) => (1.0, Part::Counter),
                _ => (2.4, Part::Rack),
            };
            v.push(iv(0.0, top, part));
            if !f.outdoor() {
                v.push(iv(WALL_H, ROOF_H, Part::Ceiling(f)));
            }
        }
        Tile::Water => {
            v.push(iv(GROUND, WATER_BED, Part::WaterBed));
            v.push(Interval { y0: WATER_BED, y1: 2.5, part: Part::Invisible, render: false, collide: true });
        }
    }
    v
}

// ------------------------------------------------------------------ geometry

#[derive(Default)]
pub struct MeshBuf {
    pub pos: Vec<[f32; 3]>,
    pub nor: Vec<[f32; 3]>,
    pub uv: Vec<[f32; 2]>,
    pub tan: Vec<[f32; 4]>,
    pub idx: Vec<u32>,
}

impl MeshBuf {
    /// corners counter-clockwise seen from the front. UVs are derived from world position.
    pub fn quad(&mut self, c: [Vec3; 4], n: Vec3, t: Vec3, bv: Vec3, su: f32, sv: f32) {
        let base = self.pos.len() as u32;
        let w = if n.cross(t).dot(bv) > 0.0 { 1.0 } else { -1.0 };
        for p in c {
            self.pos.push(p.to_array());
            self.nor.push(n.to_array());
            self.uv.push([p.dot(t) / su, p.dot(bv) / sv]);
            self.tan.push([t.x, t.y, t.z, w]);
        }
        self.idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    /// quad with explicit uvs
    pub fn quad_uv(&mut self, c: [Vec3; 4], n: Vec3, t: Vec3, uv: [[f32; 2]; 4]) {
        let base = self.pos.len() as u32;
        for (i, p) in c.iter().enumerate() {
            self.pos.push(p.to_array());
            self.nor.push(n.to_array());
            self.uv.push(uv[i]);
            self.tan.push([t.x, t.y, t.z, -1.0]);
        }
        self.idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    pub fn is_empty(&self) -> bool {
        self.idx.is_empty()
    }
    pub fn into_mesh(self) -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.nor)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uv)
            .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, self.tan)
            .with_inserted_indices(Indices::U32(self.idx))
    }
}

fn floor_surf(f: Floor) -> Surf {
    match f {
        Floor::Tile => Surf::FloorTile,
        Floor::Carpet => Surf::Carpet,
        Floor::Metal => Surf::MetalFloor,
        Floor::Asphalt => Surf::Asphalt,
        Floor::Concrete => Surf::Concrete,
    }
}

fn wall_surf(f: Floor) -> Surf {
    match f {
        Floor::Tile => Surf::WallLab,
        Floor::Carpet => Surf::WallOffice,
        Floor::Metal => Surf::WallMetal,
        Floor::Asphalt | Floor::Concrete => Surf::WallExt,
    }
}

fn side_surf(part: Part, own: Tile, nb: Tile, y_mid: f32) -> Surf {
    match part {
        Part::Wall | Part::Lintel | Part::VentTop | Part::Ceiling(_) => {
            if matches!(nb, Tile::Vent | Tile::Grate) && y_mid < VENT_H {
                return Surf::Vent;
            }
            match nb.floor() {
                Some(f) => wall_surf(f),
                None => {
                    if nb == Tile::Water {
                        Surf::WallExt
                    } else {
                        own.floor().map(wall_surf).unwrap_or(Surf::WallLab)
                    }
                }
            }
        }
        Part::ExtWall => Surf::WallBoundary,
        Part::Ground(_) => Surf::Concrete,
        Part::VentFloor => Surf::Vent,
        Part::Crate => Surf::Crate,
        Part::Stack(v) => Surf::Container(v),
        Part::Counter => Surf::Counter,
        Part::Rack => Surf::Rack,
        Part::WaterBed | Part::Invisible => Surf::WaterBed,
    }
}

fn top_surf(part: Part) -> Surf {
    match part {
        Part::Ground(f) => floor_surf(f),
        Part::Crate => Surf::Crate,
        Part::Stack(v) => Surf::Container(v),
        Part::Counter => Surf::Counter,
        Part::Rack => Surf::Rack,
        Part::WaterBed => Surf::WaterBed,
        Part::VentFloor => Surf::Vent,
        _ => Surf::Roof,
    }
}

fn bottom_surf(part: Part, own: Tile) -> Surf {
    match part {
        Part::Ceiling(Floor::Metal) => Surf::CeilingMetal,
        Part::Ceiling(_) => Surf::Ceiling,
        Part::VentTop => Surf::Vent,
        Part::Lintel | Part::Wall => own.floor().map(wall_surf).unwrap_or(Surf::WallLab),
        _ => Surf::Concrete,
    }
}

/// Builds merged meshes (one per surface material) for the whole level.
pub fn build_level_meshes(lvl: &Level) -> Vec<(Surf, Mesh)> {
    let mut bufs: HashMap<Surf, MeshBuf> = HashMap::new();
    let up = Vec3::Y;
    let dirs: [(i32, i32, Vec3); 4] = [(1, 0, Vec3::X), (-1, 0, Vec3::NEG_X), (0, 1, Vec3::Z), (0, -1, Vec3::NEG_Z)];
    for z in 0..lvl.h {
        for x in 0..lvl.w {
            let own = lvl.tile(x, z);
            let col = &lvl.cols[lvl.idx(x, z)];
            let x0 = x as f32 * CELL;
            let z0 = z as f32 * CELL;
            let x1 = x0 + CELL;
            let z1 = z0 + CELL;
            for (k, iv) in col.iter().enumerate() {
                if !iv.render {
                    continue;
                }
                // top face
                let covered_above = col.get(k + 1).map(|n| n.render && (n.y0 - iv.y1).abs() < 1e-4).unwrap_or(false);
                let skip_top = matches!(iv.part, Part::ExtWall);
                if !covered_above && !skip_top && iv.y1 < EXT_H - 0.01 {
                    let s = top_surf(iv.part);
                    let (su, sv) = s.uv_scale(true);
                    let y = iv.y1;
                    bufs.entry(s).or_default().quad(
                        [Vec3::new(x0, y, z1), Vec3::new(x1, y, z1), Vec3::new(x1, y, z0), Vec3::new(x0, y, z0)],
                        up,
                        Vec3::X,
                        Vec3::Z,
                        su,
                        sv,
                    );
                }
                // bottom face
                let covered_below = k > 0 && col.get(k - 1).map(|p| p.render && (p.y1 - iv.y0).abs() < 1e-4).unwrap_or(false);
                if iv.y0 > GROUND + 0.01 && !covered_below {
                    let s = bottom_surf(iv.part, own);
                    let (su, sv) = s.uv_scale(true);
                    let y = iv.y0;
                    bufs.entry(s).or_default().quad(
                        [Vec3::new(x0, y, z0), Vec3::new(x1, y, z0), Vec3::new(x1, y, z1), Vec3::new(x0, y, z1)],
                        -up,
                        Vec3::X,
                        Vec3::NEG_Z,
                        su,
                        sv,
                    );
                }
                // sides
                for (dx, dz, n) in dirs {
                    let nx = x + dx;
                    let nz = z + dz;
                    if !lvl.inb(nx, nz) {
                        continue;
                    }
                    let nb = lvl.tile(nx, nz);
                    let ncol = &lvl.cols[lvl.idx(nx, nz)];
                    // uncovered segments of [iv.y0, iv.y1]
                    let mut segs = vec![(iv.y0, iv.y1)];
                    for o in ncol.iter().filter(|o| o.render) {
                        let mut next = vec![];
                        for (a, b) in segs {
                            if o.y1 <= a || o.y0 >= b {
                                next.push((a, b));
                            } else {
                                if o.y0 > a {
                                    next.push((a, o.y0));
                                }
                                if o.y1 < b {
                                    next.push((o.y1, b));
                                }
                            }
                        }
                        segs = next;
                    }
                    // face plane
                    let t = up.cross(n);
                    let center = Vec3::new((x0 + x1) * 0.5, 0.0, (z0 + z1) * 0.5) + n * (CELL * 0.5);
                    for (a, b) in segs {
                        if b - a < 1e-3 {
                            continue;
                        }
                        let mut pieces = vec![(a, b)];
                        let indoor_wall = matches!(iv.part, Part::Wall) && nb.indoor() && !matches!(nb, Tile::Vent | Tile::Grate);
                        if indoor_wall && a < TRIM_H && b > TRIM_H && a > -0.01 {
                            pieces = vec![(a, TRIM_H), (TRIM_H, b)];
                        }
                        for (pa, pb) in pieces {
                            let s = if indoor_wall && pb <= TRIM_H + 1e-4 { Surf::Trim } else { side_surf(iv.part, own, nb, (pa + pb) * 0.5) };
                            let (su, sv) = s.uv_scale(false);
                            let hw = t * (CELL * 0.5);
                            let bl = center - hw + up * pa;
                            let br = center + hw + up * pa;
                            let tr = center + hw + up * pb;
                            let tl = center - hw + up * pb;
                            bufs.entry(s).or_default().quad([bl, br, tr, tl], n, t, -up, su, sv);
                        }
                    }
                }
            }
        }
    }
    bufs.into_iter().filter(|(_, b)| !b.is_empty()).map(|(s, b)| (s, b.into_mesh())).collect()
}

/// Glass panes (double sided material) for windows.
pub fn build_glass_mesh(lvl: &Level) -> Option<Mesh> {
    if lvl.glass.is_empty() {
        return None;
    }
    let mut b = MeshBuf::default();
    for &(x, z, along_x) in &lvl.glass {
        let c = Level::center(x, z);
        let (n, t) = if along_x { (Vec3::Z, Vec3::X) } else { (Vec3::X, Vec3::NEG_Z) };
        let hw = t * (CELL * 0.5);
        let y0 = SILL_H;
        let y1 = DOOR_H;
        b.quad(
            [c - hw + Vec3::Y * y0, c + hw + Vec3::Y * y0, c + hw + Vec3::Y * y1, c - hw + Vec3::Y * y1],
            n,
            t,
            Vec3::NEG_Y,
            2.0,
            2.0,
        );
    }
    Some(b.into_mesh())
}

/// Water surface over all water cells.
pub fn build_water_mesh(lvl: &Level) -> Option<Mesh> {
    let mut b = MeshBuf::default();
    for z in 0..lvl.h {
        for x in 0..lvl.w {
            if lvl.tile(x, z) != Tile::Water {
                continue;
            }
            let x0 = x as f32 * CELL;
            let z0 = z as f32 * CELL;
            let y = WATER_Y;
            b.quad(
                [Vec3::new(x0, y, z0 + CELL), Vec3::new(x0 + CELL, y, z0 + CELL), Vec3::new(x0 + CELL, y, z0), Vec3::new(x0, y, z0)],
                Vec3::Y,
                Vec3::X,
                Vec3::Z,
                6.0,
                6.0,
            );
        }
    }
    if b.is_empty() {
        None
    } else {
        Some(b.into_mesh())
    }
}
