//! Online play: a small host-authoritative layer over TCP (see `wire.rs` for framing).
//!
//! * The host owns the world: it runs the AI, validates every `WorldRequest` and
//!   broadcasts the resulting `WorldEvent`s.
//! * Clients send their own player state (position, stance, weapon...) and requests, and
//!   apply whatever the host broadcasts.
//! * Messages are the `C2S` / `S2C` enums in `game.rs`, encoded with bincode.
use bevy::prelude::*;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::sync::{mpsc, Mutex};
use std::time::Duration;

use crate::game::*;
use crate::level::Level;
use crate::teams::{MatchState, PendingReplay};
use crate::wire::Conn;

/// Seconds without any traffic before a connection is considered lost.
const TIMEOUT: f32 = 12.0;
/// A new connection must say hello within this time.
const HELLO_TIMEOUT: f32 = 6.0;
const CLIENT_SEND_HZ: f32 = 30.0;
const HOST_SEND_HZ: f32 = 20.0;

pub struct Peer {
    pub id: u8,
    pub conn: Conn,
    pub joined: bool,
    pub age: f32,
}

#[derive(Resource, Default)]
pub struct Net {
    listener: Option<TcpListener>,
    pub peers: Vec<Peer>,
    server: Option<Conn>,
    connecting: Option<Mutex<mpsc::Receiver<Result<TcpStream, String>>>>,
    pub connect_t: f32,
    /// client: stop reading messages until the level has been (re)built
    hold: bool,
    welcomed: bool,
    send_acc: f32,
    /// host: stateful world events of the current round, replayed to late joiners
    pub log: Vec<WorldEvent>,
    /// last error / status message, shown in the main menu
    pub status: String,
    /// address we are connecting / connected to
    pub addr: String,
}

fn encode<T: Serialize>(msg: &T) -> Option<Vec<u8>> {
    match bincode::serialize(msg) {
        Ok(b) => Some(b),
        Err(e) => {
            warn!("net: encode failed: {e}");
            None
        }
    }
}

fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Option<T> {
    bincode::deserialize(bytes).ok()
}

impl Net {
    pub fn is_hosting(&self) -> bool {
        self.listener.is_some()
    }

    pub fn host(&mut self, port: u16) -> Result<(), String> {
        self.shutdown();
        let l = TcpListener::bind(("0.0.0.0", port)).map_err(|e| format!("Cannot host on port {port}: {e}"))?;
        l.set_nonblocking(true).map_err(|e| e.to_string())?;
        info!("net: hosting on port {port}");
        self.listener = Some(l);
        self.status.clear();
        Ok(())
    }

    /// Start connecting in the background (DNS + TCP connect can take seconds).
    pub fn connect(&mut self, addr: &str) {
        self.shutdown();
        let mut target = addr.trim().to_string();
        if !target.contains(':') {
            target = format!("{target}:{DEFAULT_PORT}");
        }
        self.addr = target.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let res = (|| -> Result<TcpStream, String> {
                let addrs: Vec<std::net::SocketAddr> = target.to_socket_addrs().map_err(|e| format!("Bad address '{target}': {e}"))?.collect();
                let mut last = format!("No address found for '{target}'");
                for a in addrs {
                    match TcpStream::connect_timeout(&a, Duration::from_secs(5)) {
                        Ok(s) => return Ok(s),
                        Err(e) => last = format!("Could not connect to {a}: {e}"),
                    }
                }
                Err(last)
            })();
            let _ = tx.send(res);
        });
        self.connecting = Some(Mutex::new(rx));
        self.connect_t = 0.0;
        self.status.clear();
    }

    pub fn shutdown(&mut self) {
        for p in self.peers.iter_mut() {
            p.conn.close();
        }
        self.peers.clear();
        self.listener = None;
        if let Some(mut s) = self.server.take() {
            if let Some(b) = encode(&C2S::Bye) {
                s.send_frame(&b);
            }
            s.close();
        }
        self.connecting = None;
        self.hold = false;
        self.welcomed = false;
        self.log.clear();
        self.send_acc = 0.0;
    }

    pub fn to_host(&mut self, msg: &C2S) {
        if let (Some(s), Some(b)) = (self.server.as_mut(), encode(msg)) {
            s.send_frame(&b);
        }
    }

    /// Host: send to every joined client.
    pub fn broadcast(&mut self, msg: &S2C) {
        if self.peers.is_empty() {
            return;
        }
        let Some(b) = encode(msg) else { return };
        for p in self.peers.iter_mut().filter(|p| p.joined) {
            p.conn.send_frame(&b);
        }
    }

    /// Host: send to every joined client except one.
    pub fn broadcast_except(&mut self, skip: u8, msg: &S2C) {
        let Some(b) = encode(msg) else { return };
        for p in self.peers.iter_mut().filter(|p| p.joined && p.id != skip) {
            p.conn.send_frame(&b);
        }
    }

    pub fn send_to(&mut self, id: u8, msg: &S2C) {
        let Some(b) = encode(msg) else { return };
        if let Some(p) = self.peers.iter_mut().find(|p| p.id == id) {
            p.conn.send_frame(&b);
        }
    }

    pub fn is_peer(&self, id: u8) -> bool {
        self.peers.iter().any(|p| p.id == id && p.joined)
    }

    /// Host: remember state-changing events so late joiners can catch up.
    fn push_log(&mut self, ev: &WorldEvent) {
        match ev {
            WorldEvent::Door { id, .. } => self.log.retain(|e| !matches!(e, WorldEvent::Door { id: o, .. } if o == id)),
            WorldEvent::Light { id, .. } => self.log.retain(|e| !matches!(e, WorldEvent::Light { id: o, .. } if o == id)),
            WorldEvent::Alarm { .. } => self.log.retain(|e| !matches!(e, WorldEvent::Alarm { .. })),
            WorldEvent::Security { .. } => self.log.retain(|e| !matches!(e, WorldEvent::Security { .. })),
            _ => {}
        }
        self.log.push(ev.clone());
    }

    /// Called when a level (re)build finished: the client may read messages again.
    fn level_ready(&mut self) {
        self.hold = false;
    }
}

pub struct NetPlugin;

impl Plugin for NetPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Net>()
            .add_systems(OnEnter(AppState::Menu), net_back_to_menu)
            .add_systems(OnEnter(AppState::InGame), |mut net: ResMut<Net>| net.level_ready())
            .add_systems(Update, net_connect_poll.run_if(in_state(AppState::Connecting)))
            .add_systems(Update, (net_host_accept, net_receive).chain().in_set(GameSet::Input))
            .add_systems(Update, net_damage.in_set(GameSet::Apply).run_if(is_authority))
            .add_systems(Update, net_send.in_set(GameSet::Net));
    }
}

fn net_back_to_menu(mut net: ResMut<Net>, mut role: ResMut<NetRole>, mut players: ResMut<Players>, mut bots: ResMut<crate::bots::Bots>) {
    net.shutdown();
    *role = NetRole::Offline;
    players.map.clear();
    bots.list.clear();
}

/// Client: wait for the background connect, then say hello.
#[allow(clippy::too_many_arguments)]
fn net_connect_poll(
    time: Res<Time<Real>>,
    mut net: ResMut<Net>,
    settings: Res<Settings>,
    level: Res<Level>,
    mut role: ResMut<NetRole>,
    mut next: ResMut<NextState<AppState>>,
) {
    net.connect_t += time.delta_secs();
    let result = net.connecting.as_ref().and_then(|m| m.lock().ok().and_then(|rx| rx.try_recv().ok()));
    if let Some(res) = result {
        net.connecting = None;
        match res.and_then(|s| Conn::new(s).map_err(|e| e.to_string())) {
            Ok(conn) => {
                info!("net: connected to {}", conn.peer());
                net.server = Some(conn);
                net.to_host(&C2S::Hello { name: settings.name.clone(), ver: PROTOCOL_VERSION, level: level.hash, team: settings.pref_team });
            }
            Err(e) => {
                warn!("net: {e}");
                net.status = e;
                *role = NetRole::Offline;
                next.set(AppState::Menu);
                return;
            }
        }
    }
    if net.connect_t > 12.0 && !net.welcomed {
        net.status = format!("Timed out waiting for the host at {}", net.addr);
        *role = NetRole::Offline;
        next.set(AppState::Menu);
    }
}

fn net_host_accept(mut net: ResMut<Net>, players: Res<Players>) {
    let Some(listener) = &net.listener else { return };
    let mut fresh = vec![];
    loop {
        match listener.accept() {
            Ok((stream, addr)) => fresh.push((stream, addr)),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(e) => {
                warn!("net: accept failed: {e}");
                break;
            }
        }
    }
    for (stream, addr) in fresh {
        let used: Vec<u8> = players.map.keys().copied().chain(net.peers.iter().map(|p| p.id)).collect();
        let Some(id) = (1..BOT_ID_BASE).find(|i| !used.contains(i)) else { continue };
        match Conn::new(stream) {
            Ok(conn) => {
                info!("net: connection from {addr} (slot {id})");
                net.peers.push(Peer { id, conn, joined: false, age: 0.0 });
            }
            Err(e) => warn!("net: could not set up connection from {addr}: {e}"),
        }
    }
}

/// Pick a team for a new player: their preference if it keeps teams fair, otherwise the
/// side with fewer humans.
fn pick_team(players: &Players, pref: u8) -> u8 {
    let (ho, _) = players.team_counts(TEAM_ORANGE);
    let (hb, _) = players.team_counts(TEAM_BLUE);
    if pref < 2 {
        let (mine, theirs) = if pref == TEAM_ORANGE { (ho, hb) } else { (hb, ho) };
        if mine <= theirs {
            return pref;
        }
    }
    if ho <= hb {
        TEAM_ORANGE
    } else {
        TEAM_BLUE
    }
}

#[allow(clippy::too_many_arguments)]
fn net_receive(
    time: Res<Time<Real>>,
    mut net: ResMut<Net>,
    role: Res<NetRole>,
    level: Res<Level>,
    mut local: ResMut<LocalId>,
    mut players: ResMut<Players>,
    mut world: (ResMut<MatchState>, ResMut<PendingReplay>, ResMut<GameMode>),
    mut ev: (EventWriter<AuthRequest>, EventWriter<WorldEv>, EventWriter<FxEv>, EventWriter<DamageEv>, EventWriter<HudMsg>),
    mut next: ResMut<NextState<AppState>>,
    state: Res<State<AppState>>,
) {
    let dt = time.delta_secs();
    let (ms, replay, mode) = (&mut world.0, &mut world.1, &mut world.2);
    let (auth, wev, fx, dmg, msgs) = (&mut ev.0, &mut ev.1, &mut ev.2, &mut ev.3, &mut ev.4);
    match *role {
        NetRole::Host => {
            let mut gone: Vec<u8> = vec![];
            let mut i = 0;
            while i < net.peers.len() {
                net.peers[i].conn.tick(dt);
                net.peers[i].age += dt;
                net.peers[i].conn.pump();
                let mut msgs_in: Vec<C2S> = vec![];
                while let Some(frame) = net.peers[i].conn.next_frame() {
                    match decode::<C2S>(&frame) {
                        Some(m) => msgs_in.push(m),
                        None => {
                            warn!("net: bad message from slot {}", net.peers[i].id);
                            net.peers[i].conn.dead = true;
                            break;
                        }
                    }
                }
                let id = net.peers[i].id;
                for m in msgs_in {
                    let joined = net.peers[i].joined;
                    match m {
                        C2S::Hello { name, ver, level: lhash, team } if !joined => {
                            let reject = if ver != PROTOCOL_VERSION {
                                Some(format!("Version mismatch (host {PROTOCOL_VERSION}, you {ver}). Update the game."))
                            } else if lhash != level.hash {
                                Some("Map mismatch: your harbor_teams.txt differs from the host's.".to_string())
                            } else if players.map.values().filter(|p| !p.bot).count() >= MAX_PLAYERS {
                                Some("The match is full.".to_string())
                            } else if **mode != GameMode::Teams {
                                Some("The host is not running a team match.".to_string())
                            } else {
                                None
                            };
                            if let Some(r) = reject {
                                if let Some(b) = encode(&S2C::Reject(r)) {
                                    net.peers[i].conn.send_frame(&b);
                                }
                                net.peers[i].conn.close();
                                continue;
                            }
                            let mut name: String = name.chars().filter(|c| !c.is_control()).take(20).collect();
                            if name.trim().is_empty() {
                                name = format!("Agent {id}");
                            }
                            let team = pick_team(&players, team);
                            players.map.insert(id, PlayerEntry::new(name.clone(), team, false));
                            let infos: Vec<PlayerInfo> = players.map.iter().map(|(pid, p)| p.info(*pid)).collect();
                            let welcome = S2C::Welcome { id, players: infos, log: net.log.clone(), matchnet: ms.net.clone() };
                            if let Some(b) = encode(&welcome) {
                                net.peers[i].conn.send_frame(&b);
                            }
                            net.peers[i].joined = true;
                            let info = players.map[&id].info(id);
                            net.broadcast_except(id, &S2C::Joined(info));
                            msgs.write(HudMsg(format!("{name} joined team {}", team_name(team))));
                        }
                        C2S::Hello { .. } => {}
                        _ if !joined => {}
                        C2S::State(s) => {
                            if let Some(p) = players.map.get_mut(&id) {
                                p.net = s;
                            }
                        }
                        C2S::Req(r) => {
                            auth.write(AuthRequest { from: id, req: r });
                        }
                        C2S::Fx(f) => {
                            net.broadcast_except(id, &S2C::Fx(f.clone()));
                            fx.write(FxEv { fx: f, remote: true });
                        }
                        C2S::Chat(text) => {
                            let text: String = text.chars().filter(|c| !c.is_control()).take(120).collect();
                            msgs.write(HudMsg(format!("{}: {text}", players.name_of(id))));
                            net.broadcast_except(id, &S2C::Chat(id, text));
                        }
                        C2S::Bye => net.peers[i].conn.dead = true,
                    }
                }
                let p = &net.peers[i];
                let timed_out = p.conn.idle > TIMEOUT || (!p.joined && p.age > HELLO_TIMEOUT);
                if p.conn.dead || timed_out {
                    let p = net.peers.remove(i);
                    if p.joined {
                        gone.push(p.id);
                    }
                    continue;
                }
                i += 1;
            }
            for id in gone {
                let name = players.name_of(id);
                players.map.remove(&id);
                net.broadcast(&S2C::Left(id));
                msgs.write(HudMsg(format!("{name} left the match")));
            }
        }
        NetRole::Client => {
            let Some(server) = net.server.as_mut() else { return };
            server.tick(dt);
            server.pump();
            let mut lost: Option<String> = None;
            if server.dead {
                lost = Some("Lost connection to the host".into());
            } else if server.idle > TIMEOUT {
                lost = Some("The host stopped responding".into());
            }
            while lost.is_none() && !net.hold {
                let Some(frame) = net.server.as_mut().and_then(|s| s.next_frame()) else { break };
                let Some(m) = decode::<S2C>(&frame) else {
                    lost = Some("Received a corrupt message from the host".into());
                    break;
                };
                match m {
                    S2C::Welcome { id, players: list, log, matchnet } => {
                        local.0 = id;
                        players.map.clear();
                        for p in list {
                            players.map.insert(p.id, PlayerEntry::new(p.name, p.team, p.bot));
                        }
                        ms.begin_round(matchnet);
                        replay.0 = log;
                        **mode = GameMode::Teams;
                        net.welcomed = true;
                        net.hold = true;
                        next.set(AppState::InGame);
                        info!("net: joined as player {id}");
                    }
                    S2C::Reject(reason) => {
                        lost = Some(reason);
                    }
                    S2C::Joined(p) => {
                        if p.id != local.0 {
                            if *state.get() == AppState::InGame && !p.bot {
                                msgs.write(HudMsg(format!("{} joined team {}", p.name, team_name(p.team))));
                            }
                            players.map.insert(p.id, PlayerEntry::new(p.name, p.team, p.bot));
                        }
                    }
                    S2C::Left(id) => {
                        if let Some(p) = players.map.remove(&id) {
                            if !p.bot {
                                msgs.write(HudMsg(format!("{} left the match", p.name)));
                            }
                        }
                    }
                    S2C::States { players: list, time_left } => {
                        for (id, s) in list {
                            if id == local.0 {
                                continue;
                            }
                            if let Some(p) = players.map.get_mut(&id) {
                                p.net = s;
                            }
                        }
                        ms.net.time_left = time_left;
                    }
                    S2C::Npcs { .. } => {}
                    S2C::Ev(e) => {
                        wev.write(WorldEv { ev: e, quiet: false });
                    }
                    S2C::Fx(f) => {
                        fx.write(FxEv { fx: f, remote: true });
                    }
                    S2C::Damage { amount, from, by, weapon } => {
                        dmg.write(DamageEv { amount, from: Vec3::from_array(from), by, weapon });
                    }
                    S2C::Chat(id, text) => {
                        msgs.write(HudMsg(format!("{}: {text}", players.name_of(id))));
                    }
                    S2C::RoundStart(m) => {
                        ms.begin_round(m);
                        net.hold = true;
                        next.set(AppState::RoundReset);
                    }
                }
            }
            if let Some(reason) = lost {
                warn!("net: {reason}");
                net.shutdown();
                net.status = reason;
                next.set(AppState::Menu);
            }
        }
        NetRole::Offline => {}
    }
}

/// Authority: deliver damage to whoever was hit.
fn net_damage(mut evs: EventReader<PlayerDamage>, local: Res<LocalId>, mut net: ResMut<Net>, mut dmg: EventWriter<DamageEv>) {
    for d in evs.read() {
        if d.target == local.0 {
            dmg.write(DamageEv { amount: d.amount, from: d.from, by: d.by, weapon: d.weapon });
        } else if net.is_peer(d.target) {
            net.send_to(d.target, &S2C::Damage { amount: d.amount, from: d.from.to_array(), by: d.by, weapon: d.weapon });
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn net_send(
    time: Res<Time<Real>>,
    mut net: ResMut<Net>,
    role: Res<NetRole>,
    local: Res<LocalId>,
    players: Res<Players>,
    ms: Res<MatchState>,
    state: Res<State<AppState>>,
    mut evs: (EventReader<WorldEv>, EventReader<FxEv>, EventReader<AuthRequest>),
) {
    let (wev, fx, auth) = (&mut evs.0, &mut evs.1, &mut evs.2);
    match *role {
        NetRole::Host => {
            for e in wev.read() {
                if e.quiet {
                    continue;
                }
                if e.ev.stateful() {
                    net.push_log(&e.ev);
                }
                net.broadcast(&S2C::Ev(e.ev.clone()));
            }
            for f in fx.read() {
                if !f.remote {
                    net.broadcast(&S2C::Fx(f.fx.clone()));
                }
            }
            auth.clear();
            net.send_acc += time.delta_secs();
            if net.send_acc >= 1.0 / HOST_SEND_HZ && *state.get() == AppState::InGame {
                net.send_acc = 0.0;
                let list: Vec<(u8, PlayerNet)> = players.map.iter().map(|(id, p)| (*id, p.net)).collect();
                net.broadcast(&S2C::States { players: list, time_left: ms.net.time_left });
            }
            for p in net.peers.iter_mut() {
                p.conn.pump();
            }
        }
        NetRole::Client => {
            wev.clear();
            let mut out: Vec<C2S> = vec![];
            for a in auth.read() {
                if a.from == local.0 {
                    out.push(C2S::Req(a.req.clone()));
                }
            }
            for f in fx.read() {
                if !f.remote {
                    out.push(C2S::Fx(f.fx.clone()));
                }
            }
            net.send_acc += time.delta_secs();
            if net.send_acc >= 1.0 / CLIENT_SEND_HZ && *state.get() == AppState::InGame {
                net.send_acc = 0.0;
                if let Some(p) = players.map.get(&local.0) {
                    out.push(C2S::State(p.net));
                }
            }
            for m in &out {
                net.to_host(m);
            }
            if let Some(s) = net.server.as_mut() {
                s.pump();
            }
        }
        NetRole::Offline => {
            wev.clear();
            fx.clear();
            auth.clear();
        }
    }
}
