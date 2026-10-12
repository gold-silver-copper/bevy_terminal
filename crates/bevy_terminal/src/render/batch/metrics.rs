//! Font measurement and effective raster geometry.
use super::constraint::Metrics as FaceMetrics;
use super::shaping::{RunLayout, shape_run};
use super::{Face, TerminalRenderConfig, TextContext};
use crate::render::{FontFaces, TerminalSizing};
use bevy::{
    prelude::*,
    text::{
        ComputedTextBlock, FontCx, FontSource, LayoutCx, LetterSpacing, LineHeight, RemSize,
        TextElement, TextPipeline,
    },
};

/// Number of probe glyphs shaped by [`measure_advance`].
const PROBE_GLYPHS: usize = 100;
/// Font size in logical pixels used to shape the probe run.
pub(crate) const PROBE_FONT_SIZE: f32 = 64.0;
/// Font size used while [`TerminalSizing::FitCellWidth`] has not been measured yet.
const UNMEASURED_FONT_SIZE: f32 = 16.0;

/// Why the regular font's advance could not be measured.
#[derive(Debug, PartialEq)]
pub(in crate::render) enum AdvanceError {
    /// The font asset is not registered with the font context yet.
    NotRegistered,
    /// Bevy could not lay the probe run out.
    Layout(bevy::text::TextError),
    /// The font produced a non-finite or non-positive advance.
    Invalid(f32),
}

impl std::fmt::Display for AdvanceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotRegistered => f.write_str("font asset has not registered"),
            Self::Layout(error) => write!(f, "{error}"),
            Self::Invalid(advance) => write!(f, "font produced invalid advance {advance}"),
        }
    }
}

/// Measures the average advance of the regular font at [`PROBE_FONT_SIZE`] by
/// shaping a run of `0` glyphs, preserving the reason measurement failed.
pub(in crate::render) fn measure_advance(
    faces: &FontFaces,
    fonts: &Assets<Font>,
    text_pipeline: &mut TextPipeline,
    font_cx: &mut FontCx,
    layout_cx: &mut LayoutCx,
) -> Result<f32, AdvanceError> {
    // A font asset is only usable once Bevy has registered it with the font
    // context (which assigns its alias); measuring before that would shape a
    // fallback font. Report "not yet" so the caller retries next frame.
    if let FontSource::Handle(handle) = &faces.regular
        && fonts
            .get(handle.id())
            .is_none_or(|font| font.alias.is_empty())
    {
        return Err(AdvanceError::NotRegistered);
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
                TextElement::Text {
                    text: probe.as_str(),
                    font: &font,
                    color: Color::WHITE,
                    line_height: LineHeight::Px(PROBE_FONT_SIZE),
                    letter_spacing: LetterSpacing::default(),
                },
            )),
            1.0,
            &TextLayout::new(Justify::Left, LineBreak::NoWrap),
            &mut computed,
            font_cx,
            layout_cx,
            Vec2::new(f32::MAX, f32::MAX),
            RemSize::default(),
        )
        .map_err(AdvanceError::Layout)?;
    let advance = measure.max.x / PROBE_GLYPHS as f32;
    if advance.is_finite() && advance > 0.0 {
        Ok(advance)
    } else {
        Err(AdvanceError::Invalid(advance))
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

/// Uniform vertical shift (whole physical pixels) of every text glyph:
/// centers `text_box`, the configured faces' typographic enclosure around
/// their shared baseline, in the cell, as Ghostty centers the face.
pub(crate) fn centered_offset(cell_height: f32, text_box: Option<GlyphBox>) -> f32 {
    text_box.map_or(0.0, |ink| {
        snap((cell_height - ink.height()) / 2.0 - ink.top)
    })
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
    /// Stroke width of procedural grid graphics: Ghostty's `box_thickness`,
    /// the font's underline thickness rounded up.
    pub(super) box_thickness: u32,
    /// Ghostty's grid metrics of the primary face, for glyph constraints.
    pub(super) face: FaceMetrics,
    /// Whole-pixel shift centering text in a cell wider than the face's
    /// advance, as Ghostty centers it (explicit `Fixed` cells).
    pub(super) face_dx: f32,
}

pub(super) fn physical_config(logical: LogicalMetrics, raster_scale: f32) -> RasterMetrics {
    let cell_size = (logical.cell_size * raster_scale).round().max(Vec2::ONE);
    let font_size = (logical.font_size * raster_scale).max(1.0);
    RasterMetrics {
        scale: raster_scale,
        cell_size,
        font_size,
        baseline: 0.0,
        glyph_offset: 0.0,
        box_thickness: (raster_scale.round() as u32).max(1),
        face: FaceMetrics::new(
            (f64::from(cell_size.x), f64::from(cell_size.y)),
            f64::from(cell_size.x),
            f64::from(cell_size.y),
            0.0,
            0.75 * f64::from(font_size),
        ),
        face_dx: 0.0,
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

/// Vertical metrics of the regular face in physical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct FaceLineMetrics {
    /// Ascent + descent + line gap: the face's line box.
    pub(super) height: f32,
    /// The typographic line gap (leading).
    pub(super) line_gap: f32,
    /// The height of capital letters, estimated as 75% of the ascent (as
    /// Ghostty does) when the font does not record it.
    pub(super) cap_height: f32,
    /// The underline thickness, estimated as 15% of the x-height (itself 75%
    /// of the cap height when unrecorded) as Ghostty does.
    pub(super) underline_thickness: f32,
}

impl FaceLineMetrics {
    /// The face's metrics as Bevy's text layout reports them for a shaped run
    /// (`OS/2` typographic metrics when the font asks for them, `hhea`
    /// otherwise, the way terminal emulators size their rows); `None` when
    /// they describe no line.
    pub(super) fn new(
        ascent: f32,
        descent: f32,
        leading: f32,
        cap_height: Option<f32>,
        x_height: Option<f32>,
        underline_thickness: f32,
    ) -> Option<Self> {
        let line_gap = leading.max(0.0);
        let height = ascent + descent.abs() + line_gap;
        let cap_height = cap_height.filter(|cap| *cap > 0.0).unwrap_or(0.75 * ascent);
        let x_height = x_height.filter(|x| *x > 0.0).unwrap_or(0.75 * cap_height);
        let underline_thickness = Some(underline_thickness)
            .filter(|thickness| *thickness > 0.0)
            .unwrap_or(0.15 * x_height);
        (height.is_finite() && height > 0.0).then_some(Self {
            height,
            line_gap,
            cap_height,
            underline_thickness,
        })
    }
}

/// Upper bound on cell-height refinement rounds.
pub(super) const FIT_ROUNDS: usize = 3;

/// Refines the physical metrics after the logical fit: sizes the font from the
/// rounded physical cell width (so a fractional raster scale cannot open seams
/// between advances), encloses the configured faces' typographic metrics in
/// whole pixels to center text vertically, and derives Ghostty's face box and
/// box-drawing thickness.
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
    // terminal emulators size rows. A multiplier below one asks for a tighter
    // row, so the text box does not grow it back.
    let may_grow = matches!(config.sizing, super::super::TerminalSizing::FitCellWidth(_))
        || (font_driven && line_height >= 1.0);
    let layout = RunLayout {
        config,
        raster,
        viewport: Vec2::splat(4096.0),
    };
    let face_line = shape_run("0", Face::Regular, layout, cx).and_then(|run| run.face);
    if (may_grow || font_driven)
        && let Some(face_line) = face_line
    {
        raster.cell_size.y = raster
            .cell_size
            .y
            .max((face_line.height * line_height).ceil());
    }
    let mut text_box: Option<GlyphBox> = None;
    // The regular face's exact ascent, descent and advance.
    let mut regular = None;
    for _ in 0..FIT_ROUNDS {
        text_box = None;
        for face in Face::ALL {
            let layout = RunLayout {
                config,
                raster,
                viewport: Vec2::splat(4096.0),
            };
            if let Some(run) = shape_run("0", face, layout, cx) {
                if face == Face::Regular {
                    raster.baseline = run.baseline;
                    regular = Some((run.ascent, run.descent, run.advance));
                }
                let shift = raster.baseline - run.baseline;
                let line_box = GlyphBox {
                    top: run.line_box.top + shift,
                    bottom: run.line_box.bottom + shift,
                };
                text_box = Some(text_box.map_or(line_box, |box_| box_.union(line_box)));
            }
        }
        let height = text_box.map_or(0.0, |box_| box_.height());
        if !may_grow || height <= raster.cell_size.y {
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
    raster.glyph_offset = centered_offset(raster.cell_size.y, text_box);
    raster.box_thickness = face_line.map_or(raster.box_thickness, |line| {
        (line.underline_thickness.ceil() as u32).max(1)
    });
    if let Some((ascent, descent, advance)) = regular {
        // Ghostty's face box: the regular face's line box (ascent, descent and
        // line gap, split evenly above and below) around the shared baseline.
        let line_gap = face_line.map_or(0.0, |line| line.line_gap);
        let cap_height = face_line.map_or(0.75 * ascent, |line| line.cap_height);
        let face_height = ascent + descent + line_gap;
        let face_top = raster.baseline + raster.glyph_offset - ascent - line_gap / 2.0;
        raster.face = FaceMetrics::new(
            (f64::from(raster.cell_size.x), f64::from(raster.cell_size.y)),
            f64::from(advance),
            f64::from(face_height),
            f64::from(raster.cell_size.y - face_top - face_height),
            f64::from(cap_height),
        );
        raster.face_dx = if advance < raster.cell_size.x {
            ((raster.cell_size.x - advance) / 2.0).round()
        } else {
            0.0
        };
    }
    debug!(
        "bevy_terminal: cell {}x{}px font {:.2}px text box {:?} offset {} box thickness {}",
        raster.cell_size.x,
        raster.cell_size.y,
        raster.font_size,
        text_box,
        raster.glyph_offset,
        raster.box_thickness
    );
    raster
}

/// The inked columns of an atlas glyph (every column when the atlas has no
/// CPU data).
pub(super) fn inked_columns(image: &Image, rect: Rect) -> InkSpan {
    let width = rect.size().x.max(0.0) as usize;
    let Some(data) = image.data.as_ref() else {
        // Unreadable pixels count as ink.
        return InkSpan::of_columns((0..width).map(|_| true));
    };
    let atlas_width = image.texture_descriptor.size.width as usize;
    let (x0, y0, y1) = (
        rect.min.x as usize,
        rect.min.y as usize,
        rect.max.y as usize,
    );
    InkSpan::of_columns((0..width).map(|x| {
        (y0..y1).any(|y| {
            data.get((y * atlas_width + x0 + x) * 4 + 3)
                .is_some_and(|alpha| *alpha > 0)
        })
    }))
}

/// The horizontal extent `[left, right)` of a bitmap's inked columns,
/// relative to the bitmap; empty for a transparent bitmap.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct InkSpan {
    pub(super) left: f32,
    pub(super) right: f32,
}

impl InkSpan {
    /// The span of the columns, left to right, that hold ink.
    pub(super) fn of_columns(inked: impl IntoIterator<Item = bool>) -> Self {
        let mut first = None;
        let mut last = 0;
        for (column, inked) in inked.into_iter().enumerate() {
            if inked {
                first.get_or_insert(column);
                last = column;
            }
        }
        first.map_or_else(Self::default, |first| Self {
            left: first as f32,
            right: last as f32 + 1.0,
        })
    }

    pub(super) fn is_empty(self) -> bool {
        self.right <= self.left
    }
}
