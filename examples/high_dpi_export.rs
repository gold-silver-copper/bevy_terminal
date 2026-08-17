//! Exports the renderer-owned terminal texture at a fixed 2× raster scale.

mod common;

use bevy::{prelude::*, render::RenderPlugin, window::WindowResolution};
use bevy_image_export::{ImageExport, ImageExportPlugin, ImageExportSettings, ImageExportSource};
use bevy_terminal_ratatui::{
    BevyTerminalPlugin, TerminalBatchOutput, TerminalRenderConfig, TerminalRenderScale,
};

const EXPORT_FRAMES: u32 = 6;

fn main() {
    let config = TerminalRenderConfig {
        cell_size: Vec2::new(11.0, 20.0),
        font_size: 18.0,
        render_scale: TerminalRenderScale::Fixed(2.0),
        cursor_blink_hz: None,
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
    app.add_plugins((
        export_plugin,
        BevyTerminalPlugin::new(common::demo_surface())
            .with_config(fonts.configure(config))
            .headless(),
    ))
    .add_systems(Startup, setup_export)
    .add_systems(Update, stop_after_export)
    .run();

    export_threads.finish();
}

fn setup_export(
    mut commands: Commands,
    outputs: Query<&TerminalBatchOutput>,
    mut export_sources: ResMut<Assets<ImageExportSource>>,
) {
    let output = outputs
        .single()
        .expect("the example creates exactly one terminal output");
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
