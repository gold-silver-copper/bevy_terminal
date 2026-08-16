//! Compact render-world terminal batch.

use std::collections::HashMap;
use std::time::Instant;

use bevy::{
    asset::{AssetId, RenderAssetUsages},
    image::ImageSampler,
    prelude::*,
    render::{
        ExtractSchedule, MainWorld, Render, RenderApp, RenderStartup, RenderSystems,
        render_asset::RenderAssets,
        render_resource::{
            BindGroup, BindGroupEntry, BindGroupLayout, BindGroupLayoutEntry, BindingResource,
            BindingType, BlendState, Buffer, BufferDescriptor, BufferUsages, ColorTargetState,
            ColorWrites, CommandEncoderDescriptor, Extent3d, LoadOp, MultisampleState, Operations,
            PipelineCompilationOptions, PipelineLayoutDescriptor, PrimitiveState, RawFragmentState,
            RawRenderPipelineDescriptor, RawVertexBufferLayout, RawVertexState,
            RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline, SamplerBindingType,
            ShaderModuleDescriptor, ShaderSource, ShaderStages, StoreOp, TextureDimension,
            TextureFormat, TextureId, TextureSampleType, TextureUsages, TextureViewDimension,
            VertexAttribute, VertexFormat, VertexStepMode,
        },
        renderer::{RenderDevice, RenderQueue},
        texture::GpuImage,
    },
    text::{
        ComputedTextBlock, FontAtlasSet, FontCx, FontHinting, LayoutCx, LetterSpacing, LineBreak,
        LineHeight, ScaleCx, TextBounds, TextLayoutInfo, TextPipeline,
    },
    window::PrimaryWindow,
};
use ratatui::buffer::{CellDiffOption, CellWidth};

use super::{
    PixelGeometry, ResolvedStyle, TerminalRenderConfig, TerminalRenderScale, TextRun,
    block_geometry, cursor_should_be_visible, line_glyph, push_block, push_line_glyph,
    push_quadrants, quadrant_mask, row_cells, text_font,
};
use crate::{TerminalSnapshot, TerminalSurface};

const TARGET_FORMAT: TextureFormat = TextureFormat::Rgba8Unorm;
const GLYPH_FORMAT: TextureFormat = TextureFormat::Rgba8UnormSrgb;
const GLYPH_ATLAS_SIZE: u32 = 2048;

/// Whether the compact terminal texture is also exposed through a Bevy UI image node.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TerminalBatchPresentation {
    /// Spawn one Bevy UI image node that presents the terminal texture.
    #[default]
    Ui,
    /// Render only the terminal texture. Useful for headless rendering and custom composition.
    Headless,
}

/// The renderer-owned terminal texture and its current pixel dimensions.
#[derive(Clone, Debug, Resource)]
pub struct TerminalBatchOutput {
    /// Render-world image containing the completed terminal.
    ///
    /// The handle changes when the grid dimensions or raster scale change, so
    /// custom presentation code should observe this resource for changes.
    pub image: Handle<Image>,
    /// Physical pixel dimensions of `image`.
    pub size: UVec2,
    /// Logical dimensions used by the optional Bevy UI presentation node.
    pub logical_size: Vec2,
    /// Physical pixels per logical pixel used to rasterize `image`.
    pub raster_scale: f32,
}

/// Marker on the optional UI image node used to present a compact terminal batch.
#[derive(Component, Debug)]
pub struct TerminalBatchRoot;

/// Counters for the most recent compact scene update.
#[derive(Clone, Copy, Debug, Default, Resource)]
pub struct TerminalBatchStats {
    /// Main-world synchronization frames.
    pub sync_frames: u64,
    /// Frames that produced no terminal work.
    pub unchanged_frames: u64,
    /// Rows rebuilt into the latest payload.
    pub changed_rows: u32,
    /// Cells copied while updating the retained snapshot.
    pub snapshot_cells: u32,
    /// Solid rectangles in the latest payload.
    pub solid_quads: u32,
    /// Glyph rectangles in the latest payload.
    pub glyph_quads: u32,
    /// Draw batches in the latest payload.
    pub draw_batches: u32,
    /// CPU bytes transferred to the render world in the latest payload.
    pub extracted_bytes: u64,
    /// Shaped text sequences retained in the main-world glyph cache.
    pub cached_shapes: u32,
    /// Shape-cache misses in the latest update.
    pub shape_misses: u32,
    /// Nanoseconds spent updating the retained terminal snapshot.
    pub snapshot_ns: u64,
    /// Nanoseconds spent generating the compact CPU scene.
    pub scene_ns: u64,
    /// Persistent vertex-buffer growth operations implied by the latest payload.
    pub gpu_buffer_reallocations: u32,
    /// Queue buffer writes implied by the latest payload.
    pub gpu_write_calls: u32,
    /// Vertex bytes written for the latest payload.
    pub gpu_bytes_written: u64,
    /// Render passes recorded for the latest payload.
    pub render_passes: u32,
    /// Draw calls recorded for the latest payload.
    pub draw_calls: u32,
    /// Solid/glyph pipeline changes in deterministic paint order.
    pub pipeline_switches: u32,
    /// Glyph-atlas bind operations in the latest payload.
    pub atlas_bindings: u32,
}

/// Installs a compact renderer that draws one terminal texture in Bevy's render world.
///
/// Glyphs are shaped and rasterized by Bevy text. The terminal scene itself is represented by
/// compact GPU quad instances instead of one UI entity per run or rectangle. In [`Ui`](TerminalBatchPresentation::Ui)
/// mode, a single [`ImageNode`] places that texture in Bevy UI.
pub struct BevyGridBatchPlugin {
    surface: TerminalSurface,
    config: TerminalRenderConfig,
    presentation: TerminalBatchPresentation,
}

impl BevyGridBatchPlugin {
    /// Creates the compact renderer with its texture presented through Bevy UI.
    #[must_use]
    pub fn new(surface: TerminalSurface) -> Self {
        Self {
            surface,
            config: TerminalRenderConfig::default(),
            presentation: TerminalBatchPresentation::Ui,
        }
    }

    /// Replaces the renderer configuration.
    #[must_use]
    pub fn with_config(mut self, config: TerminalRenderConfig) -> Self {
        self.config = config;
        self
    }

    /// Selects texture-only rendering without creating a UI presentation node.
    #[must_use]
    pub fn headless(mut self) -> Self {
        self.presentation = TerminalBatchPresentation::Headless;
        self
    }

    /// Selects how the renderer-owned terminal texture is presented.
    #[must_use]
    pub fn with_presentation(mut self, presentation: TerminalBatchPresentation) -> Self {
        self.presentation = presentation;
        self
    }
}

impl Plugin for BevyGridBatchPlugin {
    fn build(&self, app: &mut App) {
        let raster_scale = resolve_raster_scale(self.config.render_scale, self.presentation, None);
        let raster_config = physical_config(&self.config, raster_scale);
        let logical_cell_size = raster_config.cell_size / raster_scale;
        self.surface
            .set_cell_size(logical_cell_size.x, logical_cell_size.y);
        let snapshot = self.surface.snapshot();
        let size = terminal_pixel_size(&snapshot, &raster_config);
        let output_image = make_target_image(size);
        let output = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(output_image);
        let glyph_atlas = {
            let mut images = app.world_mut().resource_mut::<Assets<Image>>();
            images.add(make_glyph_atlas_image())
        };

        let ui_root = if self.presentation == TerminalBatchPresentation::Ui {
            Some(
                app.world_mut()
                    .spawn((
                        TerminalBatchRoot,
                        super::TerminalRoot,
                        ImageNode::new(output.clone()),
                        presentation_node(size, &self.config, raster_scale),
                    ))
                    .id(),
            )
        } else {
            None
        };

        app.insert_resource(self.surface.clone())
            .insert_resource(self.config.clone())
            .insert_resource(TerminalBatchOutput {
                image: output.clone(),
                size,
                logical_size: size.as_vec2() / raster_scale,
                raster_scale,
            })
            .insert_resource(BatchMainState::new(
                output,
                glyph_atlas,
                ui_root,
                self.presentation,
                raster_scale,
                raster_config,
            ))
            .init_resource::<TerminalBatchStats>()
            .add_systems(
                Update,
                sync_batch_terminal.in_set(super::TerminalSystems::Sync),
            );

        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app
                .init_resource::<PendingBatchScene>()
                .init_resource::<BatchGpuState>()
                .add_systems(RenderStartup, reset_batch_gpu_state)
                .add_systems(ExtractSchedule, extract_batch_scene)
                .add_systems(
                    Render,
                    render_batch_scene
                        .run_if(batch_scene_can_render_early)
                        .in_set(RenderSystems::ExtractCommands),
                )
                .add_systems(
                    Render,
                    render_batch_scene.in_set(RenderSystems::PrepareMeshes),
                );
        }
    }
}

fn terminal_pixel_size(snapshot: &TerminalSnapshot, config: &TerminalRenderConfig) -> UVec2 {
    UVec2::new(
        (f32::from(snapshot.size().width) * config.cell_size.x)
            .round()
            .max(1.0) as u32,
        (f32::from(snapshot.size().height) * config.cell_size.y)
            .round()
            .max(1.0) as u32,
    )
}

fn make_target_image(size: UVec2) -> Image {
    let mut image = Image::new_uninit(
        Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TARGET_FORMAT,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.usage =
        TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_SRC;
    // The terminal is rasterized at its final physical resolution. Filtering it again in the UI
    // presentation stage softens glyph edges and can open seams between exact geometry cells.
    image.sampler = ImageSampler::nearest();
    image
}

fn make_glyph_atlas_image() -> Image {
    let mut image = Image::new_fill(
        Extent3d {
            width: GLYPH_ATLAS_SIZE,
            height: GLYPH_ATLAS_SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 0],
        GLYPH_FORMAT,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::nearest();
    image
}

fn presentation_node(size: UVec2, config: &TerminalRenderConfig, raster_scale: f32) -> Node {
    let origin = (config.origin * raster_scale).round() / raster_scale;
    Node {
        position_type: PositionType::Absolute,
        left: px(origin.x),
        top: px(origin.y),
        width: px(size.x as f32 / raster_scale),
        height: px(size.y as f32 / raster_scale),
        overflow: Overflow::clip(),
        ..default()
    }
}

fn resolve_raster_scale(
    configured: TerminalRenderScale,
    presentation: TerminalBatchPresentation,
    window_scale: Option<f32>,
) -> f32 {
    let requested = match configured {
        TerminalRenderScale::Automatic if presentation == TerminalBatchPresentation::Ui => {
            window_scale.unwrap_or(1.0)
        }
        TerminalRenderScale::Automatic => 1.0,
        TerminalRenderScale::Fixed(scale) => scale,
    };
    if requested.is_finite() && requested > 0.0 {
        requested.clamp(1.0, 8.0)
    } else {
        1.0
    }
}

fn physical_config(config: &TerminalRenderConfig, raster_scale: f32) -> TerminalRenderConfig {
    let mut physical = config.clone();
    physical.cell_size = (config.cell_size * raster_scale).round().max(Vec2::ONE);
    physical.font_size = (config.font_size * raster_scale).round().max(1.0);
    physical.origin = Vec2::ZERO;
    physical
}

#[derive(Clone)]
struct CachedGlyph {
    texture: AssetId<Image>,
    offset: Vec2,
    size: Vec2,
    uv: Vec4,
    alpha_mask: bool,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct SourceGlyph {
    texture: AssetId<Image>,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

struct UnifiedGlyphAtlas {
    image: Handle<Image>,
    glyphs: HashMap<SourceGlyph, Vec4>,
    cursor: UVec2,
    row_height: u32,
}

impl UnifiedGlyphAtlas {
    fn new(image: Handle<Image>) -> Self {
        Self {
            image,
            glyphs: HashMap::new(),
            cursor: UVec2::splat(1),
            row_height: 0,
        }
    }

    fn cache(&mut self, source: SourceGlyph, images: &mut Assets<Image>) -> Option<Vec4> {
        if let Some(uv) = self.glyphs.get(&source) {
            return Some(*uv);
        }
        if source.width == 0 || source.height == 0 || source.width + 2 > GLYPH_ATLAS_SIZE {
            return None;
        }

        let mut x = self.cursor.x;
        let mut y = self.cursor.y;
        if x + source.width + 1 > GLYPH_ATLAS_SIZE {
            x = 1;
            y = y.checked_add(self.row_height + 1)?;
            self.row_height = 0;
        }
        if y + source.height + 1 > GLYPH_ATLAS_SIZE {
            return None;
        }

        let pixels = {
            let source_image = images.get(source.texture)?;
            if source_image.texture_descriptor.format != GLYPH_FORMAT
                || source.x + source.width > source_image.width()
                || source.y + source.height > source_image.height()
            {
                return None;
            }
            let data = source_image.data.as_ref()?;
            let source_stride = source_image.width() as usize * 4;
            let row_bytes = source.width as usize * 4;
            let mut pixels = Vec::with_capacity(row_bytes * source.height as usize);
            for row in 0..source.height {
                let start = (source.y + row) as usize * source_stride + source.x as usize * 4;
                pixels.extend_from_slice(data.get(start..start + row_bytes)?);
            }
            pixels
        };

        let mut atlas = images.get_mut(&self.image)?;
        let data = atlas.data.as_mut()?;
        let atlas_stride = GLYPH_ATLAS_SIZE as usize * 4;
        let row_bytes = source.width as usize * 4;
        for row in 0..source.height {
            let source_start = row as usize * row_bytes;
            let target_start = (y + row) as usize * atlas_stride + x as usize * 4;
            data[target_start..target_start + row_bytes]
                .copy_from_slice(&pixels[source_start..source_start + row_bytes]);
        }

        self.cursor = UVec2::new(x + source.width + 1, y);
        self.row_height = self.row_height.max(source.height);
        let scale = GLYPH_ATLAS_SIZE as f32;
        let uv = Vec4::new(
            x as f32 / scale,
            y as f32 / scale,
            (x + source.width) as f32 / scale,
            (y + source.height) as f32 / scale,
        );
        self.glyphs.insert(source, uv);
        Some(uv)
    }

    fn clear(&mut self, images: &mut Assets<Image>) {
        self.glyphs.clear();
        self.cursor = UVec2::splat(1);
        self.row_height = 0;
        if let Some(mut image) = images.get_mut(&self.image)
            && let Some(data) = image.data.as_mut()
        {
            data.fill(0);
        }
    }
}

#[derive(Default)]
struct ShapeCaches {
    normal: HashMap<String, Vec<CachedGlyph>>,
    bold: HashMap<String, Vec<CachedGlyph>>,
    italic: HashMap<String, Vec<CachedGlyph>>,
    bold_italic: HashMap<String, Vec<CachedGlyph>>,
}

#[derive(Default)]
struct SceneScratch {
    backgrounds: Vec<QuadInstance>,
    foregrounds: Vec<QuadInstance>,
    glyphs: Vec<(AssetId<Image>, QuadInstance)>,
    decorations: Vec<QuadInstance>,
    cursor: Vec<QuadInstance>,
    styles: Vec<ResolvedStyle>,
}

impl SceneScratch {
    fn clear(&mut self) {
        self.backgrounds.clear();
        self.foregrounds.clear();
        self.glyphs.clear();
        self.decorations.clear();
        self.cursor.clear();
        self.styles.clear();
    }
}

impl ShapeCaches {
    fn select(&self, style: &ResolvedStyle) -> &HashMap<String, Vec<CachedGlyph>> {
        match (style.bold, style.italic) {
            (false, false) => &self.normal,
            (true, false) => &self.bold,
            (false, true) => &self.italic,
            (true, true) => &self.bold_italic,
        }
    }

    fn select_mut(&mut self, style: &ResolvedStyle) -> &mut HashMap<String, Vec<CachedGlyph>> {
        match (style.bold, style.italic) {
            (false, false) => &mut self.normal,
            (true, false) => &mut self.bold,
            (false, true) => &mut self.italic,
            (true, true) => &mut self.bold_italic,
        }
    }

    fn len(&self) -> usize {
        self.normal.len() + self.bold.len() + self.italic.len() + self.bold_italic.len()
    }

    fn clear(&mut self) {
        self.normal.clear();
        self.bold.clear();
        self.italic.clear();
        self.bold_italic.clear();
    }
}

#[derive(Resource)]
struct BatchMainState {
    output: Handle<Image>,
    ui_root: Option<Entity>,
    presentation: TerminalBatchPresentation,
    raster_scale: f32,
    raster_config: TerminalRenderConfig,
    last_snapshot: Option<TerminalSnapshot>,
    pending: Option<BatchScene>,
    shapes: ShapeCaches,
    glyph_atlas: UnifiedGlyphAtlas,
    scratch: SceneScratch,
    vertex_capacity: usize,
    blink: BlinkPhases,
}

impl BatchMainState {
    fn new(
        output: Handle<Image>,
        glyph_atlas: Handle<Image>,
        ui_root: Option<Entity>,
        presentation: TerminalBatchPresentation,
        raster_scale: f32,
        raster_config: TerminalRenderConfig,
    ) -> Self {
        Self {
            output,
            ui_root,
            presentation,
            raster_scale,
            raster_config,
            last_snapshot: None,
            pending: None,
            shapes: ShapeCaches::default(),
            glyph_atlas: UnifiedGlyphAtlas::new(glyph_atlas),
            scratch: SceneScratch::default(),
            vertex_capacity: 0,
            blink: BlinkPhases::default(),
        }
    }
}

#[derive(Clone, Copy)]
struct DrawBatch {
    texture: AssetId<Image>,
    start: u32,
    count: u32,
}

#[derive(Clone, Copy)]
struct QuadInstance {
    rect: Vec4,
    uv: Vec4,
    color: Vec4,
}

struct BatchScene {
    destination: AssetId<Image>,
    destination_size: UVec2,
    instances: Vec<QuadInstance>,
    batches: Vec<DrawBatch>,
    clear: bool,
    clear_color: Color,
    requires_prepared_assets: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct BlinkPhases {
    slow_hidden: bool,
    rapid_hidden: bool,
    cursor_hidden: bool,
}

impl BlinkPhases {
    fn at(elapsed: f32, config: &TerminalRenderConfig) -> Self {
        Self {
            slow_hidden: super::blink_hidden(elapsed, config.slow_blink_hz),
            rapid_hidden: super::blink_hidden(elapsed, config.rapid_blink_hz),
            cursor_hidden: config
                .cursor_blink_hz
                .is_some_and(|frequency| super::blink_hidden(elapsed, frequency)),
        }
    }

    fn hides(self, style: &ResolvedStyle) -> bool {
        (style.rapid_blink && self.rapid_hidden) || (style.slow_blink && self.slow_hidden)
    }
}

#[derive(Resource, Default)]
struct PendingBatchScene(Option<BatchScene>);

#[allow(clippy::too_many_arguments)]
fn sync_batch_terminal(
    surface: Res<TerminalSurface>,
    config: Res<TerminalRenderConfig>,
    mut state: ResMut<BatchMainState>,
    mut output: ResMut<TerminalBatchOutput>,
    mut stats: ResMut<TerminalBatchStats>,
    fonts: Res<Assets<Font>>,
    mut images: ResMut<Assets<Image>>,
    mut text_pipeline: ResMut<TextPipeline>,
    mut font_atlas_set: ResMut<FontAtlasSet>,
    mut font_cx: ResMut<FontCx>,
    mut layout_cx: ResMut<LayoutCx>,
    mut scale_cx: ResMut<ScaleCx>,
    mut ui_nodes: Query<(&mut Node, &mut ImageNode), With<TerminalBatchRoot>>,
    primary_window: Query<&Window, With<PrimaryWindow>>,
    ui_scale: Res<UiScale>,
    time: Option<Res<Time>>,
) {
    stats.sync_frames = stats.sync_frames.wrapping_add(1);
    stats.changed_rows = 0;
    stats.snapshot_cells = 0;
    stats.solid_quads = 0;
    stats.glyph_quads = 0;
    stats.draw_batches = 0;
    stats.extracted_bytes = 0;
    stats.shape_misses = 0;
    stats.snapshot_ns = 0;
    stats.scene_ns = 0;
    stats.gpu_buffer_reallocations = 0;
    stats.gpu_write_calls = 0;
    stats.gpu_bytes_written = 0;
    stats.render_passes = 0;
    stats.draw_calls = 0;
    stats.pipeline_switches = 0;
    stats.atlas_bindings = 0;

    let raster_scale = resolve_raster_scale(
        config.render_scale,
        state.presentation,
        primary_window
            .iter()
            .next()
            .map(|window| window.scale_factor() * ui_scale.0),
    );
    let scale_changed = state.raster_scale != raster_scale;
    let text_assets_changed = config.is_changed() || fonts.is_changed() || scale_changed;
    if config.is_changed() || scale_changed {
        state.raster_config = physical_config(&config, raster_scale);
        let logical_cell_size = state.raster_config.cell_size / raster_scale;
        surface.set_cell_size(logical_cell_size.x, logical_cell_size.y);
    }
    if text_assets_changed {
        state.shapes.clear();
        state.glyph_atlas.clear(&mut images);
    }
    let blink = BlinkPhases::at(
        time.as_ref().map_or(0.0, |time| time.elapsed_secs()),
        &config,
    );
    let blink_changed = blink != state.blink;
    if state.last_snapshot.as_ref().is_some_and(|snapshot| {
        snapshot.revision() == surface.revision() && !text_assets_changed && !blink_changed
    }) {
        stats.unchanged_frames = stats.unchanged_frames.wrapping_add(1);
        stats.cached_shapes = u32::try_from(state.shapes.len()).unwrap_or(u32::MAX);
        return;
    }

    let snapshot_start = Instant::now();
    let (snapshot, changed_rows, mut full) = if let Some(mut snapshot) = state.last_snapshot.take()
    {
        let old_cursor = snapshot.cursor_position();
        let update = surface.update_snapshot(&mut snapshot);
        stats.snapshot_cells = u32::try_from(update.changed_cells).unwrap_or(u32::MAX);
        let mut rows = update.changed_rows;
        if update.cursor_position_changed || update.cursor_visibility_changed {
            rows.push(old_cursor.y);
            rows.push(snapshot.cursor_position().y);
            rows.sort_unstable();
            rows.dedup();
            rows.retain(|row| *row < snapshot.size().height);
        }
        let full = update.resized || text_assets_changed;
        if blink_changed && !full {
            rows.extend(0..snapshot.size().height);
            rows.sort_unstable();
            rows.dedup();
        }
        (snapshot, rows, full)
    } else {
        let snapshot = surface.snapshot();
        stats.snapshot_cells = u32::try_from(snapshot.buffer().content.len()).unwrap_or(u32::MAX);
        let rows = (0..snapshot.size().height).collect();
        (snapshot, rows, true)
    };

    stats.snapshot_ns = snapshot_start
        .elapsed()
        .as_nanos()
        .min(u128::from(u64::MAX)) as u64;
    if changed_rows.is_empty() && !full && !blink_changed {
        state.last_snapshot = Some(snapshot);
        stats.unchanged_frames = stats.unchanged_frames.wrapping_add(1);
        return;
    }
    // Extraction can be delayed while a newly created output or glyph atlas reaches the render
    // world. If a newer payload is already waiting in the main world, make its replacement a
    // complete image of the newest snapshot so rows changed by an intermediate payload cannot be
    // lost when that payload is superseded.
    full |= state.pending.is_some();

    let new_size = terminal_pixel_size(&snapshot, &state.raster_config);
    let output_resized = output.size != new_size;
    if output_resized {
        // Use a new asset identity when the dimensions change. Render-asset and Bevy UI texture
        // caches can otherwise retain views or bind groups for the old allocation for a frame.
        let resized = images.add(make_target_image(new_size));
        state.output = resized.clone();
        output.image = resized;
        output.size = new_size;
    }
    output.logical_size = new_size.as_vec2() / raster_scale;
    output.raster_scale = raster_scale;
    if let Some(root) = state.ui_root
        && let Ok((mut node, mut image_node)) = ui_nodes.get_mut(root)
    {
        image_node.image = state.output.clone();
        *node = presentation_node(new_size, &config, raster_scale);
    }

    let rows: Vec<u16> = if full {
        (0..snapshot.size().height).collect()
    } else {
        changed_rows
    };
    let scene_start = Instant::now();
    let destination = state.output.id();
    let BatchMainState {
        raster_config,
        shapes,
        glyph_atlas,
        scratch,
        ..
    } = &mut *state;
    let mut scene = build_scene(
        &snapshot,
        raster_config,
        raster_scale,
        &rows,
        full,
        destination,
        &fonts,
        &mut images,
        &mut text_pipeline,
        &mut font_atlas_set,
        &mut font_cx,
        &mut layout_cx,
        &mut scale_cx,
        shapes,
        glyph_atlas,
        scratch,
        &mut stats,
        blink,
    );
    // Existing GPU textures are safe to consume before Bevy's asset preparation systems. A
    // resize or shape miss can create/modify an Image this frame, so those scenes use the later
    // submission point after RenderAsset preparation instead.
    scene.requires_prepared_assets = output_resized || stats.shape_misses != 0;
    stats.scene_ns = scene_start.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
    stats.changed_rows = u32::try_from(rows.len()).unwrap_or(u32::MAX);
    stats.draw_batches = u32::try_from(scene.batches.len()).unwrap_or(u32::MAX);
    stats.extracted_bytes = ((scene.instances.len() * std::mem::size_of::<QuadInstance>())
        + (scene.batches.len() * std::mem::size_of::<DrawBatch>()))
        as u64;
    let vertex_bytes = scene.instances.len() * 48;
    stats.gpu_bytes_written = vertex_bytes as u64;
    stats.gpu_write_calls = u32::from(vertex_bytes != 0);
    stats.render_passes = 1;
    stats.draw_calls = u32::try_from(scene.batches.len()).unwrap_or(u32::MAX);
    stats.pipeline_switches = u32::from(!scene.batches.is_empty());
    stats.atlas_bindings = stats.draw_calls;
    if vertex_bytes > state.vertex_capacity {
        state.vertex_capacity = vertex_bytes.next_power_of_two();
        stats.gpu_buffer_reallocations = 1;
    }
    stats.cached_shapes = u32::try_from(state.shapes.len()).unwrap_or(u32::MAX);
    state.pending = Some(scene);
    state.last_snapshot = Some(snapshot);
    state.blink = blink;
    state.raster_scale = raster_scale;
}

#[allow(clippy::too_many_arguments)]
fn build_scene(
    snapshot: &TerminalSnapshot,
    config: &TerminalRenderConfig,
    raster_scale: f32,
    rows: &[u16],
    full: bool,
    destination: AssetId<Image>,
    fonts: &Assets<Font>,
    images: &mut Assets<Image>,
    text_pipeline: &mut TextPipeline,
    font_atlas_set: &mut FontAtlasSet,
    font_cx: &mut FontCx,
    layout_cx: &mut LayoutCx,
    scale_cx: &mut ScaleCx,
    shapes: &mut ShapeCaches,
    glyph_atlas: &mut UnifiedGlyphAtlas,
    scratch: &mut SceneScratch,
    stats: &mut TerminalBatchStats,
    blink: BlinkPhases,
) -> BatchScene {
    let size = terminal_pixel_size(snapshot, config).as_vec2();
    scratch.clear();
    scratch.styles.reserve(usize::from(snapshot.size().width));
    let SceneScratch {
        backgrounds,
        foregrounds,
        glyphs,
        decorations,
        cursor,
        styles,
    } = scratch;

    for &row in rows {
        if !full {
            backgrounds.push(solid_quad(
                PixelGeometry {
                    x: 0.0,
                    y: f32::from(row) * config.cell_size.y,
                    width: size.x,
                    height: config.cell_size.y,
                },
                config.theme.background,
                size,
            ));
        }
        let cells = row_cells(snapshot, row);
        styles.clear();
        styles.extend(
            cells
                .iter()
                .map(|cell| ResolvedStyle::new(cell, &config.theme)),
        );
        let mut background_start = 0;
        while background_start < styles.len() {
            if cells[background_start].diff_option != CellDiffOption::Skip
                && procedural_cell_code(
                    cells[background_start].symbol(),
                    &styles[background_start],
                    blink,
                    raster_scale,
                )
                .is_some()
            {
                background_start += 1;
                continue;
            }
            let color = styles[background_start].background;
            let mut background_end = background_start + 1;
            while background_end < styles.len()
                && styles[background_end].background == color
                && (cells[background_end].diff_option == CellDiffOption::Skip
                    || procedural_cell_code(
                        cells[background_end].symbol(),
                        &styles[background_end],
                        blink,
                        raster_scale,
                    )
                    .is_none())
            {
                background_end += 1;
            }
            if full && color == config.theme.background {
                background_start = background_end;
                continue;
            }
            backgrounds.push(solid_quad(
                PixelGeometry {
                    x: background_start as f32 * config.cell_size.x,
                    y: f32::from(row) * config.cell_size.y,
                    width: (background_end - background_start) as f32 * config.cell_size.x,
                    height: config.cell_size.y,
                },
                color,
                size,
            ));
            background_start = background_end;
        }

        let mut column = 0;
        while column < cells.len() {
            let cell = &cells[column];
            if cell.diff_option == CellDiffOption::Skip {
                column += 1;
                continue;
            }
            let width = usize::from(cell.cell_width().max(1)).min(cells.len() - column);
            let style = &styles[column];
            let symbol = cell.symbol();
            if style.hidden || blink.hides(style) {
                column += width;
                continue;
            }
            let mut exact = Vec::new();
            let exact_run = || TextRun {
                start: column as u16,
                width: width as u16,
                text: symbol.to_owned(),
                style: style.clone(),
            };
            let procedural = procedural_cell_code(symbol, style, blink, raster_scale);
            if let Some(code) = procedural {
                foregrounds.push(procedural_cell_quad(
                    PixelGeometry {
                        x: column as f32 * config.cell_size.x,
                        y: f32::from(row) * config.cell_size.y,
                        width: config.cell_size.x,
                        height: config.cell_size.y,
                    },
                    style.foreground,
                    style.background,
                    code,
                    size,
                ));
            } else if let Some(geometry) = block_geometry(symbol) {
                push_block(&mut exact, &exact_run(), row, config, geometry, 1);
            } else if let Some(mask) = quadrant_mask(symbol) {
                push_quadrants(&mut exact, &exact_run(), row, config, mask, 1);
            } else if let Some(glyph) = line_glyph(symbol) {
                push_line_glyph(
                    &mut exact,
                    &exact_run(),
                    row,
                    config,
                    glyph,
                    1,
                    raster_scale.round().max(1.0),
                );
            } else if symbol != " " && !symbol.is_empty() {
                let shaped = cached_shape(
                    symbol,
                    style,
                    config,
                    size,
                    fonts,
                    images,
                    text_pipeline,
                    font_atlas_set,
                    font_cx,
                    layout_cx,
                    scale_cx,
                    shapes,
                    glyph_atlas,
                    stats,
                );
                let anchor = Vec2::new(
                    column as f32 * config.cell_size.x,
                    f32::from(row) * config.cell_size.y,
                );
                for glyph in shaped {
                    let geometry = PixelGeometry {
                        x: anchor.x + glyph.offset.x,
                        y: anchor.y + glyph.offset.y,
                        width: glyph.size.x,
                        height: glyph.size.y,
                    };
                    glyphs.push((
                        glyph.texture,
                        glyph_quad(geometry, glyph.uv, style.foreground, glyph.alpha_mask, size),
                    ));
                }
            }
            foregrounds.extend(
                exact
                    .into_iter()
                    .map(|solid| solid_quad(solid.geometry, solid.color, size)),
            );

            let decoration_x = column as f32 * config.cell_size.x;
            let decoration_width = width as f32 * config.cell_size.x;
            let decoration_thickness = raster_scale.round().max(1.0);
            if procedural.is_none() && style.underlined {
                decorations.push(solid_quad(
                    PixelGeometry {
                        x: decoration_x,
                        y: f32::from(row) * config.cell_size.y
                            + (config.cell_size.y - 2.0 * decoration_thickness).max(0.0),
                        width: decoration_width,
                        height: decoration_thickness,
                    },
                    style.underline,
                    size,
                ));
            }
            if procedural.is_none() && style.crossed_out {
                decorations.push(solid_quad(
                    PixelGeometry {
                        x: decoration_x,
                        y: f32::from(row) * config.cell_size.y + config.cell_size.y * 0.55,
                        width: decoration_width,
                        height: decoration_thickness,
                    },
                    style.foreground,
                    size,
                ));
            }
            column += width;
        }
    }

    if cursor_should_be_visible(snapshot)
        && !blink.cursor_hidden
        && (full || rows.contains(&snapshot.cursor_position().y))
    {
        let position = snapshot.cursor_position();
        let cursor_thickness = raster_scale.round().max(1.0) * 2.0;
        let (x, y, width, height) = match config.cursor_style {
            super::CursorStyle::Block => (0.0, 0.0, config.cell_size.x, config.cell_size.y),
            super::CursorStyle::Bar => (
                0.0,
                0.0,
                cursor_thickness.min(config.cell_size.x),
                config.cell_size.y,
            ),
            super::CursorStyle::Underline => (
                0.0,
                (config.cell_size.y - cursor_thickness).max(0.0),
                config.cell_size.x,
                cursor_thickness.min(config.cell_size.y),
            ),
        };
        cursor.push(solid_quad(
            PixelGeometry {
                x: f32::from(position.x) * config.cell_size.x + x,
                y: f32::from(position.y) * config.cell_size.y + y,
                width,
                height,
            },
            config.theme.cursor,
            size,
        ));
    }

    stats.solid_quads =
        u32::try_from(backgrounds.len() + foregrounds.len() + decorations.len() + cursor.len())
            .unwrap_or(u32::MAX);
    stats.glyph_quads = u32::try_from(glyphs.len()).unwrap_or(u32::MAX);

    let mut instances = Vec::with_capacity(stats.solid_quads as usize + stats.glyph_quads as usize);
    let mut batches = Vec::new();
    let primary_atlas = glyph_atlas.image.id();
    append_batch(&mut instances, &mut batches, primary_atlas, backgrounds);
    append_batch(&mut instances, &mut batches, primary_atlas, foregrounds);
    append_glyph_batches(&mut instances, &mut batches, glyphs);
    append_batch(&mut instances, &mut batches, primary_atlas, decorations);
    append_batch(&mut instances, &mut batches, primary_atlas, cursor);
    BatchScene {
        destination,
        destination_size: size.as_uvec2(),
        instances,
        batches,
        clear: full,
        clear_color: config.theme.background,
        requires_prepared_assets: false,
    }
}

#[allow(clippy::too_many_arguments)]
fn cached_shape<'a>(
    text: &str,
    style: &ResolvedStyle,
    config: &TerminalRenderConfig,
    viewport: Vec2,
    fonts: &Assets<Font>,
    images: &mut Assets<Image>,
    text_pipeline: &mut TextPipeline,
    font_atlas_set: &mut FontAtlasSet,
    font_cx: &mut FontCx,
    layout_cx: &mut LayoutCx,
    scale_cx: &mut ScaleCx,
    shapes: &'a mut ShapeCaches,
    glyph_atlas: &mut UnifiedGlyphAtlas,
    stats: &mut TerminalBatchStats,
) -> &'a [CachedGlyph] {
    if shapes.select(style).contains_key(text) {
        return shapes
            .select(style)
            .get(text)
            .expect("shape cache key was just found");
    }
    stats.shape_misses = stats.shape_misses.saturating_add(1);
    let font = text_font(config, style);
    let mut computed = ComputedTextBlock::default();
    let mut layout = TextLayoutInfo::default();
    let shape_result = text_pipeline.update_buffer(
        fonts,
        std::iter::once((
            Entity::PLACEHOLDER,
            0,
            text,
            &font,
            Color::WHITE,
            LineHeight::Px(config.cell_size.y),
            LetterSpacing::default(),
        )),
        LineBreak::NoWrap,
        Justify::Left,
        TextBounds::UNBOUNDED,
        1.0,
        &mut computed,
        font_cx,
        layout_cx,
        viewport,
        20.0,
    );
    if shape_result.is_ok() {
        let _ = text_pipeline.update_text_layout_info(
            &mut layout,
            font_atlas_set,
            images,
            &mut computed,
            scale_cx,
            TextBounds::UNBOUNDED,
            Justify::Left,
            FontHinting::Enabled,
        );
    }
    let cached = layout
        .glyphs
        .into_iter()
        .filter_map(|glyph| {
            let atlas = images.get(glyph.atlas_info.texture)?;
            let atlas_size = atlas.texture_descriptor.size;
            let rect = glyph.atlas_info.rect;
            let size = rect.size();
            let source = SourceGlyph {
                texture: glyph.atlas_info.texture,
                x: rect.min.x as u32,
                y: rect.min.y as u32,
                width: size.x as u32,
                height: size.y as u32,
            };
            let source_uv = Vec4::new(
                rect.min.x / atlas_size.width as f32,
                rect.min.y / atlas_size.height as f32,
                rect.max.x / atlas_size.width as f32,
                rect.max.y / atlas_size.height as f32,
            );
            let (texture, uv) = glyph_atlas
                .cache(source, images)
                .map_or((source.texture, source_uv), |uv| {
                    (glyph_atlas.image.id(), uv)
                });
            Some(CachedGlyph {
                texture,
                // Atlas texels must land on physical pixel boundaries. Bevy's layout positions
                // can retain fractional shaping offsets even though the glyph bitmap is an
                // integer-sized raster image.
                offset: (glyph.position - size * 0.5).round(),
                size,
                uv,
                alpha_mask: glyph.atlas_info.is_alpha_mask,
            })
        })
        .collect::<Vec<_>>();
    shapes.select_mut(style).insert(text.to_owned(), cached);
    shapes
        .select(style)
        .get(text)
        .expect("newly shaped text was inserted")
}

fn solid_quad(geometry: PixelGeometry, color: Color, target: Vec2) -> QuadInstance {
    QuadInstance {
        rect: clip_rect(snap_geometry(geometry), target),
        // A negative final UV component lets the unified fragment shader skip the atlas sample.
        uv: Vec4::new(0.0, 0.0, 0.0, -1.0),
        color: color.to_linear().to_f32_array().into(),
    }
}

fn procedural_cell_code(
    symbol: &str,
    style: &ResolvedStyle,
    blink: BlinkPhases,
    raster_scale: f32,
) -> Option<u32> {
    if style.hidden
        || blink.hides(style)
        || (style.underlined && style.underline != style.foreground)
    {
        return None;
    }
    let pattern = match symbol {
        "█" => 0,
        "▓" => 1,
        "▒" => 2,
        "░" => 3,
        "▀" => 4,
        "▄" => 5,
        "▌" => 6,
        "▐" => 7,
        _ => return None,
    };
    let underline = u32::from(style.underlined) << 3;
    let crossed = u32::from(style.crossed_out) << 4;
    let pixel_scale = raster_scale.round().clamp(1.0, 15.0) as u32;
    Some(pattern | underline | crossed | (pixel_scale << 5))
}

fn procedural_cell_quad(
    geometry: PixelGeometry,
    foreground: Color,
    background: Color,
    code: u32,
    target: Vec2,
) -> QuadInstance {
    let foreground = foreground.to_linear().to_f32_array();
    let mut background = background.to_linear().to_f32_array();
    background[3] = -(10.0 + code as f32);
    QuadInstance {
        rect: clip_rect(snap_geometry(geometry), target),
        uv: foreground.into(),
        color: background.into(),
    }
}

fn glyph_quad(
    geometry: PixelGeometry,
    uv: Vec4,
    color: Color,
    alpha_mask: bool,
    target: Vec2,
) -> QuadInstance {
    let mut color = color.to_linear().to_f32_array();
    if !alpha_mask {
        color[3] = -1.0;
    }
    QuadInstance {
        rect: clip_rect(snap_geometry(geometry), target),
        uv,
        color: color.into(),
    }
}

fn snap_geometry(geometry: PixelGeometry) -> PixelGeometry {
    let left = geometry.x.round();
    let top = geometry.y.round();
    let right = (geometry.x + geometry.width).round().max(left);
    let bottom = (geometry.y + geometry.height).round().max(top);
    PixelGeometry {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    }
}

fn clip_rect(geometry: PixelGeometry, target: Vec2) -> Vec4 {
    let left = geometry.x / target.x * 2.0 - 1.0;
    let right = (geometry.x + geometry.width) / target.x * 2.0 - 1.0;
    let top = 1.0 - geometry.y / target.y * 2.0;
    let bottom = 1.0 - (geometry.y + geometry.height) / target.y * 2.0;
    Vec4::new(left, top, right, bottom)
}

fn append_batch(
    instances: &mut Vec<QuadInstance>,
    batches: &mut Vec<DrawBatch>,
    texture: AssetId<Image>,
    quads: &[QuadInstance],
) {
    if quads.is_empty() {
        return;
    }
    let start = instances.len() as u32;
    let count = quads.len() as u32;
    instances.extend_from_slice(quads);
    if let Some(previous) = batches.last_mut()
        && previous.texture == texture
        && previous.start + previous.count == start
    {
        previous.count += count;
    } else {
        batches.push(DrawBatch {
            texture,
            start,
            count,
        });
    }
}

fn append_glyph_batches(
    instances: &mut Vec<QuadInstance>,
    batches: &mut Vec<DrawBatch>,
    glyphs: &[(AssetId<Image>, QuadInstance)],
) {
    // The renderer-owned atlas makes this one contiguous run in normal operation. Preserve
    // source order if an unusually large glyph set falls back to Bevy's source atlases; adjacent
    // runs still coalesce without changing paint order.
    for &(texture, glyph) in glyphs {
        append_batch(instances, batches, texture, std::slice::from_ref(&glyph));
    }
}

fn extract_batch_scene(mut main_world: ResMut<MainWorld>, mut pending: ResMut<PendingBatchScene>) {
    if pending.0.is_some() {
        return;
    }
    pending.0 = main_world.resource_mut::<BatchMainState>().pending.take();
}

fn batch_scene_can_render_early(pending: Res<PendingBatchScene>) -> bool {
    pending
        .0
        .as_ref()
        .is_some_and(|scene| !scene.requires_prepared_assets)
}

#[derive(Default, Resource)]
struct BatchGpuState {
    vertex_buffer: Option<Buffer>,
    vertex_capacity: u64,
    texture_layout: Option<BindGroupLayout>,
    pipeline: Option<RenderPipeline>,
    texture_bind_groups: HashMap<AssetId<Image>, (TextureId, BindGroup)>,
}

impl BatchGpuState {
    fn ensure_pipeline(&mut self, device: &RenderDevice) {
        if self.pipeline.is_some() {
            return;
        }
        let texture_layout = device.create_bind_group_layout(
            "bevy_grid batch texture layout",
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
        self.texture_layout = Some(texture_layout);
        self.pipeline = Some(pipeline);
    }
}

fn reset_batch_gpu_state(mut gpu: ResMut<BatchGpuState>) {
    *gpu = BatchGpuState::default();
}

fn create_pipeline(
    device: &RenderDevice,
    layouts: &[&BindGroupLayout],
    fragment_entry: &'static str,
    blend: BlendState,
) -> RenderPipeline {
    let shader = device.create_and_validate_shader_module(ShaderModuleDescriptor {
        label: Some("bevy_grid batch shader"),
        source: ShaderSource::Wgsl(BATCH_SHADER.into()),
    });
    let raw_layouts = layouts
        .iter()
        .map(|layout| Some(&***layout))
        .collect::<Vec<_>>();
    let layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
        label: Some("bevy_grid batch pipeline layout"),
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
        label: Some("bevy_grid batch pipeline"),
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

fn instance_bytes(instances: &[QuadInstance]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(instances.len() * 48);
    for instance in instances {
        for value in instance
            .rect
            .to_array()
            .into_iter()
            .chain(instance.uv.to_array())
            .chain(instance.color.to_array())
        {
            bytes.extend_from_slice(&value.to_ne_bytes());
        }
    }
    bytes
}

fn render_batch_scene(
    mut pending: ResMut<PendingBatchScene>,
    mut gpu: ResMut<BatchGpuState>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    let Some(scene) = pending.0.take() else {
        return;
    };
    let Some(target) = gpu_images.get(scene.destination) else {
        pending.0 = Some(scene);
        return;
    };
    let target_size = target.texture_descriptor.size;
    if target_size.width != scene.destination_size.x
        || target_size.height != scene.destination_size.y
    {
        // An Image asset replacement can coexist with its previous render asset for a frame.
        // Keep the complete replacement scene pending until the matching GPU texture is ready.
        pending.0 = Some(scene);
        return;
    }
    if scene
        .batches
        .iter()
        .any(|batch| gpu_images.get(batch.texture).is_none())
    {
        pending.0 = Some(scene);
        return;
    }

    gpu.ensure_pipeline(&device);
    if !scene.instances.is_empty() {
        let bytes = instance_bytes(&scene.instances);
        let required = bytes.len() as u64;
        if required > gpu.vertex_capacity {
            gpu.vertex_capacity = required.next_power_of_two();
            gpu.vertex_buffer = Some(device.create_buffer(&BufferDescriptor {
                label: Some("bevy_grid terminal instances"),
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
            &bytes,
        );
    }

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
                "bevy_grid glyph atlas",
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

    let load = if scene.clear {
        LoadOp::Clear(scene.clear_color.to_linear().into())
    } else {
        LoadOp::Load
    };
    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
        label: Some("bevy_grid terminal batch"),
    });
    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some("bevy_grid terminal batch"),
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
    if let Some(vertex_buffer) = &gpu.vertex_buffer {
        pass.set_vertex_buffer(0, *vertex_buffer.slice(..));
        pass.set_pipeline(gpu.pipeline.as_ref().expect("pipeline was initialized"));
        for batch in &scene.batches {
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
    queue.submit([encoder.finish()]);
}

const BATCH_SHADER: &str = r#"
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
    @location(2) @interpolate(flat) mode: i32,
    @location(3) local: vec2<f32>,
    @location(4) data: vec4<f32>,
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
    output.mode = select(0i, -1i, input.uv.w < 0.0);
    if input.color.a <= -10.0 {
        output.mode = i32(round(-input.color.a - 10.0)) + 1i;
    }
    output.local = corner;
    output.data = input.uv;
    return output;
}

@fragment
fn fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    if input.mode < 0i {
        return input.color;
    }
    if input.mode > 0i {
        let code = u32(input.mode - 1i);
        let pattern = code & 7u;
        let pixel_scale = max(code >> 5u, 1u);
        let pixel = vec2<u32>(floor(input.position.xy)) / pixel_scale;
        var foreground = false;
        switch pattern {
            case 0u: { foreground = true; }
            case 1u: { foreground = !((pixel.x & 1u) == 0u && (pixel.y & 1u) == 0u); }
            case 2u: { foreground = ((pixel.x + pixel.y) & 1u) == 0u; }
            case 3u: { foreground = (pixel.x & 1u) == 0u && (pixel.y & 1u) == 0u; }
            case 4u: { foreground = input.local.y < 0.5; }
            case 5u: { foreground = input.local.y >= 0.5; }
            case 6u: { foreground = input.local.x < 0.5; }
            default: { foreground = input.local.x >= 0.5; }
        }
        let pixel_y = abs(dpdy(input.local.y)) * f32(pixel_scale);
        if (code & 8u) != 0u
            && input.local.y >= 1.0 - 2.0 * pixel_y
            && input.local.y < 1.0 - pixel_y {
            foreground = true;
        }
        if (code & 16u) != 0u
            && input.local.y >= 0.55
            && input.local.y < 0.55 + pixel_y {
            foreground = true;
        }
        return select(vec4<f32>(input.color.rgb, 1.0), input.data, foreground);
    }
    let sample = textureSample(glyph_atlas, glyph_sampler, input.uv);
    if input.color.a >= 0.0 {
        return vec4<f32>(input.color.rgb, input.color.a * sample.a);
    }
    return sample;
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn quad(value: f32) -> QuadInstance {
        QuadInstance {
            rect: Vec4::splat(value),
            uv: Vec4::ZERO,
            color: Vec4::ONE,
        }
    }

    #[test]
    fn pixel_rectangles_map_exactly_to_clip_space() {
        assert_eq!(
            clip_rect(
                PixelGeometry {
                    x: 0.0,
                    y: 0.0,
                    width: 800.0,
                    height: 480.0,
                },
                Vec2::new(800.0, 480.0),
            ),
            Vec4::new(-1.0, 1.0, 1.0, -1.0)
        );
        let cell = clip_rect(
            PixelGeometry {
                x: 400.0,
                y: 240.0,
                width: 10.0,
                height: 20.0,
            },
            Vec2::new(800.0, 480.0),
        );
        assert!(cell.abs_diff_eq(Vec4::new(0.0, 0.0, 0.025, -1.0 / 12.0), 1e-6));
    }

    #[test]
    fn terminal_target_uses_nearest_sampling() {
        let image = make_target_image(UVec2::new(80, 40));
        assert_eq!(image.sampler, ImageSampler::nearest());
    }

    #[test]
    fn automatic_scale_tracks_ui_windows_but_not_headless_rendering() {
        assert_eq!(
            resolve_raster_scale(
                TerminalRenderScale::Automatic,
                TerminalBatchPresentation::Ui,
                Some(2.0),
            ),
            2.0
        );
        assert_eq!(
            resolve_raster_scale(
                TerminalRenderScale::Automatic,
                TerminalBatchPresentation::Headless,
                Some(2.0),
            ),
            1.0
        );
        assert_eq!(
            resolve_raster_scale(
                TerminalRenderScale::Fixed(1.5),
                TerminalBatchPresentation::Headless,
                None,
            ),
            1.5
        );
        assert_eq!(
            resolve_raster_scale(
                TerminalRenderScale::Fixed(f32::NAN),
                TerminalBatchPresentation::Ui,
                None,
            ),
            1.0
        );
    }

    #[test]
    fn physical_metrics_and_geometry_are_pixel_aligned() {
        let config = TerminalRenderConfig {
            cell_size: Vec2::new(10.8, 19.6),
            font_size: 17.6,
            ..default()
        };
        let physical = physical_config(&config, 2.0);
        assert_eq!(physical.cell_size, Vec2::new(22.0, 39.0));
        assert_eq!(physical.font_size, 35.0);

        assert_eq!(
            snap_geometry(PixelGeometry {
                x: 4.5,
                y: 9.5,
                width: 1.0,
                height: 2.0,
            }),
            PixelGeometry {
                x: 5.0,
                y: 10.0,
                width: 1.0,
                height: 2.0,
            }
        );
    }

    #[test]
    fn glyph_batches_preserve_paint_order_and_coalesce_adjacent_atlases() {
        let mut images = Assets::<Image>::default();
        let atlas_a = images.add(Image::default()).id();
        let atlas_b = images.add(Image::default()).id();
        let glyphs = vec![
            (atlas_a, quad(1.0)),
            (atlas_a, quad(2.0)),
            (atlas_b, quad(3.0)),
            (atlas_a, quad(4.0)),
        ];
        let mut instances = Vec::new();
        let mut batches = Vec::new();
        append_glyph_batches(&mut instances, &mut batches, &glyphs);

        assert_eq!(instances.len(), 4);
        assert_eq!(batches.len(), 3);
        assert_eq!(batches[0].texture, atlas_a);
        assert_eq!((batches[0].start, batches[0].count), (0, 2));
        assert_eq!(batches[1].texture, atlas_b);
        assert_eq!((batches[1].start, batches[1].count), (2, 1));
        assert_eq!(batches[2].texture, atlas_a);
        assert_eq!((batches[2].start, batches[2].count), (3, 1));
        assert_eq!(instances[0].rect, Vec4::splat(1.0));
        assert_eq!(instances[1].rect, Vec4::splat(2.0));
        assert_eq!(instances[2].rect, Vec4::splat(3.0));
        assert_eq!(instances[3].rect, Vec4::splat(4.0));
    }

    #[test]
    fn replacement_batches_never_address_stale_capacity() {
        let mut images = Assets::<Image>::default();
        let atlas = images.add(Image::default()).id();
        let mut instances = Vec::with_capacity(32);
        let mut batches = Vec::new();
        let first = vec![quad(1.0); 12];
        append_batch(&mut instances, &mut batches, atlas, &first);
        assert_eq!(batches[0].count, 12);

        instances.clear();
        batches.clear();
        let second = vec![quad(2.0); 2];
        append_batch(&mut instances, &mut batches, atlas, &second);
        assert_eq!(instances.len(), 2);
        assert_eq!((batches[0].start, batches[0].count), (0, 2));
    }

    #[test]
    fn empty_scene_produces_no_upload_or_draw_batch() {
        let mut images = Assets::<Image>::default();
        let atlas = images.add(Image::default()).id();
        let mut instances = Vec::new();
        let mut batches = Vec::new();
        append_batch(&mut instances, &mut batches, atlas, &[]);
        append_glyph_batches(&mut instances, &mut batches, &[]);
        assert!(instances.is_empty());
        assert!(batches.is_empty());
        assert!(instance_bytes(&instances).is_empty());
    }

    #[test]
    fn hidden_blocks_fall_back_to_background_rendering() {
        let mut cell = ratatui::buffer::Cell::new("█");
        cell.modifier.insert(ratatui::style::Modifier::HIDDEN);
        let style = ResolvedStyle::new(&cell, &super::super::TerminalTheme::default());
        assert_eq!(
            procedural_cell_code("█", &style, BlinkPhases::default(), 1.0),
            None
        );
    }

    #[test]
    fn unified_atlas_copies_each_bevy_glyph_once_and_reuses_its_uv() {
        let mut images = Assets::<Image>::default();
        let mut source_pixels = vec![0; 4 * 4 * 4];
        let source_offset = (4 + 1) * 4;
        source_pixels[source_offset..source_offset + 4].copy_from_slice(&[11, 22, 33, 44]);
        let source = images.add(Image::new(
            Extent3d {
                width: 4,
                height: 4,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            source_pixels,
            GLYPH_FORMAT,
            RenderAssetUsages::MAIN_WORLD,
        ));
        let target = images.add(make_glyph_atlas_image());
        let mut atlas = UnifiedGlyphAtlas::new(target.clone());
        let glyph = SourceGlyph {
            texture: source.id(),
            x: 1,
            y: 1,
            width: 1,
            height: 1,
        };

        let first = atlas.cache(glyph, &mut images).expect("glyph should fit");
        let cursor = atlas.cursor;
        let second = atlas
            .cache(glyph, &mut images)
            .expect("glyph should be cached");
        assert_eq!(first, second);
        assert_eq!(atlas.cursor, cursor);
        assert_eq!(atlas.glyphs.len(), 1);

        let target_offset = (GLYPH_ATLAS_SIZE as usize + 1) * 4;
        {
            let target = images.get(&target).expect("target atlas exists");
            assert_eq!(
                &target.data.as_ref().expect("atlas has CPU data")
                    [target_offset..target_offset + 4],
                &[11, 22, 33, 44]
            );
        }

        atlas.clear(&mut images);
        assert!(atlas.glyphs.is_empty());
        assert_eq!(atlas.cursor, UVec2::splat(1));
        assert_eq!(atlas.row_height, 0);
        let target = images.get(&target).expect("target atlas exists");
        assert_eq!(
            &target.data.as_ref().expect("atlas has CPU data")[target_offset..target_offset + 4],
            &[0, 0, 0, 0]
        );
    }

    #[test]
    fn blink_phases_follow_slow_rapid_and_disabled_cursor_rates() {
        let mut config = TerminalRenderConfig {
            slow_blink_hz: 1.0,
            rapid_blink_hz: 2.0,
            cursor_blink_hz: None,
            ..default()
        };
        let visible = BlinkPhases::at(0.1, &config);
        assert!(!visible.slow_hidden && !visible.rapid_hidden && !visible.cursor_hidden);
        let hidden = BlinkPhases::at(0.3, &config);
        assert!(!hidden.slow_hidden && hidden.rapid_hidden && !hidden.cursor_hidden);

        config.cursor_blink_hz = Some(1.0);
        assert!(BlinkPhases::at(0.6, &config).cursor_hidden);
    }
}
