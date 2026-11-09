//! Shared game types: app states, settings, events and the network protocol.
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub const PROTOCOL_VERSION: u32 = 1;
pub const DEFAULT_PORT: u16 = 27777;
pub const MAX_PLAYERS: usize = 4;

#[derive(States, Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppState {
    #[default]
    Boot,
    Menu,
    Connecting,
    InGame,
}

#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum NetRole {
    #[default]
    Offline,
    Host,
    Client,
}

impl NetRole {
    pub fn authority(&self) -> bool {
        *self != NetRole::Client
    }
}

pub fn is_authority(role: Res<NetRole>) -> bool {
    role.authority()
}

pub fn in_game(state: Res<State<AppState>>) -> bool {
    *state.get() == AppState::InGame
}

#[derive(Resource, Default, Clone, Copy)]
pub struct LocalId(pub u8);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Quality {
    Low,
    Medium,
    High,
    Ultra,
}

impl Quality {
    pub fn name(self) -> &'static str {
        match self {
            Quality::Low => "LOW",
            Quality::Medium => "MEDIUM",
            Quality::High => "HIGH",
            Quality::Ultra => "ULTRA",
        }
    }
    pub fn next(self) -> Self {
        match self {
            Quality::Low => Quality::Medium,
            Quality::Medium => Quality::High,
            Quality::High => Quality::Ultra,
            Quality::Ultra => Quality::Low,
        }
    }
}

#[derive(Resource, Clone)]
pub struct Settings {
    pub name: String,
    pub join_addr: String,
    pub port: u16,
    pub sensitivity: f32,
    pub quality: Quality,
    pub volume: f32,
    pub fov: f32,
    pub brightness: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            name: "Ghost".into(),
            join_addr: format!("127.0.0.1:{DEFAULT_PORT}"),
            port: DEFAULT_PORT,
            sensitivity: 1.0,
            quality: Quality::High,
            volume: 0.8,
            fov: 80.0,
            brightness: 0.0,
        }
    }
}

/// Which full-screen UI (if any) is open. `None` = mouse-look gameplay.
#[derive(Resource, Default, Clone, PartialEq, Debug)]
pub enum UiMode {
    #[default]
    None,
    Pause,
    Inventory(u8),
    Terminal(u16),
    Keypad(u16),
    Talk(u16),
    Read(u16),
    AugChoice,
    Complete,
}

/// Set to true whenever the open panel must be rebuilt.
#[derive(Resource, Default)]
pub struct UiDirty(pub bool);

/// Everything spawned for the current level (despawned on reload).
#[derive(Component)]
pub struct LevelEntity;

#[derive(Component)]
pub struct MainCamera;

// ---------------------------------------------------------------- item / combat types

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum WeaponKind {
    Baton,
    Pistol,
    Crossbow,
    Rifle,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum AmmoKind {
    Pistol,
    Rifle,
    Dart,
}

impl AmmoKind {
    pub fn idx(self) -> usize {
        self as usize
    }
    pub fn name(self) -> &'static str {
        match self {
            AmmoKind::Pistol => "10mm rounds",
            AmmoKind::Rifle => "7.62mm rounds",
            AmmoKind::Dart => "tranquilizer darts",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum ItemKind {
    Weapon(WeaponKind),
    Ammo(AmmoKind),
    Medkit,
    Biocell,
    Lockpick,
    Multitool,
    Key(u8),
    Datacube(u16),
    DataCore,
    AugCanister,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum DmgKind {
    Bullet,
    Melee,
    Tranq,
}

pub mod pflags {
    pub const CROUCH: u8 = 1;
    pub const LIGHT: u8 = 2;
    pub const CLOAK: u8 = 4;
    pub const RADAR: u8 = 8;
    pub const DOWNED: u8 = 16;
    pub const CORE: u8 = 32;
    pub const MOVING: u8 = 64;
    pub const SPRINT: u8 = 128;
}

/// Compact per-player state that every peer shares.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct PlayerNet {
    pub pos: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub flags: u8,
    pub weapon: u8,
    pub health: u8,
}

impl PlayerNet {
    pub fn has(&self, f: u8) -> bool {
        self.flags & f != 0
    }
    pub fn pos(&self) -> Vec3 {
        Vec3::from_array(self.pos)
    }
}

pub struct PlayerEntry {
    pub name: String,
    pub net: PlayerNet,
    pub avatar: Option<Entity>,
    /// light level at the player (computed by the authority for AI)
    pub visibility: f32,
}

#[derive(Resource, Default)]
pub struct Players {
    pub map: HashMap<u8, PlayerEntry>,
}

// ---------------------------------------------------------------- world requests / events

/// Something a player wants to change in the shared world. Clients send these to the host.
#[derive(Event, Clone, Debug, Serialize, Deserialize)]
pub enum WorldRequest {
    SetDoor { id: u16, open: bool },
    Unlock { id: u16 },
    Switch { id: u16 },
    BreakLight { id: u16 },
    Pickup { id: u16 },
    HitNpc { id: u16, dmg: f32, kind: DmgKind, dir: [f32; 3], head: bool },
    HitCamera { id: u16, dmg: f32 },
    HitTurret { id: u16, dmg: f32 },
    Noise { pos: [f32; 3], radius: f32 },
    SetCameras { on: bool },
    SetTurrets { mode: u8 },
    TriggerAlarm { pos: [f32; 3] },
    LootBody { id: u16 },
    ReadNote { id: u16 },
    Revive { target: u8 },
}

/// A request as seen by the authority (host / offline), tagged with the requesting player.
#[derive(Event, Clone, Debug)]
pub struct AuthRequest {
    pub from: u8,
    pub req: WorldRequest,
}

/// Authoritative world change, broadcast to everyone.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum WorldEvent {
    Door { id: u16, open: bool, locked: bool },
    Light { id: u16, on: bool, broken: bool },
    PickupTaken { id: u16, by: u8 },
    BodyLooted { id: u16, by: u8, pistol: u16, rifle: u16, darts: u16 },
    Alarm { active: bool, pos: [f32; 3] },
    Security { cameras: bool, turrets: u8 },
    Objective { id: u8 },
    Note { id: u16 },
    Infolink { id: u16 },
    Revived { target: u8 },
    Bark { npc: u16, line: u8 },
    MissionComplete { time: f32, kills: u16, knockouts: u16, alarms: u16 },
}

impl WorldEvent {
    /// Events that represent persistent state (replayed to late joiners).
    pub fn stateful(&self) -> bool {
        matches!(
            self,
            WorldEvent::Door { .. }
                | WorldEvent::Light { .. }
                | WorldEvent::PickupTaken { .. }
                | WorldEvent::BodyLooted { .. }
                | WorldEvent::Alarm { .. }
                | WorldEvent::Security { .. }
                | WorldEvent::Objective { .. }
                | WorldEvent::Note { .. }
        )
    }
}

#[derive(Event, Clone, Debug)]
pub struct WorldEv {
    pub ev: WorldEvent,
    /// quiet = replayed state (no messages / sounds)
    pub quiet: bool,
}

pub mod impact {
    pub const NONE: u8 = 0;
    pub const WALL: u8 = 1;
    pub const FLESH: u8 = 2;
    pub const METAL: u8 = 3;
    pub const GLASS: u8 = 4;
}

/// Cosmetic effects. Local ones are relayed to other peers.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Fx {
    /// weapon: 0 baton, 1 pistol, 2 crossbow, 3 rifle, 10 guard pistol, 11 guard rifle, 12 turret
    Shot { from: [f32; 3], to: [f32; 3], weapon: u8, impact: u8, normal: [f32; 3] },
    Sound { sfx: u8, pos: [f32; 3] },
}

#[derive(Event, Clone, Debug)]
pub struct FxEv {
    pub fx: Fx,
    pub remote: bool,
}

/// Damage dealt to the local player.
#[derive(Event, Clone, Debug)]
pub struct DamageEv {
    pub amount: f32,
    pub from: Vec3,
}

#[derive(Event, Clone, Debug)]
pub struct HudMsg(pub String);

#[derive(Event, Clone, Debug)]
pub struct Subtitle {
    pub who: String,
    pub text: String,
}

#[derive(Clone, Default, Debug)]
pub struct Stats {
    pub kills: u16,
    pub knockouts: u16,
    pub alarms: u16,
}

/// Replicated world state (kept on every peer).
#[derive(Resource)]
pub struct WorldState {
    pub alarm: bool,
    pub alarm_pos: Vec3,
    pub cameras_on: bool,
    /// 0 = off, 1 = default (alarm only), 2 = target hostiles
    pub turret_mode: u8,
    pub objectives: [bool; 4],
    pub notes: Vec<u16>,
    pub taken: HashSet<u16>,
    pub looted: HashSet<u16>,
    pub complete: Option<(f32, u16, u16, u16)>,
    pub elapsed: f32,
    pub stats: Stats,
}

impl Default for WorldState {
    fn default() -> Self {
        WorldState {
            alarm: false,
            alarm_pos: Vec3::ZERO,
            cameras_on: true,
            turret_mode: 1,
            objectives: [false; 4],
            notes: vec![],
            taken: HashSet::new(),
            looted: HashSet::new(),
            complete: None,
            elapsed: 0.0,
            stats: Stats::default(),
        }
    }
}

// ---------------------------------------------------------------- network protocol

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GuardNet {
    pub id: u16,
    pub pos: [f32; 3],
    pub yaw: f32,
    /// 0 idle, 1 walk, 2 run, 3 aim, 5 unconscious, 6 dead, 7 stagger
    pub anim: u8,
    pub alert: u8,
    pub light: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeviceNet {
    pub id: u16,
    pub yaw: f32,
    pub pitch: f32,
    /// 0 off, 1 idle, 2 tracking, 3 alarm/firing, 4 broken
    pub status: u8,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum C2S {
    Hello { name: String, ver: u32, level: u64 },
    State(PlayerNet),
    Req(WorldRequest),
    Fx(Fx),
    Chat(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum S2C {
    Welcome { id: u8, players: Vec<(u8, String)>, log: Vec<WorldEvent>, elapsed: f32 },
    Reject(String),
    Joined(u8, String),
    Left(u8),
    States(Vec<(u8, PlayerNet)>),
    Npcs { guards: Vec<GuardNet>, cams: Vec<DeviceNet>, turrets: Vec<DeviceNet> },
    Ev(WorldEvent),
    Fx(Fx),
    Damage { amount: f32, from: [f32; 3] },
    Chat(u8, String),
}

/// Update ordering inside a frame.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum GameSet {
    Input,
    Logic,
    Route,
    Authority,
    Apply,
    Present,
    Net,
}
