//! Headless texture export helpers built on `bevy_image_export`.

use bevy::prelude::*;
use bevy_image_export::{ImageExport, ImageExportSettings, ImageExportSource};
use bevy_terminal_ratatui::prelude::{TerminalReady, TerminalTexture};

#[path = "export_gpu.rs"]
pub mod gpu;

/// Exports terminal textures once measured, writing PNG frames under `output_dir`.
/// GPU availability and resizing are handled in the render world.
pub fn export_terminals_on_ready(app: &mut App, output_dir: impl Into<String>) {
    let output_dir = output_dir.into();
    gpu::install(app);
    app.add_observer(
        move |ready: On<TerminalReady>,
              textures: Query<&TerminalTexture>,
              mut commands: Commands,
              mut sources: ResMut<Assets<ImageExportSource>>| {
            if let Ok(texture) = textures.get(ready.entity) {
                commands.spawn((
                    ImageExport(sources.add(texture.image.clone())),
                    ImageExportSettings {
                        output_dir: output_dir.clone(),
                        extension: "png".into(),
                    },
                ));
            }
        },
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
