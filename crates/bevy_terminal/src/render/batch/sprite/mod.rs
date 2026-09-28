//! Procedurally drawn grid graphics, ported from Ghostty's sprite font
//! (`src/font/sprite/`) at `b40acce58dcf77df52231c3798ea58e924647c89`:
//! box drawing, block elements and shades, Braille, four geometric
//! triangles, the Powerline and branch-drawing subsets, and Symbols for
//! Legacy Computing (with its supplement).
//!
//! Ghostty is MIT licensed: Copyright (c) 2024 Mitchell Hashimoto, Ghostty
//! contributors.
//!
//! Glyphs are drawn at the exact cell size from the grid metrics, so they
//! tile without seams whatever the font's own outlines look like.

mod box_drawing;
mod canvas;
mod legacy;
mod shapes;

pub(crate) use canvas::Sprite;
use canvas::{Canvas, Shade};
pub(crate) use shapes::block_rects;

/// The grid metrics sprites are drawn with, in physical pixels.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct Metrics {
    pub(crate) cell_width: u32,
    pub(crate) cell_height: u32,
    /// Ghostty's `box_thickness`: the font's underline thickness, rounded up.
    pub(crate) box_thickness: u32,
}

/// Stroke weights of box-drawing lines.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Thickness {
    Light,
    Heavy,
}

impl Thickness {
    fn pixels(self, base: u32) -> u32 {
        match self {
            Thickness::Light => base,
            Thickness::Heavy => base * 2,
        }
    }
}

/// A fraction of the cell, with Ghostty's rounding: a minimum edge rounds the
/// complementary distance from the far edge, so adjoining fills meet exactly.
#[derive(Clone, Copy, Debug)]
struct Fraction(f64);

impl Fraction {
    const ZERO: Self = Self(0.0);
    const QUARTER: Self = Self(0.25);
    const THIRD: Self = Self(1.0 / 3.0);
    const HALF: Self = Self(0.5);
    const TWO_THIRDS: Self = Self(2.0 / 3.0);
    const THREE_QUARTERS: Self = Self(0.75);
    const ONE: Self = Self(1.0);

    fn eighths(n: u32) -> Self {
        Self(f64::from(n) / 8.0)
    }

    fn quarters(n: u32) -> Self {
        Self(f64::from(n) / 4.0)
    }

    fn min(self, size: u32) -> i32 {
        let size = f64::from(size);
        (size - ((1.0 - self.0) * size).round()) as i32
    }

    fn max(self, size: u32) -> i32 {
        (self.0 * f64::from(size)).round() as i32
    }

    fn float(self, size: u32) -> f64 {
        self.0 * f64::from(size)
    }
}

/// Fills the part of the cell between two horizontal and two vertical fractions.
fn fill(
    metrics: Metrics,
    canvas: &mut Canvas,
    x0: Fraction,
    x1: Fraction,
    y0: Fraction,
    y1: Fraction,
) {
    canvas.box_(
        x0.min(metrics.cell_width),
        y0.min(metrics.cell_height),
        x1.max(metrics.cell_width),
        y1.max(metrics.cell_height),
        Shade::On,
    );
}

/// Where a figure sits in the cell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Horizontal {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Vertical {
    Top,
    Middle,
    Bottom,
}

/// Corners of a cell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Edges of a cell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Edge {
    Top,
    Left,
    Bottom,
    Right,
}

/// Whether Ghostty draws `codepoint` as a sprite.
pub(crate) fn is_sprite(codepoint: u32) -> bool {
    matches!(
        codepoint,
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

/// The single codepoint of `text` when Ghostty draws it as a sprite.
pub(crate) fn sprite_codepoint(text: &str) -> Option<u32> {
    let mut chars = text.chars();
    let codepoint = u32::from(chars.next()?);
    (chars.next().is_none() && is_sprite(codepoint)).then_some(codepoint)
}

/// Draws `codepoint` in a `width` × `height` pixel box (a cell, or a wide
/// cell's span); `None` when it is not a sprite or draws nothing.
pub(crate) fn draw(codepoint: u32, width: u32, height: u32, metrics: Metrics) -> Option<Sprite> {
    if !is_sprite(codepoint) || width == 0 || height == 0 {
        return None;
    }
    let mut canvas = Canvas::new(width, height);
    match codepoint {
        0x2500..=0x257f => box_drawing::draw(codepoint, &mut canvas, metrics),
        0x2580..=0x259f => shapes::block_element(codepoint, &mut canvas, metrics),
        0x25e2..=0x25ff => shapes::geometric(codepoint, &mut canvas, metrics),
        0x2800..=0x28ff => shapes::braille(codepoint, &mut canvas, width, height),
        0xe0b0..=0xe0d4 => shapes::powerline(codepoint, &mut canvas, width, height, metrics),
        0xf5d0..=0xf60d => shapes::branch(codepoint, &mut canvas, metrics),
        _ => legacy::draw(codepoint, &mut canvas, width, height, metrics),
    }
    canvas.finish()
}

#[cfg(test)]
mod tests;
