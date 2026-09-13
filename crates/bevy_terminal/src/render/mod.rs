//! Rendering: the [`TerminalPlugin`], the [`TerminalRenderer`] component and its
//! configuration, the renderer-owned [`TerminalTexture`], per-terminal
//! [`TerminalStats`], and the [`TerminalTheme`]. Add the plugin once and spawn
//! a `TerminalRenderer` per rendered surface; applications present its image.

use bevy::{
    ecs::schedule::SystemSet,
    prelude::*,
    text::{FontHinting, FontSource, FontStyle, FontWeight},
};

use crate::scene::{StyleFlags, TerminalCell, TerminalSnapshot};

mod batch;
mod color;
mod fonts;
mod terminal;

pub use batch::TerminalPlugin;
pub use color::TerminalTheme;
use color::dim;
pub use fonts::font_family;
pub use terminal::{
    TerminalGeometry, TerminalRenderer, TerminalStats, TerminalStatus, TerminalTexture, grid_for,
};

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
        self.resolve(bold, italic).0
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

/// How requested logical sizes determine the terminal's measured geometry.
///
/// The renderer snaps cells to physical pixels. Font-driven and width-fitted
/// modes then refit the font advance to that width, preventing seams. Glyph
/// fitting preserves the font's line box and clips fallback overhang to cells.
/// Read effective dimensions from [`TerminalTexture::measured`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TerminalSizing {
    /// Derive cells from a logical font size and line-height multiplier.
    /// Width follows the font advance; height follows ascent, descent and
    /// leading. Multipliers below one intentionally clip outer ink. Block
    /// elements still tile because they are rendered as geometry.
    FromFont {
        /// Requested logical font size.
        font_size: f32,
        /// Positive multiplier of the font's natural line box.
        line_height: f32,
    },
    /// Fit the font to this logical cell width. Height is a minimum and grows
    /// to contain the font's line box.
    FitCellWidth(Vec2),
    /// Explicit logical cell and font sizes. Cells snap to physical pixels;
    /// glyphs exceeding them are fitted and clipped without growing the cell.
    Fixed {
        /// Logical dimensions of one cell.
        cell_size: Vec2,
        /// Requested logical font size.
        font_size: f32,
    },
}

impl TerminalSizing {
    /// Font-driven sizing with natural line height.
    #[must_use]
    pub const fn font(font_size: f32) -> Self {
        Self::FromFont {
            font_size,
            line_height: 1.0,
        }
    }

    /// Checks that every requested dimension and multiplier is finite and positive.
    pub fn is_valid(self) -> bool {
        let positive = |value: f32| value.is_finite() && value > 0.0;
        match self {
            Self::FromFont {
                font_size,
                line_height,
            } => positive(font_size) && positive(line_height),
            Self::FitCellWidth(cell) => positive(cell.x) && positive(cell.y),
            Self::Fixed {
                cell_size,
                font_size,
            } => positive(cell_size.x) && positive(cell_size.y) && positive(font_size),
        }
    }

    fn needs_advance(self) -> bool {
        !matches!(self, Self::Fixed { .. })
    }

    fn line_height(self) -> f32 {
        match self {
            Self::FromFont { line_height, .. } => line_height,
            _ => 1.0,
        }
    }
}

impl Default for TerminalSizing {
    fn default() -> Self {
        Self::FitCellWidth(Vec2::new(11.0, 20.0))
    }
}

/// Configuration for converting terminal cells into rendered geometry and text.
///
/// [`TerminalSizing`] selects font-driven or explicit cell geometry. Bevy can shape several fallback fonts
/// in one run, so there is no single font metric that is guaranteed to describe
/// every Unicode glyph; text runs are anchored to cell coordinates to prevent
/// cumulative drift either way.
///
/// This is a component: every [`TerminalRenderer`] entity requires one (a default is
/// inserted automatically). Mutating it rebuilds only that terminal.
#[derive(Clone, Component, Debug, Default, PartialEq)]
pub struct TerminalRenderConfig {
    /// Requested sizing policy; effective geometry is exposed by the texture.
    pub sizing: TerminalSizing,
    /// Font faces for regular, bold, italic and bold-italic text.
    pub font: FontFaces,
    /// Terminal color theme.
    pub theme: TerminalTheme,
    /// Cursor appearance.
    pub cursor: CursorConfig,
    /// Text blink rates.
    pub blink: BlinkConfig,
    /// Physical rasterization settings.
    pub raster: RasterConfig,
}

/// Physical rasterization settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RasterConfig {
    /// Explicit physical-to-logical pixel ratio, independent of presentation.
    ///
    /// Defaults to `1.0`. Non-finite and non-positive values fall back to `1.0`;
    /// valid values are clamped to `1.0..=8.0`. Applications choose the scale
    /// for their window, camera, UI, or offscreen target.
    pub scale: f32,
    /// Glyph rasterization hinting.
    ///
    /// Defaults to [`FontHinting::Disabled`]: hinted rasterization snaps the
    /// font to whole-pixel sizes, so a font sized to fill the cell width
    /// exactly (see [`TerminalSizing::FitCellWidth`]) is rendered a fraction too
    /// narrow or wide and adjacent block/box glyphs show seams on displays
    /// whose scale factor makes the physical font size fractional. Unhinted
    /// rasterization keeps the measured metrics exact.
    pub hinting: FontHinting,
}

impl Default for RasterConfig {
    fn default() -> Self {
        Self {
            scale: 1.0,
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
    use super::batch::metrics::{
        GlyphBox, fitted_cell_height, resolve_metrics, snap, vertical_offset,
    };
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
    fn sizing_rejects_invalid_numeric_inputs() {
        assert!(TerminalSizing::font(16.0).is_valid());
        for value in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert!(!TerminalSizing::font(value).is_valid());
            assert!(
                !TerminalSizing::FromFont {
                    font_size: 16.0,
                    line_height: value
                }
                .is_valid()
            );
            assert!(!TerminalSizing::FitCellWidth(Vec2::new(value, 20.0)).is_valid());
            assert!(
                !TerminalSizing::Fixed {
                    cell_size: Vec2::ONE,
                    font_size: value
                }
                .is_valid()
            );
        }
        assert_eq!(
            TerminalSizing::FromFont {
                font_size: 16.0,
                line_height: 0.9
            }
            .line_height(),
            0.9
        );
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
            sizing: TerminalSizing::FitCellWidth(Vec2::new(11.0, 20.0)),
            ..default()
        };
        // A font whose advance is 0.6 em measures 38.4 px at the 64 px probe.
        let fitted = resolve_metrics(&config, Some(38.4));
        assert!((fitted.font_size - 11.0 / 0.6).abs() < 1e-3);
        assert_eq!(fitted.cell_size, Vec2::new(11.0, 20.0));
        assert_eq!(resolve_metrics(&config, None).font_size, 16.0);
        let explicit = TerminalRenderConfig {
            sizing: TerminalSizing::Fixed {
                cell_size: Vec2::new(11.0, 20.0),
                font_size: 18.0,
            },
            ..config.clone()
        };
        assert_eq!(resolve_metrics(&explicit, Some(38.4)).font_size, 18.0);
        assert_eq!(resolve_metrics(&explicit, None).font_size, 18.0);

        // Before refinement, font-driven cells have a measured width and a
        // placeholder height; refinement replaces it with the measured line box.
        let from_font = TerminalRenderConfig {
            sizing: TerminalSizing::FromFont {
                font_size: 20.0,
                line_height: 1.0,
            },
            ..config
        };
        let metrics = resolve_metrics(&from_font, Some(38.4));
        assert_eq!(metrics.font_size, 20.0);
        assert!((metrics.cell_size.x - 12.0).abs() < 1e-4);
        assert_eq!(metrics.cell_size.y, 1.0);
        assert_eq!(resolve_metrics(&from_font, None).cell_size, Vec2::ONE);
        // Zooming changes the cell.
        let zoomed = TerminalRenderConfig {
            sizing: TerminalSizing::FromFont {
                font_size: 30.0,
                line_height: 1.0,
            },
            ..from_font.clone()
        };
        assert!((resolve_metrics(&zoomed, Some(38.4)).cell_size.x - 18.0).abs() < 1e-4);
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
