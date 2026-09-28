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
use scene::{RowStates, SceneScratch, build_scene};
use shaping::{AtlasUpload, ShapeCaches, UnifiedGlyphAtlas};

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
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

use super::terminal::Measurement;
use super::{
    Face, Palette, ResolvedStyle, TerminalGeometry, TerminalRenderConfig, TerminalRenderer,
    TerminalStats, TerminalStatus, TerminalTexture, cell_span, cursor_should_be_visible, text_font,
};
use crate::{
    scene::{GridSize, StyleFlags, TerminalSnapshot},
    surface::{TerminalSurface, WeakSurface},
};

/// The terminal texture format: the shader emits linear colors and the sRGB
/// target encodes them, so the image is display-ready for UI, sprites and 3D
/// materials and dark tones do not band in 8-bit storage.
const TARGET_FORMAT: TextureFormat = TextureFormat::Rgba8UnormSrgb;
const GLYPH_FORMAT: TextureFormat = TextureFormat::Rgba8UnormSrgb;
const GLYPH_ATLAS_SIZE: u32 = 2048;

/// Installs the terminal renderer. Add it once, then spawn [`TerminalRenderer`]
/// entities.
///
/// Glyphs are shaped and rasterized by Bevy text. Each terminal is represented
/// by compact GPU quad instances drawn into its own renderer-owned texture;
/// GPU pipelines and scratch buffers are shared between terminals.
#[derive(Clone, Copy, Debug, Default)]
pub struct TerminalPlugin;

impl Plugin for TerminalPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FontCatalog>()
            .init_resource::<SceneQueue>()
            .add_observer(initialize_terminal)
            .add_observer(release_terminal)
            .add_systems(
                Update,
                sync_batch_terminals.in_set(super::TerminalSystems::Sync),
            );
        if app.get_sub_app(RenderApp).is_none() {
            return;
        }
        load_shader(app);
        app.sub_app_mut(RenderApp)
            .init_resource::<PendingBatchScenes>()
            .add_systems(RenderStartup, init_batch_pipelines)
            .add_systems(ExtractSchedule, extract_batch_scenes)
            // Terminal textures are drawn first, so cameras sample this
            // frame's content.
            .add_systems(
                RenderGraph,
                render_batch_scenes.in_set(RenderGraphSystems::Begin),
            );
    }
}

/// Drops the renderer's components when a terminal stops rendering (its
/// [`TerminalRenderer`] is removed or the entity despawned), and has the
/// render world drop its scene and free its atlas.
fn release_terminal(
    remove: On<Remove, TerminalRenderer>,
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
type TerminalQuery<'w> = (
    Entity,
    &'w TerminalRenderer,
    Ref<'w, TerminalRenderConfig>,
    &'w mut BatchMainState,
    &'w mut TerminalTexture,
    &'w mut TerminalStats,
);

/// Bevy's text resources, required for shaping and measurement.
#[derive(bevy::ecs::system::SystemParam)]
struct TextResources<'w> {
    fonts: Res<'w, Assets<Font>>,
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
    fn context<'a>(&'a mut self, images: &'a mut Assets<Image>) -> TextContext<'a> {
        TextContext {
            failure: None,
            fonts: &self.fonts,
            images,
            text_pipeline: &mut self.text_pipeline,
            font_atlas_set: &mut self.font_atlas_set,
            font_cx: &mut self.font_cx,
            layout_cx: &mut self.layout_cx,
            scale_cx: &mut self.scale_cx,
        }
    }
}

/// Gives a terminal its texture, statistics and renderer state as soon as
/// its [`TerminalRenderer`] is added.
fn initialize_terminal(
    add: On<Add, TerminalRenderer>,
    terminals: Query<(&TerminalRenderer, &TerminalRenderConfig)>,
    mut images: ResMut<Assets<Image>>,
    device: Option<Res<RenderDevice>>,
    mut commands: Commands,
) {
    let limit = texture_limit(device.as_deref());
    let entity = add.entity;
    if let Ok((terminal, config)) = terminals.get(entity) {
        let raster_scale = resolve_raster_scale(config.raster.scale);
        let metrics = if config.sizing.is_valid() {
            resolve_metrics(config, None)
        } else {
            LogicalMetrics {
                cell_size: Vec2::ONE,
                font_size: 16.0,
            }
        };
        let raster_config = physical_config(metrics, raster_scale);
        let requested = terminal_pixel_size(terminal.surface().size(), &raster_config);
        let size = if requested.max_element() <= limit {
            requested
        } else {
            UVec2::ONE
        };
        let output = images.add(make_target_image(size));
        commands.entity(entity).insert((
            TerminalTexture {
                status: TerminalStatus::Loading,
                image: output.clone(),
                geometry: TerminalGeometry {
                    surface: terminal.surface().downgrade(),
                    measurement: Measurement {
                        resize_generation: terminal.surface().info().resize_generation,
                        grid: terminal.surface().size(),
                        size,
                        raster_scale,
                        physical_cell_size: raster_config.cell_size,
                        physical_font_size: raster_config.font_size,
                    },
                },
            },
            BatchMainState::new(output.clone()),
            TerminalStats::default(),
        ));
    }
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
    /// The surface being rendered and what the last scene drew from it;
    /// `None` until the first sync.
    retained: Option<Retained>,
    submissions: Submissions,
    shapes: ShapeCaches,
    glyph_atlas: UnifiedGlyphAtlas,
    scratch: SceneScratch,
    /// The rows the next scene repaints, reused between scenes.
    repaint_rows: Vec<u16>,
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

/// The surface a terminal renders and what its last scene drew from it.
struct Retained {
    surface: TerminalSurface,
    /// The content drawn; `None` when the next scene must repaint everything.
    snapshot: Option<TerminalSnapshot>,
    /// How far each drawn row's ink reaches, and which rows blink.
    rows: RowStates,
    /// The blink phases drawn.
    blink: BlinkPhases,
}

impl Retained {
    fn new(surface: TerminalSurface) -> Self {
        Self {
            surface,
            snapshot: None,
            rows: RowStates::default(),
            blink: BlinkPhases::default(),
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
    /// Withdraws the terminal's scenes that have not drawn, keeping the atlas
    /// entries they carried.
    fn discard_pending(&mut self, queue: &mut SceneQueue) {
        queue.discard(self.output.id(), &mut self.glyph_atlas);
    }

    fn invalidate(&mut self, queue: &mut SceneQueue) {
        self.discard_pending(queue);
        if let Some(retained) = &mut self.retained {
            retained.snapshot = None;
        }
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
            retained: None,
            submissions: Submissions::default(),
            shapes: ShapeCaches::default(),
            glyph_atlas: UnifiedGlyphAtlas::default(),
            scratch: SceneScratch::default(),
            repaint_rows: Vec::new(),
        }
    }

    /// The content the last scene drew.
    #[cfg(test)]
    fn snapshot(&self) -> Option<&TerminalSnapshot> {
        self.retained.as_ref()?.snapshot.as_ref()
    }

    /// The raster metrics in use.
    #[cfg(test)]
    fn raster(&self) -> RasterMetrics {
        self.measure.in_use.expect("measured metrics").raster
    }

    /// The retained rows' ink reach and blinking.
    #[cfg(test)]
    fn rows(&self) -> &RowStates {
        &self.retained.as_ref().expect("a retained surface").rows
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
    /// Raised when the render world lost the atlas this scene relies on.
    atlas_lost: Arc<std::sync::atomic::AtomicBool>,
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
    fn at(elapsed: f32, config: &TerminalRenderConfig) -> Self {
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
}

impl SceneQueue {
    /// Queues `scene`, which carries the atlas entries of the one it
    /// supersedes.
    fn submit(&mut self, mut scene: BatchScene) {
        if let Some(superseded) = self.scenes.remove(&scene.destination) {
            scene.absorb(superseded);
        }
        self.scenes.insert(scene.destination, scene);
    }

    /// Drops `output`'s queued scene, handing the atlas entries it carried
    /// back to `atlas` for the next scene, and has the render world withdraw
    /// any scene of it that has not drawn.
    fn discard(&mut self, output: AssetId<Image>, atlas: &mut UnifiedGlyphAtlas) {
        if let Some(scene) = self.scenes.remove(&output)
            && !atlas.fresh
        {
            let mut uploads = scene.atlas_uploads;
            uploads.append(&mut atlas.uploads);
            atlas.uploads = uploads;
            atlas.fresh = scene.atlas_fresh;
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
    /// Prepends the atlas entries of a superseded scene that never drew, so
    /// no entry the newer scene relies on is lost.
    ///
    /// A fresh scene already carries every entry, and the superseded entries
    /// predate the atlas's last clear, so they are dropped. This keeps queued
    /// uploads within one atlas's worth when nothing extracts scenes.
    fn absorb(&mut self, superseded: BatchScene) {
        if self.atlas_fresh {
            return;
        }
        let mut uploads = superseded.atlas_uploads;
        uploads.append(&mut self.atlas_uploads);
        self.atlas_uploads = uploads;
        self.atlas_fresh = superseded.atlas_fresh;
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
    changed: Vec<AssetId<Font>>,
    initialized: bool,
    #[cfg(test)]
    scans: usize,
}

impl FontCatalog {
    fn refresh(&mut self, fonts: &Assets<Font>, font_cx: &mut FontCx, changed: bool) -> bool {
        if self.initialized && !changed {
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

#[allow(clippy::too_many_arguments)]
fn sync_batch_terminals(
    mut terminals: Query<TerminalQuery>,
    text: Option<TextResources>,
    mut images: ResMut<Assets<Image>>,
    font_events: Option<MessageReader<AssetEvent<Font>>>,
    time: Option<Res<Time>>,
    asset_server: Option<Res<AssetServer>>,
    device: Option<Res<RenderDevice>>,
    mut catalog: ResMut<FontCatalog>,
    mut queue: ResMut<SceneQueue>,
) {
    if terminals.is_empty() {
        catalog.initialized = false;
        return;
    }
    let Some(mut text) = text else {
        catalog.initialized = false;
        warn_once!(
            "bevy_terminal: Bevy's text resources are missing (add DefaultPlugins or TextPlugin); \
             terminals will not render"
        );
        for (_, _, _, mut state, mut output, mut stats) in &mut terminals {
            stats.set_if_neq(TerminalStats::default());
            suspend_terminal(
                &mut state,
                &mut output,
                TerminalStatus::MissingTextResources,
                &mut queue,
            );
        }
        return;
    };
    let elapsed = time.as_ref().map_or(0.0, |time| time.elapsed_secs());
    // Retain event storage and rebuild registration only after asset changes.
    catalog.changed.clear();
    if let Some(mut events) = font_events {
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
    let catalog_changed = catalog.refresh(&text.fonts, &mut text.font_cx, text.fonts.is_changed());
    let registered_fonts = &catalog.registered;
    let changed_fonts = &catalog.changed;
    for (entity, terminal, config, mut state, mut output, mut stats) in &mut terminals {
        stats.set_if_neq(TerminalStats::default());
        if !config.sizing.is_valid() {
            suspend_terminal(
                &mut state,
                &mut output,
                TerminalStatus::InvalidSizing,
                &mut queue,
            );
            continue;
        }
        // Inspect effective values only when the component was touched. Invalid
        // scale/blink inputs have documented fallbacks and must compare as such.
        let mut config_changed = false;
        let mut shaping_changed = false;
        if config.is_changed() || state.last_config.is_none() {
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
            .all(|id| registered_fonts.contains(id));
        let named_faces = std::iter::once(&config.font.regular)
            .chain(config.font.bold.iter())
            .chain(config.font.italic.iter())
            .chain(config.font.bold_italic.iter())
            .any(|face| !matches!(face, FontSource::Handle(_)));
        let fonts_changed = (named_faces
            && (catalog_changed || changed_fonts.iter().any(|id| registered_fonts.contains(id))))
            || state.fonts.ids != face_ids
            || state.fonts.ready != fonts_ready
            || (!changed_fonts.is_empty()
                && face_ids
                    .iter()
                    .flatten()
                    .any(|id| changed_fonts.contains(id)));
        state.fonts = FontUse {
            ids: face_ids,
            ready: fonts_ready,
        };
        if !fonts_ready {
            let failed = face_ids.iter().flatten().any(|id| {
                let invalid = text
                    .fonts
                    .get(*id)
                    .is_some_and(|font| !font.alias.is_empty() && !registered_fonts.contains(id));
                invalid
                    || asset_server.as_ref().is_some_and(|server| {
                        matches!(
                            server.get_load_state(*id),
                            Some(bevy::asset::LoadState::Failed(_))
                        )
                    })
            });
            let status = if failed {
                TerminalStatus::FontFailed
            } else {
                TerminalStatus::Loading
            };
            suspend_terminal(&mut state, &mut output, status, &mut queue);
            continue;
        }
        // Plain values only: the texture's handles are not cloned each frame.
        let mut measurement = output.geometry.measurement;
        let mut next_stats = TerminalStats::default();
        let mut context = text.context(&mut images);
        let result = sync_batch_terminal(
            terminal,
            SyncInput {
                config,
                measured_surface: &output.geometry.surface,
                config_changed,
                shaping_changed,
                fonts_changed,
                raster_scale: resolve_raster_scale(config.raster.scale),
                elapsed,
                texture_limit: texture_limit(device.as_deref()),
            },
            &mut state,
            &mut measurement,
            &mut next_stats,
            &mut context,
            &mut queue,
        );
        match result {
            Ok(surface_changed) => {
                state.last_failure = None;
                // Write the texture component only when something changed so
                // `Changed<TerminalTexture>` observers are not woken every frame.
                if output.status != TerminalStatus::Ready
                    || output.geometry.measurement != measurement
                    || surface_changed
                {
                    let output = &mut *output;
                    output.status = TerminalStatus::Ready;
                    output.geometry.measurement = measurement;
                    if surface_changed {
                        output.geometry.surface = terminal.surface().downgrade();
                    }
                }
            }
            Err(status) => {
                if let Some(failure) = context.failure.take()
                    && state.last_failure.as_ref() != Some(&failure)
                {
                    warn!(?entity, "bevy_terminal: {failure}");
                    state.last_failure = Some(failure);
                }
                suspend_terminal(&mut state, &mut output, status, &mut queue);
                // Failed partial construction cannot publish provisional geometry.
                stats.set_if_neq(next_stats);
                continue;
            }
        };
        stats.set_if_neq(next_stats);
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

fn needs_measured_advance(config: &TerminalRenderConfig) -> bool {
    config.sizing.needs_advance()
}

fn suspend_terminal(
    state: &mut BatchMainState,
    output: &mut Mut<'_, TerminalTexture>,
    status: TerminalStatus,
    queue: &mut SceneQueue,
) {
    state.invalidate(queue);
    if output.status != status {
        output.status = status;
    }
}

/// Resolved inputs for one terminal in this update. Separate invalidation
/// causes preserve cached shaping for paint-only changes.
struct SyncInput<'a> {
    config: &'a TerminalRenderConfig,
    /// The surface the published geometry refers to.
    measured_surface: &'a WeakSurface,
    config_changed: bool,
    shaping_changed: bool,
    fonts_changed: bool,
    raster_scale: f32,
    elapsed: f32,
    texture_limit: u32,
}

/// Brings one terminal's scene up to date. Measured values are written to
/// `measurement`; the result says whether the geometry must now refer to
/// the renderer's surface instead of the measured one. Nothing is
/// published on failure, so partial construction never leaks geometry.
fn sync_batch_terminal(
    terminal: &TerminalRenderer,
    input: SyncInput<'_>,
    state: &mut BatchMainState,
    measurement: &mut Measurement,
    stats: &mut TerminalStats,
    cx: &mut TextContext<'_>,
    queue: &mut SceneQueue,
) -> Result<bool, TerminalStatus> {
    let SyncInput {
        config,
        measured_surface,
        config_changed,
        shaping_changed,
        fonts_changed,
        raster_scale,
        elapsed,
        texture_limit,
    } = input;
    let surface = terminal.surface();
    if state
        .retained
        .as_ref()
        .is_none_or(|retained| !retained.surface.shares_state_with(surface))
    {
        state.retained = Some(Retained::new(surface.clone()));
        state.discard_pending(queue);
    }
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
    let retained = retained.as_mut().expect("the surface was just retained");
    if glyph_atlas.lost.swap(false, Ordering::AcqRel) {
        // The render world recreated the atlas texture (a device reset):
        // rebuild every entry and repaint everything.
        shapes.clear();
        glyph_atlas.clear();
        retained.snapshot = None;
    }
    let needs_measured_advance = needs_measured_advance(config);
    if needs_measured_advance && (measure.advance.is_none() || shaping_changed || fonts_changed) {
        measure.advance = match measure_advance(
            &config.font,
            cx.fonts,
            cx.text_pipeline,
            cx.font_cx,
            cx.layout_cx,
        ) {
            Ok(advance) => Some(advance),
            Err(error) => {
                cx.failure = Some(ShapingFailure::Advance {
                    font: config.font.regular.clone(),
                    error,
                });
                None
            }
        };
    }
    // An unmeasured cell is not geometry. Keep the provisional component and
    // do not publish a scene or measured geometry until the selected face shapes.
    if needs_measured_advance && measure.advance.is_none() {
        return Err(TerminalStatus::ShapingFailed);
    }
    let metrics = resolve_metrics(config, measure.advance);
    let requested = physical_config(metrics, raster_scale);
    if terminal_pixel_size(surface.size(), &requested).max_element() > texture_limit
        || requested.font_size > texture_limit as f32
    {
        return Err(TerminalStatus::TextureTooLarge);
    }
    let text_assets_changed = shaping_changed
        || fonts_changed
        || measure
            .in_use
            .is_none_or(|in_use| in_use.logical != metrics || in_use.scale != raster_scale);
    if text_assets_changed {
        measure.in_use = None;
        let raster = refine_metrics(config, measure.advance, requested, cx);
        if cx.failure.is_some() {
            return Err(TerminalStatus::ShapingFailed);
        }
        measure.in_use = Some(MetricsInUse {
            logical: metrics,
            raster,
            scale: raster_scale,
        });
        measurement.physical_font_size = raster.font_size;
        measurement.physical_cell_size = raster.cell_size;
        shapes.clear();
        glyph_atlas.clear();
    }
    let raster = measure.in_use.expect("metrics were measured above").raster;
    let blink = BlinkPhases::at(elapsed, config);
    // A phase flip only matters where it changes pixels: text phases when the
    // snapshot holds blinking cells, the cursor phase when the cursor shows.
    let text_blink_changed = retained.rows.any_blinking()
        && (blink.slow_hidden != retained.blink.slow_hidden
            || blink.rapid_hidden != retained.blink.rapid_hidden);
    let cursor_blink_changed = blink.cursor_hidden != retained.blink.cursor_hidden
        && retained
            .snapshot
            .as_ref()
            .is_some_and(cursor_should_be_visible);
    let blink_changed = text_blink_changed || cursor_blink_changed;
    if retained.snapshot.as_ref().is_some_and(|snapshot| {
        snapshot.revision() == surface.revision()
            && !config_changed
            && !text_assets_changed
            && !blink_changed
    }) {
        // Keep the recorded phases current so an irrelevant flip is not
        // mistaken for a change once blinking content appears later.
        retained.blink = blink;
        return Ok(false);
    }

    #[cfg(feature = "timings")]
    let snapshot_start = Instant::now();
    let mut rows = std::mem::take(repaint_rows);
    let (snapshot, mut full) = if let Some(mut snapshot) = retained.snapshot.take() {
        let old_cursor = snapshot.cursor_position();
        let update = surface.update_snapshot(&mut snapshot, &mut rows);
        stats.snapshot_cells = u32::try_from(update.changed_cells).unwrap_or(u32::MAX);
        if update.cursor_position_changed || update.cursor_visibility_changed {
            rows.push(old_cursor.y);
            rows.push(snapshot.cursor_position().y);
            rows.sort_unstable();
            rows.dedup();
            rows.retain(|row| *row < snapshot.size().height);
        }
        let full = update.resized || config_changed || text_assets_changed;
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
        (snapshot, full)
    } else {
        let snapshot = surface.snapshot();
        stats.snapshot_cells = u32::try_from(snapshot.cells().len()).unwrap_or(u32::MAX);
        rows.clear();
        rows.extend(0..snapshot.size().height);
        (snapshot, true)
    };

    #[cfg(feature = "timings")]
    {
        stats.snapshot_ns = snapshot_start
            .elapsed()
            .as_nanos()
            .min(u128::from(u64::MAX)) as u64;
    }
    if rows.is_empty() && !full && !blink_changed {
        retained.snapshot = Some(snapshot);
        *repaint_rows = rows;
        return Ok(false);
    }
    // Extraction can be delayed while a newly created output or glyph atlas reaches the render
    // world. If a newer payload is already waiting in the main world, make its replacement a
    // complete image of the newest snapshot so rows changed by an intermediate payload cannot be
    // lost when that payload is superseded.
    full |= !submissions.all_drawn();

    let new_size = terminal_pixel_size(snapshot.size(), &raster);
    if new_size.max_element() > texture_limit {
        return Err(TerminalStatus::TextureTooLarge);
    }
    let surface_changed = !measured_surface.matches(surface);
    measurement.grid = snapshot.size();
    measurement.resize_generation = snapshot.resize_generation;
    if measurement.size != new_size {
        // Reallocate the image in place so the handle stays stable; the render world
        // recreates the GPU texture for the modified asset.
        if cx
            .images
            .insert(output.id(), make_target_image(new_size))
            .is_err()
        {
            warn!("bevy_terminal: could not reallocate a terminal texture in place");
        }
        measurement.size = new_size;
    }
    measurement.raster_scale = raster_scale;

    if full {
        rows.clear();
        rows.extend(0..snapshot.size().height);
    }
    #[cfg(feature = "timings")]
    let scene_start = Instant::now();
    let destination = output.id();
    let mut scene = build_scene(
        &snapshot,
        config,
        palette,
        raster,
        &rows,
        full,
        destination,
        cx,
        shapes,
        glyph_atlas,
        scratch,
        &mut retained.rows,
        stats,
        blink,
    );
    if glyph_atlas.overflowed {
        // The atlas filled up: start it afresh with only what this frame
        // shows. Glyphs that still do not fit are drawn from Bevy's atlases.
        debug!("bevy_terminal: glyph atlas full; rebuilding it for the current frame");
        shapes.clear();
        glyph_atlas.clear();
        rows.clear();
        rows.extend(0..snapshot.size().height);
        scene = build_scene(
            &snapshot,
            config,
            palette,
            raster,
            &rows,
            true,
            destination,
            cx,
            shapes,
            glyph_atlas,
            scratch,
            &mut retained.rows,
            stats,
            blink,
        );
    }
    *repaint_rows = rows;
    if cx.failure.is_some() {
        return Err(TerminalStatus::ShapingFailed);
    }
    #[cfg(feature = "timings")]
    {
        stats.scene_ns = scene_start.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
    }
    stats.draw_batches = u32::try_from(scene.batches.len()).unwrap_or(u32::MAX);
    scene.submission = Some(submissions.next());
    queue.submit(scene);
    retained.snapshot = Some(snapshot);
    retained.blink = blink;
    Ok(surface_changed)
}

#[cfg(test)]
mod baseline;
#[cfg(test)]
mod invariants;
#[cfg(test)]
mod probe;
#[cfg(test)]
mod replay;
#[cfg(test)]
mod tests;
