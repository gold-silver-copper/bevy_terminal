//! Extraction, resource lifetimes, and GPU submission.
use super::{
    BatchMainState, PendingBatchScenes, QuadInstance, TARGET_FORMAT, TerminalRenderer,
    TerminalTexture,
};
use bevy::{
    platform::collections::{HashMap, HashSet},
    prelude::*,
    render::{
        MainWorld,
        render_asset::RenderAssets,
        render_resource::{
            BindGroup, BindGroupEntry, BindGroupLayout, BindGroupLayoutEntry, BindingResource,
            BindingType, BlendState, Buffer, BufferDescriptor, BufferUsages, ColorTargetState,
            ColorWrites, CommandEncoderDescriptor, LoadOp, MultisampleState, Operations,
            PipelineCompilationOptions, PipelineLayoutDescriptor, PrimitiveState, RawFragmentState,
            RawRenderPipelineDescriptor, RawVertexBufferLayout, RawVertexState,
            RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline, SamplerBindingType,
            ShaderModuleDescriptor, ShaderSource, ShaderStages, StoreOp, TextureId,
            TextureSampleType, TextureViewDimension, VertexAttribute, VertexFormat, VertexStepMode,
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

pub(super) fn collect_batch_scenes(main_world: &mut World, pending: &mut PendingBatchScenes) {
    pending.live_textures.clear();
    let mut terminals = main_world
        .query_filtered::<(&mut BatchMainState, &TerminalTexture), With<TerminalRenderer>>();
    let mut alive = HashSet::<AssetId<Image>>::default();
    for (mut state, output) in terminals.iter_mut(main_world) {
        if output.measured().is_none() {
            state.pending = None;
            continue;
        }
        alive.insert(state.output.id());
        pending.live_textures.insert(state.glyph_atlas.image.id());
        if let Some(scene) = state.pending.take() {
            // An unacknowledged submission is replaced by a full scene in the
            // main world, so superseding it cannot lose intermediate rows.
            pending.scenes.insert(scene.destination, scene);
        }
    }
    pending
        .scenes
        .retain(|destination, _| alive.contains(destination));
    // Unified atlases stay cached for idle terminals. Fallback source atlases
    // belong to Bevy and may outlive every terminal; retain their bind groups
    // only while a pending scene uses them.
    pending.live_textures.extend(
        pending
            .scenes
            .values()
            .flat_map(|scene| scene.batches.iter().map(|batch| batch.texture)),
    );
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
    pub(super) texture_bind_groups: HashMap<AssetId<Image>, (TextureId, BindGroup)>,
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
        self.texture_layout = Some(texture_layout);
        self.pipeline = Some(pipeline);
        self.replace_pipeline = Some(replace_pipeline);
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
    const ATTRIBUTES: [VertexAttribute; 3] = [
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
    ];
    let vertex_buffers = [RawVertexBufferLayout {
        array_stride: 48,
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

/// Appends the 48-byte GPU encoding of each instance, writing whole instances
/// into pre-sized chunks instead of growing the vector one scalar at a time.
pub(super) fn append_instance_bytes(instances: &[QuadInstance], bytes: &mut Vec<u8>) {
    let start = bytes.len();
    bytes.resize(start + instances.len() * 48, 0);
    for (chunk, instance) in bytes[start..]
        .as_chunks_mut::<48>()
        .0
        .iter_mut()
        .zip(instances)
    {
        let values = [
            instance.rect.to_array(),
            instance.uv.to_array(),
            instance.color.to_array(),
        ];
        for (slot, value) in chunk
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(values.as_flattened())
        {
            slot.copy_from_slice(&value.to_ne_bytes());
        }
    }
}

pub(super) fn render_batch_scenes(
    mut pending: ResMut<PendingBatchScenes>,
    mut gpu: ResMut<BatchGpuState>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    gpu.texture_bind_groups.retain(|texture, _| {
        pending.live_textures.contains(texture) && gpu_images.get(*texture).is_some()
    });
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
            .any(|batch| gpu_images.get(batch.texture).is_none())
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
    // Serialize every scene into one persistent staging buffer; each scene
    // draws from its own byte offset so one buffer write, one command encoder
    // and one submission cover all terminals.
    let mut staging = std::mem::take(&mut gpu.staging);
    staging.clear();
    let mut offsets = Vec::with_capacity(renderable.len());
    for scene in &renderable {
        offsets.push(staging.len() as u64);
        append_instance_bytes(&scene.instances, &mut staging);
    }
    if !staging.is_empty() {
        let required = staging.len() as u64;
        if required > gpu.vertex_capacity {
            gpu.vertex_capacity = required.next_power_of_two();
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
                .expect("non-empty instances allocate a vertex buffer"),
            0,
            &staging,
        );
    }
    gpu.staging = staging;

    for scene in &renderable {
        for batch in &scene.batches {
            let texture = batch.texture;
            let image = gpu_images
                .get(texture)
                .expect("glyph readiness was checked before bind-group creation");
            let texture_id = image.texture.id();
            if gpu
                .texture_bind_groups
                .get(&texture)
                .is_none_or(|(cached_id, _)| *cached_id != texture_id)
            {
                let bind_group = device.create_bind_group(
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
                );
                gpu.texture_bind_groups
                    .insert(texture, (texture_id, bind_group));
            }
        }
    }

    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
        label: Some("bevy_terminal terminal batch"),
    });
    for (scene, offset) in renderable.iter().zip(&offsets) {
        let target = gpu_images
            .get(scene.destination)
            .expect("destination readiness was checked before encoding");
        let load = if scene.clear {
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
        if !scene.instances.is_empty()
            && let Some(vertex_buffer) = &gpu.vertex_buffer
        {
            pass.set_vertex_buffer(0, *vertex_buffer.slice(*offset..));
            let mut current_replace = None;
            for batch in &scene.batches {
                if current_replace != Some(batch.replace) {
                    current_replace = Some(batch.replace);
                    let pipeline = if batch.replace {
                        &gpu.replace_pipeline
                    } else {
                        &gpu.pipeline
                    };
                    pass.set_pipeline(pipeline.as_ref().expect("pipeline was initialized"));
                }
                pass.set_bind_group(
                    0,
                    gpu.texture_bind_groups
                        .get(&batch.texture)
                        .map(|(_, bind_group)| bind_group)
                        .expect("atlas bind group was prepared"),
                    &[],
                );
                pass.draw(0..6, batch.start..batch.start + batch.count);
            }
        }
        drop(pass);
    }
    queue.submit([encoder.finish()]);
    for scene in renderable {
        if let Some((submitted, generation)) = scene.submission {
            submitted.store(generation, Ordering::Release);
        }
    }
}

pub(super) const BATCH_SHADER: &str = r#"
@group(0) @binding(0) var glyph_atlas: texture_2d<f32>;
@group(0) @binding(1) var glyph_sampler: sampler;

struct VertexInput {
    @location(0) rect: vec4<f32>,
    @location(1) uv: vec4<f32>,
    @location(2) color: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) solid: u32,
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
    return output;
}

@fragment
fn fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    if input.solid != 0u {
        return input.color;
    }
    let sample = textureSample(glyph_atlas, glyph_sampler, input.uv);
    if input.color.a >= 0.0 {
        return vec4<f32>(input.color.rgb, input.color.a * sample.a);
    }
    return sample;
}
"#;
