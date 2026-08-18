//! Opens a resizable window containing the representative Ratatui terminal
//! scene; the grid follows the window size.

mod common;

use bevy::{prelude::*, window::PrimaryWindow};
use bevy_terminal_ratatui::prelude::{
    TerminalPlugin, TerminalRenderConfig, TerminalSystems, TerminalTexture,
};
use bevy_terminal_ratatui::{RatatuiBackend, TerminalRenderer};
use ratatui::Terminal;

const MARGIN: f32 = 20.0;

#[derive(Resource)]
struct Demo(Terminal<RatatuiBackend>);

fn main() {
    let terminal = common::demo_terminal();
    let surface = terminal.backend().surface();
    let config = TerminalRenderConfig {
        cell_size: Vec2::new(11.0, 20.0).into(),
        ..default()
    };

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "bevy_terminal_ratatui backend".into(),
            ..default()
        }),
        ..default()
    }));
    let fonts = common::fonts::load(&mut app);
    let config = fonts.configure(config);
    app.add_plugins(TerminalPlugin)
        .insert_resource(Demo(terminal))
        .add_systems(Startup, move |mut commands: Commands| {
            commands.spawn(Camera2d);
            commands.spawn(common::app::ui_terminal(
                TerminalRenderer::new(surface.clone()),
                config.clone(),
                Vec2::splat(MARGIN),
            ));
        })
        .add_systems(Update, fit_to_window.before(TerminalSystems::Sync))
        .run();
}

/// Refits the grid whenever the window (or the measured cell) changes.
fn fit_to_window(
    mut demo: ResMut<Demo>,
    textures: Query<&TerminalTexture>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    if common::app::fit_grid_to_window(&mut demo.0, &textures, &windows, MARGIN) {
        common::draw_demo(&mut demo.0);
    }
}
