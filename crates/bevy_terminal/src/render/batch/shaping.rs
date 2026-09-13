//! Glyph rasterization, shape lookup, and atlas storage.
use super::metrics::column_coverage;
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

/// Shapes and rasterizes `text` in `style` at the physical metrics; the
/// layout's glyphs are positioned inside a line box `raster.cell_size.y` tall.
pub(super) fn shape_run(
    text: &str,
    style: &ResolvedStyle,
    config: &TerminalRenderConfig,
    raster: RasterMetrics,
    viewport: Vec2,
    cx: &mut TextContext<'_>,
) -> Option<TextLayoutInfo> {
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
    Some(layout)
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
    /// Coverage (sum of alpha) of every bitmap column; used to fit a run wider
    /// than its cell so that clipping drops the faintest columns.
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
/// Shaped glyph runs per symbol, one map per font face.
///
/// Runs are stored in `entries` and looked up by index so a cache hit costs a
/// single hash lookup and returns a borrow that does not conflict with later
/// insertions.
pub(super) struct ShapeCaches {
    pub(super) entries: Vec<Vec<CachedGlyph>>,
    retained_bytes: usize,
    pub(super) normal: StyleShapes,
    pub(super) bold: StyleShapes,
    pub(super) italic: StyleShapes,
    pub(super) bold_italic: StyleShapes,
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
    pub(super) fn select(&self, style: &ResolvedStyle) -> &StyleShapes {
        match (style.bold, style.italic) {
            (false, false) => &self.normal,
            (true, false) => &self.bold,
            (false, true) => &self.italic,
            (true, true) => &self.bold_italic,
        }
    }

    pub(super) fn select_mut(&mut self, style: &ResolvedStyle) -> &mut StyleShapes {
        match (style.bold, style.italic) {
            (false, false) => &mut self.normal,
            (true, false) => &mut self.bold,
            (false, true) => &mut self.italic,
            (true, true) => &mut self.bold_italic,
        }
    }

    pub(super) fn lookup(&self, style: &ResolvedStyle, text: &str) -> Option<usize> {
        let shapes = self.select(style);
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
        let shapes = self.select_mut(style);
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
        self.normal.clear();
        self.bold.clear();
        self.italic.clear();
        self.bold_italic.clear();
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn cached_shape<'a>(
    text: &str,
    style: &ResolvedStyle,
    config: &TerminalRenderConfig,
    raster: RasterMetrics,
    viewport: Vec2,
    cx: &mut TextContext<'_>,
    shapes: &'a mut ShapeCaches,
    glyph_atlas: &mut UnifiedGlyphAtlas,
    stats: &mut TerminalStats,
) -> Cow<'a, [CachedGlyph]> {
    if let Some(index) = shapes.lookup(style, text) {
        return Cow::Borrowed(&shapes.entries[index]);
    }
    stats.shape_misses = stats.shape_misses.saturating_add(1);
    let Some(layout) = shape_run(text, style, config, raster, viewport, cx) else {
        return Cow::Borrowed(&[]);
    };
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
                (glyph.position - size * 0.5).map(super::metrics::snap),
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
    shapes.insert(style, text, cached)
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
                shapes.insert(&style, &format!("symbol-{index}"), Vec::new()),
                Cow::Borrowed(_)
            ));
        }
        assert!(matches!(
            shapes.insert(&style, "overflow", Vec::new()),
            Cow::Borrowed(_)
        ));
        assert_eq!(shapes.entries.len(), 1);
        assert!(shapes.lookup(&style, "symbol-0").is_none());
        assert_eq!(shapes.lookup(&style, "overflow"), Some(0));

        let glyph = CachedGlyph::new(
            AssetId::default(),
            Vec2::ZERO,
            Vec2::ONE,
            Vec4::ZERO,
            true,
            vec![1; MAX_SHAPE_BYTES / size_of::<u32>()],
        );
        let excess = shapes.insert(&style, "large", vec![glyph]);
        assert!(matches!(excess, Cow::Owned(_)));
        assert_eq!(excess.len(), 1, "the run still renders in full");
        drop(excess);
        assert_eq!(
            shapes.entries.len(),
            1,
            "an oversized run preserves the working set"
        );
        assert_eq!(shapes.lookup(&style, "overflow"), Some(0));
        assert!(matches!(
            shapes.insert(&style, "A", Vec::new()),
            Cow::Borrowed(_)
        ));
        assert_eq!(shapes.lookup(&style, "A"), Some(1));
    }
}
