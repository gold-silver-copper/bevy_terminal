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
/// [`TerminalStats`]. Applications bind the output image to their own UI,
/// sprites, or materials and choose layout and raster scale explicitly.
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
/// update after it is spawned. [`Self::measured`] exposes geometry once the selected
/// fonts and cell metrics have been measured. The image handle
/// stays the same for the lifetime of the terminal: resizes reallocate the
/// image in place.
#[derive(Clone, Debug, PartialEq, Component)]
pub struct TerminalTexture {
    /// Whether the dimensions below are currently authoritative.
    pub status: TerminalStatus,
    /// Render-world image targeted by terminal rendering; GPU completion is separate.
    pub image: Handle<Image>,
    pub(super) geometry: TerminalGeometry,
}

impl TerminalTexture {
    /// Returns authoritative geometry, or `None` while loading, failed, or
    /// waiting for a shared surface resize to be measured.
    #[must_use]
    pub fn measured(&self) -> Option<&TerminalGeometry> {
        (self.status == TerminalStatus::Ready && self.geometry.is_current())
            .then_some(&self.geometry)
    }
}

/// One coherent measurement of a particular surface and grid generation.
///
/// Keep the last accepted value if application layout must remain stable while
/// a newer measurement is pending. It cannot be adopted by a backend after a
/// resize, even when the grid later returns to the same dimensions. Different
/// renderers may legitimately measure the same surface at different scales.
/// Retaining geometry does not keep the source surface or its cells alive.
#[derive(Clone, Debug, PartialEq)]
pub struct TerminalGeometry {
    pub(super) surface: crate::surface::WeakSurface,
    pub(super) resize_generation: u64,
    pub(super) grid: GridSize,
    pub(super) size: UVec2,
    pub(super) physical_cell_size: Vec2,
    pub(super) physical_font_size: f32,
    pub(super) raster_scale: f32,
}

impl TerminalGeometry {
    /// Measured dimensions in cells.
    #[must_use]
    pub const fn grid(&self) -> GridSize {
        self.grid
    }

    /// Physical image dimensions in pixels.
    #[must_use]
    pub const fn size(&self) -> UVec2 {
        self.size
    }

    /// Logical presentation dimensions.
    #[must_use]
    pub fn logical_size(&self) -> Vec2 {
        self.size.as_vec2() / self.raster_scale
    }

    /// Physical pixels per logical pixel.
    #[must_use]
    pub const fn raster_scale(&self) -> f32 {
        self.raster_scale
    }

    /// Effective logical cell dimensions, after physical-pixel snapping.
    #[must_use]
    pub fn cell_size(&self) -> Vec2 {
        self.physical_cell_size / self.raster_scale
    }

    /// Effective logical font size.
    #[must_use]
    pub fn font_size(&self) -> f32 {
        self.physical_font_size / self.raster_scale
    }

    /// Physical font size glyphs are rasterized at; exact, unlike
    /// `font_size() * raster_scale()`.
    #[must_use]
    pub const fn physical_font_size(&self) -> f32 {
        self.physical_font_size
    }

    /// Grid fitting the available logical space, bounded by surface limits.
    #[must_use]
    pub fn grid_for(&self, logical_size: Vec2) -> GridSize {
        grid_for(logical_size, self.cell_size())
    }

    /// Whether this measurement belongs to this surface's current grid.
    /// Text and cursor changes do not invalidate it.
    #[must_use]
    pub fn matches_surface(&self, surface: &TerminalSurface) -> bool {
        self.surface.matches(surface) && self.is_current()
    }

    /// Whether the source surface has retained its measured grid generation.
    #[must_use]
    pub fn is_current(&self) -> bool {
        self.surface.is_current(self.resize_generation)
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

/// Counters for the most recent scene update of one [`TerminalRenderer`]; all zero on
/// frames that produced no terminal work.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Component)]
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
    /// Nanoseconds spent updating the retained terminal snapshot; zero without `timings`.
    pub snapshot_ns: u64,
    /// Nanoseconds spent generating the compact CPU scene; zero without `timings`.
    pub scene_ns: u64,
}

impl std::fmt::Display for TerminalStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "rows {} · cells {} · quads {} solid + {} glyph in {} batches · {} shape misses",
            self.changed_rows,
            self.snapshot_cells,
            self.solid_quads,
            self.glyph_quads,
            self.draw_batches,
            self.shape_misses,
        )?;
        #[cfg(feature = "timings")]
        write!(
            f,
            " · snapshot {} µs · scene {} µs",
            self.snapshot_ns / 1000,
            self.scene_ns / 1000,
        )?;
        Ok(())
    }
}
