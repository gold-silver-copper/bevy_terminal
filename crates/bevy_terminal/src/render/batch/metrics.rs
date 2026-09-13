//! Font measurement and effective raster geometry.
use super::shaping::shape_run;
use super::{ResolvedStyle, TerminalRenderConfig, TextContext};
use crate::render::{FontFaces, TerminalSizing};
use bevy::{
    prelude::*,
    text::{
        ComputedTextBlock, FontCx, FontSource, LayoutCx, LetterSpacing, LineHeight, TextPipeline,
    },
};

/// Number of probe glyphs shaped by [`measure_advance`].
const PROBE_GLYPHS: usize = 100;
/// Font size in logical pixels used to shape the probe run.
pub(crate) const PROBE_FONT_SIZE: f32 = 64.0;
/// Font size used while [`TerminalSizing::FitCellWidth`] has not been measured yet.
const UNMEASURED_FONT_SIZE: f32 = 16.0;

/// Measures the average advance of the regular font at [`PROBE_FONT_SIZE`] by
/// shaping a run of `0` glyphs, preserving the reason measurement failed.
pub(in crate::render) fn measure_advance(
    faces: &FontFaces,
    fonts: &Assets<Font>,
    text_pipeline: &mut TextPipeline,
    font_cx: &mut FontCx,
    layout_cx: &mut LayoutCx,
) -> Result<f32, String> {
    // A font asset is only usable once Bevy has registered it with the font
    // context (which assigns its alias); measuring before that would shape a
    // fallback font. Report "not yet" so the caller retries next frame.
    if let FontSource::Handle(handle) = &faces.regular
        && fonts
            .get(handle.id())
            .is_none_or(|font| font.alias.is_empty())
    {
        return Err("font asset has not registered".into());
    }
    let font = TextFont {
        font: faces.regular.clone(),
        font_size: PROBE_FONT_SIZE.into(),
        ..default()
    };
    let probe = "0".repeat(PROBE_GLYPHS);
    let mut computed = ComputedTextBlock::default();
    let measure = text_pipeline
        .create_text_measure(
            Entity::PLACEHOLDER,
            fonts,
            std::iter::once((
                Entity::PLACEHOLDER,
                0,
                probe.as_str(),
                &font,
                Color::WHITE,
                LineHeight::Px(PROBE_FONT_SIZE),
                LetterSpacing::default(),
            )),
            1.0,
            &TextLayout::new(Justify::Left, LineBreak::NoWrap),
            &mut computed,
            font_cx,
            layout_cx,
            Vec2::new(f32::MAX, f32::MAX),
            20.0,
        )
        .map_err(|error| error.to_string())?;
    let advance = measure.max.x / PROBE_GLYPHS as f32;
    if advance.is_finite() && advance > 0.0 {
        Ok(advance)
    } else {
        Err(format!("font produced invalid advance {advance}"))
    }
}

/// Logical metrics resolved from a configuration and a measured advance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LogicalMetrics {
    /// Logical font size to rasterize with.
    pub(crate) font_size: f32,
    /// Logical cell size.
    pub(crate) cell_size: Vec2,
}

/// Resolves the logical font and cell size for `config`.
///
/// `measured_advance` is the regular font's advance at [`PROBE_FONT_SIZE`]
/// (`None` until it could be measured).
pub(in crate::render) fn resolve_metrics(
    config: &TerminalRenderConfig,
    measured_advance: Option<f32>,
) -> LogicalMetrics {
    let advance_per_px = measured_advance.map(|advance| advance / PROBE_FONT_SIZE);
    match config.sizing {
        TerminalSizing::Fixed {
            cell_size,
            font_size,
        } => LogicalMetrics {
            font_size: font_size.max(1.0),
            cell_size,
        },
        TerminalSizing::FitCellWidth(cell_size) => LogicalMetrics {
            font_size: advance_per_px
                .map_or(UNMEASURED_FONT_SIZE, |ratio| (cell_size.x / ratio).max(1.0)),
            cell_size,
        },
        TerminalSizing::FromFont { font_size, .. } => {
            let font_size = font_size.max(1.0);
            let cell_size = advance_per_px.map_or(Vec2::ONE, |ratio| {
                Vec2::new((ratio * font_size).max(1.0), 1.0)
            });
            LogicalMetrics {
                font_size,
                cell_size,
            }
        }
    }
}

/// Vertical ink extents of a shaped probe run, in physical pixels measured
/// from the top of a cell-height line box (`top` may be negative and `bottom`
/// may exceed the cell height when the run does not fit).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GlyphBox {
    pub(crate) top: f32,
    pub(crate) bottom: f32,
}

impl GlyphBox {
    /// Height of the box in pixels.
    pub(crate) fn height(self) -> f32 {
        self.bottom - self.top
    }

    /// Union of two boxes.
    pub(crate) fn union(self, other: GlyphBox) -> GlyphBox {
        GlyphBox {
            top: self.top.min(other.top),
            bottom: self.bottom.max(other.bottom),
        }
    }
}

/// Cell height (whole physical pixels) that shows the primary font's line
/// box: the configured height, grown to the full block glyph's box when that
/// is taller. Font-driven cells start from the font's own line box (ascent +
/// descent + leading, read from its metrics tables), so a font whose block
/// glyph is shorter than its line box (DejaVu Sans Mono, Menlo) still gets a
/// row tall enough for its ascenders and descenders.
pub(crate) fn fitted_cell_height(cell_height: f32, block: Option<GlyphBox>) -> f32 {
    block
        .map(|block| block.height().ceil())
        .filter(|height| *height > cell_height)
        .unwrap_or(cell_height)
}

/// Uniform vertical shift (whole physical pixels) applied to every glyph of a
/// terminal, chosen from measured boxes in priority order:
///
/// 1. a full block that is at least cell-high keeps covering the cell (tiles of
///    blocks stay seamless);
/// 2. the `core` ink box (ASCII ascenders, descenders and brackets) stays
///    inside the cell;
/// 3. the `accents` ink box (accented capitals) stays inside the cell.
///
/// Within the freedom left by higher priorities the core box is centered. A
/// box that cannot fit at all is skipped, so an accent designed to overshoot
/// the line box clips at the top rather than pushing descenders out.
pub(crate) fn vertical_offset(
    cell_height: f32,
    block: Option<GlyphBox>,
    core: Option<GlyphBox>,
    accents: Option<GlyphBox>,
) -> f32 {
    let mut low = f32::NEG_INFINITY;
    let mut high = f32::INFINITY;
    let mut narrow = |range_low: f32, range_high: f32| {
        if range_low <= high && range_high >= low {
            low = low.max(range_low);
            high = high.min(range_high);
        }
    };
    if let Some(block) = block
        && block.height() >= cell_height
    {
        narrow(cell_height - block.bottom, -block.top);
    }
    for ink in [core, accents].into_iter().flatten() {
        if ink.height() <= cell_height {
            narrow(-ink.top, cell_height - ink.bottom);
        }
    }
    let target = core
        .or(accents)
        .map_or(0.0, |ink| (cell_height - ink.height()) / 2.0 - ink.top);
    if low.is_finite() && high.is_finite() {
        snap(target.clamp(low, high))
    } else if low.is_finite() {
        snap(target.max(low))
    } else if high.is_finite() {
        snap(target.min(high))
    } else {
        snap(target)
    }
}

/// Rounds to the nearest whole pixel, halves toward +∞ — unlike
/// `f32::round`, which rounds halves away from zero and would shift a glyph
/// at `-0.5` and one at `+0.5` in opposite directions, so an integer
/// translation of a whole layout stays an integer translation of every glyph.
pub(crate) fn snap(value: f32) -> f32 {
    (value + 0.5).floor()
}

/// Physical raster metrics derived from the logical configuration.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct RasterMetrics {
    /// Physical pixels per logical pixel.
    pub(super) scale: f32,
    /// Physical cell size, snapped to whole pixels.
    pub(super) cell_size: Vec2,
    /// Physical font size. Cells snap to whole physical pixels, but the font
    /// size stays fractional so a font can be sized to make its advance fill
    /// the cell exactly.
    pub(super) font_size: f32,
    /// Primary face's snapped baseline before the uniform text offset.
    pub(super) baseline: f32,
    /// Uniform vertical shift of ordinary text, enclosing the configured
    /// faces' typographic ascent/descent in whole physical pixels.
    pub(super) glyph_offset: f32,
    /// Box-drawing glyphs retain their grid alignment independently of text.
    pub(super) box_offset: f32,
}

pub(super) fn physical_config(logical: LogicalMetrics, raster_scale: f32) -> RasterMetrics {
    RasterMetrics {
        scale: raster_scale,
        cell_size: (logical.cell_size * raster_scale).round().max(Vec2::ONE),
        font_size: (logical.font_size * raster_scale).max(1.0),
        baseline: 0.0,
        glyph_offset: 0.0,
        box_offset: 0.0,
    }
}

pub(super) fn font_size_for_cell(
    config: &TerminalRenderConfig,
    measured_advance: Option<f32>,
    raster: RasterMetrics,
) -> f32 {
    let fit_width = config.sizing.needs_advance();
    measured_advance
        .filter(|advance| fit_width && *advance > 0.0)
        .map_or(raster.font_size, |advance| {
            (raster.cell_size.x * PROBE_FONT_SIZE / advance).max(1.0)
        })
}

/// The regular face's line box (ascent + descent + leading) in physical
/// pixels at `font_size`, read from the font's metrics tables the way
/// terminal emulators size their rows (`OS/2` typographic metrics when the
/// font asks for them, `hhea` otherwise). `None` when the face cannot be
/// resolved or loaded, in which case the block-glyph measurement stands alone.
pub(super) fn font_line_box(
    config: &TerminalRenderConfig,
    font_size: f32,
    fonts: &Assets<Font>,
    font_cx: &mut FontCx,
) -> Option<f32> {
    use skrifa::MetadataProvider as _;
    // A font asset is registered in the collection under its alias; every
    // other source resolves through Bevy's generic-family mapping.
    let family = match &config.font.regular {
        FontSource::Handle(handle) => fonts.get(handle.id()).map(|font| font.alias.clone()),
        source => font_cx.get_family(source).map(str::to_owned),
    };
    let Some(family) = family.filter(|family| !family.is_empty()) else {
        debug!(
            "bevy_terminal: line box: no family for {:?}",
            config.font.regular
        );
        return None;
    };
    let Some(family_info) = font_cx.context.collection.family_by_name(&family) else {
        debug!("bevy_terminal: line box: family {family:?} not in the collection");
        return None;
    };
    let Some(info) = family_info.default_font().cloned() else {
        debug!("bevy_terminal: line box: family {family:?} has no fonts");
        return None;
    };
    let Some(blob) = info.load(Some(&mut font_cx.context.source_cache)) else {
        debug!("bevy_terminal: line box: font data of {family:?} could not be loaded");
        return None;
    };
    let Ok(font) = skrifa::FontRef::from_index(blob.as_ref(), info.index()) else {
        debug!("bevy_terminal: line box: font data of {family:?} could not be parsed");
        return None;
    };
    let metrics = font.metrics(
        skrifa::instance::Size::new(font_size),
        skrifa::instance::LocationRef::default(),
    );
    let height = metrics.ascent + metrics.descent.abs() + metrics.leading.max(0.0);
    debug!(
        "bevy_terminal: line box of {family:?} at {font_size:.2}px: ascent {} descent {} leading {} upem {} -> {height:.2}px",
        metrics.ascent, metrics.descent, metrics.leading, metrics.units_per_em
    );
    (height.is_finite() && height > 0.0).then_some(height)
}

/// ASCII glyphs whose ink must stay inside a cell: descenders, ascenders and
/// tall brackets. Measured for every configured face.
pub(super) const CORE_PROBE: &str = "gjpqy|[]{}()_";
/// Accented capitals: kept inside the cell when the core box leaves room.
pub(super) const ACCENT_PROBE: &str = "\u{c5}\u{c9}\u{1eaa}";
/// The font's full-block outline supplies the legacy box-drawing alignment.
/// It is separate from both typographic metrics and procedural block rendering.
pub(super) const BLOCK_PROBE: &str = "\u{2588}";
/// Upper bound on cell-height refinement rounds.
pub(super) const FIT_ROUNDS: usize = 3;

/// Refines the physical metrics after the logical fit: sizes the font from the
/// rounded physical cell width (so a fractional raster scale cannot open seams
/// between advances), and encloses the configured faces' typographic metrics
/// in whole pixels. Ordinary text uses that enclosure for its vertical shift;
/// box-drawing retains its separately measured grid alignment.
pub(super) fn refine_metrics(
    config: &TerminalRenderConfig,
    measured_advance: Option<f32>,
    mut raster: RasterMetrics,
    cx: &mut TextContext<'_>,
) -> RasterMetrics {
    raster.font_size = font_size_for_cell(config, measured_advance, raster);
    let requested_height = raster.cell_size.y;
    // An explicit font size in an explicit cell is honored exactly; a font
    // derived from the cell width or a font-driven cell gets a cell at least
    // as tall as the font's line box.
    let font_driven = matches!(config.sizing, super::super::TerminalSizing::FromFont { .. });
    let line_height = config.sizing.line_height();
    // The font's own line box (ascent + descent + leading), scaled by the
    // configured line height, is the floor for a font-driven cell, the way
    // terminal emulators size rows: a block glyph shorter than the line box
    // (Menlo, DejaVu Sans Mono) must not collapse the row onto the
    // neighbouring rows' ascenders and descenders. A multiplier below one
    // asks for exactly that tighter row, so the block glyph does not grow it
    // back (block elements are geometry and tile regardless).
    let may_grow = matches!(config.sizing, super::super::TerminalSizing::FitCellWidth(_))
        || (font_driven && line_height >= 1.0);
    if (may_grow || font_driven)
        && let Some(line_box) = font_line_box(config, raster.font_size, cx.fonts, cx.font_cx)
    {
        raster.cell_size.y = raster.cell_size.y.max((line_box * line_height).ceil());
    }
    let mut block = None;
    let mut text_box: Option<GlyphBox> = None;
    for _ in 0..FIT_ROUNDS {
        text_box = None;
        for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
            let mut style = ResolvedStyle::plain();
            style.bold = bold;
            style.italic = italic;
            if let Some(run) = shape_run("0", &style, config, raster, Vec2::splat(4096.0), cx) {
                if !bold && !italic {
                    raster.baseline = run.baseline;
                }
                let shift = raster.baseline - run.baseline;
                let line_box = GlyphBox {
                    top: run.line_box.top + shift,
                    bottom: run.line_box.bottom + shift,
                };
                text_box = Some(text_box.map_or(line_box, |box_| box_.union(line_box)));
            }
        }
        // The block's fully opaque rows are what tiles seamlessly; its anti-aliased
        // edge rows are excluded (falling back to the bitmap minus one row per side).
        block = shape_boxes(BLOCK_PROBE, &ResolvedStyle::plain(), config, raster, cx).map(
            |(bitmap, opaque)| {
                opaque.unwrap_or(GlyphBox {
                    top: bitmap.top + 1.0,
                    bottom: (bitmap.bottom - 1.0).max(bitmap.top + 1.0),
                })
            },
        );
        let height = fitted_cell_height(raster.cell_size.y, block)
            .max(text_box.map_or(0.0, |box_| box_.height()));
        if !may_grow || height == raster.cell_size.y {
            break;
        }
        // The line box is centered in the cell-high line, so re-measure at the new height.
        raster.cell_size.y = height;
    }
    if raster.cell_size.y != requested_height {
        debug!(
            "bevy_terminal: cell height grown from {}px to {}px to fit the font's line box",
            requested_height, raster.cell_size.y
        );
    }
    let mut boxes = [None, None];
    for (probe, slot) in [CORE_PROBE, ACCENT_PROBE].into_iter().zip(&mut boxes) {
        for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
            if (bold && config.font.bold.is_none() && !config.font.synthesize)
                || (italic && config.font.italic.is_none() && !config.font.synthesize)
            {
                continue;
            }
            let mut style = ResolvedStyle::plain();
            style.bold = bold;
            style.italic = italic;
            let Some(measured) = shape_box(probe, &style, config, raster, cx) else {
                continue;
            };
            *slot = Some(slot.map_or(measured, |union: GlyphBox| union.union(measured)));
        }
    }
    let [core, accents] = boxes;
    raster.box_offset = vertical_offset(raster.cell_size.y, block, core, accents);
    raster.glyph_offset = vertical_offset(raster.cell_size.y, None, text_box, None);
    debug!(
        "bevy_terminal: cell {}x{}px font {:.2}px block {:?} core {:?} accents {:?} offset {}",
        raster.cell_size.x,
        raster.cell_size.y,
        raster.font_size,
        block,
        core,
        accents,
        raster.glyph_offset
    );
    raster
}

/// Shapes `text` exactly as [`cached_shape`] does and returns the vertical
/// extent of its glyph bitmaps relative to the line box top.
pub(super) fn shape_box(
    text: &str,
    style: &ResolvedStyle,
    config: &TerminalRenderConfig,
    raster: RasterMetrics,
    cx: &mut TextContext<'_>,
) -> Option<GlyphBox> {
    shape_boxes(text, style, config, raster, cx).map(|(bitmap, _)| bitmap)
}

/// Like [`shape_box`], but also returns the rows of the run's bitmaps that are
/// fully opaque across their width (the coverage a block glyph guarantees; its
/// first and last bitmap rows are usually anti-aliased edges), when the atlas
/// data is readable.
pub(super) fn shape_boxes(
    text: &str,
    style: &ResolvedStyle,
    config: &TerminalRenderConfig,
    raster: RasterMetrics,
    cx: &mut TextContext<'_>,
) -> Option<(GlyphBox, Option<GlyphBox>)> {
    let layout = shape_run(text, style, config, raster, Vec2::splat(4096.0), cx)?;
    let mut bitmap: Option<GlyphBox> = None;
    let mut opaque: Option<GlyphBox> = None;
    for glyph in &layout.glyphs {
        let rect = glyph.atlas_info.rect;
        let height = rect.size().y;
        let top = snap(glyph.position.y - height * 0.5);
        let glyph_box = GlyphBox {
            top,
            bottom: top + height,
        };
        bitmap = Some(bitmap.map_or(glyph_box, |b| b.union(glyph_box)));
        if let Some(rows) = opaque_rows(cx.images, glyph.atlas_info.texture, rect) {
            let rows = GlyphBox {
                top: top + rows.0 as f32,
                bottom: top + rows.1 as f32,
            };
            opaque = Some(opaque.map_or(rows, |b| b.union(rows)));
        }
    }
    bitmap.map(|bitmap| (bitmap, opaque))
}

/// Sum of alpha over each column of an atlas glyph (all `u32::MAX` when the
/// atlas has no CPU data, so every column counts as inked).
pub(super) fn column_coverage(image: &Image, rect: Rect) -> Vec<u32> {
    let width = rect.size().x.max(0.0) as usize;
    let Some(data) = image.data.as_ref() else {
        return vec![u32::MAX; width];
    };
    let atlas_width = image.texture_descriptor.size.width as usize;
    let (x0, y0, y1) = (
        rect.min.x as usize,
        rect.min.y as usize,
        rect.max.y as usize,
    );
    (0..width)
        .map(|x| {
            (y0..y1)
                .map(|y| {
                    data.get((y * atlas_width + x0 + x) * 4 + 3)
                        .map_or(0, |alpha| u32::from(*alpha))
                })
                .sum()
        })
        .collect()
}

/// The half-open row range `[first, last)` of an atlas glyph whose alpha is
/// fully opaque across the glyph's width; `None` if the atlas has no CPU data
/// or no such row.
pub(super) fn opaque_rows(
    images: &Assets<Image>,
    texture: AssetId<Image>,
    rect: Rect,
) -> Option<(u32, u32)> {
    let image = images.get(texture)?;
    let data = image.data.as_ref()?;
    let width = image.texture_descriptor.size.width as usize;
    let (x0, x1) = (rect.min.x as usize, rect.max.x as usize);
    let (y0, y1) = (rect.min.y as usize, rect.max.y as usize);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let row_opaque = |y: usize| {
        (x0..x1).all(|x| {
            data.get((y * width + x) * 4 + 3)
                .is_some_and(|alpha| *alpha >= 250)
        })
    };
    let first = (y0..y1).find(|y| row_opaque(*y))?;
    let last = (first..y1).take_while(|y| row_opaque(*y)).last()?;
    Some(((first - y0) as u32, (last + 1 - y0) as u32))
}
