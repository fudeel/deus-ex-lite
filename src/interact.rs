//! Looking at things and using them: doors, locks, pickups, terminals, keypads, NPCs, bodies.
use bevy::prelude::*;

use crate::audio::*;
use crate::collision::*;
use crate::game::*;
use crate::items::*;
use crate::level::*;
use crate::npc::Guard;
use crate::player::*;
use crate::world::*;

#[derive(Resource, Default)]
pub struct Focus {
    pub target: Option<Interact>,
    pub prompt: String,
    pub sub: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum HoldKind {
    #[default]
    None,
    Pick {
        door: u16,
        picks: u16,
    },
}

#[derive(Resource, Default)]
pub struct Hold {
    pub kind: HoldKind,
    pub t: f32,
    pub total: f32,
    pub sound_t: f32,
}

#[derive(Resource, Default)]
pub struct TermSession {
    pub id: u16,
    pub access: bool,
    pub hacking: bool,
    pub hack_t: f32,
    pub hack_total: f32,
    /// seconds before a hacker is kicked out (negative = legit login, no limit)
    pub trace: f32,
    pub reading: Option<u16>,
    pub user: String,
    pub tick: f32,
}

#[derive(Resource, Default)]
pub struct KeypadState {
    pub door: u16,
    pub entry: String,
    pub status: u8,
    pub flash: f32,
}

#[derive(Resource, Default)]
pub struct TalkState {
    pub npc: u16,
    pub node: u8,
}

pub enum DlgAct {
    Goto(u8),
    GiveCode(u8),
    End,
}

pub struct DlgNode {
    pub text: &'static str,
    pub choices: &'static [(&'static str, DlgAct)],
}

pub const INFORMANT: [DlgNode; 4] = [
    DlgNode {
        text: "Keep your voice down. You're the one Control sent?",
        choices: &[("I am. What have you got for me?", DlgAct::Goto(1)), ("Who are you?", DlgAct::Goto(2)), ("Not now.", DlgAct::End)],
    },
    DlgNode {
        text: "Warehouse side door - east wall, facing the yard. Keypad code is 5532. Kade rotates it weekly; you're lucky tonight.",
        choices: &[("What about the lab itself?", DlgAct::GiveCode(3)), ("That's all I need.", DlgAct::GiveCode(255))],
    },
    DlgNode {
        text: "Somebody who used to run night security for Halvorsen-Kade. Until I saw what they keep in that lab. Talk or walk.",
        choices: &[("Fine. What have you got?", DlgAct::Goto(1)), ("Goodbye.", DlgAct::End)],
    },
    DlgNode {
        text: "The security office controls the lab door. The night manager can't remember a password to save his life - check the office block. And the server room turret never sleeps. Go around it, or switch it off from security.",
        choices: &[("Thanks. Stay out of sight.", DlgAct::End)],
    },
];

pub struct InteractPlugin;

impl Plugin for InteractPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Focus>()
            .init_resource::<Hold>()
            .init_resource::<TermSession>()
            .init_resource::<KeypadState>()
            .init_resource::<TalkState>()
            .add_systems(
                Update,
                (update_focus, use_focus, terminal_tick, keypad_input)
                    .chain()
                    .in_set(GameSet::Logic)
                    .after(PlayerLogic)
                    .run_if(in_game),
            );
    }
}

pub fn picks_needed(strength: u8, skill: u8) -> u16 {
    (strength as i32 - skill as i32).max(1) as u16
}

pub fn tools_needed(strength: u8, skill: u8) -> u16 {
    (strength as i32 - skill as i32).max(1) as u16
}

pub fn known_login(level: &Level, state: &WorldState, login: &str) -> bool {
    state.notes.iter().any(|n| level.note(*n).and_then(|d| d.login.as_ref()).map(|(u, _)| u == login).unwrap_or(false))
}

pub fn known_codes(level: &Level, state: &WorldState) -> Vec<String> {
    state.notes.iter().filter_map(|n| level.note(*n).and_then(|d| d.code.clone())).collect()
}

#[allow(clippy::too_many_arguments)]
fn update_focus(
    me: Res<Me>,
    ui: Res<UiMode>,
    pose: Res<CamPose>,
    phys: Res<Physics>,
    level: Res<Level>,
    inv: Res<Inventory>,
    skills: Res<Skills>,
    state: Res<WorldState>,
    items: Query<&Interactable>,
    doors: Query<&Door>,
    guards: Query<&Guard>,
    pickups: Query<&Pickup>,
    mut focus: ResMut<Focus>,
) {
    focus.target = None;
    focus.prompt.clear();
    focus.sub.clear();
    if *ui != UiMode::None || me.dead {
        return;
    }
    let o = pose.pos;
    let d = pose.rot * Vec3::NEG_Z;
    let reach = 2.4;
    let wall = phys.raycast(o, d, reach, false, false, None);
    let mut limit = wall.map(|h| h.t + 0.08).unwrap_or(reach);
    let mut best: Option<(f32, Interact)> = None;
    if let Some(h) = wall {
        if let Body::Door(id) = h.body {
            best = Some((h.t, Interact::Door(id)));
            limit = h.t;
        }
    }
    for it in items.iter() {
        if let Interact::Body(id) = it.kind {
            if guards.iter().any(|g| g.id == id && g.conscious()) {
                continue;
            }
        }
        if let Some((t, _)) = it.aabb.ray(o, d, limit) {
            if best.map(|b| t < b.0).unwrap_or(true) {
                best = Some((t, it.kind));
            }
        }
    }
    let Some((_, target)) = best else { return };
    focus.target = Some(target);
    let lockpick = skills.get(Skill::Lockpicking);
    match target {
        Interact::Door(id) => {
            let Some(d) = doors.iter().find(|d| d.id == id) else { return };
            let def = &level.doors[id as usize];
            if d.kind == DoorKind::Grate {
                focus.prompt = if d.open { "[F] Close vent grate".into() } else { "[F] Open vent grate".into() };
            } else if !d.locked {
                focus.prompt = if d.open { "[F] Close door".into() } else { "[F] Open door".into() };
            } else {
                match d.kind {
                    DoorKind::Locked => {
                        if let Some(k) = def.key.filter(|k| inv.keys.contains(k)) {
                            focus.prompt = format!("[F] Unlock with {}", level.key_name(k));
                        } else {
                            let n = picks_needed(def.strength, lockpick);
                            let have = inv.count(ItemKind::Lockpick);
                            focus.prompt = if have >= n { format!("[Hold F] Pick lock ({n} lockpick{})", if n > 1 { "s" } else { "" }) } else { "Locked".into() };
                            focus.sub = format!("Lock strength {} - lockpicks: {have}/{n}", def.strength);
                        }
                    }
                    DoorKind::Keypad => {
                        focus.prompt = "[F] Use keypad".into();
                        focus.sub = "Electronic lock".into();
                    }
                    DoorKind::Security => {
                        focus.prompt = "Security door".into();
                        focus.sub = "Controlled from a security terminal".into();
                    }
                    _ => focus.prompt = "Locked".into(),
                }
            }
        }
        Interact::Pickup(id) => {
            if let Some(p) = pickups.iter().find(|p| p.id == id) {
                focus.prompt = match p.item {
                    ItemKind::Datacube(_) => "[F] Read datacube".into(),
                    k => format!("[F] Pick up {}", item_name(k, &level)),
                };
                if !inv.can_take(p.item) {
                    focus.sub = "Not enough room in inventory".into();
                }
            }
        }
        Interact::Terminal(id) => {
            let t = &level.terminals[id as usize];
            focus.prompt = "[F] Use computer".into();
            focus.sub = if t.kind == TermKind::Security { "Halvorsen-Kade Security Console".into() } else { "Personal workstation".into() };
        }
        Interact::Switch(_) => focus.prompt = "[F] Light switch".into(),
        Interact::Npc(_) => focus.prompt = "[F] Talk".into(),
        Interact::Body(id) => {
            let dead = guards.iter().any(|g| g.id == id && g.state == crate::npc::GState::Dead);
            focus.prompt = if state.looted.contains(&id) { "Body (searched)".into() } else { "[F] Search body".into() };
            focus.sub = if dead { "Dead".into() } else { "Unconscious".into() };
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn use_focus(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<Focus>,
    level: Res<Level>,
    doors: Query<&Door>,
    pickups: Query<&Pickup>,
    state: Res<WorldState>,
    skills: Res<Skills>,
    mut inv: ResMut<Inventory>,
    mut hold: ResMut<Hold>,
    mut ui: ResMut<UiMode>,
    mut dirty: ResMut<UiDirty>,
    mut sessions: (ResMut<TermSession>, ResMut<KeypadState>, ResMut<TalkState>),
    mut req: EventWriter<WorldRequest>,
    mut snd: EventWriter<SoundEv>,
    mut msgs: EventWriter<HudMsg>,
) {
    let (term, keypad, talk) = (&mut sessions.0, &mut sessions.1, &mut sessions.2);
    let dt = time.delta_secs();
    // hold action in progress
    if let HoldKind::Pick { door, picks } = hold.kind {
        let still = keys.pressed(KeyCode::KeyF) && focus.target == Some(Interact::Door(door)) && *ui == UiMode::None;
        if !still {
            hold.kind = HoldKind::None;
        } else {
            hold.t += dt;
            hold.sound_t -= dt;
            if hold.sound_t <= 0.0 {
                hold.sound_t = 0.55;
                if let Some(d) = level.doors.get(door as usize) {
                    snd.write(SoundEv::at(Sfx::Pick, Level::center(d.x, d.z) + Vec3::Y, 0.6));
                }
            }
            if hold.t >= hold.total {
                hold.kind = HoldKind::None;
                if inv.consume(ItemKind::Lockpick, picks) {
                    req.write(WorldRequest::Unlock { id: door });
                    msgs.write(HudMsg(format!("Lock picked ({picks} lockpick{} used)", if picks > 1 { "s" } else { "" })));
                }
            }
        }
        return;
    }
    if *ui != UiMode::None || !keys.just_pressed(KeyCode::KeyF) {
        return;
    }
    let Some(target) = focus.target else { return };
    match target {
        Interact::Door(id) => {
            let Some(d) = doors.iter().find(|d| d.id == id) else { return };
            let def = &level.doors[id as usize];
            if !d.locked {
                req.write(WorldRequest::SetDoor { id, open: !d.open });
                return;
            }
            match d.kind {
                DoorKind::Locked => {
                    if let Some(k) = def.key.filter(|k| inv.keys.contains(k)) {
                        req.write(WorldRequest::Unlock { id });
                        msgs.write(HudMsg(format!("Unlocked with {}", level.key_name(k))));
                    } else {
                        let n = picks_needed(def.strength, skills.get(Skill::Lockpicking));
                        if inv.count(ItemKind::Lockpick) >= n {
                            hold.kind = HoldKind::Pick { door: id, picks: n };
                            hold.t = 0.0;
                            hold.total = 1.2 + n as f32 * 0.9 - skills.get(Skill::Lockpicking) as f32 * 0.25;
                            hold.sound_t = 0.0;
                        } else {
                            snd.write(SoundEv::ui(Sfx::Locked, 0.6));
                            msgs.write(HudMsg(format!("Locked. You need {n} lockpicks or the key.")));
                        }
                    }
                }
                DoorKind::Keypad => {
                    **keypad = KeypadState { door: id, ..default() };
                    *ui = UiMode::Keypad(id);
                    dirty.0 = true;
                }
                _ => {
                    snd.write(SoundEv::ui(Sfx::Locked, 0.6));
                    msgs.write(HudMsg("This door is controlled by a security terminal".into()));
                }
            }
        }
        Interact::Pickup(id) => {
            let Some(p) = pickups.iter().find(|p| p.id == id) else { return };
            match p.item {
                ItemKind::Datacube(note) => {
                    req.write(WorldRequest::ReadNote { id: note });
                    *ui = UiMode::Read(note);
                    dirty.0 = true;
                    snd.write(SoundEv::ui(Sfx::Beep, 0.5));
                }
                k => {
                    if inv.can_take(k) {
                        req.write(WorldRequest::Pickup { id });
                    } else {
                        msgs.write(HudMsg("Not enough room in your inventory".into()));
                        snd.write(SoundEv::ui(Sfx::Denied, 0.5));
                    }
                }
            }
        }
        Interact::Terminal(id) => {
            **term = TermSession { id, ..default() };
            *ui = UiMode::Terminal(id);
            dirty.0 = true;
            snd.write(SoundEv::ui(Sfx::Beep, 0.5));
        }
        Interact::Switch(id) => {
            req.write(WorldRequest::Switch { id });
        }
        Interact::Npc(id) => {
            **talk = TalkState { npc: id, node: 0 };
            *ui = UiMode::Talk(id);
            dirty.0 = true;
        }
        Interact::Body(id) => {
            if !state.looted.contains(&id) {
                req.write(WorldRequest::LootBody { id });
            }
        }
    }
}

fn terminal_tick(time: Res<Time>, mut term: ResMut<TermSession>, mut ui: ResMut<UiMode>, mut dirty: ResMut<UiDirty>, mut msgs: EventWriter<HudMsg>, mut snd: EventWriter<SoundEv>) {
    if !matches!(*ui, UiMode::Terminal(_)) {
        return;
    }
    let dt = time.delta_secs();
    term.tick -= dt;
    if term.hacking {
        term.hack_t += dt;
        if term.tick <= 0.0 {
            snd.write(SoundEv::ui(Sfx::Hack, 0.35));
        }
        if term.hack_t >= term.hack_total {
            term.hacking = false;
            term.access = true;
            term.user = "[BYPASS]".into();
            snd.write(SoundEv::ui(Sfx::Granted, 0.6));
            dirty.0 = true;
        }
    } else if term.access && term.trace >= 0.0 {
        term.trace -= dt;
        if term.trace <= 0.0 {
            *ui = UiMode::None;
            msgs.write(HudMsg("Trace complete - you were locked out".into()));
            snd.write(SoundEv::ui(Sfx::Denied, 0.6));
        }
    }
    if term.tick <= 0.0 {
        term.tick = 0.25;
        dirty.0 = true;
    }
}

/// Typing digits on a keypad with the keyboard.
fn keypad_input(keys: Res<ButtonInput<KeyCode>>, ui: Res<UiMode>, mut kp: ResMut<KeypadState>, mut dirty: ResMut<UiDirty>, mut snd: EventWriter<SoundEv>, mut req: EventWriter<WorldRequest>, level: Res<Level>) {
    let UiMode::Keypad(_) = *ui else { return };
    let digits = [
        (KeyCode::Digit0, '0'),
        (KeyCode::Digit1, '1'),
        (KeyCode::Digit2, '2'),
        (KeyCode::Digit3, '3'),
        (KeyCode::Digit4, '4'),
        (KeyCode::Digit5, '5'),
        (KeyCode::Digit6, '6'),
        (KeyCode::Digit7, '7'),
        (KeyCode::Digit8, '8'),
        (KeyCode::Digit9, '9'),
        (KeyCode::Numpad0, '0'),
        (KeyCode::Numpad1, '1'),
        (KeyCode::Numpad2, '2'),
        (KeyCode::Numpad3, '3'),
        (KeyCode::Numpad4, '4'),
        (KeyCode::Numpad5, '5'),
        (KeyCode::Numpad6, '6'),
        (KeyCode::Numpad7, '7'),
        (KeyCode::Numpad8, '8'),
        (KeyCode::Numpad9, '9'),
    ];
    for (k, c) in digits {
        if keys.just_pressed(k) {
            keypad_press(&mut kp, c, &mut snd, &mut req, &level);
            dirty.0 = true;
        }
    }
    if keys.just_pressed(KeyCode::Backspace) {
        kp.entry.pop();
        dirty.0 = true;
    }
    if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter) {
        keypad_press(&mut kp, '#', &mut snd, &mut req, &level);
        dirty.0 = true;
    }
}

pub fn keypad_press(kp: &mut KeypadState, c: char, snd: &mut EventWriter<SoundEv>, req: &mut EventWriter<WorldRequest>, level: &Level) {
    match c {
        'C' => {
            kp.entry.clear();
            kp.status = 0;
            snd.write(SoundEv::ui(Sfx::Beep, 0.4));
        }
        '#' => {
            let code = level.doors.get(kp.door as usize).and_then(|d| d.code.clone()).unwrap_or_default();
            if kp.entry == code {
                kp.status = 1;
                snd.write(SoundEv::ui(Sfx::Granted, 0.6));
                req.write(WorldRequest::Unlock { id: kp.door });
            } else {
                kp.status = 2;
                kp.entry.clear();
                snd.write(SoundEv::ui(Sfx::Denied, 0.6));
            }
        }
        d => {
            if kp.entry.len() < 6 && kp.status != 1 {
                kp.entry.push(d);
                kp.status = 0;
                snd.write(SoundEv::ui(Sfx::Beep, 0.4));
            }
        }
    }
}
