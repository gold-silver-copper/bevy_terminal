//! Rendering: the [`TerminalPlugin`], the [`Terminal`] component and its
//! configuration, the renderer-owned [`TerminalTexture`], per-terminal
//! [`TerminalStats`], and the [`TerminalTheme`]. Add the plugin once and spawn
//! a `Terminal` (plus an `ImageNode` to show it) per rendered surface.

use bevy::{
    ecs::schedule::SystemSet,
    prelude::*,
    text::{
        ComputedTextBlock, FontCx, FontHinting, FontSource, FontStyle, FontWeight, LayoutCx,
        LetterSpacing, LineHeight, TextPipeline,
    },
};

use crate::scene::{StyleFlags, TerminalCell, TerminalSnapshot};

mod batch;
mod color;
mod fonts;
#[cfg(feature = "3d")]
mod world_quad;

pub use batch::{
    Terminal, TerminalPlugin, TerminalReady, TerminalRemeasured, TerminalStats, TerminalTexture,
    grid_for, grid_for_window, raster_scale_for_window,
};
pub use color::TerminalTheme;
use color::dim;
pub use fonts::{SmolStr, TerminalFonts, font_family};
#[cfg(feature = "3d")]
pub use world_quad::TerminalWorldQuad;

/// Visual shape used for the terminal cursor.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CursorStyle {
    /// A translucent rectangle covering the entire cell.
    #[default]
    Block,
    /// A two-logical-pixel bar at the cell's left edge.
    Bar,
    /// A two-logical-pixel line at the cell's bottom edge.
    Underline,
}

/// Cursor appearance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CursorConfig {
    /// Cursor shape.
    pub style: CursorStyle,
    /// Cursor overlay color.
    pub color: Color,
    /// Cursor blink frequency; `None` disables blinking.
    pub blink_hz: Option<f32>,
}

impl Default for CursorConfig {
    fn default() -> Self {
        Self {
            style: CursorStyle::Block,
            color: Color::srgba(0.82, 0.88, 1.0, 0.48),
            blink_hz: Some(1.0),
        }
    }
}

/// Text blink frequencies; `None` disables the corresponding attribute.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlinkConfig {
    /// Frequency of [`StyleFlags::SLOW_BLINK`].
    pub slow_hz: Option<f32>,
    /// Frequency of [`StyleFlags::RAPID_BLINK`].
    pub rapid_hz: Option<f32>,
}

impl Default for BlinkConfig {
    fn default() -> Self {
        Self {
            slow_hz: Some(1.0),
            rapid_hz: Some(3.0),
        }
    }
}

impl BlinkConfig {
    /// Disables text blinking entirely.
    pub const NONE: Self = Self {
        slow_hz: None,
        rapid_hz: None,
    };
}

/// The font faces used for regular, bold, italic and bold-italic text.
///
/// Missing faces fall back in the order bold-italic → bold → italic → regular
/// with the corresponding weight/style requested from the fallback face.
///
/// When a style falls back to another face, `synthesize` (default `true`)
/// controls whether the bold weight / italic style is still *requested* from
/// that face, so a variable font or a system family can produce it from its own
/// axes or sibling faces. With `synthesize` off, the fallback face is used
/// exactly as is (no faux bold or oblique).
#[derive(Clone, Debug, PartialEq)]
pub struct FontFaces {
    /// Regular text. The generic monospace family enables system fallback.
    pub regular: FontSource,
    /// Optional face for bold text.
    pub bold: Option<FontSource>,
    /// Optional face for italic text.
    pub italic: Option<FontSource>,
    /// Optional face for text that is both bold and italic.
    pub bold_italic: Option<FontSource>,
    /// Whether to request bold weight / italic style from a fallback face.
    pub synthesize: bool,
}

impl FontFaces {
    /// Uses `regular` for every style, relying on the family's own weight and
    /// style axes.
    #[must_use]
    pub fn regular(regular: impl Into<FontSource>) -> Self {
        Self {
            regular: regular.into(),
            bold: None,
            italic: None,
            bold_italic: None,
            synthesize: true,
        }
    }

    /// Sets whether missing faces are synthesized (see the type docs).
    #[must_use]
    pub const fn with_synthesis(mut self, synthesize: bool) -> Self {
        self.synthesize = synthesize;
        self
    }

    /// Returns the face for a style together with the weight and style to
    /// request from it.
    fn resolve(&self, bold: bool, italic: bool) -> (&FontSource, FontWeight, FontStyle) {
        let (face, exact) = match (bold, italic) {
            (true, true) => self
                .bold_italic
                .as_ref()
                .map(|face| (face, true))
                .or_else(|| self.bold.as_ref().map(|face| (face, false)))
                .or_else(|| self.italic.as_ref().map(|face| (face, false))),
            (true, false) => self.bold.as_ref().map(|face| (face, true)),
            (false, true) => self.italic.as_ref().map(|face| (face, true)),
            (false, false) => Some((&self.regular, true)),
        }
        .unwrap_or((&self.regular, false));
        let request = exact || self.synthesize;
        let weight = if bold && request {
            FontWeight::BOLD
        } else {
            FontWeight::NORMAL
        };
        let style = if italic && request {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        };
        (face, weight, style)
    }

    /// Returns the face used for the given weight and style.
    #[must_use]
    pub fn select(&self, bold: bool, italic: bool) -> &FontSource {
        match (bold, italic) {
            (true, true) => self
                .bold_italic
                .as_ref()
                .or(self.bold.as_ref())
                .or(self.italic.as_ref()),
            (true, false) => self.bold.as_ref(),
            (false, true) => self.italic.as_ref(),
            (false, false) => None,
        }
        .unwrap_or(&self.regular)
    }
}

impl Default for FontFaces {
    fn default() -> Self {
        Self::regular(FontSource::Monospace)
    }
}

impl<T: Into<FontSource>> From<T> for FontFaces {
    fn from(regular: T) -> Self {
        Self::regular(regular)
    }
}

/// How the rasterized font size is chosen.
///
/// # Vertical fit
///
/// Whichever variant is used, the renderer measures the primary font after
/// choosing the size: the rows a full block (`█`) covers completely are the
/// font's *line box*, and the ink of an ASCII probe (`gjpqy|[]{}()_`) in every
/// face plus an accented-capital probe are measured too. Glyphs are then
/// shifted by one uniform whole-pixel offset per terminal so that (in this
/// order) the block keeps covering the cell (tiles of blocks stay seamless),
/// the ASCII probe is fully inside the cell, and accented capitals are inside
/// when the font leaves room. Nothing about the font is assumed; the
/// measurement is repeated whenever fonts or the configuration change and
/// never per frame.
///
/// With [`FitCellWidth`](Self::FitCellWidth), the configured cell height is a
/// *minimum*; [`CellSizing::FromFont`] instead takes the measured line box as
/// its height. In either mode a primary-font glyph inside the line box is never
/// clipped and blocks tile with no seam. With [`Px`](Self::Px) in a
/// [`CellSizing::Logical`] cell both sizes are honored exactly and glyphs
/// beyond the cell are clipped after the same fitting. Fallback-font glyphs,
/// italics that overhang their advance and accents that a font draws above its
/// own line box are centered/pushed into the cell (dropping the faintest
/// columns when they are wider than it) and clipped as a last resort. A
/// sub-pixel overshoot — the fraction of a column that box-drawing and block
/// glyphs are drawn past their advance so strokes overlap — is not overhang:
/// the run keeps its bearings and the cell clips the column, so `┌` stays
/// aligned with `│`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum FontSizing {
    /// Measure the regular font's advance width by shaping it and pick the
    /// font size at which one glyph advance equals the *physical* cell width,
    /// so box-drawing and block glyphs designed to fill their advance tile the
    /// grid without seams at every raster scale. Assumes a monospaced primary
    /// font. The cell height is at least the font's line box (see above).
    #[default]
    FitCellWidth,
    /// Request this size in logical pixels. With an explicit cell this is used
    /// exactly. With [`CellSizing::FromFont`], the physical size is adjusted
    /// just enough for the measured advance to equal the cell's whole-pixel
    /// width, preventing seams in block and box-drawing glyphs; read the
    /// effective size from [`TerminalTexture::font_size`](crate::render::TerminalTexture::font_size).
    Px(f32),
}

/// Number of probe glyphs shaped by [`measure_advance`].
const PROBE_GLYPHS: usize = 100;
/// Font size in logical pixels used to shape the probe run.
pub(crate) const PROBE_FONT_SIZE: f32 = 64.0;
/// Font size used while [`FontSizing::FitCellWidth`] has not been measured yet.
const UNMEASURED_FONT_SIZE: f32 = 16.0;

/// Measures the average advance of the regular font at [`PROBE_FONT_SIZE`] by
/// shaping a run of `0` glyphs; returns `None` until the font can be shaped.
fn measure_advance(
    faces: &FontFaces,
    fonts: &Assets<Font>,
    text_pipeline: &mut TextPipeline,
    font_cx: &mut FontCx,
    layout_cx: &mut LayoutCx,
) -> Option<f32> {
    // A font asset is only usable once Bevy has registered it with the font
    // context (which assigns its alias); measuring before that would shape a
    // fallback font. Report "not yet" so the caller retries next frame.
    if let FontSource::Handle(handle) = &faces.regular
        && fonts
            .get(handle.id())
            .is_none_or(|font| font.alias.is_empty())
    {
        return None;
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
        .ok()?;
    let advance = measure.max.x / PROBE_GLYPHS as f32;
    (advance.is_finite() && advance > 0.0).then_some(advance)
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
fn resolve_metrics(config: &TerminalRenderConfig, measured_advance: Option<f32>) -> LogicalMetrics {
    let advance_per_px = measured_advance.map(|advance| advance / PROBE_FONT_SIZE);
    match (config.cell_size, config.font_size) {
        (CellSizing::Logical(cell), FontSizing::Px(size)) => LogicalMetrics {
            font_size: size.max(1.0),
            cell_size: cell,
        },
        (CellSizing::Logical(cell), FontSizing::FitCellWidth) => LogicalMetrics {
            font_size: advance_per_px
                .map_or(UNMEASURED_FONT_SIZE, |ratio| (cell.x / ratio).max(1.0)),
            cell_size: cell,
        },
        (CellSizing::FromFont { .. }, FontSizing::Px(size)) => {
            let font_size = size.max(1.0);
            let cell_size = advance_per_px.map_or(Vec2::ONE, |ratio| {
                Vec2::new((ratio * font_size).max(1.0), 1.0)
            });
            LogicalMetrics {
                font_size,
                cell_size,
            }
        }
        (CellSizing::FromFont { .. }, FontSizing::FitCellWidth) => {
            warn_once!(
                "bevy_terminal: CellSizing::FromFont requires FontSizing::Px; using the default font size"
            );
            resolve_metrics(
                &TerminalRenderConfig {
                    font_size: FontSizing::Px(UNMEASURED_FONT_SIZE),
                    ..config.clone()
                },
                measured_advance,
            )
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

/// Selects the physical resolution used by the renderer.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum TerminalRenderScale {
    /// Match the primary window's physical-to-logical scale factor when the
    /// terminal is presented through Bevy UI. Headless rendering uses `1.0`.
    #[default]
    Automatic,
    /// Rasterize at an explicit physical-to-logical scale factor.
    ///
    /// Values that are non-finite or less than or equal to zero fall back to
    /// `1.0`; valid values are clamped to `1.0..=8.0`. A custom UI or camera
    /// should use the same scale factor so the resulting texture maps
    /// one-to-one onto physical display pixels.
    Fixed(f32),
}

/// How the logical cell size is chosen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CellSizing {
    /// An explicit cell size in logical pixels. Combine with
    /// [`FontSizing::FitCellWidth`] to size the font to the cell width (the
    /// height then grows to the font's line box when it is shorter — read the
    /// effective size from [`TerminalTexture::cell_size`](crate::render::TerminalTexture::cell_size)),
    /// or with [`FontSizing::Px`] for full control of both.
    Logical(Vec2),
    /// Derive the cell from the font: width = the regular font's measured
    /// advance at the configured [`FontSizing::Px`] size, snapped to a whole
    /// physical pixel, and height = the font's line box (ascent + descent +
    /// leading from its metrics tables, as terminal emulators size rows)
    /// times `line_height`, grown to the full block glyph when that is taller
    /// and the multiplier is not below one. The final raster font size is derived back from the
    /// snapped width so glyph advance and cell width remain identical. This is
    /// how terminal emulators work ("font size in, cell size out"; zoom by
    /// changing the font size). Requires [`FontSizing::Px`].
    FromFont {
        /// Multiplier applied to the font's line box to get the cell height:
        /// `1.0` is the font's natural row (the terminal-emulator default),
        /// `0.9` packs rows tighter, `1.2` spaces them out, like WezTerm's
        /// `line_height` or Ghostty's `adjust-cell-height`. Below `1.0` the
        /// outermost ascender and descender pixels clip; block elements are
        /// drawn from geometry, so they still tile. The block glyph only
        /// grows the cell when the multiplier is at least `1.0`. Values that
        /// are not finite or not positive are treated as `1.0`.
        line_height: f32,
    },
}

impl CellSizing {
    /// Derive both cell dimensions from the selected font at its natural line
    /// height.
    pub const FROM_FONT: Self = Self::FromFont { line_height: 1.0 };

    /// The effective line-height multiplier of a font-driven cell (`1.0` for
    /// an explicit cell or an invalid value).
    #[must_use]
    pub fn line_height(self) -> f32 {
        match self {
            Self::FromFont { line_height } if line_height.is_finite() && line_height > 0.0 => {
                line_height
            }
            _ => 1.0,
        }
    }
}

impl Default for CellSizing {
    fn default() -> Self {
        Self::Logical(Vec2::new(11.0, 20.0))
    }
}

impl From<Vec2> for CellSizing {
    fn from(cell: Vec2) -> Self {
        Self::Logical(cell)
    }
}

/// Configuration for converting terminal cells into rendered geometry and text.
///
/// The cell size is either explicit ([`CellSizing::Logical`]) or derived from
/// the font ([`CellSizing::FromFont`]). Bevy can shape several fallback fonts
/// in one run, so there is no single font metric that is guaranteed to describe
/// every Unicode glyph; text runs are anchored to cell coordinates to prevent
/// cumulative drift either way.
///
/// This is a component: every [`Terminal`] entity requires one (a default is
/// inserted automatically). Mutating it rebuilds only that terminal.
#[derive(Clone, Component, Debug, PartialEq)]
pub struct TerminalRenderConfig {
    /// How the cell size is chosen.
    pub cell_size: CellSizing,
    /// Font faces for regular, bold, italic and bold-italic text.
    pub font: FontFaces,
    /// How the font size is chosen.
    pub font_size: FontSizing,
    /// Terminal color theme.
    pub theme: TerminalTheme,
    /// Cursor appearance.
    pub cursor: CursorConfig,
    /// Text blink rates.
    pub blink: BlinkConfig,
    /// Physical rasterization settings.
    pub raster: RasterConfig,
}

impl Default for TerminalRenderConfig {
    fn default() -> Self {
        Self {
            cell_size: CellSizing::default(),
            font: FontFaces::default(),
            font_size: FontSizing::FitCellWidth,
            theme: TerminalTheme::default(),
            cursor: CursorConfig::default(),
            blink: BlinkConfig::default(),
            raster: RasterConfig::default(),
        }
    }
}

/// Physical rasterization settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RasterConfig {
    /// Physical raster scale.
    pub scale: TerminalRenderScale,
    /// Glyph rasterization hinting.
    ///
    /// Defaults to [`FontHinting::Disabled`]: hinted rasterization snaps the
    /// font to whole-pixel sizes, so a font sized to fill the cell width
    /// exactly (see [`FontSizing::FitCellWidth`]) is rendered a fraction too
    /// narrow or wide and adjacent block/box glyphs show seams on displays
    /// whose scale factor makes the physical font size fractional. Unhinted
    /// rasterization keeps the measured metrics exact.
    pub hinting: FontHinting,
}

impl Default for RasterConfig {
    fn default() -> Self {
        Self {
            scale: TerminalRenderScale::Automatic,
            hinting: FontHinting::Disabled,
        }
    }
}

/// Public system set for ordering application systems around terminal syncing.
#[derive(Clone, Debug, Hash, Eq, PartialEq, SystemSet)]
pub enum TerminalSystems {
    /// Copies the latest surface state into the renderer and builds the frame's scene
    /// (newly spawned terminals are initialized just before this set).
    Sync,
}

fn text_font(faces: &FontFaces, font_size: f32, style: &ResolvedStyle) -> TextFont {
    let (face, weight, font_style) = faces.resolve(style.bold, style.italic);
    TextFont {
        font: face.clone(),
        font_size: font_size.into(),
        weight,
        style: font_style,
        ..default()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct PixelGeometry {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

/// Returns the number of columns rendered for the cell at `column`.
///
/// A wide anchor claims its declared span, clipped to the row and to the run of
/// explicit continuation cells that actually follow it, so a wide glyph can
/// never paint over a neighbor that has since been overwritten.
fn cell_span(cells: &[TerminalCell], column: usize) -> usize {
    let declared = usize::from(cells[column].columns()).min(cells.len() - column);
    let mut span = 1;
    while span < declared && cells[column + span].is_continuation() {
        span += 1;
    }
    span
}

fn cursor_should_be_visible(snapshot: &TerminalSnapshot) -> bool {
    let size = snapshot.size();
    let position = snapshot.cursor_position();
    snapshot.cursor_visible() && position.x < size.width && position.y < size.height
}

fn blink_hidden(elapsed: f32, frequency_hz: Option<f32>) -> bool {
    frequency_hz.is_some_and(|frequency_hz| {
        frequency_hz.is_finite()
            && frequency_hz > 0.0
            && (elapsed * frequency_hz * 2.0).floor() as u64 % 2 == 1
    })
}

#[derive(Clone, Debug, PartialEq)]
struct ResolvedStyle {
    foreground: Color,
    background: Color,
    underline: Color,
    bold: bool,
    italic: bool,
    underlined: bool,
    crossed_out: bool,
    slow_blink: bool,
    rapid_blink: bool,
    hidden: bool,
}

impl ResolvedStyle {
    /// White-on-black regular style used for measurement runs.
    pub(crate) fn plain() -> Self {
        Self {
            foreground: Color::WHITE,
            background: Color::BLACK,
            underline: Color::WHITE,
            bold: false,
            italic: false,
            underlined: false,
            crossed_out: false,
            slow_blink: false,
            rapid_blink: false,
            hidden: false,
        }
    }
}

impl ResolvedStyle {
    fn new(cell: &TerminalCell, theme: &TerminalTheme) -> Self {
        let mut foreground = theme.foreground(cell.style.foreground);
        let mut background = theme.background(cell.style.background);
        if cell.style.has(StyleFlags::REVERSED) {
            std::mem::swap(&mut foreground, &mut background);
        }
        let mut underline = theme.resolve(cell.style.underline, foreground);
        if cell.style.has(StyleFlags::DIM) {
            foreground = dim(foreground, background);
            underline = dim(underline, background);
        }
        if cell.style.has(StyleFlags::HIDDEN) {
            foreground = background;
            underline = background;
        }
        Self {
            foreground,
            background,
            underline,
            bold: cell.style.has(StyleFlags::BOLD),
            italic: cell.style.has(StyleFlags::ITALIC),
            underlined: cell.style.has(StyleFlags::UNDERLINED),
            crossed_out: cell.style.has(StyleFlags::CROSSED_OUT),
            slow_blink: cell.style.has(StyleFlags::SLOW_BLINK),
            rapid_blink: cell.style.has(StyleFlags::RAPID_BLINK),
            hidden: cell.style.has(StyleFlags::HIDDEN),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{TerminalColor, TerminalStyle};

    fn glyph_box(top: f32, bottom: f32) -> GlyphBox {
        GlyphBox { top, bottom }
    }

    #[test]
    fn snap_rounds_halves_consistently() {
        assert_eq!(snap(0.5), 1.0);
        assert_eq!(snap(-0.5), 0.0);
        assert_eq!(snap(4.5) - snap(-0.5), 5.0);
        assert_eq!(snap(2.49), 2.0);
        assert_eq!(snap(-2.51), -3.0);
    }

    #[test]
    fn from_font_line_height_multiplier() {
        assert_eq!(CellSizing::FROM_FONT.line_height(), 1.0);
        assert_eq!(CellSizing::FromFont { line_height: 0.9 }.line_height(), 0.9);
        assert_eq!(CellSizing::FromFont { line_height: 0.0 }.line_height(), 1.0);
        assert_eq!(
            CellSizing::FromFont {
                line_height: f32::NAN
            }
            .line_height(),
            1.0
        );
        assert_eq!(CellSizing::Logical(Vec2::ONE).line_height(), 1.0);
    }

    #[test]
    fn cell_height_grows_to_the_block_box_only() {
        // JetBrains Mono at 20 px: 24 px requested, 27 opaque block rows.
        assert_eq!(fitted_cell_height(24.0, Some(glyph_box(1.0, 28.0))), 27.0);
        // A block shorter than the request leaves the request alone.
        assert_eq!(fitted_cell_height(30.0, Some(glyph_box(1.0, 28.0))), 30.0);
        assert_eq!(fitted_cell_height(20.0, None), 20.0);
        // Fractional boxes round up so the block always covers.
        assert_eq!(fitted_cell_height(20.0, Some(glyph_box(0.0, 22.4))), 23.0);
    }

    #[test]
    fn vertical_offset_keeps_blocks_covering_then_centers_ink() {
        // Iosevka-like: 27-row cell, block opaque rows 1..28, core ink 3..27,
        // accents reaching above the block. Only a shift of -1 keeps the block
        // covering the cell, so that is the answer even though accents clip.
        let block = Some(glyph_box(1.0, 28.0));
        let core = Some(glyph_box(3.0, 27.0));
        let accents = Some(glyph_box(-4.0, 22.0));
        assert_eq!(vertical_offset(27.0, block, core, accents), -1.0);

        // A short cell in an explicit configuration: the block still covers and the
        // core ink is centered within the freedom the block leaves.
        let block = Some(glyph_box(-3.0, 22.0));
        let core = Some(glyph_box(0.0, 16.0));
        assert_eq!(vertical_offset(20.0, block, core, None), 2.0);

        // A cell taller than the block: the block no longer constrains; the core box
        // is centered and accents fit too.
        let block = Some(glyph_box(2.0, 12.0));
        let core = Some(glyph_box(4.0, 12.0));
        let accents = Some(glyph_box(1.0, 12.0));
        assert_eq!(vertical_offset(20.0, block, core, accents), 2.0);

        // Core ink taller than the cell: centered, half clipped on each side.
        assert_eq!(
            vertical_offset(10.0, None, Some(glyph_box(-2.0, 12.0)), None),
            0.0
        );
        // Nothing measured: no shift.
        assert_eq!(vertical_offset(20.0, None, None, None), 0.0);
    }

    #[test]
    fn vertical_offset_prefers_core_ink_over_accents() {
        // 20-row cell, core needs 0..20 exactly, accents want to sit 2 rows higher:
        // core wins and accents clip.
        let core = Some(glyph_box(2.0, 22.0));
        let accents = Some(glyph_box(-2.0, 18.0));
        assert_eq!(vertical_offset(20.0, None, core, accents), -2.0);
    }

    #[test]
    fn styles_resolve_reverse_hidden_dim_and_decorations() {
        let theme = TerminalTheme::default();
        let mut cell = TerminalCell::new("X").with_style(
            TerminalStyle::new()
                .fg(TerminalColor::RED)
                .bg(TerminalColor::BLUE)
                .with(
                    StyleFlags::REVERSED
                        | StyleFlags::DIM
                        | StyleFlags::UNDERLINED
                        | StyleFlags::CROSSED_OUT
                        | StyleFlags::BOLD
                        | StyleFlags::ITALIC,
                ),
        );
        let style = ResolvedStyle::new(&cell, &theme);
        assert_eq!(style.background, theme.ansi[1]);
        assert_ne!(style.foreground, theme.ansi[4]);
        assert!(style.bold && style.italic && style.underlined && style.crossed_out);

        cell.style.flags.insert(StyleFlags::HIDDEN);
        let hidden = ResolvedStyle::new(&cell, &theme);
        assert_eq!(hidden.foreground, hidden.background);
        assert!(hidden.hidden);

        let reversed = TerminalCell::new("X").with_style(
            TerminalStyle::new()
                .fg(TerminalColor::RED)
                .bg(TerminalColor::BLUE)
                .with(StyleFlags::REVERSED | StyleFlags::UNDERLINED),
        );
        let reversed = ResolvedStyle::new(&reversed, &theme);
        assert_eq!(reversed.foreground, theme.ansi[4]);
        assert_eq!(reversed.underline, reversed.foreground);
    }

    #[test]
    fn font_faces_fall_back_in_order() {
        let regular = FontSource::from("regular");
        let bold = FontSource::from("bold");
        let italic = FontSource::from("italic");
        let bold_italic = FontSource::from("bold italic");

        let only_regular = FontFaces::regular(regular.clone());
        assert_eq!(only_regular.select(true, true), &regular);
        assert_eq!(FontFaces::from(regular.clone()), only_regular);

        let with_bold = FontFaces {
            bold: Some(bold.clone()),
            ..only_regular.clone()
        };
        assert_eq!(with_bold.select(true, false), &bold);
        assert_eq!(with_bold.select(true, true), &bold);
        assert_eq!(with_bold.select(false, true), &regular);

        let with_italic = FontFaces {
            italic: Some(italic.clone()),
            ..only_regular.clone()
        };
        assert_eq!(with_italic.select(true, true), &italic);

        let complete = FontFaces {
            regular: regular.clone(),
            bold: Some(bold.clone()),
            italic: Some(italic.clone()),
            bold_italic: Some(bold_italic.clone()),
            synthesize: true,
        };
        assert_eq!(complete.select(false, false), &regular);
        assert_eq!(complete.select(true, false), &bold);
        assert_eq!(complete.select(false, true), &italic);
        assert_eq!(complete.select(true, true), &bold_italic);

        let theme = TerminalTheme::default();
        let cell = TerminalCell::new("X")
            .with_style(TerminalStyle::new().with(StyleFlags::BOLD | StyleFlags::ITALIC));
        let font = text_font(&complete, 18.0, &ResolvedStyle::new(&cell, &theme));
        assert_eq!(font.font, bold_italic);
        assert_eq!(font.weight, FontWeight::BOLD);
        assert_eq!(font.style, FontStyle::Italic);

        // Synthesis: a missing italic face is requested as italic from the
        // regular face by default, and left upright with synthesis disabled.
        let bold_only = FontFaces {
            bold: Some(bold.clone()),
            ..FontFaces::regular(regular.clone())
        };
        let italic_cell =
            TerminalCell::new("X").with_style(TerminalStyle::new().with(StyleFlags::ITALIC));
        let synthesized = text_font(&bold_only, 18.0, &ResolvedStyle::new(&italic_cell, &theme));
        assert_eq!(synthesized.font, regular);
        assert_eq!(synthesized.style, FontStyle::Italic);
        let plain = text_font(
            &bold_only.clone().with_synthesis(false),
            18.0,
            &ResolvedStyle::new(&italic_cell, &theme),
        );
        assert_eq!(plain.font, regular);
        assert_eq!(plain.style, FontStyle::Normal);
        // An explicit face is always requested with its own weight/style.
        let bold_cell =
            TerminalCell::new("X").with_style(TerminalStyle::new().with(StyleFlags::BOLD));
        let exact = text_font(
            &bold_only.with_synthesis(false),
            18.0,
            &ResolvedStyle::new(&bold_cell, &theme),
        );
        assert_eq!(exact.font, bold);
        assert_eq!(exact.weight, FontWeight::BOLD);
    }

    #[test]
    fn font_size_selection_uses_measured_advance_or_explicit_pixels() {
        let config = TerminalRenderConfig {
            cell_size: Vec2::new(11.0, 20.0).into(),
            ..default()
        };
        // A font whose advance is 0.6 em measures 38.4 px at the 64 px probe.
        let fitted = resolve_metrics(&config, Some(38.4));
        assert!((fitted.font_size - 11.0 / 0.6).abs() < 1e-3);
        assert_eq!(fitted.cell_size, Vec2::new(11.0, 20.0));
        assert_eq!(
            resolve_metrics(&config, None).font_size,
            UNMEASURED_FONT_SIZE
        );
        let explicit = TerminalRenderConfig {
            font_size: FontSizing::Px(18.0),
            ..config.clone()
        };
        assert_eq!(resolve_metrics(&explicit, Some(38.4)).font_size, 18.0);
        assert_eq!(resolve_metrics(&explicit, None).font_size, 18.0);

        // Before refinement, font-driven cells have a measured width and a
        // placeholder height; refinement replaces it with the measured line box.
        let from_font = TerminalRenderConfig {
            cell_size: CellSizing::FROM_FONT,
            font_size: FontSizing::Px(20.0),
            ..config
        };
        let metrics = resolve_metrics(&from_font, Some(38.4));
        assert_eq!(metrics.font_size, 20.0);
        assert!((metrics.cell_size.x - 12.0).abs() < 1e-4);
        assert_eq!(metrics.cell_size.y, 1.0);
        assert_eq!(resolve_metrics(&from_font, None).cell_size, Vec2::ONE);
        // Zooming changes the cell.
        let zoomed = TerminalRenderConfig {
            font_size: FontSizing::Px(30.0),
            ..from_font.clone()
        };
        assert!((resolve_metrics(&zoomed, Some(38.4)).cell_size.x - 18.0).abs() < 1e-4);
        // FromFont with FitCellWidth is a configuration error: retain
        // font-driven sizing and substitute the renderer's default font size.
        let invalid = TerminalRenderConfig {
            font_size: FontSizing::FitCellWidth,
            ..from_font
        };
        let fallback = resolve_metrics(&invalid, Some(38.4));
        assert_eq!(fallback.font_size, UNMEASURED_FONT_SIZE);
        assert!((fallback.cell_size.x - 9.6).abs() < 1e-4);
        assert_eq!(fallback.cell_size.y, 1.0);
    }

    #[test]
    fn wide_cells_span_only_their_continuations() {
        let wide = TerminalCell::wide("界", 2);
        let cells = [
            wide.clone(),
            TerminalCell::continuation_of(&wide),
            TerminalCell::new("A"),
        ];
        assert_eq!(cell_span(&cells, 0), 2);
        assert_eq!(cell_span(&cells, 2), 1);
        let overwritten = [wide.clone(), TerminalCell::new("B")];
        assert_eq!(cell_span(&overwritten, 0), 1);
        let clipped = [wide];
        assert_eq!(cell_span(&clipped, 0), 1);
    }

    #[test]
    fn blink_phase_alternates_at_twice_the_frequency() {
        assert!(!blink_hidden(0.1, Some(1.0)));
        assert!(blink_hidden(0.6, Some(1.0)));
        assert!(!blink_hidden(0.6, None));
        assert!(!blink_hidden(0.6, Some(0.0)));
    }
}
