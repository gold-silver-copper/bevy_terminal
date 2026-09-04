//! Compact render-world terminal batch.

use std::time::Instant;

use bevy::{
    asset::{AssetId, RenderAssetUsages},
    image::ImageSampler,
    platform::collections::HashMap,
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
        ComputedTextBlock, FontAtlasSet, FontCx, LayoutCx, LetterSpacing, LineBreak, LineHeight,
        ScaleCx, TextBounds, TextLayoutInfo, TextPipeline,
    },
    window::PrimaryWindow,
};

use super::{
    PixelGeometry, ResolvedStyle, TerminalRenderConfig, TerminalRenderScale, cell_span,
    cursor_should_be_visible, text_font,
};
use crate::{
    scene::{GridSize, StyleFlags, TerminalSnapshot},
    surface::TerminalSurface,
};

/// The terminal texture format: the shader emits linear colors and the sRGB
/// target encodes them, so the image is display-ready for UI, sprites and 3D
/// materials and dark tones do not band in 8-bit storage.
const TARGET_FORMAT: TextureFormat = TextureFormat::Rgba8UnormSrgb;
const GLYPH_FORMAT: TextureFormat = TextureFormat::Rgba8UnormSrgb;
const GLYPH_ATLAS_SIZE: u32 = 2048;

/// One independently rendered terminal: the surface it renders.
///
/// Spawn this component on an entity after adding [`TerminalPlugin`] once. A
/// default [`TerminalRenderConfig`] is required and inserted automatically;
/// insert your own to configure rendering, and mutate it later to rebuild
/// only this terminal. The plugin attaches [`TerminalTexture`] and
/// [`TerminalStats`]. Add an [`ImageNode`] (and [`Node`]) to the same entity
/// to present the texture through Bevy UI — the plugin keeps the node's image
/// and size in sync while your layout decides where it goes; without an
/// `ImageNode` the terminal is headless and only the texture is produced.
/// Terminals may be spawned and despawned at any time; the images are released
/// with the entity.
#[derive(Clone, Component)]
#[require(TerminalRenderConfig)]
pub struct Terminal {
    surface: TerminalSurface,
}

impl Terminal {
    /// Creates a terminal rendering `surface`.
    #[must_use]
    pub const fn new(surface: TerminalSurface) -> Self {
        Self { surface }
    }

    /// Returns the surface this terminal renders.
    #[must_use]
    pub const fn surface(&self) -> &TerminalSurface {
        &self.surface
    }
}

impl From<TerminalSurface> for Terminal {
    fn from(surface: TerminalSurface) -> Self {
        Self::new(surface)
    }
}

/// The renderer-owned terminal texture and its current dimensions.
///
/// The image is `Rgba8UnormSrgb` (display-ready, straight alpha) and its
/// handle is stable for the terminal's lifetime.
///
/// Attached to every [`Terminal`] entity by [`TerminalPlugin`] on the first
/// update after it is spawned. [`TerminalReady`] is triggered once the selected
/// fonts and any font-driven cell metrics have been measured. The image handle
/// stays the same for the lifetime of the terminal: resizes reallocate the
/// image in place.
#[derive(Clone, Debug, Component)]
pub struct TerminalTexture {
    /// Render-world image containing the completed terminal.
    pub image: Handle<Image>,
    /// Physical pixel dimensions of `image`.
    pub size: UVec2,
    /// Logical dimensions used for a Bevy UI presentation node.
    pub logical_size: Vec2,
    /// Physical pixels per logical pixel used to rasterize `image`.
    pub raster_scale: f32,
    /// Effective logical size of one cell: the physical cell (whole pixels,
    /// possibly grown to the font's line box — see [`super::FontSizing`])
    /// divided by `raster_scale`.
    pub cell_size: Vec2,
    /// Effective logical font size. This can differ slightly from a requested
    /// [`super::FontSizing::Px`] size when [`super::CellSizing::FromFont`]
    /// snaps the measured advance to a whole physical-pixel cell.
    pub font_size: f32,
}

impl TerminalTexture {
    /// Returns the grid that fits into `logical_size` (floor, at least 1×1)
    /// at this terminal's current cell size.
    #[must_use]
    pub fn grid_for(&self, logical_size: Vec2) -> GridSize {
        grid_for(logical_size, self.cell_size)
    }
}

/// Returns the grid that fits `logical_size` with cells of `cell_size`
/// (floor, clamped to at least 1×1 and at most `u16::MAX`).
#[must_use]
pub fn grid_for(logical_size: Vec2, cell_size: Vec2) -> GridSize {
    let cell = cell_size.max(Vec2::ONE);
    let columns = (logical_size.x / cell.x)
        .floor()
        .clamp(1.0, f32::from(u16::MAX)) as u16;
    let rows = (logical_size.y / cell.y)
        .floor()
        .clamp(1.0, f32::from(u16::MAX)) as u16;
    GridSize::new(columns, rows)
}

/// Returns the grid that fills `window` with cells of `cell_size` (logical pixels).
#[must_use]
pub fn grid_for_window(window: &Window, cell_size: Vec2) -> GridSize {
    grid_for(window.resolution.size(), cell_size)
}

/// Returns the physical-to-logical ratio of `window`'s actual framebuffer
/// (at least 1.0), suitable for [`TerminalRenderScale::Fixed`] so the
/// texture maps one-to-one onto physical pixels even when the reported scale
/// factor and the real ratio disagree (mixed-DPI setups).
#[must_use]
pub fn raster_scale_for_window(window: &Window) -> f32 {
    let logical = window.resolution.size().max(Vec2::ONE);
    let physical = window.resolution.physical_size().as_vec2();
    (physical.x / logical.x)
        .min(physical.y / logical.y)
        .max(1.0)
}

/// Triggered once on a [`Terminal`] entity when its [`TerminalTexture`] has
/// been allocated at its measured size and can be presented or exported.
///
/// This happens on the first sync after every configured font asset that is
/// loaded has been registered with Bevy's font system and any advance required
/// by the sizing mode has been measured. A late font never exposes provisional
/// cell geometry; after readiness, remeasurement may resize the texture in
/// place without changing its handle.
#[derive(Clone, Debug, EntityEvent)]
pub struct TerminalReady {
    /// The terminal entity; read its [`TerminalTexture`] for the handle and size.
    pub entity: Entity,
}

/// Triggered on a [`Terminal`] entity whenever its [`TerminalTexture`] changes
/// physical size *after* [`TerminalReady`] has fired: a surface resize, a
/// configuration change, a raster-scale change, or a font that arrived late and
/// re-measured the cell. The image handle is unchanged; only its dimensions
/// (and possibly `cell_size`) differ. Custom presentation code (a world-space
/// quad, an export pipeline) should rebuild anything derived from the size.
///
/// Sizes reported here are physical pixels; [`TerminalTexture`] on the entity
/// already holds the new values when the event is delivered.
#[derive(Clone, Debug, EntityEvent)]
pub struct TerminalRemeasured {
    /// The terminal entity.
    pub entity: Entity,
    /// Texture size before the re-measure.
    pub previous_size: UVec2,
    /// Texture size after the re-measure.
    pub size: UVec2,
    /// Logical cell size after the re-measure.
    pub cell_size: Vec2,
}

/// Counters for the most recent scene update of one [`Terminal`]; all zero on
/// frames that produced no terminal work.
#[derive(Clone, Copy, Debug, Default, Component)]
#[non_exhaustive]
pub struct TerminalStats {
    /// Rows rebuilt into the latest payload.
    pub changed_rows: u32,
    /// Cells copied while updating the retained snapshot.
    pub snapshot_cells: u32,
    /// Solid rectangles (backgrounds, decorations, cursor) in the latest payload.
    pub solid_quads: u32,
    /// Glyph rectangles in the latest payload.
    pub glyph_quads: u32,
    /// Draw batches (one per glyph-atlas switch) in the latest payload.
    pub draw_batches: u32,
    /// Shape-cache misses in the latest update.
    pub shape_misses: u32,
    /// Nanoseconds spent updating the retained terminal snapshot.
    pub snapshot_ns: u64,
    /// Nanoseconds spent generating the compact CPU scene.
    pub scene_ns: u64,
}

impl std::fmt::Display for TerminalStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "rows {} · cells {} · quads {} solid + {} glyph in {} batches · {} shape misses · snapshot {} µs · scene {} µs",
            self.changed_rows,
            self.snapshot_cells,
            self.solid_quads,
            self.glyph_quads,
            self.draw_batches,
            self.shape_misses,
            self.snapshot_ns / 1000,
            self.scene_ns / 1000,
        )
    }
}

/// Installs the terminal renderer. Add it once, then spawn [`Terminal`]
/// entities.
///
/// Glyphs are shaped and rasterized by Bevy text. Each terminal is represented
/// by compact GPU quad instances drawn into its own renderer-owned texture;
/// GPU pipelines and scratch buffers are shared between terminals.
#[derive(Clone, Copy, Debug, Default)]
pub struct TerminalPlugin;

impl Plugin for TerminalPlugin {
    fn build(&self, app: &mut App) {
        #[cfg(feature = "3d")]
        app.add_plugins(super::world_quad::plugin);
        app.add_systems(
            Update,
            (
                initialize_terminals.before(super::TerminalSystems::Sync),
                sync_batch_terminals.in_set(super::TerminalSystems::Sync),
            ),
        );
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app
                .init_resource::<PendingBatchScenes>()
                .init_resource::<BatchGpuState>()
                .add_systems(RenderStartup, reset_batch_gpu_state)
                .add_systems(ExtractSchedule, extract_batch_scenes)
                .add_systems(
                    Render,
                    render_batch_scenes
                        .run_if(batch_scenes_can_render_early)
                        .in_set(RenderSystems::ExtractCommands),
                )
                .add_systems(
                    Render,
                    render_batch_scenes.in_set(RenderSystems::PrepareMeshes),
                );
        }
    }
}

/// Everything the sync system touches on a terminal entity.
type TerminalQuery<'w> = (
    Entity,
    &'w Terminal,
    Ref<'w, TerminalRenderConfig>,
    &'w mut BatchMainState,
    &'w mut TerminalTexture,
    &'w mut TerminalStats,
    UiNode<'w>,
);

/// The user-owned UI presentation of a terminal, when the `ui` feature is on
/// and the entity has an `ImageNode`.
#[cfg(feature = "ui")]
type UiNode<'w> = Option<(&'w mut Node, &'w mut ImageNode)>;
#[cfg(not(feature = "ui"))]
type UiNode<'w> = ();

/// The fetched item of [`UiNode`].
#[cfg(feature = "ui")]
type UiNodeItem<'w> = Option<(Mut<'w, Node>, Mut<'w, ImageNode>)>;
#[cfg(not(feature = "ui"))]
type UiNodeItem<'w> = ();

/// Whether a terminal entity is presented through Bevy UI.
#[cfg(feature = "ui")]
type Presented = Has<ImageNode>;
#[cfg(not(feature = "ui"))]
type Presented = ();

#[cfg(feature = "ui")]
const fn is_presented(presented: bool) -> bool {
    presented
}
#[cfg(not(feature = "ui"))]
const fn is_presented((): ()) -> bool {
    false
}

/// The UI scale resource, when the `ui` feature is on.
#[cfg(feature = "ui")]
type UiScaleRes<'w> = Option<Res<'w, UiScale>>;
#[cfg(not(feature = "ui"))]
type UiScaleRes<'w> = ();

#[cfg(feature = "ui")]
fn ui_scale_value(ui_scale: &UiScaleRes<'_>) -> f32 {
    ui_scale.as_ref().map_or(1.0, |scale| scale.0)
}
#[cfg(not(feature = "ui"))]
fn ui_scale_value((): &UiScaleRes<'_>) -> f32 {
    1.0
}

/// Bevy's text resources, required for shaping and measurement.
type TextResources<'w> = (
    Res<'w, Assets<Font>>,
    ResMut<'w, TextPipeline>,
    ResMut<'w, FontAtlasSet>,
    ResMut<'w, FontCx>,
    ResMut<'w, LayoutCx>,
    ResMut<'w, ScaleCx>,
);

fn initialize_terminals(
    mut commands: Commands,
    added: Query<(Entity, &Terminal, &TerminalRenderConfig, Presented), Without<BatchMainState>>,
    mut images: ResMut<Assets<Image>>,
) {
    for (entity, terminal, config, presented) in &added {
        let raster_scale = resolve_raster_scale(config.raster.scale, is_presented(presented), None);
        let metrics = super::resolve_metrics(config, None);
        let raster_config = physical_config(metrics, raster_scale);
        let logical_cell_size = raster_config.cell_size / raster_scale;
        terminal
            .surface
            .set_cell_size(logical_cell_size.x, logical_cell_size.y);
        let size = terminal_pixel_size(terminal.surface.size(), &raster_config);
        let output = images.add(make_target_image(size));
        let glyph_atlas = images.add(make_glyph_atlas_image());
        commands.entity(entity).insert((
            TerminalTexture {
                image: output.clone(),
                size,
                logical_size: size.as_vec2() / raster_scale,
                raster_scale,
                cell_size: metrics.cell_size,
                font_size: metrics.font_size,
            },
            BatchMainState::new(output.clone(), glyph_atlas, raster_scale, raster_config),
            TerminalStats::default(),
        ));
    }
}

fn terminal_pixel_size(size: GridSize, raster: &RasterMetrics) -> UVec2 {
    UVec2::new(
        (f32::from(size.width) * raster.cell_size.x)
            .round()
            .max(1.0) as u32,
        (f32::from(size.height) * raster.cell_size.y)
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
    // presentation stage softens glyph edges and can open seams between adjacent glyph cells.
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

/// Sizes a user-owned UI node to the terminal's logical dimensions and points
/// its image at the texture. Placement (`position_type`, `left`, `top`, or a
/// parent layout) is left to the user.
#[cfg(feature = "ui")]
fn apply_ui_node(
    node: &mut Node,
    image_node: &mut ImageNode,
    image: &Handle<Image>,
    size: UVec2,
    raster_scale: f32,
) {
    let width = px(size.x as f32 / raster_scale);
    let height = px(size.y as f32 / raster_scale);
    if node.width != width {
        node.width = width;
    }
    if node.height != height {
        node.height = height;
    }
    if image_node.image != *image {
        image_node.image = image.clone();
    }
}

#[cfg(feature = "ui")]
fn present(ui: UiNodeItem<'_>, image: &Handle<Image>, size: UVec2, raster_scale: f32) -> bool {
    match ui {
        Some((mut node, mut image_node)) => {
            apply_ui_node(&mut node, &mut image_node, image, size, raster_scale);
            true
        }
        None => false,
    }
}
#[cfg(not(feature = "ui"))]
fn present((): UiNodeItem<'_>, _: &Handle<Image>, _: UVec2, _: f32) -> bool {
    false
}

#[cfg(feature = "ui")]
fn ui_present(ui: &UiNodeItem<'_>) -> bool {
    ui.is_some()
}
#[cfg(not(feature = "ui"))]
fn ui_present((): &UiNodeItem<'_>) -> bool {
    false
}

fn resolve_raster_scale(
    configured: TerminalRenderScale,
    presented: bool,
    window_scale: Option<f32>,
) -> f32 {
    let requested = match configured {
        TerminalRenderScale::Automatic if presented => window_scale.unwrap_or(1.0),
        TerminalRenderScale::Automatic => 1.0,
        TerminalRenderScale::Fixed(scale) => scale,
    };
    if requested.is_finite() && requested > 0.0 {
        requested.clamp(1.0, 8.0)
    } else {
        1.0
    }
}

/// Physical raster metrics derived from the logical configuration.
#[derive(Clone, Copy, Debug, PartialEq)]
struct RasterMetrics {
    /// Physical pixels per logical pixel.
    scale: f32,
    /// Physical cell size, snapped to whole pixels.
    cell_size: Vec2,
    /// Physical font size. Cells snap to whole physical pixels, but the font
    /// size stays fractional so a font can be sized to make its advance fill
    /// the cell exactly.
    font_size: f32,
    /// Uniform vertical shift applied to every glyph, in whole physical
    /// pixels, so the primary font's line box sits inside the cell (see
    /// [`super::vertical_offset`]).
    glyph_offset: f32,
}

fn physical_config(logical: super::LogicalMetrics, raster_scale: f32) -> RasterMetrics {
    RasterMetrics {
        scale: raster_scale,
        cell_size: (logical.cell_size * raster_scale).round().max(Vec2::ONE),
        font_size: (logical.font_size * raster_scale).max(1.0),
        glyph_offset: 0.0,
    }
}

fn font_size_for_cell(
    config: &TerminalRenderConfig,
    measured_advance: Option<f32>,
    raster: RasterMetrics,
) -> f32 {
    let fit_width = config.font_size == super::FontSizing::FitCellWidth
        || matches!(config.cell_size, super::CellSizing::FromFont { .. });
    measured_advance
        .filter(|advance| fit_width && *advance > 0.0)
        .map_or(raster.font_size, |advance| {
            (raster.cell_size.x * super::PROBE_FONT_SIZE / advance).max(1.0)
        })
}

/// ASCII glyphs whose ink must stay inside a cell: descenders, ascenders and
/// tall brackets. Measured for every configured face.
const CORE_PROBE: &str = "gjpqy|[]{}()_";
/// Accented capitals: kept inside the cell when the core box leaves room.
const ACCENT_PROBE: &str = "\u{c5}\u{c9}\u{1eaa}";
/// A full block: its box is the font's line box, which sizes the cell and is
/// kept covering the cell so tiles stay seamless.
const BLOCK_PROBE: &str = "\u{2588}";
/// Upper bound on cell-height refinement rounds.
const FIT_ROUNDS: usize = 3;

/// Refines the physical metrics after the logical fit: sizes the font from the
/// rounded physical cell width (so a fractional raster scale cannot open seams
/// between advances), grows the cell height to the primary font's line box
/// (measured on a full block glyph) and derives the vertical glyph offset from
/// the measured block, core-ASCII and accent ink boxes.
#[allow(clippy::too_many_arguments)]
fn refine_metrics(
    config: &TerminalRenderConfig,
    measured_advance: Option<f32>,
    mut raster: RasterMetrics,
    fonts: &Assets<Font>,
    images: &mut Assets<Image>,
    text_pipeline: &mut TextPipeline,
    font_atlas_set: &mut FontAtlasSet,
    font_cx: &mut FontCx,
    layout_cx: &mut LayoutCx,
    scale_cx: &mut ScaleCx,
) -> RasterMetrics {
    raster.font_size = font_size_for_cell(config, measured_advance, raster);
    let requested_height = raster.cell_size.y;
    // An explicit font size in an explicit cell is honored exactly; a font
    // derived from the cell width or a font-driven cell gets a cell at least
    // as tall as the font's line box.
    let may_grow = config.font_size == super::FontSizing::FitCellWidth
        || matches!(config.cell_size, super::CellSizing::FromFont { .. });
    let mut block = None;
    for _ in 0..FIT_ROUNDS {
        // The block's fully opaque rows are what tiles seamlessly; its anti-aliased
        // edge rows are excluded (falling back to the bitmap minus one row per side).
        block = shape_boxes(
            BLOCK_PROBE,
            &ResolvedStyle::plain(),
            config,
            raster,
            fonts,
            images,
            text_pipeline,
            font_atlas_set,
            font_cx,
            layout_cx,
            scale_cx,
        )
        .map(|(bitmap, opaque)| {
            opaque.unwrap_or(super::GlyphBox {
                top: bitmap.top + 1.0,
                bottom: (bitmap.bottom - 1.0).max(bitmap.top + 1.0),
            })
        });
        let height = super::fitted_cell_height(raster.cell_size.y, block);
        if !may_grow || height == raster.cell_size.y {
            break;
        }
        // The line box is centered in the cell-high line, so re-measure at the new height.
        raster.cell_size.y = height;
    }
    if raster.cell_size.y != requested_height {
        debug!(
            "bevy_terminal: cell height grown from {}px to {}px to fit the font's line box",
            requested_height, raster.cell_size.y
        );
    }
    let mut boxes = [None, None];
    for (probe, slot) in [CORE_PROBE, ACCENT_PROBE].into_iter().zip(&mut boxes) {
        for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
            if (bold && config.font.bold.is_none() && !config.font.synthesize)
                || (italic && config.font.italic.is_none() && !config.font.synthesize)
            {
                continue;
            }
            let mut style = ResolvedStyle::plain();
            style.bold = bold;
            style.italic = italic;
            let Some(measured) = shape_box(
                probe,
                &style,
                config,
                raster,
                fonts,
                images,
                text_pipeline,
                font_atlas_set,
                font_cx,
                layout_cx,
                scale_cx,
            ) else {
                continue;
            };
            *slot = Some(slot.map_or(measured, |union: super::GlyphBox| union.union(measured)));
        }
    }
    let [core, accents] = boxes;
    raster.glyph_offset = super::vertical_offset(raster.cell_size.y, block, core, accents);
    debug!(
        "bevy_terminal: cell {}x{}px font {:.2}px block {:?} core {:?} accents {:?} offset {}",
        raster.cell_size.x,
        raster.cell_size.y,
        raster.font_size,
        block,
        core,
        accents,
        raster.glyph_offset
    );
    raster
}

/// Shapes `text` exactly as [`cached_shape`] does and returns the vertical
/// extent of its glyph bitmaps relative to the line box top.
#[allow(clippy::too_many_arguments)]
fn shape_box(
    text: &str,
    style: &ResolvedStyle,
    config: &TerminalRenderConfig,
    raster: RasterMetrics,
    fonts: &Assets<Font>,
    images: &mut Assets<Image>,
    text_pipeline: &mut TextPipeline,
    font_atlas_set: &mut FontAtlasSet,
    font_cx: &mut FontCx,
    layout_cx: &mut LayoutCx,
    scale_cx: &mut ScaleCx,
) -> Option<super::GlyphBox> {
    shape_boxes(
        text,
        style,
        config,
        raster,
        fonts,
        images,
        text_pipeline,
        font_atlas_set,
        font_cx,
        layout_cx,
        scale_cx,
    )
    .map(|(bitmap, _)| bitmap)
}

/// Like [`shape_box`], but also returns the rows of the run's bitmaps that are
/// fully opaque across their width (the coverage a block glyph guarantees; its
/// first and last bitmap rows are usually anti-aliased edges), when the atlas
/// data is readable.
#[allow(clippy::too_many_arguments)]
fn shape_boxes(
    text: &str,
    style: &ResolvedStyle,
    config: &TerminalRenderConfig,
    raster: RasterMetrics,
    fonts: &Assets<Font>,
    images: &mut Assets<Image>,
    text_pipeline: &mut TextPipeline,
    font_atlas_set: &mut FontAtlasSet,
    font_cx: &mut FontCx,
    layout_cx: &mut LayoutCx,
    scale_cx: &mut ScaleCx,
) -> Option<(super::GlyphBox, Option<super::GlyphBox>)> {
    let layout = shape_run(
        text,
        style,
        config,
        raster,
        Vec2::splat(4096.0),
        fonts,
        images,
        text_pipeline,
        font_atlas_set,
        font_cx,
        layout_cx,
        scale_cx,
    );
    let mut bitmap: Option<super::GlyphBox> = None;
    let mut opaque: Option<super::GlyphBox> = None;
    for glyph in &layout.glyphs {
        let rect = glyph.atlas_info.rect;
        let height = rect.size().y;
        let top = super::snap(glyph.position.y - height * 0.5);
        let glyph_box = super::GlyphBox {
            top,
            bottom: top + height,
        };
        bitmap = Some(bitmap.map_or(glyph_box, |b| b.union(glyph_box)));
        if let Some(rows) = opaque_rows(images, glyph.atlas_info.texture, rect) {
            let rows = super::GlyphBox {
                top: top + rows.0 as f32,
                bottom: top + rows.1 as f32,
            };
            opaque = Some(opaque.map_or(rows, |b| b.union(rows)));
        }
    }
    bitmap.map(|bitmap| (bitmap, opaque))
}

/// Sum of alpha over each column of an atlas glyph (all `u32::MAX` when the
/// atlas has no CPU data, so every column counts as inked).
fn column_coverage(image: &Image, rect: Rect) -> Vec<u32> {
    let width = rect.size().x.max(0.0) as usize;
    let Some(data) = image.data.as_ref() else {
        return vec![u32::MAX; width];
    };
    let atlas_width = image.texture_descriptor.size.width as usize;
    let (x0, y0, y1) = (
        rect.min.x as usize,
        rect.min.y as usize,
        rect.max.y as usize,
    );
    (0..width)
        .map(|x| {
            (y0..y1)
                .map(|y| {
                    data.get((y * atlas_width + x0 + x) * 4 + 3)
                        .map_or(0, |alpha| u32::from(*alpha))
                })
                .sum()
        })
        .collect()
}

/// The half-open row range `[first, last)` of an atlas glyph whose alpha is
/// fully opaque across the glyph's width; `None` if the atlas has no CPU data
/// or no such row.
fn opaque_rows(images: &Assets<Image>, texture: AssetId<Image>, rect: Rect) -> Option<(u32, u32)> {
    let image = images.get(texture)?;
    let data = image.data.as_ref()?;
    let width = image.texture_descriptor.size.width as usize;
    let (x0, x1) = (rect.min.x as usize, rect.max.x as usize);
    let (y0, y1) = (rect.min.y as usize, rect.max.y as usize);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let row_opaque = |y: usize| {
        (x0..x1).all(|x| {
            data.get((y * width + x) * 4 + 3)
                .is_some_and(|alpha| *alpha >= 250)
        })
    };
    let first = (y0..y1).find(|y| row_opaque(*y))?;
    let last = (first..y1).take_while(|y| row_opaque(*y)).last()?;
    Some(((first - y0) as u32, (last + 1 - y0) as u32))
}

/// Shapes and rasterizes `text` in `style` at the physical metrics; the
/// layout's glyphs are positioned inside a line box `raster.cell_size.y` tall.
#[allow(clippy::too_many_arguments)]
fn shape_run(
    text: &str,
    style: &ResolvedStyle,
    config: &TerminalRenderConfig,
    raster: RasterMetrics,
    viewport: Vec2,
    fonts: &Assets<Font>,
    images: &mut Assets<Image>,
    text_pipeline: &mut TextPipeline,
    font_atlas_set: &mut FontAtlasSet,
    font_cx: &mut FontCx,
    layout_cx: &mut LayoutCx,
    scale_cx: &mut ScaleCx,
) -> TextLayoutInfo {
    let font = text_font(&config.font, raster.font_size, style);
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
            LineHeight::Px(raster.cell_size.y),
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
            config.raster.hinting,
        );
    }
    layout
}

#[derive(Clone)]
struct CachedGlyph {
    texture: AssetId<Image>,
    offset: Vec2,
    size: Vec2,
    uv: Vec4,
    alpha_mask: bool,
    /// Horizontal extent `[left, right)` of the bitmap's inked columns,
    /// relative to the bitmap.
    ink: (f32, f32),
    /// Coverage (sum of alpha) of every bitmap column; used to fit a run wider
    /// than its cell so that clipping drops the faintest columns.
    columns: Vec<u32>,
}

impl CachedGlyph {
    fn new(
        texture: AssetId<Image>,
        offset: Vec2,
        size: Vec2,
        uv: Vec4,
        alpha_mask: bool,
        columns: Vec<u32>,
    ) -> Self {
        let left = columns.iter().position(|c| *c > 0);
        let right = columns.iter().rposition(|c| *c > 0);
        let ink = match (left, right) {
            (Some(left), Some(right)) => (left as f32, right as f32 + 1.0),
            _ => (0.0, size.x),
        };
        Self {
            texture,
            offset,
            size,
            uv,
            alpha_mask,
            ink,
            columns,
        }
    }
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
            glyphs: HashMap::default(),
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
/// Shaped glyph runs per symbol, one map per font face.
///
/// Runs are stored in `entries` and looked up by index so a cache hit costs a
/// single hash lookup and returns a borrow that does not conflict with later
/// insertions.
struct ShapeCaches {
    entries: Vec<Vec<CachedGlyph>>,
    normal: StyleShapes,
    bold: StyleShapes,
    italic: StyleShapes,
    bold_italic: StyleShapes,
}

/// Sentinel for an unoccupied ASCII fast-path slot.
const ASCII_UNCACHED: u32 = u32::MAX;

/// Shape lookup for one bold/italic class: single-byte ASCII symbols — the
/// bulk of terminal content — index a table directly with no hashing or
/// string allocation; everything else uses the map.
struct StyleShapes {
    ascii: [u32; 128],
    other: HashMap<String, usize>,
}

impl Default for StyleShapes {
    fn default() -> Self {
        Self {
            ascii: [ASCII_UNCACHED; 128],
            other: HashMap::default(),
        }
    }
}

impl StyleShapes {
    fn clear(&mut self) {
        self.ascii = [ASCII_UNCACHED; 128];
        self.other.clear();
    }
}

/// The direct-index key for a single-byte ASCII symbol, if `text` is one.
fn ascii_key(text: &str) -> Option<u8> {
    match *text.as_bytes() {
        [byte] if byte < 128 => Some(byte),
        _ => None,
    }
}

#[derive(Default)]
struct SceneScratch {
    backgrounds: Vec<QuadInstance>,
    /// Background rectangles in pixel space, so adjoining rows can merge
    /// before conversion to clip-space quads.
    background_rects: Vec<(PixelGeometry, Color)>,
    /// Indices into `background_rects` emitted for the previous row.
    prev_runs: Vec<usize>,
    /// Indices into `background_rects` emitted for the current row.
    current_runs: Vec<usize>,
    foregrounds: Vec<QuadInstance>,
    glyphs: Vec<(AssetId<Image>, QuadInstance)>,
    decorations: Vec<QuadInstance>,
    cursor: Vec<QuadInstance>,
    styles: Vec<ResolvedStyle>,
}

impl SceneScratch {
    fn clear(&mut self) {
        self.backgrounds.clear();
        self.background_rects.clear();
        self.prev_runs.clear();
        self.current_runs.clear();
        self.foregrounds.clear();
        self.glyphs.clear();
        self.decorations.clear();
        self.cursor.clear();
        self.styles.clear();
    }
}

/// Records a background run, extending an identically aligned run from the
/// previous row into one taller rectangle when possible.
fn merge_background_rect(
    rects: &mut Vec<(PixelGeometry, Color)>,
    prev_runs: &[usize],
    current_runs: &mut Vec<usize>,
    geometry: PixelGeometry,
    color: Color,
) {
    for &index in prev_runs {
        let (rect, existing) = &mut rects[index];
        if *existing == color
            && rect.x == geometry.x
            && rect.width == geometry.width
            && (rect.y + rect.height - geometry.y).abs() < 0.01
        {
            // Anchor the merged bottom edge on the current row's own geometry
            // so accumulated float error cannot drift the rectangle.
            rect.height = geometry.y + geometry.height - rect.y;
            current_runs.push(index);
            return;
        }
    }
    rects.push((geometry, color));
    current_runs.push(rects.len() - 1);
}

impl ShapeCaches {
    fn select(&self, style: &ResolvedStyle) -> &StyleShapes {
        match (style.bold, style.italic) {
            (false, false) => &self.normal,
            (true, false) => &self.bold,
            (false, true) => &self.italic,
            (true, true) => &self.bold_italic,
        }
    }

    fn select_mut(&mut self, style: &ResolvedStyle) -> &mut StyleShapes {
        match (style.bold, style.italic) {
            (false, false) => &mut self.normal,
            (true, false) => &mut self.bold,
            (false, true) => &mut self.italic,
            (true, true) => &mut self.bold_italic,
        }
    }

    fn lookup(&self, style: &ResolvedStyle, text: &str) -> Option<usize> {
        let shapes = self.select(style);
        match ascii_key(text) {
            Some(byte) => {
                let index = shapes.ascii[usize::from(byte)];
                (index != ASCII_UNCACHED).then_some(index as usize)
            }
            None => shapes.other.get(text).copied(),
        }
    }

    fn insert(&mut self, style: &ResolvedStyle, text: &str, glyphs: Vec<CachedGlyph>) -> usize {
        let index = self.entries.len();
        self.entries.push(glyphs);
        let shapes = self.select_mut(style);
        match ascii_key(text) {
            Some(byte) => shapes.ascii[usize::from(byte)] = index as u32,
            None => {
                shapes.other.insert(text.to_owned(), index);
            }
        }
        index
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.normal.clear();
        self.bold.clear();
        self.italic.clear();
        self.bold_italic.clear();
    }
}

#[derive(Component)]
struct BatchMainState {
    output: Handle<Image>,
    /// Font asset ids in use, to scope re-measurement to this terminal's own fonts.
    font_ids: [Option<AssetId<Font>>; 4],
    /// Whether every handle font above is registered with the font context.
    fonts_ready: bool,
    /// Whether [`TerminalReady`] has been triggered for this terminal.
    ready_sent: bool,
    raster_scale: f32,
    raster_config: RasterMetrics,
    /// Advance of the regular font at the probe size; `None` until measured.
    measured_advance: Option<f32>,
    /// Logical font size in use.
    metrics: Option<super::LogicalMetrics>,
    last_snapshot: Option<TerminalSnapshot>,
    /// Whether the retained snapshot holds any `SLOW_BLINK`/`RAPID_BLINK` cells.
    snapshot_blinks: bool,
    pending: Option<BatchScene>,
    shapes: ShapeCaches,
    glyph_atlas: UnifiedGlyphAtlas,
    scratch: SceneScratch,
    blink: BlinkPhases,
}

impl BatchMainState {
    fn new(
        output: Handle<Image>,
        glyph_atlas: Handle<Image>,
        raster_scale: f32,
        raster_config: RasterMetrics,
    ) -> Self {
        Self {
            output,
            font_ids: [None; 4],
            fonts_ready: false,
            ready_sent: false,
            raster_scale,
            raster_config,
            measured_advance: None,
            metrics: None,
            last_snapshot: None,
            snapshot_blinks: false,
            pending: None,
            shapes: ShapeCaches::default(),
            glyph_atlas: UnifiedGlyphAtlas::new(glyph_atlas),
            scratch: SceneScratch::default(),
            blink: BlinkPhases::default(),
        }
    }
}

#[derive(Clone, Copy)]
struct DrawBatch {
    texture: AssetId<Image>,
    start: u32,
    count: u32,
    /// Replace the destination instead of alpha-blending over it. Used for
    /// cell backgrounds so a translucent theme background does not accumulate
    /// over stale texels when only some rows are repainted.
    replace: bool,
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
            slow_hidden: super::blink_hidden(elapsed, config.blink.slow_hz),
            rapid_hidden: super::blink_hidden(elapsed, config.blink.rapid_hz),
            cursor_hidden: super::blink_hidden(elapsed, config.cursor.blink_hz),
        }
    }

    fn hides(self, style: &ResolvedStyle) -> bool {
        (style.rapid_blink && self.rapid_hidden) || (style.slow_blink && self.slow_hidden)
    }
}

#[derive(Resource, Default)]
struct PendingBatchScenes(Vec<BatchScene>);

#[allow(clippy::too_many_arguments)]
fn sync_batch_terminals(
    mut commands: Commands,
    mut terminals: Query<TerminalQuery>,
    text: Option<TextResources>,
    mut images: ResMut<Assets<Image>>,
    font_events: Option<MessageReader<AssetEvent<Font>>>,
    primary_window: Query<&Window, With<PrimaryWindow>>,
    ui_scale: UiScaleRes,
    time: Option<Res<Time>>,
) {
    let Some((
        fonts,
        mut text_pipeline,
        mut font_atlas_set,
        mut font_cx,
        mut layout_cx,
        mut scale_cx,
    )) = text
    else {
        warn_once!(
            "bevy_terminal: Bevy's text resources are missing (add DefaultPlugins or TextPlugin); \
             terminals will not render"
        );
        // Nothing will ever be measured; the allocated texture is as final as it gets.
        for (entity, _, _, mut state, ..) in &mut terminals {
            if !state.ready_sent {
                state.ready_sent = true;
                commands.trigger(TerminalReady { entity });
            }
        }
        return;
    };
    let ui_scale = ui_scale_value(&ui_scale);
    let window_scale = primary_window
        .iter()
        .next()
        .map(|window| window.scale_factor() * ui_scale);
    let elapsed = time.as_ref().map_or(0.0, |time| time.elapsed_secs());
    // Font assets that changed this frame; only terminals using them re-shape.
    let changed_fonts: Vec<AssetId<Font>> = font_events
        .into_iter()
        .flat_map(|mut events| {
            events
                .read()
                .map(|event| match *event {
                    AssetEvent::Added { id }
                    | AssetEvent::Modified { id }
                    | AssetEvent::Removed { id }
                    | AssetEvent::Unused { id }
                    | AssetEvent::LoadedWithDependencies { id } => id,
                })
                .collect::<Vec<_>>()
        })
        .collect();
    for (entity, terminal, config, mut state, mut output, mut stats, ui) in &mut terminals {
        let config_changed = config.is_changed();
        let face_ids = font_asset_ids(&config.font);
        // Handle fonts become usable only once Bevy registers them with the font
        // context (assigning an alias); glyphs shaped before that used a fallback
        // family and must be re-shaped afterwards.
        let fonts_ready = face_ids
            .iter()
            .flatten()
            .all(|id| fonts.get(*id).is_some_and(|font| !font.alias.is_empty()));
        let fonts_changed = state.font_ids != face_ids
            || state.fonts_ready != fonts_ready
            || (!changed_fonts.is_empty()
                && face_ids
                    .iter()
                    .flatten()
                    .any(|id| changed_fonts.contains(id)));
        state.font_ids = face_ids;
        state.fonts_ready = fonts_ready;
        // A resize during the sync that first reports readiness is part of settling,
        // not a re-measure; only terminals that were already ready get the event.
        let was_ready = state.ready_sent;
        let previous_size = sync_batch_terminal(
            terminal,
            &config,
            config_changed,
            &mut state,
            &mut output,
            &mut stats,
            &fonts,
            fonts_changed,
            &mut images,
            &mut text_pipeline,
            &mut font_atlas_set,
            &mut font_cx,
            &mut layout_cx,
            &mut scale_cx,
            ui,
            window_scale,
            elapsed,
        );
        let metrics_ready = !needs_measured_advance(&config) || state.measured_advance.is_some();
        if fonts_ready && metrics_ready && !state.ready_sent {
            // The first sync with usable fonts settles the measured cell size, so the
            // texture is now at its final size for this configuration.
            state.ready_sent = true;
            commands.trigger(TerminalReady { entity });
        }
        if let Some(previous_size) = previous_size
            && was_ready
        {
            commands.trigger(TerminalRemeasured {
                entity,
                previous_size,
                size: output.size,
                cell_size: output.cell_size,
            });
        }
    }
}

/// Whether any snapshot cell carries a text blink attribute.
fn snapshot_blinks(snapshot: &TerminalSnapshot) -> bool {
    let blink = (StyleFlags::SLOW_BLINK | StyleFlags::RAPID_BLINK).bits();
    snapshot
        .cells()
        .iter()
        .any(|cell| cell.style.flags.bits() & blink != 0)
}

/// Font asset ids referenced by a set of faces (system/family sources have none).
fn font_asset_ids(faces: &super::FontFaces) -> [Option<AssetId<Font>>; 4] {
    let id = |source: Option<&FontSource>| match source {
        Some(FontSource::Handle(handle)) => Some(handle.id()),
        _ => None,
    };
    [
        id(Some(&faces.regular)),
        id(faces.bold.as_ref()),
        id(faces.italic.as_ref()),
        id(faces.bold_italic.as_ref()),
    ]
}

fn needs_measured_advance(config: &TerminalRenderConfig) -> bool {
    config.font_size == super::FontSizing::FitCellWidth
        || matches!(config.cell_size, super::CellSizing::FromFont { .. })
}

#[allow(clippy::too_many_arguments)]
fn sync_batch_terminal(
    terminal: &Terminal,
    config: &TerminalRenderConfig,
    config_changed: bool,
    state: &mut BatchMainState,
    output: &mut TerminalTexture,
    stats: &mut TerminalStats,
    fonts: &Assets<Font>,
    fonts_changed: bool,
    images: &mut Assets<Image>,
    text_pipeline: &mut TextPipeline,
    font_atlas_set: &mut FontAtlasSet,
    font_cx: &mut FontCx,
    layout_cx: &mut LayoutCx,
    scale_cx: &mut ScaleCx,
    ui: UiNodeItem<'_>,
    window_scale: Option<f32>,
    elapsed: f32,
) -> Option<UVec2> {
    let surface = &terminal.surface;
    let presented = ui_present(&ui);
    stats.changed_rows = 0;
    stats.snapshot_cells = 0;
    stats.solid_quads = 0;
    stats.glyph_quads = 0;
    stats.draw_batches = 0;
    stats.shape_misses = 0;
    stats.snapshot_ns = 0;
    stats.scene_ns = 0;

    let raster_scale = resolve_raster_scale(config.raster.scale, presented, window_scale);
    let scale_changed = state.raster_scale != raster_scale;
    let needs_measured_advance = needs_measured_advance(config);
    if needs_measured_advance
        && (state.measured_advance.is_none() || config_changed || fonts_changed)
    {
        state.measured_advance =
            super::measure_advance(&config.font, fonts, text_pipeline, font_cx, layout_cx);
    }
    // An unmeasured cell is not geometry. Keep the provisional component and
    // do not publish a scene or readiness event until the selected face shapes.
    if needs_measured_advance && state.measured_advance.is_none() {
        return None;
    }
    let metrics = super::resolve_metrics(config, state.measured_advance);
    let font_size_changed = state.metrics != Some(metrics);
    state.metrics = Some(metrics);
    let text_assets_changed = config_changed || fonts_changed || scale_changed || font_size_changed;
    if text_assets_changed {
        state.raster_config = refine_metrics(
            config,
            state.measured_advance,
            physical_config(metrics, raster_scale),
            fonts,
            images,
            text_pipeline,
            font_atlas_set,
            font_cx,
            layout_cx,
            scale_cx,
        );
        let logical_cell_size = state.raster_config.cell_size / raster_scale;
        surface.set_cell_size(logical_cell_size.x, logical_cell_size.y);
        output.font_size = state.raster_config.font_size / raster_scale;
        output.cell_size = logical_cell_size;
    }
    if text_assets_changed {
        state.shapes.clear();
        state.glyph_atlas.clear(images);
    }
    let blink = BlinkPhases::at(elapsed, config);
    // A phase flip only matters where it changes pixels: text phases when the
    // snapshot holds blinking cells, the cursor phase when the cursor shows.
    let text_blink_changed = state.snapshot_blinks
        && (blink.slow_hidden != state.blink.slow_hidden
            || blink.rapid_hidden != state.blink.rapid_hidden);
    let cursor_blink_changed = blink.cursor_hidden != state.blink.cursor_hidden
        && state
            .last_snapshot
            .as_ref()
            .is_some_and(cursor_should_be_visible);
    let blink_changed = text_blink_changed || cursor_blink_changed;
    if state.last_snapshot.as_ref().is_some_and(|snapshot| {
        snapshot.revision() == surface.revision() && !text_assets_changed && !blink_changed
    }) {
        // Keep the recorded phases current so an irrelevant flip is not
        // mistaken for a change once blinking content appears later.
        state.blink = blink;
        return None;
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
            if text_blink_changed {
                rows.extend(0..snapshot.size().height);
            } else {
                // Only the cursor phase flipped: its row is the only change.
                rows.push(snapshot.cursor_position().y);
                rows.retain(|row| *row < snapshot.size().height);
            }
            rows.sort_unstable();
            rows.dedup();
        }
        (snapshot, rows, full)
    } else {
        let snapshot = surface.snapshot();
        stats.snapshot_cells = u32::try_from(snapshot.cells().len()).unwrap_or(u32::MAX);
        let rows = (0..snapshot.size().height).collect();
        (snapshot, rows, true)
    };

    stats.snapshot_ns = snapshot_start
        .elapsed()
        .as_nanos()
        .min(u128::from(u64::MAX)) as u64;
    if changed_rows.is_empty() && !full && !blink_changed {
        state.last_snapshot = Some(snapshot);
        return None;
    }
    // Extraction can be delayed while a newly created output or glyph atlas reaches the render
    // world. If a newer payload is already waiting in the main world, make its replacement a
    // complete image of the newest snapshot so rows changed by an intermediate payload cannot be
    // lost when that payload is superseded.
    full |= state.pending.is_some();

    let new_size = terminal_pixel_size(snapshot.size(), &state.raster_config);
    let output_resized = output.size != new_size;
    let previous_size = output_resized.then_some(output.size);
    if output_resized {
        // Reallocate the image in place so the handle stays stable; the render world
        // recreates the GPU texture for the modified asset.
        if images
            .insert(state.output.id(), make_target_image(new_size))
            .is_err()
        {
            warn!("bevy_terminal: could not reallocate a terminal texture in place");
        }
        output.size = new_size;
    }
    // Write the texture component only when something changed so `Changed<TerminalTexture>`
    // observers are not woken on every synced frame.
    let logical_size = new_size.as_vec2() / raster_scale;
    if output.logical_size != logical_size || output.raster_scale != raster_scale {
        output.logical_size = logical_size;
        output.raster_scale = raster_scale;
    }
    present(ui, &state.output, new_size, raster_scale);

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
        config,
        *raster_config,
        &rows,
        full,
        destination,
        fonts,
        images,
        text_pipeline,
        font_atlas_set,
        font_cx,
        layout_cx,
        scale_cx,
        shapes,
        glyph_atlas,
        scratch,
        stats,
        blink,
    );
    // Existing GPU textures are safe to consume before Bevy's asset preparation systems. A
    // resize or shape miss can create/modify an Image this frame, so those scenes use the later
    // submission point after RenderAsset preparation instead.
    scene.requires_prepared_assets = output_resized || stats.shape_misses != 0;
    stats.scene_ns = scene_start.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
    stats.changed_rows = u32::try_from(rows.len()).unwrap_or(u32::MAX);
    stats.draw_batches = u32::try_from(scene.batches.len()).unwrap_or(u32::MAX);
    state.pending = Some(scene);
    state.snapshot_blinks = snapshot_blinks(&snapshot);
    state.last_snapshot = Some(snapshot);
    state.blink = blink;
    state.raster_scale = raster_scale;
    previous_size
}

#[allow(clippy::too_many_arguments)]
fn build_scene(
    snapshot: &TerminalSnapshot,
    config: &TerminalRenderConfig,
    raster: RasterMetrics,
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
    stats: &mut TerminalStats,
    blink: BlinkPhases,
) -> BatchScene {
    let size = terminal_pixel_size(snapshot.size(), &raster).as_vec2();
    let raster_scale = raster.scale;
    scratch.clear();
    scratch.styles.reserve(usize::from(snapshot.size().width));
    let SceneScratch {
        backgrounds,
        background_rects,
        prev_runs,
        current_runs,
        foregrounds,
        glyphs,
        decorations,
        cursor,
        styles,
    } = scratch;

    for &row in rows {
        current_runs.clear();
        if !full {
            // Partial repaints interleave a per-row clear with that row's runs,
            // so later rows' clears would overwrite runs merged upward; merge
            // vertically only in full rebuilds (which have no per-row clears).
            background_rects.push((
                PixelGeometry {
                    x: 0.0,
                    y: f32::from(row) * raster.cell_size.y,
                    width: size.x,
                    height: raster.cell_size.y,
                },
                config.theme.background,
            ));
        }
        let cells = snapshot.row(row);
        styles.clear();
        styles.extend(
            cells
                .iter()
                .map(|cell| ResolvedStyle::new(cell, &config.theme)),
        );
        let mut background_start = 0;
        while background_start < styles.len() {
            let color = styles[background_start].background;
            let mut background_end = background_start + 1;
            while background_end < styles.len() && styles[background_end].background == color {
                background_end += 1;
            }
            if full && color == config.theme.background {
                background_start = background_end;
                continue;
            }
            let geometry = PixelGeometry {
                x: background_start as f32 * raster.cell_size.x,
                y: f32::from(row) * raster.cell_size.y,
                width: (background_end - background_start) as f32 * raster.cell_size.x,
                height: raster.cell_size.y,
            };
            if full {
                merge_background_rect(background_rects, prev_runs, current_runs, geometry, color);
            } else {
                background_rects.push((geometry, color));
            }
            background_start = background_end;
        }
        std::mem::swap(prev_runs, current_runs);

        let mut column = 0;
        while column < cells.len() {
            let cell = &cells[column];
            if cell.is_continuation() {
                column += 1;
                continue;
            }
            let width = cell_span(cells, column);
            let style = &styles[column];
            let symbol = cell.symbol();
            if style.hidden || blink.hides(style) {
                column += width;
                continue;
            }
            if symbol != " " && !symbol.is_empty() {
                let shaped = cached_shape(
                    symbol,
                    style,
                    config,
                    raster,
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
                    column as f32 * raster.cell_size.x,
                    f32::from(row) * raster.cell_size.y,
                );
                let cell_bounds = PixelGeometry {
                    x: anchor.x,
                    y: anchor.y,
                    width: width as f32 * raster.cell_size.x,
                    height: raster.cell_size.y,
                };
                let shift = Vec2::new(
                    fit_horizontally(shaped, cell_bounds.width),
                    raster.glyph_offset,
                );
                for glyph in shaped {
                    let geometry = PixelGeometry {
                        x: anchor.x + glyph.offset.x + shift.x,
                        y: anchor.y + glyph.offset.y + shift.y,
                        width: glyph.size.x,
                        height: glyph.size.y,
                    };
                    if let Some((geometry, uv)) =
                        clip_glyph_to_cell(geometry, glyph.uv, cell_bounds)
                    {
                        glyphs.push((
                            glyph.texture,
                            glyph_quad(geometry, uv, style.foreground, glyph.alpha_mask, size),
                        ));
                    }
                }
            }
            let decoration_x = column as f32 * raster.cell_size.x;
            let decoration_width = width as f32 * raster.cell_size.x;
            let decoration_thickness = raster_scale.round().max(1.0);
            if style.underlined {
                decorations.push(solid_quad(
                    PixelGeometry {
                        x: decoration_x,
                        y: f32::from(row) * raster.cell_size.y
                            + (raster.cell_size.y - 2.0 * decoration_thickness).max(0.0),
                        width: decoration_width,
                        height: decoration_thickness,
                    },
                    style.underline,
                    size,
                ));
            }
            if style.crossed_out {
                decorations.push(solid_quad(
                    PixelGeometry {
                        x: decoration_x,
                        y: f32::from(row) * raster.cell_size.y + raster.cell_size.y * 0.55,
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

    backgrounds.extend(
        background_rects
            .iter()
            .map(|&(geometry, color)| solid_quad(geometry, color, size)),
    );

    if cursor_should_be_visible(snapshot)
        && !blink.cursor_hidden
        && (full || rows.contains(&snapshot.cursor_position().y))
    {
        let position = snapshot.cursor_position();
        let cursor_thickness = raster_scale.round().max(1.0) * 2.0;
        let (x, y, width, height) = match config.cursor.style {
            super::CursorStyle::Block => (0.0, 0.0, raster.cell_size.x, raster.cell_size.y),
            super::CursorStyle::Bar => (
                0.0,
                0.0,
                cursor_thickness.min(raster.cell_size.x),
                raster.cell_size.y,
            ),
            super::CursorStyle::Underline => (
                0.0,
                (raster.cell_size.y - cursor_thickness).max(0.0),
                raster.cell_size.x,
                cursor_thickness.min(raster.cell_size.y),
            ),
        };
        cursor.push(solid_quad(
            PixelGeometry {
                x: f32::from(position.x) * raster.cell_size.x + x,
                y: f32::from(position.y) * raster.cell_size.y + y,
                width,
                height,
            },
            config.cursor.color,
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
    append_batch_with(
        &mut instances,
        &mut batches,
        primary_atlas,
        backgrounds,
        true,
    );
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
    raster: RasterMetrics,
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
    stats: &mut TerminalStats,
) -> &'a [CachedGlyph] {
    if let Some(index) = shapes.lookup(style, text) {
        return &shapes.entries[index];
    }
    stats.shape_misses = stats.shape_misses.saturating_add(1);
    let layout = shape_run(
        text,
        style,
        config,
        raster,
        viewport,
        fonts,
        images,
        text_pipeline,
        font_atlas_set,
        font_cx,
        layout_cx,
        scale_cx,
    );
    let cached = layout
        .glyphs
        .into_iter()
        .filter_map(|glyph| {
            let atlas = images.get(glyph.atlas_info.texture)?;
            let atlas_size = atlas.texture_descriptor.size;
            let rect = glyph.atlas_info.rect;
            let size = rect.size();
            let columns = column_coverage(atlas, rect);
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
            Some(CachedGlyph::new(
                texture,
                // Atlas texels must land on physical pixel boundaries. Bevy's layout positions
                // can retain fractional shaping offsets even though the glyph bitmap is an
                // integer-sized raster image.
                (glyph.position - size * 0.5).map(super::snap),
                size,
                uv,
                glyph.atlas_info.is_alpha_mask,
                columns,
            ))
        })
        .collect::<Vec<_>>();
    let index = shapes.insert(style, text, cached);
    &shapes.entries[index]
}

/// Horizontal shift (whole pixels) that keeps a run's bitmaps inside the
/// `span` it is drawn in: a run that fits but overhangs one side (an italic
/// or a negative bearing) is pushed inside; a run inside the span keeps its
/// bearings; a run wider than the span (a fallback family with a larger
/// advance, a wide italic) is placed so the clipped columns carry the least
/// coverage — centered when the sides are equally faint.
///
/// # Sub-pixel overshoot
///
/// Box-drawing and block glyphs are commonly drawn a little past their
/// advance on purpose (JetBrains Mono's `─` spans -20..620 units of a 600
/// advance) so that neighbouring strokes overlap instead of gapping. Rasterised,
/// that overshoot lights one faint extra column outside the span. Pushing the
/// run inside for it would move `┌` one pixel away from a `│` in the row
/// below, which is exactly the misalignment the overshoot exists to prevent.
/// A single outside column on either side whose coverage is below the run's
/// strongest column is therefore treated as overshoot: the run keeps its
/// bearings and the per-cell clip drops the column, the way Ghostty renders
/// ordinary text without any alignment constraint. A full-strength column
/// outside the span is real overhang and is still pushed inside.
fn fit_horizontally(glyphs: &[CachedGlyph], span: f32) -> f32 {
    let mut left = f32::INFINITY;
    let mut right = f32::NEG_INFINITY;
    for glyph in glyphs {
        left = left.min(glyph.offset.x + glyph.ink.0);
        right = right.max(glyph.offset.x + glyph.ink.1);
    }
    if right <= left {
        return 0.0;
    }
    let (left, right) = trim_overshoot(glyphs, span, left, right);
    if right <= left {
        0.0
    } else if right - left <= span {
        if left < 0.0 {
            super::snap(-left)
        } else if right > span {
            super::snap(span - right)
        } else {
            0.0
        }
    } else {
        // Try every whole shift that keeps the run covering the span and keep the
        // one retaining the most coverage; ties resolve toward the centered shift.
        let centered = super::snap((span - (right - left)) / 2.0 - left);
        let lowest = super::snap(span - right);
        let highest = super::snap(-left);
        let retained = |shift: f32| -> u64 {
            glyphs
                .iter()
                .map(|glyph| {
                    let start = glyph.offset.x + shift;
                    glyph
                        .columns
                        .iter()
                        .enumerate()
                        .filter(|(index, _)| {
                            let x = start + *index as f32;
                            x >= 0.0 && x < span
                        })
                        .map(|(_, coverage)| u64::from(*coverage))
                        .sum::<u64>()
                })
                .sum()
        };
        let mut best = centered;
        let mut best_retained = retained(centered);
        let mut shift = lowest;
        while shift <= highest {
            let value = retained(shift);
            if value > best_retained
                || (value == best_retained && (shift - centered).abs() < (best - centered).abs())
            {
                best = shift;
                best_retained = value;
            }
            shift += 1.0;
        }
        best
    }
}

/// Largest number of outside columns per side that may be sub-pixel overshoot.
const OVERSHOOT_COLUMNS: f32 = 1.0;

/// Narrows a run's ink extents `[left, right)` by dropping, on each side, a
/// single outside column that is fainter than the run's strongest column (a
/// rasterised sub-pixel overshoot). Extents of runs without such columns are
/// returned unchanged.
fn trim_overshoot(glyphs: &[CachedGlyph], span: f32, left: f32, right: f32) -> (f32, f32) {
    let coverage_at = |x: f32| -> u32 {
        glyphs
            .iter()
            .filter_map(|glyph| {
                let index = x - glyph.offset.x;
                (index >= 0.0)
                    .then(|| glyph.columns.get(index as usize).copied())
                    .flatten()
            })
            .sum()
    };
    let peak = glyphs
        .iter()
        .flat_map(|glyph| glyph.columns.iter().copied())
        .max()
        .unwrap_or(0);
    let mut trimmed_left = left;
    let mut trimmed_right = right;
    let left_over = -left;
    if left_over > 0.0 && left_over <= OVERSHOOT_COLUMNS && coverage_at(left) < peak {
        trimmed_left = left + left_over;
    }
    let right_over = right - span;
    if right_over > 0.0 && right_over <= OVERSHOOT_COLUMNS && coverage_at(right - 1.0) < peak {
        trimmed_right = right - right_over;
    }
    (trimmed_left, trimmed_right)
}

fn solid_quad(geometry: PixelGeometry, color: Color, target: Vec2) -> QuadInstance {
    QuadInstance {
        rect: clip_rect(snap_geometry(geometry), target),
        // A negative final UV component lets the unified fragment shader skip the atlas sample.
        uv: Vec4::new(0.0, 0.0, 0.0, -1.0),
        color: color.to_linear().to_f32_array().into(),
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

fn clip_glyph_to_cell(
    glyph: PixelGeometry,
    uv: Vec4,
    cell: PixelGeometry,
) -> Option<(PixelGeometry, Vec4)> {
    if glyph.width <= 0.0 || glyph.height <= 0.0 || cell.width <= 0.0 || cell.height <= 0.0 {
        return None;
    }
    let left = glyph.x.max(cell.x);
    let top = glyph.y.max(cell.y);
    let right = (glyph.x + glyph.width).min(cell.x + cell.width);
    let bottom = (glyph.y + glyph.height).min(cell.y + cell.height);
    if right <= left || bottom <= top {
        return None;
    }

    let u_span = uv.z - uv.x;
    let v_span = uv.w - uv.y;
    let clipped_uv = Vec4::new(
        ((left - glyph.x) / glyph.width).mul_add(u_span, uv.x),
        ((top - glyph.y) / glyph.height).mul_add(v_span, uv.y),
        ((right - glyph.x) / glyph.width).mul_add(u_span, uv.x),
        ((bottom - glyph.y) / glyph.height).mul_add(v_span, uv.y),
    );
    Some((
        PixelGeometry {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        },
        clipped_uv,
    ))
}

fn snap_geometry(geometry: PixelGeometry) -> PixelGeometry {
    let left = super::snap(geometry.x);
    let top = super::snap(geometry.y);
    let right = super::snap(geometry.x + geometry.width).max(left);
    let bottom = super::snap(geometry.y + geometry.height).max(top);
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
    append_batch_with(instances, batches, texture, quads, false);
}

fn append_batch_with(
    instances: &mut Vec<QuadInstance>,
    batches: &mut Vec<DrawBatch>,
    texture: AssetId<Image>,
    quads: &[QuadInstance],
    replace: bool,
) {
    if quads.is_empty() {
        return;
    }
    let start = instances.len() as u32;
    let count = quads.len() as u32;
    instances.extend_from_slice(quads);
    if let Some(previous) = batches.last_mut()
        && previous.texture == texture
        && previous.replace == replace
        && previous.start + previous.count == start
    {
        previous.count += count;
    } else {
        batches.push(DrawBatch {
            texture,
            start,
            count,
            replace,
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

fn extract_batch_scenes(
    mut main_world: ResMut<MainWorld>,
    mut pending: ResMut<PendingBatchScenes>,
) {
    let mut terminals = main_world.query::<&mut BatchMainState>();
    for mut state in terminals.iter_mut(&mut main_world) {
        if let Some(scene) = state.pending.take() {
            pending.0.push(scene);
        }
    }
}

fn batch_scenes_can_render_early(pending: Res<PendingBatchScenes>) -> bool {
    !pending.0.is_empty()
        && pending
            .0
            .iter()
            .all(|scene| !scene.requires_prepared_assets)
}

#[derive(Default, Resource)]
struct BatchGpuState {
    vertex_buffer: Option<Buffer>,
    vertex_capacity: u64,
    /// Persistent CPU staging for instance serialization, reused every frame.
    staging: Vec<u8>,
    texture_layout: Option<BindGroupLayout>,
    pipeline: Option<RenderPipeline>,
    replace_pipeline: Option<RenderPipeline>,
    texture_bind_groups: HashMap<AssetId<Image>, (TextureId, BindGroup)>,
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
fn append_instance_bytes(instances: &[QuadInstance], bytes: &mut Vec<u8>) {
    let start = bytes.len();
    bytes.resize(start + instances.len() * 48, 0);
    for (chunk, instance) in bytes[start..].chunks_exact_mut(48).zip(instances) {
        let values = [
            instance.rect.to_array(),
            instance.uv.to_array(),
            instance.color.to_array(),
        ];
        for (slot, value) in chunk.chunks_exact_mut(4).zip(values.as_flattened()) {
            slot.copy_from_slice(&value.to_ne_bytes());
        }
    }
}

fn render_batch_scenes(
    mut pending: ResMut<PendingBatchScenes>,
    mut gpu: ResMut<BatchGpuState>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    let scenes = std::mem::take(&mut pending.0);
    let mut renderable = Vec::with_capacity(scenes.len());
    for scene in scenes {
        let Some(target) = gpu_images.get(scene.destination) else {
            pending.0.push(scene);
            continue;
        };
        let target_size = target.texture_descriptor.size;
        if target_size.width != scene.destination_size.x
            || target_size.height != scene.destination_size.y
        {
            // An Image asset replacement can coexist with its previous render asset for a frame.
            // Keep the complete replacement scene pending until the matching GPU texture is ready.
            pending.0.push(scene);
            continue;
        }
        if scene
            .batches
            .iter()
            .any(|batch| gpu_images.get(batch.texture).is_none())
        {
            pending.0.push(scene);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{GridSize, TerminalCell, TerminalStyle};

    fn quad(value: f32) -> QuadInstance {
        QuadInstance {
            rect: Vec4::splat(value),
            uv: Vec4::ZERO,
            color: Vec4::ONE,
        }
    }

    fn test_app() -> App {
        let mut app = App::new();
        app.init_resource::<Assets<Image>>()
            .add_plugins(TerminalPlugin);
        app
    }

    #[test]
    fn spawned_terminals_own_distinct_textures_and_can_be_despawned() {
        let first_surface = TerminalSurface::new((12, 4));
        let second_surface = TerminalSurface::new((7, 9));
        let mut app = test_app();
        // A presented terminal: the user owns the UI node.
        let first = app
            .world_mut()
            .spawn((
                Terminal::new(first_surface.clone()),
                ImageNode::default(),
                Node {
                    position_type: PositionType::Absolute,
                    left: px(30.0),
                    top: px(40.0),
                    ..default()
                },
            ))
            .id();
        app.update();
        // A second, headless terminal spawned later with an explicit config.
        let second = app
            .world_mut()
            .spawn((
                Terminal::new(second_surface.clone()),
                TerminalRenderConfig {
                    cell_size: Vec2::new(11.0, 20.0).into(),
                    ..default()
                },
            ))
            .id();
        app.update();

        let mut terminals = app
            .world_mut()
            .query::<(Entity, &Terminal, &TerminalTexture, &TerminalStats)>();
        let mut instances = terminals
            .iter(app.world())
            .map(|(entity, terminal, texture, _)| {
                (
                    entity,
                    terminal.surface().size(),
                    texture.image.id(),
                    texture.size,
                )
            })
            .collect::<Vec<_>>();
        instances.sort_by_key(|(_, size, _, _)| size.width);

        assert_eq!(instances.len(), 2);
        assert_eq!(instances[0].0, second);
        assert_eq!(instances[0].1, GridSize::new(7, 9));
        assert_eq!(instances[0].3, UVec2::new(77, 180));
        assert_eq!(instances[1].0, first);
        assert_eq!(instances[1].1, GridSize::new(12, 4));
        assert_eq!(instances[1].3, UVec2::new(132, 80));
        assert_ne!(instances[0].2, instances[1].2);
        // Both required a config; the first got the default one.
        assert!(app.world().get::<TerminalRenderConfig>(first).is_some());
        assert_eq!(first_surface.snapshot().size().width, 12);
        assert_eq!(second_surface.snapshot().size().width, 7);

        // Despawning a terminal removes everything with the entity.
        app.world_mut().despawn(first);
        app.update();
        assert_eq!(
            app.world_mut()
                .query::<&TerminalTexture>()
                .iter(app.world())
                .count(),
            1
        );
    }

    /// An app with Bevy's text pipeline but no window or renderer: enough to
    /// shape glyphs into the main-world caches and exercise the sync system.
    fn text_app() -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            bevy::text::TextPlugin,
            TerminalPlugin,
        ))
        .init_asset::<Image>();
        app
    }

    fn write_text(surface: &TerminalSurface, text: &str) {
        surface.update(|update| {
            for (column, symbol) in text.chars().enumerate() {
                update.set_cell((column as u16, 0), &TerminalCell::from(symbol));
            }
        });
    }

    #[test]
    fn config_changes_rebuild_but_unrelated_changes_keep_the_shape_cache() {
        #[derive(Component)]
        struct Unrelated(u32);
        let mut app = text_app();
        let surface = TerminalSurface::new((6, 1));
        write_text(&surface, "hello!");
        let entity = app
            .world_mut()
            .spawn((Terminal::new(surface.clone()), Unrelated(0)))
            .id();
        for _ in 0..4 {
            app.update();
        }
        // Redrawing the same content re-uses the shape cache: no misses, no rows.
        write_text(&surface, "hello!");
        app.update();
        let idle = *app.world().get::<TerminalStats>(entity).unwrap();
        assert_eq!(idle.shape_misses, 0, "{idle}");
        assert_eq!(idle.changed_rows, 0);
        // Rewriting a cell with a new glyph shapes only that glyph.
        surface.update(|u| {
            u.set_cell((0, 0), &TerminalCell::new("Z"));
        });
        app.update();
        let one = *app.world().get::<TerminalStats>(entity).unwrap();
        assert_eq!(one.shape_misses, 1, "{one}");
        assert_eq!(one.changed_rows, 1);

        // Touching an unrelated component and redrawing identical content: the
        // shape cache survives and nothing is rebuilt.
        app.world_mut().get_mut::<Unrelated>(entity).unwrap().0 += 1;
        write_text(&surface, "Zello!");
        app.update();
        let after_unrelated = *app.world().get::<TerminalStats>(entity).unwrap();
        assert_eq!(after_unrelated.shape_misses, 0);
        assert_eq!(after_unrelated.changed_rows, 0);

        // Changing the render config re-shapes everything.
        app.world_mut()
            .get_mut::<TerminalRenderConfig>(entity)
            .unwrap()
            .cell_size = Vec2::new(12.0, 22.0).into();
        app.update();
        let after_config = *app.world().get::<TerminalStats>(entity).unwrap();
        assert!(after_config.shape_misses > 0, "{after_config}");
        assert_eq!(after_config.changed_rows, 1);
        // The font is sized to the 12 px width; the requested 22 px height is a
        // minimum that grows to the (default) font's line box.
        let size = app.world().get::<TerminalTexture>(entity).unwrap().size;
        assert_eq!(size.x, 72);
        assert!(size.y >= 22, "{size:?}");
    }

    #[test]
    fn resizing_keeps_the_texture_handle_and_updates_the_ui_node() {
        let mut app = text_app();
        let surface = TerminalSurface::new((4, 2));
        write_text(&surface, "abcd");
        let entity = app
            .world_mut()
            .spawn((
                Terminal::new(surface.clone()),
                TerminalRenderConfig {
                    cell_size: Vec2::new(10.0, 20.0).into(),
                    ..default()
                },
                ImageNode::default(),
                Node::default(),
            ))
            .id();
        for _ in 0..3 {
            app.update();
        }
        let texture = app.world().get::<TerminalTexture>(entity).unwrap().clone();
        assert_eq!(texture.size, UVec2::new(40, 40));
        let node = app.world().get::<Node>(entity).unwrap();
        assert_eq!(node.width, px(40.0));
        assert_eq!(node.height, px(40.0));
        assert_eq!(
            app.world().get::<ImageNode>(entity).unwrap().image,
            texture.image
        );

        surface.update(|update| {
            update.resize((8, 3));
        });
        app.update();
        let resized = app.world().get::<TerminalTexture>(entity).unwrap();
        assert_eq!(resized.image, texture.image, "handle must stay stable");
        assert_eq!(resized.size, UVec2::new(80, 60));
        let image = app
            .world()
            .resource::<Assets<Image>>()
            .get(&texture.image)
            .expect("the image was reallocated in place");
        assert_eq!(image.width(), 80);
        assert_eq!(image.height(), 60);
        let node = app.world().get::<Node>(entity).unwrap();
        assert_eq!(node.width, px(80.0));
        assert_eq!(node.height, px(60.0));

        surface.update(|update| {
            update.resize((2, 1));
        });
        app.update();
        let shrunk = app.world().get::<TerminalTexture>(entity).unwrap();
        assert_eq!(shrunk.image, texture.image);
        assert_eq!(shrunk.size, UVec2::new(20, 20));
    }

    #[test]
    fn unrelated_font_assets_do_not_trigger_remeasurement() {
        let mut app = text_app();
        let surface = TerminalSurface::new((4, 1));
        write_text(&surface, "abcd");
        let entity = app.world_mut().spawn(Terminal::new(surface.clone())).id();
        for _ in 0..3 {
            app.update();
        }
        let baseline = *app.world().get::<TerminalStats>(entity).unwrap();
        assert_eq!(baseline.shape_misses, 0, "{baseline}");
        // Adding a font this terminal does not use must not clear its caches:
        // the next redraw of the same text shapes nothing.
        app.world_mut()
            .resource_mut::<Assets<Font>>()
            .add(Font::from_bytes(Vec::new()));
        app.update();
        write_text(&surface, "abcd");
        app.update();
        let after = *app.world().get::<TerminalStats>(entity).unwrap();
        assert_eq!(after.shape_misses, 0, "{after}");
        assert_eq!(after.changed_rows, 0);
    }

    #[test]
    fn font_driven_cells_measure_the_embedded_font() {
        let mut app = text_app();
        let regular = app
            .world_mut()
            .resource_mut::<Assets<Font>>()
            .add(Font::from_bytes(
                include_bytes!("../../assets/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf")
                    .to_vec(),
            ));
        let surface = TerminalSurface::new((4, 1));
        write_text(&surface, "abcd");
        let entity = app
            .world_mut()
            .spawn((
                Terminal::new(surface.clone()),
                TerminalRenderConfig {
                    cell_size: super::super::CellSizing::FROM_FONT,
                    font_size: super::super::FontSizing::Px(20.0),
                    font: super::super::FontFaces::regular(regular),
                    ..default()
                },
            ))
            .id();
        for _ in 0..4 {
            app.update();
        }
        let texture = app.world().get::<TerminalTexture>(entity).unwrap();
        // JetBrains Mono's advance is 0.6 em: 12 px wide at 20 px. Its full
        // block has 26 fully covered rows, which becomes the measured height.
        assert!(
            (texture.cell_size.x - 12.0).abs() < 0.05,
            "{:?}",
            texture.cell_size
        );
        assert!(
            (texture.cell_size.y - 26.0).abs() < 0.05,
            "{:?}",
            texture.cell_size
        );
        assert_eq!(texture.size, UVec2::new(48, 26));
        assert_eq!(
            texture.grid_for(Vec2::new(125.0, 60.0)),
            GridSize::new(10, 2)
        );

        // Zoom: a larger font grows the cell and the texture (same handle).
        let handle = texture.image.clone();
        app.world_mut()
            .get_mut::<TerminalRenderConfig>(entity)
            .unwrap()
            .font_size = super::super::FontSizing::Px(30.0);
        app.update();
        let zoomed = app.world().get::<TerminalTexture>(entity).unwrap();
        assert!((zoomed.cell_size.x - 18.0).abs() < 0.05);
        // 30 px: 18 px advance and a measured 39 px block box.
        assert_eq!(zoomed.size, UVec2::new(72, 39));
        assert_eq!(zoomed.image, handle);
    }

    #[test]
    fn font_driven_cells_wait_for_a_loading_handle_before_ready() {
        #[derive(Resource, Default)]
        struct Ready(usize);

        let mut app = text_app();
        app.init_resource::<Ready>()
            .add_observer(|_: On<TerminalReady>, mut ready: ResMut<Ready>| ready.0 += 1);
        let regular = Handle::<Font>::from(bevy::asset::uuid::Uuid::from_u128(0x5241_5454_5901));
        let entity = app
            .world_mut()
            .spawn((
                Terminal::new(TerminalSurface::new((4, 1))),
                TerminalRenderConfig {
                    cell_size: super::super::CellSizing::FROM_FONT,
                    font_size: super::super::FontSizing::Px(20.0),
                    font: super::super::FontFaces::regular(regular.clone()),
                    ..default()
                },
            ))
            .id();

        for _ in 0..2 {
            app.update();
        }
        assert_eq!(app.world().resource::<Ready>().0, 0);
        let state = app.world().get::<BatchMainState>(entity).unwrap();
        assert!(state.measured_advance.is_none());
        assert!(
            state.last_snapshot.is_none(),
            "no 1x1 scene may be published"
        );

        app.world_mut()
            .resource_mut::<Assets<Font>>()
            .insert(
                regular.id(),
                Font::from_bytes(
                    include_bytes!("../../assets/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf")
                        .to_vec(),
                ),
            )
            .expect("the pending font id is unused");
        for _ in 0..3 {
            app.update();
        }

        assert_eq!(app.world().resource::<Ready>().0, 1);
        let texture = app.world().get::<TerminalTexture>(entity).unwrap();
        assert!((texture.cell_size.x - 12.0).abs() < 0.05);
        assert!((texture.cell_size.y - 26.0).abs() < 0.05);
        assert_eq!(texture.size, UVec2::new(48, 26));
    }

    fn glyph(offset_x: f32, columns: &[u32]) -> CachedGlyph {
        CachedGlyph::new(
            AssetId::default(),
            Vec2::new(offset_x, 0.0),
            Vec2::new(columns.len() as f32, 10.0),
            Vec4::ZERO,
            true,
            columns.to_vec(),
        )
    }

    #[test]
    fn horizontal_fit_pushes_overhang_inside_and_centers_overflow() {
        // Inside the span: bearings are kept.
        assert_eq!(fit_horizontally(&[glyph(2.0, &[9, 9, 9])], 11.0), 0.0);
        // Overhanging left (an italic): pushed right by the overhang.
        assert_eq!(fit_horizontally(&[glyph(-2.0, &[9; 8])], 11.0), 2.0);
        // Overhanging right: pushed left.
        assert_eq!(fit_horizontally(&[glyph(6.0, &[9; 8])], 11.0), -3.0);
        // Leading transparent columns do not count as ink.
        assert_eq!(fit_horizontally(&[glyph(-2.0, &[0, 0, 9, 9])], 11.0), 0.0);
        // Wider than the span with symmetric coverage: centered.
        assert_eq!(fit_horizontally(&[glyph(0.0, &[9; 15])], 11.0), -2.0);
        // Wider than the span with a faint left column: the faint side is clipped.
        let mut columns = vec![255; 12];
        columns[0] = 3;
        assert_eq!(fit_horizontally(&[glyph(0.0, &columns)], 11.0), -1.0);
        columns.reverse();
        assert_eq!(fit_horizontally(&[glyph(0.0, &columns)], 11.0), 0.0);
        // Blank runs never shift.
        assert_eq!(fit_horizontally(&[glyph(3.0, &[0, 0])], 11.0), 0.0);
    }

    /// A box-drawing bar drawn a fraction past its advance rasterises to one
    /// faint column outside the span; that is overshoot to clip, not overhang
    /// to push, or `┌` would land a pixel away from `│`.
    #[test]
    fn horizontal_fit_ignores_sub_pixel_overshoot() {
        // `─`: full-strength bar across the cell plus a 47% column past it.
        let mut bar = vec![255; 11];
        bar.push(120);
        assert_eq!(fit_horizontally(&[glyph(0.0, &bar)], 11.0), 0.0);
        // The same on the left (`┐`'s bar reaching into the previous cell).
        let mut bar = vec![120];
        bar.extend([255; 11]);
        assert_eq!(fit_horizontally(&[glyph(-1.0, &bar)], 11.0), 0.0);
        // Overshoot on both sides at once.
        let mut bar = vec![120];
        bar.extend([255; 11]);
        bar.push(120);
        assert_eq!(fit_horizontally(&[glyph(-1.0, &bar)], 11.0), 0.0);
        // A full-strength column outside the span is real overhang: pushed.
        assert_eq!(fit_horizontally(&[glyph(3.0, &[255; 9])], 11.0), -1.0);
        // Two faint columns are past the tolerance: the run is wider than the
        // span and placed by retained coverage, which keeps the solid columns.
        let mut bar = vec![255; 11];
        bar.extend([120, 120]);
        assert_eq!(fit_horizontally(&[glyph(0.0, &bar)], 11.0), 0.0);
        // A negative-bearing italic with a solid first column is still pushed.
        assert_eq!(fit_horizontally(&[glyph(-1.0, &[255; 9])], 11.0), 1.0);
    }

    #[test]
    fn snapping_and_clipping_keep_glyphs_that_fit_inside_their_cell() {
        let cell = PixelGeometry {
            x: 22.0,
            y: 40.0,
            width: 11.0,
            height: 20.0,
        };
        // A glyph that fits mathematically survives snapping intact.
        let glyph = PixelGeometry {
            x: 22.0,
            y: 40.0,
            width: 11.0,
            height: 20.0,
        };
        let (clipped, _) = clip_glyph_to_cell(glyph, Vec4::new(0.0, 0.0, 1.0, 1.0), cell).unwrap();
        let snapped = snap_geometry(clipped);
        assert_eq!(
            (snapped.x, snapped.y, snapped.width, snapped.height),
            (22.0, 40.0, 11.0, 20.0)
        );
        // A glyph a pixel below the cell loses exactly that pixel row and its UVs.
        let glyph = PixelGeometry {
            x: 22.0,
            y: 41.0,
            width: 11.0,
            height: 20.0,
        };
        let (clipped, uv) = clip_glyph_to_cell(glyph, Vec4::new(0.0, 0.0, 1.0, 1.0), cell).unwrap();
        assert_eq!(clipped.height, 19.0);
        assert!((uv.w - 0.95).abs() < 1e-6, "{uv:?}");
        // Halves snap consistently: a rectangle at .5 keeps its size.
        let snapped = snap_geometry(PixelGeometry {
            x: 0.5,
            y: -0.5,
            width: 4.0,
            height: 4.0,
        });
        assert_eq!(
            (snapped.x, snapped.y, snapped.width, snapped.height),
            (1.0, 0.0, 4.0, 4.0)
        );
    }

    #[test]
    fn grid_and_scale_helpers() {
        assert_eq!(
            grid_for(Vec2::new(805.0, 245.0), Vec2::new(10.0, 20.0)),
            GridSize::new(80, 12)
        );
        assert_eq!(
            grid_for(Vec2::ZERO, Vec2::new(10.0, 20.0)),
            GridSize::new(1, 1)
        );
        let mut window = Window::default();
        window.resolution.set_scale_factor(2.0);
        window.resolution.set(1200.0, 800.0);
        assert_eq!(window.resolution.size(), Vec2::new(1200.0, 800.0));
        assert_eq!(
            grid_for_window(&window, Vec2::new(10.0, 20.0)),
            GridSize::new(120, 40)
        );
        assert!((raster_scale_for_window(&window) - 2.0).abs() < 1e-4);
        window.resolution.set_scale_factor(0.5);
        assert!((raster_scale_for_window(&window) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn terminal_ready_fires_once_per_terminal() {
        #[derive(Resource, Default)]
        struct Ready(Vec<(Entity, UVec2)>);
        let mut app = test_app();
        app.init_resource::<Ready>().add_observer(
            |ready: On<TerminalReady>,
             mut seen: ResMut<Ready>,
             textures: Query<&TerminalTexture>| {
                let size = textures.get(ready.entity).map_or(UVec2::ZERO, |t| t.size);
                seen.0.push((ready.entity, size));
            },
        );
        let entity = app
            .world_mut()
            .spawn(Terminal::new(TerminalSurface::new((4, 2))))
            .id();
        app.update();
        app.update();
        app.update();
        let seen = &app.world().resource::<Ready>().0;
        assert_eq!(seen, &[(entity, UVec2::new(44, 40))]);
        let texture = app.world().get::<TerminalTexture>(entity).unwrap();
        assert_eq!(texture.size, UVec2::new(44, 40));
    }

    #[test]
    fn terminal_remeasured_fires_on_resizes_after_ready() {
        #[derive(Resource, Default)]
        struct Seen(Vec<(UVec2, UVec2)>);
        let mut app = text_app();
        app.init_resource::<Seen>().add_observer(
            |event: On<TerminalRemeasured>, mut seen: ResMut<Seen>| {
                seen.0.push((event.previous_size, event.size));
            },
        );
        let surface = TerminalSurface::new((4, 2));
        let entity = app.world_mut().spawn(Terminal::new(surface.clone())).id();
        app.update();
        app.update();
        assert!(
            app.world().resource::<Seen>().0.is_empty(),
            "settling is not a re-measure"
        );
        let initial = app.world().get::<TerminalTexture>(entity).unwrap().size;
        surface.update(|update| {
            update.resize((8, 3));
        });
        app.update();
        let texture = app.world().get::<TerminalTexture>(entity).unwrap();
        assert_ne!(texture.size, initial);
        assert_eq!(
            app.world().resource::<Seen>().0,
            vec![(initial, texture.size)]
        );
        app.update();
        assert_eq!(
            app.world().resource::<Seen>().0.len(),
            1,
            "no event without a resize"
        );
    }

    #[test]
    fn user_ui_node_receives_the_texture_and_headless_terminals_use_scale_one() {
        assert_eq!(
            resolve_raster_scale(TerminalRenderScale::Automatic, true, Some(2.0)),
            2.0
        );
        assert_eq!(
            resolve_raster_scale(TerminalRenderScale::Automatic, false, Some(2.0)),
            1.0
        );
        let mut node = Node {
            position_type: PositionType::Absolute,
            left: px(12.0),
            top: px(8.0),
            ..default()
        };
        let mut image_node = ImageNode::default();
        let handle = Handle::<Image>::default();
        apply_ui_node(
            &mut node,
            &mut image_node,
            &handle,
            UVec2::new(220, 400),
            2.0,
        );
        assert_eq!(node.width, px(110.0));
        assert_eq!(node.height, px(200.0));
        assert_eq!(node.left, px(12.0));
        assert_eq!(node.top, px(8.0));
        assert_eq!(image_node.image, handle);
        let stats = TerminalStats::default();
        assert_eq!(stats.changed_rows, 0);
        assert!(stats.to_string().contains("rows 0"));
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
            resolve_raster_scale(TerminalRenderScale::Automatic, true, Some(2.0),),
            2.0
        );
        assert_eq!(
            resolve_raster_scale(TerminalRenderScale::Automatic, false, Some(2.0),),
            1.0
        );
        assert_eq!(
            resolve_raster_scale(TerminalRenderScale::Fixed(1.5), false, None,),
            1.5
        );
        assert_eq!(
            resolve_raster_scale(TerminalRenderScale::Fixed(f32::NAN), true, None,),
            1.0
        );
    }

    #[test]
    fn physical_metrics_and_geometry_are_pixel_aligned() {
        let physical = physical_config(
            super::super::LogicalMetrics {
                font_size: 17.6,
                cell_size: Vec2::new(10.8, 19.6),
            },
            2.0,
        );
        assert_eq!(physical.cell_size, Vec2::new(22.0, 39.0));
        assert!((physical.font_size - 35.2).abs() < 1e-4);

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
    fn font_driven_cells_refit_the_font_after_physical_pixel_rounding() {
        let raster = physical_config(
            super::super::LogicalMetrics {
                font_size: 23.0,
                cell_size: Vec2::new(11.5, 1.0),
            },
            1.0,
        );
        let from_font = TerminalRenderConfig {
            cell_size: super::super::CellSizing::FROM_FONT,
            font_size: super::super::FontSizing::Px(23.0),
            ..default()
        };
        assert_eq!(raster.cell_size.x, 12.0);
        assert_eq!(font_size_for_cell(&from_font, Some(32.0), raster), 24.0);

        let explicit = TerminalRenderConfig {
            cell_size: super::super::CellSizing::Logical(Vec2::new(11.5, 20.0)),
            ..from_font
        };
        assert_eq!(font_size_for_cell(&explicit, Some(32.0), raster), 23.0);
    }

    #[test]
    fn fallback_glyph_bitmaps_are_clipped_to_their_terminal_cells() {
        let clipped = clip_glyph_to_cell(
            PixelGeometry {
                x: -2.0,
                y: 3.0,
                width: 16.0,
                height: 20.0,
            },
            Vec4::new(0.1, 0.2, 0.9, 0.8),
            PixelGeometry {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
        )
        .expect("the glyph overlaps the cell");

        assert_eq!(
            clipped.0,
            PixelGeometry {
                x: 0.0,
                y: 3.0,
                width: 10.0,
                height: 7.0,
            }
        );
        assert!(clipped.1.abs_diff_eq(Vec4::new(0.2, 0.2, 0.7, 0.41), 1e-6));
        assert!(
            clip_glyph_to_cell(
                PixelGeometry {
                    x: 20.0,
                    y: 20.0,
                    width: 5.0,
                    height: 5.0,
                },
                Vec4::ONE,
                PixelGeometry {
                    x: 0.0,
                    y: 0.0,
                    width: 10.0,
                    height: 10.0,
                },
            )
            .is_none()
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
        let mut bytes = Vec::new();
        append_instance_bytes(&instances, &mut bytes);
        assert!(bytes.is_empty());
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
            blink: super::super::BlinkConfig {
                slow_hz: Some(1.0),
                rapid_hz: Some(2.0),
            },
            cursor: super::super::CursorConfig {
                blink_hz: None,
                ..default()
            },
            ..default()
        };
        let visible = BlinkPhases::at(0.1, &config);
        assert!(!visible.slow_hidden && !visible.rapid_hidden && !visible.cursor_hidden);
        let hidden = BlinkPhases::at(0.3, &config);
        assert!(!hidden.slow_hidden && hidden.rapid_hidden && !hidden.cursor_hidden);

        config.cursor.blink_hz = Some(1.0);
        assert!(BlinkPhases::at(0.6, &config).cursor_hidden);
    }

    /// A text app with a manually driven clock, for deterministic blink tests.
    fn timed_app() -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins.build().disable::<bevy::time::TimePlugin>(),
            bevy::asset::AssetPlugin::default(),
            bevy::text::TextPlugin,
            TerminalPlugin,
        ))
        .init_asset::<Image>()
        .init_resource::<Time>();
        app
    }

    fn advance(app: &mut App, seconds: f32) {
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(seconds));
    }

    /// Consumes pending payloads the way render-world extraction would, so a
    /// later partial repaint is not upgraded to a full one.
    fn drain_pending(app: &mut App) {
        let mut states = app.world_mut().query::<&mut BatchMainState>();
        for mut state in states.iter_mut(app.world_mut()) {
            state.pending = None;
        }
    }

    #[test]
    fn blink_phases_only_rebuild_blinking_content() {
        let mut app = timed_app();
        let surface = TerminalSurface::new((6, 3));
        write_text(&surface, "hello!");
        let entity = app.world_mut().spawn(Terminal::new(surface.clone())).id();
        for _ in 0..4 {
            app.update();
        }

        // No blinking cells, hidden cursor: a text/cursor phase flip is
        // invisible and must not rebuild anything.
        advance(&mut app, 0.6);
        app.update();
        let idle = *app.world().get::<TerminalStats>(entity).unwrap();
        assert_eq!(idle.changed_rows, 0, "{idle}");

        // A visible blinking cursor dirties only its own row on a phase flip.
        surface.update(|update| {
            update.set_cursor_position((0, 2));
            update.set_cursor_visible(true);
        });
        app.update();
        drain_pending(&mut app);
        advance(&mut app, 0.5);
        app.update();
        let cursor_only = *app.world().get::<TerminalStats>(entity).unwrap();
        assert_eq!(cursor_only.changed_rows, 1, "{cursor_only}");

        // A SLOW_BLINK cell restores the full-surface phase rebuild.
        surface.update(|update| {
            let mut cell = TerminalCell::new("x");
            cell.style = TerminalStyle::new().with(StyleFlags::SLOW_BLINK);
            update.set_cell((0, 0), &cell);
        });
        app.update();
        drain_pending(&mut app);
        advance(&mut app, 0.5);
        app.update();
        let blinking = *app.world().get::<TerminalStats>(entity).unwrap();
        assert_eq!(blinking.changed_rows, 3, "{blinking}");
    }

    #[test]
    fn full_rebuilds_merge_identical_backgrounds_vertically() {
        let mut app = text_app();
        let surface = TerminalSurface::new((4, 3));
        surface.update(|update| {
            for row in 0..3 {
                for column in 0..4 {
                    let mut cell = TerminalCell::new(" ");
                    cell.style =
                        TerminalStyle::new().bg(crate::scene::TerminalColor::Rgb(200, 30, 30));
                    update.set_cell((column, row), &cell);
                }
            }
        });
        let entity = app.world_mut().spawn(Terminal::new(surface.clone())).id();
        for _ in 0..4 {
            app.update();
        }
        // Force a full rebuild and check the uniform background collapsed into
        // a single quad instead of one per row.
        app.world_mut()
            .get_mut::<TerminalRenderConfig>(entity)
            .unwrap()
            .cell_size = Vec2::new(12.0, 22.0).into();
        app.update();
        let stats = *app.world().get::<TerminalStats>(entity).unwrap();
        assert_eq!(stats.changed_rows, 3, "{stats}");
        assert_eq!(stats.solid_quads, 1, "{stats}");
    }

    #[test]
    fn ascii_and_non_ascii_symbols_reuse_the_shape_cache() {
        let mut app = text_app();
        let surface = TerminalSurface::new((4, 1));
        write_text(&surface, "abéé");
        let entity = app.world_mut().spawn(Terminal::new(surface.clone())).id();
        for _ in 0..4 {
            app.update();
        }
        // Re-shuffling the same symbols shapes nothing new: the ASCII fast
        // path and the map fallback both hit.
        write_text(&surface, "ébéa");
        app.update();
        let stats = *app.world().get::<TerminalStats>(entity).unwrap();
        assert_eq!(stats.shape_misses, 0, "{stats}");
        assert_eq!(stats.changed_rows, 1, "{stats}");
        // A genuinely new symbol still misses once.
        write_text(&surface, "cbéa");
        app.update();
        let stats = *app.world().get::<TerminalStats>(entity).unwrap();
        assert_eq!(stats.shape_misses, 1, "{stats}");
    }

    #[test]
    fn instance_bytes_append_whole_instances_in_order() {
        let instances = [
            QuadInstance {
                rect: Vec4::new(1.0, 2.0, 3.0, 4.0),
                uv: Vec4::new(5.0, 6.0, 7.0, 8.0),
                color: Vec4::new(9.0, 10.0, 11.0, 12.0),
            },
            quad(42.0),
        ];
        let mut bytes = Vec::new();
        append_instance_bytes(&instances, &mut bytes);
        assert_eq!(bytes.len(), 96);
        let floats: Vec<f32> = bytes
            .chunks_exact(4)
            .map(|chunk| f32::from_ne_bytes(chunk.try_into().unwrap()))
            .collect();
        assert_eq!(
            &floats[..12],
            &[
                1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0
            ]
        );
        assert_eq!(&floats[12..16], &[42.0; 4]);
        // Appending again extends at the previous end, as the shared staging
        // buffer relies on.
        append_instance_bytes(&instances[1..], &mut bytes);
        assert_eq!(bytes.len(), 144);
    }
}
