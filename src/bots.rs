//! Team-match bots. They are simulated by the authority (offline skirmish, or the host of an
//! online match) and fill empty team slots up to `Settings::team_size`.
//!
//! Bots play by the same rules as people: they do not know where the objective is until a
//! teammate has seen it, so they search the candidate spots; they notice enemies based on
//! light, distance, movement and stance (cloaked players are invisible to them); they crawl
//! through vents, open doors and fight. Their actions go through the same `WorldRequest`
//! validation as a human client's.
use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

use crate::audio::*;
use crate::collision::*;
use crate::game::*;
use crate::level::*;
use crate::net::Net;
use crate::npc::find_path_ex;
use crate::teams::*;
use crate::util::*;
use crate::world::*;

const NAMES: [&str; 16] = [
    "Raven", "Vector", "Mosaic", "Cipher", "Lynx", "Halcyon", "Spectre", "Nomad", "Kestrel", "Onyx", "Wraith", "Talon", "Echo", "Saber", "Quill", "Drift",
];
const WALK: f32 = 3.4;
const RUN: f32 = 5.2;
const CRAWL: f32 = 1.9;
const BOT_HP: f32 = 100.0;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Task {
    Idle,
    Explore(Vec3),
    Take,
    Carry,
    Escort(u8),
    Hunt,
    Hack(u8),
    Defend(Vec3),
    Investigate(Vec3),
}

pub struct Bot {
    pub id: u8,
    pub team: u8,
    pub pos: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub hp: f32,
    pub dead: bool,
    pub dead_t: f32,
    pub task: Task,
    pub task_t: f32,
    pub path: Vec<Vec3>,
    pub path_i: usize,
    pub goal: Option<Vec3>,
    pub repath_t: f32,
    pub target: Option<u8>,
    pub aware: f32,
    pub last_seen: Vec3,
    pub lost_t: f32,
    pub fire_cd: f32,
    pub burst: u8,
    pub rifle: bool,
    pub hack_t: f32,
    pub think_t: f32,
    pub crouch: bool,
    pub speed: f32,
    pub sprint: bool,
    /// accuracy multiplier, so bots are not all identical
    pub skill: f32,
    pub stuck_t: f32,
    pub stuck_pos: Vec3,
}

impl Bot {
    fn new(id: u8, team: u8, pos: Vec3, yaw: f32, rng: &mut Rng) -> Bot {
        Bot {
            id,
            team,
            pos,
            yaw,
            pitch: 0.0,
            hp: BOT_HP,
            dead: false,
            dead_t: 0.0,
            task: Task::Idle,
            task_t: 0.0,
            path: vec![],
            path_i: 0,
            goal: None,
            repath_t: 0.0,
            target: None,
            aware: 0.0,
            last_seen: pos,
            lost_t: 0.0,
            fire_cd: 0.0,
            burst: 0,
            rifle: rng.chance(0.35),
            hack_t: 0.0,
            think_t: rng.range(0.0, 0.5),
            crouch: false,
            speed: 0.0,
            sprint: false,
            skill: rng.range(0.75, 1.05),
            stuck_t: 0.0,
            stuck_pos: pos,
        }
    }

    fn respawn(&mut self, pos: Vec3, yaw: f32) {
        self.pos = pos;
        self.yaw = yaw;
        self.pitch = 0.0;
        self.hp = BOT_HP;
        self.dead = false;
        self.dead_t = 0.0;
        self.task = Task::Idle;
        self.task_t = 0.0;
        self.path.clear();
        self.path_i = 0;
        self.goal = None;
        self.target = None;
        self.aware = 0.0;
        self.think_t = 0.0;
        self.hack_t = 0.0;
        self.crouch = false;
        self.speed = 0.0;
        self.stuck_pos = pos;
        self.stuck_t = 0.0;
    }

    fn net(&self, carrier: u8) -> PlayerNet {
        let mut flags = 0u8;
        if self.crouch {
            flags |= pflags::CROUCH;
        }
        if self.speed > 0.5 {
            flags |= pflags::MOVING;
        }
        if self.sprint && self.speed > 0.5 {
            flags |= pflags::SPRINT;
        }
        if self.dead {
            flags |= pflags::DOWNED;
        }
        if carrier == self.id {
            flags |= pflags::CORE;
        }
        PlayerNet { pos: self.pos.to_array(), yaw: self.yaw, pitch: self.pitch, flags, weapon: if self.rifle { 3 } else { 1 }, health: self.hp.clamp(0.0, 255.0) as u8 }
    }
}

#[derive(Resource, Default)]
pub struct Bots {
    pub list: Vec<Bot>,
    /// per team: candidate spots already searched this round
    pub checked: [HashSet<(i32, i32)>; 2],
    /// per team: where the enemy prototype carrier was last seen (position, age)
    pub carrier_seen: [Option<(Vec3, f32)>; 2],
    /// sounds bots can hear this frame: (position, radius, team of the source)
    pub noises: Vec<(Vec3, f32, u8)>,
}

pub struct BotsPlugin;

impl Plugin for BotsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Bots>()
            .add_systems(OnEnter(AppState::InGame), reset_bots.run_if(teams_mode).run_if(is_authority))
            .add_systems(
                Update,
                (bot_roster, compute_visibility, bot_hear, bot_think)
                    .chain()
                    .after(TeamAuthority)
                    .in_set(GameSet::Authority)
                    .run_if(is_authority)
                    .run_if(in_game)
                    .run_if(teams_mode),
            )
            .add_systems(Update, bot_damage.in_set(GameSet::Apply).run_if(is_authority).run_if(teams_mode));
    }
}

fn reset_bots(mut bots: ResMut<Bots>, level: Res<Level>, mut rng: ResMut<GameRng>) {
    bots.checked = [HashSet::new(), HashSet::new()];
    bots.carrier_seen = [None, None];
    bots.noises.clear();
    for b in bots.list.iter_mut() {
        let (p, yaw) = spawn_point(&level, b.team, &mut rng.0);
        b.respawn(p, yaw);
    }
}

/// Keep both teams at `team_size` players by adding / removing bots.
fn bot_roster(mut bots: ResMut<Bots>, mut players: ResMut<Players>, settings: Res<Settings>, level: Res<Level>, mut net: ResMut<Net>, mut rng: ResMut<GameRng>) {
    let size = settings.team_size.clamp(1, 6) as usize;
    for team in [TEAM_ORANGE, TEAM_BLUE] {
        let (humans, nbots) = players.team_counts(team);
        let want = size.saturating_sub(humans);
        if nbots > want {
            let mut ids: Vec<u8> = players.map.iter().filter(|(_, p)| p.bot && p.team == team).map(|(id, _)| *id).collect();
            ids.sort_unstable();
            for id in ids.into_iter().rev().take(nbots - want) {
                players.map.remove(&id);
                bots.list.retain(|b| b.id != id);
                net.broadcast(&S2C::Left(id));
            }
        } else if nbots < want {
            for _ in nbots..want {
                if players.map.len() >= MAX_PLAYERS {
                    break;
                }
                let Some(id) = (BOT_ID_BASE..BOT_ID_BASE + 40).find(|i| !players.map.contains_key(i)) else { break };
                let name = format!("{} [{}]", NAMES[(id - BOT_ID_BASE) as usize % NAMES.len()], if team == TEAM_ORANGE { "O" } else { "B" });
                let entry = PlayerEntry::new(name, team, true);
                let info = entry.info(id);
                players.map.insert(id, entry);
                let (p, yaw) = spawn_point(&level, team, &mut rng.0);
                bots.list.push(Bot::new(id, team, p, yaw, &mut rng.0));
                net.broadcast(&S2C::Joined(info));
            }
        }
    }
}

/// Light level at every player the local game does not control (bots and remote humans),
/// so bots can judge how visible they are.
fn compute_visibility(time: Res<Time>, mut acc: Local<f32>, mut players: ResMut<Players>, local: Res<LocalId>, reg: Res<LightReg>, phys: Res<Physics>, level: Res<Level>) {
    *acc += time.delta_secs();
    if *acc < 0.15 {
        return;
    }
    *acc = 0.0;
    for (id, p) in players.map.iter_mut() {
        if *id == local.0 {
            continue;
        }
        let chest = p.net.pos() + Vec3::Y * if p.net.has(pflags::CROUCH) { 0.7 } else { 1.1 };
        let mut v = light_level(chest, &reg, &phys, !level.indoor_at(chest));
        if p.net.has(pflags::LIGHT) {
            v = (v + 0.45).min(1.0);
        }
        p.visibility += (v - p.visibility) * 0.6;
    }
}

fn bot_hear(mut reqs: EventReader<AuthRequest>, mut bots: ResMut<Bots>, players: Res<Players>) {
    for r in reqs.read() {
        if let WorldRequest::Noise { pos, radius } = &r.req {
            bots.noises.push((Vec3::from_array(*pos), *radius, players.team_of(r.from)));
        }
    }
}

fn bot_damage(mut evs: EventReader<PlayerDamage>, mut bots: ResMut<Bots>, players: Res<Players>, mut auth: EventWriter<AuthRequest>) {
    for d in evs.read() {
        let Some(b) = bots.list.iter_mut().find(|b| b.id == d.target) else { continue };
        if b.dead {
            continue;
        }
        b.hp -= d.amount;
        if d.by != NO_PLAYER && players.team_of(d.by) != b.team {
            b.target = Some(d.by);
            b.aware = b.aware.max(1.0);
            b.last_seen = players.map.get(&d.by).map(|p| p.net.pos()).unwrap_or(d.from);
            b.lost_t = 0.0;
        }
        if b.hp <= 0.0 {
            b.hp = 0.0;
            b.dead = true;
            b.dead_t = 0.0;
            b.speed = 0.0;
            auth.write(AuthRequest { from: b.id, req: WorldRequest::Died { killer: d.by, weapon: d.weapon } });
        }
    }
}

// ------------------------------------------------------------------ brain

struct Other {
    id: u8,
    team: u8,
    pos: Vec3,
    net: PlayerNet,
    vis: f32,
}

struct Seen {
    id: u8,
    pos: Vec3,
    chest: Vec3,
    vis: f32,
    moving: bool,
    crouch: bool,
    carrier: bool,
    dist: f32,
    rate: f32,
}

struct DoorSnap {
    open: bool,
    t: f32,
}

struct Ctx<'a> {
    level: &'a Level,
    phys: &'a Physics,
    ms: &'a MatchState,
    others: Vec<Other>,
    objectives: Vec<(usize, Vec3)>,
    doors: HashMap<u16, DoorSnap>,
    closed: HashSet<(i32, i32)>,
    claims: Vec<(u8, Vec3)>,
    /// (bot, team, terminal) currently hacking / heading to a terminal
    hackers: Vec<(u8, u8, u8)>,
    checked: &'a mut [HashSet<(i32, i32)>; 2],
    carrier_seen: &'a mut [Option<(Vec3, f32)>; 2],
    noises: Vec<(Vec3, f32, u8)>,
    rng: &'a mut Rng,
    auth: Vec<AuthRequest>,
    fx: Vec<Fx>,
    snd: Vec<SoundEv>,
    open_doors: Vec<u16>,
    keep_open: Vec<u16>,
    new_noise: Vec<(Vec3, f32, u8)>,
}

impl Ctx<'_> {
    fn team_of(&self, id: u8) -> u8 {
        self.others.iter().find(|o| o.id == id).map(|o| o.team).unwrap_or(NO_TEAM)
    }
    fn pos_of(&self, id: u8) -> Option<Vec3> {
        self.others.iter().find(|o| o.id == id).map(|o| o.pos)
    }
}

#[allow(clippy::too_many_arguments)]
fn bot_think(
    time: Res<Time>,
    mut bots: ResMut<Bots>,
    mut players: ResMut<Players>,
    ms: Res<MatchState>,
    level: Res<Level>,
    phys: Res<Physics>,
    mut doors: Query<&mut Door>,
    mut rng: ResMut<GameRng>,
    mut ev: (EventWriter<AuthRequest>, EventWriter<WorldEv>, EventWriter<FxEv>, EventWriter<SoundEv>),
) {
    let dt = time.delta_secs().min(0.05);
    let others: Vec<Other> = players.map.iter().map(|(id, p)| Other { id: *id, team: p.team, pos: p.net.pos(), net: p.net, vis: p.visibility }).collect();
    let mut door_snap = HashMap::new();
    let mut closed = HashSet::new();
    for d in doors.iter() {
        door_snap.insert(d.id, DoorSnap { open: d.open, t: d.t });
        if d.locked {
            if let Some(def) = level.doors.get(d.id as usize) {
                closed.insert((def.x, def.z));
            }
        }
    }
    let Bots { list, checked, carrier_seen, noises } = &mut *bots;
    for s in carrier_seen.iter_mut().flatten() {
        s.1 += dt;
    }
    let claims: Vec<(u8, Vec3)> = list
        .iter()
        .filter_map(|b| match b.task {
            Task::Explore(p) => Some((b.id, p)),
            _ => None,
        })
        .collect();
    let hackers: Vec<(u8, u8, u8)> = list
        .iter()
        .filter_map(|b| match b.task {
            Task::Hack(i) => Some((b.id, b.team, i)),
            _ => None,
        })
        .collect();
    let mut c = Ctx {
        level: &level,
        phys: &phys,
        ms: &ms,
        others,
        objectives: objective_points(&ms, &level),
        doors: door_snap,
        closed,
        claims,
        hackers,
        checked,
        carrier_seen,
        noises: std::mem::take(noises),
        rng: &mut rng.0,
        auth: vec![],
        fx: vec![],
        snd: vec![],
        open_doors: vec![],
        keep_open: vec![],
        new_noise: vec![],
    };
    for b in list.iter_mut() {
        tick_bot(b, &mut c, dt);
    }
    // publish bot state
    for b in list.iter() {
        if let Some(p) = players.map.get_mut(&b.id) {
            p.net = b.net(ms.net.carrier);
        }
    }
    let (auth, wev, fx, snd) = (&mut ev.0, &mut ev.1, &mut ev.2, &mut ev.3);
    for a in c.auth.drain(..) {
        auth.write(a);
    }
    for f in c.fx.drain(..) {
        fx.write(FxEv { fx: f, remote: false });
    }
    for s in c.snd.drain(..) {
        snd.write(s);
    }
    c.open_doors.sort_unstable();
    c.open_doors.dedup();
    for mut d in doors.iter_mut() {
        if c.open_doors.contains(&d.id) && !d.open && !d.locked {
            wev.write(WorldEv { ev: WorldEvent::Door { id: d.id, open: true, locked: false }, quiet: false });
            d.npc_timer = 4.0;
        }
        if c.keep_open.contains(&d.id) && d.open {
            d.npc_timer = d.npc_timer.max(3.0);
        }
    }
    let new_noise = std::mem::take(&mut c.new_noise);
    drop(c);
    bots.noises = new_noise;
}

fn tick_bot(b: &mut Bot, c: &mut Ctx, dt: f32) {
    b.fire_cd -= dt;
    b.think_t -= dt;
    b.repath_t -= dt;
    b.task_t += dt;
    if b.dead {
        b.dead_t += dt;
        b.speed = 0.0;
        if b.dead_t > RESPAWN_TIME {
            let (p, yaw) = spawn_point(c.level, b.team, c.rng);
            b.respawn(p, yaw);
        }
        return;
    }
    let eye = b.pos + Vec3::Y * if b.crouch { 0.95 } else { 1.6 };
    let fwd = yaw_dir(b.yaw);
    let ti = (b.team as usize).min(1);

    // ---- perception
    let mut best: Option<Seen> = None;
    for o in c.others.iter() {
        if o.id == b.id || o.team == b.team || o.team >= 2 || o.net.has(pflags::DOWNED) || o.net.has(pflags::CLOAK) {
            continue;
        }
        let crouch = o.net.has(pflags::CROUCH);
        let chest = o.pos + Vec3::Y * if crouch { 0.75 } else { 1.2 };
        let to = chest - eye;
        let dist = to.length();
        if dist > 36.0 {
            continue;
        }
        let flat = to.with_y(0.0).normalize_or_zero();
        let fov = if b.target == Some(o.id) && b.aware > 0.5 { 0.0 } else { 0.3 };
        if flat.dot(fwd) < fov && dist > 2.5 {
            continue;
        }
        if !c.phys.los(eye, chest) && !c.phys.los(eye, o.pos + Vec3::Y * if crouch { 1.0 } else { 1.65 }) {
            continue;
        }
        let mut vf = (o.vis * 1.6).clamp(0.04, 1.0);
        if dist < 3.0 {
            vf = vf.max(0.8);
        }
        let df = (1.3 - dist / 26.0).clamp(0.12, 1.0);
        let mv = if o.net.has(pflags::SPRINT) { 1.6 } else if o.net.has(pflags::MOVING) { 1.25 } else { 1.0 };
        let cr = if crouch { 0.7 } else { 1.0 };
        let k = if b.target == Some(o.id) { 3.0 } else { 1.0 };
        let rate = 1.8 * vf * df * mv * cr * k;
        if best.as_ref().map(|s| rate > s.rate).unwrap_or(true) {
            best = Some(Seen { id: o.id, pos: o.pos, chest, vis: o.vis, moving: o.net.has(pflags::MOVING), crouch, carrier: o.net.has(pflags::CORE), dist, rate });
        }
    }
    match &best {
        Some(s) => {
            if s.carrier {
                c.carrier_seen[ti] = Some((s.pos, 0.0));
            }
            if b.target != Some(s.id) && b.aware < 0.5 {
                b.target = Some(s.id);
            }
            if b.target == Some(s.id) {
                b.aware = (b.aware + s.rate * dt).min(1.5);
                b.last_seen = s.pos;
                b.lost_t = 0.0;
            }
        }
        None => {
            b.lost_t += dt;
            b.aware = (b.aware - 0.2 * dt).max(0.0);
            if b.lost_t > 12.0 {
                b.target = None;
            }
        }
    }

    // ---- objective discovery (same rule as for people: see it from close enough)
    for (what, p) in c.objectives.iter() {
        if !c.ms.known(b.team, *what) && eye.distance(*p) < DISCOVER_RANGE && c.phys.los(eye, *p) {
            c.auth.push(AuthRequest { from: b.id, req: WorldRequest::Discover { what: *what as u8 } });
        }
    }

    // ---- hearing
    let carrying = c.ms.net.carrier == b.id;
    if b.aware < 1.0 && !carrying && !matches!(b.task, Task::Hack(_)) {
        for (p, r, team) in c.noises.iter() {
            if *team != b.team && p.distance(b.pos) < *r {
                b.task = Task::Investigate(*p);
                b.task_t = 0.0;
                b.think_t = 6.0;
            }
        }
    }

    let mut move_to: Option<(Vec3, f32)> = None;
    let mut face: Option<Vec3> = None;
    b.sprint = false;

    // ---- combat
    let engaged = best.as_ref().filter(|s| b.aware >= 1.0 && Some(s.id) == b.target);
    if let Some(s) = engaged {
        face = Some(s.chest);
        shoot(b, s, c, eye);
        if s.dist > 15.0 && !carrying {
            move_to = Some((s.pos, RUN));
            b.sprint = true;
        }
        b.hack_t = 0.0;
    } else if b.aware >= 1.0 && b.lost_t < 8.0 && !carrying && b.target.is_some() {
        // lost sight: go where we last saw them
        move_to = Some((b.last_seen, RUN));
        b.sprint = true;
    }

    // ---- objectives
    if move_to.is_none() && face.is_none() {
        if b.think_t <= 0.0 {
            b.think_t = 0.7;
            let t = choose_task(b, c);
            if t != b.task {
                b.task = t;
                b.task_t = 0.0;
                b.hack_t = 0.0;
            }
        }
        match b.task {
            Task::Idle => {
                if b.task_t > 3.0 {
                    b.think_t = 0.0;
                }
            }
            Task::Explore(p) => {
                let close = b.pos.distance(p) < 3.0 || (b.pos.distance(p) < 8.0 && c.phys.los(eye, p + Vec3::Y));
                if close {
                    c.checked[ti].insert(Level::cell_of(p));
                    b.think_t = 0.0;
                } else {
                    move_to = Some((p, WALK));
                }
            }
            Task::Take => {
                let p = proto_ground_pos(c.ms, c.level);
                if b.pos.with_y(0.0).distance(p.with_y(0.0)) < 1.6 {
                    if b.task_t > 0.3 {
                        c.auth.push(AuthRequest { from: b.id, req: WorldRequest::TakeObjective });
                        b.task_t = 0.0;
                    }
                } else {
                    move_to = Some((p.with_y(0.0), RUN));
                    b.sprint = true;
                }
            }
            Task::Carry => {
                move_to = Some((c.level.base_center(b.team), RUN));
                b.sprint = true;
            }
            Task::Escort(id) => {
                if let Some(p) = c.pos_of(id) {
                    if b.pos.distance(p) > 4.0 {
                        move_to = Some((p, RUN));
                    }
                }
            }
            Task::Hunt => {
                let goal = match c.carrier_seen[ti] {
                    Some((p, age)) if age < 20.0 => p,
                    _ => c.level.base_center(other_team(b.team)),
                };
                if b.pos.distance(goal) > 3.0 {
                    move_to = Some((goal, RUN));
                    b.sprint = true;
                } else {
                    b.think_t = b.think_t.min(0.2);
                }
            }
            Task::Hack(i) => {
                let spot = c.ms.net.hack_spots[(i as usize).min(2)];
                let tp = c.level.hack_pos(spot);
                let owner = c.ms.net.hack_owner[(i as usize).min(2)];
                if owner == b.team {
                    b.think_t = 0.0;
                } else if b.pos.with_y(0.0).distance(tp) < 1.9 {
                    face = Some(tp + Vec3::Y);
                    b.hack_t += dt;
                    if b.hack_t >= hack_time(owner, 1) {
                        c.auth.push(AuthRequest { from: b.id, req: WorldRequest::HackObjective { idx: i } });
                        b.hack_t = 0.0;
                        b.think_t = 0.5;
                    }
                } else {
                    b.hack_t = 0.0;
                    move_to = Some((tp, WALK));
                }
            }
            Task::Defend(p) => {
                if b.pos.distance(p) > 4.0 {
                    move_to = Some((p, WALK));
                } else {
                    b.yaw += dt * 0.6;
                }
            }
            Task::Investigate(p) => {
                if b.pos.distance(p) > 1.5 && b.task_t < 15.0 {
                    move_to = Some((p, WALK));
                } else {
                    b.yaw += dt * 1.2;
                    if b.task_t > 5.0 {
                        b.think_t = 0.0;
                        b.task = Task::Idle;
                    }
                }
            }
        }
    }

    // ---- movement
    let tile = c.level.tile_at(b.pos);
    let in_vent = matches!(tile, Tile::Vent | Tile::Grate);
    b.crouch = in_vent;
    b.speed = 0.0;
    if let Some((goal, mut speed)) = move_to {
        if in_vent {
            speed = speed.min(CRAWL);
            b.sprint = false;
        }
        walk(b, c, goal, speed, dt, face.is_some());
    } else {
        b.path.clear();
        b.goal = None;
    }
    // stuck detection: try a fresh path
    b.stuck_t += dt;
    if b.stuck_t > 1.5 {
        if move_to.is_some() && b.pos.distance(b.stuck_pos) < 0.3 {
            b.repath_t = 0.0;
            b.path.clear();
            b.yaw += c.rng.range(-1.0, 1.0);
        }
        b.stuck_t = 0.0;
        b.stuck_pos = b.pos;
    }
    if let Some(f) = face {
        let d = f - eye;
        if d.with_y(0.0).length() > 0.1 {
            b.yaw = approach_angle(b.yaw, dir_yaw(d), dt * 7.0);
            b.pitch = d.y.atan2(d.with_y(0.0).length()).clamp(-1.2, 1.2);
        }
    } else {
        b.pitch *= 1.0 - (dt * 4.0).min(1.0);
    }
}

fn choose_task(b: &Bot, c: &Ctx) -> Task {
    let ti = (b.team as usize).min(1);
    if c.ms.net.winner.is_some() {
        return Task::Idle;
    }
    if c.ms.net.objective == OBJ_STEAL {
        let carrier = c.ms.net.carrier;
        if carrier == b.id {
            return Task::Carry;
        }
        if carrier != NO_PLAYER {
            return if c.team_of(carrier) == b.team { Task::Escort(carrier) } else { Task::Hunt };
        }
        if c.ms.known(b.team, 0) {
            return Task::Take;
        }
        let spots: Vec<Vec3> = c.level.steal_spots.iter().map(|&(x, z)| Level::center(x, z)).collect();
        return explore(b, c, &spots).unwrap_or(Task::Idle);
    }
    // hack: nearest known terminal we do not hold (spread out: avoid ones teammates work on)
    let mut best: Option<(f32, u8)> = None;
    for i in 0..3usize {
        if c.ms.known(b.team, i + 1) && c.ms.net.hack_owner[i] != b.team {
            let d = b.pos.distance(c.level.hack_pos(c.ms.net.hack_spots[i]));
            let busy = c.hackers.iter().any(|(id, team, idx)| *id != b.id && *team == b.team && *idx as usize == i);
            let d = if busy { d * 1.6 + 10.0 } else { d };
            if best.map(|x| d < x.0).unwrap_or(true) {
                best = Some((d, i as u8));
            }
        }
    }
    if let Some((_, i)) = best {
        return Task::Hack(i);
    }
    let spots: Vec<Vec3> = c.level.hack_spots.iter().map(|&(x, z, _)| Level::center(x, z)).collect();
    if let Some(t) = explore(b, c, &spots) {
        return t;
    }
    // everything found and nothing left to take: guard one of ours
    let ours: Vec<Vec3> = (0..3).filter(|i| c.ms.net.hack_owner[*i] == b.team).map(|i| c.level.hack_pos(c.ms.net.hack_spots[i])).collect();
    if !ours.is_empty() {
        let p = ours[b.id as usize % ours.len()];
        return Task::Defend(p);
    }
    let _ = ti;
    Task::Idle
}


/// Nearest candidate spot the team has not searched yet and no teammate is heading to.
fn explore(b: &Bot, c: &Ctx, spots: &[Vec3]) -> Option<Task> {
    let ti = (b.team as usize).min(1);
    let mut best: Option<(f32, Vec3)> = None;
    for p in spots {
        if c.checked[ti].contains(&Level::cell_of(*p)) {
            continue;
        }
        let claimed = c.claims.iter().any(|(id, cp)| *id != b.id && cp.distance(*p) < 0.5 && c.team_of(*id) == b.team);
        let d = b.pos.distance(*p) * if claimed { 2.5 } else { 1.0 };
        if best.map(|x| d < x.0).unwrap_or(true) {
            best = Some((d, *p));
        }
    }
    best.map(|(_, p)| Task::Explore(p))
}

fn walk(b: &mut Bot, c: &mut Ctx, goal: Vec3, speed: f32, dt: f32, facing_target: bool) {
    let need = b.goal.map(|g| g.distance(goal) > 1.5).unwrap_or(true) || b.path_i >= b.path.len() || b.repath_t <= 0.0;
    if need {
        b.path = find_path_ex(c.level, &c.closed, b.pos, goal, true).unwrap_or_default();
        b.path_i = 0;
        b.goal = Some(goal);
        b.repath_t = 2.0 + c.rng.f32() * 1.5;
    }
    if b.path_i >= b.path.len() {
        return;
    }
    let wp = b.path[b.path_i];
    let to = (wp - b.pos).with_y(0.0);
    let d = to.length();
    if d < 0.25 {
        b.path_i += 1;
        return;
    }
    let dir = to / d;
    // open doors on the way, wait until they are out of the way
    let mut blocked = false;
    let ahead = b.pos + dir * 1.2;
    for cell in [Level::cell_of(ahead), Level::cell_of(b.pos)] {
        let Some(def) = c.level.door_at(cell.0, cell.1) else { continue };
        let Some(st) = c.doors.get(&def.id) else { continue };
        if !st.open {
            c.open_doors.push(def.id);
            blocked = true;
        } else if st.t < 0.6 {
            blocked = true;
        } else {
            c.keep_open.push(def.id);
        }
    }
    if blocked {
        return;
    }
    let step = (speed * dt).min(d);
    b.pos += dir * step;
    b.pos.y = 0.0;
    b.speed = speed;
    if !facing_target {
        b.yaw = approach_angle(b.yaw, dir_yaw(dir), dt * 7.0);
    }
}

fn shoot(b: &mut Bot, s: &Seen, c: &mut Ctx, eye: Vec3) {
    let to = (s.chest - eye).normalize_or_zero();
    let facing = yaw_dir(b.yaw).dot(to.with_y(0.0).normalize_or_zero()) > 0.97;
    if !facing || b.fire_cd > 0.0 {
        return;
    }
    let right = Vec3::new(b.yaw.cos(), 0.0, -b.yaw.sin());
    let muzzle = eye - Vec3::Y * 0.2 + yaw_dir(b.yaw) * 0.5 + right * 0.18;
    let mut acc = (0.74 - s.dist * 0.012) * (0.45 + 0.55 * (s.vis * 1.6).min(1.0)) * b.skill;
    if s.moving {
        acc *= 0.8;
    }
    if s.crouch {
        acc *= 0.9;
    }
    if b.speed > 0.5 {
        acc *= 0.85;
    }
    let acc = acc.clamp(0.08, 0.8);
    let weapon = if b.rifle { 3 } else { 1 };
    let hit = c.rng.chance(acc);
    let (end, imp) = if hit {
        (s.chest + c.rng.unit_sphere() * 0.15, impact::FLESH)
    } else {
        let aim = (s.chest + c.rng.unit_sphere() * 0.9 - muzzle).normalize_or_zero();
        match c.phys.raycast(muzzle, aim, 60.0, false, false, None) {
            Some(h) => (h.pos, impact::WALL),
            None => (muzzle + aim * 60.0, impact::NONE),
        }
    };
    if hit {
        let head = c.rng.chance(0.1);
        c.auth.push(AuthRequest {
            from: b.id,
            req: WorldRequest::HitPlayer { target: s.id, dmg: if b.rifle { 13.0 } else { 20.0 }, kind: DmgKind::Bullet, dir: to.to_array(), head, weapon },
        });
    }
    c.fx.push(Fx::Shot { from: muzzle.to_array(), to: end.to_array(), weapon, impact: imp, normal: [0.0, 1.0, 0.0] });
    c.snd.push(SoundEv::at(if b.rifle { Sfx::Rifle } else { Sfx::Pistol }, muzzle, 0.9));
    c.new_noise.push((muzzle, 30.0, b.team));
    if b.rifle {
        b.burst += 1;
        if b.burst >= 3 {
            b.burst = 0;
            b.fire_cd = c.rng.range(0.8, 1.3);
        } else {
            b.fire_cd = 0.11;
        }
    } else {
        b.fire_cd = c.rng.range(0.45, 0.85);
    }
}
