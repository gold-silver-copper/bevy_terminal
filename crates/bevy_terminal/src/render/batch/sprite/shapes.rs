//! Block elements, geometric triangles, Braille, Powerline and branch
//! drawing (Ghostty's `sprite/draw/{block,geometric_shapes,braille,
//! powerline,branch}.zig`).

use super::box_drawing::{Style, arc, diagonal_falling, diagonal_rising, lines};
use super::canvas::{Canvas, Path, Shade};
use super::{Corner, Edge, Horizontal, Metrics, Vertical, fill_grid, grid_cell};
use bevy::math::DVec2;

/// A block of `width` × `height` cell fractions aligned in the cell.
pub(super) fn block(
    metrics: Metrics,
    canvas: &mut Canvas,
    alignment: (Horizontal, Vertical),
    width: f64,
    height: f64,
    shade: Shade,
) {
    let [x0, y0, x1, y1] = block_rect(metrics, alignment, width, height);
    canvas.box_(x0, y0, x1, y1, shade);
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

/// The pixel rectangle `block` fills.
fn block_rect(
    metrics: Metrics,
    (horizontal, vertical): (Horizontal, Vertical),
    width: f64,
    height: f64,
) -> [i32; 4] {
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
    [x as i32, y as i32, (x + w) as i32, (y + h) as i32]
}

/// The quadrants each of U+2596–259F fills, as bits (top-left, top-right,
/// bottom-left, bottom-right from the lowest). Complementary rounding makes
/// quadrants overlap at odd cell sizes.
fn quadrants(codepoint: u32) -> u32 {
    const QUADRANTS: [u32; 10] = [
        0b0100, 0b1000, 0b0001, 0b1101, 0b1001, 0b0111, 0b1011, 0b0010, 0b0110, 0b1110,
    ];
    codepoint
        .checked_sub(0x2596)
        .and_then(|index| QUADRANTS.get(index as usize))
        .copied()
        .unwrap_or(0)
}

/// The single opaque rectangle `[x0, y0, x1, y1]` (cell pixels) of a block
/// element drawn as one: eighths, halves, the full block and single
/// quadrants. `None` for every other codepoint, including combined
/// quadrants, whose rectangles overlap at odd sizes.
pub(crate) fn solid_block(codepoint: u32, metrics: Metrics) -> Option<[i32; 4]> {
    let eighth = |n: u32| f64::from(n) / 8.0;
    let rect = |alignment, width, height| block_rect(metrics, alignment, width, height);
    Some(match codepoint {
        0x2580 => rect(UPPER, 1.0, 0.5),
        0x2581..=0x2587 => rect(LOWER, 1.0, eighth(codepoint - 0x2580)),
        0x2588 => [0, 0, metrics.cell_width as i32, metrics.cell_height as i32],
        0x2589..=0x258f => rect(LEFT, eighth(0x2590 - codepoint), 1.0),
        0x2590 => rect(RIGHT, 0.5, 1.0),
        0x2594 => rect(UPPER, 1.0, eighth(1)),
        0x2595 => rect(RIGHT, eighth(1), 1.0),
        0x2596..=0x259f => {
            let bits = quadrants(codepoint);
            if bits.count_ones() != 1 {
                return None;
            }
            grid_cell(metrics, bits.trailing_zeros(), 2)
        }
        _ => return None,
    })
}

/// Block Elements, U+2580–259F: eighths, halves, quadrants and shades.
pub(super) fn block_element(codepoint: u32, canvas: &mut Canvas, metrics: Metrics) {
    match codepoint {
        0x2591 => full(metrics, canvas, Shade::Light),
        0x2592 => full(metrics, canvas, Shade::Medium),
        0x2593 => full(metrics, canvas, Shade::Dark),
        0x2596..=0x259f => fill_grid(metrics, canvas, quadrants(codepoint), 2),
        _ => {
            if let Some([x0, y0, x1, y1]) = solid_block(codepoint, metrics) {
                canvas.box_(x0, y0, x1, y1, Shade::On);
            }
        }
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
    canvas.inner_stroke_path(&path, f64::from(metrics.box_thickness), Shade::On);
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
        // U+25FF.
        _ => corner_triangle_outline(metrics, canvas, Corner::BottomRight),
    }
}

/// Braille, U+2800–28FF: square dots on a 2 × 4 grid, spacing and margins
/// distributed so every cell size keeps the pattern even.
pub(super) fn braille(codepoint: u32, canvas: &mut Canvas, metrics: Metrics) {
    let (width, height) = (metrics.cell_width, metrics.cell_height);
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
            let x = x_margin + column * (w + x_spacing);
            let y = y_margin + row * (w + y_spacing);
            canvas.box_(x, y, x + w, y + w, Shade::On);
        }
    }
}

/// The geometric Powerline glyphs, U+E0B0–E0BF, E0D2 and E0D4.
pub(super) fn powerline(codepoint: u32, canvas: &mut Canvas, metrics: Metrics) {
    let (w, h) = (
        f64::from(metrics.cell_width),
        f64::from(metrics.cell_height),
    );
    let point = DVec2::new;
    let thin = f64::from(metrics.box_thickness);
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
        // U+E0D2 and U+E0D4.
        _ => {
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
    }
}

/// Branch drawing, U+F5D0–F60D (git graph lines, arcs and nodes).
pub(super) fn branch(codepoint: u32, canvas: &mut Canvas, metrics: Metrics) {
    use Corner::{BottomLeft as BL, BottomRight as BR, TopLeft as TL, TopRight as TR};
    use Style::{Light as L, None as N};
    let horizontal = |canvas: &mut Canvas| lines(metrics, canvas, [N, L, N, L]);
    let vertical = |canvas: &mut Canvas| lines(metrics, canvas, [L, N, L, N]);
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
        // Nodes U+F5EE–F60D: filled on even codepoints, with lines to the
        // edges (bits up, right, down, left from the lowest) of each pair.
        _ => {
            const EDGES: [u32; 16] = [0, 2, 8, 10, 4, 1, 5, 6, 12, 3, 9, 7, 13, 14, 11, 15];
            let index = codepoint.saturating_sub(0xf5ee);
            let edges = EDGES.get((index / 2) as usize).copied().unwrap_or(0);
            branch_node(metrics, canvas, edges, index.is_multiple_of(2));
        }
    }
}

/// A circle, filled or outlined, with lines to the edges set in `edges`
/// (bits up, right, down, left from the lowest), centered on the
/// box-drawing lines.
fn branch_node(metrics: Metrics, canvas: &mut Canvas, edges: u32, filled: bool) {
    let [up, right, down, left] = std::array::from_fn(|bit| edges & (1 << bit) != 0);
    let thick_px = metrics.box_thickness;
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
    canvas.circle(DVec2::new(cx, cy), r, (!filled).then_some(thick));
}

/// A light line from the center line fading out towards `to`.
fn fading_line(metrics: Metrics, canvas: &mut Canvas, to: Edge) {
    let thick = metrics.box_thickness;
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
