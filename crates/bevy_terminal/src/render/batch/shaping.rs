//! Glyph rasterization, shape lookup, and atlas storage.
use super::metrics::{GlyphBox, column_coverage};
use super::{
    GLYPH_ATLAS_SIZE, GLYPH_FORMAT, RasterMetrics, ResolvedStyle, TerminalRenderConfig,
    TerminalStats, TextContext, text_font,
};
use bevy::{
    platform::collections::HashMap,
    prelude::*,
    text::{ComputedTextBlock, LetterSpacing, LineBreak, LineHeight, TextBounds, TextLayoutInfo},
};
use std::borrow::Cow;

// Keep common terminal alphabets hot without retaining every grapheme ever
// displayed. A full cache starts a fresh working set; individually oversized
// runs remain uncached. No per-hit recency bookkeeping is needed.
const MAX_SHAPE_ENTRIES: usize = 4096;
const MAX_SHAPE_BYTES: usize = 4 * 1024 * 1024;

pub(super) struct ShapedRun {
    pub(super) glyphs: Vec<bevy::text::PositionedGlyph>,
    /// Snapped line baseline before terminal placement.
    pub(super) baseline: f32,
    /// Pixel enclosure of the resolved face's typographic ascent/descent.
    pub(super) line_box: GlyphBox,
}

/// Shapes and rasterizes `text` in `style` at the physical metrics; the
/// layout's glyphs are positioned inside a line box `raster.cell_size.y` tall.
pub(super) fn shape_run(
    text: &str,
    style: &ResolvedStyle,
    config: &TerminalRenderConfig,
    raster: RasterMetrics,
    viewport: Vec2,
    cx: &mut TextContext<'_>,
) -> Option<ShapedRun> {
    let font = text_font(&config.font, raster.font_size, style);
    let mut computed = ComputedTextBlock::default();
    let mut layout = TextLayoutInfo::default();
    let shape_result = cx.text_pipeline.update_buffer(
        cx.fonts,
        std::iter::once((
            Entity::PLACEHOLDER,
            0,
            text,
            &font,
            Color::WHITE,
            LineHeight::Px(raster.cell_size.y),
            LetterSpacing::default(),
        )),
        LineBreak::NoWrap,
        Justify::Left,
        TextBounds::UNBOUNDED,
        1.0,
        &mut computed,
        cx.font_cx,
        cx.layout_cx,
        viewport,
        20.0,
    );
    let shape_result = shape_result
        .map_err(|error| ("layout", error))
        .and_then(|()| {
            cx.text_pipeline
                .update_text_layout_info(
                    &mut layout,
                    cx.font_atlas_set,
                    cx.images,
                    &mut computed,
                    cx.scale_cx,
                    TextBounds::UNBOUNDED,
                    Justify::Left,
                    config.raster.hinting,
                )
                .map_err(|error| ("rasterization", error))
        });
    if let Err((phase, error)) = shape_result {
        cx.failure
            .get_or_insert_with(|| format!("{phase} for {:?}: {error}", font.font));
        return None;
    }
    let line = computed.buffer().lines().next()?;
    let metrics = line.metrics();
    Some(ShapedRun {
        glyphs: layout.glyphs,
        baseline: super::metrics::snap(metrics.baseline),
        line_box: GlyphBox {
            top: (metrics.baseline - metrics.ascent).floor(),
            bottom: (metrics.baseline + metrics.descent).ceil(),
        },
    })
}

pub(super) fn is_box_drawing(text: &str) -> bool {
    let mut chars = text.chars();
    matches!(chars.next(), Some('\u{2500}'..='\u{257f}')) && chars.next().is_none()
}

/// Whether a grapheme starts with a codepoint Ghostty constrains as a symbol:
/// the blocks whose glyphs fonts commonly draw wider than a text advance
/// (arrows, enclosed alphanumerics, miscellaneous symbols, dingbats,
/// pictographs, emoticons, transport and map symbols, private use).
pub(super) fn is_symbol(text: &str) -> bool {
    text.chars().next().is_some_and(|c| {
        matches!(
            u32::from(c),
            0x2190..=0x21ff
                | 0x2460..=0x24ff
                | 0x2600..=0x26ff
                | 0x2700..=0x27bf
                | 0xe000..=0xf8ff
                | 0x1f100..=0x1f1ff
                | 0x1f300..=0x1f5ff
                | 0x1f600..=0x1f64f
                | 0x1f680..=0x1f6ff
                | 0xf0000..=0xffffd
                | 0x100000..=0x10fffd
        )
    })
}

/// Powerline private-use glyphs: the only symbols that are graphics
/// elements, so the symbol after one may still spread into a blank
/// neighbour (Ghostty's `isSymbol(prev) and !isGraphicsElement(prev)`; box
/// drawing, block elements and legacy computing are never symbols).
pub(super) fn is_powerline(text: &str) -> bool {
    text.chars()
        .next()
        .is_some_and(|c| matches!(u32::from(c), 0xe0b0..=0xe0d7))
}

/// Glyphs that tile the grid (box drawing, shades, legacy computing,
/// Powerline): Ghostty draws these itself; here they come from the font and
/// keep the per-cell clip, so a stroke's sub-pixel overshoot does not paint
/// over the neighbouring cell's anti-aliased edge.
pub(super) fn is_graphics(text: &str) -> bool {
    let mut chars = text.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return false;
    };
    matches!(
        u32::from(c),
        0x2500..=0x257f | 0x2591..=0x2593 | 0x1fb00..=0x1fbff | 0x1cc00..=0x1cebf | 0xe0b0..=0xe0d7
    )
}

/// Upper bound on rescales of one symbol: hinting makes ink extents
/// discontinuous in font size, so a rescaled symbol is measured again.
const SYMBOL_RESCALES: usize = 2;

/// Complete nontransparent ink of a shaped run, in the run's own pixel
/// coordinates (glyph origins snapped as [`cached_shape`] places them).
/// `None` when the run has no ink or its atlas cannot be read, in which case
/// the symbol is drawn unconstrained.
fn run_ink(run: &ShapedRun, images: &Assets<Image>) -> Option<Rect> {
    let mut bounds: Option<Rect> = None;
    for glyph in &run.glyphs {
        let image = images.get(glyph.atlas_info.texture);
        let Some((image, data)) = image
            .filter(|image| image.texture_descriptor.format == GLYPH_FORMAT)
            .and_then(|image| image.data.as_ref().map(|data| (image, data)))
        else {
            debug!("bevy_terminal: glyph atlas unreadable; symbol drawn unconstrained");
            return None;
        };
        let rect = glyph.atlas_info.rect;
        let size = rect.size().as_uvec2();
        let origin = (glyph.position - rect.size() * 0.5).map(super::metrics::snap);
        for y in 0..size.y {
            for x in 0..size.x {
                let offset =
                    ((rect.min.y as u32 + y) * image.width() + rect.min.x as u32 + x) as usize * 4;
                if data.get(offset + 3).is_none_or(|alpha| *alpha == 0) {
                    continue;
                }
                let pixel = origin + Vec2::new(x as f32, y as f32);
                let pixel = Rect::from_corners(pixel, pixel + Vec2::ONE);
                bounds = Some(bounds.map_or(pixel, |bounds| bounds.union(pixel)));
            }
        }
    }
    bounds
}

#[derive(Clone)]
pub(super) struct CachedGlyph {
    pub(super) texture: AssetId<Image>,
    pub(super) offset: Vec2,
    pub(super) size: Vec2,
    pub(super) uv: Vec4,
    pub(super) alpha_mask: bool,
    /// Horizontal extent `[left, right)` of the bitmap's inked columns,
    /// relative to the bitmap.
    pub(super) ink: (f32, f32),
    /// Coverage (sum of alpha) of every bitmap column; tells a box-drawing
    /// stroke's faint sub-pixel overshoot from real overhang.
    pub(super) columns: Vec<u32>,
}

impl CachedGlyph {
    pub(super) fn new(
        texture: AssetId<Image>,
        offset: Vec2,
        size: Vec2,
        uv: Vec4,
        alpha_mask: bool,
        columns: Vec<u32>,
    ) -> Self {
        let left = columns.iter().position(|c| *c > 0);
        let right = columns.iter().rposition(|c| *c > 0);
        let ink = match (left, right) {
            (Some(left), Some(right)) => (left as f32, right as f32 + 1.0),
            _ => (0.0, size.x),
        };
        Self {
            texture,
            offset,
            size,
            uv,
            alpha_mask,
            ink,
            columns,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) struct SourceGlyph {
    pub(super) texture: AssetId<Image>,
    pub(super) x: u32,
    pub(super) y: u32,
    pub(super) width: u32,
    pub(super) height: u32,
}

pub(super) struct UnifiedGlyphAtlas {
    pub(super) image: Handle<Image>,
    pub(super) glyphs: HashMap<SourceGlyph, Vec4>,
    pub(super) cursor: UVec2,
    pub(super) row_height: u32,
}

impl UnifiedGlyphAtlas {
    pub(super) fn new(image: Handle<Image>) -> Self {
        Self {
            image,
            glyphs: HashMap::default(),
            cursor: UVec2::splat(1),
            row_height: 0,
        }
    }

    pub(super) fn cache(
        &mut self,
        source: SourceGlyph,
        images: &mut Assets<Image>,
    ) -> Option<Vec4> {
        if let Some(uv) = self.glyphs.get(&source) {
            return Some(*uv);
        }
        if source.width == 0
            || source.height == 0
            || source.width > GLYPH_ATLAS_SIZE - 2
            || source.height > GLYPH_ATLAS_SIZE - 2
        {
            return None;
        }

        let mut x = self.cursor.x;
        let mut y = self.cursor.y;
        let mut row_height = self.row_height;
        if x + source.width + 1 > GLYPH_ATLAS_SIZE {
            x = 1;
            y = y.checked_add(self.row_height + 1)?;
            row_height = 0;
        }
        if y + source.height + 1 > GLYPH_ATLAS_SIZE {
            return None;
        }

        let pixels = {
            let source_image = images.get(source.texture)?;
            if source_image.texture_descriptor.format != GLYPH_FORMAT
                || source.x.checked_add(source.width)? > source_image.width()
                || source.y.checked_add(source.height)? > source_image.height()
            {
                return None;
            }
            let data = source_image.data.as_ref()?;
            let source_stride = source_image.width() as usize * 4;
            let row_bytes = source.width as usize * 4;
            let mut pixels = Vec::with_capacity(row_bytes * source.height as usize);
            for row in 0..source.height {
                let start = (source.y + row) as usize * source_stride + source.x as usize * 4;
                pixels.extend_from_slice(data.get(start..start + row_bytes)?);
            }
            pixels
        };

        let mut atlas = images.get_mut(&self.image)?;
        if atlas.width() != GLYPH_ATLAS_SIZE {
            atlas.resize(bevy::render::render_resource::Extent3d {
                width: GLYPH_ATLAS_SIZE,
                height: GLYPH_ATLAS_SIZE,
                depth_or_array_layers: 1,
            });
        }
        let data = atlas.data.as_mut()?;
        let atlas_stride = GLYPH_ATLAS_SIZE as usize * 4;
        let row_bytes = source.width as usize * 4;
        for row in 0..source.height {
            let source_start = row as usize * row_bytes;
            let target_start = (y + row) as usize * atlas_stride + x as usize * 4;
            data[target_start..target_start + row_bytes]
                .copy_from_slice(&pixels[source_start..source_start + row_bytes]);
        }

        self.cursor = UVec2::new(x + source.width + 1, y);
        self.row_height = row_height.max(source.height);
        let scale = GLYPH_ATLAS_SIZE as f32;
        let uv = Vec4::new(
            x as f32 / scale,
            y as f32 / scale,
            (x + source.width) as f32 / scale,
            (y + source.height) as f32 / scale,
        );
        self.glyphs.insert(source, uv);
        Some(uv)
    }

    pub(super) fn clear(&mut self, images: &mut Assets<Image>) {
        self.glyphs.clear();
        self.cursor = UVec2::splat(1);
        self.row_height = 0;
        if let Some(mut image) = images.get_mut(&self.image)
            && let Some(data) = image.data.as_mut()
        {
            data.fill(0);
        }
    }
}

#[derive(Default)]
/// Shaped glyph runs per symbol, keyed by face and by the number of cells
/// the run is drawn over (which decides how a symbol is constrained).
///
/// Runs are stored in `entries` and looked up by index so a cache hit costs a
/// single hash lookup and returns a borrow that does not conflict with later
/// insertions. One-cell runs, the bulk of terminal content, index a table
/// directly; wider spans use a map.
pub(super) struct ShapeCaches {
    pub(super) entries: Vec<Vec<CachedGlyph>>,
    retained_bytes: usize,
    narrow: [StyleShapes; 4],
    wide: HashMap<(u16, usize), HashMap<String, usize>>,
}

/// Sentinel for an unoccupied ASCII fast-path slot.
pub(super) const ASCII_UNCACHED: u32 = u32::MAX;

/// Shape lookup for one bold/italic class: single-byte ASCII symbols — the
/// bulk of terminal content — index a table directly with no hashing or
/// string allocation; everything else uses the map.
pub(super) struct StyleShapes {
    pub(super) ascii: [u32; 128],
    pub(super) other: HashMap<String, usize>,
}

impl Default for StyleShapes {
    fn default() -> Self {
        Self {
            ascii: [ASCII_UNCACHED; 128],
            other: HashMap::default(),
        }
    }
}

impl StyleShapes {
    pub(super) fn clear(&mut self) {
        self.ascii = [ASCII_UNCACHED; 128];
        self.other.clear();
    }
}

/// The direct-index key for a single-byte ASCII symbol, if `text` is one.
pub(super) fn ascii_key(text: &str) -> Option<u8> {
    match *text.as_bytes() {
        [byte] if byte < 128 => Some(byte),
        _ => None,
    }
}

impl ShapeCaches {
    fn style_index(style: &ResolvedStyle) -> usize {
        usize::from(style.bold) + 2 * usize::from(style.italic)
    }

    pub(super) fn lookup(&self, style: &ResolvedStyle, text: &str, columns: u16) -> Option<usize> {
        let index = Self::style_index(style);
        let shapes = if columns == 1 {
            &self.narrow[index]
        } else {
            return self.wide.get(&(columns, index))?.get(text).copied();
        };
        match ascii_key(text) {
            Some(byte) => {
                let index = shapes.ascii[usize::from(byte)];
                (index != ASCII_UNCACHED).then_some(index as usize)
            }
            None => shapes.other.get(text).copied(),
        }
    }

    pub(super) fn insert(
        &mut self,
        style: &ResolvedStyle,
        text: &str,
        columns: u16,
        glyphs: Vec<CachedGlyph>,
    ) -> Cow<'_, [CachedGlyph]> {
        let bytes = text.len()
            + glyphs.capacity() * size_of::<CachedGlyph>()
            + glyphs
                .iter()
                .map(|glyph| glyph.columns.capacity() * size_of::<u32>())
                .sum::<usize>();
        if bytes > MAX_SHAPE_BYTES {
            return Cow::Owned(glyphs);
        }
        if self.entries.len() >= MAX_SHAPE_ENTRIES || bytes > MAX_SHAPE_BYTES - self.retained_bytes
        {
            // Scene construction has already consumed earlier borrowed runs.
            // Reset every lookup table with the entries so no index goes stale.
            self.clear();
        }
        self.retained_bytes += bytes;
        let index = self.entries.len();
        self.entries.push(glyphs);
        let style_index = Self::style_index(style);
        if columns != 1 {
            self.wide
                .entry((columns, style_index))
                .or_default()
                .insert(text.to_owned(), index);
            return Cow::Borrowed(&self.entries[index]);
        }
        let shapes = &mut self.narrow[style_index];
        match ascii_key(text) {
            Some(byte) => shapes.ascii[usize::from(byte)] = index as u32,
            None => {
                shapes.other.insert(text.to_owned(), index);
            }
        }
        Cow::Borrowed(&self.entries[index])
    }

    pub(super) fn clear(&mut self) {
        self.entries.clear();
        self.retained_bytes = 0;
        self.narrow.iter_mut().for_each(StyleShapes::clear);
        self.wide.clear();
    }
}

/// The cached run for `text` drawn in `style` over `columns` cells, shaping
/// and rasterizing it on a miss.
///
/// Ordinary text keeps its rasterized size and bearings and shares the
/// configured primary face's baseline. Symbols ([`is_symbol`]) are constrained
/// the way Ghostty's `fit` rule constrains them: scaled down uniformly, only as
/// far as their ink needs to fit the `columns`-wide cell box, about their own
/// center, and then pushed inside that box (leading edges win). Ghostty's box
/// is the face's line box; here it is the cell, which encloses it for
/// font-driven and width-fitted geometry (a compact `Fixed` cell shrinks
/// symbols further). Rescaling
/// steps to the next whole-pixel font size below the size that fits, so the
/// rescaled symbols occupy a bounded number of Bevy font atlases.
#[allow(clippy::too_many_arguments)]
pub(super) fn cached_shape<'a>(
    text: &str,
    columns: u16,
    style: &ResolvedStyle,
    config: &TerminalRenderConfig,
    raster: RasterMetrics,
    viewport: Vec2,
    cx: &mut TextContext<'_>,
    shapes: &'a mut ShapeCaches,
    glyph_atlas: &mut UnifiedGlyphAtlas,
    stats: &mut TerminalStats,
) -> Cow<'a, [CachedGlyph]> {
    if let Some(index) = shapes.lookup(style, text, columns) {
        return Cow::Borrowed(&shapes.entries[index]);
    }
    stats.shape_misses = stats.shape_misses.saturating_add(1);
    let mut request = raster;
    let Some(mut layout) = shape_run(text, style, config, request, viewport, cx) else {
        return Cow::Borrowed(&[]);
    };
    // Each independently shaped cell otherwise centers its own fallback face
    // in the line. Ordinary runs share the configured primary face's baseline.
    // This is an integer translation, so rasterization phase is unchanged.
    let mut translate = Vec2::new(
        0.0,
        if is_box_drawing(text) {
            0.0
        } else {
            raster.baseline - layout.baseline
        },
    );
    if is_symbol(text)
        && let Some(mut ink) = run_ink(&layout, cx.images)
    {
        let bounds = Vec2::new(f32::from(columns) * raster.cell_size.x, raster.cell_size.y);
        // The scene adds the uniform text offset; constrain in cell coordinates.
        let offset = Vec2::new(0.0, raster.glyph_offset);
        let target = Rect::from_corners(ink.min + translate + offset, ink.max + translate + offset);
        for rescale in 0..=SYMBOL_RESCALES {
            let factor = (bounds / ink.size()).min_element();
            if factor >= 1.0 {
                break;
            }
            if rescale == SYMBOL_RESCALES || request.font_size <= 1.0 {
                debug!("bevy_terminal: {text:?} still exceeds its cells; drawn as is");
                break;
            }
            let next = (request.font_size * factor).floor();
            let next = if next < request.font_size {
                next
            } else {
                request.font_size - 1.0
            }
            .max(1.0);
            let rescaled = RasterMetrics {
                font_size: next,
                ..request
            };
            let Some(rescaled_run) = shape_run(text, style, config, rescaled, viewport, cx) else {
                return Cow::Borrowed(&[]);
            };
            // A rescale that changes nothing (a bitmap strike) or whose ink
            // cannot be measured keeps the previous raster.
            let Some(rescaled_ink) = run_ink(&rescaled_run, cx.images)
                .filter(|rescaled| rescaled.size().cmplt(ink.size()).any())
            else {
                break;
            };
            request = rescaled;
            layout = rescaled_run;
            ink = rescaled_ink;
            translate = (((target.min + target.max) - (ink.min + ink.max)) * 0.5)
                .map(super::metrics::snap)
                - offset;
        }
        let overflow = (ink.max + translate + offset - bounds).max(Vec2::ZERO);
        translate -= overflow;
        translate += (-(ink.min + translate + offset)).max(Vec2::ZERO);
    }
    let cached = layout
        .glyphs
        .into_iter()
        .map(|glyph| {
            let atlas = cx.images.get(glyph.atlas_info.texture)?;
            let atlas_size = atlas.texture_descriptor.size;
            let rect = glyph.atlas_info.rect;
            let size = rect.size();
            let columns = column_coverage(atlas, rect);
            let source = SourceGlyph {
                texture: glyph.atlas_info.texture,
                x: rect.min.x as u32,
                y: rect.min.y as u32,
                width: size.x as u32,
                height: size.y as u32,
            };
            let source_uv = Vec4::new(
                rect.min.x / atlas_size.width as f32,
                rect.min.y / atlas_size.height as f32,
                rect.max.x / atlas_size.width as f32,
                rect.max.y / atlas_size.height as f32,
            );
            let (texture, uv) = glyph_atlas
                .cache(source, cx.images)
                .map_or((source.texture, source_uv), |uv| {
                    (glyph_atlas.image.id(), uv)
                });
            Some(CachedGlyph::new(
                texture,
                // Atlas texels must land on physical pixel boundaries. Bevy's layout positions
                // can retain fractional shaping offsets even though the glyph bitmap is an
                // integer-sized raster image.
                (glyph.position - size * 0.5).map(super::metrics::snap) + translate,
                size,
                uv,
                glyph.atlas_info.is_alpha_mask,
                columns,
            ))
        })
        .collect::<Option<Vec<_>>>();
    let Some(cached) = cached else {
        cx.failure
            .get_or_insert_with(|| format!("glyph atlas unavailable for {:?}", config.font));
        return Cow::Borrowed(&[]);
    };
    shapes.insert(style, text, columns, cached)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_admits_new_working_sets_and_leaves_oversized_runs_uncached() {
        let style = ResolvedStyle::plain();
        let mut shapes = ShapeCaches::default();
        for index in 0..MAX_SHAPE_ENTRIES {
            assert!(matches!(
                shapes.insert(&style, &format!("symbol-{index}"), 1, Vec::new()),
                Cow::Borrowed(_)
            ));
        }
        assert!(matches!(
            shapes.insert(&style, "overflow", 1, Vec::new()),
            Cow::Borrowed(_)
        ));
        assert_eq!(shapes.entries.len(), 1);
        assert!(shapes.lookup(&style, "symbol-0", 1).is_none());
        assert_eq!(shapes.lookup(&style, "overflow", 1), Some(0));

        let glyph = CachedGlyph::new(
            AssetId::default(),
            Vec2::ZERO,
            Vec2::ONE,
            Vec4::ZERO,
            true,
            vec![1; MAX_SHAPE_BYTES / size_of::<u32>()],
        );
        let excess = shapes.insert(&style, "large", 1, vec![glyph]);
        assert!(matches!(excess, Cow::Owned(_)));
        assert_eq!(excess.len(), 1, "the run still renders in full");
        drop(excess);
        assert_eq!(
            shapes.entries.len(),
            1,
            "an oversized run preserves the working set"
        );
        assert_eq!(shapes.lookup(&style, "overflow", 1), Some(0));
        assert!(matches!(
            shapes.insert(&style, "A", 1, Vec::new()),
            Cow::Borrowed(_)
        ));
        assert_eq!(shapes.lookup(&style, "A", 1), Some(1));
    }
}
