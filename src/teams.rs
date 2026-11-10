//! Team match: Orange vs Blue.
//!
//! Each round picks one objective at random:
//! * **Steal** - a prototype appears at one of the map's `steal_spot`s. Find it and carry it
//!   to your team's extraction zone. Kill the carrier and it drops where they fell.
//! * **Hack** - three terminals appear at random `hack_spot`s. Hold [F] on a terminal to
//!   hack it for your team (enemy-held terminals can be hacked back). Hold two at once to win.
//!
//! Nothing is marked on the HUD until someone on your team has actually seen it.
//!
//! The authority (host, or the local game offline) validates every request and broadcasts
//! `WorldEvent`s; `apply_match` applies them identically everywhere.
use bevy::ecs::hierarchy::ChildSpawnerCommands;
use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;
use std::collections::VecDeque;

use crate::audio::*;
use crate::collision::*;
use crate::game::*;
use crate::hud::{txt, C_ACCENT, C_BORDER, C_DIM, C_PANEL, C_TEXT};
use crate::items::*;
use crate::level::*;
use crate::materials::*;
use crate::net::Net;
use crate::player::*;
use crate::util::*;
use crate::world::*;

pub const RESPAWN_TIME: f32 = 5.0;
/// A dropped prototype nobody picks up is moved to a new random spot after this long.
pub const DROP_RETURN_TIME: f32 = 30.0;
/// How close you must be (with line of sight) to discover an objective.
pub const DISCOVER_RANGE: f32 = 15.0;
pub const OBJ_REACH: f32 = 3.0;
/// Seconds to hack a neutral terminal (enemy-held ones take 50% longer).
pub const HACK_TIME: f32 = 7.0;
pub const TERM_LETTERS: [&str; 3] = ["A", "B", "C"];
const MARKERS: usize = 16;

#[derive(Resource, Default)]
pub struct MatchState {
    pub net: MatchNet,
    /// authority: the prototype is lying where someone dropped it
    pub dropped: bool,
    pub drop_t: f32,
    pub over_t: f32,
    pub last_carrier_pos: Vec3,
    /// local: discovery requests already sent this round
    pub sent_discover: [bool; 4],
    /// local: the authority moved us to the other team
    pub respawn_local: bool,
    /// kill feed lines (text, real time)
    pub feed: VecDeque<(String, f32)>,
    /// big centre message (text, colour, real time)
    pub banner: Option<(String, String, Color, f32)>,
}

impl MatchState {
    pub fn begin_round(&mut self, m: MatchNet) {
        self.net = m;
        self.dropped = false;
        self.drop_t = 0.0;
        self.over_t = 0.0;
        self.sent_discover = [false; 4];
        self.respawn_local = false;
        self.last_carrier_pos = Vec3::from_array(self.net.obj_pos);
    }
    pub fn hack_count(&self, team: u8) -> usize {
        self.net.hack_owner.iter().filter(|o| **o == team).count()
    }
    pub fn known(&self, team: u8, what: usize) -> bool {
        team < 2 && what < 4 && self.net.discovered[team as usize][what]
    }
    pub fn objective_name(&self) -> &'static str {
        if self.net.objective == OBJ_STEAL {
            "STEAL THE PROTOTYPE"
        } else {
            "HACK 2 OF 3 TERMINALS"
        }
    }
    pub fn objective_help(&self) -> &'static str {
        if self.net.objective == OBJ_STEAL {
            "Find it, take it, bring it to your extraction zone"
        } else {
            "Find the terminals and hold [F] to hack them. Hold two to win"
        }
    }
}

/// World events a late joiner must replay (doors, lights...), applied once the level exists.
#[derive(Resource, Default)]
pub struct PendingReplay(pub Vec<WorldEvent>);

#[derive(Component)]
pub struct ProtoEnt;

#[derive(Component)]
pub struct HackTermEnt {
    pub idx: u8,
    pub screen: Entity,
    pub light: Entity,
    pub owner_shown: u8,
}

#[derive(Component)]
pub struct TeamHudRoot;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum THud {
    ScoreOrange,
    ScoreBlue,
    Clock,
    Objective,
    Help,
    Status,
    PipRow,
    Pip(u8),
    Feed,
    BannerBox,
    Banner,
    BannerSub,
    Board,
    Marker(u8),
    MarkerText(u8),
}

// ------------------------------------------------------------------ helpers

/// Choose the objective and spots of a new round.
pub fn roll_round(level: &Level, rng: &mut Rng, round: u16, scores: [u16; 2]) -> MatchNet {
    let objective = if rng.chance(0.5) { OBJ_STEAL } else { OBJ_HACK };
    let steal_spot = rng.below(level.steal_spots.len().max(1)) as u8;
    let mut order: Vec<u8> = (0..level.hack_spots.len().max(3) as u8).collect();
    for i in (1..order.len()).rev() {
        let j = rng.below(i + 1);
        order.swap(i, j);
    }
    let n = level.hack_spots.len().max(1) as u8;
    MatchNet {
        round,
        objective,
        scores,
        steal_spot,
        hack_spots: [order[0] % n, order[1] % n, order[2] % n],
        hack_owner: [NO_TEAM; 3],
        carrier: NO_PLAYER,
        obj_pos: level.steal_pos(steal_spot).to_array(),
        discovered: [[false; 4]; 2],
        time_left: ROUND_TIME,
        winner: None,
    }
}

/// A random respawn point of a team, facing the middle of the map.
pub fn spawn_point(level: &Level, team: u8, rng: &mut Rng) -> (Vec3, f32) {
    let p = match level.team_spawns.get(team as usize).filter(|l| !l.is_empty()) {
        Some(list) => list[rng.below(list.len())],
        None => level.spawns[0],
    };
    let center = Vec3::new(level.w as f32 * CELL * 0.5, 0.0, level.h as f32 * CELL * 0.5);
    (p, dir_yaw(center - p))
}

/// What every operative carries into a team match.
pub fn team_loadout() -> Inventory {
    let mut inv = Inventory::default();
    inv.add(ItemKind::Weapon(WeaponKind::Crossbow), 1);
    inv.ammo[AmmoKind::Dart.idx()] += 4;
    inv.ammo[AmmoKind::Pistol.idx()] += 10;
    inv.add(ItemKind::Medkit, 1);
    inv
}

pub fn weapon_label(w: u8) -> &'static str {
    match w {
        0 => "baton",
        1 => "pistol",
        2 => "crossbow",
        3 => "rifle",
        _ => "",
    }
}

/// Where the prototype is drawn when it lies on the ground.
pub fn proto_ground_pos(ms: &MatchState, level: &Level) -> Vec3 {
    let p = Vec3::from_array(ms.net.obj_pos);
    let home = level.steal_pos(ms.net.steal_spot);
    if p.with_y(0.0).distance(home) < 0.1 {
        home + Vec3::Y * 1.01
    } else {
        p.with_y(0.03)
    }
}

/// Objectives currently in the world: (what, position). what 0 = prototype, 1..=3 terminals.
/// A carried prototype is not listed (it moves with its carrier).
pub fn objective_points(ms: &MatchState, level: &Level) -> Vec<(usize, Vec3)> {
    let mut v = vec![];
    if ms.net.objective == OBJ_STEAL {
        if ms.net.carrier == NO_PLAYER {
            v.push((0, proto_ground_pos(ms, level) + Vec3::Y * 0.2));
        }
    } else {
        for (i, s) in ms.net.hack_spots.iter().enumerate() {
            v.push((i + 1, level.hack_pos(*s) + Vec3::Y * 1.0));
        }
    }
    v
}

pub fn hack_time(owner: u8, computer: u8) -> f32 {
    let base = if owner == NO_TEAM { HACK_TIME } else { HACK_TIME * 1.5 };
    base / (1.0 + computer as f32 * 0.25)
}

// ------------------------------------------------------------------ plugin

/// The team-match authority systems (bots run after them).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct TeamAuthority;

pub struct TeamsPlugin;

impl Plugin for TeamsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MatchState>()
            .init_resource::<PendingReplay>()
            .add_systems(OnEnter(AppState::RoundReset), |mut next: ResMut<NextState<AppState>>| next.set(AppState::InGame))
            .add_systems(OnEnter(AppState::InGame), (spawn_objectives.after(crate::world::spawn_level), spawn_team_hud).run_if(teams_mode))
            .add_systems(OnExit(AppState::InGame), despawn_team_hud)
            .add_systems(Update, flush_replay.in_set(GameSet::Route).run_if(in_game))
            .add_systems(
                Update,
                (match_requests, match_tick).chain().in_set(TeamAuthority).in_set(GameSet::Authority).run_if(is_authority).run_if(in_game).run_if(teams_mode),
            )
            .add_systems(Update, apply_match.in_set(GameSet::Apply).run_if(teams_mode))
            .add_systems(
                Update,
                (team_life, publish_carrier, local_discovery).chain().in_set(GameSet::Logic).after(PlayerLogic).run_if(in_game).run_if(teams_mode),
            )
            .add_systems(
                Update,
                (animate_objectives, update_team_hud, update_markers, update_scoreboard).in_set(GameSet::Present).run_if(in_game).run_if(teams_mode),
            );
    }
}

fn flush_replay(mut replay: ResMut<PendingReplay>, mut out: EventWriter<WorldEv>) {
    for ev in replay.0.drain(..) {
        out.write(WorldEv { ev, quiet: true });
    }
}

// ------------------------------------------------------------------ authority

#[allow(clippy::too_many_arguments)]
fn match_requests(
    mut reqs: EventReader<AuthRequest>,
    ms: Res<MatchState>,
    players: Res<Players>,
    level: Res<Level>,
    mut out: EventWriter<WorldEv>,
    mut dmg: EventWriter<PlayerDamage>,
) {
    let mut carrier = ms.net.carrier;
    let mut ended = ms.net.winner.is_some();
    let mut owners = ms.net.hack_owner;
    let mut found = ms.net.discovered;
    let obj_pos = proto_ground_pos(&ms, &level);
    for r in reqs.read() {
        let Some(me) = players.map.get(&r.from) else { continue };
        let pos = me.net.pos();
        let alive = !me.net.has(pflags::DOWNED);
        let team = me.team;
        let mut send = |ev: WorldEvent| {
            out.write(WorldEv { ev, quiet: false });
        };
        match &r.req {
            WorldRequest::HitPlayer { target, dmg: amount, kind, dir, head, weapon } => {
                let Some(victim) = players.map.get(target) else { continue };
                if *target == r.from || victim.team == team || victim.net.has(pflags::DOWNED) {
                    continue;
                }
                let d = Vec3::from_array(*dir).with_y(0.0).normalize_or_zero();
                let mut amount = amount.clamp(0.0, 90.0);
                match kind {
                    DmgKind::Bullet => {
                        if *head {
                            amount *= 2.2;
                        }
                    }
                    DmgKind::Tranq => amount = if *head { 95.0 } else { 30.0 },
                    DmgKind::Melee => {
                        // a baton strike from behind is a silent takedown, like on guards
                        if yaw_dir(victim.net.yaw).dot(d) > 0.45 {
                            amount = 200.0;
                        }
                    }
                }
                dmg.write(PlayerDamage { target: *target, amount, from: pos + Vec3::Y * 1.4, by: r.from, weapon: *weapon });
            }
            WorldRequest::Died { killer, weapon } => {
                let killer = if *killer == r.from { NO_PLAYER } else { *killer };
                send(WorldEvent::Kill { killer, victim: r.from, weapon: *weapon });
                if carrier == r.from {
                    carrier = NO_PLAYER;
                    send(WorldEvent::ObjDropped { pos: pos.to_array() });
                }
            }
            WorldRequest::TakeObjective => {
                if ms.net.objective == OBJ_STEAL && !ended && carrier == NO_PLAYER && alive && (pos + Vec3::Y * 0.6).distance(obj_pos) < OBJ_REACH + 0.6 {
                    carrier = r.from;
                    send(WorldEvent::ObjTaken { by: r.from });
                }
            }
            WorldRequest::HackObjective { idx } => {
                let i = *idx as usize;
                if ms.net.objective != OBJ_HACK || ended || i >= 3 || !alive || team >= 2 || owners[i] == team {
                    continue;
                }
                let tp = level.hack_pos(ms.net.hack_spots[i]);
                if pos.with_y(0.0).distance(tp) > OBJ_REACH + 1.0 {
                    continue;
                }
                owners[i] = team;
                send(WorldEvent::Hacked { idx: *idx, team, by: r.from });
                if owners.iter().filter(|o| **o == team).count() >= HACK_TO_WIN {
                    let mut scores = ms.net.scores;
                    scores[team as usize] += 1;
                    ended = true;
                    send(WorldEvent::RoundEnd { winner: team, scores });
                }
            }
            WorldRequest::Discover { what } => {
                let w = *what as usize;
                let valid = (w == 0 && ms.net.objective == OBJ_STEAL) || ((1..=3).contains(&w) && ms.net.objective == OBJ_HACK);
                if valid && team < 2 && !found[team as usize][w] {
                    found[team as usize][w] = true;
                    send(WorldEvent::Discovered { team, what: *what, by: r.from });
                }
            }
            WorldRequest::SwitchTeam => {
                if team < 2 && !me.bot {
                    if carrier == r.from {
                        carrier = NO_PLAYER;
                        send(WorldEvent::ObjDropped { pos: pos.to_array() });
                    }
                    send(WorldEvent::TeamChange { id: r.from, team: other_team(team) });
                }
            }
            _ => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn match_tick(
    time: Res<Time>,
    mut ms: ResMut<MatchState>,
    players: Res<Players>,
    level: Res<Level>,
    mut rng: ResMut<GameRng>,
    mut net: ResMut<Net>,
    mut out: EventWriter<WorldEv>,
    mut next: ResMut<NextState<AppState>>,
) {
    let dt = time.delta_secs();
    if ms.net.winner.is_some() {
        ms.over_t += dt;
        if ms.over_t >= ROUND_OVER_TIME {
            let m = roll_round(&level, &mut rng.0, ms.net.round + 1, ms.net.scores);
            ms.begin_round(m.clone());
            net.log.clear();
            net.broadcast(&S2C::RoundStart(m));
            next.set(AppState::RoundReset);
        }
        return;
    }
    ms.net.time_left = (ms.net.time_left - dt).max(0.0);
    if ms.net.time_left <= 0.0 {
        let (o, b) = (ms.hack_count(TEAM_ORANGE), ms.hack_count(TEAM_BLUE));
        let winner = if ms.net.objective == OBJ_HACK && o != b {
            if o > b {
                TEAM_ORANGE
            } else {
                TEAM_BLUE
            }
        } else {
            NO_TEAM
        };
        end_round(winner, &mut ms, &mut out);
        return;
    }
    if ms.net.objective != OBJ_STEAL {
        return;
    }
    let carrier = ms.net.carrier;
    if carrier != NO_PLAYER {
        match players.map.get(&carrier) {
            None => {
                // the carrier left the game
                let pos = ms.last_carrier_pos;
                ms.net.carrier = NO_PLAYER;
                out.write(WorldEv { ev: WorldEvent::ObjDropped { pos: pos.to_array() }, quiet: false });
            }
            Some(c) if !c.net.has(pflags::DOWNED) => {
                let p = c.net.pos();
                ms.last_carrier_pos = p;
                if c.team < 2 && level.in_base(c.team, p) {
                    end_round(c.team, &mut ms, &mut out);
                }
            }
            _ => {}
        }
    } else if ms.dropped {
        ms.drop_t += dt;
        if ms.drop_t > DROP_RETURN_TIME {
            let n = level.steal_spots.len().max(1);
            let mut spot = rng.0.below(n) as u8;
            if n > 1 && spot == ms.net.steal_spot {
                spot = ((spot as usize + 1) % n) as u8;
            }
            ms.dropped = false;
            ms.drop_t = 0.0;
            out.write(WorldEv { ev: WorldEvent::ObjReturned { spot }, quiet: false });
        }
    }
}

fn end_round(winner: u8, ms: &mut MatchState, out: &mut EventWriter<WorldEv>) {
    let mut scores = ms.net.scores;
    if winner < 2 {
        scores[winner as usize] += 1;
    }
    ms.net.winner = Some(winner);
    out.write(WorldEv { ev: WorldEvent::RoundEnd { winner, scores }, quiet: false });
}

// ------------------------------------------------------------------ apply (everyone)

#[allow(clippy::too_many_arguments)]
fn apply_match(
    time: Res<Time<Real>>,
    mut evs: EventReader<WorldEv>,
    mut ms: ResMut<MatchState>,
    mut players: ResMut<Players>,
    local: Res<LocalId>,
    level: Res<Level>,
    mut msgs: EventWriter<HudMsg>,
    mut snd: EventWriter<SoundEv>,
) {
    let now = time.elapsed_secs();
    let my_team = players.team_of(local.0);
    for WorldEv { ev, .. } in evs.read() {
        match ev {
            WorldEvent::Kill { killer, victim, weapon } => {
                if let Some(p) = players.map.get_mut(victim) {
                    p.deaths += 1;
                }
                let line = if *killer != NO_PLAYER && killer != victim {
                    if let Some(p) = players.map.get_mut(killer) {
                        p.kills += 1;
                    }
                    let w = weapon_label(*weapon);
                    if w.is_empty() {
                        format!("{} > {}", players.name_of(*killer), players.name_of(*victim))
                    } else {
                        format!("{} [{w}] {}", players.name_of(*killer), players.name_of(*victim))
                    }
                } else {
                    format!("{} died", players.name_of(*victim))
                };
                ms.feed.push_back((line, now));
                while ms.feed.len() > 6 {
                    ms.feed.pop_front();
                }
            }
            WorldEvent::ObjTaken { by } => {
                ms.net.carrier = *by;
                ms.dropped = false;
                let team = players.team_of(*by);
                let text = if *by == local.0 {
                    "You have the prototype - get it to your extraction zone!".to_string()
                } else if team == my_team {
                    format!("{} has the prototype - cover them!", players.name_of(*by))
                } else {
                    "The enemy has stolen the prototype!".to_string()
                };
                msgs.write(HudMsg(text));
                snd.write(SoundEv::ui(if team == my_team { Sfx::Granted } else { Sfx::Denied }, 0.6));
            }
            WorldEvent::ObjDropped { pos } => {
                ms.net.carrier = NO_PLAYER;
                ms.net.obj_pos = *pos;
                ms.last_carrier_pos = Vec3::from_array(*pos);
                ms.dropped = true;
                ms.drop_t = 0.0;
                msgs.write(HudMsg("The prototype was dropped".into()));
            }
            WorldEvent::ObjReturned { spot } => {
                ms.net.carrier = NO_PLAYER;
                ms.net.steal_spot = *spot;
                ms.net.obj_pos = level.steal_pos(*spot).to_array();
                ms.net.discovered[0][0] = false;
                ms.net.discovered[1][0] = false;
                ms.sent_discover[0] = false;
                ms.dropped = false;
                msgs.write(HudMsg("Nobody recovered the prototype - it was moved to a new hiding place".into()));
                snd.write(SoundEv::ui(Sfx::Chirp, 0.5));
            }
            WorldEvent::Hacked { idx, team, by } => {
                if let Some(o) = ms.net.hack_owner.get_mut(*idx as usize) {
                    *o = *team;
                }
                let letter = TERM_LETTERS.get(*idx as usize).copied().unwrap_or("?");
                let text = if *team == my_team {
                    format!("Terminal {letter} hacked by {}", players.name_of(*by))
                } else {
                    format!("The enemy hacked terminal {letter}!")
                };
                msgs.write(HudMsg(text));
                snd.write(SoundEv::ui(if *team == my_team { Sfx::Granted } else { Sfx::Denied }, 0.6));
            }
            WorldEvent::Discovered { team, what, by } => {
                if let Some(row) = ms.net.discovered.get_mut(*team as usize) {
                    if let Some(f) = row.get_mut(*what as usize) {
                        *f = true;
                    }
                }
                if *team == my_team {
                    let thing = if *what == 0 { "the prototype".to_string() } else { format!("terminal {}", TERM_LETTERS[(*what as usize - 1).min(2)]) };
                    let who = if *by == local.0 { "You".to_string() } else { players.name_of(*by) };
                    msgs.write(HudMsg(format!("{who} found {thing} - marked on your HUD")));
                    snd.write(SoundEv::ui(Sfx::Chirp, 0.5));
                }
            }
            WorldEvent::RoundEnd { winner, scores } => {
                ms.net.winner = Some(*winner);
                ms.net.scores = *scores;
                ms.over_t = 0.0;
                let (title, color) = if *winner < 2 { (format!("{} TEAM WINS THE ROUND", team_name(*winner)), team_color(*winner)) } else { ("DRAW".to_string(), C_TEXT) };
                let sub = format!("ORANGE {}  -  {} BLUE      next round in {:.0}s", scores[0], scores[1], ROUND_OVER_TIME);
                ms.banner = Some((title, sub, color, now));
                snd.write(SoundEv::ui(if *winner == my_team { Sfx::Granted } else { Sfx::Denied }, 0.8));
            }
            WorldEvent::TeamChange { id, team } => {
                if let Some(p) = players.map.get_mut(id) {
                    p.team = *team;
                    p.avatar = None;
                }
                if *id == local.0 {
                    ms.respawn_local = true;
                }
                msgs.write(HudMsg(format!("{} switched to team {}", players.name_of(*id), team_name(*team))));
            }
            _ => {}
        }
    }
}

// ------------------------------------------------------------------ local player

#[allow(clippy::too_many_arguments)]
fn team_life(
    mut me: ResMut<Me>,
    mut inv: ResMut<Inventory>,
    mut augs: ResMut<Augs>,
    mut ms: ResMut<MatchState>,
    level: Res<Level>,
    players: Res<Players>,
    local: Res<LocalId>,
    mut rng: ResMut<GameRng>,
    mut req: EventWriter<WorldRequest>,
    mut was_dead: Local<bool>,
) {
    if me.dead && !*was_dead {
        req.write(WorldRequest::Died { killer: me.killer, weapon: me.killer_weapon });
    }
    *was_dead = me.dead;
    let respawn = ms.respawn_local || (me.dead && me.dead_t > RESPAWN_TIME);
    if !respawn {
        return;
    }
    ms.respawn_local = false;
    let team = players.team_of(local.0);
    let (p, yaw) = spawn_point(&level, team, &mut rng.0);
    let energy = me.energy;
    *me = Me::new(p, yaw);
    me.energy = energy.max(40.0);
    *inv = team_loadout();
    augs.active = [false; 6];
    *was_dead = false;
}

fn publish_carrier(ms: Res<MatchState>, local: Res<LocalId>, mut players: ResMut<Players>) {
    if ms.net.carrier == local.0 {
        if let Some(p) = players.map.get_mut(&local.0) {
            p.net.flags |= pflags::CORE;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn local_discovery(
    time: Res<Time>,
    mut acc: Local<f32>,
    me: Res<Me>,
    mut ms: ResMut<MatchState>,
    players: Res<Players>,
    local: Res<LocalId>,
    level: Res<Level>,
    phys: Res<Physics>,
    mut req: EventWriter<WorldRequest>,
) {
    *acc += time.delta_secs();
    if *acc < 0.25 || me.dead {
        return;
    }
    *acc = 0.0;
    let team = players.team_of(local.0);
    if team >= 2 || ms.net.winner.is_some() {
        return;
    }
    let eye = me.eye();
    for (what, pos) in objective_points(&ms, &level) {
        if ms.known(team, what) || ms.sent_discover[what] {
            continue;
        }
        if eye.distance(pos) < DISCOVER_RANGE && phys.los(eye, pos) {
            ms.sent_discover[what] = true;
            req.write(WorldRequest::Discover { what: what as u8 });
        }
    }
}

// ------------------------------------------------------------------ objective entities

fn spawn_terminal(commands: &mut Commands, mats: &Mats, shapes: &Shapes, phys: &mut Physics, idx: u8, x: i32, z: i32, yaw: f32) {
    let c = Level::center(x, z);
    let face = yaw_dir(yaw);
    let desk_c = c - face * 0.5;
    let mut screen = Entity::PLACEHOLDER;
    let mut light = Entity::PLACEHOLDER;
    let root = commands
        .spawn((Transform::from_translation(desk_c).with_rotation(Quat::from_rotation_y(yaw)), Visibility::default(), LevelEntity))
        .with_children(|p| {
            p.spawn((Mesh3d(shapes.cube.clone()), MeshMaterial3d(mats.surf(Surf::Counter)), Transform::from_xyz(0.0, 0.76, 0.0).with_scale(Vec3::new(1.4, 0.05, 0.62))));
            for sx in [-0.67f32, 0.67] {
                p.spawn((Mesh3d(shapes.cube.clone()), MeshMaterial3d(mats.gun_dark.clone()), Transform::from_xyz(sx, 0.37, 0.0).with_scale(Vec3::new(0.04, 0.74, 0.58))));
            }
            // a chunky "uplink" tower next to the screen so it reads as an objective
            p.spawn((Mesh3d(shapes.cube.clone()), MeshMaterial3d(mats.gunmetal.clone()), Transform::from_xyz(0.48, 1.05, 0.12).with_scale(Vec3::new(0.26, 0.55, 0.3))));
            p.spawn((Mesh3d(shapes.cube.clone()), MeshMaterial3d(mats.plastic.clone()), Transform::from_xyz(-0.12, 1.1, 0.16).with_scale(Vec3::new(0.7, 0.46, 0.05))));
            screen = p
                .spawn((Mesh3d(shapes.cube.clone()), MeshMaterial3d(mats.screen.clone()), Transform::from_xyz(-0.12, 1.1, 0.132).with_scale(Vec3::new(0.64, 0.4, 0.006)), NotShadowCaster))
                .id();
            p.spawn((Mesh3d(shapes.cube.clone()), MeshMaterial3d(mats.gun_dark.clone()), Transform::from_xyz(-0.12, 0.88, 0.19).with_scale(Vec3::new(0.06, 0.2, 0.06))));
            p.spawn((Mesh3d(shapes.cube.clone()), MeshMaterial3d(mats.plastic.clone()), Transform::from_xyz(-0.12, 0.795, -0.12).with_scale(Vec3::new(0.46, 0.02, 0.15))));
            light = p.spawn((PointLight { intensity: 9_000.0, range: 4.0, color: Color::srgb(0.3, 0.9, 0.8), shadows_enabled: false, ..default() }, Transform::from_xyz(0.0, 1.2, -0.3))).id();
        })
        .id();
    commands.entity(root).insert(HackTermEnt { idx, screen, light, owner_shown: NO_TEAM });
    let half = if face.z.abs() > 0.5 { Vec3::new(0.7, 0.4, 0.31) } else { Vec3::new(0.31, 0.4, 0.7) };
    phys.props.push(Aabb::centered(desk_c + Vec3::Y * 0.4, half));
    commands.spawn((
        Transform::from_translation(desk_c),
        Interactable { kind: Interact::HackTerm(idx), aabb: Aabb::centered(desk_c + Vec3::Y * 1.0, half + Vec3::new(0.05, 0.3, 0.05)) },
        LevelEntity,
    ));
}

#[allow(clippy::too_many_arguments)]
fn spawn_objectives(mut commands: Commands, level: Res<Level>, mats: Res<Mats>, shapes: Res<Shapes>, ms: Res<MatchState>, mut phys: ResMut<Physics>) {
    // extraction zones + a coloured beacon over each base
    for team in [TEAM_ORANGE, TEAM_BLUE] {
        let t = team as usize;
        let cells = &level.team_bases[t];
        if cells.is_empty() {
            continue;
        }
        for &(x, z) in cells {
            let c = Level::center(x, z);
            commands.spawn((
                Mesh3d(shapes.quad.clone()),
                MeshMaterial3d(mats.team_holo[t].clone()),
                Transform::from_xyz(c.x, 0.025, c.z).with_scale(Vec3::new(CELL * 0.96, 1.0, CELL * 0.96)),
                NotShadowCaster,
                LevelEntity,
            ));
        }
        let center = level.base_center(team);
        commands.spawn((
            Mesh3d(shapes.cylinder.clone()),
            MeshMaterial3d(mats.team_holo[t].clone()),
            Transform::from_xyz(center.x, 15.0, center.z).with_scale(Vec3::new(0.45, 30.0, 0.45)),
            NotShadowCaster,
            LevelEntity,
        ));
        commands.spawn((
            PointLight { intensity: 40_000.0, range: 8.0, color: team_color(team), shadows_enabled: false, ..default() },
            Transform::from_translation(center + Vec3::Y * 1.5),
            LevelEntity,
        ));
    }

    if ms.net.objective == OBJ_STEAL {
        let home = level.steal_pos(ms.net.steal_spot);
        commands.spawn((Mesh3d(shapes.cylinder.clone()), MeshMaterial3d(mats.metal.clone()), Transform::from_translation(home + Vec3::Y * 0.5).with_scale(Vec3::new(0.5, 1.0, 0.5)), LevelEntity));
        commands.spawn((Mesh3d(shapes.cylinder.clone()), MeshMaterial3d(mats.cyan_glow.clone()), Transform::from_translation(home + Vec3::Y * 1.0).with_scale(Vec3::new(0.54, 0.02, 0.54)), LevelEntity));
        phys.props.push(Aabb::centered(home + Vec3::Y * 0.5, Vec3::new(0.25, 0.5, 0.25)));
        let pos = proto_ground_pos(&ms, &level);
        let parts = item_parts(ItemKind::DataCore, &mats, &shapes);
        commands
            .spawn((
                Transform::from_translation(pos),
                Visibility::default(),
                ProtoEnt,
                Spin(0.6),
                Interactable { kind: Interact::Objective, aabb: Aabb::centered(pos + Vec3::Y * 0.2, Vec3::new(0.35, 0.3, 0.35)) },
                LevelEntity,
            ))
            .with_children(|c| {
                spawn_parts(c, &parts);
                c.spawn((PointLight { intensity: 20_000.0, range: 4.5, color: Color::srgb(0.3, 0.85, 1.0), shadows_enabled: false, ..default() }, Transform::from_xyz(0.0, 0.25, 0.0)));
            });
    } else {
        for (i, s) in ms.net.hack_spots.iter().enumerate() {
            if let Some(&(x, z, yaw)) = level.hack_spots.get(*s as usize) {
                spawn_terminal(&mut commands, &mats, &shapes, &mut phys, i as u8, x, z, yaw);
            }
        }
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn animate_objectives(
    ms: Res<MatchState>,
    level: Res<Level>,
    local: Res<LocalId>,
    players: Res<Players>,
    mats: Res<Mats>,
    avatars: Query<(&crate::avatars::Avatar, &Transform), Without<ProtoEnt>>,
    mut proto: Query<(&mut Transform, &mut Visibility, &mut Interactable), With<ProtoEnt>>,
    mut terms: Query<&mut HackTermEnt>,
    mut screens: Query<&mut MeshMaterial3d<StandardMaterial>>,
    mut lights: Query<&mut PointLight>,
) {
    let my_team = players.team_of(local.0);
    let away = Aabb::centered(Vec3::new(0.0, -500.0, 0.0), Vec3::splat(0.01));
    for (mut tf, mut vis, mut it) in proto.iter_mut() {
        let carrier = ms.net.carrier;
        if carrier == NO_PLAYER {
            let p = proto_ground_pos(&ms, &level);
            tf.translation = p;
            *vis = Visibility::Inherited;
            it.aabb = Aabb::centered(p + Vec3::Y * 0.2, Vec3::new(0.35, 0.3, 0.35));
        } else {
            it.aabb = away;
            let carried = avatars.iter().find(|(a, _)| a.id == carrier).map(|(_, t)| (t.translation, t.rotation));
            match carried {
                Some((p, rot)) if carrier != local.0 => {
                    let cloaked = players.map.get(&carrier).map(|c| c.net.has(pflags::CLOAK) && c.team != my_team).unwrap_or(false);
                    tf.translation = p + Vec3::Y * 1.05 + rot * Vec3::new(0.0, 0.0, 0.22);
                    *vis = if cloaked { Visibility::Hidden } else { Visibility::Inherited };
                }
                _ => *vis = Visibility::Hidden,
            }
        }
    }
    for mut t in terms.iter_mut() {
        let owner = ms.net.hack_owner.get(t.idx as usize).copied().unwrap_or(NO_TEAM);
        if owner == t.owner_shown {
            continue;
        }
        t.owner_shown = owner;
        if let Ok(mut m) = screens.get_mut(t.screen) {
            m.0 = if owner < 2 { mats.team_screen[owner as usize].clone() } else { mats.screen.clone() };
        }
        if let Ok(mut l) = lights.get_mut(t.light) {
            l.color = if owner < 2 { team_color(owner) } else { Color::srgb(0.3, 0.9, 0.8) };
        }
    }
}

// ------------------------------------------------------------------ HUD

fn despawn_team_hud(mut commands: Commands, q: Query<Entity, With<TeamHudRoot>>) {
    for e in q.iter() {
        commands.entity(e).despawn();
    }
}

fn hud_text(p: &mut ChildSpawnerCommands, size: f32, color: Color, el: THud) {
    p.spawn((Text::new(""), TextFont { font_size: size, ..default() }, TextColor(color), el));
}

fn spawn_team_hud(mut commands: Commands, mut ms: ResMut<MatchState>, time: Res<Time<Real>>) {
    let full = Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() };
    ms.banner = Some((format!("ROUND {}", ms.net.round.max(1)), format!("{}  -  {}", ms.objective_name(), ms.objective_help()), C_ACCENT, time.elapsed_secs()));
    commands.spawn((full, GlobalZIndex(5), TeamHudRoot)).with_children(|r| {
        // score / clock / objective (top centre)
        r.spawn(Node {
            position_type: PositionType::Absolute,
            top: Val::Px(12.0),
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: Val::Px(2.0),
            ..default()
        })
        .with_children(|c| {
            c.spawn(Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: Val::Px(18.0), ..default() }).with_children(|row| {
                hud_text(row, 26.0, team_color(TEAM_ORANGE), THud::ScoreOrange);
                hud_text(row, 18.0, C_TEXT, THud::Clock);
                hud_text(row, 26.0, team_color(TEAM_BLUE), THud::ScoreBlue);
            });
            hud_text(c, 15.0, C_ACCENT, THud::Objective);
            hud_text(c, 12.0, C_DIM, THud::Help);
            c.spawn((Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(6.0), margin: UiRect::top(Val::Px(4.0)), ..default() }, THud::PipRow)).with_children(|row| {
                for i in 0..3u8 {
                    row.spawn((
                        Node { width: Val::Px(34.0), height: Val::Px(24.0), border: UiRect::all(Val::Px(1.0)), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
                        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
                        BorderColor(C_BORDER),
                        THud::Pip(i),
                    ))
                    .with_children(|b| {
                        txt(b, TERM_LETTERS[i as usize], 14.0, C_TEXT);
                    });
                }
            });
            hud_text(c, 15.0, C_TEXT, THud::Status);
        });
        // kill feed (top right, under the augmentation icons)
        r.spawn(Node { position_type: PositionType::Absolute, right: Val::Px(24.0), top: Val::Px(72.0), flex_direction: FlexDirection::Column, align_items: AlignItems::FlexEnd, ..default() })
            .with_children(|c| hud_text(c, 14.0, C_TEXT, THud::Feed));
        // banner
        r.spawn((
            Node { position_type: PositionType::Absolute, top: Val::Percent(26.0), width: Val::Percent(100.0), flex_direction: FlexDirection::Column, align_items: AlignItems::Center, ..default() },
            Visibility::Hidden,
            THud::BannerBox,
        ))
        .with_children(|c| {
            hud_text(c, 40.0, C_ACCENT, THud::Banner);
            hud_text(c, 17.0, C_TEXT, THud::BannerSub);
        });
        // world markers (pool)
        for i in 0..MARKERS as u8 {
            r.spawn((
                Node { position_type: PositionType::Absolute, width: Val::Px(240.0), justify_content: JustifyContent::Center, left: Val::Px(0.0), top: Val::Px(0.0), ..default() },
                Visibility::Hidden,
                THud::Marker(i),
            ))
            .with_children(|m| hud_text(m, 13.0, C_TEXT, THud::MarkerText(i)));
        }
        // scoreboard (hold P)
        r.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(50.0),
                top: Val::Percent(50.0),
                width: Val::Px(760.0),
                margin: UiRect { left: Val::Px(-380.0), top: Val::Px(-200.0), ..default() },
                padding: UiRect::all(Val::Px(16.0)),
                border: UiRect::all(Val::Px(1.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(4.0),
                ..default()
            },
            BackgroundColor(C_PANEL),
            BorderColor(C_BORDER),
            Visibility::Hidden,
            THud::Board,
        ));
    });
}

fn fmt_clock(t: f32) -> String {
    let t = t.max(0.0) as u32;
    format!("{:02}:{:02}", t / 60, t % 60)
}

#[allow(clippy::too_many_arguments)]
fn update_team_hud(
    time: Res<Time<Real>>,
    mut ms: ResMut<MatchState>,
    players: Res<Players>,
    local: Res<LocalId>,
    me: Res<Me>,
    mut q: Query<(&THud, Option<&mut Text>, Option<&mut Visibility>, Option<&mut BackgroundColor>, Option<&mut BorderColor>, Option<&mut TextColor>)>,
) {
    let now = time.elapsed_secs();
    let my_team = players.team_of(local.0);
    ms.feed.retain(|(_, t)| now - *t < 9.0);
    let life = if ms.net.winner.is_some() { ROUND_OVER_TIME + 1.0 } else { 5.0 };
    let expired = ms.banner.as_ref().map(|(_, _, _, t)| now - *t > life).unwrap_or(false);
    if expired {
        ms.banner = None;
    }
    let carrier = ms.net.carrier;
    let status = if ms.net.winner.is_some() {
        String::new()
    } else if ms.net.objective == OBJ_STEAL {
        if carrier == local.0 {
            "YOU HAVE THE PROTOTYPE - REACH YOUR EXTRACTION ZONE".to_string()
        } else if carrier != NO_PLAYER && players.team_of(carrier) == my_team {
            format!("{} HAS THE PROTOTYPE - ESCORT THEM", players.name_of(carrier).to_uppercase())
        } else if carrier != NO_PLAYER {
            "THE ENEMY HAS THE PROTOTYPE - STOP THEM".to_string()
        } else if ms.known(my_team, 0) {
            if ms.dropped {
                format!("PROTOTYPE DROPPED - returns in {:.0}s", (DROP_RETURN_TIME - ms.drop_t).max(0.0))
            } else {
                "PROTOTYPE LOCATED".to_string()
            }
        } else {
            "PROTOTYPE LOCATION UNKNOWN - SEARCH".to_string()
        }
    } else {
        let known = (1..=3).filter(|w| ms.known(my_team, *w)).count();
        format!("TERMINALS FOUND {known}/3   -   YOURS {}  ENEMY {}", ms.hack_count(my_team), ms.hack_count(other_team(my_team)))
    };
    for (el, text, vis, bg, border, tcol) in q.iter_mut() {
        match *el {
            THud::ScoreOrange => {
                if let Some(mut t) = text {
                    t.0 = format!("ORANGE {}", ms.net.scores[0]);
                }
            }
            THud::ScoreBlue => {
                if let Some(mut t) = text {
                    t.0 = format!("{} BLUE", ms.net.scores[1]);
                }
            }
            THud::Clock => {
                if let Some(mut t) = text {
                    t.0 = format!("ROUND {}   {}", ms.net.round.max(1), fmt_clock(ms.net.time_left));
                }
            }
            THud::Objective => {
                if let Some(mut t) = text {
                    t.0 = format!("{}  //  YOU ARE TEAM {}", ms.objective_name(), team_name(my_team));
                }
                if let Some(mut c) = tcol {
                    c.0 = team_color(my_team);
                }
            }
            THud::Help => {
                if let Some(mut t) = text {
                    t.0 = format!("{}    [P] scoreboard", ms.objective_help());
                }
            }
            THud::Status => {
                if let Some(mut t) = text {
                    t.0 = status.clone();
                }
            }
            THud::PipRow => {
                if let Some(mut v) = vis {
                    *v = if ms.net.objective == OBJ_HACK { Visibility::Inherited } else { Visibility::Hidden };
                }
            }
            THud::Pip(i) => {
                let owner = ms.net.hack_owner[i as usize];
                let known = ms.known(my_team, i as usize + 1);
                if let Some(mut b) = bg {
                    b.0 = match owner {
                        TEAM_ORANGE | TEAM_BLUE => team_color(owner).with_alpha(0.55),
                        _ => Color::srgba(0.0, 0.0, 0.0, 0.5),
                    };
                }
                if let Some(mut b) = border {
                    b.0 = if known { C_ACCENT } else { C_BORDER };
                }
            }
            THud::Feed => {
                if let Some(mut t) = text {
                    t.0 = ms.feed.iter().map(|(s, _)| s.as_str()).collect::<Vec<_>>().join("\n");
                }
            }
            THud::BannerBox => {
                if let Some(mut v) = vis {
                    *v = if ms.banner.is_some() { Visibility::Inherited } else { Visibility::Hidden };
                }
            }
            THud::Banner => {
                if let (Some(mut t), Some((s, _, c, _))) = (text, &ms.banner) {
                    t.0 = s.clone();
                    if let Some(mut tc) = tcol {
                        tc.0 = *c;
                    }
                }
            }
            THud::BannerSub => {
                if let (Some(mut t), Some((_, s, _, _))) = (text, &ms.banner) {
                    t.0 = if ms.net.winner.is_some() {
                        format!("ORANGE {}  -  {} BLUE      next round in {:.0}s", ms.net.scores[0], ms.net.scores[1], (ROUND_OVER_TIME - ms.over_t).max(0.0))
                    } else {
                        s.clone()
                    };
                }
            }
            _ => {}
        }
    }
    let _ = me;
}

/// Screen position of a world point. Points off screen are pinned to the screen edge.
fn project(cam: &Camera, gt: &GlobalTransform, p: Vec3, size: Vec2) -> (Vec2, bool) {
    if let Ok(v) = cam.world_to_viewport(gt, p) {
        if v.x >= 0.0 && v.y >= 0.0 && v.x <= size.x && v.y <= size.y {
            return (v, true);
        }
    }
    let view = gt.compute_matrix().inverse().transform_point3(p);
    let mut d = Vec2::new(view.x, -view.y);
    if d.length_squared() < 1e-6 {
        d = Vec2::Y;
    }
    let d = d.normalize();
    let c = size * 0.5;
    let r = (size * 0.5 - Vec2::new(130.0, 40.0)).max(Vec2::splat(10.0));
    let k = (r.x / d.x.abs().max(1e-4)).min(r.y / d.y.abs().max(1e-4));
    (c + d * k, false)
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn update_markers(
    ms: Res<MatchState>,
    level: Res<Level>,
    players: Res<Players>,
    local: Res<LocalId>,
    me: Res<Me>,
    ui: Res<UiMode>,
    cam: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    avatars: Query<(&crate::avatars::Avatar, &Transform)>,
    mut nodes: Query<(&THud, &mut Node, &mut Visibility), Without<Text>>,
    mut texts: Query<(&THud, &mut Text, &mut TextColor)>,
) {
    let Ok((camera, gt)) = cam.single() else { return };
    let Some(size) = camera.logical_viewport_size() else { return };
    let my_team = players.team_of(local.0);
    let eye = me.eye();
    // (world position, label, colour, keep on screen edge)
    let mut list: Vec<(Vec3, String, Color, bool)> = vec![];
    if *ui == UiMode::None && my_team < 2 {
        if ms.net.objective == OBJ_STEAL {
            let carrier = ms.net.carrier;
            if carrier == local.0 {
                let base = level.base_center(my_team);
                list.push((base + Vec3::Y * 2.0, format!("EXTRACT  {:.0}m", base.distance(eye)), team_color(my_team), true));
            } else if carrier == NO_PLAYER {
                if ms.known(my_team, 0) {
                    let p = proto_ground_pos(&ms, &level) + Vec3::Y * 0.6;
                    list.push((p, format!("PROTOTYPE  {:.0}m", p.distance(eye)), C_ACCENT, true));
                }
            } else if players.team_of(carrier) == my_team {
                if let Some((_, t)) = avatars.iter().find(|(a, _)| a.id == carrier) {
                    let p = t.translation + Vec3::Y * 2.3;
                    list.push((p, format!("ESCORT {}", players.name_of(carrier)), C_ACCENT, true));
                }
            }
        } else {
            for i in 0..3usize {
                if !ms.known(my_team, i + 1) {
                    continue;
                }
                let p = level.hack_pos(ms.net.hack_spots[i]) + Vec3::Y * 1.6;
                let owner = ms.net.hack_owner[i];
                let tag = match owner {
                    o if o == my_team => "  (ours)",
                    NO_TEAM => "",
                    _ => "  (enemy)",
                };
                let color = if owner < 2 { team_color(owner) } else { C_ACCENT };
                list.push((p, format!("TERMINAL {}  {:.0}m{tag}", TERM_LETTERS[i], p.distance(eye)), color, true));
            }
        }
        // teammates
        for (a, t) in avatars.iter() {
            let Some(p) = players.map.get(&a.id) else { continue };
            if a.id == local.0 || p.team != my_team || p.net.has(pflags::DOWNED) {
                continue;
            }
            let pos = t.translation + Vec3::Y * 2.05;
            if pos.distance(eye) > 70.0 {
                continue;
            }
            list.push((pos, p.name.clone(), team_color(my_team), false));
        }
    }
    list.truncate(MARKERS);
    let mut placed: Vec<Option<(Vec2, String, Color)>> = vec![None; MARKERS];
    for (i, (p, label, color, pin)) in list.into_iter().enumerate() {
        let (v, on) = project(camera, gt, p, size);
        if on || pin {
            let label = if on { label } else { format!("> {label} <") };
            placed[i] = Some((v, label, color));
        }
    }
    for (el, mut node, mut vis) in nodes.iter_mut() {
        let THud::Marker(i) = *el else { continue };
        match &placed[i as usize] {
            Some((v, _, _)) => {
                node.left = Val::Px(v.x - 120.0);
                node.top = Val::Px(v.y - 10.0);
                *vis = Visibility::Inherited;
            }
            None => *vis = Visibility::Hidden,
        }
    }
    for (el, mut text, mut color) in texts.iter_mut() {
        let THud::MarkerText(i) = *el else { continue };
        if let Some((_, label, c)) = &placed[i as usize] {
            if text.0 != *label {
                text.0 = label.clone();
            }
            color.0 = *c;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn update_scoreboard(
    mut commands: Commands,
    time: Res<Time<Real>>,
    keys: Res<ButtonInput<KeyCode>>,
    ms: Res<MatchState>,
    players: Res<Players>,
    local: Res<LocalId>,
    mut board: Query<(Entity, &THud, &mut Visibility, Option<&Children>)>,
    mut acc: Local<f32>,
) {
    let show = keys.pressed(KeyCode::KeyP) || ms.net.winner.is_some();
    *acc -= time.delta_secs();
    for (e, el, mut vis, children) in board.iter_mut() {
        if *el != THud::Board {
            continue;
        }
        if !show {
            *vis = Visibility::Hidden;
            *acc = 0.0;
            continue;
        }
        *vis = Visibility::Inherited;
        if *acc > 0.0 {
            continue;
        }
        *acc = 0.5;
        if let Some(ch) = children {
            for c in ch.iter() {
                commands.entity(c).despawn();
            }
        }
        commands.entity(e).with_children(|b| {
            txt(b, format!("ROUND {}  //  {}", ms.net.round.max(1), ms.objective_name()), 18.0, C_ACCENT);
            b.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(24.0), margin: UiRect::top(Val::Px(8.0)), ..default() }).with_children(|row| {
                for team in [TEAM_ORANGE, TEAM_BLUE] {
                    row.spawn(Node { flex_direction: FlexDirection::Column, width: Val::Px(350.0), row_gap: Val::Px(2.0), ..default() }).with_children(|col| {
                        txt(col, format!("{} TEAM  -  {}", team_name(team), ms.net.scores[team as usize]), 18.0, team_color(team));
                        col.spawn(Node { flex_direction: FlexDirection::Row, ..default() }).with_children(|h| {
                            h.spawn(Node { width: Val::Px(220.0), ..default() }).with_children(|c| {
                                txt(c, "NAME", 12.0, C_DIM);
                            });
                            h.spawn(Node { width: Val::Px(60.0), ..default() }).with_children(|c| {
                                txt(c, "KILLS", 12.0, C_DIM);
                            });
                            h.spawn(Node { width: Val::Px(60.0), ..default() }).with_children(|c| {
                                txt(c, "DEATHS", 12.0, C_DIM);
                            });
                        });
                        let mut list: Vec<(&u8, &PlayerEntry)> = players.map.iter().filter(|(_, p)| p.team == team).collect();
                        list.sort_by(|a, b| b.1.kills.cmp(&a.1.kills).then(a.1.deaths.cmp(&b.1.deaths)).then(a.0.cmp(b.0)));
                        for (id, p) in list {
                            let color = if *id == local.0 { C_ACCENT } else { C_TEXT };
                            let name = if p.bot { format!("{}  [BOT]", p.name) } else { p.name.clone() };
                            col.spawn(Node { flex_direction: FlexDirection::Row, ..default() }).with_children(|h| {
                                h.spawn(Node { width: Val::Px(220.0), ..default() }).with_children(|c| {
                                    txt(c, name, 15.0, color);
                                });
                                h.spawn(Node { width: Val::Px(60.0), ..default() }).with_children(|c| {
                                    txt(c, p.kills.to_string(), 15.0, color);
                                });
                                h.spawn(Node { width: Val::Px(60.0), ..default() }).with_children(|c| {
                                    txt(c, p.deaths.to_string(), 15.0, color);
                                });
                            });
                        }
                    });
                }
            });
        });
    }
}
