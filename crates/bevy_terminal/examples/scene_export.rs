//! Exports the direct-scene example's renderer-owned textures to PNG files.

mod common;

use bevy::{prelude::*, render::RenderPlugin, window::WindowResolution};
use bevy_image_export::{ImageExport, ImageExportPlugin, ImageExportSettings, ImageExportSource};
use bevy_terminal::{
    BevyTerminalPlugin, TerminalBatch, TerminalBatchOutput, TerminalRenderConfig,
    TerminalRenderScale, TerminalSurface,
};

const EXPORT_FRAMES: u32 = 6;

#[derive(Resource)]
struct Surfaces {
    main: TerminalSurface,
    status: TerminalSurface,
}

fn main() {
    let export_plugin = ImageExportPlugin::default();
    let export_threads = export_plugin.threads.clone();
    let main = common::scene_surface();
    let status = common::status_surface();

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
    let config = common::configure_fonts(
        &mut app,
        TerminalRenderConfig {
            cell_size: common::CELL_SIZE,
            font_size: 18.0,
            render_scale: TerminalRenderScale::Fixed(1.0),
            cursor_blink_hz: None,
            ..default()
        },
    );
    app.add_plugins((
        export_plugin,
        BevyTerminalPlugin::new(main.clone())
            .with_config(config.clone())
            .headless(),
        BevyTerminalPlugin::new(status.clone())
            .with_config(config)
            .headless(),
    ))
    .insert_resource(Surfaces { main, status })
    .add_systems(Startup, setup_export)
    .add_systems(Update, stop_after_export)
    .run();

    export_threads.finish();
}

fn setup_export(
    mut commands: Commands,
    surfaces: Res<Surfaces>,
    outputs: Query<(&TerminalBatch, &TerminalBatchOutput)>,
    mut export_sources: ResMut<Assets<ImageExportSource>>,
) {
    for (terminal, output) in &outputs {
        let name = if terminal.renders_surface(&surfaces.main) {
            "scene"
        } else if terminal.renders_surface(&surfaces.status) {
            "status"
        } else {
            continue;
        };
        commands.spawn((
            ImageExport(export_sources.add(output.image.clone())),
            ImageExportSettings {
                output_dir: format!("target/bevy-terminal-qa/{name}"),
                extension: "png".into(),
            },
        ));
    }
}

fn stop_after_export(mut frame: Local<u32>, mut exit: MessageWriter<AppExit>) {
    *frame += 1;
    if *frame >= EXPORT_FRAMES {
        exit.write(AppExit::Success);
    }
}
