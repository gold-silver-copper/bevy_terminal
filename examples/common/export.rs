//! Headless texture export helpers built on `bevy_image_export`.

use bevy::prelude::*;
use bevy_image_export::{ImageExport, ImageExportSettings, ImageExportSource};
use bevy_terminal_ratatui::prelude::{TerminalReady, TerminalTexture};

/// Registers an image exporter for every terminal texture as soon as it is
/// ready, writing PNG frames under `output_dir`.
///
/// `TerminalReady` fires on the frame the texture reaches its measured size;
/// the exporter is attached one frame later so `bevy_image_export` sizes its
/// readback buffer from the settled GPU texture.
pub fn export_terminals_on_ready(app: &mut App, output_dir: impl Into<String>) {
    let output_dir = output_dir.into();
    app.init_resource::<PendingExports>()
        .add_observer(
            |ready: On<TerminalReady>,
             textures: Query<&TerminalTexture>,
             mut pending: ResMut<PendingExports>| {
                if let Ok(texture) = textures.get(ready.entity) {
                    pending.0.push((texture.image.clone(), 1));
                }
            },
        )
        .add_systems(
            Update,
            move |mut commands: Commands,
                  mut pending: ResMut<PendingExports>,
                  mut sources: ResMut<Assets<ImageExportSource>>| {
                pending.0.retain_mut(|(handle, frames)| {
                    if *frames > 0 {
                        *frames -= 1;
                        return true;
                    }
                    commands.spawn((
                        ImageExport(sources.add(handle.clone())),
                        ImageExportSettings {
                            output_dir: output_dir.clone(),
                            extension: "png".into(),
                        },
                    ));
                    false
                });
            },
        );
}

/// Terminal textures waiting for their exporter, with the frames left to wait.
#[derive(Resource, Default)]
struct PendingExports(Vec<(Handle<Image>, u32)>);

/// Exits after `frames` updates.
pub fn exit_after(frames: u32) -> impl FnMut(Local<u32>, MessageWriter<AppExit>) {
    move |mut frame: Local<u32>, mut exit: MessageWriter<AppExit>| {
        *frame += 1;
        if *frame >= frames {
            exit.write(AppExit::Success);
        }
    }
}
