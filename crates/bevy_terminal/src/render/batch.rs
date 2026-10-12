//! Retained-state synchronization and render scheduling.

mod constraint;
mod gpu;
pub(super) mod metrics;
#[rustfmt::skip]
mod nerd_font;
mod quad;
mod scene;
mod shaping;
mod sprite;
use gpu::{extract_batch_scenes, init_batch_pipelines, load_shader, render_batch_scenes};
use metrics::{
    LogicalMetrics, RasterMetrics, measure_advance, physical_config, refine_metrics,
    resolve_metrics,
};
use quad::QuadInstance;
use scene::{RowStates, SceneInput, SceneScratch, build_scene};
use shaping::{AtlasUpload, ShapeCaches, UnifiedGlyphAtlas};

use std::{
    marker::PhantomData,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

#[cfg(feature = "timings")]
use std::time::Instant;

use bevy::{
    asset::{AssetId, RenderAssetUsages},
    image::ImageSampler,
    platform::collections::{HashMap, HashSet},
    prelude::*,
    render::{
        ExtractSchedule, RenderApp, RenderStartup,
        render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages},
        renderer::{RenderDevice, RenderGraph, RenderGraphSystems},
    },
    text::{FontAtlasSet, FontCx, LayoutCx, ScaleCx, TextError, TextPipeline},
};

use super::{
    Face, Palette, ResolvedStyle, TerminalFailure, TerminalGeometry, TerminalRenderConfig,
    TerminalSource, TerminalStats, TerminalStatus, TerminalTexture, cell_span,
    cursor_should_be_visible, text_font,
};
use crate::{
    grid::TerminalGrid,
    scene::{CellPosition, GridSize, StyleFlags},
};

/// The terminal texture format: the shader emits linear colors and the sRGB
/// target encodes them, so the image is display-ready for UI, sprites and 3D
/// materials and dark tones do not band in 8-bit storage.
const TARGET_FORMAT: TextureFormat = TextureFormat::Rgba8UnormSrgb;
const GLYPH_FORMAT: TextureFormat = TextureFormat::Rgba8UnormSrgb;
const GLYPH_ATLAS_SIZE: u32 = 2048;

/// Installs the terminal renderer and draws every [`TerminalGrid`] entity.
///
/// Glyphs are shaped and rasterized by Bevy text. Each terminal is represented
/// by compact GPU quad instances drawn into its own renderer-owned texture;
/// GPU pipelines and scratch buffers are shared between terminals. Adding it
/// more than once, or after a [`TerminalSourcePlugin`] added it, is harmless.
#[derive(Clone, Copy, Debug, Default)]
pub struct TerminalPlugin;

impl Plugin for TerminalPlugin {
    fn build(&self, app: &mut App) {
        if app.world().contains_resource::<SceneQueue>() {
            return;
        }
        app.init_resource::<FontCatalog>()
            .init_resource::<SceneQueue>()
            .add_systems(
                Update,
                refresh_font_catalog.in_set(super::TerminalSystems::Sync),
            );
        register_source::<TerminalGrid>(app);
        if app.get_sub_app(RenderApp).is_none() {
            return;
        }
        load_shader(app);
        let device = app.world().resource::<SceneQueue>().device.clone();
        app.sub_app_mut(RenderApp)
            .init_resource::<PendingBatchScenes>()
            .insert_resource(gpu::DeviceGeneration(device))
            .add_systems(RenderStartup, init_batch_pipelines)
            .add_systems(ExtractSchedule, extract_batch_scenes)
            // Terminal textures are drawn first, so cameras sample this
            // frame's content.
            .add_systems(
                RenderGraph,
                render_batch_scenes.in_set(RenderGraphSystems::Begin),
            );
    }

    fn is_unique(&self) -> bool {
        false
    }
}

/// Draws every entity with the [`TerminalSource`] `S`, as
/// [`TerminalPlugin`] does for [`TerminalGrid`]. Adds the `TerminalPlugin`
/// if it is missing.
pub struct TerminalSourcePlugin<S>(PhantomData<fn() -> S>);

impl<S> Default for TerminalSourcePlugin<S> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<S: TerminalSource + Component> Plugin for TerminalSourcePlugin<S> {
    fn build(&self, app: &mut App) {
        app.add_plugins(TerminalPlugin);
        register_source::<S>(app);
    }
}

fn register_source<S: TerminalSource + Component>(app: &mut App) {
    app.add_observer(initialize_terminal::<S>)
        .add_observer(release_terminal::<S>)
        .add_systems(
            Update,
            sync_terminals::<S>
                .in_set(super::TerminalSystems::Sync)
                .after(refresh_font_catalog),
        );
}

/// Draws the grid of the [`TerminalSource`] resource `R` into every entity
/// marked with [`ResourceTerminal<R>`], for producers that keep their
/// terminal in a resource. Adds the [`TerminalPlugin`] if it is missing.
pub struct TerminalResourcePlugin<R>(PhantomData<fn() -> R>);

impl<R> Default for TerminalResourcePlugin<R> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<R: TerminalSource + Resource> Plugin for TerminalResourcePlugin<R> {
    fn build(&self, app: &mut App) {
        app.add_plugins(TerminalPlugin)
            .add_observer(initialize_resource_terminal::<R>)
            .add_observer(release_terminal::<ResourceTerminal<R>>)
            .add_systems(
                Update,
                sync_resource_terminals::<R>
                    .in_set(super::TerminalSystems::Sync)
                    .after(refresh_font_catalog),
            );
    }
}

/// Draws the grid of the resource `R` into this entity's [`TerminalTexture`],
/// after adding [`TerminalResourcePlugin<R>`]. Several entities may draw the
/// same resource, each with its own [`TerminalRenderConfig`].
#[derive(Component)]
#[require(TerminalRenderConfig)]
pub struct ResourceTerminal<R: Send + Sync + 'static>(PhantomData<fn() -> R>);

impl<R: Send + Sync + 'static> Default for ResourceTerminal<R> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

/// Drops the renderer's components when a terminal stops rendering (its
/// source is removed or the entity despawned), and has the render world drop
/// its scene and free its atlas.
fn release_terminal<C: Component>(
    remove: On<Remove<C>>,
    states: Query<&BatchMainState>,
    mut queue: ResMut<SceneQueue>,
    mut commands: Commands,
) {
    if let Ok(state) = states.get(remove.entity) {
        queue.release(state.output.id(), state.glyph_atlas.id);
    }
    commands
        .entity(remove.entity)
        .try_remove::<(BatchMainState, TerminalTexture, TerminalStats)>();
}

/// Everything the sync system touches on a terminal entity.
type TerminalQuery<'w, S> = (
    Entity,
    Ref<'w, S>,
    Ref<'w, TerminalRenderConfig>,
    &'w mut BatchMainState,
    &'w mut TerminalTexture,
    &'w mut TerminalStats,
);

/// What the sync system touches on a terminal entity besides its source.
type RendererQuery<'w> = (
    Entity,
    Ref<'w, TerminalRenderConfig>,
    &'w mut BatchMainState,
    &'w mut TerminalTexture,
    &'w mut TerminalStats,
);

/// One [`RendererQuery`] item.
type RendererItem<'w> = (
    Entity,
    Ref<'w, TerminalRenderConfig>,
    Mut<'w, BatchMainState>,
    Mut<'w, TerminalTexture>,
    Mut<'w, TerminalStats>,
);

/// Bevy's text resources, required for shaping and measurement. They are
/// borrowed mutably (which marks them changed) only by terminals that
/// measure or shape, never by idle ones.
#[derive(bevy::ecs::system::SystemParam)]
struct TextResources<'w> {
    fonts: Res<'w, Assets<Font>>,
    images: ResMut<'w, Assets<Image>>,
    text_pipeline: ResMut<'w, TextPipeline>,
    font_atlas_set: ResMut<'w, FontAtlasSet>,
    font_cx: ResMut<'w, FontCx>,
    layout_cx: ResMut<'w, LayoutCx>,
    scale_cx: ResMut<'w, ScaleCx>,
}

/// Why a terminal's text could not be measured or shaped. It is formatted
/// only when logged, and a repeated failure is logged once.
#[derive(Debug, PartialEq)]
enum ShapingFailure {
    /// The regular font's advance could not be measured.
    Advance {
        font: FontSource,
        error: metrics::AdvanceError,
    },
    /// Bevy's text layout of a run failed.
    Layout { font: FontSource, error: TextError },
    /// Rasterizing a run into Bevy's font atlases failed.
    Rasterization { font: FontSource, error: TextError },
    /// A shaped glyph's Bevy font atlas was missing.
    AtlasUnavailable { fonts: Box<super::FontFaces> },
}

impl std::fmt::Display for ShapingFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Advance { font, error } => write!(f, "advance measurement for {font:?}: {error}"),
            Self::Layout { font, error } => write!(f, "layout for {font:?}: {error}"),
            Self::Rasterization { font, error } => {
                write!(f, "rasterization for {font:?}: {error}")
            }
            Self::AtlasUnavailable { fonts } => write!(f, "glyph atlas unavailable for {fonts:?}"),
        }
    }
}

/// Why a terminal cannot draw this update.
#[derive(Debug, PartialEq)]
enum SyncError {
    /// It waits for its fonts.
    Loading,
    /// It cannot be drawn for this reason.
    Failed(TerminalFailure),
    /// Its text could not be measured or shaped.
    Shaping(ShapingFailure),
}

impl From<TerminalFailure> for SyncError {
    fn from(failure: TerminalFailure) -> Self {
        Self::Failed(failure)
    }
}

impl From<ShapingFailure> for SyncError {
    fn from(failure: ShapingFailure) -> Self {
        Self::Shaping(failure)
    }
}

struct TextContext<'a> {
    /// The first failure of this update.
    failure: Option<ShapingFailure>,
    fonts: &'a Assets<Font>,
    images: &'a mut Assets<Image>,
    text_pipeline: &'a mut TextPipeline,
    font_atlas_set: &'a mut FontAtlasSet,
    font_cx: &'a mut FontCx,
    layout_cx: &'a mut LayoutCx,
    scale_cx: &'a mut ScaleCx,
}

impl TextResources<'_> {
    fn context(&mut self) -> TextContext<'_> {
        TextContext {
            failure: None,
            fonts: &self.fonts,
            images: &mut self.images,
            text_pipeline: &mut self.text_pipeline,
            font_atlas_set: &mut self.font_atlas_set,
            font_cx: &mut self.font_cx,
            layout_cx: &mut self.layout_cx,
            scale_cx: &mut self.scale_cx,
        }
    }
}

/// Gives a terminal its texture, statistics and renderer state as soon as
/// its source is added.
fn initialize_terminal<S: TerminalSource + Component>(
    add: On<Add<S>>,
    terminals: Query<(&S, &TerminalRenderConfig)>,
    images: ResMut<Assets<Image>>,
    device: Option<Res<RenderDevice>>,
    commands: Commands,
) {
    if let Ok((terminal, config)) = terminals.get(add.entity) {
        let grid = terminal.grid().size();
        initialize(add.entity, grid, config, images, device, commands);
    }
}

/// [`initialize_terminal`] for a terminal drawing a resource, which may not
/// exist yet.
fn initialize_resource_terminal<R: TerminalSource + Resource>(
    add: On<Add<ResourceTerminal<R>>>,
    source: Option<Res<R>>,
    configs: Query<&TerminalRenderConfig>,
    images: ResMut<Assets<Image>>,
    device: Option<Res<RenderDevice>>,
    commands: Commands,
) {
    if let Ok(config) = configs.get(add.entity) {
        let grid = source.map_or(GridSize::new(1, 1), |source| source.grid().size());
        initialize(add.entity, grid, config, images, device, commands);
    }
}

/// Gives a terminal drawing a `grid`-sized grid its texture, statistics and
/// renderer state.
fn initialize(
    entity: Entity,
    grid: GridSize,
    config: &TerminalRenderConfig,
    mut images: ResMut<Assets<Image>>,
    device: Option<Res<RenderDevice>>,
    mut commands: Commands,
) {
    let raster_scale = resolve_raster_scale(config.raster.scale);
    let metrics = if config.sizing.is_valid() {
        resolve_metrics(config, None)
    } else {
        LogicalMetrics {
            cell_size: Vec2::ONE,
            font_size: 16.0,
        }
    };
    let requested = terminal_pixel_size(grid, &physical_config(metrics, raster_scale));
    let size = if requested.max_element() <= texture_limit(device.as_deref()) {
        requested
    } else {
        UVec2::ONE
    };
    let output = images.add(make_target_image(size));
    commands.entity(entity).insert((
        TerminalTexture {
            status: TerminalStatus::Loading,
            image: output.clone(),
        },
        BatchMainState::new(output),
        TerminalStats::default(),
    ));
}

fn texture_limit(device: Option<&RenderDevice>) -> u32 {
    device.map_or(8192, |device| device.limits().max_texture_dimension_2d)
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

fn resolve_raster_scale(requested: f32) -> f32 {
    if requested.is_finite() && requested > 0.0 {
        requested.clamp(1.0, 8.0)
    } else {
        1.0
    }
}

#[derive(Component)]
struct BatchMainState {
    output: Handle<Image>,
    fonts: FontUse,
    /// The failure last logged, so a repeated one is not logged again.
    last_failure: Option<ShapingFailure>,
    last_config: Option<TerminalRenderConfig>,
    /// The configured theme, resolved once per configuration change.
    palette: Palette,
    measure: FontMeasurement,
    /// What the last scene drew.
    retained: Retained,
    submissions: Submissions,
    shapes: ShapeCaches,
    glyph_atlas: UnifiedGlyphAtlas,
    scratch: SceneScratch,
    /// The rows the next scene repaints, reused between scenes.
    repaint_rows: Vec<u16>,
    /// The render device (`SceneQueue::device`) the atlas and output were
    /// drawn on.
    device: u64,
}

/// The fonts a terminal uses.
#[derive(Default)]
struct FontUse {
    /// Font asset ids in use, to scope re-measurement to this terminal's own
    /// fonts.
    ids: [Option<AssetId<Font>>; 4],
    /// Whether every handle font above is registered with the font context.
    ready: bool,
}

/// What measuring a terminal's fonts produced.
#[derive(Default)]
struct FontMeasurement {
    /// Advance of the regular font at the probe size; `None` until measured.
    advance: Option<f32>,
    /// The metrics in use, which change together; `None` until measured and
    /// after invalidation.
    in_use: Option<MetricsInUse>,
}

#[derive(Clone, Copy)]
struct MetricsInUse {
    /// Logical font and cell size.
    logical: LogicalMetrics,
    /// The physical raster metrics refined from `logical` at `scale`.
    raster: RasterMetrics,
    scale: f32,
}

/// What a terminal's last scene drew.
#[derive(Default)]
struct Retained {
    /// The grid state drawn; `None` when the next scene must repaint
    /// everything.
    drawn: Option<Drawn>,
    /// How far each drawn row's ink reaches, and which rows blink.
    rows: RowStates,
    /// The blink phases drawn.
    blink: BlinkPhases,
}

/// The grid state a scene drew.
#[derive(Clone, Copy)]
struct Drawn {
    grid: u64,
    revision: u64,
    size: GridSize,
    cursor: CellPosition,
    /// Whether the cursor was shown.
    cursor_shown: bool,
}

impl Drawn {
    fn of(grid: &TerminalGrid) -> Self {
        Self {
            grid: grid.identity(),
            revision: grid.revision(),
            size: grid.size(),
            cursor: grid.cursor_position(),
            cursor_shown: cursor_should_be_visible(grid),
        }
    }
}

/// Scene numbering shared with the render world, which stores a scene's
/// generation in `acknowledged` once it has drawn it.
#[derive(Default)]
struct Submissions {
    generation: u64,
    acknowledged: Arc<AtomicU64>,
}

impl Submissions {
    /// Numbers the next scene.
    fn next(&mut self) -> (Arc<AtomicU64>, u64) {
        self.generation = self.generation.wrapping_add(1);
        (self.acknowledged.clone(), self.generation)
    }

    /// Whether the render world has drawn every submitted scene.
    fn all_drawn(&self) -> bool {
        self.acknowledged.load(Ordering::Acquire) == self.generation
    }

    /// Acknowledges every submitted scene, as drawing them would.
    #[cfg(test)]
    fn acknowledge(&self) {
        self.acknowledged.store(self.generation, Ordering::Release);
    }
}

impl BatchMainState {
    /// Withdraws the terminal's scenes that have not drawn (keeping the atlas
    /// entries they carried) and has the next sync measure and repaint
    /// everything.
    fn invalidate(&mut self, queue: &mut SceneQueue) {
        queue.discard(self.output.id(), &mut self.glyph_atlas);
        self.retained.drawn = None;
        self.measure.in_use = None;
    }

    fn new(output: Handle<Image>) -> Self {
        Self {
            output,
            fonts: FontUse::default(),
            last_failure: None,
            last_config: None,
            palette: Palette::default(),
            measure: FontMeasurement::default(),
            retained: Retained::default(),
            submissions: Submissions::default(),
            shapes: ShapeCaches::default(),
            glyph_atlas: UnifiedGlyphAtlas::default(),
            scratch: SceneScratch::default(),
            repaint_rows: Vec::new(),
            device: 0,
        }
    }

    /// The grid revision the last scene drew.
    #[cfg(test)]
    fn drawn_revision(&self) -> Option<u64> {
        self.retained.drawn.map(|drawn| drawn.revision)
    }

    /// The raster metrics in use.
    #[cfg(test)]
    fn raster(&self) -> RasterMetrics {
        self.measure.in_use.expect("measured metrics").raster
    }

    /// The retained rows' ink reach and blinking.
    #[cfg(test)]
    fn rows(&self) -> &RowStates {
        &self.retained.rows
    }
}

#[derive(Clone, Copy)]
struct DrawBatch {
    texture: AssetId<Image>,
    start: u32,
    count: u32,
    blend: Blend,
}

/// How a draw batch writes its target.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Blend {
    /// Alpha-blend over the target.
    Alpha,
    /// Replace the destination. Used for cell backgrounds so a translucent
    /// theme background does not accumulate over stale texels when only some
    /// rows are repainted.
    Replace,
}

struct BatchScene {
    submission: Option<(Arc<AtomicU64>, u64)>,
    destination: AssetId<Image>,
    /// The terminal's atlas texture, and the entries to write to it first.
    atlas: AssetId<Image>,
    atlas_uploads: Vec<AtlasUpload>,
    /// Whether `atlas_uploads` holds every entry of the atlas.
    atlas_fresh: bool,
    destination_size: UVec2,
    instances: Vec<QuadInstance>,
    batches: Vec<DrawBatch>,
    clear: bool,
    clear_color: Color,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct BlinkPhases {
    slow_hidden: bool,
    rapid_hidden: bool,
    cursor_hidden: bool,
}

impl BlinkPhases {
    fn at(elapsed: f64, config: &TerminalRenderConfig) -> Self {
        Self {
            slow_hidden: super::blink_hidden(elapsed, config.blink.slow_hz),
            rapid_hidden: super::blink_hidden(elapsed, config.blink.rapid_hz),
            cursor_hidden: super::blink_hidden(elapsed, config.cursor.blink_hz),
        }
    }

    fn hides(self, style: &ResolvedStyle) -> bool {
        (self.rapid_hidden && style.any(StyleFlags::RAPID_BLINK))
            || (self.slow_hidden && style.any(StyleFlags::SLOW_BLINK))
    }
}

/// Scenes the main world has built and the render world has not taken yet,
/// at most one per terminal output, and what the render world must
/// withdraw or free. Nothing is scanned per frame: sync submits scenes,
/// suspension withdraws them, removal releases a terminal's resources and
/// extraction drains the queue.
#[derive(Resource, Default)]
struct SceneQueue {
    scenes: HashMap<AssetId<Image>, BatchScene>,
    /// Outputs whose extracted scenes must no longer draw.
    withdrawn: HashSet<AssetId<Image>>,
    /// Outputs and atlases of removed terminals.
    released: Vec<(AssetId<Image>, AssetId<Image>)>,
    /// Whether a render world takes scenes. Until one does, it holds no
    /// scene or atlas to withdraw or free.
    extracting: bool,
    /// Counts the render devices recreated (after a device loss), which
    /// start with blank terminal textures and no atlases. Shared with the
    /// render world.
    device: Arc<AtomicU64>,
}

impl SceneQueue {
    /// Queues `scene`, which carries the atlas entries of the one it
    /// supersedes.
    fn submit(&mut self, mut scene: BatchScene) {
        if let Some(superseded) = self.scenes.remove(&scene.destination) {
            superseded.carry_into(&mut scene.atlas_uploads, &mut scene.atlas_fresh);
        }
        self.scenes.insert(scene.destination, scene);
    }

    /// Drops `output`'s queued scene, handing the atlas entries it carried
    /// back to `atlas` for the next scene, and has the render world withdraw
    /// any scene of it that has not drawn.
    fn discard(&mut self, output: AssetId<Image>, atlas: &mut UnifiedGlyphAtlas) {
        if let Some(scene) = self.scenes.remove(&output) {
            scene.carry_into(&mut atlas.uploads, &mut atlas.fresh);
        }
        if self.extracting {
            self.withdrawn.insert(output);
        }
    }

    /// Drops a removed terminal's scene and has the render world free its
    /// scene and atlas.
    fn release(&mut self, output: AssetId<Image>, atlas: AssetId<Image>) {
        self.scenes.remove(&output);
        if self.extracting {
            self.released.push((output, atlas));
        }
    }

    #[cfg(test)]
    fn scene(&self, output: AssetId<Image>) -> Option<&BatchScene> {
        self.scenes.get(&output)
    }
}

/// The terminal's scene waiting for extraction.
#[cfg(test)]
fn queued(world: &World, entity: Entity) -> Option<&BatchScene> {
    let output = world.get::<BatchMainState>(entity)?.output.id();
    world.resource::<SceneQueue>().scene(output)
}

/// Takes the terminal's queued scene and acknowledges it, as extraction
/// and the GPU would.
#[cfg(test)]
fn take_queued(world: &mut World, entity: Entity) -> Option<BatchScene> {
    let output = world.get::<BatchMainState>(entity)?.output.id();
    let scene = world.resource_mut::<SceneQueue>().scenes.remove(&output)?;
    let state = world.get::<BatchMainState>(entity)?;
    state.submissions.acknowledge();
    Some(scene)
}

/// Scenes the render world has taken and not drawn yet, per output.
#[derive(Resource, Default)]
struct PendingBatchScenes {
    scenes: HashMap<AssetId<Image>, BatchScene>,
    /// Atlases of removed terminals, freed before the next draw.
    released_atlases: Vec<AssetId<Image>>,
}

impl BatchScene {
    /// Prepends the atlas entries of this superseded scene, which never
    /// drew, to the newer `uploads` (a newer scene's, or the atlas's own), so
    /// no entry the newer one relies on is lost.
    ///
    /// Fresh newer uploads already carry every entry, and the superseded ones
    /// predate the atlas's last clear, so they are dropped. This keeps queued
    /// uploads within one atlas's worth when nothing extracts scenes.
    fn carry_into(self, uploads: &mut Vec<AtlasUpload>, fresh: &mut bool) {
        if !*fresh {
            let mut older = self.atlas_uploads;
            older.append(uploads);
            *uploads = older;
            *fresh = self.atlas_fresh;
        }
    }

    /// Keeps the atlas entries of a scene that must not draw: nothing is
    /// drawn, cleared or acknowledged, and a later scene absorbs it.
    fn withdraw(&mut self) {
        self.instances.clear();
        self.batches.clear();
        self.clear = false;
        self.submission = None;
    }
}

/// Registration changes only when font assets change. Bevy assigns aliases
/// through `Assets<Font>` during registration, so change detection also covers
/// the update after an asset arrives but before its family is usable.
#[derive(Default, Resource)]
struct FontCatalog {
    registered: HashSet<AssetId<Font>>,
    /// Font assets with events this update.
    changed: Vec<AssetId<Font>>,
    /// Whether the registered set changed this update.
    registration_changed: bool,
    initialized: bool,
    #[cfg(test)]
    scans: usize,
}

impl FontCatalog {
    /// Rescans the registered fonts on the first update and after font
    /// assets change; says whether they changed.
    fn refresh(&mut self, fonts: &Res<Assets<Font>>, font_cx: &mut ResMut<FontCx>) -> bool {
        if self.initialized && !fonts.is_changed() {
            return false;
        }
        self.initialized = true;
        #[cfg(test)]
        {
            self.scans += 1;
        }
        let registered = fonts
            .iter()
            .filter(|(_, font)| {
                !font.alias.is_empty() && font_cx.collection.family_by_name(&font.alias).is_some()
            })
            .map(|(id, _)| id)
            .collect();
        let changed = self.registered != registered;
        self.registered = registered;
        changed
    }
}

/// Records this update's font asset events and registration changes, once
/// for every source's sync.
fn refresh_font_catalog(
    mut catalog: ResMut<FontCatalog>,
    events: Option<MessageReader<AssetEvent<Font>>>,
    fonts: Option<Res<Assets<Font>>>,
    font_cx: Option<ResMut<FontCx>>,
) {
    let catalog = &mut *catalog;
    // Retain event storage and rebuild registration only after asset changes.
    catalog.changed.clear();
    if let Some(mut events) = events {
        catalog
            .changed
            .extend(events.read().map(|event| match *event {
                AssetEvent::Added { id }
                | AssetEvent::Modified { id }
                | AssetEvent::Removed { id }
                | AssetEvent::Unused { id }
                | AssetEvent::LoadedWithDependencies { id } => id,
            }));
    }
    catalog.registration_changed = match (fonts, font_cx) {
        (Some(fonts), Some(mut font_cx)) => catalog.refresh(&fonts, &mut font_cx),
        _ => {
            catalog.initialized = false;
            false
        }
    };
}

/// The world state the sync reads besides the terminals: Bevy's text,
/// image, font, time and device resources, and the renderer's own.
#[derive(bevy::ecs::system::SystemParam)]
struct SyncResources<'w> {
    text: Option<TextResources<'w>>,
    time: Option<Res<'w, Time>>,
    asset_server: Option<Res<'w, AssetServer>>,
    device: Option<Res<'w, RenderDevice>>,
    catalog: Res<'w, FontCatalog>,
    queue: ResMut<'w, SceneQueue>,
}

fn sync_terminals<S: TerminalSource + Component>(
    mut terminals: Query<TerminalQuery<S>>,
    resources: SyncResources,
) {
    if terminals.is_empty() {
        return;
    }
    resources.run(|frame| {
        for (entity, source, config, state, output, stats) in &mut terminals {
            let terminal = (entity, config, state, output, stats);
            sync_entity(terminal, source.grid(), source.is_changed(), frame);
        }
    });
}

fn sync_resource_terminals<R: TerminalSource + Resource>(
    mut terminals: Query<RendererQuery, With<ResourceTerminal<R>>>,
    source: Option<Res<R>>,
    resources: SyncResources,
) {
    let Some(source) = source.filter(|_| !terminals.is_empty()) else {
        return;
    };
    resources.run(|frame| {
        for terminal in &mut terminals {
            sync_entity(terminal, source.grid(), source.is_changed(), frame);
        }
    });
}

/// What every terminal's sync shares in one update.
struct Frame<'a, 'w> {
    text: Option<&'a mut TextResources<'w>>,
    asset_server: Option<&'a AssetServer>,
    catalog: &'a FontCatalog,
    queue: &'a mut SceneQueue,
    /// Seconds since startup, for blink phases.
    elapsed: f64,
    texture_limit: u32,
    /// The render device generation (`SceneQueue::device`).
    device: u64,
}

impl SyncResources<'_> {
    fn run(self, sync: impl FnOnce(&mut Frame<'_, '_>)) {
        let SyncResources {
            mut text,
            time,
            asset_server,
            device,
            catalog,
            mut queue,
        } = self;
        if text.is_none() {
            warn_once!(
                "bevy_terminal: Bevy's text resources are missing (add DefaultPlugins or \
                 TextPlugin); terminals will not render"
            );
        }
        let generation = queue.device.load(Ordering::Acquire);
        sync(&mut Frame {
            text: text.as_mut(),
            asset_server: asset_server.as_deref(),
            catalog: &catalog,
            queue: &mut queue,
            elapsed: time.as_ref().map_or(0.0, |time| time.elapsed_secs_f64()),
            texture_limit: texture_limit(device.as_deref()),
            device: generation,
        });
    }
}

/// Brings one terminal drawing `grid` up to date, and writes its status
/// and statistics.
fn sync_entity(
    (entity, config, mut state, mut output, mut stats): RendererItem<'_>,
    grid: &TerminalGrid,
    source_changed: bool,
    frame: &mut Frame<'_, '_>,
) {
    let Frame {
        text,
        asset_server,
        catalog,
        queue,
        elapsed,
        texture_limit,
        device,
    } = frame;
    let (elapsed, texture_limit, device) = (*elapsed, *texture_limit, *device);
    let device_changed = state.device != device;
    if device_changed {
        // A recreated render device starts with a blank texture and no
        // atlas: rebuild and repaint everything.
        state.device = device;
        state.invalidate(queue);
    }
    let mut next_stats = TerminalStats::default();
    let result = 'sync: {
        let Some(text) = text.as_mut() else {
            break 'sync Err(TerminalFailure::MissingTextResources.into());
        };
        if !config.sizing.is_valid() {
            break 'sync Err(TerminalFailure::InvalidSizing.into());
        }
        // Inspect effective values only when the component was touched. Invalid
        // scale/blink inputs have documented fallbacks and must compare as such.
        let config_touched = config.is_changed();
        let mut config_changed = false;
        let mut shaping_changed = false;
        if config_touched || state.last_config.is_none() {
            let mut effective = (*config).clone();
            effective.raster.scale = resolve_raster_scale(effective.raster.scale);
            for frequency in [
                &mut effective.blink.slow_hz,
                &mut effective.blink.rapid_hz,
                &mut effective.cursor.blink_hz,
            ] {
                *frequency = frequency.filter(|hz| hz.is_finite() && *hz > 0.0);
            }
            config_changed = state.last_config.as_ref() != Some(&effective);
            shaping_changed = state.last_config.as_ref().is_none_or(|previous| {
                previous.font != effective.font
                    || previous.sizing != effective.sizing
                    || previous.raster.hinting != effective.raster.hinting
            });
            if config_changed {
                state.palette = Palette::new(&effective.theme);
                state.last_config = Some(effective);
            }
        }
        let config = &*config;
        let face_ids = font_asset_ids(&config.font);
        // Handle fonts become usable only once Bevy registers them with the font
        // context (assigning an alias); glyphs shaped before that used a fallback
        // family and must be re-shaped afterwards.
        let fonts_ready = face_ids
            .iter()
            .flatten()
            .all(|id| catalog.registered.contains(id));
        let named_faces = std::iter::once(&config.font.regular)
            .chain(config.font.bold.iter())
            .chain(config.font.italic.iter())
            .chain(config.font.bold_italic.iter())
            .any(|face| !matches!(face, FontSource::Handle(_)));
        let fonts_changed = (named_faces
            && (catalog.registration_changed
                || catalog
                    .changed
                    .iter()
                    .any(|id| catalog.registered.contains(id))))
            || state.fonts.ids != face_ids
            || state.fonts.ready != fonts_ready
            || face_ids
                .iter()
                .flatten()
                .any(|id| catalog.changed.contains(id));
        state.fonts = FontUse {
            ids: face_ids,
            ready: fonts_ready,
        };
        if !fonts_ready {
            let failed = face_ids.iter().flatten().any(|id| {
                let invalid = text
                    .fonts
                    .get(*id)
                    .is_some_and(|font| !font.alias.is_empty() && !catalog.registered.contains(id));
                invalid
                    || asset_server.as_ref().is_some_and(|server| {
                        matches!(
                            server.get_load_state(*id),
                            Some(bevy::asset::LoadState::Failed(_))
                        )
                    })
            });
            break 'sync Err(if failed {
                TerminalFailure::FontFailed.into()
            } else {
                SyncError::Loading
            });
        }
        // A terminal that failed to shape or fit stays idle until one of
        // the inputs that decided it changes.
        if matches!(
            output.status,
            TerminalStatus::Failed(
                TerminalFailure::ShapingFailed | TerminalFailure::TextureTooLarge
            )
        ) && !(source_changed || config_touched || fonts_changed || device_changed)
        {
            break 'sync Ok(None);
        }
        sync_terminal(
            grid,
            SyncInput {
                config,
                config_changed,
                shaping_changed,
                fonts_changed,
                raster_scale: resolve_raster_scale(config.raster.scale),
                elapsed,
                texture_limit,
            },
            &mut state,
            &mut next_stats,
            text,
            queue,
        )
    };
    // The one place a terminal's statistics and status are written.
    stats.set_if_neq(next_stats);
    let status = match result {
        Ok(Some(geometry)) => {
            state.last_failure = None;
            TerminalStatus::Ready(geometry)
        }
        Ok(None) => return,
        Err(error) => {
            // Failed partial construction cannot publish provisional geometry.
            let status = match error {
                SyncError::Loading => TerminalStatus::Loading,
                SyncError::Failed(failure) => TerminalStatus::Failed(failure),
                SyncError::Shaping(failure) => {
                    if state.last_failure.as_ref() != Some(&failure) {
                        warn!(?entity, "bevy_terminal: {failure}");
                        state.last_failure = Some(failure);
                    }
                    TerminalStatus::Failed(TerminalFailure::ShapingFailed)
                }
            };
            // Metrics that are only too large stay valid: keeping them lets
            // an idle oversized terminal skip measuring.
            let measured = state
                .measure
                .in_use
                .filter(|_| status == TerminalStatus::Failed(TerminalFailure::TextureTooLarge));
            state.invalidate(queue);
            state.measure.in_use = measured;
            status
        }
    };
    // Written only on change, so `Changed<TerminalTexture>` observers are
    // not woken every frame.
    if output.status != status {
        output.status = status;
    }
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

/// Resolved inputs for one terminal in this update. Separate invalidation
/// causes preserve cached shaping for paint-only changes.
struct SyncInput<'a> {
    config: &'a TerminalRenderConfig,
    config_changed: bool,
    shaping_changed: bool,
    fonts_changed: bool,
    raster_scale: f32,
    /// Seconds since startup, for blink phases.
    elapsed: f64,
    texture_limit: u32,
}

/// Brings one terminal's scene up to date. Returns the geometry of a new
/// scene, whose image now has that size, or `None` when nothing needed
/// drawing. Nothing is published on failure, so partial construction never
/// leaks geometry.
fn sync_terminal(
    grid: &TerminalGrid,
    input: SyncInput<'_>,
    state: &mut BatchMainState,
    stats: &mut TerminalStats,
    text: &mut TextResources<'_>,
    queue: &mut SceneQueue,
) -> Result<Option<TerminalGeometry>, SyncError> {
    let SyncInput {
        config,
        config_changed,
        shaping_changed,
        fonts_changed,
        raster_scale,
        elapsed,
        texture_limit,
    } = input;
    let BatchMainState {
        output,
        palette,
        measure,
        retained,
        submissions,
        shapes,
        glyph_atlas,
        scratch,
        repaint_rows,
        ..
    } = state;
    // An unmeasured cell is not geometry: no scene or measured geometry is
    // published until the selected face shapes.
    if config.sizing.needs_advance()
        && (measure.advance.is_none() || shaping_changed || fonts_changed)
    {
        measure.advance = None;
        let advance = measure_advance(
            &config.font,
            &text.fonts,
            &mut text.text_pipeline,
            &mut text.font_cx,
            &mut text.layout_cx,
        )
        .map_err(|error| ShapingFailure::Advance {
            font: config.font.regular.clone(),
            error,
        })?;
        measure.advance = Some(advance);
    }
    let metrics = resolve_metrics(config, measure.advance);
    let requested = physical_config(metrics, raster_scale);
    if terminal_pixel_size(grid.size(), &requested).max_element() > texture_limit
        || requested.font_size > texture_limit as f32
    {
        return Err(TerminalFailure::TextureTooLarge.into());
    }
    let text_assets_changed = shaping_changed
        || fonts_changed
        || measure
            .in_use
            .is_none_or(|in_use| in_use.logical != metrics || in_use.scale != raster_scale);
    let raster = if let Some(in_use) = measure.in_use.filter(|_| !text_assets_changed) {
        in_use.raster
    } else {
        measure.in_use = None;
        let mut cx = text.context();
        let raster = refine_metrics(config, measure.advance, requested, &mut cx);
        if let Some(failure) = cx.failure {
            return Err(failure.into());
        }
        measure.in_use = Some(MetricsInUse {
            logical: metrics,
            raster,
            scale: raster_scale,
        });
        shapes.clear();
        glyph_atlas.clear();
        raster
    };
    // Refining the cell to the font can grow it past the limit.
    let size = terminal_pixel_size(grid.size(), &raster);
    if size.max_element() > texture_limit {
        return Err(TerminalFailure::TextureTooLarge.into());
    }
    let blink = BlinkPhases::at(elapsed, config);
    // A phase flip only matters where it changes pixels: text phases when the
    // grid holds blinking cells, the cursor phase when the cursor shows.
    let text_blink_changed = (blink.slow_hidden != retained.blink.slow_hidden
        || blink.rapid_hidden != retained.blink.rapid_hidden)
        && retained.rows.blinking().next().is_some();
    let cursor_blink_changed = blink.cursor_hidden != retained.blink.cursor_hidden
        && retained.drawn.is_some_and(|drawn| drawn.cursor_shown);
    if retained.drawn.is_some_and(|drawn| {
        drawn.grid == grid.identity()
            && drawn.revision == grid.revision()
            && !config_changed
            && !text_assets_changed
            && !text_blink_changed
            && !cursor_blink_changed
    }) {
        // Keep the recorded phases current so an irrelevant flip is not
        // mistaken for a change once blinking content appears later.
        retained.blink = blink;
        return Ok(None);
    }

    let now = Drawn::of(grid);
    let mut rows = std::mem::take(repaint_rows);
    rows.clear();
    let mut full = match retained.drawn {
        Some(drawn) if drawn.grid == now.grid && drawn.size == now.size => {
            rows.extend(grid.rows_changed_since(drawn.revision));
            if drawn.cursor != now.cursor || drawn.cursor_shown != now.cursor_shown {
                rows.extend([drawn.cursor.y, now.cursor.y]);
            } else if cursor_blink_changed {
                rows.push(now.cursor.y);
            }
            if text_blink_changed {
                rows.extend(retained.rows.blinking());
            }
            rows.retain(|row| *row < now.size.height);
            rows.sort_unstable();
            rows.dedup();
            config_changed || text_assets_changed
        }
        _ => true,
    };
    if rows.is_empty() && !full {
        retained.drawn = Some(now);
        *repaint_rows = rows;
        return Ok(None);
    }
    // Extraction can be delayed while a newly created output or glyph atlas reaches the render
    // world. If a newer payload is already waiting in the main world, make its replacement a
    // complete image of the newest grid so rows changed by an intermediate payload cannot be
    // lost when that payload is superseded.
    full |= !submissions.all_drawn();

    #[cfg(feature = "timings")]
    let scene_start = Instant::now();
    let input = SceneInput {
        grid,
        config,
        palette,
        raster,
        changed: &rows,
        full,
        destination: output.id(),
        blink,
    };
    let mut cx = text.context();
    let mut scene = build_scene(
        input,
        &mut cx,
        shapes,
        glyph_atlas,
        scratch,
        &mut retained.rows,
        stats,
    );
    if glyph_atlas.overflowed {
        // The atlas filled up: start it afresh with only what this frame
        // shows. Glyphs that still do not fit are drawn from Bevy's atlases.
        debug!("bevy_terminal: glyph atlas full; rebuilding it for the current frame");
        shapes.clear();
        glyph_atlas.clear();
        scene = build_scene(
            SceneInput {
                full: true,
                ..input
            },
            &mut cx,
            shapes,
            glyph_atlas,
            scratch,
            &mut retained.rows,
            stats,
        );
    }
    *repaint_rows = rows;
    if let Some(failure) = cx.failure {
        return Err(failure.into());
    }
    #[cfg(feature = "timings")]
    {
        stats.scene_ns = scene_start.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
    }
    stats.draw_batches = u32::try_from(scene.batches.len()).unwrap_or(u32::MAX);
    scene.submission = Some(submissions.next());
    queue.submit(scene);
    retained.drawn = Some(now);
    retained.blink = blink;
    Ok(Some(publish(
        &mut text.images,
        output,
        TerminalGeometry {
            grid: now.size,
            size,
            raster_scale,
            physical_cell_size: raster.cell_size,
            physical_font_size: raster.font_size,
        },
    )))
}

/// Sizes `output` to `geometry`, the only way geometry is published, so an
/// image and the geometry describing it cannot disagree. The image is
/// reallocated in place so its handle stays stable; the render world
/// recreates the GPU texture.
fn publish(
    images: &mut Assets<Image>,
    output: &Handle<Image>,
    geometry: TerminalGeometry,
) -> TerminalGeometry {
    if images
        .get(output.id())
        .is_none_or(|image| image.size() != geometry.size)
        && images
            .insert(output.id(), make_target_image(geometry.size))
            .is_err()
    {
        warn!("bevy_terminal: could not reallocate a terminal texture in place");
    }
    geometry
}

#[cfg(test)]
mod invariants;
#[cfg(test)]
mod probe;
#[cfg(test)]
mod replay;
#[cfg(test)]
mod tests;
