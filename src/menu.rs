//! Main menu (drawn over a slow cinematic pass through the level).
use bevy::prelude::*;

use crate::game::*;
use crate::hud::*;

#[derive(Component)]
pub struct MenuRoot;

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::Menu), spawn_menu)
            .add_systems(OnExit(AppState::Menu), despawn_menu)
            .add_systems(Update, (menu_actions, refresh_menu).run_if(in_state(AppState::Menu)));
    }
}

fn despawn_menu(mut commands: Commands, q: Query<Entity, With<MenuRoot>>) {
    for e in q.iter() {
        commands.entity(e).despawn();
    }
}

fn spawn_menu(mut commands: Commands, settings: Res<Settings>, hud: Res<HudState>) {
    let full = Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() };
    commands
        .spawn((full.clone(), MenuRoot))
        .with_children(|r| {
            if let Some(v) = &hud.vignette {
                r.spawn((full.clone(), ImageNode::new(v.clone())));
            }
            r.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px(0.0),
                    width: Val::Px(560.0),
                    height: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    justify_content: JustifyContent::Center,
                    padding: UiRect::left(Val::Px(70.0)),
                    row_gap: Val::Px(6.0),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.01, 0.02, 0.55)),
            ))
            .with_children(|c| {
                txt(c, "DEUSMOBILE", 64.0, C_ACCENT);
                txt(c, "PORT MERIDIAN  //  2071", 18.0, C_TEAL);
                c.spawn(Node { height: Val::Px(10.0), ..default() });
                txt(c, "Halvorsen-Kade is moving something out of its harbour\nbiolab before dawn. VEIL sent you to take it first.\nStay in the dark.", 15.0, C_DIM);
                c.spawn(Node { height: Val::Px(26.0), ..default() });
                button(c, "START MISSION", UiAction::MenuStart, true);
                button(c, format!("GRAPHICS: {}", settings.quality.name()), UiAction::CycleQuality, true);
                button(c, "QUIT", UiAction::MenuQuit, true);
                c.spawn(Node { height: Val::Px(26.0), ..default() });
                txt(c, "Co-op multiplayer: planned for a future update", 13.0, C_DIM);
                txt(c, "Placeholder procedural art. Drop PNGs in assets/textures to replace surfaces.", 12.0, C_DIM);
            });
        });
}

fn refresh_menu(settings: Res<Settings>, commands: Commands, q: Query<Entity, With<MenuRoot>>, hud: Res<HudState>, mut first: Local<bool>) {
    if !*first {
        *first = true;
        return;
    }
    if settings.is_changed() {
        let mut commands = commands;
        for e in q.iter() {
            commands.entity(e).despawn();
        }
        spawn_menu(commands, settings, hud);
    }
}

fn menu_actions(q: Query<(&Interaction, &UiButton), Changed<Interaction>>, mut role: ResMut<NetRole>, mut local: ResMut<LocalId>, mut next: ResMut<NextState<AppState>>) {
    for (i, b) in q.iter() {
        if *i != Interaction::Pressed {
            continue;
        }
        if b.action == UiAction::MenuStart {
            *role = NetRole::Offline;
            local.0 = 0;
            next.set(AppState::InGame);
        }
    }
}
