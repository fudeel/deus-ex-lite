//! In-game HUD and full-screen panels (inventory, skills, terminals, keypads, dialogue...).
use bevy::app::AppExit;
use bevy::asset::RenderAssetUsages;
use bevy::ecs::hierarchy::ChildSpawnerCommands;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use std::collections::VecDeque;

use crate::audio::*;
use crate::game::*;
use crate::interact::*;
use crate::items::*;
use crate::level::*;
use crate::player::*;
use crate::weapons::WeaponState;
use crate::world::*;

pub const C_TEXT: Color = Color::srgb(0.82, 0.9, 0.95);
pub const C_DIM: Color = Color::srgb(0.45, 0.52, 0.58);
pub const C_ACCENT: Color = Color::srgb(1.0, 0.72, 0.22);
pub const C_TEAL: Color = Color::srgb(0.25, 0.85, 0.95);
pub const C_RED: Color = Color::srgb(1.0, 0.25, 0.2);
pub const C_PANEL: Color = Color::srgba(0.015, 0.03, 0.045, 0.9);
pub const C_BORDER: Color = Color::srgba(0.25, 0.8, 0.95, 0.45);

#[derive(Component)]
pub struct HudRoot;

#[derive(Component)]
pub struct PanelRoot;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum HudEl {
    HealthFill,
    EnergyFill,
    HealthText,
    EnergyText,
    LightFill,
    LightText,
    Weapon,
    Ammo,
    Prompt,
    PromptSub,
    HoldBar,
    HoldFill,
    Messages,
    Subtitle,
    InfoBox,
    InfoWho,
    InfoText,
    Alarm,
    DeathBox,
    DeathText,
    Vision,
    Hurt,
    Cloak,
    Cross(u8),
    Belt(u8),
    BeltText(u8),
    Aug(u8),
    Objective,
}

#[derive(Component, Clone, Copy, PartialEq, Debug)]
pub enum UiAction {
    Close,
    Resume,
    QuitToMenu,
    QuitGame,
    CycleQuality,
    Sens(f32),
    Vol(f32),
    Fov(f32),
    Bright(f32),
    Tab(u8),
    UseItem(u32),
    Upgrade(u8),
    ReadNote(u16),
    TermLogin,
    TermHack,
    TermCams,
    TermTurrets(u8),
    TermDoor(u16, bool),
    TermMail(u16),
    TermBack,
    Key(char),
    KeyBypass,
    Talk(u8),
    InstallAug(u8),
    MenuStart,
    MenuQuit,
    // ---- team match / online
    MenuSkirmish,
    MenuHost,
    MenuJoin,
    EditName,
    EditAddr,
    TeamSize(i8),
    CycleTeam,
    CancelConnect,
    SwitchTeam,
}

#[derive(Component)]
pub struct UiButton {
    pub action: UiAction,
    pub enabled: bool,
}

#[derive(Resource, Default)]
pub struct HudState {
    pub msgs: VecDeque<(String, f32)>,
    pub sub: Option<(String, String, f32)>,
    pub info: Option<(u16, f32)>,
    pub note_sel: Option<u16>,
    pub vignette: Option<Handle<Image>>,
}

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HudState>()
            .add_systems(Startup, make_vignette)
            .add_systems(OnEnter(AppState::InGame), (despawn_hud, spawn_hud).chain())
            .add_systems(OnExit(AppState::InGame), (despawn_hud, close_ui))
            .add_systems(Update, (collect_messages, update_hud, update_overlays, update_crosshair).in_set(GameSet::Present).run_if(in_game))
            .add_systems(Update, (rebuild_panel, completion_watch).in_set(GameSet::Present).run_if(in_game))
            .add_systems(Update, (button_visuals, game_actions).in_set(GameSet::Logic));
    }
}

fn close_ui(mut ui: ResMut<UiMode>) {
    *ui = UiMode::None;
}

fn make_vignette(mut images: ResMut<Assets<Image>>, mut hud: ResMut<HudState>) {
    let s = 256u32;
    let mut data = vec![0u8; (s * s * 4) as usize];
    for y in 0..s {
        for x in 0..s {
            let u = x as f32 / s as f32 * 2.0 - 1.0;
            let v = y as f32 / s as f32 * 2.0 - 1.0;
            let r = (u * u * 0.8 + v * v).sqrt();
            let a = crate::util::smoothstep(0.55, 1.35, r) * 0.85;
            let i = ((y * s + x) * 4) as usize;
            data[i + 3] = (a * 255.0) as u8;
        }
    }
    let img = Image::new(Extent3d { width: s, height: s, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD);
    hud.vignette = Some(images.add(img));
}

fn despawn_hud(mut commands: Commands, q: Query<Entity, Or<(With<HudRoot>, With<PanelRoot>)>>) {
    for e in q.iter() {
        commands.entity(e).despawn();
    }
}

pub fn txt(p: &mut ChildSpawnerCommands, s: impl Into<String>, size: f32, color: Color) -> Entity {
    p.spawn((Text::new(s.into()), TextFont { font_size: size, ..default() }, TextColor(color))).id()
}

fn txt_el(p: &mut ChildSpawnerCommands, s: &str, size: f32, color: Color, el: HudEl) {
    p.spawn((Text::new(s), TextFont { font_size: size, ..default() }, TextColor(color), el));
}

pub fn button(p: &mut ChildSpawnerCommands, label: impl Into<String>, action: UiAction, enabled: bool) {
    p.spawn((
        Button,
        Node {
            padding: UiRect::axes(Val::Px(14.0), Val::Px(7.0)),
            margin: UiRect::all(Val::Px(3.0)),
            border: UiRect::all(Val::Px(1.0)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(if enabled { Color::srgba(0.05, 0.12, 0.16, 0.95) } else { Color::srgba(0.05, 0.05, 0.06, 0.8) }),
        BorderColor(if enabled { C_BORDER } else { Color::srgba(0.3, 0.3, 0.3, 0.3) }),
        UiButton { action, enabled },
    ))
    .with_children(|b| {
        txt(b, label, 16.0, if enabled { C_TEXT } else { C_DIM });
    });
}

fn abs_node(left: Option<f32>, right: Option<f32>, top: Option<f32>, bottom: Option<f32>) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: left.map(Val::Px).unwrap_or(Val::Auto),
        right: right.map(Val::Px).unwrap_or(Val::Auto),
        top: top.map(Val::Px).unwrap_or(Val::Auto),
        bottom: bottom.map(Val::Px).unwrap_or(Val::Auto),
        ..default()
    }
}

fn bar(p: &mut ChildSpawnerCommands, label: &str, color: Color, fill: HudEl, text: HudEl) {
    p.spawn(Node { flex_direction: FlexDirection::Column, margin: UiRect::bottom(Val::Px(6.0)), ..default() }).with_children(|c| {
        c.spawn(Node { justify_content: JustifyContent::SpaceBetween, width: Val::Px(250.0), ..default() }).with_children(|r| {
            txt(r, label, 13.0, C_DIM);
            txt_el(r, "100", 13.0, C_TEXT, text);
        });
        c.spawn((
            Node { width: Val::Px(250.0), height: Val::Px(9.0), border: UiRect::all(Val::Px(1.0)), ..default() },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
            BorderColor(Color::srgba(1.0, 1.0, 1.0, 0.12)),
        ))
        .with_children(|b| {
            b.spawn((Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }, BackgroundColor(color), fill));
        });
    });
}

fn spawn_hud(mut commands: Commands, hud: Res<HudState>) {
    let full = Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() };
    let root = commands.spawn((full.clone(), HudRoot)).id();
    commands.entity(root).with_children(|r| {
        if let Some(v) = &hud.vignette {
            r.spawn((full.clone(), ImageNode::new(v.clone())));
        }
        r.spawn((full.clone(), BackgroundColor(Color::srgba(0.05, 1.0, 0.25, 0.10)), Visibility::Hidden, HudEl::Vision));
        r.spawn((full.clone(), BackgroundColor(Color::srgba(0.3, 0.7, 1.0, 0.06)), Visibility::Hidden, HudEl::Cloak));
        r.spawn((full.clone(), BackgroundColor(Color::srgba(0.7, 0.0, 0.0, 0.0)), HudEl::Hurt));

        // vitals
        r.spawn(Node { flex_direction: FlexDirection::Column, ..abs_node(Some(24.0), None, Some(20.0), None) })
            .with_children(|c| {
                bar(c, "HEALTH", Color::srgb(0.85, 0.15, 0.12), HudEl::HealthFill, HudEl::HealthText);
                bar(c, "BIOENERGY", Color::srgb(0.2, 0.6, 1.0), HudEl::EnergyFill, HudEl::EnergyText);
                bar(c, "LIGHT EXPOSURE", Color::srgb(0.95, 0.85, 0.4), HudEl::LightFill, HudEl::LightText);
                txt_el(c, "", 13.0, C_DIM, HudEl::Objective);
            });

        // augmentation icons
        r.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(4.0), ..abs_node(None, Some(24.0), Some(20.0), None) }).with_children(|c| {
            for (i, a) in AUGS.iter().enumerate() {
                c.spawn((
                    Node { width: Val::Px(46.0), height: Val::Px(40.0), border: UiRect::all(Val::Px(1.0)), flex_direction: FlexDirection::Column, align_items: AlignItems::Center, justify_content: JustifyContent::Center, ..default() },
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
                    BorderColor(C_BORDER),
                    HudEl::Aug(i as u8),
                ))
                .with_children(|b| {
                    txt(b, format!("F{}", i + 1), 10.0, C_DIM);
                    txt(b, a.short(), 13.0, C_TEXT);
                });
            }
        });

        // alarm banner
        r.spawn((Node { justify_content: JustifyContent::Center, width: Val::Percent(100.0), ..abs_node(Some(0.0), None, Some(70.0), None) }, Visibility::Hidden, HudEl::Alarm))
            .with_children(|c| {
                c.spawn((Node { padding: UiRect::axes(Val::Px(18.0), Val::Px(6.0)), border: UiRect::all(Val::Px(1.0)), ..default() }, BackgroundColor(Color::srgba(0.4, 0.0, 0.0, 0.6)), BorderColor(C_RED)))
                    .with_children(|b| {
                        txt(b, "ALARM  -  SECURITY ALERTED", 18.0, C_RED);
                    });
            });

        // infolink
        r.spawn((
            Node {
                width: Val::Px(620.0),
                padding: UiRect::all(Val::Px(10.0)),
                border: UiRect::all(Val::Px(1.0)),
                column_gap: Val::Px(12.0),
                ..abs_node(None, Some(24.0), Some(76.0), None)
            },
            BackgroundColor(C_PANEL),
            BorderColor(C_BORDER),
            Visibility::Hidden,
            HudEl::InfoBox,
        ))
        .with_children(|c| {
            c.spawn((Node { width: Val::Px(64.0), height: Val::Px(64.0), border: UiRect::all(Val::Px(1.0)), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() }, BackgroundColor(Color::srgb(0.02, 0.1, 0.12)), BorderColor(C_TEAL)))
                .with_children(|b| {
                    txt(b, "((o))", 16.0, C_TEAL);
                });
            c.spawn(Node { flex_direction: FlexDirection::Column, flex_shrink: 1.0, ..default() }).with_children(|b| {
                txt_el(b, "INFOLINK", 13.0, C_TEAL, HudEl::InfoWho);
                b.spawn((Text::new(""), TextFont { font_size: 15.0, ..default() }, TextColor(C_TEXT), Node { max_width: Val::Px(520.0), ..default() }, HudEl::InfoText));
            });
        });

        // crosshair
        for i in 0..5u8 {
            let (w, h) = if i == 4 { (2.0, 2.0) } else if i < 2 { (10.0, 2.0) } else { (2.0, 10.0) };
            r.spawn((
                Node { position_type: PositionType::Absolute, width: Val::Px(w), height: Val::Px(h), left: Val::Percent(50.0), top: Val::Percent(50.0), ..default() },
                BackgroundColor(Color::srgba(0.85, 0.95, 1.0, 0.85)),
                HudEl::Cross(i),
            ));
        }

        // prompt
        r.spawn(Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, width: Val::Percent(100.0), ..abs_node(Some(0.0), None, None, Some(330.0)) }).with_children(|c| {
            txt_el(c, "", 18.0, C_TEXT, HudEl::Prompt);
            txt_el(c, "", 13.0, C_DIM, HudEl::PromptSub);
            c.spawn((Node { width: Val::Px(220.0), height: Val::Px(6.0), margin: UiRect::top(Val::Px(6.0)), ..default() }, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)), Visibility::Hidden, HudEl::HoldBar))
                .with_children(|b| {
                    b.spawn((Node { width: Val::Percent(0.0), height: Val::Percent(100.0), ..default() }, BackgroundColor(C_ACCENT), HudEl::HoldFill));
                });
        });

        // subtitles
        r.spawn(Node { justify_content: JustifyContent::Center, width: Val::Percent(100.0), ..abs_node(Some(0.0), None, None, Some(120.0)) }).with_children(|c| {
            txt_el(c, "", 17.0, C_TEXT, HudEl::Subtitle);
        });

        // messages
        r.spawn(Node { flex_direction: FlexDirection::Column, ..abs_node(Some(24.0), None, None, Some(110.0)) }).with_children(|c| {
            txt_el(c, "", 14.0, C_TEXT, HudEl::Messages);
        });

        // weapon
        r.spawn(Node { flex_direction: FlexDirection::Column, align_items: AlignItems::FlexEnd, ..abs_node(None, Some(28.0), None, Some(26.0)) }).with_children(|c| {
            txt_el(c, "", 14.0, C_DIM, HudEl::Weapon);
            txt_el(c, "", 34.0, C_TEXT, HudEl::Ammo);
        });

        // belt
        r.spawn(Node { justify_content: JustifyContent::Center, width: Val::Percent(100.0), column_gap: Val::Px(4.0), ..abs_node(Some(0.0), None, None, Some(22.0)) }).with_children(|c| {
            for i in 0..10u8 {
                c.spawn((
                    Node { width: Val::Px(64.0), height: Val::Px(46.0), border: UiRect::all(Val::Px(1.0)), flex_direction: FlexDirection::Column, padding: UiRect::all(Val::Px(3.0)), ..default() },
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
                    BorderColor(Color::srgba(1.0, 1.0, 1.0, 0.1)),
                    HudEl::Belt(i),
                ))
                .with_children(|b| {
                    txt(b, format!("{}", (i + 1) % 10), 10.0, C_DIM);
                    txt_el(b, "", 12.0, C_TEXT, HudEl::BeltText(i));
                });
            }
        });

        // death
        r.spawn((
            Node { width: Val::Percent(100.0), height: Val::Percent(100.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, flex_direction: FlexDirection::Column, ..abs_node(Some(0.0), None, Some(0.0), None) },
            BackgroundColor(Color::srgba(0.15, 0.0, 0.0, 0.35)),
            Visibility::Hidden,
            HudEl::DeathBox,
        ))
        .with_children(|c| {
            txt(c, "YOU HAVE BEEN KILLED", 40.0, C_RED);
            txt_el(c, "", 18.0, C_TEXT, HudEl::DeathText);
        });
    });
}

fn collect_messages(time: Res<Time>, mut hud: ResMut<HudState>, mut msgs: EventReader<HudMsg>, mut subs: EventReader<Subtitle>, mut info: ResMut<InfolinkQueue>, mut snd: EventWriter<SoundEv>) {
    let now = time.elapsed_secs();
    for m in msgs.read() {
        hud.msgs.push_back((m.0.clone(), now));
        while hud.msgs.len() > 6 {
            hud.msgs.pop_front();
        }
    }
    hud.msgs.retain(|(_, t)| now - *t < 7.0);
    for s in subs.read() {
        hud.sub = Some((s.who.clone(), s.text.clone(), now));
    }
    if let Some((_, _, t)) = &hud.sub {
        if now - t > 3.5 {
            hud.sub = None;
        }
    }
    let busy = hud.info.map(|(id, t)| now - t < 4.0 + infolink(id).1.len() as f32 * 0.045).unwrap_or(false);
    if !busy {
        hud.info = None;
        if !info.0.is_empty() {
            let id = info.0.remove(0);
            hud.info = Some((id, now));
            snd.write(SoundEv::ui(Sfx::Chirp, 0.5));
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn update_hud(
    time: Res<Time>,
    me: Res<Me>,
    inv: Res<Inventory>,
    augs: Res<Augs>,
    ws: Res<WeaponState>,
    focus: Res<Focus>,
    hold: Res<Hold>,
    hud: Res<HudState>,
    state: Res<WorldState>,
    team: (Res<GameMode>, Res<Players>, Res<LocalId>),
    mut nodes: Query<(&HudEl, Option<&mut Node>, Option<&mut Text>, Option<&mut Visibility>, Option<&mut BackgroundColor>, Option<&mut BorderColor>, Option<&mut TextColor>)>,
) {
    let (mode, players, local) = (&team.0, &team.1, &team.2);
    let teams = **mode == GameMode::Teams;
    let my_team = players.team_of(local.0);
    let now = time.elapsed_secs();
    let weapon = inv.equipped_weapon();
    for (el, node, text, vis, bg, border, tcol) in nodes.iter_mut() {
        match *el {
            HudEl::HealthFill => {
                if let Some(mut n) = node {
                    n.width = Val::Percent(me.health.clamp(0.0, 100.0));
                }
            }
            HudEl::EnergyFill => {
                if let Some(mut n) = node {
                    n.width = Val::Percent(me.energy.clamp(0.0, 100.0));
                }
            }
            HudEl::LightFill => {
                if let Some(mut n) = node {
                    n.width = Val::Percent(me.visibility * 100.0);
                }
            }
            HudEl::HealthText => {
                if let Some(mut t) = text {
                    t.0 = format!("{:.0}", me.health.max(0.0));
                }
            }
            HudEl::EnergyText => {
                if let Some(mut t) = text {
                    t.0 = format!("{:.0}", me.energy);
                }
            }
            HudEl::LightText => {
                let (s, c) = if augs.on(Aug::Cloak) {
                    ("CLOAKED", C_TEAL)
                } else if me.visibility < 0.18 {
                    ("HIDDEN", C_TEAL)
                } else if me.visibility < 0.45 {
                    ("VISIBLE", C_ACCENT)
                } else {
                    ("EXPOSED", C_RED)
                };
                if let Some(mut t) = text {
                    t.0 = s.into();
                }
                if let Some(mut tc) = tcol {
                    tc.0 = c;
                }
            }
            HudEl::Objective => {
                if teams {
                    if let Some(mut t) = text {
                        t.0 = format!("> TEAM {}", team_name(my_team));
                    }
                    if let Some(mut c) = tcol {
                        c.0 = team_color(my_team);
                    }
                } else if let Some(mut t) = text {
                    let next = (0..3).find(|i| !state.objectives[*i]).map(|i| OBJECTIVES[i]).unwrap_or("Mission complete");
                    t.0 = format!("> {next}");
                }
            }
            HudEl::Weapon => {
                if let Some(mut t) = text {
                    t.0 = weapon.map(|(_, w)| weapon_def(w).name.to_uppercase()).unwrap_or_default();
                }
            }
            HudEl::Ammo => {
                if let Some(mut t) = text {
                    t.0 = match weapon {
                        Some((uid, w)) => match weapon_def(w).ammo {
                            Some(a) => {
                                let mag = inv.get(uid).map(|i| i.mag).unwrap_or(0);
                                if ws.reload_t > 0.0 {
                                    "RELOADING".into()
                                } else {
                                    format!("{mag} / {}", inv.ammo[a.idx()])
                                }
                            }
                            None => "-".into(),
                        },
                        None => String::new(),
                    };
                }
            }
            HudEl::Prompt => {
                if let Some(mut t) = text {
                    t.0 = focus.prompt.clone();
                }
            }
            HudEl::PromptSub => {
                if let Some(mut t) = text {
                    t.0 = focus.sub.clone();
                }
            }
            HudEl::HoldBar => {
                if let Some(mut v) = vis {
                    *v = if hold.kind != HoldKind::None { Visibility::Inherited } else { Visibility::Hidden };
                }
            }
            HudEl::HoldFill => {
                if let Some(mut n) = node {
                    n.width = Val::Percent((hold.t / hold.total.max(0.01) * 100.0).min(100.0));
                }
            }
            HudEl::Messages => {
                if let Some(mut t) = text {
                    t.0 = hud.msgs.iter().map(|(m, _)| m.as_str()).collect::<Vec<_>>().join("\n");
                }
            }
            HudEl::Subtitle => {
                if let Some(mut t) = text {
                    t.0 = hud.sub.as_ref().map(|(w, s, _)| format!("{w}: \"{s}\"")).unwrap_or_default();
                }
            }
            HudEl::InfoBox => {
                if let Some(mut v) = vis {
                    *v = if hud.info.is_some() { Visibility::Inherited } else { Visibility::Hidden };
                }
            }
            HudEl::InfoWho => {
                if let (Some(mut t), Some((id, _))) = (text, hud.info) {
                    t.0 = format!("INFOLINK  //  {}", infolink(id).0);
                }
            }
            HudEl::InfoText => {
                if let (Some(mut t), Some((id, t0))) = (text, hud.info) {
                    let full = infolink(id).1;
                    let n = (((now - t0) * 55.0) as usize).min(full.len());
                    let cut = full.char_indices().nth(n).map(|(i, _)| i).unwrap_or(full.len());
                    t.0 = full[..cut].to_string();
                }
            }
            HudEl::Alarm => {
                if let Some(mut v) = vis {
                    *v = if state.alarm && (now * 3.0).sin() > -0.3 { Visibility::Inherited } else { Visibility::Hidden };
                }
            }
            HudEl::DeathBox => {
                if let Some(mut v) = vis {
                    *v = if me.dead { Visibility::Inherited } else { Visibility::Hidden };
                }
            }
            HudEl::DeathText => {
                if let Some(mut t) = text {
                    t.0 = if teams {
                        let by = if me.killer == NO_PLAYER || me.killer == local.0 { "Killed".to_string() } else { format!("Killed by {}", players.name_of(me.killer)) };
                        format!("{by} - back in the fight in {:.0}...", (crate::teams::RESPAWN_TIME - me.dead_t).max(0.0))
                    } else {
                        format!("Recovering in {:.0}...", (5.0 - me.dead_t).max(0.0))
                    };
                }
            }
            HudEl::Belt(i) => {
                let uid = inv.belt[i as usize];
                let selected = uid.is_some() && uid == inv.equipped;
                if let Some(mut b) = border {
                    b.0 = if selected { C_ACCENT } else { Color::srgba(1.0, 1.0, 1.0, 0.1) };
                }
                if let Some(mut b) = bg {
                    b.0 = if selected { Color::srgba(0.25, 0.16, 0.02, 0.6) } else { Color::srgba(0.0, 0.0, 0.0, 0.45) };
                }
            }
            HudEl::BeltText(i) => {
                if let Some(mut t) = text {
                    t.0 = match inv.belt[i as usize].and_then(|u| inv.get(u)) {
                        Some(it) => {
                            if it.count > 1 {
                                format!("{}\nx{}", item_short(it.kind), it.count)
                            } else {
                                item_short(it.kind).to_string()
                            }
                        }
                        None => String::new(),
                    };
                }
            }
            HudEl::Aug(i) => {
                let inst = augs.installed[i as usize];
                let on = augs.active[i as usize];
                if let Some(mut b) = bg {
                    b.0 = if on { Color::srgba(0.05, 0.35, 0.45, 0.75) } else { Color::srgba(0.0, 0.0, 0.0, 0.5) };
                }
                if let Some(mut b) = border {
                    b.0 = if on { C_TEAL } else if inst { C_BORDER } else { Color::srgba(1.0, 1.0, 1.0, 0.05) };
                }
                if let Some(mut v) = vis {
                    *v = if inst { Visibility::Inherited } else { Visibility::Hidden };
                }
            }
            _ => {}
        }
    }
}

fn update_overlays(me: Res<Me>, augs: Res<Augs>, mut q: Query<(&HudEl, &mut Visibility, &mut BackgroundColor)>) {
    for (el, mut vis, mut bg) in q.iter_mut() {
        match el {
            HudEl::Vision => *vis = if augs.on(Aug::Vision) { Visibility::Inherited } else { Visibility::Hidden },
            HudEl::Cloak => *vis = if augs.on(Aug::Cloak) { Visibility::Inherited } else { Visibility::Hidden },
            HudEl::Hurt => bg.0 = Color::srgba(0.6, 0.0, 0.0, (me.hurt * 0.45).min(0.5)),
            _ => {}
        }
    }
}

fn update_crosshair(
    me: Res<Me>,
    ws: Res<WeaponState>,
    inv: Res<Inventory>,
    ui: Res<UiMode>,
    windows: Query<&Window>,
    cam: Query<&Projection, With<MainCamera>>,
    mut q: Query<(&HudEl, &mut Node, &mut Visibility)>,
) {
    let Ok(win) = windows.single() else { return };
    let h = win.height();
    let fov = cam.single().ok().and_then(|p| if let Projection::Perspective(pp) = p { Some(pp.fov) } else { None }).unwrap_or(1.4);
    let weapon = inv.equipped_weapon().map(|e| e.1);
    let gun = matches!(weapon, Some(WeaponKind::Pistol | WeaponKind::Rifle | WeaponKind::Crossbow));
    let px = (ws.spread.to_radians().tan() / (fov * 0.5).tan()) * h * 0.5;
    let gap = px.clamp(3.0, 120.0);
    let show = *ui == UiMode::None && !me.dead;
    for (el, mut n, mut vis) in q.iter_mut() {
        let HudEl::Cross(i) = *el else { continue };
        *vis = if show && (gun || i == 4) { Visibility::Inherited } else { Visibility::Hidden };
        let (dx, dy, w, hh) = match i {
            0 => (-gap - 10.0, -1.0, 10.0, 2.0),
            1 => (gap, -1.0, 10.0, 2.0),
            2 => (-1.0, -gap - 10.0, 2.0, 10.0),
            3 => (-1.0, gap, 2.0, 10.0),
            _ => (-1.0, -1.0, 2.0, 2.0),
        };
        n.margin = UiRect { left: Val::Px(dx), top: Val::Px(dy), ..default() };
        n.width = Val::Px(w);
        n.height = Val::Px(hh);
    }
}

fn button_visuals(mut q: Query<(&Interaction, &UiButton, &mut BackgroundColor), Changed<Interaction>>) {
    for (i, b, mut bg) in q.iter_mut() {
        if !b.enabled {
            continue;
        }
        bg.0 = match i {
            Interaction::Pressed => Color::srgba(0.3, 0.2, 0.03, 0.95),
            Interaction::Hovered => Color::srgba(0.08, 0.25, 0.32, 0.95),
            Interaction::None => Color::srgba(0.05, 0.12, 0.16, 0.95),
        };
    }
}

fn completion_watch(state: Res<WorldState>, mut ui: ResMut<UiMode>, mut dirty: ResMut<UiDirty>, mut seen: Local<bool>) {
    if state.complete.is_some() && !*seen {
        *seen = true;
        *ui = UiMode::Complete;
        dirty.0 = true;
    }
    if state.complete.is_none() {
        *seen = false;
    }
}

// ------------------------------------------------------------------ panels

#[allow(clippy::too_many_arguments)]
fn rebuild_panel(
    mut commands: Commands,
    ui: Res<UiMode>,
    mut dirty: ResMut<UiDirty>,
    mut last: Local<Option<UiMode>>,
    roots: Query<Entity, With<PanelRoot>>,
    data: (Res<Inventory>, Res<Skills>, Res<Augs>, Res<WorldState>, Res<Level>, Res<Settings>),
    sessions: (Res<TermSession>, Res<KeypadState>, Res<TalkState>, Res<HudState>),
    doors: Query<&Door>,
    online: (Res<GameMode>, Res<NetRole>, Res<Players>, Res<LocalId>, Res<crate::net::Net>),
) {
    if last.as_ref() == Some(&*ui) && !dirty.0 {
        return;
    }
    *last = Some(ui.clone());
    dirty.0 = false;
    for e in roots.iter() {
        commands.entity(e).despawn();
    }
    if *ui == UiMode::None {
        return;
    }
    let (inv, skills, augs, state, level, settings) = data;
    let (term, kp, talk, hud) = sessions;
    let root = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
            PanelRoot,
            GlobalZIndex(10),
        ))
        .id();
    commands.entity(root).with_children(|r| {
        r.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(22.0)),
                border: UiRect::all(Val::Px(1.0)),
                min_width: Val::Px(560.0),
                max_width: Val::Px(980.0),
                row_gap: Val::Px(6.0),
                ..default()
            },
            BackgroundColor(C_PANEL),
            BorderColor(C_BORDER),
        ))
        .with_children(|p| match &*ui {
            UiMode::Pause => {
                let (mode, role, players, local, net) = (&online.0, &online.1, &online.2, &online.3, &online.4);
                let title = if **role == NetRole::Offline { "PAUSED" } else { "MENU" };
                txt(p, title, 30.0, C_ACCENT);
                match **role {
                    NetRole::Host => {
                        let humans = players.map.values().filter(|p| !p.bot).count();
                        txt(p, format!("Hosting a team match on port {}  -  {humans} player(s) connected. The game keeps running.", settings.port), 14.0, C_TEAL);
                    }
                    NetRole::Client => {
                        txt(p, format!("Connected to {}  -  the game keeps running.", net.addr), 14.0, C_TEAL);
                    }
                    NetRole::Offline => {}
                }
                button(p, "Resume", UiAction::Resume, true);
                if **mode == GameMode::Teams {
                    let mine = players.team_of(local.0);
                    button(p, format!("Switch to team {}", team_name(other_team(mine))), UiAction::SwitchTeam, mine < 2);
                }
                p.spawn(Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, ..default() }).with_children(|r| {
                    txt(r, format!("Graphics: {}   ", settings.quality.name()), 16.0, C_TEXT);
                    button(r, "Change", UiAction::CycleQuality, true);
                });
                setting_row(p, &format!("Mouse sensitivity: {:.2}", settings.sensitivity), UiAction::Sens(-0.1), UiAction::Sens(0.1));
                setting_row(p, &format!("Volume: {:.0}%", settings.volume * 100.0), UiAction::Vol(-0.1), UiAction::Vol(0.1));
                setting_row(p, &format!("Field of view: {:.0}", settings.fov), UiAction::Fov(-5.0), UiAction::Fov(5.0));
                setting_row(p, &format!("Brightness: {:+.1}", settings.brightness), UiAction::Bright(-0.25), UiAction::Bright(0.25));
                txt(
                    p,
                    "WASD move  SHIFT sprint  C/CTRL crouch  SPACE jump  Q/E lean  F use (hold to pick locks)\nLMB fire  RMB aim  R reload  1-0 belt  L flashlight  H medkit  F1-F6 augmentations  TAB inventory",
                    13.0,
                    C_DIM,
                );
                let quit = match *online.1 {
                    NetRole::Host => "End the match (main menu)",
                    NetRole::Client => "Leave the match (main menu)",
                    NetRole::Offline => "Quit to main menu",
                };
                button(p, quit, UiAction::QuitToMenu, true);
                button(p, "Quit game", UiAction::QuitGame, true);
            }
            UiMode::Inventory(tab) => inventory_panel(p, *tab, &inv, &skills, &augs, &state, &level, &hud),
            UiMode::Terminal(id) => terminal_panel(p, *id, &term, &state, &level, &skills, &doors),
            UiMode::Keypad(id) => {
                txt(p, "KEYPAD", 24.0, C_ACCENT);
                let status = match kp.status {
                    1 => "ACCESS GRANTED",
                    2 => "INVALID CODE",
                    _ => "ENTER CODE",
                };
                txt(p, status, 14.0, if kp.status == 2 { C_RED } else { C_DIM });
                p.spawn((Node { padding: UiRect::all(Val::Px(10.0)), border: UiRect::all(Val::Px(1.0)), justify_content: JustifyContent::Center, ..default() }, BackgroundColor(Color::srgb(0.01, 0.05, 0.03)), BorderColor(C_BORDER)))
                    .with_children(|d| {
                        txt(d, if kp.entry.is_empty() { "_ _ _ _".to_string() } else { kp.entry.clone() }, 30.0, Color::srgb(0.3, 1.0, 0.5));
                    });
                for row in [["1", "2", "3"], ["4", "5", "6"], ["7", "8", "9"], ["C", "0", "#"]] {
                    p.spawn(Node { flex_direction: FlexDirection::Row, justify_content: JustifyContent::Center, ..default() }).with_children(|r| {
                        for k in row {
                            let label = if k == "#" { "ENTER" } else if k == "C" { "CLEAR" } else { k };
                            button(r, label, UiAction::Key(k.chars().next().unwrap()), true);
                        }
                    });
                }
                let codes = known_codes(&level, &state);
                if !codes.is_empty() {
                    txt(p, format!("Codes in your notes: {}", codes.join(", ")), 14.0, C_TEAL);
                }
                let def = &level.doors[*id as usize];
                let n = tools_needed(def.strength, skills.get(Skill::Electronics));
                let have = inv.count(ItemKind::Multitool);
                button(p, format!("Bypass with multitool ({n} needed, have {have})"), UiAction::KeyBypass, have >= n && kp.status != 1);
                button(p, "Close", UiAction::Close, true);
            }
            UiMode::Talk(_) => {
                let node = &INFORMANT[talk.node.min(3) as usize];
                txt(p, "INFORMANT", 20.0, C_ACCENT);
                p.spawn(Node { max_width: Val::Px(760.0), margin: UiRect::vertical(Val::Px(10.0)), ..default() }).with_children(|t| {
                    txt(t, format!("\"{}\"", node.text), 18.0, C_TEXT);
                });
                for (i, (label, _)) in node.choices.iter().enumerate() {
                    button(p, format!("{}. {}", i + 1, label), UiAction::Talk(i as u8), true);
                }
            }
            UiMode::Read(id) => {
                if let Some(n) = level.note(*id) {
                    txt(p, n.title.clone(), 22.0, C_ACCENT);
                    p.spawn(Node { max_width: Val::Px(760.0), margin: UiRect::vertical(Val::Px(10.0)), ..default() }).with_children(|t| {
                        txt(t, n.text.clone(), 16.0, C_TEXT);
                    });
                }
                button(p, "Close", UiAction::Close, true);
            }
            UiMode::AugChoice => {
                txt(p, "AUGMENTATION CANISTER", 24.0, C_ACCENT);
                txt(p, "The nano-surgical unit can install one augmentation. Choose carefully.", 15.0, C_TEXT);
                for (i, a) in AUGS.iter().enumerate() {
                    if !augs.installed[i] {
                        button(p, format!("Install {} - {}", a.name(), a.desc()), UiAction::InstallAug(i as u8), true);
                    }
                }
                button(p, "Not now", UiAction::Close, true);
            }
            UiMode::Complete => {
                txt(p, "MISSION COMPLETE", 34.0, C_TEAL);
                txt(p, "The LAZARUS core is in VEIL hands. Halvorsen-Kade will be looking for whoever did this.", 16.0, C_TEXT);
                if let Some((t, kills, kos, alarms)) = state.complete {
                    txt(p, format!("Time: {:02}:{:02}", (t / 60.0) as u32, (t % 60.0) as u32), 16.0, C_TEXT);
                    txt(p, format!("Kills: {kills}    Knockouts: {kos}    Alarms raised: {alarms}"), 16.0, C_TEXT);
                    let rating = if kills == 0 && alarms == 0 { "GHOST" } else if kills == 0 { "PACIFIST" } else if alarms == 0 { "SILENT" } else { "OPERATIVE" };
                    txt(p, format!("Rating: {rating}"), 20.0, C_ACCENT);
                    txt(p, format!("Notes collected: {}/{}", state.notes.len(), level.notes.len()), 14.0, C_DIM);
                }
                button(p, "Return to main menu", UiAction::QuitToMenu, true);
            }
            UiMode::None => {}
        });
    });
}

fn setting_row(p: &mut ChildSpawnerCommands, label: &str, minus: UiAction, plus: UiAction) {
    p.spawn(Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, ..default() }).with_children(|r| {
        button(r, "-", minus, true);
        button(r, "+", plus, true);
        txt(r, format!("  {label}"), 16.0, C_TEXT);
    });
}

#[allow(clippy::too_many_arguments)]
fn inventory_panel(p: &mut ChildSpawnerCommands, tab: u8, inv: &Inventory, skills: &Skills, augs: &Augs, state: &WorldState, level: &Level, hud: &HudState) {
    p.spawn(Node { flex_direction: FlexDirection::Row, margin: UiRect::bottom(Val::Px(8.0)), ..default() }).with_children(|t| {
        for (i, name) in ["INVENTORY", "AUGMENTATIONS", "SKILLS", "GOALS", "NOTES"].iter().enumerate() {
            let label = if i as u8 == tab { format!("[ {name} ]") } else { name.to_string() };
            button(t, label, UiAction::Tab(i as u8), true);
        }
    });
    match tab {
        0 => {
            let cell = 74.0;
            p.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(20.0), ..default() }).with_children(|row| {
                row.spawn((
                    Node { width: Val::Px(cell * GRID_W as f32), height: Val::Px(cell * GRID_H as f32), border: UiRect::all(Val::Px(1.0)), ..default() },
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.4)),
                    BorderColor(C_BORDER),
                ))
                .with_children(|g| {
                    for y in 0..GRID_H {
                        for x in 0..GRID_W {
                            g.spawn((
                                Node {
                                    position_type: PositionType::Absolute,
                                    left: Val::Px(x as f32 * cell),
                                    top: Val::Px(y as f32 * cell),
                                    width: Val::Px(cell),
                                    height: Val::Px(cell),
                                    border: UiRect::all(Val::Px(1.0)),
                                    ..default()
                                },
                                BorderColor(Color::srgba(1.0, 1.0, 1.0, 0.05)),
                            ));
                        }
                    }
                    for it in &inv.items {
                        let (w, h) = item_size(it.kind);
                        let equipped = inv.equipped == Some(it.uid);
                        let belt = inv.belt.iter().position(|b| *b == Some(it.uid));
                        g.spawn((
                            Button,
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Px(it.gx as f32 * cell + 3.0),
                                top: Val::Px(it.gy as f32 * cell + 3.0),
                                width: Val::Px(w as f32 * cell - 6.0),
                                height: Val::Px(h as f32 * cell - 6.0),
                                border: UiRect::all(Val::Px(1.0)),
                                flex_direction: FlexDirection::Column,
                                padding: UiRect::all(Val::Px(4.0)),
                                ..default()
                            },
                            BackgroundColor(Color::srgba(0.05, 0.12, 0.16, 0.95)),
                            BorderColor(if equipped { C_ACCENT } else { C_BORDER }),
                            UiButton { action: UiAction::UseItem(it.uid), enabled: true },
                        ))
                        .with_children(|b| {
                            txt(b, item_name(it.kind, level), 11.0, C_TEXT);
                            if it.count > 1 {
                                txt(b, format!("x{}", it.count), 12.0, C_ACCENT);
                            }
                            if let ItemKind::Weapon(wk) = it.kind {
                                if weapon_def(wk).ammo.is_some() {
                                    txt(b, format!("{} loaded", it.mag), 11.0, C_DIM);
                                }
                            }
                            if let Some(s) = belt {
                                txt(b, format!("belt {}", (s + 1) % 10), 10.0, C_DIM);
                            }
                        });
                    }
                });
                row.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(4.0), max_width: Val::Px(320.0), ..default() }).with_children(|s| {
                    txt(s, "AMMUNITION", 14.0, C_ACCENT);
                    for a in [AmmoKind::Pistol, AmmoKind::Rifle, AmmoKind::Dart] {
                        txt(s, format!("{}: {}", a.name(), inv.ammo[a.idx()]), 14.0, C_TEXT);
                    }
                    txt(s, "KEYRING", 14.0, C_ACCENT);
                    if inv.keys.is_empty() {
                        txt(s, "(empty)", 14.0, C_DIM);
                    }
                    for k in &inv.keys {
                        txt(s, level.key_name(*k), 14.0, C_TEXT);
                    }
                    txt(s, "Click a weapon to equip it, a consumable to use it.", 12.0, C_DIM);
                    for it in &inv.items {
                        if inv.equipped == Some(it.uid) {
                            txt(s, item_desc(it.kind), 12.0, C_DIM);
                        }
                    }
                });
            });
        }
        1 => {
            for (i, a) in AUGS.iter().enumerate() {
                let status = if augs.active[i] {
                    "ACTIVE"
                } else if augs.installed[i] {
                    "installed"
                } else {
                    "not installed"
                };
                p.spawn(Node { flex_direction: FlexDirection::Column, margin: UiRect::bottom(Val::Px(6.0)), ..default() }).with_children(|r| {
                    txt(r, format!("[F{}] {}  -  {}  ({:.1} energy/s)", i + 1, a.name(), status, a.drain()), 16.0, if augs.installed[i] { C_TEXT } else { C_DIM });
                    txt(r, a.desc(), 13.0, C_DIM);
                });
            }
        }
        2 => {
            txt(p, format!("Skill points available: {}", skills.points), 18.0, C_ACCENT);
            for (i, s) in SKILLS.iter().enumerate() {
                let lvl = skills.get(*s);
                p.spawn(Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: Val::Px(10.0), ..default() }).with_children(|r| {
                    r.spawn(Node { width: Val::Px(150.0), ..default() }).with_children(|c| {
                        txt(c, s.name(), 16.0, C_TEXT);
                    });
                    r.spawn(Node { width: Val::Px(110.0), ..default() }).with_children(|c| {
                        txt(c, SKILL_LEVELS[lvl as usize], 14.0, C_TEAL);
                    });
                    if lvl < 3 {
                        let cost = s.cost(lvl + 1);
                        button(r, format!("Upgrade ({cost})"), UiAction::Upgrade(i as u8), skills.points >= cost);
                    } else {
                        txt(r, "MAXED", 14.0, C_DIM);
                    }
                    txt(r, s.desc(), 12.0, C_DIM);
                });
            }
        }
        3 => {
            txt(p, "MISSION: PORT MERIDIAN", 18.0, C_ACCENT);
            txt(p, "Halvorsen-Kade is moving the LAZARUS research core out of its harbour biolab at dawn. Get it first.", 14.0, C_DIM);
            for (i, o) in OBJECTIVES.iter().enumerate() {
                let done = state.objectives[i];
                txt(p, format!("{} {}", if done { "[X]" } else { "[ ]" }, o), 16.0, if done { C_DIM } else { C_TEXT });
            }
        }
        _ => {
            if state.notes.is_empty() {
                txt(p, "No notes yet. Read datacubes and computers, and talk to people.", 14.0, C_DIM);
            }
            p.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(16.0), ..default() }).with_children(|r| {
                r.spawn(Node { flex_direction: FlexDirection::Column, ..default() }).with_children(|l| {
                    for n in &state.notes {
                        if let Some(d) = level.note(*n) {
                            button(l, d.title.clone(), UiAction::ReadNote(*n), true);
                        }
                    }
                });
                if let Some(sel) = hud.note_sel.and_then(|s| level.note(s)) {
                    r.spawn(Node { flex_direction: FlexDirection::Column, max_width: Val::Px(520.0), ..default() }).with_children(|t| {
                        txt(t, sel.title.clone(), 16.0, C_ACCENT);
                        txt(t, sel.text.clone(), 14.0, C_TEXT);
                    });
                }
            });
        }
    }
    button(p, "Close  [TAB]", UiAction::Close, true);
}

fn terminal_panel(p: &mut ChildSpawnerCommands, id: u16, term: &TermSession, state: &WorldState, level: &Level, skills: &Skills, doors: &Query<&Door>) {
    let t = &level.terminals[id as usize];
    let sec = t.kind == TermKind::Security;
    txt(p, if sec { "HALVORSEN-KADE // SECURITY CONSOLE" } else { "HALVORSEN-KADE // WORKSTATION" }, 22.0, if sec { C_ACCENT } else { C_TEAL });
    if !term.access {
        if term.hacking {
            let pct = (term.hack_t / term.hack_total.max(0.01) * 100.0).min(100.0);
            txt(p, format!("BYPASSING SECURITY... {pct:.0}%"), 18.0, C_TEXT);
            p.spawn((Node { width: Val::Px(400.0), height: Val::Px(10.0), ..default() }, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)))).with_children(|b| {
                b.spawn((Node { width: Val::Percent(pct), height: Val::Percent(100.0), ..default() }, BackgroundColor(C_TEAL)));
            });
        } else {
            txt(p, "LOGIN REQUIRED", 16.0, C_DIM);
            if let Some((user, _)) = &t.login {
                let known = known_login(level, state, user);
                button(p, if known { format!("Log in as '{user}' (from your notes)") } else { "Log in (credentials unknown)".into() }, UiAction::TermLogin, known);
            }
            let comp = skills.get(Skill::Computer);
            let can = comp + 1 >= t.hack;
            let time = 6.0 * t.hack as f32 / (1.0 + comp as f32);
            button(
                p,
                if can { format!("Hack  (Computer: {}  ~{time:.0}s)", SKILL_LEVELS[comp as usize]) } else { format!("Hack  (requires Computer: {})", SKILL_LEVELS[(t.hack - 1).min(3) as usize]) },
                UiAction::TermHack,
                can,
            );
        }
    } else {
        let who = if term.trace >= 0.0 { format!("USER: {}   TRACE IN {:.0}s", term.user, term.trace.max(0.0)) } else { format!("USER: {}", term.user) };
        txt(p, who, 14.0, if term.trace >= 0.0 { C_RED } else { C_DIM });
        if let Some(note) = term.reading {
            if let Some(n) = level.note(note) {
                txt(p, n.title.clone(), 18.0, C_ACCENT);
                p.spawn(Node { max_width: Val::Px(760.0), ..default() }).with_children(|b| {
                    txt(b, n.text.clone(), 15.0, C_TEXT);
                });
            }
            button(p, "Back", UiAction::TermBack, true);
        } else if sec {
            txt(p, "SECURITY SYSTEMS", 15.0, C_ACCENT);
            button(p, format!("Cameras: {}  (toggle)", if state.cameras_on { "ONLINE" } else { "OFFLINE" }), UiAction::TermCams, true);
            p.spawn(Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, ..default() }).with_children(|r| {
                txt(r, "Turrets: ", 15.0, C_TEXT);
                for (m, name) in [(0u8, "OFF"), (1, "STANDARD"), (2, "TARGET HOSTILES")] {
                    let label = if state.turret_mode == m { format!("[{name}]") } else { name.to_string() };
                    button(r, label, UiAction::TermTurrets(m), true);
                }
            });
            txt(p, "DOORS", 15.0, C_ACCENT);
            for d in level.doors.iter().filter(|d| d.kind == DoorKind::Security) {
                let open = doors.iter().find(|x| x.id == d.id).map(|x| x.open).unwrap_or(false);
                button(p, format!("Lab security door ({}, {}): {}", d.x, d.z, if open { "OPEN - close it" } else { "LOCKED - open it" }), UiAction::TermDoor(d.id, !open), true);
            }
        } else {
            txt(p, "MAIL", 15.0, C_ACCENT);
            if t.mails.is_empty() {
                txt(p, "(no messages)", 14.0, C_DIM);
            }
            for m in &t.mails {
                if let Some(n) = level.note(*m) {
                    button(p, n.title.clone(), UiAction::TermMail(*m), true);
                }
            }
        }
    }
    button(p, "Log off", UiAction::Close, true);
}

// ------------------------------------------------------------------ actions

#[allow(clippy::too_many_arguments)]
fn game_actions(
    q: Query<(&Interaction, &UiButton), Changed<Interaction>>,
    mut ui: ResMut<UiMode>,
    mut dirty: ResMut<UiDirty>,
    mut player: (ResMut<Inventory>, ResMut<Skills>, ResMut<Augs>, ResMut<Me>),
    mut sessions: (ResMut<TermSession>, ResMut<KeypadState>, ResMut<TalkState>, ResMut<HudState>),
    mut settings: ResMut<Settings>,
    level: Option<Res<Level>>,
    state: Res<WorldState>,
    mut req: EventWriter<WorldRequest>,
    mut snd: EventWriter<SoundEv>,
    mut msgs: EventWriter<HudMsg>,
    mut next: ResMut<NextState<AppState>>,
    mut exit: EventWriter<AppExit>,
) {
    let (inv, skills, augs, me) = (&mut player.0, &mut player.1, &mut player.2, &mut player.3);
    let (term, kp, talk, hud) = (&mut sessions.0, &mut sessions.1, &mut sessions.2, &mut sessions.3);
    for (i, b) in q.iter() {
        if *i != Interaction::Pressed || !b.enabled {
            continue;
        }
        snd.write(SoundEv::ui(Sfx::Click, 0.5));
        dirty.0 = true;
        match b.action {
            UiAction::Close | UiAction::Resume => *ui = UiMode::None,
            UiAction::QuitToMenu => {
                *ui = UiMode::None;
                next.set(AppState::Menu);
            }
            UiAction::QuitGame | UiAction::MenuQuit => {
                exit.write(AppExit::Success);
            }
            UiAction::CycleQuality => settings.quality = settings.quality.next(),
            UiAction::Sens(d) => settings.sensitivity = (settings.sensitivity + d).clamp(0.1, 5.0),
            UiAction::Vol(d) => settings.volume = (settings.volume + d).clamp(0.0, 1.0),
            UiAction::Fov(d) => settings.fov = (settings.fov + d).clamp(60.0, 110.0),
            UiAction::Bright(d) => settings.brightness = (settings.brightness + d).clamp(-1.5, 2.5),
            UiAction::Tab(t) => *ui = UiMode::Inventory(t),
            UiAction::UseItem(uid) => {
                let Some(kind) = inv.get(uid).map(|i| i.kind) else { continue };
                match kind {
                    ItemKind::Weapon(_) => {
                        inv.equipped = if inv.equipped == Some(uid) { None } else { Some(uid) };
                        if !inv.belt.contains(&Some(uid)) {
                            if let Some(slot) = inv.belt.iter().position(|b| b.is_none()) {
                                inv.belt[slot] = Some(uid);
                            }
                        }
                    }
                    other => {
                        let mut mode = ui.clone();
                        use_consumable(other, me, inv, skills, &mut snd, &mut msgs, &mut mode);
                        if mode != *ui {
                            *ui = mode;
                        }
                    }
                }
            }
            UiAction::Upgrade(i) => {
                if let Some(s) = SKILLS.get(i as usize) {
                    if skills.try_upgrade(*s) {
                        msgs.write(HudMsg(format!("{} is now {}", s.name(), SKILL_LEVELS[skills.get(*s) as usize])));
                    }
                }
            }
            UiAction::ReadNote(n) => hud.note_sel = Some(n),
            UiAction::TermLogin => {
                if let Some(level) = &level {
                    if let Some((user, _)) = &level.terminals[term.id as usize].login {
                        term.access = true;
                        term.trace = -1.0;
                        term.user = user.clone();
                        snd.write(SoundEv::ui(Sfx::Granted, 0.6));
                    }
                }
            }
            UiAction::TermHack => {
                if let Some(level) = &level {
                    let t = &level.terminals[term.id as usize];
                    let comp = skills.get(Skill::Computer) as f32;
                    term.hacking = true;
                    term.hack_t = 0.0;
                    term.hack_total = 6.0 * t.hack as f32 / (1.0 + comp);
                    term.trace = 15.0 + comp * 12.0;
                }
            }
            UiAction::TermCams => {
                req.write(WorldRequest::SetCameras { on: !state.cameras_on });
            }
            UiAction::TermTurrets(m) => {
                req.write(WorldRequest::SetTurrets { mode: m });
            }
            UiAction::TermDoor(id, open) => {
                if open {
                    req.write(WorldRequest::Unlock { id });
                } else {
                    req.write(WorldRequest::SetDoor { id, open: false });
                }
            }
            UiAction::TermMail(n) => {
                term.reading = Some(n);
                req.write(WorldRequest::ReadNote { id: n });
            }
            UiAction::TermBack => term.reading = None,
            UiAction::Key(c) => {
                if let Some(level) = &level {
                    keypad_press(kp, c, &mut snd, &mut req, level);
                }
            }
            UiAction::KeyBypass => {
                if let Some(level) = &level {
                    let def = &level.doors[kp.door as usize];
                    let n = tools_needed(def.strength, skills.get(Skill::Electronics));
                    if inv.consume(ItemKind::Multitool, n) {
                        kp.status = 1;
                        req.write(WorldRequest::Unlock { id: kp.door });
                        snd.write(SoundEv::ui(Sfx::Granted, 0.6));
                        msgs.write(HudMsg(format!("Keypad bypassed ({n} multitool{} used)", if n > 1 { "s" } else { "" })));
                    }
                }
            }
            UiAction::Talk(c) => {
                let node = &INFORMANT[talk.node.min(3) as usize];
                if let Some((_, act)) = node.choices.get(c as usize) {
                    match act {
                        DlgAct::Goto(n) => talk.node = *n,
                        DlgAct::GiveCode(n) => {
                            req.write(WorldRequest::ReadNote { id: 1 });
                            if *n == 255 {
                                *ui = UiMode::None;
                            } else {
                                talk.node = *n;
                            }
                        }
                        DlgAct::End => *ui = UiMode::None,
                    }
                }
            }
            UiAction::InstallAug(i) => {
                if inv.consume(ItemKind::AugCanister, 1) {
                    augs.installed[i as usize] = true;
                    msgs.write(HudMsg(format!("{} installed (F{})", AUGS[i as usize].name(), i + 1)));
                    snd.write(SoundEv::ui(Sfx::AugOn, 0.7));
                }
                *ui = UiMode::None;
            }
            UiAction::SwitchTeam => {
                req.write(WorldRequest::SwitchTeam);
                *ui = UiMode::None;
            }
            UiAction::MenuStart
            | UiAction::MenuSkirmish
            | UiAction::MenuHost
            | UiAction::MenuJoin
            | UiAction::EditName
            | UiAction::EditAddr
            | UiAction::TeamSize(_)
            | UiAction::CycleTeam
            | UiAction::CancelConnect => {}
        }
    }
}
