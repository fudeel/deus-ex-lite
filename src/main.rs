//! DEUSMOBILE - a stealth / action immersive sim in the spirit of the classic cyberpunk spy RPGs.
//!
//! Dev flags (useful for testing):
//!   --start                 skip the menu and start the mission
//!   --tp X,Z[,YAW,PITCH]    start at a world position (meters) / view angles (degrees)
//!   --quality low|medium|high|ultra
//!   --shot PATH [--shot-delay SECS]   save a screenshot then quit
//!   --flashlight --vision   start with the flashlight / vision augmentation on
//!
//! Original Deus Ex assets (textures, sounds, characters) are used when the game is
//! installed; see `dxassets.rs`:
//!   --dx PATH               the Deus Ex folder (default: DEUSEX_DIR or the usual Steam / GOG paths)
//!   --no-dx                 always use the procedural assets
//!
//! Team match (Orange vs Blue):
//!   --skirmish              start an offline team match against bots
//!   --host                  host an online team match (port 27777)
//!   --join ADDR             join an online team match (e.g. 192.168.1.20 or host:port)
//!   --name NAME --team orange|blue --team-size N
// Some protocol types (guard / device snapshots) are kept for a future co-op campaign.
#![allow(dead_code)]

mod audio;
mod avatars;
mod bots;
mod collision;
mod dx;
mod dxassets;
mod effects;
mod game;
mod hud;
mod interact;
mod items;
mod level;
mod materials;
mod menu;
mod net;
mod npc;
mod player;
mod render;
mod teams;
mod util;
mod weapons;
mod wire;
mod world;

use bevy::audio::{AudioPlugin, SpatialScale};
use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use bevy::window::PresentMode;

use game::*;

#[derive(Resource, Default, Clone)]
struct DevArgs {
    start: bool,
    tp: Option<(f32, f32, f32, f32)>,
    shot: Option<String>,
    shot_delay: f32,
    flashlight: bool,
    vision: bool,
    panel: Option<String>,
    /// team match started from the command line
    team_start: Option<menu::StartGame>,
}

#[derive(Default)]
struct NetArgs {
    name: Option<String>,
    join: Option<String>,
    team: Option<u8>,
    team_size: Option<u8>,
}

fn parse_args() -> (DevArgs, Option<Quality>, NetArgs, dxassets::DxConfig) {
    let args: Vec<String> = std::env::args().collect();
    let mut d = DevArgs { shot_delay: 5.0, ..default() };
    let mut q = None;
    let mut n = NetArgs::default();
    let mut dxc = dxassets::DxConfig::default();
    let mut i = 1;
    while i < args.len() {
        let next = args.get(i + 1).cloned();
        match args[i].as_str() {
            "--start" => d.start = true,
            "--skirmish" => d.team_start = Some(menu::StartGame::Skirmish),
            "--host" => d.team_start = Some(menu::StartGame::Host),
            "--join" => {
                n.join = next;
                d.team_start = Some(menu::StartGame::Join);
                i += 1;
            }
            "--name" => {
                n.name = next;
                i += 1;
            }
            "--team" => {
                n.team = match next.as_deref() {
                    Some("orange") => Some(TEAM_ORANGE),
                    Some("blue") => Some(TEAM_BLUE),
                    _ => None,
                };
                i += 1;
            }
            "--team-size" => {
                n.team_size = next.and_then(|v| v.parse().ok());
                i += 1;
            }
            "--dx" => {
                dxc.path = next;
                i += 1;
            }
            "--no-dx" => dxc.off = true,
            "--flashlight" => d.flashlight = true,
            "--vision" => d.vision = true,
            "--panel" => {
                d.panel = next;
                i += 1;
            }
            "--tp" => {
                if let Some(v) = next {
                    let n: Vec<f32> = v.split(',').filter_map(|s| s.trim().parse().ok()).collect();
                    if n.len() >= 2 {
                        d.tp = Some((n[0], n[1], *n.get(2).unwrap_or(&0.0), *n.get(3).unwrap_or(&0.0)));
                    }
                    i += 1;
                }
            }
            "--shot" => {
                d.shot = next;
                i += 1;
            }
            "--shot-delay" => {
                d.shot_delay = next.and_then(|v| v.parse().ok()).unwrap_or(5.0);
                i += 1;
            }
            "--quality" => {
                q = match next.as_deref() {
                    Some("low") => Some(Quality::Low),
                    Some("medium") => Some(Quality::Medium),
                    Some("high") => Some(Quality::High),
                    Some("ultra") => Some(Quality::Ultra),
                    _ => None,
                };
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    (d, q, n, dxc)
}

fn main() {
    let (dev, quality, netargs, dxconfig) = parse_args();
    let mut settings = Settings::default();
    if let Some(q) = quality {
        settings.quality = q;
    }
    if let Some(name) = netargs.name {
        settings.name = name;
    }
    if let Some(addr) = netargs.join {
        settings.join_addr = addr;
    }
    if let Some(t) = netargs.team {
        settings.pref_team = t;
    }
    if let Some(n) = netargs.team_size {
        settings.team_size = n.clamp(1, 6);
    }
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "DEUSMOBILE".into(),
                        resolution: (1600.0_f32, 900.0_f32).into(),
                        present_mode: PresentMode::AutoVsync,
                        ..default()
                    }),
                    ..default()
                })
                .set(AudioPlugin { default_spatial_scale: SpatialScale::new(0.18), ..default() }),
        )
        .insert_resource(settings)
        .insert_resource(dev)
        .insert_resource(dxconfig)
        .init_state::<AppState>()
        .init_resource::<UiMode>()
        .init_resource::<UiDirty>()
        .init_resource::<NetRole>()
        .init_resource::<GameMode>()
        .init_resource::<LocalId>()
        .init_resource::<Players>()
        .init_resource::<util::GameRng>()
        .add_event::<WorldRequest>()
        .add_event::<AuthRequest>()
        .add_event::<WorldEv>()
        .add_event::<FxEv>()
        .add_event::<DamageEv>()
        .add_event::<PlayerDamage>()
        .add_event::<HudMsg>()
        .add_event::<Subtitle>()
        .add_event::<audio::SoundEv>()
        .configure_sets(Update, (GameSet::Input, GameSet::Logic, GameSet::Route, GameSet::Authority, GameSet::Apply, GameSet::Present, GameSet::Net).chain())
        .add_plugins((
            render::RenderPlugin,
            world::WorldPlugin,
            player::PlayerPlugin,
            weapons::WeaponsPlugin,
            npc::NpcPlugin,
            interact::InteractPlugin,
            effects::EffectsPlugin,
            hud::HudPlugin,
            menu::MenuPlugin,
            net::NetPlugin,
            teams::TeamsPlugin,
            bots::BotsPlugin,
            avatars::AvatarPlugin,
            dxassets::DxPlugin,
        ))
        .add_systems(Startup, (load_level_res, materials::setup_materials, audio::setup_audio))
        .add_systems(Update, (audio::fx_sounds, audio::play_sounds).chain().in_set(GameSet::Net))
        .add_systems(Update, audio::update_loops.in_set(GameSet::Present))
        .add_systems(Update, boot_to_menu.run_if(in_state(AppState::Boot)))
        .add_systems(Update, dev_tools)
        .run();
}

fn load_level_res(mut commands: Commands) {
    commands.insert_resource(level::load_level("mission01"));
}

fn boot_to_menu(mut next: ResMut<NextState<AppState>>) {
    next.set(AppState::Menu);
}

#[allow(clippy::too_many_arguments)]
fn dev_tools(
    mut commands: Commands,
    dev: Res<DevArgs>,
    time: Res<Time<Real>>,
    state: Res<State<AppState>>,
    mut next: ResMut<NextState<AppState>>,
    mut me: ResMut<player::Me>,
    mut augs: ResMut<items::Augs>,
    mut ui: ResMut<UiMode>,
    level: Res<level::Level>,
    mut exit: EventWriter<AppExit>,
    mut t_state: Local<(f32, bool, bool, f32)>,
    mut fps: Local<(u32, f32)>,
    mut start: EventWriter<menu::StartGame>,
    mut team_started: Local<bool>,
) {
    let (ref mut t_in, ref mut placed, ref mut shot_taken, ref mut shot_t) = *t_state;
    match state.get() {
        AppState::Menu if dev.team_start.is_some() && !*team_started => {
            *team_started = true;
            if let Some(kind) = dev.team_start {
                start.write(kind);
            }
        }
        AppState::Menu if dev.start => next.set(AppState::InGame),
        AppState::InGame => {
            *t_in += time.delta_secs();
            if !*placed && *t_in > 0.2 {
                *placed = true;
                if let Some((x, z, yaw, pitch)) = dev.tp {
                    me.feet = Vec3::new(x, 0.05, z);
                    me.yaw = yaw.to_radians();
                    me.pitch = pitch.to_radians();
                }
                if dev.flashlight {
                    me.flashlight = true;
                }
                if dev.vision {
                    augs.active[items::Aug::Vision as usize] = true;
                }
                if let Some(p) = &dev.panel {
                    *ui = match p.as_str() {
                        "inv" => UiMode::Inventory(0),
                        "skills" => UiMode::Inventory(2),
                        "goals" => UiMode::Inventory(3),
                        "term" => UiMode::Terminal(0),
                        "keypad" => UiMode::Keypad(level.doors.iter().find(|d| d.code.is_some()).map(|d| d.id).unwrap_or(0)),
                        "talk" => UiMode::Talk(0),
                        _ => UiMode::Pause,
                    };
                }
            }
        }
        _ => {}
    }
    if let Some(path) = &dev.shot {
        let ready = match state.get() {
            AppState::InGame => true,
            AppState::Menu => !dev.start && dev.team_start.is_none(),
            _ => false,
        };
        if ready {
            *shot_t += time.delta_secs();
        }
        if *shot_t > 1.0 && !*shot_taken {
            fps.0 += 1;
            fps.1 += time.delta_secs();
        }
        if !*shot_taken && *shot_t > dev.shot_delay {
            *shot_taken = true;
            info!("dev: avg {:.1} fps over {} frames", fps.0 as f32 / fps.1.max(0.001), fps.0);
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path.clone()));
        }
        if *shot_taken && *shot_t > dev.shot_delay + 1.5 {
            exit.write(AppExit::Success);
        }
    }
}
