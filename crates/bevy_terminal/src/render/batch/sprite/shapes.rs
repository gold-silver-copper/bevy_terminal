//! Block elements, geometric triangles, Braille, Powerline and branch
//! drawing (Ghostty's `sprite/draw/{block,geometric_shapes,braille,
//! powerline,branch}.zig`).

use super::box_drawing::{arc, diagonal_falling, diagonal_rising};
use super::canvas::{Canvas, Path, Shade};
use super::{Corner, Edge, Fraction, Horizontal, Metrics, Thickness, Vertical, fill};
use bevy::math::DVec2;

/// A block of `width` × `height` cell fractions aligned in the cell.
pub(super) fn block(
    metrics: Metrics,
    canvas: &mut Canvas,
    (horizontal, vertical): (Horizontal, Vertical),
    width: f64,
    height: f64,
    shade: Shade,
) {
    let w = (f64::from(metrics.cell_width) * width).round() as u32;
    let h = (f64::from(metrics.cell_height) * height).round() as u32;
    let x = match horizontal {
        Horizontal::Left => 0,
        Horizontal::Right => metrics.cell_width - w,
        Horizontal::Center => (metrics.cell_width - w) / 2,
    };
    let y = match vertical {
        Vertical::Top => 0,
        Vertical::Bottom => metrics.cell_height - h,
        Vertical::Middle => (metrics.cell_height - h) / 2,
    };
    canvas.box_(x as i32, y as i32, (x + w) as i32, (y + h) as i32, shade);
}

pub(super) const UPPER: (Horizontal, Vertical) = (Horizontal::Center, Vertical::Top);
pub(super) const LOWER: (Horizontal, Vertical) = (Horizontal::Center, Vertical::Bottom);
pub(super) const LEFT: (Horizontal, Vertical) = (Horizontal::Left, Vertical::Middle);
pub(super) const RIGHT: (Horizontal, Vertical) = (Horizontal::Right, Vertical::Middle);

/// The whole cell in `shade`.
pub(super) fn full(metrics: Metrics, canvas: &mut Canvas, shade: Shade) {
    canvas.box_(
        0,
        0,
        metrics.cell_width as i32,
        metrics.cell_height as i32,
        shade,
    );
}

/// Block Elements, U+2580–259F: eighths, halves, quadrants and shades.
pub(super) fn block_element(codepoint: u32, canvas: &mut Canvas, metrics: Metrics) {
    let on = Shade::On;
    let eighth = |n: u32| f64::from(n) / 8.0;
    match codepoint {
        0x2580 => block(metrics, canvas, UPPER, 1.0, 0.5, on),
        0x2581..=0x2587 => block(metrics, canvas, LOWER, 1.0, eighth(codepoint - 0x2580), on),
        0x2588 => full(metrics, canvas, on),
        0x2589..=0x258f => block(metrics, canvas, LEFT, eighth(0x2590 - codepoint), 1.0, on),
        0x2590 => block(metrics, canvas, RIGHT, 0.5, 1.0, on),
        0x2591 => full(metrics, canvas, Shade::Light),
        0x2592 => full(metrics, canvas, Shade::Medium),
        0x2593 => full(metrics, canvas, Shade::Dark),
        0x2594 => block(metrics, canvas, UPPER, 1.0, eighth(1), on),
        0x2595 => block(metrics, canvas, RIGHT, eighth(1), 1.0, on),
        0x2596..=0x259f => {
            // Top-left, top-right, bottom-left and bottom-right quadrants.
            let [tl, tr, bl, br] = match codepoint {
                0x2596 => [false, false, true, false],
                0x2597 => [false, false, false, true],
                0x2598 => [true, false, false, false],
                0x2599 => [true, false, true, true],
                0x259a => [true, false, false, true],
                0x259b => [true, true, true, false],
                0x259c => [true, true, false, true],
                0x259d => [false, true, false, false],
                0x259e => [false, true, true, false],
                _ => [false, true, true, true],
            };
            let (z, h, f) = (Fraction::ZERO, Fraction::HALF, Fraction::ONE);
            if tl {
                fill(metrics, canvas, z, h, z, h);
            }
            if tr {
                fill(metrics, canvas, h, f, z, h);
            }
            if bl {
                fill(metrics, canvas, z, h, h, f);
            }
            if br {
                fill(metrics, canvas, h, f, h, f);
            }
        }
        _ => unreachable!("block element {codepoint:x}"),
    }
}

/// The corners of the right triangle filling `corner` of the cell.
fn corner_triangle(metrics: Metrics, corner: Corner) -> [DVec2; 3] {
    let (w, h) = (
        f64::from(metrics.cell_width),
        f64::from(metrics.cell_height),
    );
    let point = DVec2::new;
    match corner {
        Corner::TopLeft => [point(0.0, 0.0), point(0.0, h), point(w, 0.0)],
        Corner::TopRight => [point(0.0, 0.0), point(w, h), point(w, 0.0)],
        Corner::BottomLeft => [point(0.0, 0.0), point(0.0, h), point(w, h)],
        Corner::BottomRight => [point(0.0, h), point(w, h), point(w, 0.0)],
    }
}

pub(super) fn corner_triangle_shade(
    metrics: Metrics,
    canvas: &mut Canvas,
    corner: Corner,
    shade: Shade,
) {
    canvas.triangle(corner_triangle(metrics, corner), shade);
}

fn corner_triangle_outline(metrics: Metrics, canvas: &mut Canvas, corner: Corner) {
    let [a, b, c] = corner_triangle(metrics, corner);
    let mut path = Path::default();
    path.move_to(a.x, a.y)
        .line_to(b.x, b.y)
        .line_to(c.x, c.y)
        .close();
    let thick = f64::from(Thickness::Light.pixels(metrics.box_thickness));
    canvas.inner_stroke_path(&path, thick, Shade::On);
}

/// `◢◣◤◥` and `◸◹◺◿`.
pub(super) fn geometric(codepoint: u32, canvas: &mut Canvas, metrics: Metrics) {
    match codepoint {
        0x25e2 => corner_triangle_shade(metrics, canvas, Corner::BottomRight, Shade::On),
        0x25e3 => corner_triangle_shade(metrics, canvas, Corner::BottomLeft, Shade::On),
        0x25e4 => corner_triangle_shade(metrics, canvas, Corner::TopLeft, Shade::On),
        0x25e5 => corner_triangle_shade(metrics, canvas, Corner::TopRight, Shade::On),
        0x25f8 => corner_triangle_outline(metrics, canvas, Corner::TopLeft),
        0x25f9 => corner_triangle_outline(metrics, canvas, Corner::TopRight),
        0x25fa => corner_triangle_outline(metrics, canvas, Corner::BottomLeft),
        0x25ff => corner_triangle_outline(metrics, canvas, Corner::BottomRight),
        _ => unreachable!("geometric sprite {codepoint:x}"),
    }
}

/// Braille, U+2800–28FF: square dots on a 2 × 4 grid, spacing and margins
/// distributed so every cell size keeps the pattern even.
pub(super) fn braille(codepoint: u32, canvas: &mut Canvas, width: u32, height: u32) {
    let mut w = (width / 4).min(height / 8) as i32;
    let mut x_spacing = (width / 4) as i32;
    let mut y_spacing = (height / 8) as i32;
    let mut x_margin = x_spacing.div_euclid(2);
    let mut y_margin = y_spacing.div_euclid(2);
    let mut x_left = width as i32 - 2 * x_margin - x_spacing - 2 * w;
    let mut y_left = height as i32 - 2 * y_margin - 3 * y_spacing - 4 * w;
    // First, a non-zero dot.
    if x_left >= 2 && y_left >= 4 && w == 0 {
        w += 1;
        x_left -= 2;
        y_left -= 4;
    }
    // Then non-zero margins.
    if x_left >= 2 && x_margin == 0 {
        x_margin = 1;
        x_left -= 2;
    }
    if y_left >= 2 && y_margin == 0 {
        y_margin = 1;
        y_left -= 2;
    }
    // Then wider spacing.
    if x_left >= 1 {
        x_spacing += 1;
        x_left -= 1;
    }
    if y_left >= 3 {
        y_spacing += 1;
        y_left -= 3;
    }
    // Then wider margins.
    if x_left >= 2 {
        x_margin += 1;
        x_left -= 2;
    }
    if y_left >= 2 {
        y_margin += 1;
        y_left -= 2;
    }
    // Last, larger dots.
    if x_left >= 2 && y_left >= 4 {
        w += 1;
    }
    let x = [x_margin, x_margin + w + x_spacing];
    let y: [i32; 4] = std::array::from_fn(|i| y_margin + i as i32 * (w + y_spacing));
    // Bits: dots 1–3 left, 4–6 right, 7 left bottom, 8 right bottom.
    let dots = [
        (0, 0),
        (0, 1),
        (0, 2),
        (1, 0),
        (1, 1),
        (1, 2),
        (0, 3),
        (1, 3),
    ];
    for (bit, (column, row)) in dots.into_iter().enumerate() {
        if codepoint & (1 << bit) != 0 {
            canvas.box_(x[column], y[row], x[column] + w, y[row] + w, Shade::On);
        }
    }
}

/// The geometric Powerline glyphs, U+E0B0–E0BF, E0D2 and E0D4.
pub(super) fn powerline(
    codepoint: u32,
    canvas: &mut Canvas,
    width: u32,
    height: u32,
    metrics: Metrics,
) {
    let (w, h) = (f64::from(width), f64::from(height));
    let point = DVec2::new;
    let thin = f64::from(Thickness::Light.pixels(metrics.box_thickness));
    // Coefficient approximating a circular arc with a cubic curve.
    let c = (std::f64::consts::SQRT_2 - 1.0) * 4.0 / 3.0;
    let radius = w.min(h / 2.0);
    match codepoint {
        0xe0b0 => canvas.triangle(
            [point(0.0, 0.0), point(w, h / 2.0), point(0.0, h)],
            Shade::On,
        ),
        0xe0b2 => canvas.triangle([point(w, 0.0), point(0.0, h / 2.0), point(w, h)], Shade::On),
        0xe0b8 => canvas.triangle([point(0.0, 0.0), point(w, h), point(0.0, h)], Shade::On),
        0xe0ba => canvas.triangle([point(w, 0.0), point(w, h), point(0.0, h)], Shade::On),
        0xe0bc => canvas.triangle([point(0.0, 0.0), point(w, 0.0), point(0.0, h)], Shade::On),
        0xe0be => canvas.triangle([point(0.0, 0.0), point(w, 0.0), point(w, h)], Shade::On),
        0xe0b9 | 0xe0bf => diagonal_falling(metrics, canvas),
        0xe0bb | 0xe0bd => diagonal_rising(metrics, canvas),
        0xe0b1 | 0xe0b3 => {
            let mut path = Path::default();
            path.move_to(0.0, 0.0).line_to(w, h / 2.0).line_to(0.0, h);
            canvas.stroke_path(&path, thin, Shade::On);
            if codepoint == 0xe0b3 {
                canvas.flip_horizontal();
            }
        }
        0xe0b4 | 0xe0b6 => {
            let mut path = Path::default();
            path.move_to(0.0, 0.0)
                .curve_to(radius * c, 0.0, radius, radius - radius * c, radius, radius)
                .line_to(radius, h - radius)
                .curve_to(radius, h - radius + radius * c, radius * c, h, 0.0, h)
                .close();
            canvas.fill_path(&path, Shade::On);
            if codepoint == 0xe0b6 {
                canvas.flip_horizontal();
            }
        }
        0xe0b5 | 0xe0b7 => {
            // Straight ends keep the offset stroke's caps perpendicular.
            let mut path = Path::default();
            path.move_to(0.0, 0.0)
                .line_to(1.0, 0.0)
                .curve_to(radius * c, 0.0, radius, radius - radius * c, radius, radius)
                .line_to(radius, h - radius)
                .curve_to(radius, h - radius + radius * c, radius * c, h, 1.0, h)
                .line_to(0.0, h);
            canvas.inner_stroke_path(&path, f64::from(metrics.box_thickness), Shade::On);
            if codepoint == 0xe0b7 {
                canvas.flip_horizontal();
            }
        }
        0xe0d2 | 0xe0d4 => {
            let thick = f64::from(metrics.box_thickness);
            let mut top = Path::default();
            top.move_to(0.0, 0.0)
                .line_to(w, 0.0)
                .line_to(w / 2.0, h / 2.0 - thick / 2.0)
                .line_to(0.0, h / 2.0 - thick / 2.0)
                .close();
            canvas.fill_path(&top, Shade::On);
            let mut bottom = Path::default();
            bottom
                .move_to(0.0, h)
                .line_to(w, h)
                .line_to(w / 2.0, h / 2.0 + thick / 2.0)
                .line_to(0.0, h / 2.0 + thick / 2.0)
                .close();
            canvas.fill_path(&bottom, Shade::On);
            if codepoint == 0xe0d4 {
                canvas.flip_horizontal();
            }
        }
        _ => unreachable!("powerline sprite {codepoint:x}"),
    }
}

/// A centered light line along the cell.
fn line_middle(metrics: Metrics, canvas: &mut Canvas, horizontal: bool) {
    let thick = Thickness::Light.pixels(metrics.box_thickness);
    if horizontal {
        let y = metrics.cell_height.saturating_sub(thick) / 2;
        canvas.box_(
            0,
            y as i32,
            metrics.cell_width as i32,
            (y + thick) as i32,
            Shade::On,
        );
    } else {
        let x = metrics.cell_width.saturating_sub(thick) / 2;
        canvas.box_(
            x as i32,
            0,
            (x + thick) as i32,
            metrics.cell_height as i32,
            Shade::On,
        );
    }
}

/// Branch drawing, U+F5D0–F60D (git graph lines, arcs and nodes).
pub(super) fn branch(codepoint: u32, canvas: &mut Canvas, metrics: Metrics) {
    use Corner::{BottomLeft as BL, BottomRight as BR, TopLeft as TL, TopRight as TR};
    let horizontal = |canvas: &mut Canvas| line_middle(metrics, canvas, true);
    let vertical = |canvas: &mut Canvas| line_middle(metrics, canvas, false);
    let arcs = |canvas: &mut Canvas, corners: &[Corner]| {
        for corner in corners {
            arc(metrics, canvas, *corner);
        }
    };
    match codepoint {
        0xf5d0 => horizontal(canvas),
        0xf5d1 => vertical(canvas),
        0xf5d2 => fading_line(metrics, canvas, Edge::Right),
        0xf5d3 => fading_line(metrics, canvas, Edge::Left),
        0xf5d4 => fading_line(metrics, canvas, Edge::Bottom),
        0xf5d5 => fading_line(metrics, canvas, Edge::Top),
        0xf5d6 => arcs(canvas, &[BR]),
        0xf5d7 => arcs(canvas, &[BL]),
        0xf5d8 => arcs(canvas, &[TR]),
        0xf5d9 => arcs(canvas, &[TL]),
        0xf5da => {
            vertical(canvas);
            arcs(canvas, &[TR]);
        }
        0xf5db => {
            vertical(canvas);
            arcs(canvas, &[BR]);
        }
        0xf5dc => arcs(canvas, &[TR, BR]),
        0xf5dd => {
            vertical(canvas);
            arcs(canvas, &[TL]);
        }
        0xf5de => {
            vertical(canvas);
            arcs(canvas, &[BL]);
        }
        0xf5df => arcs(canvas, &[TL, BL]),
        0xf5e0 => {
            arcs(canvas, &[BL]);
            horizontal(canvas);
        }
        0xf5e1 => {
            arcs(canvas, &[BR]);
            horizontal(canvas);
        }
        0xf5e2 => arcs(canvas, &[BR, BL]),
        0xf5e3 => {
            arcs(canvas, &[TL]);
            horizontal(canvas);
        }
        0xf5e4 => {
            arcs(canvas, &[TR]);
            horizontal(canvas);
        }
        0xf5e5 => arcs(canvas, &[TR, TL]),
        0xf5e6 => {
            vertical(canvas);
            arcs(canvas, &[TL, TR]);
        }
        0xf5e7 => {
            vertical(canvas);
            arcs(canvas, &[BL, BR]);
        }
        0xf5e8 => {
            horizontal(canvas);
            arcs(canvas, &[BL, TL]);
        }
        0xf5e9 => {
            horizontal(canvas);
            arcs(canvas, &[TR, BR]);
        }
        0xf5ea => {
            vertical(canvas);
            arcs(canvas, &[TL, BR]);
        }
        0xf5eb => {
            vertical(canvas);
            arcs(canvas, &[TR, BL]);
        }
        0xf5ec => {
            horizontal(canvas);
            arcs(canvas, &[TL, BR]);
        }
        0xf5ed => {
            horizontal(canvas);
            arcs(canvas, &[TR, BL]);
        }
        0xf5ee..=0xf60d => {
            // Nodes: filled on even codepoints; lines to the edges below.
            let index = codepoint - 0xf5ee;
            let filled = index.is_multiple_of(2);
            // [up, right, down, left] for each pair of codepoints.
            let edges: [[bool; 4]; 16] = [
                [false, false, false, false],
                [false, true, false, false],
                [false, false, false, true],
                [false, true, false, true],
                [false, false, true, false],
                [true, false, false, false],
                [true, false, true, false],
                [false, true, true, false],
                [false, false, true, true],
                [true, true, false, false],
                [true, false, false, true],
                [true, true, true, false],
                [true, false, true, true],
                [false, true, true, true],
                [true, true, false, true],
                [true, true, true, true],
            ];
            branch_node(metrics, canvas, edges[(index / 2) as usize], filled);
        }
        _ => unreachable!("branch sprite {codepoint:x}"),
    }
}

/// A circle, filled or outlined, with lines to the chosen `[up, right,
/// down, left]` edges, centered on the box-drawing lines.
fn branch_node(
    metrics: Metrics,
    canvas: &mut Canvas,
    [up, right, down, left]: [bool; 4],
    filled: bool,
) {
    let thick_px = Thickness::Light.pixels(metrics.box_thickness);
    let (width, height) = (
        f64::from(metrics.cell_width),
        f64::from(metrics.cell_height),
    );
    let thick = f64::from(thick_px);
    let h_top = metrics.cell_height.saturating_sub(thick_px) / 2;
    let h_bottom = h_top + thick_px;
    let v_left = metrics.cell_width.saturating_sub(thick_px) / 2;
    let v_right = v_left + thick_px;
    let cx = f64::from(v_left) + thick / 2.0;
    let cy = f64::from(h_top) + thick / 2.0;
    let r = cx.min(cy).min(width - cx).min(height - cy);
    if up {
        canvas.box_(
            v_left as i32,
            0,
            v_right as i32,
            (cy - r + thick / 2.0).ceil() as i32,
            Shade::On,
        );
    }
    if right {
        let x = (cx + r - thick / 2.0).floor() as i32;
        canvas.box_(
            x,
            h_top as i32,
            metrics.cell_width as i32,
            h_bottom as i32,
            Shade::On,
        );
    }
    if down {
        let y = (cy + r - thick / 2.0).floor() as i32;
        canvas.box_(
            v_left as i32,
            y,
            v_right as i32,
            metrics.cell_height as i32,
            Shade::On,
        );
    }
    if left {
        canvas.box_(
            0,
            h_top as i32,
            (cx - r + thick / 2.0).ceil() as i32,
            h_bottom as i32,
            Shade::On,
        );
    }
    let mut path = Path::default();
    if filled {
        path.circle(DVec2::new(cx, cy), r);
        canvas.fill_path(&path, Shade::On);
    } else {
        path.circle(DVec2::new(cx, cy), r - thick / 2.0);
        canvas.stroke_path(&path, thick, Shade::On);
    }
}

/// A light line from the center line fading out towards `to`.
fn fading_line(metrics: Metrics, canvas: &mut Canvas, to: Edge) {
    let thick = Thickness::Light.pixels(metrics.box_thickness);
    let (width, height) = (metrics.cell_width, metrics.cell_height);
    let h_top = height.saturating_sub(thick) / 2;
    let v_left = width.saturating_sub(thick) / 2;
    let (mut color, increment) = match to {
        Edge::Top => (0.0, 255.0 / f64::from(height)),
        Edge::Left => (0.0, 255.0 / f64::from(width)),
        Edge::Bottom => (255.0, -255.0 / f64::from(height)),
        Edge::Right => (255.0, -255.0 / f64::from(width)),
    };
    match to {
        Edge::Top | Edge::Bottom => {
            for y in 0..height {
                for x in v_left..v_left + thick {
                    canvas.pixel(x as i32, y as i32, f64::round(color) as u8);
                }
                color += increment;
            }
        }
        Edge::Left | Edge::Right => {
            for x in 0..width {
                for y in h_top..h_top + thick {
                    canvas.pixel(x as i32, y as i32, f64::round(color) as u8);
                }
                color += increment;
            }
        }
    }
}
