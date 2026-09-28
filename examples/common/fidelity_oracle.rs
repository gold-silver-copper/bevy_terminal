//! Independent, unclipped text reference for the GPU fidelity harness.
//!
//! This uses Bevy's font rasterizer and raw font tables, but none of the
//! terminal renderer's measurement probes, fitting, geometry, atlas copy, or
//! clipping functions. The expected placement of a run restates the
//! renderer's contract from Ghostty's rules (`renderer/generic.zig`,
//! `font/SharedGrid.zig`, `font/Glyph.zig`):
//!
//! - ordinary text keeps its rasterized shape and bearings on the shared
//!   baseline, centered in cells wider than the face's advance, and overflows
//!   neighbouring cells and rows;
//! - symbols are scaled down uniformly (`fit`), only as far as needed to fit
//!   the face box of the cells they may occupy, about their center, then
//!   moved inside it (left and bottom edges win);
//! - colour glyphs are scaled up or down to cover that face box with 2.5%
//!   side padding, centered (`cover`);
//! - only the texture's edges (or a grid graphic's cells) clip ink.

use bevy::{
    ecs::system::SystemParam,
    prelude::*,
    text::{
        ComputedTextBlock, FontAtlasSet, FontCx, FontStyle, FontWeight, LayoutCx, LetterSpacing,
        LineHeight, ScaleCx, TextBounds, TextLayoutInfo, TextPipeline,
    },
};
use bevy_terminal_ratatui::prelude::{StyleFlags, TerminalCell, TerminalRenderConfig};

#[derive(SystemParam)]
pub struct Rasterizer<'w> {
    fonts: Res<'w, Assets<Font>>,
    pipeline: ResMut<'w, TextPipeline>,
    font_cx: ResMut<'w, FontCx>,
    layout_cx: ResMut<'w, LayoutCx>,
    scale_cx: ResMut<'w, ScaleCx>,
    atlases: ResMut<'w, FontAtlasSet>,
    images: ResMut<'w, Assets<Image>>,
}

/// One shaped run, composed into an unbounded premultiplied linear RGBA image. `origin` is relative
/// to the line origin, so negative bearings and accents are retained.
pub struct Reference {
    pub origin: IVec2,
    pub size: UVec2,
    pub rgba: Vec<Vec4>,
    /// The run's glyphs, each drawn (and blended) on its own.
    pub layers: Vec<Layer>,
    pub color_glyphs: usize,
    pub glyph_count: usize,
    pub baseline: f32,
    pub ascent: f32,
    pub descent: f32,
    pub advance: f32,
    pub faces: Vec<(u64, u32)>,
    pub supported: bool,
}

/// One glyph bitmap of a run in premultiplied linear RGBA, relative to the
/// line origin; `mask` glyphs are coverage drawn in the text colour.
#[derive(Clone, Debug)]
pub struct Layer {
    pub origin: IVec2,
    pub size: UVec2,
    pub rgba: Vec<Vec4>,
    pub mask: bool,
}

impl Rasterizer<'_> {
    /// Center the configured faces' typographic pixel enclosure on a shared
    /// baseline. This expectation comes only from raw font metrics, never from
    /// captured ink or the renderer's measurement/fitting functions.
    pub fn baseline(
        &mut self,
        config: &TerminalRenderConfig,
        font_size: f32,
        height: f32,
    ) -> Result<i32, String> {
        let mut above = 0.0_f32;
        let mut below = 0.0_f32;
        for flags in [
            StyleFlags::empty(),
            StyleFlags::BOLD,
            StyleFlags::ITALIC,
            StyleFlags::BOLD | StyleFlags::ITALIC,
        ] {
            let mut cell = TerminalCell::new("0");
            cell.style.flags = flags;
            let run = self.shape(&cell, config, font_size, height)?;
            let baseline = (run.baseline + 0.5).floor();
            above = above.max(baseline - (run.baseline - run.ascent).floor());
            below = below.max((run.baseline + run.descent).ceil() - baseline);
        }
        Ok(((height + above - below) / 2.0 + 0.5).floor() as i32)
    }

    pub fn shape(
        &mut self,
        cell: &TerminalCell,
        config: &TerminalRenderConfig,
        font_size: f32,
        line_height: f32,
    ) -> Result<Reference, String> {
        let bold = cell.style.has(StyleFlags::BOLD);
        let italic = cell.style.has(StyleFlags::ITALIC);
        // Fixtures use either four explicit faces or one regular source with
        // synthesis enabled. Reject partial face chains instead of reproducing
        // the renderer's font-resolution policy in the oracle.
        let faces = &config.font;
        let explicit = [&faces.bold, &faces.italic, &faces.bold_italic];
        let source = if explicit.iter().all(|face| face.is_none()) && faces.synthesize {
            &faces.regular
        } else if explicit.iter().all(|face| face.is_some()) {
            match (bold, italic) {
                (false, false) => &faces.regular,
                (true, false) => faces.bold.as_ref().unwrap(),
                (false, true) => faces.italic.as_ref().unwrap(),
                (true, true) => faces.bold_italic.as_ref().unwrap(),
            }
        } else {
            return Err(
                "oracle requires four explicit faces or a regular-only synthesized fixture".into(),
            );
        };
        let font = TextFont {
            font: source.clone(),
            font_size: font_size.into(),
            weight: if bold {
                FontWeight::BOLD
            } else {
                FontWeight::NORMAL
            },
            style: if italic {
                FontStyle::Italic
            } else {
                FontStyle::Normal
            },
            ..default()
        };
        let mut computed = ComputedTextBlock::default();
        self.pipeline
            .update_buffer(
                &self.fonts,
                std::iter::once((
                    Entity::PLACEHOLDER,
                    0,
                    cell.symbol(),
                    &font,
                    Color::WHITE,
                    LineHeight::Px(line_height),
                    LetterSpacing::default(),
                )),
                LineBreak::NoWrap,
                Justify::Left,
                TextBounds::UNBOUNDED,
                1.0,
                &mut computed,
                &mut self.font_cx,
                &mut self.layout_cx,
                Vec2::splat(4096.0),
                20.0,
            )
            .map_err(|e| e.to_string())?;
        let mut layout = TextLayoutInfo::default();
        self.pipeline
            .update_text_layout_info(
                &mut layout,
                &mut self.atlases,
                &mut self.images,
                &mut computed,
                &mut self.scale_cx,
                TextBounds::UNBOUNDED,
                Justify::Left,
                config.raster.hinting,
            )
            .map_err(|e| e.to_string())?;
        let line = computed.buffer().lines().next().ok_or("missing line")?;
        let metrics = line.metrics();
        let mut reference = Reference {
            origin: IVec2::ZERO,
            size: UVec2::ZERO,
            rgba: Vec::new(),
            layers: Vec::new(),
            color_glyphs: layout
                .glyphs
                .iter()
                .filter(|g| !g.atlas_info.is_alpha_mask)
                .count(),
            glyph_count: layout.glyphs.len(),
            baseline: metrics.baseline,
            ascent: metrics.ascent,
            descent: metrics.descent,
            advance: metrics.advance,
            supported: line.runs().all(|run| {
                run.clusters()
                    .all(|cluster| cluster.glyphs().all(|glyph| glyph.id != 0))
            }),
            faces: line
                .runs()
                .map(|run| (run.font().data.id(), run.font().index))
                .collect(),
        };
        if layout.glyphs.is_empty() {
            return Err("no rasterized glyphs".into());
        }
        // Integer translation with ties toward +infinity preserves identical
        // rasterization phase on either side of zero. No terminal fit is used.
        let origins: Vec<IVec2> = layout
            .glyphs
            .iter()
            .map(|glyph| {
                (glyph.position - glyph.atlas_info.rect.size() / 2.0 + Vec2::splat(0.5))
                    .floor()
                    .as_ivec2()
            })
            .collect();
        let min = origins.iter().copied().reduce(IVec2::min).unwrap();
        let max = layout
            .glyphs
            .iter()
            .zip(&origins)
            .map(|(glyph, origin)| *origin + glyph.atlas_info.rect.size().as_ivec2())
            .reduce(IVec2::max)
            .unwrap();
        reference.origin = min;
        reference.size = (max - min).as_uvec2();
        reference
            .rgba
            .resize((reference.size.x * reference.size.y) as usize, Vec4::ZERO);
        for (glyph, origin) in layout.glyphs.iter().zip(origins) {
            let image = self
                .images
                .get(glyph.atlas_info.texture)
                .ok_or("missing glyph atlas")?;
            if image.texture_descriptor.format
                != bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb
            {
                return Err("reference requires Bevy's RGBA8 sRGB glyph atlas".into());
            }
            let data = image.data.as_ref().ok_or("unreadable glyph atlas")?;
            let rect = glyph.atlas_info.rect;
            let size = rect.size().as_uvec2();
            let mut layer = Layer {
                origin,
                size,
                rgba: Vec::with_capacity((size.x * size.y) as usize),
                mask: glyph.atlas_info.is_alpha_mask,
            };
            for y in 0..size.y {
                for x in 0..size.x {
                    let source = ((rect.min.y as u32 + y) * image.width() + rect.min.x as u32 + x)
                        as usize
                        * 4;
                    // Bevy stores mask glyphs as white RGB plus coverage and
                    // CBDT color glyphs as straight RGBA. The sRGB atlas decodes
                    // RGB before the shader's straight-alpha blend.
                    let color = LinearRgba::from(Srgba::from_u8_array(
                        data[source..source + 4].try_into().unwrap(),
                    ));
                    let sample = Vec4::new(
                        color.red * color.alpha,
                        color.green * color.alpha,
                        color.blue * color.alpha,
                        color.alpha,
                    );
                    let target = (origin - min).as_uvec2() + UVec2::new(x, y);
                    let dest =
                        &mut reference.rgba[(target.y * reference.size.x + target.x) as usize];
                    *dest = sample + *dest * (1.0 - sample.w);
                    layer.rgba.push(sample);
                }
            }
            reference.layers.push(layer);
        }
        Ok(reference)
    }
}

/// Codepoints constrained as symbols, after Ghostty: arrows, enclosed
/// alphanumerics, miscellaneous symbols, dingbats, pictographs, emoticons,
/// transport and map symbols, and private use. Restated here so the
/// expectation does not depend on the renderer's classification.
pub fn is_symbol(symbol: &str) -> bool {
    symbol.chars().next().is_some_and(|c| {
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

/// Codepoints Ghostty draws procedurally at cell size instead of taking
/// them from the font (`font/sprite/Face.zig`): box drawing, block elements,
/// Braille, four geometric triangles, the Powerline and branch subsets and
/// Symbols for Legacy Computing. Restated here so the harness does not depend
/// on the renderer's classification; the sprites themselves are checked
/// against Ghostty's reference atlases and by the tile panels.
pub fn is_sprite(symbol: &str) -> bool {
    let mut chars = symbol.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return false;
    };
    matches!(
        u32::from(c),
        0x2500..=0x259f
            | 0x25e2..=0x25e5
            | 0x25f8..=0x25fa
            | 0x25ff
            | 0x2800..=0x28ff
            | 0xe0b0..=0xe0bf
            | 0xe0d2
            | 0xe0d4
            | 0xf5d0..=0xf60d
            | 0x1fb00..=0x1fbaf
            | 0x1fbbd..=0x1fbbf
            | 0x1fbce..=0x1fbef
            | 0x1cc1b..=0x1cc1e
            | 0x1cc21..=0x1cc3f
            | 0x1cd00..=0x1cde5
            | 0x1ce00
            | 0x1ce01
            | 0x1ce0b
            | 0x1ce0c
            | 0x1ce16..=0x1ce19
            | 0x1ce51..=0x1ceaf
    )
}

/// Sprites whose diagonal strokes deliberately overshoot their cell (half a
/// pixel along the diagonal, so diagonals tile; the butt caps' corners reach
/// a little further); their neighbours' two outermost pixels are not judged
/// against a raster that lacks them.
pub fn sprite_overshoots(symbol: &str) -> bool {
    symbol.chars().next().is_some_and(|c| {
        matches!(
            u32::from(c),
            0x2571..=0x2573 | 0xe0b9 | 0xe0bb | 0xe0bd | 0xe0bf | 0x1fba0..=0x1fbae | 0x1fbd0..=0x1fbdf
        )
    })
}

/// Cells the anchor at `column` occupies: its declared span, cut at the row's
/// end and at the first cell that is not its continuation.
pub fn span(cells: &[TerminalCell], column: usize) -> u32 {
    let declared = usize::from(cells[column].columns()).min(cells.len() - column);
    (1..declared)
        .take_while(|offset| cells[column + offset].is_continuation())
        .count() as u32
        + 1
}

/// Cells the run at `column` may visually occupy, after Ghostty's
/// `constraintWidth`: its span, or two cells for a one-cell symbol
/// before a blank cell that is not the row's last cell and does not follow
/// another symbol (Powerline graphics excepted).
pub fn visual_columns(cells: &[TerminalCell], column: usize) -> u32 {
    let span = span(cells, column);
    if span != 1 || column + 1 >= cells.len() || !is_symbol(cells[column].symbol()) {
        return span;
    }
    let previous = column.checked_sub(1).map(|c| cells[c].symbol());
    if previous.is_some_and(|p| {
        is_symbol(p)
            && !p
                .chars()
                .next()
                .is_some_and(|c| matches!(u32::from(c), 0xe0b0..=0xe0d7))
    }) {
        return 1;
    }
    let next = cells[column + 1].symbol();
    if next.is_empty() || next == " " || next == "\u{2002}" {
        2
    } else {
        1
    }
}

/// The primary face's box in physical cell pixels, from raw font metrics:
/// its advance, its line height (ascent + descent + line gap) around the
/// shared baseline, and the whole-pixel shift centering text in wider cells.
#[derive(Clone, Copy, Debug)]
pub struct Face {
    pub width: f32,
    pub height: f32,
    /// Top of the face box below the cell's top edge.
    pub top: f32,
    pub dx: f32,
}

/// Expected placement of one cell's run, from raw rasters and Ghostty's rules.
pub struct Placement {
    /// The run's raster at the size it is expected to be drawn at.
    pub reference: Reference,
    /// Whole-pixel translation from the run's line origin to the cell origin.
    pub shift: IVec2,
    /// Whether the run was rescaled by its constraint.
    pub scaled: bool,
    /// Size of the box the constraint measures, unconstrained.
    pub unconstrained: Vec2,
    /// Size of that box after Ghostty's constraint, before whole-pixel font
    /// sizes; `None` for unconstrained runs.
    pub target: Option<Vec2>,
    /// Size of that box as rasterized.
    pub measured: Vec2,
}

impl Placement {
    /// The ink rectangle relative to the cell origin.
    pub fn ink(&self) -> Option<(IVec2, IVec2)> {
        self.reference
            .ink_bounds()
            .map(|(min, max)| (min + self.shift, max + self.shift))
    }
}

/// Ghostty's `fit` and emoji `cover` rules on a box `size` at `min` (cell
/// pixels, y down) over `columns` cells, restated from `font/Glyph.zig`: the
/// target size scales against `columns` face widths and the face height, the
/// position against the span from the first cell's left edge to the last
/// face's right edge. Returns the target box's top-left corner and size.
pub fn ghostty_box(
    min: Vec2,
    size: Vec2,
    face: Face,
    cell_width: f32,
    columns: u32,
    cover: bool,
) -> (Vec2, Vec2) {
    let pad = if cover { 0.025 * face.width } else { 0.0 };
    let width = columns as f32 * face.width - 2.0 * pad;
    let factor = (width / size.x).min(face.height / size.y);
    let factor = if cover { factor } else { factor.min(1.0) };
    let scaled = size * factor;
    let center = min + size / 2.0;
    let span = face.width + (columns - 1) as f32 * cell_width;
    let (left, right) = (pad, span - scaled.x - pad);
    let (top, bottom) = (face.top, face.top + face.height - scaled.y);
    let x = if cover {
        left.max((left + right) / 2.0)
    } else {
        (center.x - scaled.x / 2.0).min(right).max(left)
    };
    // The bottom edge wins when the box is too tall; too tall to fit at all
    // centers it.
    let y = if cover || bottom < top {
        (top + bottom) / 2.0
    } else {
        (center.y - scaled.y / 2.0).max(top).min(bottom)
    };
    (Vec2::new(x + face.dx, y), scaled)
}

impl Rasterizer<'_> {
    /// The configured regular face's box for a terminal drawn at `font_size`
    /// in `cell` pixels on the shared `baseline`.
    pub fn face(
        &mut self,
        config: &TerminalRenderConfig,
        font_size: f32,
        cell: UVec2,
        baseline: i32,
    ) -> Result<Face, String> {
        use skrifa::MetadataProvider as _;
        let zero = self.shape(&TerminalCell::new("0"), config, font_size, cell.y as f32)?;
        let bevy::text::FontSource::Handle(handle) = &config.font.regular else {
            return Err("the oracle's face box requires a font asset".into());
        };
        let font = self.fonts.get(handle.id()).ok_or("font asset missing")?;
        let file = skrifa::FontRef::from_index(font.data.data(), 0).map_err(|e| e.to_string())?;
        let metrics = file.metrics(
            skrifa::instance::Size::new(font_size),
            skrifa::instance::LocationRef::default(),
        );
        let line_gap = metrics.leading.max(0.0);
        let dx = if zero.advance < cell.x as f32 {
            ((cell.x as f32 - zero.advance) / 2.0).round()
        } else {
            0.0
        };
        Ok(Face {
            width: zero.advance,
            height: zero.ascent + zero.descent + line_gap,
            top: baseline as f32 - zero.ascent - line_gap / 2.0,
            dx,
        })
    }

    /// Places `cell` drawn over `columns` cells of `cell_size` physical
    /// pixels, on the terminal's shared `baseline`, under the rules in the
    /// module documentation. Rescaled runs are rasterized at whole-pixel font
    /// sizes (measured again when hinting leaves a fitted run a pixel too
    /// large) and centered on Ghostty's target box.
    #[allow(clippy::too_many_arguments)]
    pub fn place(
        &mut self,
        cell: &TerminalCell,
        config: &TerminalRenderConfig,
        font_size: f32,
        cell_size: UVec2,
        columns: u32,
        baseline: i32,
        face: Face,
    ) -> Result<Placement, String> {
        let mut reference = self.shape(cell, config, font_size, cell_size.y as f32)?;
        let dy = baseline - reference.baseline.round() as i32;
        let color = reference.color_glyphs > 0;
        // Colour glyphs are measured by their whole bitmaps, outlines by ink.
        let measure = |reference: &Reference| -> Result<(Vec2, Vec2), String> {
            if color {
                Ok((reference.origin.as_vec2(), reference.size.as_vec2()))
            } else {
                let (min, max) = reference.ink_bounds().ok_or("empty raster")?;
                Ok((min.as_vec2(), (max - min).as_vec2()))
            }
        };
        let (min, size) = measure(&reference)?;
        if !color && !is_symbol(cell.symbol()) {
            let dx = face.dx as i32;
            return Ok(Placement {
                reference,
                shift: IVec2::new(dx, dy),
                scaled: false,
                unconstrained: size,
                target: None,
                measured: size,
            });
        }
        let columns = columns.min(2);
        let (target_min, target_size) = ghostty_box(
            min + Vec2::new(0.0, dy as f32),
            size,
            face,
            cell_size.x as f32,
            columns,
            color,
        );
        let mut font = font_size;
        let mut scale = (target_size / size).y;
        let mut drawn = (min, size);
        if (scale - 1.0).abs() > 1e-3 {
            for _ in 0..3 {
                let next = (font * scale).floor().clamp(1.0, font_size * 8.0);
                if next == font {
                    break;
                }
                font = next;
                reference = self.shape(cell, config, font, cell_size.y as f32)?;
                drawn = measure(&reference)?;
                if color || (drawn.1 - target_size).max_element() <= 0.5 {
                    break;
                }
                scale = (target_size / drawn.1).min_element().min(1.0 - 1e-3);
            }
        }
        // Center the rasterized box on the target box, in whole pixels
        // (ties toward +infinity).
        let corner = target_min + (target_size - drawn.1) / 2.0;
        let shift = (corner - drawn.0 + Vec2::splat(0.5)).floor().as_ivec2();
        Ok(Placement {
            reference,
            shift,
            scaled: font != font_size,
            unconstrained: size,
            target: Some(target_size),
            measured: drawn.1,
        })
    }
}

/// Whole-pixel shift keeping ink `left..right` (texture pixels) inside a
/// texture `width` pixels wide: the renderer's substitute for Ghostty's
/// window padding. Ink wider than the texture keeps its place.
pub fn edge_shift(left: i32, right: i32, width: i32) -> i32 {
    if right - left > width {
        0
    } else if left < 0 {
        -left
    } else if right > width {
        width - right
    } else {
        0
    }
}

/// Code values of sRGB difference tolerated for one blend: a software Vulkan
/// rasterizer blends with 8-bit linear precision, which is two code values in
/// dark sRGB; never a silhouette threshold.
pub const BLEND_TOLERANCE: u8 = 2;

/// Number of pixels differing by more than [`BLEND_TOLERANCE`] in any channel.
pub fn differing_pixels(expected: &[[u8; 3]], actual: &[[u8; 3]]) -> usize {
    differing_pixels_within(expected, actual, &vec![1; expected.len()])
}

/// [`differing_pixels`] with the number of blends each pixel went through:
/// every draw over an 8-bit target rounds once more (a run overflowing onto
/// its neighbour's ink), so each layer adds one code value.
pub fn differing_pixels_within(expected: &[[u8; 3]], actual: &[[u8; 3]], layers: &[u8]) -> usize {
    assert_eq!(expected.len(), actual.len());
    assert_eq!(expected.len(), layers.len());
    expected
        .iter()
        .zip(actual)
        .zip(layers)
        .filter(|((e, a), l)| pixel_difference(e, a) > BLEND_TOLERANCE + l.saturating_sub(1))
        .count()
}

/// Largest channel difference between two pixels.
pub fn pixel_difference(expected: &[u8; 3], actual: &[u8; 3]) -> u8 {
    expected
        .iter()
        .zip(actual)
        .map(|(e, a)| e.abs_diff(*a))
        .max()
        .unwrap_or(0)
}

/// Ghostty's `linear-corrected` coverage (`shaders.metal`, `cell_text_fragment`):
/// the coverage `a` of text of luminance `fg` over a background of luminance
/// `bg` is remapped so that blending in linear light gives the luminance a
/// blend of the sRGB-encoded luminances would give. Luminances within 0.001
/// of each other are left alone.
pub fn corrected_coverage(a: f32, fg: f32, bg: f32) -> f32 {
    if (fg - bg).abs() <= 0.001 {
        return a;
    }
    let encode = |v: f32| Srgba::from(LinearRgba::rgb(v, v, v)).red;
    let decode = |v: f32| LinearRgba::from(Srgba::rgb(v, v, v)).red;
    let blend = decode(encode(fg) * a + encode(bg) * (1.0 - a));
    ((blend - bg) / (fg - bg)).clamp(0.0, 1.0)
}

/// Relative luminance of a linear colour.
pub fn luminance(color: Vec3) -> f32 {
    color.dot(Vec3::new(0.2126, 0.7152, 0.0722))
}

/// One glyph texel blended over a stored sRGB pixel the way the renderer's
/// quads blend: straight alpha in linear light, then 8-bit sRGB storage.
fn blend(ink: Vec4, background: [u8; 3]) -> [u8; 3] {
    if ink.w == 0.0 {
        return background;
    }
    let bg = LinearRgba::from(Srgba::rgb(
        f32::from(background[0]) / 255.0,
        f32::from(background[1]) / 255.0,
        f32::from(background[2]) / 255.0,
    ));
    let srgb = Srgba::from(LinearRgba::rgb(
        ink.x + bg.red * (1.0 - ink.w),
        ink.y + bg.green * (1.0 - ink.w),
        ink.z + bg.blue * (1.0 - ink.w),
    ))
    .to_u8_array();
    [srgb[0], srgb[1], srgb[2]]
}

impl Reference {
    /// Draws this run at `shift` onto `canvas` (of `size`), one quad after
    /// the others already drawn: texels outside the canvas are dropped, texels
    /// over earlier runs blend over them.
    pub fn composite(&self, canvas: &mut [[u8; 3]], size: UVec2, shift: IVec2) {
        self.composite_within(canvas, &mut [], &[], size, shift);
    }

    /// [`Reference::composite`] in white text, glyph by glyph. `layers`, when
    /// as large as the canvas, counts the blends each pixel went through.
    /// `backgrounds`, when as large as the canvas, holds the luminance of
    /// the cell background under each pixel, and coverage glyphs are blended
    /// with Ghostty's linear correction against it.
    pub fn composite_within(
        &self,
        canvas: &mut [[u8; 3]],
        layers: &mut [u8],
        backgrounds: &[f32],
        size: UVec2,
        shift: IVec2,
    ) {
        assert_eq!(canvas.len(), (size.x * size.y) as usize);
        for layer in &self.layers {
            for y in 0..layer.size.y {
                for x in 0..layer.size.x {
                    let ink = layer.rgba[(y * layer.size.x + x) as usize];
                    let p = layer.origin + UVec2::new(x, y).as_ivec2() + shift;
                    if ink.w == 0.0
                        || p.x < 0
                        || p.y < 0
                        || p.x >= size.x as i32
                        || p.y >= size.y as i32
                    {
                        continue;
                    }
                    let index = (p.y as u32 * size.x + p.x as u32) as usize;
                    let ink = match backgrounds.get(index) {
                        Some(background) if layer.mask => {
                            let white = Vec3::ONE;
                            Vec4::splat(corrected_coverage(ink.w, luminance(white), *background))
                        }
                        _ => ink,
                    };
                    canvas[index] = blend(ink, canvas[index]);
                    if let Some(layer) = layers.get_mut(index) {
                        *layer = layer.saturating_add(1);
                    }
                }
            }
        }
    }

    pub fn ink_bounds(&self) -> Option<(IVec2, IVec2)> {
        let mut min = IVec2::splat(i32::MAX);
        let mut max = IVec2::splat(i32::MIN);
        for y in 0..self.size.y {
            for x in 0..self.size.x {
                if self.rgba[(y * self.size.x + x) as usize].w > 0.0 {
                    let p = self.origin + UVec2::new(x, y).as_ivec2();
                    min = min.min(p);
                    max = max.max(p + IVec2::ONE);
                }
            }
        }
        (min.x < max.x).then_some((min, max))
    }

    pub fn pixel(&self, position: IVec2, background: [u8; 3]) -> [u8; 3] {
        let p = position - self.origin;
        let ink = if p.x >= 0 && p.y >= 0 && p.x < self.size.x as i32 && p.y < self.size.y as i32 {
            self.rgba[(p.y as u32 * self.size.x + p.x as u32) as usize]
        } else {
            Vec4::ZERO
        };
        blend(ink, background)
    }
}

/// Writes a diagnostic image without involving the terminal renderer.
pub fn save_png(path: &std::path::Path, size: UVec2, pixels: Vec<u8>) {
    use bevy::{
        asset::RenderAssetUsages,
        render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    };
    Image::new(
        Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD,
    )
    .try_into_dynamic()
    .expect("RGBA8 image")
    .save(path)
    .expect("save diagnostic PNG");
}

/// Native pixels and an 8x nearest-neighbor detail, for visual review.
pub fn save_detail(path: &std::path::Path, size: UVec2, pixels: Vec<u8>) {
    let zoom = size * 8;
    let mut expanded = Vec::with_capacity((zoom.x * zoom.y * 4) as usize);
    for y in 0..zoom.y {
        for x in 0..zoom.x {
            let index = (((y / 8) * size.x + x / 8) * 4) as usize;
            expanded.extend_from_slice(&pixels[index..index + 4]);
        }
    }
    save_png(&path.with_extension("8x.png"), zoom, expanded);
    save_png(path, size, pixels);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference(origin: IVec2, size: UVec2, alpha: &[f32]) -> Reference {
        let rgba: Vec<Vec4> = alpha.iter().map(|a| Vec4::splat(*a)).collect();
        Reference {
            origin,
            size,
            layers: vec![Layer {
                origin,
                size,
                rgba: rgba.clone(),
                mask: true,
            }],
            rgba,
            color_glyphs: 0,
            glyph_count: 1,
            baseline: 2.0,
            ascent: 2.0,
            descent: 1.0,
            advance: 1.0,
            faces: Vec::new(),
            supported: true,
        }
    }

    fn render(reference: &Reference, size: UVec2, shift: IVec2) -> Vec<[u8; 3]> {
        (0..size.y)
            .flat_map(|y| {
                (0..size.x).map(move |x| {
                    reference.pixel(UVec2::new(x, y).as_ivec2() - shift, [40, 44, 64])
                })
            })
            .collect()
    }

    #[test]
    fn negative_bearings_and_faint_pixels_survive_the_reference() {
        let glyph = reference(
            IVec2::new(-1, -2),
            UVec2::new(3, 1),
            &[1.0, 0.0, 1.0 / 255.0],
        );
        assert_eq!(
            glyph.ink_bounds(),
            Some((IVec2::new(-1, -2), IVec2::new(2, -1)))
        );
        assert_eq!(glyph.pixel(IVec2::new(-1, -2), [40, 44, 64]), [255; 3]);
        assert_ne!(glyph.pixel(IVec2::new(1, -2), [40, 44, 64]), [40, 44, 64]);
    }

    #[test]
    fn equal_ink_counts_do_not_hide_changed_shapes_or_missing_pixels() {
        let glyph = reference(IVec2::ZERO, UVec2::new(2, 2), &[1.0, 0.0, 1.0, 1.0]);
        let size = UVec2::new(3, 2);
        let expected = render(&glyph, size, IVec2::ZERO);
        let mut pixels = expected.clone();
        assert_eq!(differing_pixels(&expected, &pixels), 0);
        pixels.swap(0, 1); // Three ink pixels remain, but the shape changes.
        assert_eq!(differing_pixels(&expected, &pixels), 2);
        pixels[1] = [40, 44, 64];
        assert_eq!(differing_pixels(&expected, &pixels), 1);
    }

    #[test]
    fn composites_drop_ink_outside_the_row_and_blend_later_runs_over_earlier_ones() {
        let glyph = reference(
            IVec2::new(-1, -1),
            UVec2::new(4, 2),
            &[0.2, 0.4, 0.6, 0.8, 1.0, 0.0, 0.3, 1.0],
        );
        let size = UVec2::new(3, 2);
        let bg = [40, 44, 64];
        let mut canvas = vec![bg; 6];
        // Shifted down by one, the glyph's rows land on both canvas rows and
        // its first column (x = -1) is dropped; every kept texel is the
        // single-blend value, transparent texels leave the background.
        glyph.composite(&mut canvas, size, IVec2::new(0, 1));
        assert_eq!(canvas, render(&glyph, size, IVec2::new(0, 1)));
        assert_eq!(canvas[3], bg);
        assert_eq!(canvas[5], [255; 3]);
        // Unshifted, the glyph's top row lies above the canvas and is dropped;
        // its second row lands on the canvas's first row.
        let mut upper = vec![bg; 6];
        glyph.composite(&mut upper, size, IVec2::ZERO);
        assert_eq!(&upper[3..], &[bg; 3]);
        assert_eq!(&upper[..3], &render(&glyph, size, IVec2::new(0, 1))[3..]);
        // A later run over the same pixel blends over the stored 8-bit value.
        let overlay = reference(IVec2::ZERO, UVec2::ONE, &[0.5]);
        let under = canvas[0];
        overlay.composite(&mut canvas, size, IVec2::ZERO);
        assert_eq!(canvas[0], overlay.pixel(IVec2::ZERO, under));
        assert_ne!(canvas[0], under);
    }

    #[test]
    fn color_tolerance_is_a_few_code_values_not_a_silhouette_threshold() {
        let glyph = reference(IVec2::ZERO, UVec2::ONE, &[0.5]);
        let expected = render(&glyph, UVec2::ONE, IVec2::ZERO);
        let mut pixels = expected.clone();
        pixels[0][0] += BLEND_TOLERANCE;
        assert_eq!(differing_pixels(&expected, &pixels), 0);
        pixels[0][0] += 1;
        assert_eq!(differing_pixels(&expected, &pixels), 1);
        // A second blend over the pixel earns one more code value.
        assert_eq!(differing_pixels_within(&expected, &pixels, &[2]), 0);
        pixels[0][0] += 1;
        assert_eq!(differing_pixels_within(&expected, &pixels, &[2]), 1);
    }

    #[test]
    fn symbols_before_blank_cells_may_spread_and_ordinary_text_may_not() {
        let row = |text: &str| -> Vec<TerminalCell> {
            text.chars()
                .map(|c| TerminalCell::new(&c.to_string()))
                .collect()
        };
        assert_eq!(visual_columns(&row("→ "), 0), 2);
        assert_eq!(visual_columns(&row("→x"), 0), 1);
        assert_eq!(visual_columns(&row("→"), 0), 1);
        assert_eq!(visual_columns(&row("→→ "), 1), 1);
        assert_eq!(visual_columns(&row("\u{e0b0}→ "), 1), 2);
        assert!(is_sprite("─") && is_sprite("█") && is_sprite("⣿") && is_sprite("\u{e0b0}"));
        assert!(!is_sprite("◆") && !is_sprite("\u{e0c0}") && !is_sprite("─\u{301}"));
        assert_eq!(visual_columns(&row("∑ "), 0), 1);
        assert_eq!(
            visual_columns(&[TerminalCell::wide("🙂", 2), TerminalCell::new(" ")], 0),
            2
        );
    }

    #[test]
    fn coverage_glyphs_blend_with_ghosttys_linear_correction() {
        // White at half coverage over black: a gamma-space blend's luminance
        // (half the sRGB range), not linear light's 188.
        let corrected = corrected_coverage(0.5, 1.0, 0.0);
        assert!((corrected - 0.2140).abs() < 1e-3, "{corrected}");
        let glyph = reference(IVec2::ZERO, UVec2::ONE, &[0.5]);
        let mut canvas = vec![[0, 0, 0]];
        glyph.composite_within(&mut canvas, &mut [], &[0.0], UVec2::ONE, IVec2::ZERO);
        assert_eq!(canvas, vec![[127; 3]]);
        // Equal luminances and full or empty coverage are unchanged.
        assert_eq!(corrected_coverage(0.5, 0.3, 0.3005), 0.5);
        assert!((corrected_coverage(1.0, 1.0, 0.2) - 1.0).abs() < 1e-6);
        assert!(corrected_coverage(0.0, 1.0, 0.2).abs() < 1e-6);
    }

    #[test]
    fn color_reference_preserves_hue_and_blends_in_linear_space() {
        let mut glyph = reference(IVec2::ZERO, UVec2::ONE, &[0.5]);
        glyph.rgba[0] = Vec4::new(0.5, 0.0, 0.0, 0.5);
        // Half transparent red over opaque blue: linear 0.5 encodes to 188.
        assert_eq!(glyph.pixel(IVec2::ZERO, [0, 0, 255]), [188, 0, 188]);
        assert_ne!(glyph.pixel(IVec2::ZERO, [0, 0, 255]), [128, 0, 128]);
        assert_eq!(glyph.pixel(IVec2::ONE, [4, 8, 12]), [4, 8, 12]);
        assert_eq!(differing_pixels(&[[188, 0, 188]], &[[188, 188, 255]]), 1);
    }

    #[test]
    fn partial_face_chains_are_rejected_instead_of_emulating_production_resolution() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            bevy::text::TextPlugin,
        ))
        .init_asset::<Image>();
        let mut config = TerminalRenderConfig::default();
        config.font.bold = Some(bevy::text::FontSource::Monospace);
        let mut state = bevy::ecs::system::SystemState::<Rasterizer>::new(app.world_mut());
        let mut rasterizer = state.get_mut(app.world_mut()).unwrap();
        assert!(
            rasterizer
                .shape(&TerminalCell::new("W"), &config, 18.0, 24.0)
                .err()
                .is_some_and(|error| error.contains("four explicit faces"))
        );
    }

    #[test]
    fn font_line_metrics_are_independent_of_procedural_block_cell_height() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            bevy::text::TextPlugin,
        ))
        .init_asset::<Image>();
        let handle = app
            .world_mut()
            .resource_mut::<Assets<Font>>()
            .add(Font::from_bytes(
                include_bytes!("../../assets/fonts/cascadia-mono/CascadiaMono-Regular.ttf")
                    .to_vec(),
            ));
        app.update();
        let config = TerminalRenderConfig {
            font: bevy_terminal_ratatui::prelude::FontFaces::regular(handle),
            ..default()
        };
        let mut state = bevy::ecs::system::SystemState::<Rasterizer>::new(app.world_mut());
        let mut rasterizer = state.get_mut(app.world_mut()).unwrap();
        let small = rasterizer
            .shape(&TerminalCell::new("█"), &config, 18.77, 23.0)
            .unwrap();
        let large = rasterizer
            .shape(&TerminalCell::new("█"), &config, 18.77, 200.0)
            .unwrap();
        assert_eq!(small.ascent, large.ascent);
        assert_eq!(small.descent, large.descent);
        assert_eq!(small.rgba, large.rgba);
        assert!(
            large.size.y < 30,
            "raw block raster is not a 200px cell fill"
        );
        assert!(large.baseline > small.baseline);
    }
}
