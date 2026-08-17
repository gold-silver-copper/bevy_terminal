//! Exports the renderer-owned terminal texture at a fixed 2× raster scale.

mod common;

use bevy::{prelude::*, render::RenderPlugin, window::WindowResolution};
use bevy_image_export::{ImageExport, ImageExportPlugin, ImageExportSettings, ImageExportSource};
use bevy_terminal_ratatui::{
    CursorConfig, Presentation, Terminal, TerminalPlugin, TerminalRenderConfig,
    TerminalRenderScale, TerminalTexture,
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
    app.add_plugins((export_plugin, TerminalPlugin))
        .add_systems(Startup, move |mut commands: Commands| {
            commands.spawn(
                Terminal::new(common::demo_surface())
                    .with_config(config.clone())
                    .with_presentation(Presentation::Headless),
            );
        })
        .add_systems(Update, (setup_export, stop_after_export).chain())
        .run();

    export_threads.finish();
}

/// Registers the exporter once the terminal texture exists.
fn setup_export(
    mut commands: Commands,
    mut done: Local<bool>,
    outputs: Query<&TerminalTexture>,
    mut export_sources: ResMut<Assets<ImageExportSource>>,
) {
    if *done {
        return;
    }
    let Ok(output) = outputs.single() else {
        return;
    };
    *done = true;
    commands.spawn((
        ImageExport(export_sources.add(output.image.clone())),
        ImageExportSettings {
            output_dir: "target/render-qa-2x".into(),
            extension: "png".into(),
        },
    ));
}

fn stop_after_export(mut frame: Local<u32>, mut exit: MessageWriter<AppExit>) {
    *frame += 1;
    if *frame >= EXPORT_FRAMES {
        exit.write(AppExit::Success);
    }
}
