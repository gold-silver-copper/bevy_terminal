//! Public renderer components, measurement state, and coordinate helpers.
use super::TerminalRenderConfig;
use crate::{scene::GridSize, surface::TerminalSurface};
use bevy::prelude::*;

/// One independently rendered terminal: the surface it renders.
///
/// Spawn this component on an entity after adding [`super::TerminalPlugin`] once. A
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
pub struct TerminalRenderer {
    surface: TerminalSurface,
}

impl TerminalRenderer {
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

impl From<TerminalSurface> for TerminalRenderer {
    fn from(surface: TerminalSurface) -> Self {
        Self::new(surface)
    }
}

/// Current measurement state. Readiness describes main-world geometry, not
/// completion of GPU commands; exports still require render-world readback.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TerminalStatus {
    /// Waiting for configured fonts to load and register.
    #[default]
    Loading,
    /// Geometry is measured and usable by layout consumers.
    Ready,
    /// The application's Bevy text resources are missing.
    MissingTextResources,
    /// A configured font asset failed to load.
    FontFailed,
    /// Text shaping failed; the renderer retries without caching the failure.
    ShapingFailed,
    /// A sizing dimension is non-finite or non-positive.
    InvalidSizing,
    /// The requested geometry exceeds the active device's texture limits.
    TextureTooLarge,
}

/// The renderer-owned terminal texture and its current dimensions.
///
/// The image is `Rgba8UnormSrgb` (display-ready, straight alpha) and its
/// handle is stable for the terminal's lifetime.
///
/// Attached to every [`TerminalRenderer`] entity by [`super::TerminalPlugin`] on the first
/// update after it is spawned. [`TerminalReady`] is triggered once the selected
/// fonts and any font-driven cell metrics have been measured. The image handle
/// stays the same for the lifetime of the terminal: resizes reallocate the
/// image in place.
#[derive(Clone, Debug, PartialEq, Component)]
pub struct TerminalTexture {
    /// Whether the dimensions below are currently authoritative.
    pub status: TerminalStatus,
    /// Render-world image targeted by terminal rendering; GPU completion is separate.
    pub image: Handle<Image>,
    /// Physical pixel dimensions of `image`.
    pub size: UVec2,
    /// Logical dimensions used for a Bevy UI presentation node.
    pub logical_size: Vec2,
    /// Physical pixels per logical pixel used to rasterize `image`.
    pub raster_scale: f32,
    /// Effective logical size of one cell: the physical cell (whole pixels,
    /// possibly grown to the font's line box — see [`super::TerminalSizing`])
    /// divided by `raster_scale`.
    pub cell_size: Vec2,
    /// Effective logical font size. This can differ slightly from a requested
    /// [`super::TerminalSizing::FromFont`] size when [`super::TerminalSizing::FromFont`]
    /// snaps the measured advance to a whole physical-pixel cell.
    pub font_size: f32,
}

impl TerminalTexture {
    /// Returns authoritative geometry and its image handle, or `None` while
    /// loading or failed. Provisional fields must not drive PTY reflow.
    #[must_use]
    pub fn measured(&self) -> Option<&Self> {
        (self.status == TerminalStatus::Ready).then_some(self)
    }

    /// Returns the grid that fits into `logical_size` (floor, at least 1×1)
    /// at this terminal's current cell size.
    #[must_use]
    pub fn grid_for(&self, logical_size: Vec2) -> GridSize {
        grid_for(logical_size, self.cell_size)
    }
}

/// Returns the grid that fits `logical_size` with cells of `cell_size`
/// (floor, at least 1×1, each dimension at most `u16::MAX`, and total cells
/// bounded by [`TerminalSurface::MAX_CELLS`]).
#[must_use]
pub fn grid_for(logical_size: Vec2, cell_size: Vec2) -> GridSize {
    let fit = |logical: f32, cell: f32| {
        if !logical.is_finite() || logical <= 0.0 {
            return 1;
        }
        let cell = if cell.is_finite() && cell > 0.0 {
            cell
        } else {
            1.0
        };
        (logical / cell).floor().clamp(1.0, f32::from(u16::MAX)) as u16
    };
    let columns = fit(logical_size.x, cell_size.x);
    let rows = fit(logical_size.y, cell_size.y)
        .min((TerminalSurface::MAX_CELLS / usize::from(columns)).min(usize::from(u16::MAX)) as u16);
    GridSize::new(columns, rows)
}

/// Returns the grid that fills `window` with cells of `cell_size` (logical pixels).
#[must_use]
pub fn grid_for_window(window: &Window, cell_size: Vec2) -> GridSize {
    grid_for(window.resolution.size(), cell_size)
}

/// Returns the physical-to-logical ratio of `window`'s actual framebuffer
/// (at least 1.0), suitable for [`super::TerminalRenderScale::Fixed`] so the
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

/// Triggered once on a [`TerminalRenderer`] entity when its [`TerminalTexture`] has
/// been allocated at its measured size. GPU submission and readback happen later.
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

/// Triggered on a [`TerminalRenderer`] entity whenever its [`TerminalTexture`] changes
/// physical or logical geometry *after* [`TerminalReady`] has fired: a surface
/// resize, a configuration change, a raster-scale change, or a font that arrived
/// late and re-measured the cell. The image handle is unchanged. Physical sizes
/// may be equal when a scale change preserves snapped pixels but changes the
/// logical cell size. Custom presentation code should refresh derived geometry.
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

/// Counters for the most recent scene update of one [`TerminalRenderer`]; all zero on
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
