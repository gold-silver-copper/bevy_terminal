//! Opens a window containing the representative Ratatui terminal scene.

mod common;

use bevy::{prelude::*, window::WindowResolution};
use bevy_terminal_ratatui::{Presentation, Terminal, TerminalPlugin, TerminalRenderConfig};

fn main() {
    let surface = common::demo_surface();
    let config = TerminalRenderConfig {
        cell_size: Vec2::new(11.0, 20.0),
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
    let config = fonts.configure(config);
    app.add_plugins(TerminalPlugin)
        .add_systems(Startup, move |mut commands: Commands| {
            commands.spawn(Camera2d);
            commands.spawn(
                Terminal::new(surface.clone())
                    .with_config(config.clone())
                    .with_presentation(Presentation::Ui {
                        origin: Vec2::new(20.0, 20.0),
                    }),
            );
        })
        .run();
}
