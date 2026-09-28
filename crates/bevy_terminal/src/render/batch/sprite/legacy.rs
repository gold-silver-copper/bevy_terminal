//! Symbols for Legacy Computing, U+1FB00–1FBEF, and its supplement,
//! U+1CC00–1CEBF (Ghostty's `sprite/draw/symbols_for_legacy_computing*.zig`
//! and `octants.txt`).

use super::box_drawing::{Style, diagonal_falling, diagonal_rising, lines};
use super::canvas::{Canvas, Path, Shade};
use super::shapes::{LEFT, LOWER, RIGHT, UPPER, block, corner_triangle_shade, full};
use super::{Corner, Edge, Fraction, Horizontal, Metrics, Thickness, Vertical, fill};
use bevy::math::DVec2;

/// Smooth mosaics U+1FB3C–1FB67 as 4 × 3 patterns: `#` cells are filled and
/// the outline runs through the corners and edge midpoints they leave.
const MOSAICS: [&str; 44] = [
    "......#..##.",  // 0x1fb3c
    "......#\\.###", // 0x1fb3d
    "...#..#\\.##.", // 0x1fb3e
    "...#..##.###",  // 0x1fb3f
    "#..#..##.##.",  // 0x1fb40
    "/###########",  // 0x1fb41
    "./##########",  // 0x1fb42
    ".##.########",  // 0x1fb43
    "..#.########",  // 0x1fb44
    ".##.##.#####",  // 0x1fb45
    "..../#######",  // 0x1fb46
    "........#.##",  // 0x1fb47
    "......./####",  // 0x1fb48
    ".....#./#.##",  // 0x1fb49
    ".....#.#####",  // 0x1fb4a
    "..#..#.##.##",  // 0x1fb4b
    "##\\#########", // 0x1fb4c
    "#\\.#########", // 0x1fb4d
    "##.##.######",  // 0x1fb4e
    "#..##.######",  // 0x1fb4f
    "##.##.##.###",  // 0x1fb50
    "...#\\.######", // 0x1fb51
    "#########\\##", // 0x1fb52
    "#########.\\#", // 0x1fb53
    "######.##.##",  // 0x1fb54
    "######.##..#",  // 0x1fb55
    "###.##.##.##",  // 0x1fb56
    "##.#........",  // 0x1fb57
    "####/.......",  // 0x1fb58
    "##.#/.#.....",  // 0x1fb59
    "#####.#.....",  // 0x1fb5a
    "##.##.#..#..",  // 0x1fb5b
    "#######/....",  // 0x1fb5c
    "###########/",  // 0x1fb5d
    "##########/.",  // 0x1fb5e
    "########.##.",  // 0x1fb5f
    "########.#..",  // 0x1fb60
    "#####.##.##.",  // 0x1fb61
    ".##..#......",  // 0x1fb62
    "###.\\#......", // 0x1fb63
    ".##.\\#..#...", // 0x1fb64
    "###.##..#...",  // 0x1fb65
    ".##.##..#..#",  // 0x1fb66
    "######.\\#...", // 0x1fb67
];

pub(super) fn draw(codepoint: u32, canvas: &mut Canvas, width: u32, height: u32, metrics: Metrics) {
    match codepoint {
        0x1fb00..=0x1fb3b => sextant(codepoint, canvas, metrics),
        0x1fb3c..=0x1fb67 => {
            smooth_mosaic(MOSAICS[(codepoint - 0x1fb3c) as usize], canvas, metrics)
        }
        0x1fb68..=0x1fb6f => {
            let edge = [Edge::Left, Edge::Top, Edge::Right, Edge::Bottom][(codepoint % 4) as usize];
            edge_triangle(metrics, canvas, edge);
            if codepoint < 0x1fb6c {
                canvas.invert();
                canvas.clip_to_cell();
            }
        }
        0x1fb70..=0x1fb75 => {
            let n = codepoint + 1 - 0x1fb70;
            fill(
                metrics,
                canvas,
                Fraction::eighths(n),
                Fraction::eighths(n + 1),
                Fraction::ZERO,
                Fraction::ONE,
            );
        }
        0x1fb76..=0x1fb7b => horizontal_eighth(codepoint + 1 - 0x1fb76, canvas, metrics),
        0x1fb7c..=0x1fb97 => blocks(codepoint, canvas, width, height, metrics),
        0x1fb98 | 0x1fb99 => hatching(codepoint == 0x1fb99, canvas, metrics),
        0x1fb9a => {
            edge_triangle(metrics, canvas, Edge::Top);
            edge_triangle(metrics, canvas, Edge::Bottom);
        }
        0x1fb9b => {
            edge_triangle(metrics, canvas, Edge::Left);
            edge_triangle(metrics, canvas, Edge::Right);
        }
        0x1fb9c => corner_triangle_shade(metrics, canvas, Corner::TopLeft, Shade::Medium),
        0x1fb9d => corner_triangle_shade(metrics, canvas, Corner::TopRight, Shade::Medium),
        0x1fb9e => corner_triangle_shade(metrics, canvas, Corner::BottomRight, Shade::Medium),
        0x1fb9f => corner_triangle_shade(metrics, canvas, Corner::BottomLeft, Shade::Medium),
        0x1fba0..=0x1fbae => {
            // [top-left, top-right, bottom-left, bottom-right] diagonals.
            let corners: [[bool; 4]; 15] = [
                [true, false, false, false],
                [false, true, false, false],
                [false, false, true, false],
                [false, false, false, true],
                [true, false, true, false],
                [false, true, false, true],
                [false, false, true, true],
                [true, true, false, false],
                [true, false, false, true],
                [false, true, true, false],
                [false, true, true, true],
                [true, false, true, true],
                [true, true, false, true],
                [true, true, true, false],
                [true, true, true, true],
            ];
            corner_diagonal_lines(metrics, canvas, corners[(codepoint - 0x1fba0) as usize]);
        }
        0x1fbaf => lines(
            metrics,
            canvas,
            [Style::Heavy, Style::Light, Style::Heavy, Style::Light],
        ),
        0x1fbbd => {
            diagonal_rising(metrics, canvas);
            diagonal_falling(metrics, canvas);
            canvas.invert();
            canvas.clip_to_cell();
        }
        0x1fbbe => {
            corner_diagonal_lines(metrics, canvas, [false, false, false, true]);
            canvas.invert();
            canvas.clip_to_cell();
        }
        0x1fbbf => {
            corner_diagonal_lines(metrics, canvas, [true; 4]);
            canvas.invert();
            canvas.clip_to_cell();
        }
        0x1fbce => block(metrics, canvas, LEFT, 2.0 / 3.0, 1.0, Shade::On),
        0x1fbcf => block(metrics, canvas, LEFT, 1.0 / 3.0, 1.0, Shade::On),
        0x1fbd0..=0x1fbdf => cell_diagonals(codepoint, canvas, metrics),
        0x1fbe0..=0x1fbef => {
            use Horizontal::{Center as C, Left as L, Right as R};
            use Vertical::{Bottom as B, Middle as M, Top as T};
            match codepoint {
                0x1fbe0 => circle(metrics, canvas, (C, T), false),
                0x1fbe1 => circle(metrics, canvas, (R, M), false),
                0x1fbe2 => circle(metrics, canvas, (C, B), false),
                0x1fbe3 => circle(metrics, canvas, (L, M), false),
                0x1fbe4 => block(metrics, canvas, UPPER, 0.5, 0.5, Shade::On),
                0x1fbe5 => block(metrics, canvas, LOWER, 0.5, 0.5, Shade::On),
                0x1fbe6 => block(metrics, canvas, LEFT, 0.5, 0.5, Shade::On),
                0x1fbe7 => block(metrics, canvas, RIGHT, 0.5, 0.5, Shade::On),
                0x1fbe8 => circle(metrics, canvas, (C, T), true),
                0x1fbe9 => circle(metrics, canvas, (R, M), true),
                0x1fbea => circle(metrics, canvas, (C, B), true),
                0x1fbeb => circle(metrics, canvas, (L, M), true),
                0x1fbec => circle(metrics, canvas, (R, T), true),
                0x1fbed => circle(metrics, canvas, (L, B), true),
                0x1fbee => circle(metrics, canvas, (R, B), true),
                _ => circle(metrics, canvas, (L, T), true),
            }
        }
        0x1cc1b..=0x1cc1e => box_corners(codepoint, canvas, width, height, metrics),
        0x1cc21..=0x1cc2f => separated_quadrants(codepoint, canvas, width, height),
        0x1cc30..=0x1cc3f => {
            // (x, y, width, height) in pieces of the circle and its corner.
            let (x, y, w, h, corner) = match codepoint {
                0x1cc30 => (0.0, 0.0, 2.0, 2.0, Corner::TopLeft),
                0x1cc31 => (1.0, 0.0, 2.0, 2.0, Corner::TopLeft),
                0x1cc32 => (2.0, 0.0, 2.0, 2.0, Corner::TopRight),
                0x1cc33 => (3.0, 0.0, 2.0, 2.0, Corner::TopRight),
                0x1cc34 => (0.0, 1.0, 2.0, 2.0, Corner::TopLeft),
                0x1cc35 => (0.0, 0.0, 1.0, 1.0, Corner::TopLeft),
                0x1cc36 => (1.0, 0.0, 1.0, 1.0, Corner::TopRight),
                0x1cc37 => (3.0, 1.0, 2.0, 2.0, Corner::TopRight),
                0x1cc38 => (0.0, 2.0, 2.0, 2.0, Corner::BottomLeft),
                0x1cc39 => (0.0, 1.0, 1.0, 1.0, Corner::BottomLeft),
                0x1cc3a => (1.0, 1.0, 1.0, 1.0, Corner::BottomRight),
                0x1cc3b => (3.0, 2.0, 2.0, 2.0, Corner::BottomRight),
                0x1cc3c => (0.0, 3.0, 2.0, 2.0, Corner::BottomLeft),
                0x1cc3d => (1.0, 3.0, 2.0, 2.0, Corner::BottomLeft),
                0x1cc3e => (2.0, 3.0, 2.0, 2.0, Corner::BottomRight),
                _ => (3.0, 3.0, 2.0, 2.0, Corner::BottomRight),
            };
            circle_piece(canvas, width, height, metrics, (x, y, w, h), corner);
        }
        0x1cd00..=0x1cde5 => octant(codepoint, canvas, metrics),
        0x1ce00 => {
            circle(metrics, canvas, (Horizontal::Left, Vertical::Middle), false);
            circle(
                metrics,
                canvas,
                (Horizontal::Right, Vertical::Middle),
                false,
            );
        }
        0x1ce01 => {
            circle(metrics, canvas, (Horizontal::Center, Vertical::Top), false);
            circle(
                metrics,
                canvas,
                (Horizontal::Center, Vertical::Bottom),
                false,
            );
        }
        0x1ce0b => {
            circle_piece(
                canvas,
                width,
                height,
                metrics,
                (0.0, 0.0, 1.0, 0.5),
                Corner::TopLeft,
            );
            circle_piece(
                canvas,
                width,
                height,
                metrics,
                (0.0, 0.0, 1.0, 0.5),
                Corner::BottomLeft,
            );
        }
        0x1ce0c => {
            circle_piece(
                canvas,
                width,
                height,
                metrics,
                (1.0, 0.0, 1.0, 0.5),
                Corner::TopRight,
            );
            circle_piece(
                canvas,
                width,
                height,
                metrics,
                (1.0, 0.0, 1.0, 0.5),
                Corner::BottomRight,
            );
        }
        0x1ce16..=0x1ce19 => {
            let (w, h, t) = (width as i32, height as i32, metrics.box_thickness as i32);
            lines(
                metrics,
                canvas,
                [Style::Light, Style::None, Style::Light, Style::None],
            );
            match codepoint {
                0x1ce16 => canvas.box_(w.div_euclid(2), 0, w, t, Shade::On),
                0x1ce17 => canvas.box_(w.div_euclid(2), h - t, w, h, Shade::On),
                0x1ce18 => canvas.box_(0, 0, w.div_euclid(2), t, Shade::On),
                _ => canvas.box_(0, h - t, w.div_euclid(2), h, Shade::On),
            }
        }
        0x1ce51..=0x1ce8f => separated_sextants(codepoint, canvas, width, height),
        0x1ce90..=0x1ceaf => {
            let q = Fraction::quarters;
            // Quarter-cell (x0, x1, y0, y1) boxes.
            let [x0, x1, y0, y1] = match codepoint {
                0x1ce90..=0x1ce9f => {
                    let index = codepoint - 0x1ce90;
                    [index % 4, index % 4 + 1, index / 4, index / 4 + 1]
                }
                0x1cea0 => [2, 4, 3, 4],
                0x1cea1 => [1, 4, 3, 4],
                0x1cea2 => [0, 3, 3, 4],
                0x1cea3 => [0, 2, 3, 4],
                0x1cea4 => [0, 1, 2, 4],
                0x1cea5 => [0, 1, 1, 4],
                0x1cea6 => [0, 1, 0, 3],
                0x1cea7 => [0, 1, 0, 2],
                0x1cea8 => [0, 2, 0, 1],
                0x1cea9 => [0, 3, 0, 1],
                0x1ceaa => [1, 4, 0, 1],
                0x1ceab => [2, 4, 0, 1],
                0x1ceac => [3, 4, 0, 2],
                0x1cead => [3, 4, 0, 3],
                0x1ceae => [3, 4, 1, 4],
                _ => [3, 4, 2, 4],
            };
            fill(metrics, canvas, q(x0), q(x1), q(y0), q(y1));
        }
        _ => unreachable!("legacy computing sprite {codepoint:x}"),
    }
}

/// Sextants U+1FB00–1FB3B: the 2 × 3 patterns other than empty, the two
/// half blocks and full (encoded as bits skipping those).
fn sextant(codepoint: u32, canvas: &mut Canvas, metrics: Metrics) {
    let index = codepoint - 0x1fb00;
    let bits = index + index / 0x14 + 1;
    let (z, h, f) = (Fraction::ZERO, Fraction::HALF, Fraction::ONE);
    let (t1, t2) = (Fraction::THIRD, Fraction::TWO_THIRDS);
    let cells = [
        (z, h, z, t1),
        (h, f, z, t1),
        (z, h, t1, t2),
        (h, f, t1, t2),
        (z, h, t2, f),
        (h, f, t2, f),
    ];
    for (bit, (x0, x1, y0, y1)) in cells.into_iter().enumerate() {
        if bits & (1 << bit) != 0 {
            fill(metrics, canvas, x0, x1, y0, y1);
        }
    }
}

fn smooth_mosaic(pattern: &str, canvas: &mut Canvas, metrics: Metrics) {
    let m: Vec<bool> = pattern.bytes().map(|c| c == b'#').collect();
    let (height, width) = (metrics.cell_height, metrics.cell_width);
    let (top, bottom) = (0.0, f64::from(height));
    let (upper, lower) = (
        Fraction::THIRD.float(height),
        Fraction::TWO_THIRDS.float(height),
    );
    let (left, center, right) = (0.0, Fraction::HALF.float(width), f64::from(width));
    let corners = [
        (m[0], left, top),
        (m[3] && (!m[0] || !m[6]), left, upper),
        (m[6] && (!m[3] || !m[9]), left, lower),
        (m[9], left, bottom),
        (m[10] && (!m[9] || !m[11]), center, bottom),
        (m[11], right, bottom),
        (m[8] && (!m[11] || !m[5]), right, lower),
        (m[5] && (!m[8] || !m[2]), right, upper),
        (m[2], right, top),
        (m[1] && (!m[2] || !m[0]), center, top),
    ];
    let mut path = Path::default();
    for (_, x, y) in corners.into_iter().filter(|(on, _, _)| *on) {
        path.line_to(x, y);
    }
    path.close();
    canvas.fill_path(&path, Shade::On);
}

/// The horizontal bar between the `n`th and next eighth of the cell.
fn horizontal_eighth(n: u32, canvas: &mut Canvas, metrics: Metrics) {
    fill(
        metrics,
        canvas,
        Fraction::ZERO,
        Fraction::ONE,
        Fraction::eighths(n),
        Fraction::eighths(n + 1),
    );
}

fn blocks(codepoint: u32, canvas: &mut Canvas, width: u32, height: u32, metrics: Metrics) {
    let on = Shade::On;
    let eighth = 1.0 / 8.0;
    match codepoint {
        0x1fb7c => {
            block(metrics, canvas, LEFT, eighth, 1.0, on);
            block(metrics, canvas, LOWER, 1.0, eighth, on);
        }
        0x1fb7d => {
            block(metrics, canvas, LEFT, eighth, 1.0, on);
            block(metrics, canvas, UPPER, 1.0, eighth, on);
        }
        0x1fb7e => {
            block(metrics, canvas, RIGHT, eighth, 1.0, on);
            block(metrics, canvas, UPPER, 1.0, eighth, on);
        }
        0x1fb7f => {
            block(metrics, canvas, RIGHT, eighth, 1.0, on);
            block(metrics, canvas, LOWER, 1.0, eighth, on);
        }
        0x1fb80 => {
            block(metrics, canvas, UPPER, 1.0, eighth, on);
            block(metrics, canvas, LOWER, 1.0, eighth, on);
        }
        0x1fb81 => {
            for n in [0, 2, 4, 7] {
                horizontal_eighth(n, canvas, metrics);
            }
        }
        0x1fb82 => block(metrics, canvas, UPPER, 1.0, 0.25, on),
        0x1fb83 => block(metrics, canvas, UPPER, 1.0, 0.375, on),
        0x1fb84 => block(metrics, canvas, UPPER, 1.0, 0.625, on),
        0x1fb85 => block(metrics, canvas, UPPER, 1.0, 0.75, on),
        0x1fb86 => block(metrics, canvas, UPPER, 1.0, 0.875, on),
        0x1fb87 => block(metrics, canvas, RIGHT, 0.25, 1.0, on),
        0x1fb88 => block(metrics, canvas, RIGHT, 0.375, 1.0, on),
        0x1fb89 => block(metrics, canvas, RIGHT, 0.625, 1.0, on),
        0x1fb8a => block(metrics, canvas, RIGHT, 0.75, 1.0, on),
        0x1fb8b => block(metrics, canvas, RIGHT, 0.875, 1.0, on),
        0x1fb8c => block(metrics, canvas, LEFT, 0.5, 1.0, Shade::Medium),
        0x1fb8d => block(metrics, canvas, RIGHT, 0.5, 1.0, Shade::Medium),
        0x1fb8e => block(metrics, canvas, UPPER, 1.0, 0.5, Shade::Medium),
        0x1fb8f => block(metrics, canvas, LOWER, 1.0, 0.5, Shade::Medium),
        0x1fb90 => full(metrics, canvas, Shade::Medium),
        0x1fb91 => {
            full(metrics, canvas, Shade::Medium);
            block(metrics, canvas, UPPER, 1.0, 0.5, on);
        }
        0x1fb92 => {
            full(metrics, canvas, Shade::Medium);
            block(metrics, canvas, LOWER, 1.0, 0.5, on);
        }
        // Unallocated.
        0x1fb93 => {}
        0x1fb94 => {
            full(metrics, canvas, Shade::Medium);
            block(metrics, canvas, RIGHT, 0.5, 1.0, on);
        }
        0x1fb95 | 0x1fb96 => checkerboard(metrics, canvas, codepoint - 0x1fb95),
        _ => {
            canvas.box_(
                0,
                (height / 4) as i32,
                width as i32,
                (2 * height / 4) as i32,
                on,
            );
            canvas.box_(0, (3 * height / 4) as i32, width as i32, height as i32, on);
        }
    }
}

/// Diagonal hatching, falling (`🮘`) or rising (`🮙`), clipped to the cell.
fn hatching(rising: bool, canvas: &mut Canvas, metrics: Metrics) {
    canvas.clip_to_cell();
    let thick = Thickness::Light.pixels(metrics.box_thickness);
    let count = (metrics.cell_width / (2 * thick)).max(1);
    let (width, height) = (
        f64::from(metrics.cell_width),
        f64::from(metrics.cell_height),
    );
    let stride = (width / f64::from(count)).round();
    for i in 0..=2 * count {
        let offset = f64::from(i as i32 - count as i32) * stride;
        let (top, bottom) = if rising {
            (width + offset, offset)
        } else {
            (offset, width + offset)
        };
        canvas.line(
            DVec2::new(top, 0.0),
            DVec2::new(bottom, height),
            f64::from(thick),
            Shade::On,
        );
    }
}

fn edge_triangle(metrics: Metrics, canvas: &mut Canvas, edge: Edge) {
    let (width, height) = (
        f64::from(metrics.cell_width),
        f64::from(metrics.cell_height),
    );
    let (middle, center) = ((height / 2.0).round(), (width / 2.0).round());
    let (a, b) = match edge {
        Edge::Top => (DVec2::new(width, 0.0), DVec2::new(0.0, 0.0)),
        Edge::Left => (DVec2::new(0.0, 0.0), DVec2::new(0.0, height)),
        Edge::Bottom => (DVec2::new(0.0, height), DVec2::new(width, height)),
        Edge::Right => (DVec2::new(width, height), DVec2::new(width, 0.0)),
    };
    canvas.triangle([DVec2::new(center, middle), a, b], Shade::On);
}

fn corner_diagonal_lines(metrics: Metrics, canvas: &mut Canvas, [tl, tr, bl, br]: [bool; 4]) {
    let thick = f64::from(Thickness::Light.pixels(metrics.box_thickness));
    let (width, height) = (
        f64::from(metrics.cell_width),
        f64::from(metrics.cell_height),
    );
    let cx = f64::from(metrics.cell_width / 2 + metrics.cell_width % 2);
    let cy = f64::from(metrics.cell_height / 2 + metrics.cell_height % 2);
    let point = DVec2::new;
    for (on, from, to) in [
        (tl, point(cx, 0.0), point(0.0, cy)),
        (tr, point(cx, 0.0), point(width, cy)),
        (bl, point(cx, height), point(0.0, cy)),
        (br, point(cx, height), point(width, cy)),
    ] {
        if on {
            canvas.line(from, to, thick, Shade::On);
        }
    }
}

/// A cell corner, edge midpoint or center.
type Position = (Horizontal, Vertical);

/// U+1FBD0–1FBDF: diagonals between cell corners and edge midpoints.
fn cell_diagonals(codepoint: u32, canvas: &mut Canvas, metrics: Metrics) {
    use Horizontal::{Center as C, Left as L, Right as R};
    use Vertical::{Bottom as B, Middle as M, Top as T};
    let segments: &[(Position, Position)] = match codepoint {
        0x1fbd0 => &[((R, M), (L, B))],
        0x1fbd1 => &[((R, T), (L, M))],
        0x1fbd2 => &[((L, T), (R, M))],
        0x1fbd3 => &[((L, M), (R, B))],
        0x1fbd4 => &[((L, T), (C, B))],
        0x1fbd5 => &[((C, T), (R, B))],
        0x1fbd6 => &[((R, T), (C, B))],
        0x1fbd7 => &[((C, T), (L, B))],
        0x1fbd8 => &[((L, T), (C, M)), ((C, M), (R, T))],
        0x1fbd9 => &[((R, T), (C, M)), ((C, M), (R, B))],
        0x1fbda => &[((L, B), (C, M)), ((C, M), (R, B))],
        0x1fbdb => &[((L, T), (C, M)), ((C, M), (L, B))],
        0x1fbdc => &[((L, T), (C, B)), ((C, B), (R, T))],
        0x1fbdd => &[((R, T), (L, M)), ((L, M), (R, B))],
        0x1fbde => &[((L, B), (C, T)), ((C, T), (R, B))],
        _ => &[((L, T), (R, M)), ((R, M), (L, B))],
    };
    let (width, height) = (
        f64::from(metrics.cell_width),
        f64::from(metrics.cell_height),
    );
    let position = |(h, v): (Horizontal, Vertical)| {
        DVec2::new(
            match h {
                Horizontal::Left => 0.0,
                Horizontal::Center => width / 2.0,
                Horizontal::Right => width,
            },
            match v {
                Vertical::Top => 0.0,
                Vertical::Middle => height / 2.0,
                Vertical::Bottom => height,
            },
        )
    };
    let thick = f64::from(Thickness::Light.pixels(metrics.box_thickness));
    for (from, to) in segments {
        canvas.line(position(*from), position(*to), thick, Shade::On);
    }
}

fn checkerboard(metrics: Metrics, canvas: &mut Canvas, parity: u32) {
    let (width, height) = (metrics.cell_width, metrics.cell_height);
    let x_size = 4;
    let y_size = (4.0 * (f64::from(height) / f64::from(width))).round() as u32;
    for x in 0..x_size {
        let (x0, x1) = (width * x / x_size, width * (x + 1) / x_size);
        for y in 0..y_size {
            let (y0, y1) = (height * y / y_size, height * (y + 1) / y_size);
            if (x + y) % 2 == parity {
                canvas.box_(x0 as i32, y0 as i32, x1 as i32, y1 as i32, Shade::On);
            }
        }
    }
}

/// A circle of half the cell's smaller side, centered on a cell corner,
/// edge midpoint or center, clipped to the cell.
fn circle(
    metrics: Metrics,
    canvas: &mut Canvas,
    (horizontal, vertical): (Horizontal, Vertical),
    filled: bool,
) {
    canvas.clip_to_cell();
    let (width, height) = (
        f64::from(metrics.cell_width),
        f64::from(metrics.cell_height),
    );
    let x = match horizontal {
        Horizontal::Left => 0.0,
        Horizontal::Center => width / 2.0,
        Horizontal::Right => width,
    };
    let y = match vertical {
        Vertical::Top => 0.0,
        Vertical::Middle => height / 2.0,
        Vertical::Bottom => height,
    };
    let r = 0.5 * width.min(height);
    let thick = f64::from(Thickness::Light.pixels(metrics.box_thickness));
    let mut path = Path::default();
    if filled {
        path.circle(DVec2::new(x, y), r);
        canvas.fill_path(&path, Shade::On);
    } else {
        path.circle(DVec2::new(x, y), r - thick / 2.0);
        canvas.stroke_path(&path, thick, Shade::On);
    }
}

/// Octants U+1CD00–1CDE5 (2 × 4 patterns, listed in `octants.txt`).
fn octant(codepoint: u32, canvas: &mut Canvas, metrics: Metrics) {
    let line = include_str!("octants.txt")
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .nth((codepoint - 0x1cd00) as usize)
        .expect("octant listed");
    let cells = line.rsplit_once('-').map_or("", |(_, cells)| cells);
    let (z, h, f) = (Fraction::ZERO, Fraction::HALF, Fraction::ONE);
    let (q1, q2, q3) = (Fraction::QUARTER, Fraction::HALF, Fraction::THREE_QUARTERS);
    for cell in cells.bytes() {
        let (x0, x1, y0, y1) = match cell {
            b'1' => (z, h, z, q1),
            b'2' => (h, f, z, q1),
            b'3' => (z, h, q1, q2),
            b'4' => (h, f, q1, q2),
            b'5' => (z, h, q2, q3),
            b'6' => (h, f, q2, q3),
            b'7' => (z, h, q3, f),
            _ => (h, f, q3, f),
        };
        fill(metrics, canvas, x0, x1, y0, y1);
    }
}

/// U+1CC1B–1CC1E: box-drawing lines with a corner mark.
fn box_corners(codepoint: u32, canvas: &mut Canvas, width: u32, height: u32, metrics: Metrics) {
    let (w, h, t) = (width as i32, height as i32, metrics.box_thickness as i32);
    match codepoint {
        0x1cc1b => {
            lines(
                metrics,
                canvas,
                [Style::None, Style::Light, Style::None, Style::Light],
            );
            canvas.box_(w - t, 0, w, h.div_euclid(2), Shade::On);
        }
        0x1cc1c => {
            lines(
                metrics,
                canvas,
                [Style::None, Style::Light, Style::None, Style::Light],
            );
            canvas.box_(w - t, h.div_euclid(2), w, h, Shade::On);
        }
        0x1cc1d => {
            canvas.box_(0, 0, w, t, Shade::On);
            canvas.box_(0, 0, t, h.div_euclid(2), Shade::On);
        }
        _ => {
            canvas.box_(0, h - t, w, h, Shade::On);
            canvas.box_(0, h.div_euclid(2), t, h, Shade::On);
        }
    }
}

/// U+1CC21–1CC2F: quadrants separated by gaps.
fn separated_quadrants(codepoint: u32, canvas: &mut Canvas, width: u32, height: u32) {
    let bits = codepoint - 0x1cc20;
    let gap = (width / 12).max(1) as i32;
    let mid_x = gap * 2 + (width % 2) as i32;
    let mid_y = gap * 2 + (height % 2) as i32;
    let w = (width as i32 - gap * 2 - mid_x) / 2;
    let h = (height as i32 - gap * 2 - mid_y) / 2;
    let columns = [gap, gap + w + mid_x];
    let rows = [gap, gap + h + mid_y];
    for (bit, (column, row)) in [(0, 0), (1, 0), (0, 1), (1, 1)].into_iter().enumerate() {
        if bits & (1 << bit) != 0 {
            let (x, y) = (columns[column], rows[row]);
            canvas.box_(x, y, x + w, y + h, Shade::On);
        }
    }
}

/// U+1CE51–1CE8F: sextants separated by gaps.
fn separated_sextants(codepoint: u32, canvas: &mut Canvas, width: u32, height: u32) {
    let bits = codepoint - 0x1ce50;
    let gap = (width / 12).max(1) as i32;
    let mid_x = gap * 2 + (width % 2) as i32;
    let mid_y = gap * 2 + ((height % 3) as i32).div_euclid(2);
    let w = (width as i32 - gap * 2 - mid_x) / 2;
    let h = (height as i32 - gap * 2 - mid_y * 2).div_euclid(3);
    let h_middle = height as i32 - gap * 2 - mid_y * 2 - h * 2;
    let columns = [gap, gap + w + mid_x];
    let rows = [
        (gap, h),
        (gap + h + mid_y, h_middle),
        (gap + h + mid_y + h_middle + mid_y, h),
    ];
    for bit in 0..6 {
        if bits & (1 << bit) != 0 {
            let x = columns[bit % 2];
            let (y, row_height) = rows[bit / 2];
            canvas.box_(x, y, x + w, y + row_height, Shade::On);
        }
    }
}

/// A quarter of an ellipse spanning `(x, y, w, h)` cells' worth of pieces,
/// stroked in the corner `corner` and clipped to the cell.
fn circle_piece(
    canvas: &mut Canvas,
    width: u32,
    height: u32,
    metrics: Metrics,
    (x, y, w, h): (f64, f64, f64, f64),
    corner: Corner,
) {
    let (width, height) = (f64::from(width), f64::from(height));
    let (wdth, hght) = (width * w, height * h);
    let (xp, yp) = (width * x, height * y);
    canvas.clip_to_cell();
    let c = (std::f64::consts::SQRT_2 - 1.0) * 4.0 / 3.0;
    let (cw, ch) = (c * wdth, c * hght);
    let thick = f64::from(metrics.box_thickness);
    let ht = thick * 0.5;
    let mut path = Path::default();
    match corner {
        Corner::TopLeft => {
            path.move_to(wdth - xp, ht - yp).curve_to(
                wdth - cw - xp,
                ht - yp,
                ht - xp,
                hght - ch - yp,
                ht - xp,
                hght - yp,
            );
        }
        Corner::TopRight => {
            path.move_to(wdth - xp, ht - yp).curve_to(
                wdth + cw - xp,
                ht - yp,
                wdth * 2.0 - ht - xp,
                hght - ch - yp,
                wdth * 2.0 - ht - xp,
                hght - yp,
            );
        }
        Corner::BottomLeft => {
            path.move_to(ht - xp, hght - yp).curve_to(
                ht - xp,
                hght + ch - yp,
                wdth - cw - xp,
                hght * 2.0 - ht - yp,
                wdth - xp,
                hght * 2.0 - ht - yp,
            );
        }
        Corner::BottomRight => {
            path.move_to(wdth * 2.0 - ht - xp, hght - yp).curve_to(
                wdth * 2.0 - ht - xp,
                hght + ch - yp,
                wdth + cw - xp,
                hght * 2.0 - ht - yp,
                wdth - xp,
                hght * 2.0 - ht - yp,
            );
        }
    }
    canvas.stroke_path(&path, thick, Shade::On);
}
