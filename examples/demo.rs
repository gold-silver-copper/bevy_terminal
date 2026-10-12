//! Opens a resizable window containing the representative Ratatui terminal
//! scene; the grid follows the window size.

mod common;

use bevy::{prelude::*, window::PrimaryWindow};
use bevy_terminal_ratatui::prelude::{
    RatatuiPlugin, RatatuiTerminal, TerminalRenderConfig, TerminalSizing, TerminalSystems,
    TerminalTexture,
};

const MARGIN: f32 = 20.0;

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
    app.add_plugins(RatatuiPlugin)
        .add_plugins((common::app::presentation, common::app::window_scale))
        .add_systems(Startup, move |mut commands: Commands| {
            commands.spawn(Camera2d);
            commands.spawn(common::app::ui_terminal(
                common::demo_terminal(),
                config.clone(),
                Vec2::splat(MARGIN),
            ));
        })
        .add_systems(Update, fit_to_window.before(TerminalSystems::Sync))
        .run();
}

/// Refits the grid whenever the window (or the measured cell) changes.
fn fit_to_window(
    mut terminal: Single<&mut RatatuiTerminal>,
    textures: Query<&TerminalTexture>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    if common::app::fit_grid_to_window(&mut terminal, &textures, &windows, MARGIN) {
        common::draw_demo(&mut terminal);
    }
}
