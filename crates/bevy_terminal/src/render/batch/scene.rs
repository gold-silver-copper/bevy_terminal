//! CPU scene construction, glyph fitting, and quad geometry.
use super::super::pixel_rect;
use super::quad::Ink;
use super::shaping::{
    CachedGlyph, ShapeCaches, UnifiedGlyphAtlas, cached_shape, is_powerline, is_symbol,
};
use super::sprite::{self, sprite_codepoint};
use super::{
    BatchScene, Blend, BlinkPhases, DrawBatch, Palette, QuadInstance, RasterMetrics, ResolvedStyle,
    TerminalRenderConfig, TerminalSnapshot, TerminalStats, TextContext, cell_span,
    cursor_should_be_visible, terminal_pixel_size,
};
use crate::scene::{StyleFlags, TerminalCell};
use bevy::prelude::*;

#[derive(Default)]
pub(super) struct SceneScratch {
    /// Background rectangles in pixel space, so adjoining rows can merge
    /// before conversion to clip-space quads.
    pub(super) background_rects: Vec<(Rect, LinearRgba)>,
    /// Indices into `background_rects` emitted for the previous row.
    pub(super) prev_runs: Vec<usize>,
    /// Indices into `background_rects` emitted for the current row.
    pub(super) current_runs: Vec<usize>,
    /// Glyphs of the laid-out rows before clipping to the repainted bands.
    pub(super) placed: Vec<PlacedGlyph>,
    pub(super) decorations: Vec<QuadInstance>,
    pub(super) styles: Vec<ResolvedStyle>,
    /// Rows whose bands this scene repaints.
    pub(super) repaint: Vec<bool>,
    /// Rows whose backgrounds, glyphs and decorations have been laid out.
    pub(super) painted: Vec<bool>,
    /// Per painted row, its background runs.
    pub(super) row_backgrounds: Vec<Vec<BackgroundRun>>,
    /// Glyphs of the last full scene before clipping, recorded for the probe.
    #[cfg(test)]
    pub(super) probe: Option<Vec<super::probe::ProbeGlyph>>,
}

impl SceneScratch {
    pub(super) fn clear(&mut self) {
        self.background_rects.clear();
        self.prev_runs.clear();
        self.current_runs.clear();
        self.placed.clear();
        self.decorations.clear();
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
    rects: &mut Vec<(Rect, LinearRgba)>,
    prev_runs: &[usize],
    current_runs: &mut Vec<usize>,
    geometry: Rect,
    color: LinearRgba,
) {
    for &index in prev_runs {
        let (rect, existing) = &mut rects[index];
        if *existing == color
            && rect.min.x == geometry.min.x
            && rect.width() == geometry.width()
            && (rect.max.y - geometry.min.y).abs() < 0.01
        {
            // Anchor the merged bottom edge on the current row's own geometry
            // so accumulated float error cannot drift the rectangle.
            rect.max.y = geometry.max.y;
            current_runs.push(index);
            return;
        }
    }
    rects.push((geometry, color));
    current_runs.push(rects.len() - 1);
}

/// A run of a row's cells whose backgrounds share one luminance, as
/// `[left, right)` texture pixels.
#[derive(Clone, Copy, Debug)]
pub(super) struct BackgroundRun {
    left: f32,
    right: f32,
    /// Linear luminance, against which glyphs over the run are corrected.
    luminance: f32,
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

/// What the last layout of each row left behind: how far its ink reaches,
/// and whether it holds blinking text, with a count of such rows so blink
/// phases can be skipped without scanning the grid.
#[derive(Default)]
pub(super) struct RowStates {
    rows: Vec<RowState>,
    blinking: usize,
}

#[derive(Clone, Copy, Default)]
struct RowState {
    reach: RowReach,
    blinks: bool,
}

impl RowStates {
    /// Forgets every row; `height` rows start with no reach and no blinking.
    fn reset(&mut self, height: u16) {
        self.rows.clear();
        self.rows.resize(usize::from(height), RowState::default());
        self.blinking = 0;
    }

    pub(super) fn reach(&self, row: u16) -> RowReach {
        self.rows[usize::from(row)].reach
    }

    fn set(&mut self, row: u16, reach: RowReach, blinks: bool) {
        let state = &mut self.rows[usize::from(row)];
        self.blinking = self.blinking + usize::from(blinks) - usize::from(state.blinks);
        *state = RowState { reach, blinks };
    }

    /// Whether any row holds `SLOW_BLINK` or `RAPID_BLINK` text.
    pub(super) fn any_blinking(&self) -> bool {
        self.blinking > 0
    }

    #[cfg(test)]
    pub(super) fn reaches(&self) -> impl Iterator<Item = RowReach> + '_ {
        self.rows.iter().map(|row| row.reach)
    }
}

/// A glyph bitmap placed in texture pixels, before clipping.
#[derive(Clone, Copy, Debug)]
pub(super) struct PlacedGlyph {
    pub(super) row: u16,
    pub(super) texture: AssetId<Image>,
    pub(super) geometry: Rect,
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
    rows: &mut RowStates,
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
    if full || rows.rows.len() != usize::from(height) {
        // Nothing drawn before a full scene survives it.
        rows.reset(height);
    }
    let SceneScratch {
        background_rects,
        prev_runs,
        current_runs,
        placed,
        decorations,
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
        for reached in rows.reach(row).rows(row, height) {
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
                    glyph.geometry.min.y - row_top,
                    glyph.geometry.max.y - row_top,
                    cell_height,
                ))
            });
        let blinks = painter
            .styles
            .iter()
            .any(|style| style.any(StyleFlags::SLOW_BLINK | StyleFlags::RAPID_BLINK));
        rows.set(row, row_reach, blinks);
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
        let row_reach = rows.reach(row);
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
    // Quads are written once, in paint order: backgrounds (whose runs are
    // all known by now), glyphs, decorations, the cursor.
    let primary_atlas = glyph_atlas.id;
    let mut quads =
        SceneQuads::with_capacity(background_rects.len() + placed.len() + decorations.len() + 1);
    quads.extend(
        primary_atlas,
        Blend::Replace,
        background_rects
            .iter()
            .map(|&(geometry, color)| QuadInstance::solid(geometry, color, size)),
    );
    let background_quads = quads.instances.len();

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
        let pieces_before = quads.instances.len();
        let first = (glyph.geometry.min.y / cell_height).floor().max(0.0) as u16;
        let last = ((glyph.geometry.max.y / cell_height).ceil() as u16).min(height);
        // Most glyphs lie inside their row and the texture: no clip needed.
        let inside =
            last == first + 1 && glyph.geometry.min.x >= 0.0 && glyph.geometry.max.x <= size.x;
        for row in first..last {
            if !repaint[usize::from(row)] {
                continue;
            }
            let band = pixel_rect(0.0, f32::from(row) * cell_height, size.x, cell_height);
            let clipped = if inside {
                Some((glyph.geometry, glyph.uv))
            } else {
                clip_glyph_to_row(glyph.geometry, glyph.uv, band)
            };
            let Some((piece, uv)) = clipped else {
                continue;
            };
            if glyph.solid {
                quads.push(
                    glyph.texture,
                    Blend::Alpha,
                    QuadInstance::solid(piece, glyph.color, size),
                );
                continue;
            }
            if !glyph.alpha_mask {
                quads.push(
                    glyph.texture,
                    Blend::Alpha,
                    QuadInstance::glyph(piece, uv, glyph.color, Ink::Color, size),
                );
                continue;
            }
            // Runs are sorted and disjoint: start at the first one the piece
            // reaches and stop past its right edge.
            let runs = &row_backgrounds[usize::from(row)];
            let first = runs.partition_point(|run| run.right <= piece.min.x);
            if let Some(&BackgroundRun {
                left,
                right,
                luminance: background,
            }) = runs.get(first)
                && left <= piece.min.x
                && right >= piece.max.x
            {
                quads.push(
                    glyph.texture,
                    Blend::Alpha,
                    QuadInstance::glyph(piece, uv, glyph.color, Ink::Coverage { background }, size),
                );
                continue;
            }
            for &BackgroundRun {
                left,
                right,
                luminance: background,
            } in &runs[first..]
            {
                if left >= piece.max.x {
                    break;
                }
                let run = Rect {
                    min: Vec2::new(left, band.min.y),
                    max: Vec2::new(right, band.max.y),
                };
                if let Some((part, uv)) = clip_glyph_to_row(piece, uv, run) {
                    quads.push(
                        glyph.texture,
                        Blend::Alpha,
                        QuadInstance::glyph(
                            part,
                            uv,
                            glyph.color,
                            Ink::Coverage { background },
                            size,
                        ),
                    );
                }
            }
        }
        #[cfg(test)]
        if let Some(pieces) = emitted.get_mut(placed_index) {
            pieces.extend(quads.instances[pieces_before..].iter().map(|quad| {
                let [left, top, right, bottom] = quad.rect();
                pixel_rect(
                    (left + 1.0) / 2.0 * size.x,
                    (1.0 - top) / 2.0 * size.y,
                    (right - left) / 2.0 * size.x,
                    (top - bottom) / 2.0 * size.y,
                )
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

    quads.extend(primary_atlas, Blend::Alpha, decorations.iter().copied());
    let mut solid_quads = background_quads + decorations.len();

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
        solid_quads += 1;
        quads.push(
            primary_atlas,
            Blend::Alpha,
            QuadInstance::solid(
                pixel_rect(
                    f32::from(position.x) * raster.cell_size.x + x,
                    f32::from(position.y) * raster.cell_size.y + y,
                    width,
                    height,
                ),
                config.cursor.color,
                size,
            ),
        );
    }

    #[cfg(test)]
    {
        scratch.probe = probe;
    }
    stats.changed_rows = u32::try_from(painted_rows).unwrap_or(u32::MAX);
    stats.solid_quads = u32::try_from(solid_quads).unwrap_or(u32::MAX);
    stats.glyph_quads = u32::try_from(quads.instances.len() - solid_quads).unwrap_or(u32::MAX);
    let SceneQuads { instances, batches } = quads;
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
        rects: &mut Vec<(Rect, LinearRgba)>,
        prev_runs: &[usize],
        current_runs: &mut Vec<usize>,
        luminances: &mut Vec<BackgroundRun>,
    ) {
        let raster = self.raster;
        let theme_background = self.palette.background.linear;
        if !self.full {
            // Partial repaints interleave a per-row clear with that row's runs,
            // so later rows' clears would overwrite runs merged upward; merge
            // vertically only in full rebuilds (which have no per-row clears).
            rects.push((
                pixel_rect(
                    0.0,
                    f32::from(row) * raster.cell_size.y,
                    self.size.x,
                    raster.cell_size.y,
                ),
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
                Some(last) if last.luminance == run_luminance && last.right == left => {
                    last.right = right;
                }
                _ => luminances.push(BackgroundRun {
                    left,
                    right,
                    luminance: run_luminance,
                }),
            }
            if !(self.full && color == theme_background) {
                let geometry = pixel_rect(
                    start as f32 * raster.cell_size.x,
                    f32::from(row) * raster.cell_size.y,
                    (end - start) as f32 * raster.cell_size.x,
                    raster.cell_size.y,
                );
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
            if style.any(StyleFlags::HIDDEN) || self.blink.hides(style) {
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
                    geometry: pixel_rect(
                        cell_x + x0 as f32,
                        cell_y + y0 as f32,
                        (x1 - x0) as f32,
                        (y1 - y0) as f32,
                    ),
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
                    style.face(),
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
                    let geometry = pixel_rect(
                        cell_x + glyph.offset.x + shift.x,
                        cell_y + glyph.offset.y + shift.y,
                        glyph.size.x,
                        glyph.size.y,
                    );
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
                if style.any(StyleFlags::UNDERLINED) {
                    decorations.push(QuadInstance::solid(
                        pixel_rect(
                            decoration_x,
                            cell_y + (raster.cell_size.y - 2.0 * decoration_thickness).max(0.0),
                            decoration_width,
                            decoration_thickness,
                        ),
                        style.underline,
                        size,
                    ));
                }
                if style.any(StyleFlags::CROSSED_OUT) {
                    decorations.push(QuadInstance::solid(
                        pixel_rect(
                            decoration_x,
                            cell_y + raster.cell_size.y * 0.55,
                            decoration_width,
                            decoration_thickness,
                        ),
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
    let (left, right) = glyphs.iter().filter(|g| !g.ink.is_empty()).fold(
        (f32::INFINITY, f32::NEG_INFINITY),
        |(l, r), g| {
            (
                l.min(x + g.offset.x + g.ink.left),
                r.max(x + g.offset.x + g.ink.right),
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

/// Linear luminance of a colour, as Ghostty computes it.
pub(super) fn luminance(color: impl Into<LinearRgba>) -> f32 {
    let linear = color.into();
    0.2126 * linear.red + 0.7152 * linear.green + 0.0722 * linear.blue
}

/// Crops a glyph quad to the band it is drawn in (its row, or its cells for
/// grid graphics), adjusting its atlas coordinates so the visible texels stay
/// in place.
pub(super) fn clip_glyph_to_row(glyph: Rect, uv: Vec4, cell: Rect) -> Option<(Rect, Vec4)> {
    if glyph.width() <= 0.0 || glyph.height() <= 0.0 || cell.width() <= 0.0 || cell.height() <= 0.0
    {
        return None;
    }
    let clipped = glyph.intersect(cell);
    if clipped.is_empty() {
        return None;
    }
    let u_span = uv.z - uv.x;
    let v_span = uv.w - uv.y;
    let clipped_uv = Vec4::new(
        ((clipped.min.x - glyph.min.x) / glyph.width()).mul_add(u_span, uv.x),
        ((clipped.min.y - glyph.min.y) / glyph.height()).mul_add(v_span, uv.y),
        ((clipped.max.x - glyph.min.x) / glyph.width()).mul_add(u_span, uv.x),
        ((clipped.max.y - glyph.min.y) / glyph.height()).mul_add(v_span, uv.y),
    );
    Some((clipped, clipped_uv))
}

/// Rounds a rectangle's edges to whole pixels, as the GPU rasterizes them.
pub(super) fn snap_geometry(geometry: Rect) -> IRect {
    let left = super::metrics::snap(geometry.min.x);
    let top = super::metrics::snap(geometry.min.y);
    let right = super::metrics::snap(geometry.max.x).max(left);
    let bottom = super::metrics::snap(geometry.max.y).max(top);
    IRect::new(left as i32, top as i32, right as i32, bottom as i32)
}

/// A whole-pixel rectangle of a `target`-sized texture in clip space, as
/// `[left, top, right, bottom]`.
pub(super) fn clip_rect(geometry: IRect, target: Vec2) -> Vec4 {
    let rect = geometry.as_rect();
    let left = rect.min.x / target.x * 2.0 - 1.0;
    let right = rect.max.x / target.x * 2.0 - 1.0;
    let top = 1.0 - rect.min.y / target.y * 2.0;
    let bottom = 1.0 - rect.max.y / target.y * 2.0;
    Vec4::new(left, top, right, bottom)
}

/// A scene's quads in paint order and the draw batches over them. Each
/// quad is written once; it joins the previous batch when that uses the
/// same texture and blending.
pub(super) struct SceneQuads {
    pub(super) instances: Vec<QuadInstance>,
    pub(super) batches: Vec<DrawBatch>,
}

impl SceneQuads {
    pub(super) fn with_capacity(quads: usize) -> Self {
        Self {
            instances: Vec::with_capacity(quads),
            batches: Vec::new(),
        }
    }

    pub(super) fn push(&mut self, texture: AssetId<Image>, blend: Blend, quad: QuadInstance) {
        let index = self.instances.len() as u32;
        self.instances.push(quad);
        match self.batches.last_mut() {
            Some(batch)
                if batch.texture == texture
                    && batch.blend == blend
                    && batch.start + batch.count == index =>
            {
                batch.count += 1;
            }
            _ => self.batches.push(DrawBatch {
                texture,
                start: index,
                count: 1,
                blend,
            }),
        }
    }

    pub(super) fn extend(
        &mut self,
        texture: AssetId<Image>,
        blend: Blend,
        quads: impl IntoIterator<Item = QuadInstance>,
    ) {
        for quad in quads {
            self.push(texture, blend, quad);
        }
    }
}
