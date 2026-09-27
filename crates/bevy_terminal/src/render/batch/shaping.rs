//! Glyph rasterization, shape lookup, and atlas storage.
use super::constraint::{Align, Constraint, GlyphSize, Size};
use super::metrics::{GlyphBox, column_coverage};
use super::sprite::{self, Sprite};
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
    /// The line's typographic ascent and descent and its advance.
    pub(super) ascent: f32,
    pub(super) descent: f32,
    pub(super) advance: f32,
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
        ascent: metrics.ascent,
        descent: metrics.descent,
        advance: metrics.advance,
    })
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

/// Upper bound on further rescales of one constrained run: hinting makes ink
/// extents discontinuous in font size, so a rescaled run is measured again.
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
    /// relative to the bitmap; empty for a transparent bitmap.
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
            _ => (0.0, 0.0),
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
    /// The size the glyph is resampled to; zero for an unscaled copy.
    pub(super) scaled: UVec2,
}

/// The RGBA8 pixels of a Bevy atlas glyph.
fn source_pixels(source: SourceGlyph, images: &Assets<Image>) -> Option<Vec<u8>> {
    let image = images.get(source.texture)?;
    if image.texture_descriptor.format != GLYPH_FORMAT
        || source.x.checked_add(source.width)? > image.width()
        || source.y.checked_add(source.height)? > image.height()
    {
        return None;
    }
    let data = image.data.as_ref()?;
    let stride = image.width() as usize * 4;
    let row_bytes = source.width as usize * 4;
    let mut pixels = Vec::with_capacity(row_bytes * source.height as usize);
    for row in 0..source.height {
        let start = (source.y + row) as usize * stride + source.x as usize * 4;
        pixels.extend_from_slice(data.get(start..start + row_bytes)?);
    }
    Some(pixels)
}

/// Resamples straight-alpha RGBA8 pixels from `from` to `to` with a box
/// filter (each output pixel averages the source area it covers), in
/// premultiplied form so transparent texels do not darken edges.
pub(super) fn resample(pixels: &[u8], from: UVec2, to: UVec2) -> Vec<u8> {
    // One axis at a time: the weights of each output index over source indices.
    let weights = |from: u32, to: u32| -> Vec<Vec<(usize, f32)>> {
        let step = from as f32 / to as f32;
        (0..to)
            .map(|i| {
                let (start, end) = (i as f32 * step, (i + 1) as f32 * step);
                (start.floor() as u32..(end.ceil() as u32).min(from))
                    .map(|j| {
                        let overlap = (end.min(j as f32 + 1.0) - start.max(j as f32)) / step;
                        (j as usize, overlap)
                    })
                    .collect()
            })
            .collect()
    };
    let (columns, rows) = (weights(from.x, to.x), weights(from.y, to.y));
    let premultiplied: Vec<[f32; 4]> = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| {
            let a = f32::from(p[3]) / 255.0;
            [
                f32::from(p[0]) * a,
                f32::from(p[1]) * a,
                f32::from(p[2]) * a,
                a,
            ]
        })
        .collect();
    let mut horizontal = vec![[0.0f32; 4]; (to.x * from.y) as usize];
    for y in 0..from.y as usize {
        for (x, taps) in columns.iter().enumerate() {
            let out = &mut horizontal[y * to.x as usize + x];
            for &(j, w) in taps {
                let p = premultiplied[y * from.x as usize + j];
                for c in 0..4 {
                    out[c] += p[c] * w;
                }
            }
        }
    }
    let mut result = Vec::with_capacity((to.x * to.y * 4) as usize);
    for taps in &rows {
        for x in 0..to.x as usize {
            let mut sum = [0.0f32; 4];
            for &(j, w) in taps {
                let p = horizontal[j * to.x as usize + x];
                for c in 0..4 {
                    sum[c] += p[c] * w;
                }
            }
            let a = sum[3].clamp(0.0, 1.0);
            let unpremultiply = |v: f32| {
                if a > 0.0 {
                    (v / a).round().clamp(0.0, 255.0) as u8
                } else {
                    0
                }
            };
            result.extend_from_slice(&[
                unpremultiply(sum[0]),
                unpremultiply(sum[1]),
                unpremultiply(sum[2]),
                (a * 255.0).round() as u8,
            ]);
        }
    }
    result
}

/// What a unified atlas entry holds: a copy of a Bevy atlas glyph (possibly
/// resampled) or a procedurally drawn sprite of a given size.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum AtlasKey {
    Source(SourceGlyph),
    Sprite { codepoint: u32, size: UVec2 },
}

pub(super) struct UnifiedGlyphAtlas {
    pub(super) image: Handle<Image>,
    pub(super) glyphs: HashMap<AtlasKey, Vec4>,
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

    /// Copies a Bevy atlas glyph into the atlas, returning its UV rectangle.
    pub(super) fn cache(
        &mut self,
        source: SourceGlyph,
        images: &mut Assets<Image>,
    ) -> Option<Vec4> {
        if let Some(uv) = self.glyphs.get(&AtlasKey::Source(source)) {
            return Some(*uv);
        }
        let pixels = source_pixels(source, images)?;
        self.insert(
            AtlasKey::Source(source),
            UVec2::new(source.width, source.height),
            &pixels,
            images,
        )
    }

    /// Resamples a Bevy atlas glyph to `scaled` pixels into the atlas,
    /// returning its UV rectangle and column coverage.
    pub(super) fn cache_scaled(
        &mut self,
        source: SourceGlyph,
        scaled: UVec2,
        images: &mut Assets<Image>,
    ) -> Option<(Vec4, Vec<u32>)> {
        let key = SourceGlyph { scaled, ..source };
        let pixels = resample(
            &source_pixels(source, images)?,
            UVec2::new(source.width, source.height),
            scaled,
        );
        let uv = match self.glyphs.get(&AtlasKey::Source(key)) {
            Some(uv) => *uv,
            None => self.insert(AtlasKey::Source(key), scaled, &pixels, images)?,
        };
        let columns = (0..scaled.x as usize)
            .map(|x| {
                (0..scaled.y as usize)
                    .map(|y| u32::from(pixels[(y * scaled.x as usize + x) * 4 + 3]))
                    .sum()
            })
            .collect();
        Some((uv, columns))
    }

    /// Adds a sprite's coverage to the atlas, returning its UV rectangle.
    pub(super) fn cache_sprite(
        &mut self,
        codepoint: u32,
        cell: UVec2,
        sprite: &Sprite,
        images: &mut Assets<Image>,
    ) -> Option<Vec4> {
        let key = AtlasKey::Sprite {
            codepoint,
            size: cell,
        };
        if let Some(uv) = self.glyphs.get(&key) {
            return Some(*uv);
        }
        let pixels: Vec<u8> = sprite
            .alpha
            .iter()
            .flat_map(|alpha| [255, 255, 255, *alpha])
            .collect();
        self.insert(key, sprite.size, &pixels, images)
    }

    fn insert(
        &mut self,
        key: AtlasKey,
        size: UVec2,
        pixels: &[u8],
        images: &mut Assets<Image>,
    ) -> Option<Vec4> {
        if size.x == 0
            || size.y == 0
            || size.x > GLYPH_ATLAS_SIZE - 2
            || size.y > GLYPH_ATLAS_SIZE - 2
        {
            return None;
        }
        let mut x = self.cursor.x;
        let mut y = self.cursor.y;
        let mut row_height = self.row_height;
        if x + size.x + 1 > GLYPH_ATLAS_SIZE {
            x = 1;
            y = y.checked_add(self.row_height + 1)?;
            row_height = 0;
        }
        if y + size.y + 1 > GLYPH_ATLAS_SIZE {
            return None;
        }

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
        let row_bytes = size.x as usize * 4;
        for row in 0..size.y {
            let source_start = row as usize * row_bytes;
            let target_start = (y + row) as usize * atlas_stride + x as usize * 4;
            data[target_start..target_start + row_bytes]
                .copy_from_slice(&pixels[source_start..source_start + row_bytes]);
        }

        self.cursor = UVec2::new(x + size.x + 1, y);
        self.row_height = row_height.max(size.y);
        let scale = GLYPH_ATLAS_SIZE as f32;
        let uv = Vec4::new(
            x as f32 / scale,
            y as f32 / scale,
            (x + size.x) as f32 / scale,
            (y + size.y) as f32 / scale,
        );
        self.glyphs.insert(key, uv);
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

/// The constraint Ghostty applies to a run: emoji-presentation (colour)
/// glyphs cover their cells, codepoints with Nerd Fonts attributes use them,
/// other symbols fit, and ordinary text is unconstrained.
fn constraint_for(text: &str, run: &ShapedRun) -> Option<Constraint> {
    if run
        .glyphs
        .iter()
        .any(|glyph| !glyph.atlas_info.is_alpha_mask)
    {
        return Some(Constraint::EMOJI);
    }
    let codepoint = u32::from(text.chars().next()?);
    Constraint::nerd_font(codepoint).or_else(|| is_symbol(text).then_some(Constraint::SYMBOL))
}

/// Union of a run's glyph bitmaps in run pixels: the box of a bitmap glyph,
/// transparent margins included, as Ghostty measures bitmap emoji.
fn run_bitmaps(run: &ShapedRun) -> Option<Rect> {
    run.glyphs
        .iter()
        .map(|glyph| {
            let size = glyph.atlas_info.rect.size();
            let origin = (glyph.position - size * 0.5).map(super::metrics::snap);
            Rect::from_corners(origin, origin + size)
        })
        .reduce(|a, b| a.union(b))
}

/// A constrained run: the raster to draw, its translation, and for
/// stretched glyphs the anisotropic scale its measured box still needs.
struct Fitted {
    run: ShapedRun,
    translate: Vec2,
    stretch: Option<Stretch>,
}

/// Resampling of a stretched run: its measured box (run pixels) is scaled by
/// `factors` about its top-left corner.
#[derive(Clone, Copy)]
struct Stretch {
    factors: Vec2,
    measured: Rect,
}

/// Applies Ghostty's `constraint` to `run` drawn over `columns` cells.
///
/// The constraint's target box comes from Ghostty's arithmetic on the run's
/// measured box (ink for outlines, whole bitmaps for colour glyphs) and the
/// face metrics. Uniform scaling re-rasterizes at the whole-pixel font size
/// just below the target, measured again at most [`SYMBOL_RESCALES`] times
/// because hinting makes extents discontinuous; the result is aligned to the
/// target box by the constraint's alignment and snapped to whole pixels.
/// Stretched glyphs are rasterized at the larger of their two scales and
/// resampled to the target box when copied to the atlas.
#[allow(clippy::too_many_arguments)]
fn fit_run(
    text: &str,
    style: &ResolvedStyle,
    config: &TerminalRenderConfig,
    raster: RasterMetrics,
    viewport: Vec2,
    cx: &mut TextContext<'_>,
    run: ShapedRun,
    translate: Vec2,
    constraint: Constraint,
    columns: u16,
) -> Result<Fitted, ShapedRun> {
    let color = constraint == Constraint::EMOJI;
    let measure = |run: &ShapedRun, images: &Assets<Image>| {
        if color {
            run_bitmaps(run)
        } else {
            run_ink(run, images)
        }
    };
    let Some(measured) = measure(&run, cx.images) else {
        return Err(run);
    };
    // Cell pixels (y down) of the scene, which adds the uniform text offset.
    let offset = Vec2::new(0.0, raster.glyph_offset);
    let cell_height = f64::from(raster.cell_size.y);
    let glyph = GlyphSize {
        width: f64::from(measured.width()),
        height: f64::from(measured.height()),
        x: f64::from(measured.min.x + translate.x),
        y: cell_height - f64::from(measured.max.y + translate.y + offset.y),
    };
    let mut target = constraint.constrain(glyph, raster.face, columns.min(2) as u8);
    if constraint.size != Size::Stretch {
        target.x += f64::from(raster.face_dx);
    }
    let factors = Vec2::new(
        (target.width / glyph.width) as f32,
        (target.height / glyph.height) as f32,
    );
    let uniform = if constraint.size == Size::Stretch {
        factors.max_element()
    } else {
        factors.y
    };
    let target_size = Vec2::new(target.width as f32, target.height as f32);
    let mut run = run;
    let mut measured = measured;
    if (uniform - 1.0).abs() > 1e-3 {
        let mut request = raster;
        let mut scale = uniform;
        for _ in 0..=SYMBOL_RESCALES {
            let next = (request.font_size * scale)
                .floor()
                .clamp(1.0, raster.font_size * 8.0);
            if next == request.font_size {
                break;
            }
            request.font_size = next;
            let Some(rescaled) = shape_run(text, style, config, request, viewport, cx) else {
                return Err(run);
            };
            let Some(rescaled_box) = measure(&rescaled, cx.images) else {
                break;
            };
            run = rescaled;
            measured = rescaled_box;
            // Hinting can leave a fitted raster a pixel too large; shrink again.
            let excess = (measured.size() - target_size).max_element();
            if constraint.size == Size::Stretch || constraint.size == Size::Cover || excess <= 0.5 {
                break;
            }
            scale = (target_size / measured.size())
                .min_element()
                .min(1.0 - 1e-3);
        }
    }
    let stretch = (constraint.size == Size::Stretch).then(|| Stretch {
        factors: target_size / measured.size(),
        measured,
    });
    let drawn = measured.size() * stretch.map_or(Vec2::ONE, |stretch| stretch.factors);
    // Align the drawn box to the target box along each axis.
    let left = match constraint.align_horizontal {
        Align::Start => target.x as f32,
        Align::End => (target.x + target.width) as f32 - drawn.x,
        _ => (target.x + target.width / 2.0) as f32 - drawn.x / 2.0,
    };
    let bottom = match constraint.align_vertical {
        Align::Start => target.y as f32,
        Align::End => (target.y + target.height) as f32 - drawn.y,
        _ => (target.y + target.height / 2.0) as f32 - drawn.y / 2.0,
    };
    let top = raster.cell_size.y - bottom - drawn.y;
    Ok(Fitted {
        run,
        translate: (Vec2::new(left, top) - offset - measured.min).map(super::metrics::snap),
        stretch,
    })
}

/// The cached run for `text` drawn in `style` over `columns` cells, shaping
/// and rasterizing it on a miss.
///
/// Ordinary text keeps its rasterized size and bearings, shares the
/// configured primary face's baseline, and is centered in cells wider than
/// the face's advance, as Ghostty draws it. Emoji, Nerd Fonts icons and other
/// symbols follow Ghostty's constraints (see [`constraint_for`] and
/// [`fit_run`]) against the primary face's box.
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
    if let Some(codepoint) = sprite::sprite_codepoint(text) {
        // Grid graphics are drawn, not shaped, at the size of their cells.
        let cell = UVec2::new(
            raster.cell_size.x as u32 * u32::from(columns),
            raster.cell_size.y as u32,
        );
        let metrics = sprite::Metrics {
            cell_width: raster.cell_size.x as u32,
            cell_height: raster.cell_size.y as u32,
            box_thickness: raster.box_thickness,
        };
        let glyphs = sprite::draw(codepoint, cell.x, cell.y, metrics)
            .and_then(|drawn| {
                let uv = glyph_atlas.cache_sprite(codepoint, cell, &drawn, cx.images)?;
                let columns = (0..drawn.size.x as usize)
                    .map(|x| {
                        (0..drawn.size.y as usize)
                            .map(|y| u32::from(drawn.alpha[y * drawn.size.x as usize + x]))
                            .sum()
                    })
                    .collect();
                Some(vec![CachedGlyph::new(
                    glyph_atlas.image.id(),
                    drawn.offset.as_vec2(),
                    drawn.size.as_vec2(),
                    uv,
                    true,
                    columns,
                )])
            })
            .unwrap_or_default();
        return shapes.insert(style, text, columns, glyphs);
    }
    let Some(layout) = shape_run(text, style, config, raster, viewport, cx) else {
        return Cow::Borrowed(&[]);
    };
    // Each independently shaped cell otherwise centers its own fallback face
    // in the line. Ordinary runs share the configured primary face's baseline.
    // This is an integer translation, so rasterization phase is unchanged.
    let translate = Vec2::new(0.0, raster.baseline - layout.baseline);
    // Text is centered in cells wider than the face; constraints position
    // their glyphs themselves.
    let centered = Vec2::new(raster.face_dx, 0.0);
    let (layout, translate, stretch) = match constraint_for(text, &layout) {
        Some(constraint) => match fit_run(
            text, style, config, raster, viewport, cx, layout, translate, constraint, columns,
        ) {
            Ok(fitted) => (fitted.run, fitted.translate, fitted.stretch),
            Err(layout) => {
                debug!("bevy_terminal: {text:?} could not be measured; drawn unconstrained");
                (layout, translate + centered, None)
            }
        },
        None => (layout, translate + centered, None),
    };
    let cached = layout
        .glyphs
        .into_iter()
        .map(|glyph| {
            let atlas_size = cx
                .images
                .get(glyph.atlas_info.texture)?
                .texture_descriptor
                .size;
            let rect = glyph.atlas_info.rect;
            let size = rect.size();
            let source = SourceGlyph {
                texture: glyph.atlas_info.texture,
                x: rect.min.x as u32,
                y: rect.min.y as u32,
                width: size.x as u32,
                height: size.y as u32,
                scaled: UVec2::ZERO,
            };
            // Atlas texels must land on physical pixel boundaries. Bevy's layout positions
            // can retain fractional shaping offsets even though the glyph bitmap is an
            // integer-sized raster image.
            let position = (glyph.position - size * 0.5).map(super::metrics::snap);
            if let Some(Stretch { factors, measured }) = stretch {
                // Resample the part of the bitmap inside the measured box so
                // its ink maps exactly onto the target box.
                let crop = Rect::from_corners(position, position + size).intersect(measured);
                if crop.is_empty() {
                    return Some(None);
                }
                let min = ((crop.min - measured.min) * factors).round();
                let max = ((crop.max - measured.min) * factors).round();
                let scaled = (max - min).max(Vec2::ONE).as_uvec2();
                let cropped = SourceGlyph {
                    x: source.x + (crop.min.x - position.x) as u32,
                    y: source.y + (crop.min.y - position.y) as u32,
                    width: crop.width() as u32,
                    height: crop.height() as u32,
                    ..source
                };
                if let Some((uv, columns)) = glyph_atlas.cache_scaled(cropped, scaled, cx.images) {
                    return Some(Some(CachedGlyph::new(
                        glyph_atlas.image.id(),
                        measured.min + translate + min,
                        scaled.as_vec2(),
                        uv,
                        glyph.atlas_info.is_alpha_mask,
                        columns,
                    )));
                }
                debug!("bevy_terminal: no atlas space to stretch {text:?}; drawn as rasterized");
            }
            let columns = column_coverage(cx.images.get(glyph.atlas_info.texture)?, rect);
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
            Some(Some(CachedGlyph::new(
                texture,
                position + translate,
                size,
                uv,
                glyph.atlas_info.is_alpha_mask,
                columns,
            )))
        })
        .collect::<Option<Vec<_>>>()
        .map(|glyphs| glyphs.into_iter().flatten().collect::<Vec<_>>());
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
    fn resampling_averages_premultiplied_areas() {
        // A white opaque texel next to a transparent red one halves to white
        // at half coverage: transparent colour does not bleed in.
        let pixels = [255, 255, 255, 255, 255, 0, 0, 0];
        assert_eq!(
            resample(&pixels, UVec2::new(2, 1), UVec2::ONE),
            vec![255, 255, 255, 128]
        );
        // Enlarging repeats texels; each output keeps its source's value.
        let enlarged = resample(&pixels, UVec2::new(2, 1), UVec2::new(4, 2));
        assert_eq!(
            &enlarged[..16],
            &[
                255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0
            ]
        );
        assert_eq!(enlarged[..16], enlarged[16..]);
        // A fractional step splits a texel between two outputs.
        let thirds = resample(
            &[0, 0, 0, 255, 0, 0, 0, 0, 0, 0, 0, 0],
            UVec2::new(3, 1),
            UVec2::new(2, 1),
        );
        let alpha: Vec<u8> = thirds.iter().skip(3).step_by(4).copied().collect();
        assert_eq!(alpha, [170, 0]);
    }

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
