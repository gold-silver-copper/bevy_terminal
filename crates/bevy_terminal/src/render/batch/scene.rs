//! CPU scene construction, glyph fitting, and quad geometry.
use super::shaping::{
    CachedGlyph, ShapeCaches, UnifiedGlyphAtlas, cached_shape, is_powerline, is_symbol,
};
use super::sprite::{self, sprite_codepoint};
use super::{
    BatchScene, BlinkPhases, DrawBatch, Palette, PixelGeometry, QuadInstance, RasterMetrics,
    ResolvedStyle, TerminalRenderConfig, TerminalSnapshot, TerminalStats, TextContext, cell_span,
    cursor_should_be_visible, terminal_pixel_size,
};
use crate::scene::TerminalCell;
use bevy::prelude::*;

#[derive(Default)]
pub(super) struct SceneScratch {
    pub(super) backgrounds: Vec<QuadInstance>,
    /// Background rectangles in pixel space, so adjoining rows can merge
    /// before conversion to clip-space quads.
    pub(super) background_rects: Vec<(PixelGeometry, LinearRgba)>,
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
    /// Per painted row, its background runs as `[left, right)` pixels and
    /// the background's luminance.
    pub(super) row_backgrounds: Vec<Vec<(f32, f32, f32)>>,
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
        for runs in &mut self.row_backgrounds {
            runs.clear();
        }
    }
}

/// Records a background run, extending an identically aligned run from the
/// previous row into one taller rectangle when possible.
pub(super) fn merge_background_rect(
    rects: &mut Vec<(PixelGeometry, LinearRgba)>,
    prev_runs: &[usize],
    current_runs: &mut Vec<usize>,
    geometry: PixelGeometry,
    color: LinearRgba,
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
    pub(super) color: LinearRgba,
    pub(super) alpha_mask: bool,
    /// A solid rectangle (an opaque block element) rather than atlas texels.
    pub(super) solid: bool,
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
    palette: &Palette,
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
        row_backgrounds,
        ..
    } = scratch;
    repaint.resize(usize::from(height), false);
    painted.resize(usize::from(height), false);
    row_backgrounds.resize_with(usize::from(height), Vec::new);
    let mut painter = RowPainter {
        snapshot,
        config,
        palette,
        raster,
        size,
        full,
        blink,
        cx: &mut *cx,
        shapes: &mut *shapes,
        glyph_atlas: &mut *glyph_atlas,
        stats: &mut *stats,
        styles: &mut *styles,
        styles_row: None,
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
        painter.backgrounds(
            row,
            background_rects,
            prev_runs,
            current_runs,
            &mut row_backgrounds[usize::from(row)],
        );
        std::mem::swap(prev_runs, current_runs);
        let first = placed.len();
        painter.cells(row, placed, Some(decorations));
        painted[usize::from(row)] = true;
        let cell_height = raster.cell_size.y;
        let row_top = f32::from(row) * cell_height;
        let row_reach = placed[first..]
            .iter()
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
            painter.backgrounds(
                row,
                background_rects,
                prev_runs,
                current_runs,
                &mut row_backgrounds[usize::from(row)],
            );
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
    // Each repainted row's part of a glyph is drawn over that row's cell
    // backgrounds; coverage glyphs are split where the background changes
    // so every piece is blended against the background under it.
    let cell_height = raster.cell_size.y;
    #[cfg(test)]
    let mut emitted = vec![
        Vec::new();
        if probe.is_some() && full {
            placed.len()
        } else {
            0
        }
    ];
    #[cfg(test)]
    let mut placed_index = 0;
    for glyph in placed.iter() {
        #[cfg(test)]
        let pieces_before = glyphs.len();
        let first = (glyph.geometry.y / cell_height).floor().max(0.0) as u16;
        let last =
            (((glyph.geometry.y + glyph.geometry.height) / cell_height).ceil() as u16).min(height);
        // Most glyphs lie inside their row and the texture: no clip needed.
        let inside = last == first + 1
            && glyph.geometry.x >= 0.0
            && glyph.geometry.x + glyph.geometry.width <= size.x;
        for row in first..last {
            if !repaint[usize::from(row)] {
                continue;
            }
            let band = PixelGeometry {
                x: 0.0,
                y: f32::from(row) * cell_height,
                width: size.x,
                height: cell_height,
            };
            let clipped = if inside {
                Some((glyph.geometry, glyph.uv))
            } else {
                clip_glyph_to_row(glyph.geometry, glyph.uv, band)
            };
            let Some((piece, uv)) = clipped else {
                continue;
            };
            if glyph.solid {
                glyphs.push((glyph.texture, solid_quad(piece, glyph.color, size)));
                continue;
            }
            if !glyph.alpha_mask {
                glyphs.push((
                    glyph.texture,
                    glyph_quad(piece, uv, glyph.color, false, -1.0, size),
                ));
                continue;
            }
            // Runs are sorted and disjoint: start at the first one the piece
            // reaches and stop past its right edge.
            let runs = &row_backgrounds[usize::from(row)];
            let first = runs.partition_point(|&(_, right, _)| right <= piece.x);
            if let Some(&(left, right, background)) = runs.get(first)
                && left <= piece.x
                && right >= piece.x + piece.width
            {
                glyphs.push((
                    glyph.texture,
                    glyph_quad(piece, uv, glyph.color, true, background, size),
                ));
                continue;
            }
            for &(left, right, background) in &runs[first..] {
                if left >= piece.x + piece.width {
                    break;
                }
                let run = PixelGeometry {
                    x: left,
                    width: right - left,
                    ..band
                };
                if let Some((part, uv)) = clip_glyph_to_row(piece, uv, run) {
                    glyphs.push((
                        glyph.texture,
                        glyph_quad(part, uv, glyph.color, true, background, size),
                    ));
                }
            }
        }
        #[cfg(test)]
        if let Some(pieces) = emitted.get_mut(placed_index) {
            pieces.extend(glyphs[pieces_before..].iter().map(|(_, quad)| {
                let [left, top, right, bottom] = quad.rect.to_array();
                PixelGeometry {
                    x: (left + 1.0) / 2.0 * size.x,
                    y: (1.0 - top) / 2.0 * size.y,
                    width: (right - left) / 2.0 * size.x,
                    height: (top - bottom) / 2.0 * size.y,
                }
            }));
        }
        #[cfg(test)]
        {
            placed_index += 1;
        }
    }
    #[cfg(test)]
    if let Some(probe) = probe.as_mut() {
        for glyph in probe.iter_mut() {
            if let Some(pieces) = emitted.get_mut(glyph.placed) {
                glyph.pieces = std::mem::take(pieces);
            }
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
    let primary_atlas = glyph_atlas.id;
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
    let (atlas_uploads, atlas_fresh) = glyph_atlas.take_uploads();
    BatchScene {
        submission: None,
        destination,
        atlas: glyph_atlas.id,
        atlas_uploads,
        atlas_fresh,
        atlas_lost: glyph_atlas.lost.clone(),
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
    palette: &'a Palette,
    raster: RasterMetrics,
    size: Vec2,
    full: bool,
    blink: BlinkPhases,
    cx: &'a mut TextContext<'w>,
    shapes: &'a mut ShapeCaches,
    glyph_atlas: &'a mut UnifiedGlyphAtlas,
    stats: &'a mut TerminalStats,
    styles: &'a mut Vec<ResolvedStyle>,
    /// The row `styles` currently holds.
    styles_row: Option<u16>,
    #[cfg(test)]
    probe: Option<&'a mut Vec<super::probe::ProbeGlyph>>,
}

impl RowPainter<'_, '_> {
    fn resolve_styles(&mut self, row: u16) {
        if self.styles_row == Some(row) {
            return;
        }
        self.styles_row = Some(row);
        self.styles.clear();
        self.styles.extend(
            self.snapshot
                .row(row)
                .iter()
                .map(|cell| ResolvedStyle::new(cell, self.palette)),
        );
    }

    /// Records the band clear (partial scenes) and background runs of `row`,
    /// merging runs with identical runs of the previous row in full scenes.
    fn backgrounds(
        &mut self,
        row: u16,
        rects: &mut Vec<(PixelGeometry, LinearRgba)>,
        prev_runs: &[usize],
        current_runs: &mut Vec<usize>,
        luminances: &mut Vec<(f32, f32, f32)>,
    ) {
        let raster = self.raster;
        let theme_background = self.palette.background.linear;
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
            // Adjacent runs of equal luminance correct identically: merge them.
            let left = start as f32 * raster.cell_size.x;
            let right = end as f32 * raster.cell_size.x;
            let run_luminance = luminance(color);
            match luminances.last_mut() {
                Some(last) if last.2 == run_luminance && last.1 == left => last.1 = right,
                _ => luminances.push((left, right, run_luminance)),
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
            // A wide cell is drawn as one cell spanning its columns.
            let sprite_metrics = sprite::Metrics {
                cell_width: raster.cell_size.x as u32 * width as u32,
                cell_height: raster.cell_size.y as u32,
                box_thickness: raster.box_thickness,
            };
            let sprite = sprite_codepoint(symbol);
            if let Some([x0, y0, x1, y1]) =
                sprite.and_then(|codepoint| sprite::solid_block(codepoint, sprite_metrics))
            {
                // Opaque single-rectangle block elements are Ghostty's sprite
                // rectangles, drawn as solid quads in the glyphs' paint order
                // (combined quadrants overlap at odd sizes, so they are drawn
                // from the atlas instead).
                placed.push(PlacedGlyph {
                    row,
                    texture: self.glyph_atlas.id,
                    geometry: PixelGeometry {
                        x: cell_x + x0 as f32,
                        y: cell_y + y0 as f32,
                        width: (x1 - x0) as f32,
                        height: (y1 - y0) as f32,
                    },
                    uv: Vec4::ZERO,
                    color: style.foreground,
                    alpha_mask: true,
                    solid: true,
                });
            } else if symbol != " " && !symbol.is_empty() {
                // Sprites span their cells, like Ghostty's `gridWidth`; other
                // runs may use Ghostty's `constraintWidth`.
                let sprite = sprite.is_some();
                let columns = if sprite {
                    width
                } else {
                    visual_columns(cells, column, width)
                };
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
                // Sprites are drawn in cell pixels; text shares the centered
                // baseline and keeps its bearings, pushed in only at the
                // texture's edges.
                let foreground = style.foreground;
                let shift = if sprite {
                    Vec2::ZERO
                } else {
                    Vec2::new(edge_shift(&shaped, cell_x, size.x), raster.glyph_offset)
                };
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
                            sprite,
                            color: !glyph.alpha_mask,
                            texture: glyph.texture,
                            uv: glyph.uv,
                            geometry,
                            placed: placed.len(),
                            pieces: Vec::new(),
                            shift: shift.x,
                        });
                    }
                    placed.push(PlacedGlyph {
                        row,
                        texture: glyph.texture,
                        geometry,
                        uv: glyph.uv,
                        color: foreground,
                        alpha_mask: glyph.alpha_mask,
                        solid: false,
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
    let (left, right) = glyphs.iter().filter(|g| g.ink.1 > g.ink.0).fold(
        (f32::INFINITY, f32::NEG_INFINITY),
        |(l, r), g| {
            (
                l.min(x + g.offset.x + g.ink.0),
                r.max(x + g.offset.x + g.ink.1),
            )
        },
    );
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

pub(super) fn solid_quad(
    geometry: PixelGeometry,
    color: impl Into<LinearRgba>,
    target: Vec2,
) -> QuadInstance {
    QuadInstance {
        rect: clip_rect(snap_geometry(geometry), target),
        // A negative final UV component lets the unified fragment shader skip the atlas sample.
        uv: Vec4::new(0.0, 0.0, 0.0, -1.0),
        color: color.into().to_f32_array().into(),
        background: -1.0,
    }
}

/// A glyph quad; `background` is the luminance of the cell background under
/// it (coverage glyphs are blended with Ghostty's linear correction).
pub(super) fn glyph_quad(
    geometry: PixelGeometry,
    uv: Vec4,
    color: impl Into<LinearRgba>,
    alpha_mask: bool,
    background: f32,
    target: Vec2,
) -> QuadInstance {
    let mut color = color.into().to_f32_array();
    if !alpha_mask {
        color[3] = -1.0;
    }
    QuadInstance {
        rect: clip_rect(snap_geometry(geometry), target),
        uv,
        color: color.into(),
        background,
    }
}

/// Linear luminance of a colour, as Ghostty computes it.
pub(super) fn luminance(color: impl Into<LinearRgba>) -> f32 {
    let linear = color.into();
    0.2126 * linear.red + 0.7152 * linear.green + 0.0722 * linear.blue
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
