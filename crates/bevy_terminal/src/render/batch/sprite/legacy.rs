//! Symbols for Legacy Computing, U+1FB00–1FBEF, and its supplement,
//! U+1CC00–1CEBF (Ghostty's `sprite/draw/symbols_for_legacy_computing*.zig`
//! and `octants.txt`).

use super::box_drawing::{Style, diagonal_falling, diagonal_rising, lines};
use super::canvas::{Canvas, Path, Shade};
use super::shapes::{LEFT, LOWER, RIGHT, UPPER, block, corner_triangle_shade, full};
use super::{Corner, Edge, Fraction, Horizontal, Metrics, Vertical, fill, fill_grid};
use bevy::math::DVec2;

/// The cells (bits row-major from the lowest, in a 2 × 4 grid) of the octants
/// U+1CD00–1CDE5: the patterns no other block character draws.
const OCTANTS: [u8; 230] = [
    0x04, 0x06, 0x07, 0x08, 0x09, 0x0b, 0x0c, 0x0d, 0x0e, 0x10, 0x11, 0x12, 0x13, 0x15, 0x16, 0x17,
    0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27,
    0x29, 0x2a, 0x2b, 0x2c, 0x2d, 0x2e, 0x2f, 0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38,
    0x39, 0x3a, 0x3b, 0x3c, 0x3d, 0x3e, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4a,
    0x4b, 0x4c, 0x4d, 0x4e, 0x4f, 0x51, 0x52, 0x53, 0x54, 0x56, 0x57, 0x58, 0x59, 0x5b, 0x5c, 0x5d,
    0x5e, 0x60, 0x61, 0x62, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6a, 0x6b, 0x6c, 0x6d, 0x6e,
    0x6f, 0x70, 0x71, 0x72, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x7b, 0x7c, 0x7d, 0x7e,
    0x7f, 0x81, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8a, 0x8b, 0x8c, 0x8d, 0x8e, 0x8f,
    0x90, 0x91, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0x9b, 0x9c, 0x9d, 0x9e, 0x9f,
    0xa1, 0xa2, 0xa3, 0xa4, 0xa6, 0xa7, 0xa8, 0xa9, 0xab, 0xac, 0xad, 0xae, 0xb0, 0xb1, 0xb2, 0xb3,
    0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xbb, 0xbc, 0xbd, 0xbe, 0xbf, 0xc1, 0xc2, 0xc3, 0xc4,
    0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xcb, 0xcc, 0xcd, 0xce, 0xcf, 0xd0, 0xd1, 0xd2, 0xd3, 0xd4,
    0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xdb, 0xdc, 0xdd, 0xde, 0xdf, 0xe0, 0xe1, 0xe2, 0xe3, 0xe4,
    0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xeb, 0xec, 0xed, 0xee, 0xef, 0xf1, 0xf2, 0xf3, 0xf4, 0xf6,
    0xf7, 0xf8, 0xf9, 0xfb, 0xfd, 0xfe,
];

/// Smooth mosaics U+1FB3C–1FB67 as 4 × 3 patterns: `#` cells are filled and
/// the outline runs through the corners and edge midpoints they leave.
const MOSAICS: [&[u8; 12]; 44] = [
    b"......#..##.",  // 0x1fb3c
    b"......#\\.###", // 0x1fb3d
    b"...#..#\\.##.", // 0x1fb3e
    b"...#..##.###",  // 0x1fb3f
    b"#..#..##.##.",  // 0x1fb40
    b"/###########",  // 0x1fb41
    b"./##########",  // 0x1fb42
    b".##.########",  // 0x1fb43
    b"..#.########",  // 0x1fb44
    b".##.##.#####",  // 0x1fb45
    b"..../#######",  // 0x1fb46
    b"........#.##",  // 0x1fb47
    b"......./####",  // 0x1fb48
    b".....#./#.##",  // 0x1fb49
    b".....#.#####",  // 0x1fb4a
    b"..#..#.##.##",  // 0x1fb4b
    b"##\\#########", // 0x1fb4c
    b"#\\.#########", // 0x1fb4d
    b"##.##.######",  // 0x1fb4e
    b"#..##.######",  // 0x1fb4f
    b"##.##.##.###",  // 0x1fb50
    b"...#\\.######", // 0x1fb51
    b"#########\\##", // 0x1fb52
    b"#########.\\#", // 0x1fb53
    b"######.##.##",  // 0x1fb54
    b"######.##..#",  // 0x1fb55
    b"###.##.##.##",  // 0x1fb56
    b"##.#........",  // 0x1fb57
    b"####/.......",  // 0x1fb58
    b"##.#/.#.....",  // 0x1fb59
    b"#####.#.....",  // 0x1fb5a
    b"##.##.#..#..",  // 0x1fb5b
    b"#######/....",  // 0x1fb5c
    b"###########/",  // 0x1fb5d
    b"##########/.",  // 0x1fb5e
    b"########.##.",  // 0x1fb5f
    b"########.#..",  // 0x1fb60
    b"#####.##.##.",  // 0x1fb61
    b".##..#......",  // 0x1fb62
    b"###.\\#......", // 0x1fb63
    b".##.\\#..#...", // 0x1fb64
    b"###.##..#...",  // 0x1fb65
    b".##.##..#..#",  // 0x1fb66
    b"######.\\#...", // 0x1fb67
];

pub(super) fn draw(codepoint: u32, canvas: &mut Canvas, metrics: Metrics) {
    let (width, height) = (metrics.cell_width, metrics.cell_height);
    match codepoint {
        0x1fb00..=0x1fb3b => {
            // The 2 × 3 patterns other than empty, the two half blocks and
            // full, encoded as bits skipping those.
            let index = codepoint - 0x1fb00;
            fill_grid(metrics, canvas, index + index / 0x14 + 1, 3);
        }
        0x1fb3c..=0x1fb67 => {
            if let Some(pattern) = MOSAICS.get((codepoint - 0x1fb3c) as usize) {
                smooth_mosaic(**pattern, canvas, metrics);
            }
        }
        0x1fb68..=0x1fb6f => {
            let edge = match codepoint % 4 {
                0 => Edge::Left,
                1 => Edge::Top,
                2 => Edge::Right,
                _ => Edge::Bottom,
            };
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
        0x1fb7c..=0x1fb97 => blocks(codepoint, canvas, metrics),
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
            const CORNERS: [u32; 15] = [1, 2, 4, 8, 5, 10, 12, 3, 9, 6, 14, 13, 11, 7, 15];
            if let Some(corners) = CORNERS.get((codepoint - 0x1fba0) as usize) {
                corner_diagonal_lines(metrics, canvas, *corners);
            }
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
            corner_diagonal_lines(metrics, canvas, 0b1000);
            canvas.invert();
            canvas.clip_to_cell();
        }
        0x1fbbf => {
            corner_diagonal_lines(metrics, canvas, 0b1111);
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
        0x1cc1b..=0x1cc1e => box_corners(codepoint, canvas, metrics),
        0x1cc21..=0x1cc2f => separated_quadrants(codepoint, canvas, metrics),
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
            circle_piece(canvas, metrics, (x, y, w, h), corner);
        }
        0x1cd00..=0x1cde5 => {
            if let Some(bits) = OCTANTS.get((codepoint - 0x1cd00) as usize) {
                fill_grid(metrics, canvas, u32::from(*bits), 4);
            }
        }
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
            circle_piece(canvas, metrics, (0.0, 0.0, 1.0, 0.5), Corner::TopLeft);
            circle_piece(canvas, metrics, (0.0, 0.0, 1.0, 0.5), Corner::BottomLeft);
        }
        0x1ce0c => {
            circle_piece(canvas, metrics, (1.0, 0.0, 1.0, 0.5), Corner::TopRight);
            circle_piece(canvas, metrics, (1.0, 0.0, 1.0, 0.5), Corner::BottomRight);
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
        0x1ce51..=0x1ce8f => separated_sextants(codepoint, canvas, metrics),
        // U+1CE90–1CEAF.
        _ => {
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
    }
}

fn smooth_mosaic(pattern: [u8; 12], canvas: &mut Canvas, metrics: Metrics) {
    let m = pattern.map(|c| c == b'#');
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

fn blocks(codepoint: u32, canvas: &mut Canvas, metrics: Metrics) {
    let (width, height) = (metrics.cell_width, metrics.cell_height);
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
    let thick = metrics.box_thickness;
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

/// Diagonals between edge midpoints across the corners set in `corners`
/// (bits top-left, top-right, bottom-left, bottom-right from the lowest).
fn corner_diagonal_lines(metrics: Metrics, canvas: &mut Canvas, corners: u32) {
    let thick = f64::from(metrics.box_thickness);
    let (width, height) = (
        f64::from(metrics.cell_width),
        f64::from(metrics.cell_height),
    );
    let cx = f64::from(metrics.cell_width / 2 + metrics.cell_width % 2);
    let cy = f64::from(metrics.cell_height / 2 + metrics.cell_height % 2);
    let point = DVec2::new;
    let lines = [
        (point(cx, 0.0), point(0.0, cy)),
        (point(cx, 0.0), point(width, cy)),
        (point(cx, height), point(0.0, cy)),
        (point(cx, height), point(width, cy)),
    ];
    for (bit, (from, to)) in lines.into_iter().enumerate() {
        if corners & (1 << bit) != 0 {
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
    let thick = f64::from(metrics.box_thickness);
    for (from, to) in segments {
        let (from, to) = (position(metrics, *from), position(metrics, *to));
        canvas.line(from, to, thick, Shade::On);
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

/// The pixel position of a cell corner, edge midpoint or center.
fn position(metrics: Metrics, (horizontal, vertical): Position) -> DVec2 {
    let (width, height) = (
        f64::from(metrics.cell_width),
        f64::from(metrics.cell_height),
    );
    DVec2::new(
        match horizontal {
            Horizontal::Left => 0.0,
            Horizontal::Center => width / 2.0,
            Horizontal::Right => width,
        },
        match vertical {
            Vertical::Top => 0.0,
            Vertical::Middle => height / 2.0,
            Vertical::Bottom => height,
        },
    )
}

/// A circle of half the cell's smaller side, filled or outlined, centered on
/// a cell corner, edge midpoint or center, clipped to the cell.
fn circle(metrics: Metrics, canvas: &mut Canvas, center: Position, filled: bool) {
    canvas.clip_to_cell();
    let radius = 0.5 * f64::from(metrics.cell_width.min(metrics.cell_height));
    let stroke = f64::from(metrics.box_thickness);
    canvas.circle(
        position(metrics, center),
        radius,
        (!filled).then_some(stroke),
    );
}

/// U+1CC1B–1CC1E: box-drawing lines with a corner mark.
fn box_corners(codepoint: u32, canvas: &mut Canvas, metrics: Metrics) {
    let (w, h) = (metrics.cell_width as i32, metrics.cell_height as i32);
    let t = metrics.box_thickness as i32;
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
fn separated_quadrants(codepoint: u32, canvas: &mut Canvas, metrics: Metrics) {
    let (width, height) = (metrics.cell_width as i32, metrics.cell_height as i32);
    let bits = codepoint - 0x1cc20;
    let gap = (width / 12).max(1);
    let mid_x = gap * 2 + width % 2;
    let mid_y = gap * 2 + height % 2;
    let w = (width - gap * 2 - mid_x) / 2;
    let h = (height - gap * 2 - mid_y) / 2;
    for bit in (0..4).filter(|bit| bits & (1 << bit) != 0) {
        let x = gap + bit % 2 * (w + mid_x);
        let y = gap + bit / 2 * (h + mid_y);
        canvas.box_(x, y, x + w, y + h, Shade::On);
    }
}

/// U+1CE51–1CE8F: sextants separated by gaps.
fn separated_sextants(codepoint: u32, canvas: &mut Canvas, metrics: Metrics) {
    let (width, height) = (metrics.cell_width as i32, metrics.cell_height as i32);
    let bits = codepoint - 0x1ce50;
    let gap = (width / 12).max(1);
    let mid_x = gap * 2 + width % 2;
    let mid_y = gap * 2 + (height % 3).div_euclid(2);
    let w = (width - gap * 2 - mid_x) / 2;
    let h = (height - gap * 2 - mid_y * 2).div_euclid(3);
    let h_middle = height - gap * 2 - mid_y * 2 - h * 2;
    for bit in (0..6).filter(|bit| bits & (1 << bit) != 0) {
        let x = gap + bit % 2 * (w + mid_x);
        let (y, row_height) = match bit / 2 {
            0 => (gap, h),
            1 => (gap + h + mid_y, h_middle),
            _ => (gap + h + mid_y + h_middle + mid_y, h),
        };
        canvas.box_(x, y, x + w, y + row_height, Shade::On);
    }
}

/// A quarter of an ellipse spanning `(x, y, w, h)` cells' worth of pieces,
/// stroked in the corner `corner` and clipped to the cell.
fn circle_piece(
    canvas: &mut Canvas,
    metrics: Metrics,
    (x, y, w, h): (f64, f64, f64, f64),
    corner: Corner,
) {
    let (width, height) = (
        f64::from(metrics.cell_width),
        f64::from(metrics.cell_height),
    );
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
