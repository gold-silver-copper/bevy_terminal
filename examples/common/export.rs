//! Headless texture export helpers built on `bevy_image_export`.

use bevy::prelude::*;
use bevy_image_export::{ImageExport, ImageExportSettings, ImageExportSource};
use bevy_terminal_ratatui::prelude::{TerminalSystems, TerminalTexture};

#[path = "export_gpu.rs"]
pub mod gpu;

#[derive(Component)]
struct ExportStarted;

/// Exports terminal textures once measured, writing PNG frames under `output_dir`.
/// GPU availability and resizing are handled in the render world.
pub fn export_terminals_on_ready(app: &mut App, output_dir: impl Into<String>) {
    let output_dir = output_dir.into();
    gpu::install(app);
    app.add_systems(
        Update,
        (move |textures: Query<(Entity, &TerminalTexture), Without<ExportStarted>>,
               mut commands: Commands,
               mut sources: ResMut<Assets<ImageExportSource>>| {
            for (entity, texture) in &textures {
                if texture.measured().is_none() {
                    continue;
                }
                commands.entity(entity).insert(ExportStarted);
                commands.spawn((
                    ImageExport(sources.add(texture.image.clone())),
                    ImageExportSettings {
                        output_dir: output_dir.clone(),
                        extension: "png".into(),
                    },
                ));
            }
        })
        .after(TerminalSystems::Sync),
    );
}

/// Exits after `frames` updates.
pub fn exit_after(frames: u32) -> impl FnMut(Local<u32>, MessageWriter<AppExit>) {
    move |mut frame: Local<u32>, mut exit: MessageWriter<AppExit>| {
        *frame += 1;
        if *frame >= frames {
            exit.write(AppExit::Success);
        }
    }
}
