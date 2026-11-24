//! Opens a resizable window containing the representative Ratatui terminal
//! scene; the grid follows the window size.

mod common;

use bevy::prelude::*;
use bevy_terminal_ratatui::prelude::{
    GridSize, RatatuiTerminal, RatatuiWindowPlugin, TerminalRenderConfig, TerminalSizing,
};

fn main() {
    let config = TerminalRenderConfig {
        sizing: TerminalSizing::FitCellWidth(Vec2::new(11.0, 20.0)),
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
    // The plugin spawns the camera and the UI node, and keeps the grid the
    // size of the window.
    app.add_plugins(RatatuiWindowPlugin::<RatatuiTerminal>::default())
        .add_systems(Startup, move |mut commands: Commands| {
            commands.spawn((common::demo_terminal(), config.clone()));
        })
        .add_systems(Update, redraw_when_resized)
        .run();
}

/// Draws the scene again at each new size the window gives the grid.
fn redraw_when_resized(mut terminal: Single<&mut RatatuiTerminal>, mut drawn: Local<GridSize>) {
    let size = terminal.grid().size();
    if *drawn != size {
        *drawn = size;
        common::draw_demo(&mut terminal);
    }
}
