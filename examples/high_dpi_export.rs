//! Exports the renderer-owned terminal texture at a fixed 2× raster scale.

mod common;

use bevy::{prelude::*, render::RenderPlugin, window::WindowResolution};
use bevy_grid::{
    BevyGridBatchPlugin, TerminalBatchOutput, TerminalRenderConfig, TerminalRenderScale,
};
use bevy_image_export::{ImageExport, ImageExportPlugin, ImageExportSettings, ImageExportSource};

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

    App::new()
        .add_plugins((
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
            export_plugin,
            BevyGridBatchPlugin::new(common::demo_surface())
                .with_config(config)
                .headless(),
        ))
        .add_systems(Startup, setup_export)
        .add_systems(Update, stop_after_export)
        .run();

    export_threads.finish();
}

fn setup_export(
    mut commands: Commands,
    output: Res<TerminalBatchOutput>,
    mut export_sources: ResMut<Assets<ImageExportSource>>,
) {
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
