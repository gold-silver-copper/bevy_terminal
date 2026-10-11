//! Rules every sprite follows: drawing never fails at any cell size, box
//! drawing lines meet their neighbours' lines at the cell edges, blocks fill
//! their fraction of the cell, and shades are dithered dots that tile.

use super::*;
use bevy::math::{IVec2, UVec2};

fn metrics(cell_width: u32, cell_height: u32, box_thickness: u32) -> Metrics {
    Metrics {
        cell_width,
        cell_height,
        box_thickness,
    }
}

fn sprites() -> impl Iterator<Item = u32> {
    (0x2500..=0x1ceaf).filter(|codepoint| is_sprite(*codepoint))
}

/// The coverage of `codepoint` inside the cell, row by row; ink drawn
/// outside the cell is dropped.
fn cell_alpha(codepoint: u32, metrics: Metrics) -> Vec<u8> {
    let (width, height) = (metrics.cell_width, metrics.cell_height);
    let mut cell = vec![0; (width * height) as usize];
    let Some(sprite) = draw(codepoint, metrics) else {
        return cell;
    };
    for (index, alpha) in sprite.alpha.iter().enumerate() {
        let index = index as u32;
        let at =
            sprite.offset + UVec2::new(index % sprite.size.x, index / sprite.size.x).as_ivec2();
        if at.x >= 0 && at.y >= 0 && (at.x as u32) < width && (at.y as u32) < height {
            cell[(at.y as u32 * width + at.x as u32) as usize] = *alpha;
        }
    }
    cell
}

#[test]
fn every_sprite_draws_at_any_cell_size() {
    let tiny = (1..=5).flat_map(|width| (1..=5).map(move |height| (width, height, 1)));
    for (width, height, thickness) in tiny.chain([(7, 30, 2), (40, 20, 5), (96, 200, 9)]) {
        let metrics = metrics(width, height, thickness);
        for codepoint in sprites() {
            let Some(sprite) = draw(codepoint, metrics) else {
                continue;
            };
            let label = format!("U+{codepoint:04X} at {width}x{height}");
            assert_eq!(
                sprite.alpha.len(),
                (sprite.size.x * sprite.size.y) as usize,
                "{label}"
            );
            // Drawings may overshoot the cell by at most its quarter margin.
            let margin = IVec2::new(width as i32 / 4, height as i32 / 4);
            let end = sprite.offset + sprite.size.as_ivec2();
            assert!(sprite.offset.cmpge(-margin).all(), "{label}");
            assert!(
                end.cmple(IVec2::new(width as i32, height as i32) + margin)
                    .all(),
                "{label}"
            );
        }
    }
}

/// Every edge a box-drawing line reaches is crossed exactly like a straight
/// light, heavy or double line crosses it, so neighbouring cells join
/// without seams or steps.
#[test]
fn box_drawing_lines_meet_at_cell_edges() {
    let fading = 0xf5d2..=0xf5d5;
    let diagonals = 0x2571..=0x2573;
    for (width, height, thickness) in [
        (18, 36, 4),
        (12, 24, 3),
        (11, 21, 2),
        (9, 17, 1),
        (8, 15, 1),
    ] {
        let metrics = metrics(width, height, thickness);
        let inked = |codepoint: u32| -> Vec<bool> {
            let alpha = cell_alpha(codepoint, metrics);
            alpha.iter().map(|alpha| *alpha >= 128).collect()
        };
        let column = |ink: &[bool], x: u32| -> Vec<bool> {
            (0..height).map(|y| ink[(y * width + x) as usize]).collect()
        };
        let row = |ink: &[bool], y: u32| -> Vec<bool> {
            (0..width).map(|x| ink[(y * width + x) as usize]).collect()
        };
        // ─ ━ ═ and │ ┃ ║: straight lines continue across their own edges.
        let across: Vec<Vec<bool>> = [0x2500, 0x2501, 0x2550]
            .map(|codepoint| {
                let ink = inked(codepoint);
                assert_eq!(column(&ink, 0), column(&ink, width - 1));
                column(&ink, 0)
            })
            .into();
        let down: Vec<Vec<bool>> = [0x2502, 0x2503, 0x2551]
            .map(|codepoint| {
                let ink = inked(codepoint);
                assert_eq!(row(&ink, 0), row(&ink, height - 1));
                row(&ink, 0)
            })
            .into();
        // Light lines are the box thickness, heavy ones twice that.
        let count = |line: &[bool]| line.iter().filter(|inked| **inked).count() as u32;
        assert_eq!(
            (count(&across[0]), count(&across[1])),
            (thickness, 2 * thickness)
        );
        assert_eq!(
            (count(&down[0]), count(&down[1])),
            (thickness, 2 * thickness)
        );

        let lines = (0x2500..=0x257f).chain([0x1fbaf]).chain(0xf5d0..=0xf5ed);
        for codepoint in lines.filter(|c| !fading.contains(c) && !diagonals.contains(c)) {
            let ink = inked(codepoint);
            let edges = [
                (column(&ink, 0), &across),
                (column(&ink, width - 1), &across),
                (row(&ink, 0), &down),
                (row(&ink, height - 1), &down),
            ];
            for (edge, (crossing, straight)) in ["left", "right", "top", "bottom"].iter().zip(edges)
            {
                assert!(
                    crossing.iter().all(|inked| !inked) || straight.contains(&crossing),
                    "U+{codepoint:04X} at {width}x{height}+{thickness}: {edge} edge {crossing:?}"
                );
            }
        }
    }
}

/// Eighth blocks fill whole rows from the bottom or columns from the left.
#[test]
fn eighth_blocks_fill_their_fraction() {
    for (width, height) in [(9, 17), (10, 20), (11, 21), (3, 5)] {
        let metrics = metrics(width, height, 1);
        let eighths = |n: u32, size: u32| (f64::from(n * size) / 8.0).round() as u32;
        for n in 1..=8 {
            let lower = cell_alpha(0x2580 + n, metrics);
            let left = cell_alpha(0x2590 - n, metrics);
            for y in 0..height {
                for x in 0..width {
                    let index = (y * width + x) as usize;
                    let in_lower = y >= height - eighths(n, height);
                    assert_eq!(lower[index] == 255, in_lower, "U+{:04X}", 0x2580 + n);
                    assert_eq!(
                        left[index] == 255,
                        x < eighths(n, width),
                        "U+{:04X}",
                        0x2590 - n
                    );
                }
            }
        }
    }
}

/// Separated quadrants and sextants keep a gap at every edge of the cell,
/// however small, so neighbouring mosaics stay apart.
#[test]
fn separated_mosaics_keep_their_gaps() {
    for width in 1..=12 {
        for height in 1..=24 {
            let metrics = metrics(width, height, 1);
            for codepoint in (0x1cc21..=0x1cc2f).chain(0x1ce51..=0x1ce8f) {
                let alpha = cell_alpha(codepoint, metrics);
                let edge = |index: usize| {
                    let (x, y) = (index as u32 % width, index as u32 / width);
                    x == 0 || y == 0 || x == width - 1 || y == height - 1
                };
                assert!(
                    alpha
                        .iter()
                        .enumerate()
                        .all(|(index, alpha)| *alpha == 0 || !edge(index)),
                    "U+{codepoint:04X} at {width}x{height}"
                );
            }
        }
    }
}

/// The scene draws `solid_block` rectangles as solid quads instead of the
/// sprite: they must cover exactly the pixels the sprite fills, and exist
/// only for blocks the sprite draws as one opaque rectangle.
#[test]
fn solid_blocks_are_exactly_the_single_rectangle_sprites() {
    for (width, height) in [(9, 17), (10, 20), (11, 21)] {
        let metrics = Metrics {
            cell_width: width,
            cell_height: height,
            box_thickness: 1,
        };
        for codepoint in 0x2580..=0x259f {
            let sprite = draw(codepoint, metrics).expect("block element");
            let inked = |x: i32, y: i32| {
                let (x, y) = (x - sprite.offset.x, y - sprite.offset.y);
                x >= 0
                    && y >= 0
                    && (x as u32) < sprite.size.x
                    && (y as u32) < sprite.size.y
                    && sprite.alpha[(y as u32 * sprite.size.x + x as u32) as usize] > 0
            };
            let opaque = sprite
                .alpha
                .iter()
                .all(|alpha| *alpha == 0 || *alpha == 255);
            match solid_block(codepoint, metrics) {
                Some([x0, y0, x1, y1]) => {
                    for y in -(height as i32)..2 * height as i32 {
                        for x in -(width as i32)..2 * width as i32 {
                            let inside = x >= x0 && x < x1 && y >= y0 && y < y1;
                            assert_eq!(inked(x, y), inside, "U+{codepoint:04X} at ({x},{y})");
                        }
                    }
                    assert!(opaque, "U+{codepoint:04X}");
                }
                // Shades are dithered; combined quadrants are unions.
                None => assert!(
                    matches!(codepoint, 0x2591..=0x2593 | 0x2599..=0x259c | 0x259e | 0x259f),
                    "U+{codepoint:04X}"
                ),
            }
        }
    }
}

/// The inked pixels of `codepoint` drawn in one `width` × `height` cell.
fn ink(codepoint: u32, width: u32, height: u32) -> Vec<bool> {
    let metrics = Metrics {
        cell_width: width,
        cell_height: height,
        box_thickness: 1,
    };
    let sprite = draw(codepoint, metrics).expect("shade");
    assert!(
        sprite
            .alpha
            .iter()
            .all(|alpha| *alpha == 0 || *alpha == 255),
        "U+{codepoint:04X} dots are opaque"
    );
    let mut ink = vec![false; (width * height) as usize];
    for y in 0..sprite.size.y {
        for x in 0..sprite.size.x {
            let cell = sprite.offset + IVec2::new(x as i32, y as i32);
            assert!(
                cell.x >= 0 && cell.y >= 0,
                "U+{codepoint:04X} stays in the cell"
            );
            assert!((cell.x as u32) < width && (cell.y as u32) < height);
            ink[(cell.y as u32 * width + cell.x as u32) as usize] =
                sprite.alpha[(y * sprite.size.x + x) as usize] == 255;
        }
    }
    ink
}

const SHADE_CELLS: [(u32, u32); 6] = [(8, 16), (9, 17), (11, 21), (14, 31), (18, 36), (28, 62)];

#[test]
fn shades_are_opaque_dots_at_their_density() {
    for (width, height) in SHADE_CELLS {
        for (codepoint, density) in [(0x2591, 0.25), (0x2592, 0.5), (0x2593, 0.75)] {
            let ink = ink(codepoint, width, height);
            let inked = ink.iter().filter(|inked| **inked).count() as f64 / ink.len() as f64;
            assert!(
                (inked - density).abs() <= 0.06,
                "U+{codepoint:04X} at {width}x{height}: {inked:.3} inked, expected {density}"
            );
        }
        // Light and dark shades are each other's negative.
        let light = ink(0x2591, width, height);
        let dark = ink(0x2593, width, height);
        assert!(
            light.iter().zip(&dark).all(|(l, d)| l != d),
            "{width}x{height}"
        );
    }
}

/// Every pattern period fits the cell a whole number of times, so the
/// checkerboard keeps alternating across the edges between cells.
#[test]
fn shades_tile_across_cells() {
    for (width, height) in SHADE_CELLS {
        let ink = ink(0x2592, width, height);
        let at = |x: u32, y: u32| ink[(y * width + x) as usize];
        for y in 0..height {
            assert_ne!(at(width - 1, y), at(0, y), "{width}x{height} row {y}");
        }
        for x in 0..width {
            assert_ne!(at(x, height - 1), at(x, 0), "{width}x{height} column {x}");
        }
    }
}

/// The legacy-computing medium shades are parts of `▒`, dot for dot.
#[test]
fn legacy_medium_shades_match_the_medium_shade() {
    for (width, height) in SHADE_CELLS {
        let medium = ink(0x2592, width, height);
        // Halves round as `block` rounds them.
        let half = |size: u32| (f64::from(size) / 2.0).round() as u32;
        let (half_width, half_height) = (half(width), half(height));
        for (codepoint, inside) in [
            (
                0x1fb8c,
                Box::new(move |x: u32, _| x < half_width) as Box<dyn Fn(u32, u32) -> bool>,
            ),
            (0x1fb8d, Box::new(move |x: u32, _| x >= width - half_width)),
            (0x1fb8e, Box::new(move |_, y: u32| y < half_height)),
            (
                0x1fb8f,
                Box::new(move |_, y: u32| y >= height - half_height),
            ),
            (0x1fb90, Box::new(|_, _| true)),
        ] {
            let ink = ink(codepoint, width, height);
            for y in 0..height {
                for x in 0..width {
                    let index = (y * width + x) as usize;
                    let expected = inside(x, y) && medium[index];
                    assert_eq!(
                        ink[index], expected,
                        "U+{codepoint:04X} at ({x},{y}) in {width}x{height}"
                    );
                }
            }
        }
    }
}
