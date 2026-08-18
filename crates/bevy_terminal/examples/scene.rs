//! Renders two independent terminal scenes written directly through the
//! `bevy_terminal` surface API, without any terminal UI library.

mod common;

use bevy::prelude::*;
use bevy_terminal::prelude::*;

#[derive(Resource)]
struct Scenes {
    main: TerminalSurface,
    tick: u32,
}

fn main() {
    let main = common::scene_surface();
    let status = common::status_surface();

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "bevy_terminal · direct scene".into(),
            resolution: bevy::window::WindowResolution::new(900, 460),
            ..default()
        }),
        ..default()
    }));
    let config = common::configure_fonts(
        &mut app,
        TerminalRenderConfig {
            cell_size: common::CELL_SIZE.into(),
            ..default()
        },
    );
    // The status terminal sits in the top-right corner; the main terminal fills
    // the rest of the (resizable) window.
    let status_origin = Vec2::new(-1.0, 16.0);
    // The main terminal is spawned at startup; the status terminal a moment
    // later to show that terminals can be added while the app runs.
    app.add_plugins(TerminalPlugin)
        .insert_resource(Scenes {
            main: main.clone(),
            tick: 0,
        })
        .insert_resource(PendingStatus(Some((status, config.clone()))))
        .add_systems(Startup, move |mut commands: Commands| {
            commands.spawn(Camera2d);
            commands.spawn(common::ui_terminal(
                main.clone(),
                config.clone(),
                Vec2::splat(16.0),
            ));
        })
        .add_systems(
            Update,
            (
                move |mut commands: Commands,
                      time: Res<Time>,
                      mut pending: ResMut<PendingStatus>| {
                    if time.elapsed_secs() > 1.0
                        && let Some((surface, config)) = pending.0.take()
                    {
                        commands.spawn(common::ui_terminal(surface, config, status_origin));
                    }
                },
                fit_to_window,
                animate,
            )
                .before(TerminalSystems::Sync),
        )
        .run();
}

#[derive(Resource)]
struct PendingStatus(Option<(TerminalSurface, TerminalRenderConfig)>);

const MARGIN: f32 = 16.0;
const GAP: f32 = 24.0;

/// Places the status terminal at the top-right and refits the main terminal's
/// grid to the remaining window area at its measured cell size.
fn fit_to_window(
    scenes: Res<Scenes>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut terminals: Query<(&Terminal, &TerminalTexture, &mut Node)>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let size = window.resolution.size();
    let mut status_width = 0.0;
    for (terminal, texture, mut node) in &mut terminals {
        if !terminal.surface().shares_state_with(&scenes.main) {
            status_width = texture.logical_size.x;
            let left = (size.x - MARGIN - status_width).max(MARGIN);
            if node.left != px(left) {
                node.left = px(left);
            }
        }
    }
    let available = Vec2::new(
        size.x
            - MARGIN * 2.0
            - if status_width > 0.0 {
                status_width + GAP
            } else {
                0.0
            },
        size.y - MARGIN * 2.0,
    );
    for (terminal, texture, _) in &terminals {
        if terminal.surface().shares_state_with(&scenes.main) {
            let grid = texture.grid_for(available);
            if scenes.main.size() != grid {
                scenes.main.update(|update| {
                    update.resize(grid);
                    common::draw_scene(update);
                });
            }
        }
    }
}

/// Moves the cursor and rewrites one cell each frame through a single transaction.
fn animate(mut scenes: ResMut<Scenes>, time: Res<Time>) {
    let tick = (time.elapsed_secs() * 4.0) as u32;
    if tick == scenes.tick {
        return;
    }
    scenes.tick = tick;
    let column = 39 + (tick % 6) as u16;
    scenes.main.update(|update| {
        common::write_text(update, 39, 11, "      ", TerminalStyle::new());
        update.set_cursor_position((column, 11));
    });
}
