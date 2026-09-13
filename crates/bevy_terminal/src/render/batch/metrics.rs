//! Font measurement and effective raster geometry.
use super::{ResolvedStyle, TerminalRenderConfig, TextContext, shape_run};
use bevy::{prelude::*, text::FontCx};

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
    /// Uniform vertical shift applied to every glyph, in whole physical
    /// pixels, so the primary font's line box sits inside the cell (see
    /// [`super::super::vertical_offset`]).
    pub(super) glyph_offset: f32,
}

pub(super) fn physical_config(
    logical: super::super::LogicalMetrics,
    raster_scale: f32,
) -> RasterMetrics {
    RasterMetrics {
        scale: raster_scale,
        cell_size: (logical.cell_size * raster_scale).round().max(Vec2::ONE),
        font_size: (logical.font_size * raster_scale).max(1.0),
        glyph_offset: 0.0,
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
            (raster.cell_size.x * super::super::PROBE_FONT_SIZE / advance).max(1.0)
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
/// A full block: its box is the font's line box, which sizes the cell and is
/// kept covering the cell so tiles stay seamless.
pub(super) const BLOCK_PROBE: &str = "\u{2588}";
/// Upper bound on cell-height refinement rounds.
pub(super) const FIT_ROUNDS: usize = 3;

/// Refines the physical metrics after the logical fit: sizes the font from the
/// rounded physical cell width (so a fractional raster scale cannot open seams
/// between advances), grows the cell height to the primary font's line box
/// (measured on a full block glyph) and derives the vertical glyph offset from
/// the measured block, core-ASCII and accent ink boxes.
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
    for _ in 0..FIT_ROUNDS {
        // The block's fully opaque rows are what tiles seamlessly; its anti-aliased
        // edge rows are excluded (falling back to the bitmap minus one row per side).
        block = shape_boxes(BLOCK_PROBE, &ResolvedStyle::plain(), config, raster, cx).map(
            |(bitmap, opaque)| {
                opaque.unwrap_or(super::super::GlyphBox {
                    top: bitmap.top + 1.0,
                    bottom: (bitmap.bottom - 1.0).max(bitmap.top + 1.0),
                })
            },
        );
        let height = super::super::fitted_cell_height(raster.cell_size.y, block);
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
            *slot = Some(slot.map_or(measured, |union: super::super::GlyphBox| {
                union.union(measured)
            }));
        }
    }
    let [core, accents] = boxes;
    raster.glyph_offset = super::super::vertical_offset(raster.cell_size.y, block, core, accents);
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
) -> Option<super::super::GlyphBox> {
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
) -> Option<(super::super::GlyphBox, Option<super::super::GlyphBox>)> {
    let layout = shape_run(text, style, config, raster, Vec2::splat(4096.0), cx)?;
    let mut bitmap: Option<super::super::GlyphBox> = None;
    let mut opaque: Option<super::super::GlyphBox> = None;
    for glyph in &layout.glyphs {
        let rect = glyph.atlas_info.rect;
        let height = rect.size().y;
        let top = super::super::snap(glyph.position.y - height * 0.5);
        let glyph_box = super::super::GlyphBox {
            top,
            bottom: top + height,
        };
        bitmap = Some(bitmap.map_or(glyph_box, |b| b.union(glyph_box)));
        if let Some(rows) = opaque_rows(cx.images, glyph.atlas_info.texture, rect) {
            let rows = super::super::GlyphBox {
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
