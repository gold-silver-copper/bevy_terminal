//! CPU scene construction, glyph fitting, and quad geometry.
use super::shaping::{CachedGlyph, ShapeCaches, UnifiedGlyphAtlas, cached_shape};
use super::{
    BatchScene, BlinkPhases, DrawBatch, PixelGeometry, QuadInstance, RasterMetrics, ResolvedStyle,
    TerminalRenderConfig, TerminalSnapshot, TerminalStats, TextContext, cell_span,
    cursor_should_be_visible, terminal_pixel_size,
};
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
    pub(super) decorations: Vec<QuadInstance>,
    pub(super) cursor: Vec<QuadInstance>,
    pub(super) styles: Vec<ResolvedStyle>,
}

impl SceneScratch {
    pub(super) fn clear(&mut self) {
        self.backgrounds.clear();
        self.background_rects.clear();
        self.prev_runs.clear();
        self.current_runs.clear();
        self.glyphs.clear();
        self.decorations.clear();
        self.cursor.clear();
        self.styles.clear();
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

#[allow(clippy::too_many_arguments)]
pub(super) fn build_scene(
    snapshot: &TerminalSnapshot,
    config: &TerminalRenderConfig,
    raster: RasterMetrics,
    rows: &[u16],
    full: bool,
    destination: AssetId<Image>,
    cx: &mut TextContext<'_>,
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
            if let Some(rects) = block_element(symbol) {
                let cell_x = column as f32 * raster.cell_size.x;
                let cell_y = f32::from(row) * raster.cell_size.y;
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
            } else if symbol != " " && !symbol.is_empty() {
                let shaped = cached_shape(
                    symbol,
                    style,
                    config,
                    raster,
                    size,
                    cx,
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
                    fit_horizontally(&shaped, cell_bounds.width),
                    raster.glyph_offset,
                );
                for glyph in shaped.iter() {
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
pub(super) fn fit_horizontally(glyphs: &[CachedGlyph], span: f32) -> f32 {
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
            super::metrics::snap(-left)
        } else if right > span {
            super::metrics::snap(span - right)
        } else {
            0.0
        }
    } else {
        // Try every whole shift that keeps the run covering the span and keep the
        // one retaining the most coverage; ties resolve toward the centered shift.
        let centered = super::metrics::snap((span - (right - left)) / 2.0 - left);
        let lowest = super::metrics::snap(span - right);
        let highest = super::metrics::snap(-left);
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

pub(super) fn clip_glyph_to_cell(
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
