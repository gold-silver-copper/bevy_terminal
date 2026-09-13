//! Retained-state synchronization and render scheduling.

mod gpu;
pub(super) mod metrics;
mod scene;
mod shaping;
use gpu::{
    BatchGpuState, batch_scenes_can_render_early, extract_batch_scenes, render_batch_scenes,
    reset_batch_gpu_state,
};
use metrics::{
    LogicalMetrics, RasterMetrics, measure_advance, physical_config, refine_metrics,
    resolve_metrics,
};
use scene::{SceneScratch, build_scene};
use shaping::{ShapeCaches, UnifiedGlyphAtlas};

use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

use bevy::{
    asset::{AssetId, RenderAssetUsages},
    image::ImageSampler,
    platform::collections::{HashMap, HashSet},
    prelude::*,
    render::{
        ExtractSchedule, Render, RenderApp, RenderStartup, RenderSystems,
        render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages},
        renderer::RenderDevice,
    },
    text::{FontAtlasSet, FontCx, LayoutCx, ScaleCx, TextPipeline},
    window::PrimaryWindow,
};

use super::{
    PixelGeometry, ResolvedStyle, TerminalGeometry, TerminalReady, TerminalRemeasured,
    TerminalRenderConfig, TerminalRenderScale, TerminalRenderer, TerminalStats, TerminalStatus,
    TerminalTexture, cell_span, cursor_should_be_visible, text_font,
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
        #[cfg(feature = "3d")]
        app.add_plugins(super::world_quad::plugin);
        app.init_resource::<FontCatalog>().add_systems(
            Update,
            (
                release_removed_terminals.before(initialize_terminals),
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

fn release_removed_terminals(
    mut commands: Commands,
    removed: Query<Entity, (With<BatchMainState>, Without<TerminalRenderer>)>,
) {
    for entity in &removed {
        commands
            .entity(entity)
            .remove::<(BatchMainState, TerminalTexture, TerminalStats)>();
    }
}

/// Everything the sync system touches on a terminal entity.
type TerminalQuery<'w> = (
    Entity,
    &'w TerminalRenderer,
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
#[derive(bevy::ecs::system::SystemParam)]
struct TextResources<'w> {
    fonts: Res<'w, Assets<Font>>,
    text_pipeline: ResMut<'w, TextPipeline>,
    font_atlas_set: ResMut<'w, FontAtlasSet>,
    font_cx: ResMut<'w, FontCx>,
    layout_cx: ResMut<'w, LayoutCx>,
    scale_cx: ResMut<'w, ScaleCx>,
}

struct TextContext<'a> {
    failure: Option<String>,
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

fn initialize_terminals(
    mut commands: Commands,
    added: Query<
        (Entity, &TerminalRenderer, &TerminalRenderConfig, Presented),
        Without<BatchMainState>,
    >,
    mut images: ResMut<Assets<Image>>,
    device: Option<Res<RenderDevice>>,
) {
    let limit = texture_limit(device.as_deref());
    for (entity, terminal, config, presented) in &added {
        let raster_scale = resolve_raster_scale(config.raster.scale, is_presented(presented), None);
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
        let glyph_atlas = images.add(make_glyph_atlas_image());
        commands.entity(entity).insert((
            TerminalTexture {
                status: TerminalStatus::Loading,
                image: output.clone(),
                geometry: TerminalGeometry {
                    surface: terminal.surface().downgrade(),
                    resize_generation: terminal.surface().info().resize_generation,
                    grid: terminal.surface().size(),
                    size,
                    raster_scale,
                    physical_cell_size: raster_config.cell_size,
                    physical_font_size: raster_config.font_size,
                },
            },
            BatchMainState::new(output.clone(), glyph_atlas, raster_scale, raster_config),
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

// Solid-only and loading terminals need a binding, not a 16 MiB glyph atlas.
// The stable handle grows to the fixed atlas size on the first cached glyph.
fn make_glyph_atlas_image() -> Image {
    let mut image = Image::new_fill(
        Extent3d {
            width: 1,
            height: 1,
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
    node: &mut Mut<'_, Node>,
    image_node: &mut Mut<'_, ImageNode>,
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
fn present(ui: UiNodeItem<'_>, image: &Handle<Image>, size: UVec2, raster_scale: f32) {
    if let Some((mut node, mut image_node)) = ui {
        apply_ui_node(&mut node, &mut image_node, image, size, raster_scale);
    }
}
#[cfg(not(feature = "ui"))]
fn present((): UiNodeItem<'_>, _: &Handle<Image>, _: UVec2, _: f32) {}

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

#[derive(Component)]
struct BatchMainState {
    output: Handle<Image>,
    /// Font asset ids in use, to scope re-measurement to this terminal's own fonts.
    font_ids: [Option<AssetId<Font>>; 4],
    /// Whether every handle font above is registered with the font context.
    fonts_ready: bool,
    /// Whether [`TerminalReady`] has been triggered for this terminal.
    ready_sent: bool,
    last_failure: Option<String>,
    raster_scale: f32,
    raster_config: RasterMetrics,
    /// Advance of the regular font at the probe size; `None` until measured.
    measured_advance: Option<f32>,
    /// Logical font size in use.
    metrics: Option<LogicalMetrics>,
    last_config: Option<TerminalRenderConfig>,
    surface: Option<TerminalSurface>,
    last_snapshot: Option<TerminalSnapshot>,
    /// Whether the retained snapshot holds any `SLOW_BLINK`/`RAPID_BLINK` cells.
    snapshot_blinks: bool,
    pending: Option<BatchScene>,
    generation: u64,
    submitted: Arc<AtomicU64>,
    shapes: ShapeCaches,
    glyph_atlas: UnifiedGlyphAtlas,
    scratch: SceneScratch,
    blink: BlinkPhases,
}

impl BatchMainState {
    fn invalidate(&mut self) {
        self.pending = None;
        self.last_snapshot = None;
        self.metrics = None;
    }

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
            last_failure: None,
            raster_scale,
            raster_config,
            measured_advance: None,
            metrics: None,
            last_config: None,
            surface: None,
            last_snapshot: None,
            snapshot_blinks: false,
            pending: None,
            generation: 0,
            submitted: Arc::default(),
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
    submission: Option<(Arc<AtomicU64>, u64)>,
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
struct PendingBatchScenes {
    scenes: HashMap<AssetId<Image>, BatchScene>,
    live_textures: HashSet<AssetId<Image>>,
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
    mut commands: Commands,
    mut terminals: Query<TerminalQuery>,
    text: Option<TextResources>,
    mut images: ResMut<Assets<Image>>,
    font_events: Option<MessageReader<AssetEvent<Font>>>,
    primary_window: Query<&Window, With<PrimaryWindow>>,
    ui_scale: UiScaleRes,
    time: Option<Res<Time>>,
    asset_server: Option<Res<AssetServer>>,
    device: Option<Res<RenderDevice>>,
    mut catalog: ResMut<FontCatalog>,
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
        for (_, _, _, mut state, mut output, mut stats, _) in &mut terminals {
            stats.set_if_neq(TerminalStats::default());
            suspend_terminal(
                &mut state,
                &mut output,
                TerminalStatus::MissingTextResources,
            );
        }
        return;
    };
    let ui_scale = ui_scale_value(&ui_scale);
    let window_scale = primary_window
        .iter()
        .next()
        .map(|window| window.scale_factor() * ui_scale);
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
    for (entity, terminal, config, mut state, mut output, mut stats, ui) in &mut terminals {
        stats.set_if_neq(TerminalStats::default());
        if !config.sizing.is_valid() {
            suspend_terminal(&mut state, &mut output, TerminalStatus::InvalidSizing);
            continue;
        }
        // Inspect effective values only when the component was touched. Invalid
        // scale/blink inputs have documented fallbacks and must compare as such.
        let mut config_changed = false;
        let mut shaping_changed = false;
        if config.is_changed() || state.last_config.is_none() {
            let mut effective = (*config).clone();
            if matches!(effective.raster.scale, TerminalRenderScale::Fixed(_)) {
                effective.raster.scale = TerminalRenderScale::Fixed(resolve_raster_scale(
                    effective.raster.scale,
                    false,
                    None,
                ));
            }
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
            || state.font_ids != face_ids
            || state.fonts_ready != fonts_ready
            || (!changed_fonts.is_empty()
                && face_ids
                    .iter()
                    .flatten()
                    .any(|id| changed_fonts.contains(id)));
        state.font_ids = face_ids;
        state.fonts_ready = fonts_ready;
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
            suspend_terminal(&mut state, &mut output, status);
            continue;
        }
        // A resize during the sync that first reports readiness is part of settling,
        // not a re-measure; only terminals that were already ready get the event.
        let was_ready = state.ready_sent;
        let mut next_output = output.clone();
        let mut next_stats = TerminalStats::default();
        let mut context = text.context(&mut images);
        let result = sync_batch_terminal(
            terminal,
            SyncInput {
                config,
                config_changed,
                shaping_changed,
                fonts_changed,
                raster_scale: resolve_raster_scale(
                    config.raster.scale,
                    ui_present(&ui),
                    window_scale,
                ),
                elapsed,
                texture_limit: texture_limit(device.as_deref()),
            },
            &mut state,
            &mut next_output,
            &mut next_stats,
            &mut context,
        );
        let previous_size = match result {
            Ok(previous_size) => {
                state.last_failure = None;
                next_output.status = TerminalStatus::Ready;
                previous_size
            }
            Err(status) => {
                if let Some(failure) = context.failure.take()
                    && state.last_failure.as_ref() != Some(&failure)
                {
                    warn!(?entity, "bevy_terminal: {failure}");
                    state.last_failure = Some(failure);
                }
                suspend_terminal(&mut state, &mut output, status);
                // Failed partial construction cannot publish provisional geometry.
                stats.set_if_neq(next_stats);
                continue;
            }
        };
        stats.set_if_neq(next_stats);
        output.set_if_neq(next_output);
        present(
            ui,
            &output.image,
            output.geometry.size,
            output.geometry.raster_scale,
        );
        let metrics_ready = !needs_measured_advance(config) || state.measured_advance.is_some();
        if output.status == TerminalStatus::Ready && metrics_ready && !state.ready_sent {
            // The first sync with usable fonts settles the measured cell size, so the
            // texture is now at its final size for this configuration.
            state.ready_sent = true;
            commands.trigger(TerminalReady { entity });
        }
        if let Some(previous_size) = previous_size
            && was_ready
            && output.status == TerminalStatus::Ready
        {
            commands.trigger(TerminalRemeasured {
                entity,
                previous_size,
                size: output.geometry.size,
                cell_size: output.geometry.cell_size(),
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
    config.sizing.needs_advance()
}

fn suspend_terminal(
    state: &mut BatchMainState,
    output: &mut Mut<'_, TerminalTexture>,
    status: TerminalStatus,
) {
    state.invalidate();
    if output.status != status {
        output.status = status;
    }
}

/// Resolved inputs for one terminal in this update. Separate invalidation
/// causes preserve cached shaping for paint-only changes.
struct SyncInput<'a> {
    config: &'a TerminalRenderConfig,
    config_changed: bool,
    shaping_changed: bool,
    fonts_changed: bool,
    raster_scale: f32,
    elapsed: f32,
    texture_limit: u32,
}

fn sync_batch_terminal(
    terminal: &TerminalRenderer,
    input: SyncInput<'_>,
    state: &mut BatchMainState,
    output: &mut TerminalTexture,
    stats: &mut TerminalStats,
    cx: &mut TextContext<'_>,
) -> Result<Option<UVec2>, TerminalStatus> {
    let SyncInput {
        config,
        config_changed,
        shaping_changed,
        fonts_changed,
        raster_scale,
        elapsed,
        texture_limit,
    } = input;
    let surface = terminal.surface();
    if state
        .surface
        .as_ref()
        .is_none_or(|previous| !previous.shares_state_with(surface))
    {
        state.surface = Some(surface.clone());
        state.last_snapshot = None;
        state.pending = None;
        state.snapshot_blinks = false;
    }
    let scale_changed = state.raster_scale != raster_scale;
    let needs_measured_advance = needs_measured_advance(config);
    if needs_measured_advance
        && (state.measured_advance.is_none() || shaping_changed || fonts_changed)
    {
        state.measured_advance = match measure_advance(
            &config.font,
            cx.fonts,
            cx.text_pipeline,
            cx.font_cx,
            cx.layout_cx,
        ) {
            Ok(advance) => Some(advance),
            Err(error) => {
                cx.failure = Some(format!(
                    "advance measurement for {:?}: {error}",
                    config.font.regular
                ));
                None
            }
        };
    }
    // An unmeasured cell is not geometry. Keep the provisional component and
    // do not publish a scene or readiness event until the selected face shapes.
    if needs_measured_advance && state.measured_advance.is_none() {
        return Err(TerminalStatus::ShapingFailed);
    }
    let metrics = resolve_metrics(config, state.measured_advance);
    let requested = physical_config(metrics, raster_scale);
    if terminal_pixel_size(surface.size(), &requested).max_element() > texture_limit
        || requested.font_size > texture_limit as f32
    {
        return Err(TerminalStatus::TextureTooLarge);
    }
    let font_size_changed = state.metrics != Some(metrics);
    state.metrics = Some(metrics);
    let text_assets_changed =
        shaping_changed || fonts_changed || scale_changed || font_size_changed;
    let previous_cell_size = output.geometry.cell_size();
    if text_assets_changed {
        state.raster_config = refine_metrics(
            config,
            state.measured_advance,
            physical_config(metrics, raster_scale),
            cx,
        );
        if cx.failure.is_some() {
            return Err(TerminalStatus::ShapingFailed);
        }
        output.geometry.physical_font_size = state.raster_config.font_size;
        output.geometry.physical_cell_size = state.raster_config.cell_size;
    }
    if text_assets_changed {
        state.shapes.clear();
        state.glyph_atlas.clear(cx.images);
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
        snapshot.revision() == surface.revision()
            && !config_changed
            && !text_assets_changed
            && !blink_changed
    }) {
        // Keep the recorded phases current so an irrelevant flip is not
        // mistaken for a change once blinking content appears later.
        state.blink = blink;
        return Ok(None);
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
        return Ok(None);
    }
    // Extraction can be delayed while a newly created output or glyph atlas reaches the render
    // world. If a newer payload is already waiting in the main world, make its replacement a
    // complete image of the newest snapshot so rows changed by an intermediate payload cannot be
    // lost when that payload is superseded.
    full |= state.submitted.load(Ordering::Acquire) != state.generation;

    let new_size = terminal_pixel_size(snapshot.size(), &state.raster_config);
    if new_size.max_element() > texture_limit {
        return Err(TerminalStatus::TextureTooLarge);
    }
    output.geometry.surface = surface.downgrade();
    output.geometry.grid = snapshot.size();
    output.geometry.resize_generation = snapshot.resize_generation;
    let output_resized = output.geometry.size != new_size;
    let logical_size = new_size.as_vec2() / raster_scale;
    let geometry_changed = output_resized
        || output.geometry.cell_size() != previous_cell_size
        || output.geometry.logical_size() != logical_size
        || output.geometry.raster_scale != raster_scale;
    let previous_size = geometry_changed.then_some(output.geometry.size);
    if output_resized {
        // Reallocate the image in place so the handle stays stable; the render world
        // recreates the GPU texture for the modified asset.
        if cx
            .images
            .insert(state.output.id(), make_target_image(new_size))
            .is_err()
        {
            warn!("bevy_terminal: could not reallocate a terminal texture in place");
        }
        output.geometry.size = new_size;
    }
    // Write the texture component only when something changed so `Changed<TerminalTexture>`
    // observers are not woken on every synced frame.
    if output.geometry.logical_size() != logical_size
        || output.geometry.raster_scale != raster_scale
    {
        output.geometry.raster_scale = raster_scale;
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
        config,
        *raster_config,
        &rows,
        full,
        destination,
        cx,
        shapes,
        glyph_atlas,
        scratch,
        stats,
        blink,
    );
    if cx.failure.is_some() {
        return Err(TerminalStatus::ShapingFailed);
    }
    // Existing GPU textures are safe to consume before Bevy's asset preparation systems. A
    // resize or shape miss can create/modify an Image this frame, so those scenes use the later
    // submission point after RenderAsset preparation instead.
    scene.requires_prepared_assets = output_resized || stats.shape_misses != 0;
    stats.scene_ns = scene_start.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
    stats.changed_rows = u32::try_from(rows.len()).unwrap_or(u32::MAX);
    stats.draw_batches = u32::try_from(scene.batches.len()).unwrap_or(u32::MAX);
    state.generation = state.generation.wrapping_add(1);
    scene.submission = Some((state.submitted.clone(), state.generation));
    state.pending = Some(scene);
    state.snapshot_blinks = snapshot_blinks(&snapshot);
    state.last_snapshot = Some(snapshot);
    state.blink = blink;
    state.raster_scale = raster_scale;
    Ok(previous_size)
}

#[cfg(test)]
mod tests;
