//! Opens a window containing the representative Ratatui terminal scene.

mod common;

use bevy::{prelude::*, window::WindowResolution};
use bevy_terminal_ratatui::{BevyTerminalPlugin, TerminalRenderConfig};

fn main() {
    let surface = common::demo_surface();
    let config = TerminalRenderConfig {
        cell_size: Vec2::new(11.0, 20.0),
        font_size: 18.0,
        origin: Vec2::new(20.0, 20.0),
        ..default()
    };

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "bevy_terminal_ratatui backend".into(),
            resolution: WindowResolution::new(832, 480),
            resizable: false,
            ..default()
        }),
        ..default()
    }));
    let fonts = common::fonts::load(&mut app);
    app.add_plugins(BevyTerminalPlugin::new(surface).with_config(fonts.configure(config)))
        .add_systems(Startup, |mut commands: Commands| {
            commands.spawn(Camera2d);
        })
        .run();
}
