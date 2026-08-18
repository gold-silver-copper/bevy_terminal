//! Exports the renderer-owned terminal texture at a fixed 2× raster scale.

mod common;

use bevy::{prelude::*, render::RenderPlugin, window::WindowResolution};
use bevy_image_export::ImageExportPlugin;
use bevy_terminal_ratatui::{
    CursorConfig, TerminalPlugin, TerminalRenderConfig, TerminalRenderScale, TerminalRenderer,
};

const EXPORT_FRAMES: u32 = 8;

fn main() {
    let config = TerminalRenderConfig {
        cell_size: Vec2::new(11.0, 20.0),
        render_scale: TerminalRenderScale::Fixed(2.0),
        cursor: CursorConfig {
            blink_hz: None,
            ..default()
        },
        ..default()
    };
    let export_plugin = ImageExportPlugin::default();
    let export_threads = export_plugin.threads.clone();

    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    resolution: WindowResolution::new(1, 1).with_scale_factor_override(1.0),
                    visible: false,
                    ..default()
                }),
                ..default()
            })
            .set(RenderPlugin {
                synchronous_pipeline_compilation: true,
                ..default()
            }),
    );
    let fonts = common::fonts::load(&mut app);
    let config = fonts.configure(config);
    common::export::export_terminals_on_ready(&mut app, "target/render-qa-2x");
    app.add_plugins((export_plugin, TerminalPlugin::default()))
        .add_systems(Startup, move |mut commands: Commands| {
            commands.spawn(common::app::headless_terminal(
                TerminalRenderer::new(common::demo_surface()),
                config.clone(),
            ));
        })
        .add_systems(Update, common::export::exit_after(EXPORT_FRAMES))
        .run();

    export_threads.finish();
}
