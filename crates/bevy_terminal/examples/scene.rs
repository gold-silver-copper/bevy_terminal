//! Renders two independent terminal scenes written directly through the
//! `bevy_terminal` surface API, without any terminal UI library.

mod common;

use bevy::prelude::*;
use bevy_terminal::{BevyTerminalPlugin, TerminalRenderConfig, TerminalSurface, TerminalSystems};

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
            font_size: 18.0,
            origin: Vec2::new(16.0, 16.0),
            ..default()
        },
    );
    let status_config = TerminalRenderConfig {
        origin: Vec2::new(
            16.0 + f32::from(common::COLUMNS) * common::CELL_SIZE.x + 24.0,
            16.0,
        ),
        ..config.clone()
    };
    app.add_plugins((
        BevyTerminalPlugin::new(main.clone()).with_config(config),
        BevyTerminalPlugin::new(status).with_config(status_config),
    ))
    .insert_resource(Scenes { main, tick: 0 })
    .add_systems(Startup, |mut commands: Commands| {
        commands.spawn(Camera2d);
    })
    .add_systems(Update, animate.before(TerminalSystems::Sync))
    .run();
}

/// Moves the cursor and rewrites one cell each frame through a single transaction.
fn animate(mut scenes: ResMut<Scenes>, time: Res<Time>) {
    let tick = (time.elapsed_secs() * 4.0) as u32;
    if tick == scenes.tick {
        return;
    }
    scenes.tick = tick;
    let column = 39 + (tick % 6) as u16;
    let mut update = scenes.main.begin_update();
    common::write_text(
        &mut update,
        39,
        11,
        "      ",
        bevy_terminal::TerminalStyle::new(),
    );
    update.set_cursor_position(column, 11);
}
