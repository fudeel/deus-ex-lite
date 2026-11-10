//! Main menu (drawn over a slow cinematic pass through the level): campaign, team match
//! (offline against bots, host online, join online) and the "connecting..." screen.
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;

use crate::bots::Bots;
use crate::dxassets::DxAssets;
use crate::game::*;
use crate::hud::*;
use crate::items::Skills;
use crate::level::*;
use crate::net::Net;
use crate::teams::{roll_round, MatchState};
use crate::util::GameRng;

#[derive(Component)]
pub struct MenuRoot;

#[derive(Component)]
pub struct ConnectRoot;

#[derive(Component)]
pub struct ConnectText;

/// Which text field of the menu is being typed into.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum EditField {
    #[default]
    None,
    Name,
    Addr,
}

/// Start playing. Sent by the menu buttons and by the dev command line flags.
#[derive(Event, Clone, Copy, PartialEq, Eq, Debug)]
pub enum StartGame {
    Campaign,
    Skirmish,
    Host,
    Join,
}

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EditField>()
            .add_event::<StartGame>()
            .add_systems(OnEnter(AppState::Menu), spawn_menu)
            .add_systems(OnExit(AppState::Menu), despawn_menu)
            .add_systems(OnEnter(AppState::Connecting), spawn_connecting)
            .add_systems(OnExit(AppState::Connecting), despawn_connecting)
            .add_systems(Update, (menu_actions, menu_typing, refresh_menu).chain().run_if(in_state(AppState::Menu)))
            .add_systems(Update, connecting_ui.run_if(in_state(AppState::Connecting)))
            .add_systems(Update, start_game);
    }
}

fn despawn_menu(mut commands: Commands, q: Query<Entity, With<MenuRoot>>) {
    for e in q.iter() {
        commands.entity(e).despawn();
    }
}

fn section(c: &mut bevy::ecs::hierarchy::ChildSpawnerCommands, title: &str) {
    c.spawn(Node { height: Val::Px(14.0), ..default() });
    txt(c, title, 14.0, C_TEAL);
}

fn field(c: &mut bevy::ecs::hierarchy::ChildSpawnerCommands, label: &str, value: &str, editing: bool, action: UiAction) {
    c.spawn(Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, ..default() }).with_children(|r| {
        r.spawn(Node { width: Val::Px(92.0), ..default() }).with_children(|l| {
            txt(l, label, 14.0, C_DIM);
        });
        let shown = if editing { format!("{value}_") } else { value.to_string() };
        button(r, shown, action, true);
        if editing {
            txt(r, "  type, ENTER to confirm", 12.0, C_ACCENT);
        }
    });
}

fn spawn_menu(mut commands: Commands, settings: Res<Settings>, hud: Res<HudState>, net: Res<Net>, edit: Res<EditField>, dxa: Res<DxAssets>) {
    build_menu(&mut commands, &settings, &hud, &net, *edit, &dxa);
}

fn build_menu(commands: &mut Commands, settings: &Settings, hud: &HudState, net: &Net, edit: EditField, dxa: &DxAssets) {
    let full = Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() };
    commands.spawn((full.clone(), MenuRoot)).with_children(|r| {
        if let Some(v) = &hud.vignette {
            r.spawn((full.clone(), ImageNode::new(v.clone())));
        }
        r.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Px(620.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                padding: UiRect::left(Val::Px(70.0)),
                row_gap: Val::Px(4.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.01, 0.02, 0.6)),
        ))
        .with_children(|c| {
            txt(c, "DEUSMOBILE", 64.0, C_ACCENT);
            txt(c, "PORT MERIDIAN  //  2071", 18.0, C_TEAL);

            section(c, "CAMPAIGN");
            txt(c, "Halvorsen-Kade is moving something out of its harbour biolab before dawn.\nVEIL sent you to take it first. Stay in the dark.", 13.0, C_DIM);
            button(c, "START MISSION", UiAction::MenuStart, true);

            section(c, "TEAM MATCH  -  ORANGE vs BLUE");
            txt(c, "Two stealth teams. Each round: steal the prototype, or hack 2 of 3 terminals.\nObjectives appear at random - you have to find them.", 13.0, C_DIM);
            field(c, "Name", &settings.name, edit == EditField::Name, UiAction::EditName);
            c.spawn(Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, ..default() }).with_children(|r| {
                r.spawn(Node { width: Val::Px(92.0), ..default() }).with_children(|l| {
                    txt(l, "Team size", 14.0, C_DIM);
                });
                button(r, "-", UiAction::TeamSize(-1), settings.team_size > 1);
                txt(r, format!("  {} vs {}  ", settings.team_size, settings.team_size), 15.0, C_TEXT);
                button(r, "+", UiAction::TeamSize(1), settings.team_size < 6);
                txt(r, "   bots fill empty slots", 12.0, C_DIM);
            });
            c.spawn(Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, ..default() }).with_children(|r| {
                r.spawn(Node { width: Val::Px(92.0), ..default() }).with_children(|l| {
                    txt(l, "Team", 14.0, C_DIM);
                });
                let label = if settings.pref_team < 2 { team_name(settings.pref_team) } else { "AUTO" };
                button(r, label, UiAction::CycleTeam, true);
            });
            c.spawn(Node { flex_direction: FlexDirection::Row, flex_wrap: FlexWrap::Wrap, ..default() }).with_children(|r| {
                button(r, "PLAY OFFLINE (BOTS)", UiAction::MenuSkirmish, true);
                button(r, format!("HOST ONLINE (port {})", settings.port), UiAction::MenuHost, true);
            });
            field(c, "Join", &settings.join_addr, edit == EditField::Addr, UiAction::EditAddr);
            button(c, "JOIN ONLINE GAME", UiAction::MenuJoin, !settings.join_addr.trim().is_empty());
            txt(c, format!("Hosting: friends join with your IP (LAN, or forward TCP port {} on your router).", settings.port), 12.0, C_DIM);

            c.spawn(Node { height: Val::Px(14.0), ..default() });
            c.spawn(Node { flex_direction: FlexDirection::Row, ..default() }).with_children(|r| {
                button(r, format!("GRAPHICS: {}", settings.quality.name()), UiAction::CycleQuality, true);
                button(r, "QUIT", UiAction::MenuQuit, true);
            });
            if !net.status.is_empty() {
                c.spawn(Node { height: Val::Px(8.0), ..default() });
                txt(c, net.status.clone(), 15.0, C_RED);
            }
            match &dxa.root {
                Some(root) => txt(
                    c,
                    format!(
                        "Original Deus Ex assets: {} textures, {} sounds, {} characters from {}",
                        dxa.textures,
                        dxa.sounds,
                        dxa.models.len(),
                        root.display()
                    ),
                    12.0,
                    C_DIM,
                ),
                None => txt(c, "Procedural art. Install Deus Ex (or run with --dx PATH) to use its textures, sounds and characters.", 12.0, C_DIM),
            };
        });
    });
}

/// Rebuild the menu whenever something it shows changes.
#[allow(clippy::too_many_arguments)]
fn refresh_menu(
    mut commands: Commands,
    settings: Res<Settings>,
    q: Query<Entity, With<MenuRoot>>,
    hud: Res<HudState>,
    net: Res<Net>,
    edit: Res<EditField>,
    dxa: Res<DxAssets>,
    mut last: Local<String>,
) {
    let sig = format!(
        "{}|{}|{}|{}|{}|{}|{:?}|{}",
        settings.quality.name(),
        settings.name,
        settings.join_addr,
        settings.team_size,
        settings.pref_team,
        settings.port,
        *edit,
        net.status
    );
    if *last == sig {
        return;
    }
    let first = last.is_empty();
    *last = sig;
    if first {
        return;
    }
    for e in q.iter() {
        commands.entity(e).despawn();
    }
    build_menu(&mut commands, &settings, &hud, &net, *edit, &dxa);
}

fn menu_actions(q: Query<(&Interaction, &UiButton), Changed<Interaction>>, mut settings: ResMut<Settings>, mut edit: ResMut<EditField>, mut start: EventWriter<StartGame>) {
    for (i, b) in q.iter() {
        if *i != Interaction::Pressed || !b.enabled {
            continue;
        }
        let was = *edit;
        *edit = EditField::None;
        match b.action {
            UiAction::MenuStart => {
                start.write(StartGame::Campaign);
            }
            UiAction::MenuSkirmish => {
                start.write(StartGame::Skirmish);
            }
            UiAction::MenuHost => {
                start.write(StartGame::Host);
            }
            UiAction::MenuJoin => {
                start.write(StartGame::Join);
            }
            UiAction::EditName => *edit = if was == EditField::Name { EditField::None } else { EditField::Name },
            UiAction::EditAddr => *edit = if was == EditField::Addr { EditField::None } else { EditField::Addr },
            UiAction::TeamSize(d) => settings.team_size = (settings.team_size as i8 + d).clamp(1, 6) as u8,
            UiAction::CycleTeam => {
                settings.pref_team = match settings.pref_team {
                    TEAM_ORANGE => TEAM_BLUE,
                    TEAM_BLUE => NO_TEAM,
                    _ => TEAM_ORANGE,
                }
            }
            _ => {}
        }
    }
}

fn menu_typing(mut evs: EventReader<KeyboardInput>, mut edit: ResMut<EditField>, mut settings: ResMut<Settings>) {
    if *edit == EditField::None {
        evs.clear();
        return;
    }
    for e in evs.read() {
        if !e.state.is_pressed() {
            continue;
        }
        match &e.logical_key {
            Key::Enter | Key::Escape | Key::Tab => {
                *edit = EditField::None;
                continue;
            }
            Key::Backspace => {
                match *edit {
                    EditField::Name => {
                        settings.name.pop();
                    }
                    EditField::Addr => {
                        settings.join_addr.pop();
                    }
                    EditField::None => {}
                }
                continue;
            }
            _ => {}
        }
        let Some(text) = &e.text else { continue };
        for ch in text.chars() {
            if ch.is_control() {
                continue;
            }
            match *edit {
                EditField::Name if settings.name.chars().count() < 20 => settings.name.push(ch),
                EditField::Addr if settings.join_addr.len() < 64 && !ch.is_whitespace() => settings.join_addr.push(ch),
                _ => {}
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn start_game(
    mut commands: Commands,
    mut evs: EventReader<StartGame>,
    mut level: ResMut<Level>,
    settings: Res<Settings>,
    mut mode: ResMut<GameMode>,
    mut role: ResMut<NetRole>,
    mut local: ResMut<LocalId>,
    mut players: ResMut<Players>,
    mut ms: ResMut<MatchState>,
    mut net: ResMut<Net>,
    mut bots: ResMut<Bots>,
    mut rng: ResMut<GameRng>,
    mut next: ResMut<NextState<AppState>>,
    mut edit: ResMut<EditField>,
) {
    let Some(kind) = evs.read().last().copied() else { return };
    *edit = EditField::None;
    net.status.clear();
    bots.list.clear();
    players.map.clear();
    local.0 = 0;
    commands.insert_resource(Skills::default());
    match kind {
        StartGame::Campaign => {
            if level.name != CAMPAIGN_LEVEL {
                *level = load_level(CAMPAIGN_LEVEL);
            }
            net.shutdown();
            *mode = GameMode::Campaign;
            *role = NetRole::Offline;
            next.set(AppState::InGame);
        }
        StartGame::Skirmish | StartGame::Host => {
            if level.name != TEAM_LEVEL {
                *level = load_level(TEAM_LEVEL);
            }
            if !level.is_team_map() {
                net.status = format!("assets/levels/{TEAM_LEVEL}.txt has no team spawns / objective spots");
                return;
            }
            if kind == StartGame::Host {
                if let Err(e) = net.host(settings.port) {
                    net.status = e;
                    return;
                }
                *role = NetRole::Host;
            } else {
                net.shutdown();
                *role = NetRole::Offline;
            }
            *mode = GameMode::Teams;
            let team = if settings.pref_team < 2 { settings.pref_team } else { TEAM_ORANGE };
            players.map.insert(0, PlayerEntry::new(settings.name.clone(), team, false));
            ms.begin_round(roll_round(&level, &mut rng.0, 1, [0, 0]));
            ms.feed.clear();
            next.set(AppState::InGame);
        }
        StartGame::Join => {
            if level.name != TEAM_LEVEL {
                *level = load_level(TEAM_LEVEL);
            }
            *mode = GameMode::Teams;
            *role = NetRole::Client;
            ms.feed.clear();
            net.connect(&settings.join_addr);
            next.set(AppState::Connecting);
        }
    }
}

fn spawn_connecting(mut commands: Commands, net: Res<Net>) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
            ConnectRoot,
        ))
        .with_children(|r| {
            r.spawn((
                Node { flex_direction: FlexDirection::Column, padding: UiRect::all(Val::Px(22.0)), border: UiRect::all(Val::Px(1.0)), row_gap: Val::Px(8.0), ..default() },
                BackgroundColor(C_PANEL),
                BorderColor(C_BORDER),
            ))
            .with_children(|p| {
                txt(p, "TEAM MATCH", 24.0, C_ACCENT);
                p.spawn((Text::new(format!("Connecting to {} ...", net.addr)), TextFont { font_size: 16.0, ..default() }, TextColor(C_TEXT), ConnectText));
                button(p, "Cancel", UiAction::CancelConnect, true);
            });
        });
}

fn despawn_connecting(mut commands: Commands, q: Query<Entity, With<ConnectRoot>>) {
    for e in q.iter() {
        commands.entity(e).despawn();
    }
}

fn connecting_ui(
    q: Query<(&Interaction, &UiButton), Changed<Interaction>>,
    mut texts: Query<&mut Text, With<ConnectText>>,
    mut net: ResMut<Net>,
    mut role: ResMut<NetRole>,
    mut next: ResMut<NextState<AppState>>,
) {
    for mut t in texts.iter_mut() {
        let s = format!("Connecting to {} ...  {:.0}s", net.addr, net.connect_t);
        if t.0 != s {
            t.0 = s;
        }
    }
    for (i, b) in q.iter() {
        if *i == Interaction::Pressed && b.action == UiAction::CancelConnect {
            net.shutdown();
            net.status = "Connection cancelled".into();
            *role = NetRole::Offline;
            next.set(AppState::Menu);
        }
    }
}
