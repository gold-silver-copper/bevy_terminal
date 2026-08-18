//! Headless texture export helpers built on `bevy_image_export`.

use bevy::prelude::*;
use bevy_image_export::{ImageExport, ImageExportSettings, ImageExportSource};
use bevy_terminal_ratatui::TerminalReady;

/// Registers an image exporter for every terminal texture as soon as it is
/// ready, writing PNG frames under `output_dir`.
pub fn export_terminals_on_ready(app: &mut App, output_dir: impl Into<String>) {
    let output_dir = output_dir.into();
    app.add_observer(
        move |ready: On<TerminalReady>,
              mut commands: Commands,
              mut sources: ResMut<Assets<ImageExportSource>>| {
            commands.spawn((
                ImageExport(sources.add(ready.image.clone())),
                ImageExportSettings {
                    output_dir: output_dir.clone(),
                    extension: "png".into(),
                },
            ));
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
