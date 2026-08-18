//! Renders two independent terminal scenes written directly through the
//! `bevy_terminal` surface API, without any terminal UI library.

mod common;

use bevy::prelude::*;
use bevy_terminal::{TerminalPlugin, TerminalRenderConfig, TerminalSurface, TerminalSystems};

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
            resolution: bevy::window::WindowResolution::new(860, 340),
            ..default()
        }),
        ..default()
    }));
    let config = common::configure_fonts(
        &mut app,
        TerminalRenderConfig {
            cell_size: common::CELL_SIZE,
            ..default()
        },
    );
    let status_origin = Vec2::new(
        16.0 + f32::from(common::COLUMNS) * common::CELL_SIZE.x + 24.0,
        16.0,
    );
    // The main terminal is spawned at startup; the status terminal a moment
    // later to show that terminals can be added while the app runs.
    app.add_plugins(TerminalPlugin::default())
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
                animate,
            )
                .before(TerminalSystems::Sync),
        )
        .run();
}

#[derive(Resource)]
struct PendingStatus(Option<(TerminalSurface, TerminalRenderConfig)>);

/// Moves the cursor and rewrites one cell each frame through a single transaction.
fn animate(mut scenes: ResMut<Scenes>, time: Res<Time>) {
    let tick = (time.elapsed_secs() * 4.0) as u32;
    if tick == scenes.tick {
        return;
    }
    scenes.tick = tick;
    let column = 39 + (tick % 6) as u16;
    scenes.main.update(|update| {
        common::write_text(
            update,
            39,
            11,
            "      ",
            bevy_terminal::TerminalStyle::new(),
        );
        update.set_cursor_position((column, 11));
    });
}
