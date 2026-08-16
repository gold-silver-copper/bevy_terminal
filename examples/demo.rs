//! Opens a window containing the representative Ratatui terminal scene.

mod common;

use bevy::{prelude::*, window::WindowResolution};
use bevy_grid::{BevyGridPlugin, TerminalRenderConfig};

fn main() {
    let surface = common::demo_surface();
    let config = TerminalRenderConfig {
        cell_size: Vec2::new(10.8, 20.0),
        font_size: 18.0,
        origin: Vec2::new(20.0, 20.0),
        ..default()
    };

    App::new()
        .add_plugins((
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "bevy_grid Ratatui backend".into(),
                    resolution: WindowResolution::new(818, 480),
                    resizable: false,
                    ..default()
                }),
                ..default()
            }),
            BevyGridPlugin::new(surface).with_config(config),
        ))
        .add_systems(Startup, |mut commands: Commands| {
            commands.spawn(Camera2d);
        })
        .run();
}
