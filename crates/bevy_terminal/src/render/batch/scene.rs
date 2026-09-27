//! CPU scene construction, glyph fitting, and quad geometry.
use super::shaping::{
    CachedGlyph, ShapeCaches, UnifiedGlyphAtlas, cached_shape, is_box_drawing, is_graphics,
    is_powerline, is_symbol,
};
use super::{
    BatchScene, BlinkPhases, DrawBatch, PixelGeometry, QuadInstance, RasterMetrics, ResolvedStyle,
    TerminalRenderConfig, TerminalSnapshot, TerminalStats, TextContext, cell_span,
    cursor_should_be_visible, terminal_pixel_size,
};
use crate::scene::TerminalCell;
use bevy::prelude::*;

#[derive(Default)]
pub(super) struct SceneScratch {
    pub(super) backgrounds: Vec<QuadInstance>,
    /// Background rectangles in pixel space, so adjoining rows can merge
    /// before conversion to clip-space quads.
    pub(super) background_rects: Vec<(PixelGeometry, Color)>,
    /// Indices into `background_rects` emitted for the previous row.
    pub(super) prev_runs: Vec<usize>,
    /// Indices into `background_rects` emitted for the current row.
    pub(super) current_runs: Vec<usize>,
    pub(super) glyphs: Vec<(AssetId<Image>, QuadInstance)>,
    /// Glyphs of the laid-out rows before clipping to the repainted bands.
    pub(super) placed: Vec<PlacedGlyph>,
    pub(super) decorations: Vec<QuadInstance>,
    pub(super) cursor: Vec<QuadInstance>,
    pub(super) styles: Vec<ResolvedStyle>,
    /// Rows whose bands this scene repaints.
    pub(super) repaint: Vec<bool>,
    /// Rows whose backgrounds, glyphs and decorations have been laid out.
    pub(super) painted: Vec<bool>,
    /// Glyphs of the last full scene before clipping, recorded for the probe.
    #[cfg(test)]
    pub(super) probe: Option<Vec<super::probe::ProbeGlyph>>,
}

impl SceneScratch {
    pub(super) fn clear(&mut self) {
        self.backgrounds.clear();
        self.background_rects.clear();
        self.prev_runs.clear();
        self.current_runs.clear();
        self.glyphs.clear();
        self.placed.clear();
        self.decorations.clear();
        self.cursor.clear();
        self.styles.clear();
        self.repaint.clear();
        self.painted.clear();
    }
}

/// Records a background run, extending an identically aligned run from the
/// previous row into one taller rectangle when possible.
pub(super) fn merge_background_rect(
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

/// Solid rectangles, as fractions of the cell `(left, top, right, bottom)`,
/// for a Unicode Block Elements symbol (U+2580..U+259F, shades excluded).
///
/// Drawing these from geometry instead of the font, as Ghostty does, makes
/// halves, eighths and quadrants tile the cell exactly whatever the font's
/// block glyphs look like: a cell sized to the font's line box no longer
/// leaves a seam under a shorter `█`, and fonts with no block glyphs at all
/// still render them.
pub(super) fn block_element(symbol: &str) -> Option<&'static [(f32, f32, f32, f32)]> {
    const FULL: &[(f32, f32, f32, f32)] = &[(0.0, 0.0, 1.0, 1.0)];
    macro_rules! rects {
        ($($r:expr),* $(,)?) => {{
            const R: &[(f32, f32, f32, f32)] = &[$($r),*];
            Some(R)
        }};
    }
    let mut chars = symbol.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return None;
    };
    match c {
        '\u{2580}' => rects![(0.0, 0.0, 1.0, 0.5)],
        '\u{2581}' => rects![(0.0, 7.0 / 8.0, 1.0, 1.0)],
        '\u{2582}' => rects![(0.0, 6.0 / 8.0, 1.0, 1.0)],
        '\u{2583}' => rects![(0.0, 5.0 / 8.0, 1.0, 1.0)],
        '\u{2584}' => rects![(0.0, 0.5, 1.0, 1.0)],
        '\u{2585}' => rects![(0.0, 3.0 / 8.0, 1.0, 1.0)],
        '\u{2586}' => rects![(0.0, 2.0 / 8.0, 1.0, 1.0)],
        '\u{2587}' => rects![(0.0, 1.0 / 8.0, 1.0, 1.0)],
        '\u{2588}' => Some(FULL),
        '\u{2589}' => rects![(0.0, 0.0, 7.0 / 8.0, 1.0)],
        '\u{258a}' => rects![(0.0, 0.0, 6.0 / 8.0, 1.0)],
        '\u{258b}' => rects![(0.0, 0.0, 5.0 / 8.0, 1.0)],
        '\u{258c}' => rects![(0.0, 0.0, 0.5, 1.0)],
        '\u{258d}' => rects![(0.0, 0.0, 3.0 / 8.0, 1.0)],
        '\u{258e}' => rects![(0.0, 0.0, 2.0 / 8.0, 1.0)],
        '\u{258f}' => rects![(0.0, 0.0, 1.0 / 8.0, 1.0)],
        '\u{2590}' => rects![(0.5, 0.0, 1.0, 1.0)],
        '\u{2594}' => rects![(0.0, 0.0, 1.0, 1.0 / 8.0)],
        '\u{2595}' => rects![(7.0 / 8.0, 0.0, 1.0, 1.0)],
        '\u{2596}' => rects![(0.0, 0.5, 0.5, 1.0)],
        '\u{2597}' => rects![(0.5, 0.5, 1.0, 1.0)],
        '\u{2598}' => rects![(0.0, 0.0, 0.5, 0.5)],
        '\u{2599}' => rects![(0.0, 0.0, 0.5, 0.5), (0.0, 0.5, 1.0, 1.0)],
        '\u{259a}' => rects![(0.0, 0.0, 0.5, 0.5), (0.5, 0.5, 1.0, 1.0)],
        '\u{259b}' => rects![(0.0, 0.0, 1.0, 0.5), (0.0, 0.5, 0.5, 1.0)],
        '\u{259c}' => rects![(0.0, 0.0, 1.0, 0.5), (0.5, 0.5, 1.0, 1.0)],
        '\u{259d}' => rects![(0.5, 0.0, 1.0, 0.5)],
        '\u{259e}' => rects![(0.5, 0.0, 1.0, 0.5), (0.0, 0.5, 0.5, 1.0)],
        '\u{259f}' => rects![(0.5, 0.0, 1.0, 0.5), (0.0, 0.5, 1.0, 1.0)],
        _ => None,
    }
}

/// How many rows above and below its own a row's glyph ink reaches.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct RowReach {
    pub(super) up: u16,
    pub(super) down: u16,
}

impl RowReach {
    /// The rows `row`'s ink covers, cut to a grid `height` rows tall.
    pub(super) fn rows(self, row: u16, height: u16) -> std::ops::Range<u16> {
        row.saturating_sub(self.up)..row.saturating_add(self.down).saturating_add(1).min(height)
    }

    /// The reach of ink spanning `top..bottom` pixels from a row of `cell_height`.
    fn of(top: f32, bottom: f32, cell_height: f32) -> Self {
        let rows = |pixels: f32| {
            (pixels.max(0.0) / cell_height)
                .ceil()
                .min(f32::from(u16::MAX)) as u16
        };
        Self {
            up: rows(-top),
            down: rows(bottom - cell_height),
        }
    }

    fn union(self, other: Self) -> Self {
        Self {
            up: self.up.max(other.up),
            down: self.down.max(other.down),
        }
    }
}

/// A glyph bitmap placed in texture pixels, before clipping.
#[derive(Clone, Copy, Debug)]
pub(super) struct PlacedGlyph {
    pub(super) row: u16,
    pub(super) texture: AssetId<Image>,
    pub(super) geometry: PixelGeometry,
    pub(super) uv: Vec4,
    pub(super) color: Color,
    pub(super) alpha_mask: bool,
    /// Grid graphics keep the clip of the cells they are drawn over.
    pub(super) cells: Option<PixelGeometry>,
}

/// Builds the scene that repaints `changed` rows (every row when `full`).
///
/// Glyph ink is not confined to its row: like Ghostty, accents, stacked marks
/// and tall scripts overflow into the rows above and below, and only the
/// texture's edges clip them. Rows remain the unit of repaint, so a partial
/// scene must reproduce exactly what a full one draws in the rows it repaints:
///
/// - a changed row also repaints the rows its previous and its new ink reach
///   (`reach` records, per row, how far its drawn ink reaches);
/// - the repainted bands are cleared and get their backgrounds first, then the
///   glyphs of every row whose ink reaches a repainted band are drawn, clipped
///   to those bands, in the row-major order of a full scene, then
///   decorations and the cursor.
///
/// Rows without overflowing ink add no work: their reach is zero, so the
/// repainted set is the changed set and no neighbour is laid out.
#[allow(clippy::too_many_arguments)]
pub(super) fn build_scene(
    snapshot: &TerminalSnapshot,
    config: &TerminalRenderConfig,
    raster: RasterMetrics,
    changed: &[u16],
    full: bool,
    destination: AssetId<Image>,
    cx: &mut TextContext<'_>,
    shapes: &mut ShapeCaches,
    glyph_atlas: &mut UnifiedGlyphAtlas,
    scratch: &mut SceneScratch,
    reach: &mut Vec<RowReach>,
    stats: &mut TerminalStats,
    blink: BlinkPhases,
) -> BatchScene {
    let size = terminal_pixel_size(snapshot.size(), &raster).as_vec2();
    let height = snapshot.size().height;
    scratch.clear();
    #[cfg(test)]
    let mut probe = scratch.probe.take().map(|mut probe| {
        if full {
            probe.clear();
        }
        probe
    });
    if full || reach.len() != usize::from(height) {
        // Nothing drawn before a full scene survives it.
        reach.clear();
        reach.resize(usize::from(height), RowReach::default());
    }
    let SceneScratch {
        backgrounds,
        background_rects,
        prev_runs,
        current_runs,
        glyphs,
        placed,
        decorations,
        cursor,
        styles,
        repaint,
        painted,
        ..
    } = scratch;
    repaint.resize(usize::from(height), false);
    painted.resize(usize::from(height), false);
    let mut painter = RowPainter {
        snapshot,
        config,
        raster,
        size,
        full,
        blink,
        cx: &mut *cx,
        shapes: &mut *shapes,
        glyph_atlas: &mut *glyph_atlas,
        stats: &mut *stats,
        styles: &mut *styles,
        #[cfg(test)]
        probe: probe.as_mut(),
    };

    // Changed rows and every row their previous ink reached.
    for &row in changed {
        for reached in reach[usize::from(row)].rows(row, height) {
            repaint[usize::from(reached)] = true;
        }
    }
    for &row in changed {
        current_runs.clear();
        painter.backgrounds(row, background_rects, prev_runs, current_runs);
        std::mem::swap(prev_runs, current_runs);
        let first = placed.len();
        painter.cells(row, placed, Some(decorations));
        painted[usize::from(row)] = true;
        let cell_height = raster.cell_size.y;
        let row_top = f32::from(row) * cell_height;
        let row_reach = placed[first..]
            .iter()
            .filter(|glyph| glyph.cells.is_none())
            .fold(RowReach::default(), |reach, glyph| {
                reach.union(RowReach::of(
                    glyph.geometry.y - row_top,
                    glyph.geometry.y + glyph.geometry.height - row_top,
                    cell_height,
                ))
            });
        reach[usize::from(row)] = row_reach;
        for reached in row_reach.rows(row, height) {
            repaint[usize::from(reached)] = true;
        }
    }
    // Unchanged rows repainted because changed ink reaches or reached them.
    let mut reordered = false;
    for row in 0..height {
        if repaint[usize::from(row)] && !painted[usize::from(row)] {
            painter.backgrounds(row, background_rects, prev_runs, current_runs);
            painter.cells(row, placed, Some(decorations));
            painted[usize::from(row)] = true;
            reordered = true;
        }
    }
    // Unchanged rows whose ink reaches into a repainted band.
    for row in 0..height {
        let row_reach = reach[usize::from(row)];
        if !painted[usize::from(row)]
            && row_reach != RowReach::default()
            && row_reach
                .rows(row, height)
                .any(|reached| repaint[usize::from(reached)])
        {
            painter.cells(row, placed, None);
            reordered = true;
        }
    }
    let painted_rows = painted.iter().filter(|painted| **painted).count();
    if reordered {
        // A full scene draws glyphs in row-major order; the sort is stable.
        placed.sort_by_key(|glyph| glyph.row);
    }
    for glyph in placed.iter() {
        let color = glyph.color;
        let mut push = |band: PixelGeometry| {
            let band = match glyph.cells {
                Some(cells) => intersect(band, cells),
                None => Some(band),
            };
            if let Some((geometry, uv)) =
                band.and_then(|band| clip_glyph_to_row(glyph.geometry, glyph.uv, band))
            {
                glyphs.push((
                    glyph.texture,
                    glyph_quad(geometry, uv, color, glyph.alpha_mask, size),
                ));
            }
        };
        if full {
            push(PixelGeometry {
                x: 0.0,
                y: 0.0,
                width: size.x,
                height: size.y,
            });
            continue;
        }
        // Clip to each run of repainted rows the bitmap covers.
        let cell_height = raster.cell_size.y;
        let first = (glyph.geometry.y / cell_height).floor().max(0.0) as u16;
        let last =
            (((glyph.geometry.y + glyph.geometry.height) / cell_height).ceil() as u16).min(height);
        let mut row = first;
        while row < last {
            if !repaint[usize::from(row)] {
                row += 1;
                continue;
            }
            let start = row;
            while row < last && repaint[usize::from(row)] {
                row += 1;
            }
            push(PixelGeometry {
                x: 0.0,
                y: f32::from(start) * cell_height,
                width: size.x,
                height: f32::from(row - start) * cell_height,
            });
        }
    }

    backgrounds.extend(
        background_rects
            .iter()
            .map(|&(geometry, color)| solid_quad(geometry, color, size)),
    );

    if cursor_should_be_visible(snapshot)
        && !blink.cursor_hidden
        && repaint[usize::from(snapshot.cursor_position().y)]
    {
        let position = snapshot.cursor_position();
        let cursor_thickness = raster.scale.round().max(1.0) * 2.0;
        let (x, y, width, height) = match config.cursor.style {
            super::super::CursorStyle::Block => (0.0, 0.0, raster.cell_size.x, raster.cell_size.y),
            super::super::CursorStyle::Bar => (
                0.0,
                0.0,
                cursor_thickness.min(raster.cell_size.x),
                raster.cell_size.y,
            ),
            super::super::CursorStyle::Underline => (
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

    #[cfg(test)]
    {
        scratch.probe = probe;
    }
    stats.changed_rows = u32::try_from(painted_rows).unwrap_or(u32::MAX);
    stats.solid_quads =
        u32::try_from(backgrounds.len() + decorations.len() + cursor.len()).unwrap_or(u32::MAX);
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
    append_glyph_batches(&mut instances, &mut batches, glyphs);
    append_batch(&mut instances, &mut batches, primary_atlas, decorations);
    append_batch(&mut instances, &mut batches, primary_atlas, cursor);
    BatchScene {
        submission: None,
        destination,
        destination_size: size.as_uvec2(),
        instances,
        batches,
        clear: full,
        clear_color: config.theme.background,
        requires_prepared_assets: false,
    }
}

/// Lays out the rows of one scene.
struct RowPainter<'a, 'w> {
    snapshot: &'a TerminalSnapshot,
    config: &'a TerminalRenderConfig,
    raster: RasterMetrics,
    size: Vec2,
    full: bool,
    blink: BlinkPhases,
    cx: &'a mut TextContext<'w>,
    shapes: &'a mut ShapeCaches,
    glyph_atlas: &'a mut UnifiedGlyphAtlas,
    stats: &'a mut TerminalStats,
    styles: &'a mut Vec<ResolvedStyle>,
    #[cfg(test)]
    probe: Option<&'a mut Vec<super::probe::ProbeGlyph>>,
}

impl RowPainter<'_, '_> {
    fn resolve_styles(&mut self, row: u16) {
        self.styles.clear();
        self.styles.extend(
            self.snapshot
                .row(row)
                .iter()
                .map(|cell| ResolvedStyle::new(cell, &self.config.theme)),
        );
    }

    /// Records the band clear (partial scenes) and background runs of `row`,
    /// merging runs with identical runs of the previous row in full scenes.
    fn backgrounds(
        &mut self,
        row: u16,
        rects: &mut Vec<(PixelGeometry, Color)>,
        prev_runs: &[usize],
        current_runs: &mut Vec<usize>,
    ) {
        let raster = self.raster;
        let theme_background = self.config.theme.background;
        if !self.full {
            // Partial repaints interleave a per-row clear with that row's runs,
            // so later rows' clears would overwrite runs merged upward; merge
            // vertically only in full rebuilds (which have no per-row clears).
            rects.push((
                PixelGeometry {
                    x: 0.0,
                    y: f32::from(row) * raster.cell_size.y,
                    width: self.size.x,
                    height: raster.cell_size.y,
                },
                theme_background,
            ));
        }
        self.resolve_styles(row);
        let styles = &*self.styles;
        let mut start = 0;
        while start < styles.len() {
            let color = styles[start].background;
            let mut end = start + 1;
            while end < styles.len() && styles[end].background == color {
                end += 1;
            }
            if !(self.full && color == theme_background) {
                let geometry = PixelGeometry {
                    x: start as f32 * raster.cell_size.x,
                    y: f32::from(row) * raster.cell_size.y,
                    width: (end - start) as f32 * raster.cell_size.x,
                    height: raster.cell_size.y,
                };
                if self.full {
                    merge_background_rect(rects, prev_runs, current_runs, geometry, color);
                } else {
                    rects.push((geometry, color));
                }
            }
            start = end;
        }
    }

    /// Places `row`'s glyphs into `placed`; with `decorations`, also records
    /// its block elements, underlines and strike-throughs.
    fn cells(
        &mut self,
        row: u16,
        placed: &mut Vec<PlacedGlyph>,
        mut decorations: Option<&mut Vec<QuadInstance>>,
    ) {
        self.resolve_styles(row);
        let raster = self.raster;
        let size = self.size;
        let cells = self.snapshot.row(row);
        let mut column = 0;
        while column < cells.len() {
            let cell = &cells[column];
            if cell.is_continuation() {
                column += 1;
                continue;
            }
            let width = cell_span(cells, column);
            let style = &self.styles[column];
            let symbol = cell.symbol();
            if style.hidden || self.blink.hides(style) {
                column += width;
                continue;
            }
            let cell_x = column as f32 * raster.cell_size.x;
            let cell_y = f32::from(row) * raster.cell_size.y;
            if let Some(rects) = block_element(symbol) {
                if let Some(decorations) = decorations.as_deref_mut() {
                    let cell_w = width as f32 * raster.cell_size.x;
                    let cell_h = raster.cell_size.y;
                    for &(left, top, right, bottom) in rects {
                        decorations.push(solid_quad(
                            PixelGeometry {
                                x: cell_x + left * cell_w,
                                y: cell_y + top * cell_h,
                                width: (right - left) * cell_w,
                                height: (bottom - top) * cell_h,
                            },
                            style.foreground,
                            size,
                        ));
                    }
                }
            } else if symbol != " " && !symbol.is_empty() {
                let columns = visual_columns(cells, column, width);
                let shaped = cached_shape(
                    symbol,
                    columns as u16,
                    style,
                    self.config,
                    raster,
                    size,
                    self.cx,
                    self.shapes,
                    self.glyph_atlas,
                    self.stats,
                );
                // Grid graphics keep the per-cell clip so overshoot cannot seam.
                let cells_clip = is_graphics(symbol).then_some(PixelGeometry {
                    x: cell_x,
                    y: cell_y,
                    width: columns as f32 * raster.cell_size.x,
                    height: raster.cell_size.y,
                });
                let box_drawing = is_box_drawing(symbol);
                let shift = Vec2::new(
                    if cells_clip.is_some() {
                        fit_horizontally(&shaped, columns as f32 * raster.cell_size.x, box_drawing)
                    } else {
                        edge_shift(&shaped, cell_x, size.x)
                    },
                    if box_drawing {
                        raster.box_offset
                    } else {
                        raster.glyph_offset
                    },
                );
                for glyph in shaped.iter() {
                    let geometry = PixelGeometry {
                        x: cell_x + glyph.offset.x + shift.x,
                        y: cell_y + glyph.offset.y + shift.y,
                        width: glyph.size.x,
                        height: glyph.size.y,
                    };
                    #[cfg(test)]
                    if self.full
                        && let Some(probe) = self.probe.as_deref_mut()
                    {
                        probe.push(super::probe::ProbeGlyph {
                            symbol: symbol.to_owned(),
                            row,
                            column: column as u16,
                            columns: columns as u16,
                            graphics: cells_clip.is_some(),
                            color: !glyph.alpha_mask,
                            texture: glyph.texture,
                            uv: glyph.uv,
                            geometry,
                            clip: cells_clip.unwrap_or(PixelGeometry {
                                x: 0.0,
                                y: 0.0,
                                width: size.x,
                                height: size.y,
                            }),
                            shift: shift.x,
                        });
                    }
                    placed.push(PlacedGlyph {
                        row,
                        texture: glyph.texture,
                        geometry,
                        uv: glyph.uv,
                        color: style.foreground,
                        alpha_mask: glyph.alpha_mask,
                        cells: cells_clip,
                    });
                }
            }
            if let Some(decorations) = decorations.as_deref_mut() {
                let decoration_x = column as f32 * raster.cell_size.x;
                let decoration_width = width as f32 * raster.cell_size.x;
                let decoration_thickness = raster.scale.round().max(1.0);
                if style.underlined {
                    decorations.push(solid_quad(
                        PixelGeometry {
                            x: decoration_x,
                            y: cell_y + (raster.cell_size.y - 2.0 * decoration_thickness).max(0.0),
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
                            y: cell_y + raster.cell_size.y * 0.55,
                            width: decoration_width,
                            height: decoration_thickness,
                        },
                        style.foreground,
                        size,
                    ));
                }
            }
            column += width;
        }
    }
}

/// The overlap of two rectangles, if any.
fn intersect(a: PixelGeometry, b: PixelGeometry) -> Option<PixelGeometry> {
    let left = a.x.max(b.x);
    let top = a.y.max(b.y);
    let right = (a.x + a.width).min(b.x + b.width);
    let bottom = (a.y + a.height).min(b.y + b.height);
    (right > left && bottom > top).then_some(PixelGeometry {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    })
}

/// Number of cells a run at `column` may visually occupy, after Ghostty's
/// `constraintWidth`: its declared `span`, except that a one-cell symbol
/// ([`is_symbol`]) followed by a blank cell may spread into that cell, unless
/// it is the row's last cell or directly follows another symbol that is not a
/// Powerline graphic (so runs of icons stay aligned). Logical occupancy is
/// unchanged: the blank neighbour still belongs to the terminal.
pub(super) fn visual_columns(cells: &[TerminalCell], column: usize, span: usize) -> usize {
    if span != 1 || column + 1 >= cells.len() || !is_symbol(cells[column].symbol()) {
        return span;
    }
    if column > 0 {
        let previous = cells[column - 1].symbol();
        if is_symbol(previous) && !is_powerline(previous) {
            return 1;
        }
    }
    let next = cells[column + 1].symbol();
    if next.is_empty() || next == " " || next == "\u{2002}" {
        2
    } else {
        1
    }
}

/// Whole-pixel shift keeping a run drawn at `x` inside a texture `width`
/// pixels wide. Ordinary text keeps its bearings, like Ghostty, and overflows
/// into neighbouring cells; only ink that would cross the texture's outer
/// edge, where Ghostty has window padding, is pushed back in. A run wider
/// than the texture keeps its place.
pub(super) fn edge_shift(glyphs: &[CachedGlyph], x: f32, width: f32) -> f32 {
    let (left, right) = glyphs
        .iter()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(l, r), g| {
            (
                l.min(x + g.offset.x + g.ink.0),
                r.max(x + g.offset.x + g.ink.1),
            )
        });
    if right <= left || right - left > width {
        0.0
    } else if left < 0.0 {
        super::metrics::snap(-left)
    } else if right > width {
        super::metrics::snap(width - right)
    } else {
        0.0
    }
}

/// Horizontal shift (whole pixels) applied to a grid graphic drawn over `span` pixels:
/// a run that fits but overhangs one side (an italic or a negative bearing) is
/// pushed inside; a run inside the span keeps its bearings; a run wider than
/// the span (a fallback family with a larger advance, a wide italic, a symbol
/// that could not be rescaled) keeps its bearings too and overflows, as
/// Ghostty draws unconstrained glyphs.
///
/// # Sub-pixel overshoot
///
/// Box-drawing and block glyphs are commonly drawn a little past their
/// advance on purpose (JetBrains Mono's `─` spans -20..620 units of a 600
/// advance) so that neighbouring strokes overlap instead of gapping. Rasterised,
/// that overshoot lights one faint extra column outside the span. Pushing the
/// run inside for it would move `┌` one pixel away from a `│` in the row
/// below, which is exactly the misalignment the overshoot exists to prevent.
/// For a single box-drawing character, an outside column with coverage below
/// the run's strongest column is treated as overshoot: the run keeps its
/// bearings and the graphics clip drops the column. Ordinary text does not
/// use this allowance. A full-strength column outside the span is real
/// overhang and is still pushed inside.
pub(super) fn fit_horizontally(glyphs: &[CachedGlyph], span: f32, box_drawing: bool) -> f32 {
    let mut left = f32::INFINITY;
    let mut right = f32::NEG_INFINITY;
    for glyph in glyphs {
        left = left.min(glyph.offset.x + glyph.ink.0);
        right = right.max(glyph.offset.x + glyph.ink.1);
    }
    if right <= left {
        return 0.0;
    }
    let (left, right) = if box_drawing {
        trim_overshoot(glyphs, span, left, right)
    } else {
        (left, right)
    };
    if right - left > span {
        0.0
    } else if left < 0.0 {
        super::metrics::snap(-left)
    } else if right > span {
        super::metrics::snap(span - right)
    } else {
        0.0
    }
}

/// Largest number of outside columns per side that may be sub-pixel overshoot.
pub(super) const OVERSHOOT_COLUMNS: f32 = 1.0;

/// Narrows a run's ink extents `[left, right)` by dropping, on each side, a
/// single outside column that is fainter than the run's strongest column (a
/// rasterised sub-pixel overshoot). Extents of runs without such columns are
/// returned unchanged.
pub(super) fn trim_overshoot(
    glyphs: &[CachedGlyph],
    span: f32,
    left: f32,
    right: f32,
) -> (f32, f32) {
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

pub(super) fn solid_quad(geometry: PixelGeometry, color: Color, target: Vec2) -> QuadInstance {
    QuadInstance {
        rect: clip_rect(snap_geometry(geometry), target),
        // A negative final UV component lets the unified fragment shader skip the atlas sample.
        uv: Vec4::new(0.0, 0.0, 0.0, -1.0),
        color: color.to_linear().to_f32_array().into(),
    }
}

pub(super) fn glyph_quad(
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

/// Crops a glyph quad to the band it is drawn in (its row, or its cells for
/// grid graphics), adjusting its atlas coordinates so the visible texels stay
/// in place.
pub(super) fn clip_glyph_to_row(
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

pub(super) fn snap_geometry(geometry: PixelGeometry) -> PixelGeometry {
    let left = super::metrics::snap(geometry.x);
    let top = super::metrics::snap(geometry.y);
    let right = super::metrics::snap(geometry.x + geometry.width).max(left);
    let bottom = super::metrics::snap(geometry.y + geometry.height).max(top);
    PixelGeometry {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    }
}

pub(super) fn clip_rect(geometry: PixelGeometry, target: Vec2) -> Vec4 {
    let left = geometry.x / target.x * 2.0 - 1.0;
    let right = (geometry.x + geometry.width) / target.x * 2.0 - 1.0;
    let top = 1.0 - geometry.y / target.y * 2.0;
    let bottom = 1.0 - (geometry.y + geometry.height) / target.y * 2.0;
    Vec4::new(left, top, right, bottom)
}

pub(super) fn append_batch(
    instances: &mut Vec<QuadInstance>,
    batches: &mut Vec<DrawBatch>,
    texture: AssetId<Image>,
    quads: &[QuadInstance],
) {
    append_batch_with(instances, batches, texture, quads, false);
}

pub(super) fn append_batch_with(
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

pub(super) fn append_glyph_batches(
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
