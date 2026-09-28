//! Extraction, resource lifetimes, pipelines and drawing.
use super::{
    BatchScene, Blend, GLYPH_ATLAS_SIZE, GLYPH_FORMAT, PendingBatchScenes, QuadInstance,
    SceneQueue, TARGET_FORMAT,
};
use bevy::{
    asset::{load_internal_asset, uuid_handle},
    platform::collections::HashMap,
    prelude::*,
    render::{
        MainWorld,
        mesh::VertexBufferLayout,
        render_asset::RenderAssets,
        render_resource::{
            BindGroup, BindGroupEntries, BindGroupLayout, BindGroupLayoutDescriptor,
            BindGroupLayoutEntries, BlendState, BufferUsages, CachedRenderPipelineId,
            ColorTargetState, ColorWrites, Extent3d, FragmentState, LoadOp, Operations, Origin3d,
            PipelineCache, RawBufferVec, RenderPassColorAttachment, RenderPassDescriptor,
            RenderPipelineDescriptor, Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages,
            StoreOp, TexelCopyBufferLayout, TexelCopyTextureInfo, Texture, TextureAspect,
            TextureDescriptor, TextureDimension, TextureSampleType, TextureUsages,
            TextureViewDescriptor, VertexAttribute, VertexFormat, VertexState, VertexStepMode,
            binding_types::{sampler, texture_2d},
        },
        renderer::{RenderContext, RenderDevice, RenderQueue},
        texture::GpuImage,
    },
    shader::Shader,
};
use std::sync::atomic::Ordering;

/// The terminal shader, `batch.wgsl`.
const BATCH_SHADER: Handle<Shader> = uuid_handle!("3db953c8-0f3f-4124-9a1c-6aa749bb0655");

/// Adds the terminal shader to the app's shaders directly, as Bevy adds its
/// internal ones. Loading it through the asset server would take frames, and
/// the pipelines (so a new terminal's first scenes) would wait for it.
pub(super) fn load_shader(app: &mut App) {
    load_internal_asset!(app, BATCH_SHADER, "batch.wgsl", Shader::from_wgsl);
}

pub(super) fn extract_batch_scenes(
    mut main_world: ResMut<MainWorld>,
    mut pending: ResMut<PendingBatchScenes>,
) {
    collect_batch_scenes(&mut main_world, &mut pending);
}

/// Takes the main world's queued scenes and releases, without looking at
/// any terminal.
pub(super) fn collect_batch_scenes(main_world: &mut World, pending: &mut PendingBatchScenes) {
    let Some(mut queue) = main_world.get_resource_mut::<SceneQueue>() else {
        return;
    };
    queue.extracting = true;
    for (output, atlas) in queue.released.drain(..) {
        pending.scenes.remove(&output);
        pending.released_atlases.push(atlas);
    }
    // Withdrawn before newer scenes arrive, which then absorb them.
    for output in queue.withdrawn.drain() {
        if let Some(scene) = pending.scenes.get_mut(&output) {
            scene.withdraw();
        }
    }
    for (destination, mut scene) in queue.scenes.drain() {
        // An unacknowledged submission is replaced by a full scene in the
        // main world, so superseding it cannot lose intermediate rows;
        // its atlas entries are carried over.
        if let Some(superseded) = pending.scenes.remove(&destination) {
            scene.absorb(superseded);
        }
        pending.scenes.insert(destination, scene);
    }
    // A scene not yet drawn whose surface has been resized since it was built
    // (by a producer after `TerminalSystems::Sync`) is withdrawn; the next
    // sync repaints the new grid. Only waiting scenes are checked, not idle
    // terminals.
    for scene in pending.scenes.values_mut() {
        if !scene.is_current() {
            scene.withdraw();
        }
    }
}

/// The terminal pipelines (alpha-blended and replacing) and their bind
/// group layout, queued once per render device.
#[derive(Resource)]
pub(super) struct BatchPipelines {
    layout: BindGroupLayoutDescriptor,
    alpha: CachedRenderPipelineId,
    replace: CachedRenderPipelineId,
    atlas_sampler: Sampler,
}

/// The instance layout: `QuadInstance`'s fields, in order.
fn instance_layout() -> VertexBufferLayout {
    let attribute = |format, offset, shader_location| VertexAttribute {
        format,
        offset,
        shader_location,
    };
    VertexBufferLayout {
        array_stride: size_of::<QuadInstance>() as u64,
        step_mode: VertexStepMode::Instance,
        attributes: vec![
            attribute(VertexFormat::Float32x4, 0, 0),
            attribute(VertexFormat::Float32x4, 16, 1),
            attribute(VertexFormat::Float32x4, 32, 2),
            attribute(VertexFormat::Float32, 48, 3),
        ],
    }
}

/// Queues the pipelines and starts the GPU state afresh; `RenderStartup`
/// also runs after a render device is recreated.
pub(super) fn init_batch_pipelines(
    mut commands: Commands,
    device: Res<RenderDevice>,
    pipeline_cache: Res<PipelineCache>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "bevy_terminal glyph atlas layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
            ),
        ),
    );
    let shader = BATCH_SHADER;
    let pipeline = |blend| RenderPipelineDescriptor {
        label: Some("bevy_terminal batch pipeline".into()),
        layout: vec![layout.clone()],
        vertex: VertexState {
            shader: shader.clone(),
            entry_point: Some("vertex".into()),
            buffers: vec![instance_layout()],
            ..default()
        },
        fragment: Some(FragmentState {
            shader: shader.clone(),
            entry_point: Some("fragment".into()),
            targets: vec![Some(ColorTargetState {
                format: TARGET_FORMAT,
                blend: Some(blend),
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        ..default()
    };
    commands.insert_resource(BatchPipelines {
        alpha: pipeline_cache.queue_render_pipeline(pipeline(BlendState::ALPHA_BLENDING)),
        replace: pipeline_cache.queue_render_pipeline(pipeline(BlendState::REPLACE)),
        layout,
        atlas_sampler: device.create_sampler(&SamplerDescriptor {
            label: Some("bevy_terminal glyph atlas sampler"),
            ..default()
        }),
    });
    commands.insert_resource(BatchGpuState::default());
}

#[derive(Default, Resource)]
pub(super) struct BatchGpuState {
    /// Instance buffers, one per upload chunk: a single one unless a frame's
    /// instances exceed the device's maximum buffer size.
    instances: Vec<RawBufferVec<QuadInstance>>,
    /// Terminals' glyph atlases, created on their first entry.
    atlases: HashMap<AssetId<Image>, GpuAtlas>,
    /// Binding for scenes whose atlas holds nothing yet (solid quads only).
    empty_atlas: Option<BindGroup>,
}

/// A terminal's glyph atlas texture.
pub(super) struct GpuAtlas {
    texture: Texture,
    bind_group: BindGroup,
}

fn atlas_bind_group(
    device: &RenderDevice,
    layout: &BindGroupLayout,
    texture: &Texture,
    sampler: &Sampler,
) -> BindGroup {
    let view = texture.create_view(&TextureViewDescriptor::default());
    device.create_bind_group(
        "bevy_terminal glyph atlas",
        layout,
        &BindGroupEntries::sequential((&view, sampler)),
    )
}

fn atlas_texture(device: &RenderDevice, size: u32, usage: TextureUsages) -> Texture {
    device.create_texture(&TextureDescriptor {
        label: Some("bevy_terminal glyph atlas"),
        size: Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: GLYPH_FORMAT,
        usage,
        view_formats: &[],
    })
}

/// Writes a scene's new atlas entries, creating the atlas on its first.
fn upload_atlas(
    atlases: &mut HashMap<AssetId<Image>, GpuAtlas>,
    device: &RenderDevice,
    queue: &RenderQueue,
    layout: &BindGroupLayout,
    sampler: &Sampler,
    scene: &BatchScene,
) {
    if !atlases.contains_key(&scene.atlas) && !scene.atlas_fresh {
        // Earlier entries never reached this texture (the render world was
        // reset): have the main world rebuild the atlas.
        scene.atlas_lost.store(true, Ordering::Release);
    }
    if scene.atlas_uploads.is_empty() {
        return;
    }
    let atlas = atlases.entry(scene.atlas).or_insert_with(|| {
        let texture = atlas_texture(
            device,
            GLYPH_ATLAS_SIZE,
            TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
        );
        let bind_group = atlas_bind_group(device, layout, &texture, sampler);
        GpuAtlas {
            texture,
            bind_group,
        }
    });
    for upload in &scene.atlas_uploads {
        queue.write_texture(
            TexelCopyTextureInfo {
                texture: &atlas.texture,
                mip_level: 0,
                origin: Origin3d {
                    x: upload.origin.x,
                    y: upload.origin.y,
                    z: 0,
                },
                aspect: TextureAspect::All,
            },
            &upload.pixels,
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(upload.size.x * 4),
                rows_per_image: Some(upload.size.y),
            },
            Extent3d {
                width: upload.size.x,
                height: upload.size.y,
                depth_or_array_layers: 1,
            },
        );
    }
}

#[derive(Debug, PartialEq, Eq)]
struct UploadSlice {
    scene: usize,
    instances: std::ops::Range<usize>,
    offset: usize,
}

#[derive(Default)]
struct UploadChunk {
    slices: Vec<UploadSlice>,
    instances: usize,
}

/// Preserve scene and instance order while bounding each upload. Empty scenes
/// still get a pass, because clearing an image does not require any instances.
fn plan_uploads(counts: impl IntoIterator<Item = usize>, limit: usize) -> Vec<UploadChunk> {
    assert!(limit > 0);
    let mut chunks = Vec::new();
    let mut chunk = UploadChunk::default();
    for (scene, count) in counts.into_iter().enumerate() {
        let mut start = 0;
        loop {
            if chunk.instances == limit {
                chunks.push(std::mem::take(&mut chunk));
            }
            let len = (count - start).min(limit - chunk.instances);
            chunk.slices.push(UploadSlice {
                scene,
                instances: start..start + len,
                offset: chunk.instances,
            });
            chunk.instances += len;
            start += len;
            if start == count {
                break;
            }
        }
    }
    if !chunk.slices.is_empty() {
        chunks.push(chunk);
    }
    chunks
}

/// Instances to allocate for `required`, doubling up to `limit`.
fn buffer_capacity(required: u64, limit: u64) -> u64 {
    debug_assert!(required <= limit);
    required
        .checked_next_power_of_two()
        .unwrap_or(required)
        .min(limit)
}

/// Draws the pending scenes into their terminals' textures, in the
/// `RenderGraph` schedule: after every prepare system and before cameras
/// sample the textures, in Bevy's own submission. Scenes whose target or
/// source textures, or the pipelines, are not ready stay pending.
pub(super) fn render_batch_scenes(
    mut render_context: RenderContext,
    mut pending: ResMut<PendingBatchScenes>,
    mut gpu: ResMut<BatchGpuState>,
    pipelines: Option<Res<BatchPipelines>>,
    pipeline_cache: Res<PipelineCache>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    queue: Res<RenderQueue>,
) {
    for atlas in std::mem::take(&mut pending.released_atlases) {
        gpu.atlases.remove(&atlas);
    }
    if pending.scenes.is_empty() {
        return;
    }
    let Some(pipelines) = pipelines else {
        return;
    };
    let (Some(alpha_pipeline), Some(replace_pipeline)) = (
        pipeline_cache.get_render_pipeline(pipelines.alpha),
        pipeline_cache.get_render_pipeline(pipelines.replace),
    ) else {
        return;
    };
    let layout = pipeline_cache.get_bind_group_layout(&pipelines.layout);
    let scenes = std::mem::take(&mut pending.scenes);
    let mut renderable = Vec::with_capacity(scenes.len());
    for scene in scenes.into_values() {
        let Some(target) = gpu_images.get(scene.destination) else {
            pending.scenes.insert(scene.destination, scene);
            continue;
        };
        let target_size = target.texture_descriptor.size;
        if target_size.width != scene.destination_size.x
            || target_size.height != scene.destination_size.y
        {
            // An Image asset replacement can coexist with its previous render asset for a frame.
            // Keep the complete replacement scene pending until the matching GPU texture is ready.
            pending.scenes.insert(scene.destination, scene);
            continue;
        }
        if scene
            .batches
            .iter()
            .any(|batch| batch.texture != scene.atlas && gpu_images.get(batch.texture).is_none())
        {
            pending.scenes.insert(scene.destination, scene);
            continue;
        }
        renderable.push(scene);
    }
    if renderable.is_empty() {
        return;
    }

    let device = render_context.render_device().clone();
    let BatchGpuState {
        instances,
        atlases,
        empty_atlas,
    } = &mut *gpu;
    let empty_atlas = empty_atlas.get_or_insert_with(|| {
        let empty = atlas_texture(&device, 1, TextureUsages::TEXTURE_BINDING);
        atlas_bind_group(&device, &layout, &empty, &pipelines.atlas_sampler)
    });
    // Glyphs that did not fit a terminal's atlas are drawn from Bevy's font
    // atlases, rarely; their bind groups live for this draw only.
    let mut source_bind_groups = HashMap::<AssetId<Image>, BindGroup>::default();
    for scene in &renderable {
        upload_atlas(
            atlases,
            &device,
            &queue,
            &layout,
            &pipelines.atlas_sampler,
            scene,
        );
        for batch in scene
            .batches
            .iter()
            .filter(|batch| batch.texture != scene.atlas)
        {
            let image = gpu_images
                .get(batch.texture)
                .expect("glyph readiness was checked before bind-group creation");
            source_bind_groups.entry(batch.texture).or_insert_with(|| {
                device.create_bind_group(
                    "bevy_terminal glyph atlas",
                    &layout,
                    &BindGroupEntries::sequential((&image.texture_view, &image.sampler)),
                )
            });
        }
    }

    // Each chunk of instances gets its own buffer, so every scene is recorded
    // in one encoder and nothing is submitted before Bevy's submission. WGPU's
    // minimum supported buffer size exceeds one instance; the limit is also
    // clamped to the host address space.
    let buffer_limit = device.limits().max_buffer_size;
    let instance_limit = (buffer_limit / size_of::<QuadInstance>() as u64)
        .min(u64::from(u32::MAX))
        .min(usize::MAX as u64) as usize;
    let chunks = plan_uploads(
        renderable.iter().map(|scene| scene.instances.len()),
        instance_limit,
    );
    instances.truncate(chunks.len());
    instances.resize_with(chunks.len(), || {
        let mut buffer = RawBufferVec::new(BufferUsages::VERTEX);
        buffer.set_label(Some("bevy_terminal terminal instances"));
        buffer
    });
    for (chunk, buffer) in chunks.iter().zip(instances.iter_mut()) {
        buffer.clear();
        for slice in &chunk.slices {
            for instance in &renderable[slice.scene].instances[slice.instances.clone()] {
                buffer.push(*instance);
            }
        }
        if !buffer.is_empty() {
            let capacity = buffer_capacity(buffer.len() as u64, instance_limit as u64);
            buffer.reserve(capacity as usize, &device);
            buffer.write_buffer(&device, &queue);
        }
    }

    let encoder = render_context.command_encoder();
    for (chunk, buffer) in chunks.iter().zip(instances.iter()) {
        for slice in &chunk.slices {
            let scene = &renderable[slice.scene];
            let target = gpu_images
                .get(scene.destination)
                .expect("destination readiness was checked before encoding");
            let load = if scene.clear && slice.instances.start == 0 {
                LoadOp::Clear(scene.clear_color.to_linear().into())
            } else {
                LoadOp::Load
            };
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("bevy_terminal terminal batch"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &target.texture_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load,
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if !slice.instances.is_empty() {
                let vertex_buffer = buffer.buffer().expect("non-empty chunks are written");
                pass.set_vertex_buffer(
                    0,
                    *vertex_buffer.slice((slice.offset * size_of::<QuadInstance>()) as u64..),
                );
                let mut current_blend = None;
                for batch in &scene.batches {
                    let start = (batch.start as usize).max(slice.instances.start);
                    let end =
                        (batch.start as usize + batch.count as usize).min(slice.instances.end);
                    if start >= end {
                        continue;
                    }
                    if current_blend != Some(batch.blend) {
                        current_blend = Some(batch.blend);
                        pass.set_pipeline(match batch.blend {
                            Blend::Alpha => alpha_pipeline,
                            Blend::Replace => replace_pipeline,
                        });
                    }
                    let bind_group = if batch.texture == scene.atlas {
                        atlases
                            .get(&scene.atlas)
                            .map_or(&*empty_atlas, |atlas| &atlas.bind_group)
                    } else {
                        &source_bind_groups[&batch.texture]
                    };
                    pass.set_bind_group(0, bind_group, &[]);
                    pass.draw(
                        0..6,
                        (start - slice.instances.start) as u32
                            ..(end - slice.instances.start) as u32,
                    );
                }
            }
        }
    }
    // Recorded for this frame's submission, ahead of any later scene, so a
    // partial repaint built on an acknowledged scene draws after it.
    for scene in &renderable {
        if let Some((submitted, generation)) = &scene.submission {
            submitted.store(*generation, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod upload_tests {
    use super::*;

    #[test]
    fn bounded_uploads_preserve_every_instance_and_empty_scene() {
        let counts = [3, 0, 4, 1];
        for limit in [1, 2, 3, 8, 16] {
            let chunks = plan_uploads(counts, limit);
            let mut visited = Vec::new();
            let mut clears = [0; 4];
            let mut completions = [0; 4];
            for chunk in chunks {
                assert!(chunk.instances <= limit);
                for slice in chunk.slices {
                    assert!(slice.offset + slice.instances.len() <= chunk.instances);
                    clears[slice.scene] += usize::from(slice.instances.start == 0);
                    completions[slice.scene] +=
                        usize::from(slice.instances.end == counts[slice.scene]);
                    visited.extend(slice.instances.map(|index| (slice.scene, index)));
                }
            }
            assert_eq!(
                visited,
                vec![
                    (0, 0),
                    (0, 1),
                    (0, 2),
                    (2, 0),
                    (2, 1),
                    (2, 2),
                    (2, 3),
                    (3, 0)
                ]
            );
            assert_eq!(clears, [1; 4]);
            assert_eq!(completions, [1; 4]);
        }
    }

    #[test]
    fn buffer_growth_respects_non_power_of_two_limits_and_overflow() {
        assert_eq!(buffer_capacity(96, 100), 100);
        assert_eq!(buffer_capacity(48, 100), 64);
        assert_eq!(buffer_capacity(u64::MAX - 1, u64::MAX), u64::MAX - 1);
        assert_eq!(plan_uploads([3, 0, 4, 1], 8).len(), 1);
    }
}
