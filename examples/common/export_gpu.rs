//! Keep the example exporter's readback buffers aligned with prepared images.
use bevy::{
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems,
        render_asset::{RenderAsset, RenderAssets},
        renderer::RenderDevice,
        texture::GpuImage,
    },
};
use bevy_image_export::{GpuImageExportSource, ImageExportSource};

/// Install after the renderer and `ImageExportPlugin`.
///
/// The exporter retries missing GPU images itself, but caches dimensions once
/// prepared. Refresh after image preparation so a late asset or resize never
/// relies on a fixed number of main-world frames. Copying happens later in the
/// render graph, after terminal rendering has submitted its commands.
pub fn install(app: &mut App) {
    app.sub_app_mut(RenderApp).add_systems(
        Render,
        refresh_buffers
            .after(RenderSystems::PrepareAssets)
            .before(RenderSystems::Render),
    );
}

pub fn refresh_buffers(
    mut sources: ResMut<RenderAssets<GpuImageExportSource>>,
    device: Res<RenderDevice>,
    images: Res<RenderAssets<GpuImage>>,
) {
    for (id, source) in sources.iter_mut() {
        let Some(image) = images.get(&source.source_handle) else {
            continue;
        };
        if image.texture.size() == source.source_size {
            continue;
        }
        // Reuse the exporter's allocation and format rules instead of copying
        // its row-padding/buffer-size calculations into these examples.
        if let Ok(replacement) = GpuImageExportSource::prepare_asset(
            ImageExportSource(source.source_handle.clone()),
            id,
            &mut (Res::clone(&device), Res::clone(&images)),
            Some(source),
        ) {
            *source = replacement;
        }
    }
}
