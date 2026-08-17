//! Exports the direct-scene example's renderer-owned textures to PNG files.

mod common;

use bevy::{prelude::*, render::RenderPlugin, window::WindowResolution};
use bevy_image_export::{ImageExport, ImageExportPlugin, ImageExportSettings, ImageExportSource};
use bevy_terminal::{
    CursorConfig, Presentation, Terminal, TerminalPlugin, TerminalRenderConfig,
    TerminalRenderScale, TerminalSurface, TerminalTexture,
};

const EXPORT_FRAMES: u32 = 8;

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
            render_scale: TerminalRenderScale::Fixed(1.0),
            cursor: CursorConfig {
                blink_hz: None,
                ..default()
            },
            ..default()
        },
    );
    app.add_plugins((export_plugin, TerminalPlugin))
        .insert_resource(Surfaces {
            main: main.clone(),
            status: status.clone(),
        })
        .add_systems(Startup, move |mut commands: Commands| {
            for surface in [&main, &status] {
                commands.spawn(
                    Terminal::new(surface.clone())
                        .with_config(config.clone())
                        .with_presentation(Presentation::Headless),
                );
            }
        })
        .add_systems(Update, (setup_export, stop_after_export).chain())
        .run();

    export_threads.finish();
}

/// Registers an exporter for each terminal texture once both textures exist.
fn setup_export(
    mut commands: Commands,
    mut done: Local<bool>,
    surfaces: Res<Surfaces>,
    outputs: Query<(&Terminal, &TerminalTexture)>,
    mut export_sources: ResMut<Assets<ImageExportSource>>,
) {
    if *done || outputs.iter().count() < 2 {
        return;
    }
    *done = true;
    for (terminal, output) in &outputs {
        let name = if terminal.surface().shares_state_with(&surfaces.main) {
            "scene"
        } else if terminal.surface().shares_state_with(&surfaces.status) {
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
