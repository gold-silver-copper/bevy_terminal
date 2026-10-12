//! Public renderer components, measurement state, and coordinate helpers.
use crate::{grid::TerminalGrid, scene::GridSize};
use bevy::prelude::*;

/// Something the renderer can draw: it provides a [`TerminalGrid`].
///
/// A component source is drawn into its own entity's [`TerminalTexture`].
/// [`TerminalGrid`] itself is one, registered by [`super::TerminalPlugin`];
/// register another with [`super::TerminalSourcePlugin`] (Ratatui terminals
/// are registered by `bevy_terminal_ratatui`) and require
/// [`TerminalRenderConfig`](super::TerminalRenderConfig) on it so each
/// terminal has a configuration. A resource source, registered with
/// [`super::TerminalResourcePlugin`], is drawn into every entity marked with
/// [`super::ResourceTerminal`].
pub trait TerminalSource: Send + Sync + 'static {
    /// The grid to draw.
    fn grid(&self) -> &TerminalGrid;
}

impl TerminalSource for TerminalGrid {
    fn grid(&self) -> &TerminalGrid {
        self
    }
}

/// Where a terminal's output stands. Geometry exists only while the output
/// is [`Ready`](Self::Ready), and then it describes the image exactly.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum TerminalStatus {
    /// Waiting for configured fonts to load and register.
    #[default]
    Loading,
    /// The image holds the grid at this geometry. Readiness describes
    /// main-world geometry, not completion of GPU commands; exports still
    /// require render-world readback.
    Ready(TerminalGeometry),
    /// The terminal cannot be drawn. It stays idle until one of its inputs
    /// changes: its grid or configuration, its fonts, or the render device.
    Failed(TerminalFailure),
}

/// Why a terminal cannot be drawn.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalFailure {
    /// The application's Bevy text resources are missing.
    MissingTextResources,
    /// A configured font asset failed to load.
    FontFailed,
    /// Text shaping failed; the failure is logged once.
    ShapingFailed,
    /// A sizing dimension is non-finite or non-positive.
    InvalidSizing,
    /// The requested geometry exceeds the active device's texture limits.
    TextureTooLarge,
}

/// The renderer-owned terminal texture and its status.
///
/// The image is `Rgba8UnormSrgb` (display-ready, straight alpha). Its handle
/// is stable for the terminal's lifetime: resizes reallocate the image in
/// place, so applications bind it once to their UI, sprites or materials.
///
/// Attached to every terminal entity by [`super::TerminalPlugin`] as soon as
/// its [`TerminalSource`] is added, and removed with it. The component
/// changes (for `Changed<TerminalTexture>`) only when the status does,
/// including the geometry, never on content alone.
#[derive(Clone, Debug, PartialEq, Component)]
pub struct TerminalTexture {
    pub(crate) image: Handle<Image>,
    pub(crate) status: TerminalStatus,
}

impl TerminalTexture {
    /// Whether the image is ready, and its geometry, or why not.
    #[must_use]
    pub const fn status(&self) -> &TerminalStatus {
        &self.status
    }

    /// Render-world image targeted by terminal rendering; GPU completion is separate.
    #[must_use]
    pub const fn image(&self) -> &Handle<Image> {
        &self.image
    }

    /// Returns the measured geometry, or `None` while loading or failed.
    ///
    /// It describes the grid as of the last [`super::TerminalSystems::Sync`]:
    /// a producer that resizes the grid after `Sync` sees the previous
    /// geometry until the next one; compare [`TerminalGeometry::grid`] with
    /// the grid's size to tell.
    #[must_use]
    pub const fn measured(&self) -> Option<&TerminalGeometry> {
        match &self.status {
            TerminalStatus::Ready(geometry) => Some(geometry),
            _ => None,
        }
    }
}

/// One coherent measurement of a terminal: its grid, cell and font size, and
/// the size of the image holding it.
///
/// Only the renderer creates geometry, together with the image it describes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerminalGeometry {
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

    /// Physical cell size in whole pixels; exact, unlike
    /// `cell_size() * raster_scale()`. `size()` is the grid times this.
    #[must_use]
    pub const fn physical_cell_size(&self) -> Vec2 {
        self.physical_cell_size
    }
}

/// Returns the grid that fits `logical_size` with cells of `cell_size`
/// (floor, at least 1×1, each dimension at most `u16::MAX`, and total cells
/// bounded by [`TerminalGrid::MAX_CELLS`]), such as
/// `grid_for(available, geometry.cell_size())` for a measured [`TerminalGeometry`].
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
    GridSize::new(
        fit(logical_size.x, cell_size.x),
        fit(logical_size.y, cell_size.y),
    )
    .bounded()
}

/// Counters for the most recent scene update of one terminal; all zero on
/// frames that produced no terminal work.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Component)]
pub struct TerminalStats {
    /// Rows rebuilt into the latest payload.
    pub changed_rows: u32,
    /// Solid rectangles (backgrounds, decorations, cursor) in the latest payload.
    pub solid_quads: u32,
    /// Glyph rectangles in the latest payload.
    pub glyph_quads: u32,
    /// Draw batches (one per glyph-atlas switch) in the latest payload.
    pub draw_batches: u32,
    /// Shape-cache misses in the latest update.
    pub shape_misses: u32,
    /// Nanoseconds spent generating the compact CPU scene.
    #[cfg(feature = "timings")]
    pub scene_ns: u64,
}

impl std::fmt::Display for TerminalStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "rows {} · quads {} solid + {} glyph in {} batches · {} shape misses",
            self.changed_rows,
            self.solid_quads,
            self.glyph_quads,
            self.draw_batches,
            self.shape_misses,
        )?;
        #[cfg(feature = "timings")]
        write!(f, " · scene {} µs", self.scene_ns / 1000,)?;
        Ok(())
    }
}
