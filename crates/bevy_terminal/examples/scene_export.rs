//! Exports the direct-scene example's renderer-owned textures to PNG files.

mod common;

use bevy::{prelude::*, render::RenderPlugin, window::WindowResolution};
use bevy_image_export::{ImageExport, ImageExportPlugin, ImageExportSettings, ImageExportSource};
use bevy_terminal::prelude::*;

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
            cell_size: common::CELL_SIZE.into(),
            raster: RasterConfig {
                scale: TerminalRenderScale::Fixed(1.0),
                ..default()
            },
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
                commands.spawn((Terminal::new(surface.clone()), config.clone()));
            }
        })
        .init_resource::<PendingExports>()
        .add_observer(export_when_ready)
        .add_systems(Update, (spawn_pending_exports, stop_after_export))
        .run();

    export_threads.finish();
}

/// Queues an exporter for a terminal texture as soon as it is ready; it is
/// spawned one frame later so `bevy_image_export` sees the settled GPU texture.
fn export_when_ready(
    ready: On<TerminalReady>,
    surfaces: Res<Surfaces>,
    terminals: Query<(&Terminal, &TerminalTexture)>,
    mut pending: ResMut<PendingExports>,
) {
    let Ok((terminal, texture)) = terminals.get(ready.entity) else {
        return;
    };
    let name = if terminal.surface().shares_state_with(&surfaces.main) {
        "scene"
    } else if terminal.surface().shares_state_with(&surfaces.status) {
        "status"
    } else {
        return;
    };
    pending.0.push((texture.image.clone(), name, 1));
}

/// Textures waiting for their exporter and the frames left to wait.
#[derive(Resource, Default)]
struct PendingExports(Vec<(Handle<Image>, &'static str, u32)>);

fn spawn_pending_exports(
    mut commands: Commands,
    mut pending: ResMut<PendingExports>,
    mut export_sources: ResMut<Assets<ImageExportSource>>,
) {
    pending.0.retain_mut(|(handle, name, frames)| {
        if *frames > 0 {
            *frames -= 1;
            return true;
        }
        commands.spawn((
            ImageExport(export_sources.add(handle.clone())),
            ImageExportSettings {
                output_dir: format!("target/bevy-terminal-qa/{name}"),
                extension: "png".into(),
            },
        ));
        false
    });
}

fn stop_after_export(mut frame: Local<u32>, mut exit: MessageWriter<AppExit>) {
    *frame += 1;
    if *frame >= EXPORT_FRAMES {
        exit.write(AppExit::Success);
    }
}
