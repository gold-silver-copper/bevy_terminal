//! Extraction, resource lifetimes, and GPU submission.
use super::{
    BatchScene, GLYPH_ATLAS_SIZE, GLYPH_FORMAT, PendingBatchScenes, QuadInstance, SceneQueue,
    TARGET_FORMAT,
};
use bevy::{
    platform::collections::HashMap,
    prelude::*,
    render::{
        MainWorld,
        render_asset::RenderAssets,
        render_resource::{
            BindGroup, BindGroupEntry, BindGroupLayout, BindGroupLayoutEntry, BindingResource,
            BindingType, BlendState, Buffer, BufferDescriptor, BufferUsages, ColorTargetState,
            ColorWrites, CommandEncoderDescriptor, Extent3d, LoadOp, MultisampleState, Operations,
            Origin3d, PipelineCompilationOptions, PipelineLayoutDescriptor, PrimitiveState,
            RawFragmentState, RawRenderPipelineDescriptor, RawVertexBufferLayout, RawVertexState,
            RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline, Sampler,
            SamplerBindingType, SamplerDescriptor, ShaderModuleDescriptor, ShaderSource,
            ShaderStages, StoreOp, TexelCopyBufferLayout, TexelCopyTextureInfo, Texture,
            TextureAspect, TextureDescriptor, TextureDimension, TextureSampleType, TextureUsages,
            TextureViewDescriptor, TextureViewDimension, VertexAttribute, VertexFormat,
            VertexStepMode,
        },
        renderer::{RenderDevice, RenderQueue},
        texture::GpuImage,
    },
};
use std::sync::atomic::Ordering;

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
}

pub(super) fn batch_scenes_can_render_early(pending: Res<PendingBatchScenes>) -> bool {
    !pending.scenes.is_empty()
        && pending
            .scenes
            .values()
            .all(|scene| !scene.requires_prepared_assets)
}

#[derive(Default, Resource)]
pub(super) struct BatchGpuState {
    pub(super) vertex_buffer: Option<Buffer>,
    pub(super) vertex_capacity: u64,
    /// Persistent CPU staging for instance serialization, reused every frame.
    pub(super) staging: Vec<u8>,
    pub(super) texture_layout: Option<BindGroupLayout>,
    pub(super) pipeline: Option<RenderPipeline>,
    pub(super) replace_pipeline: Option<RenderPipeline>,
    /// Terminals' glyph atlases, created on their first entry.
    pub(super) atlases: HashMap<AssetId<Image>, GpuAtlas>,
    pub(super) atlas_sampler: Option<Sampler>,
    /// Binding for scenes whose atlas holds nothing yet (solid quads only).
    pub(super) empty_atlas: Option<BindGroup>,
}

/// A terminal's glyph atlas texture.
pub(super) struct GpuAtlas {
    texture: Texture,
    bind_group: BindGroup,
}

impl BatchGpuState {
    fn ensure_pipeline(&mut self, device: &RenderDevice) {
        if self.pipeline.is_some() {
            return;
        }
        let texture_layout = device.create_bind_group_layout(
            "bevy_terminal batch texture layout",
            &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        );
        let pipeline = create_pipeline(
            device,
            &[&texture_layout],
            "fragment",
            BlendState::ALPHA_BLENDING,
        );
        let replace_pipeline =
            create_pipeline(device, &[&texture_layout], "fragment", BlendState::REPLACE);
        let sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("bevy_terminal glyph atlas sampler"),
            ..default()
        });
        let empty = device.create_texture(&TextureDescriptor {
            label: Some("bevy_terminal empty glyph atlas"),
            size: Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: GLYPH_FORMAT,
            usage: TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        self.empty_atlas = Some(atlas_bind_group(device, &texture_layout, &empty, &sampler));
        self.atlas_sampler = Some(sampler);
        self.texture_layout = Some(texture_layout);
        self.pipeline = Some(pipeline);
        self.replace_pipeline = Some(replace_pipeline);
    }
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
        &[
            BindGroupEntry {
                binding: 0,
                resource: BindingResource::TextureView(&view),
            },
            BindGroupEntry {
                binding: 1,
                resource: BindingResource::Sampler(sampler),
            },
        ],
    )
}

/// Writes a scene's new atlas entries, creating the atlas on its first.
fn upload_atlas(
    gpu: &mut BatchGpuState,
    device: &RenderDevice,
    queue: &RenderQueue,
    scene: &BatchScene,
) {
    if scene.atlas_uploads.is_empty() && gpu.atlases.contains_key(&scene.atlas) {
        return;
    }
    if !gpu.atlases.contains_key(&scene.atlas) && !scene.atlas_fresh {
        // Earlier entries never reached this texture (the render world was
        // reset): have the main world rebuild the atlas.
        scene.atlas_lost.store(true, Ordering::Release);
    }
    if scene.atlas_uploads.is_empty() {
        return;
    }
    let BatchGpuState {
        atlases,
        texture_layout,
        atlas_sampler,
        ..
    } = gpu;
    let atlas = atlases.entry(scene.atlas).or_insert_with(|| {
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("bevy_terminal glyph atlas"),
            size: Extent3d {
                width: GLYPH_ATLAS_SIZE,
                height: GLYPH_ATLAS_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: GLYPH_FORMAT,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let bind_group = atlas_bind_group(
            device,
            texture_layout
                .as_ref()
                .expect("pipeline initialization creates texture layout"),
            &texture,
            atlas_sampler
                .as_ref()
                .expect("pipeline initialization creates the sampler"),
        );
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

pub(super) fn reset_batch_gpu_state(mut gpu: ResMut<BatchGpuState>) {
    *gpu = BatchGpuState::default();
}

pub(super) fn create_pipeline(
    device: &RenderDevice,
    layouts: &[&BindGroupLayout],
    fragment_entry: &'static str,
    blend: BlendState,
) -> RenderPipeline {
    let shader = device.create_and_validate_shader_module(ShaderModuleDescriptor {
        label: Some("bevy_terminal batch shader"),
        source: ShaderSource::Wgsl(BATCH_SHADER.into()),
    });
    let raw_layouts = layouts
        .iter()
        .map(|layout| Some(&***layout))
        .collect::<Vec<_>>();
    let layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
        label: Some("bevy_terminal batch pipeline layout"),
        bind_group_layouts: &raw_layouts,
        immediate_size: 0,
    });
    let compilation = PipelineCompilationOptions::default();
    const ATTRIBUTES: [VertexAttribute; 4] = [
        VertexAttribute {
            format: VertexFormat::Float32x4,
            offset: 0,
            shader_location: 0,
        },
        VertexAttribute {
            format: VertexFormat::Float32x4,
            offset: 16,
            shader_location: 1,
        },
        VertexAttribute {
            format: VertexFormat::Float32x4,
            offset: 32,
            shader_location: 2,
        },
        VertexAttribute {
            format: VertexFormat::Float32,
            offset: 48,
            shader_location: 3,
        },
    ];
    let vertex_buffers = [RawVertexBufferLayout {
        array_stride: INSTANCE_BYTES as u64,
        step_mode: VertexStepMode::Instance,
        attributes: &ATTRIBUTES,
    }];
    device.create_render_pipeline(&RawRenderPipelineDescriptor {
        label: Some("bevy_terminal batch pipeline"),
        layout: Some(&layout),
        vertex: RawVertexState {
            module: &shader,
            entry_point: Some("vertex"),
            compilation_options: compilation.clone(),
            buffers: &vertex_buffers,
        },
        fragment: Some(RawFragmentState {
            module: &shader,
            entry_point: Some(fragment_entry),
            compilation_options: compilation,
            targets: &[Some(ColorTargetState {
                format: TARGET_FORMAT,
                blend: Some(blend),
                write_mask: ColorWrites::ALL,
            })],
        }),
        primitive: PrimitiveState::default(),
        depth_stencil: None,
        multisample: MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

/// Appends the 52-byte GPU encoding of each instance, writing whole instances
/// into pre-sized chunks instead of growing the vector one scalar at a time.
pub(super) fn append_instance_bytes(instances: &[QuadInstance], bytes: &mut Vec<u8>) {
    let start = bytes.len();
    bytes.resize(start + instances.len() * INSTANCE_BYTES, 0);
    for (chunk, instance) in bytes[start..]
        .as_chunks_mut::<INSTANCE_BYTES>()
        .0
        .iter_mut()
        .zip(instances)
    {
        let values = [
            instance.rect.to_array(),
            instance.uv.to_array(),
            instance.color.to_array(),
        ];
        let values = values
            .as_flattened()
            .iter()
            .chain(std::iter::once(&instance.background));
        for (slot, value) in chunk.as_chunks_mut::<4>().0.iter_mut().zip(values) {
            slot.copy_from_slice(&value.to_ne_bytes());
        }
    }
}

const INSTANCE_BYTES: usize = 52;

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

fn buffer_capacity(required: u64, limit: u64) -> u64 {
    debug_assert!(required <= limit);
    required
        .checked_next_power_of_two()
        .unwrap_or(required)
        .min(limit)
}

pub(super) fn render_batch_scenes(
    mut pending: ResMut<PendingBatchScenes>,
    mut gpu: ResMut<BatchGpuState>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    for atlas in pending.released_atlases.drain(..) {
        gpu.atlases.remove(&atlas);
    }
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

    gpu.ensure_pipeline(&device);
    // Glyphs that did not fit a terminal's atlas are drawn from Bevy's font
    // atlases, rarely; their bind groups live for this draw only.
    let mut source_bind_groups = HashMap::<AssetId<Image>, BindGroup>::default();
    for scene in &renderable {
        upload_atlas(&mut gpu, &device, &queue, scene);
        for batch in scene
            .batches
            .iter()
            .filter(|batch| batch.texture != scene.atlas)
        {
            let texture = batch.texture;
            let image = gpu_images
                .get(texture)
                .expect("glyph readiness was checked before bind-group creation");
            source_bind_groups.entry(texture).or_insert_with(|| {
                device.create_bind_group(
                    "bevy_terminal glyph atlas",
                    gpu.texture_layout
                        .as_ref()
                        .expect("pipeline initialization creates texture layout"),
                    &[
                        BindGroupEntry {
                            binding: 0,
                            resource: BindingResource::TextureView(&image.texture_view),
                        },
                        BindGroupEntry {
                            binding: 1,
                            resource: BindingResource::Sampler(&image.sampler),
                        },
                    ],
                )
            });
        }
    }

    let buffer_limit = device.limits().max_buffer_size;
    // WGPU's minimum supported buffer size exceeds one instance. Clamp to the
    // host address space as well so serialization arithmetic cannot overflow.
    let instance_limit = (buffer_limit / INSTANCE_BYTES as u64)
        .min((usize::MAX / INSTANCE_BYTES) as u64)
        .min(u64::from(u32::MAX)) as usize;
    let chunks = plan_uploads(
        renderable.iter().map(|scene| scene.instances.len()),
        instance_limit,
    );
    let mut staging = std::mem::take(&mut gpu.staging);
    for chunk in chunks {
        staging.clear();
        for slice in &chunk.slices {
            append_instance_bytes(
                &renderable[slice.scene].instances[slice.instances.clone()],
                &mut staging,
            );
        }
        if !staging.is_empty() {
            let required = staging.len() as u64;
            if required > gpu.vertex_capacity {
                gpu.vertex_capacity = buffer_capacity(required, buffer_limit);
                gpu.vertex_buffer = Some(device.create_buffer(&BufferDescriptor {
                    label: Some("bevy_terminal terminal instances"),
                    size: gpu.vertex_capacity,
                    usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
            }
            queue.write_buffer(
                gpu.vertex_buffer
                    .as_ref()
                    .expect("non-empty upload allocates a buffer"),
                0,
                &staging,
            );
        }
        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("bevy_terminal terminal batch"),
        });
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
                let vertex_buffer = gpu
                    .vertex_buffer
                    .as_ref()
                    .expect("upload allocated a buffer");
                pass.set_vertex_buffer(
                    0,
                    *vertex_buffer.slice((slice.offset * INSTANCE_BYTES) as u64..),
                );
                let mut current_replace = None;
                for batch in &scene.batches {
                    let start = (batch.start as usize).max(slice.instances.start);
                    let end =
                        (batch.start as usize + batch.count as usize).min(slice.instances.end);
                    if start >= end {
                        continue;
                    }
                    if current_replace != Some(batch.replace) {
                        current_replace = Some(batch.replace);
                        let pipeline = if batch.replace {
                            &gpu.replace_pipeline
                        } else {
                            &gpu.pipeline
                        };
                        pass.set_pipeline(pipeline.as_ref().expect("pipeline was initialized"));
                    }
                    let bind_group = if batch.texture == scene.atlas {
                        gpu.atlases
                            .get(&scene.atlas)
                            .map(|atlas| &atlas.bind_group)
                            .or(gpu.empty_atlas.as_ref())
                    } else {
                        source_bind_groups.get(&batch.texture)
                    };
                    pass.set_bind_group(0, bind_group.expect("atlas bind group was prepared"), &[]);
                    pass.draw(
                        0..6,
                        (start - slice.instances.start) as u32
                            ..(end - slice.instances.start) as u32,
                    );
                }
            }
        }
        // Queue writes take effect on submission. Submit each chunk before
        // overwriting the retained buffer for the next chunk.
        queue.submit([encoder.finish()]);
        for slice in &chunk.slices {
            let scene = &renderable[slice.scene];
            if slice.instances.end == scene.instances.len()
                && let Some((submitted, generation)) = &scene.submission
            {
                submitted.store(*generation, Ordering::Release);
            }
        }
    }
    gpu.staging = staging;
}

pub(super) const BATCH_SHADER: &str = r#"
@group(0) @binding(0) var glyph_atlas: texture_2d<f32>;
@group(0) @binding(1) var glyph_sampler: sampler;

struct VertexInput {
    @location(0) rect: vec4<f32>,
    @location(1) uv: vec4<f32>,
    @location(2) color: vec4<f32>,
    @location(3) background: f32,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) solid: u32,
    @location(3) @interpolate(flat) background: f32,
}

@vertex
fn vertex(input: VertexInput, @builtin(vertex_index) index: u32) -> VertexOutput {
    let corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 1.0),
    );
    let corner = corners[index % 6u];
    var output: VertexOutput;
    output.position = vec4<f32>(mix(input.rect.xy, input.rect.zw, corner), 0.0, 1.0);
    output.uv = mix(input.uv.xy, input.uv.zw, corner);
    output.color = input.color;
    output.solid = select(0u, 1u, input.uv.w < 0.0);
    output.background = input.background;
    return output;
}

fn linearize(v: f32) -> f32 {
    return select(pow((v + 0.055) / 1.055, 2.4), v / 12.92, v <= 0.04045);
}

fn unlinearize(v: f32) -> f32 {
    return select(pow(v, 1.0 / 2.4) * 1.055 - 0.055, v * 12.92, v <= 0.0031308);
}

fn luminance(color: vec3<f32>) -> f32 {
    return dot(color, vec3<f32>(0.2126, 0.7152, 0.0722));
}

@fragment
fn fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    if input.solid != 0u {
        return input.color;
    }
    let sample = textureSample(glyph_atlas, glyph_sampler, input.uv);
    if input.color.a < 0.0 {
        return sample;
    }
    // Ghostty's linear-corrected blending: blend in linear light, with the
    // coverage remapped so the result has the luminance a gamma-space blend
    // of the text and cell background would have.
    var coverage = sample.a;
    let bg_l = input.background;
    let fg_l = luminance(input.color.rgb);
    if bg_l >= 0.0 && abs(fg_l - bg_l) > 0.001 {
        let blend_l = linearize(unlinearize(fg_l) * coverage + unlinearize(bg_l) * (1.0 - coverage));
        coverage = clamp((blend_l - bg_l) / (fg_l - bg_l), 0.0, 1.0);
    }
    return vec4<f32>(input.color.rgb, input.color.a * coverage);
}
"#;

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
