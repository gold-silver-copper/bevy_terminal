//! Compares every sprite with Ghostty's reference atlases
//! (`src/font/sprite/testdata`, copied under `testdata/` with Ghostty's MIT
//! license). Each atlas holds 256 codepoints in a 16 × 16 grid of canvases
//! including their quarter-cell margins, drawn at four sets of metrics.
//!
//! Rectangle-built glyphs must match exactly. Anti-aliased paths come from a
//! different rasterizer (z2d in Ghostty), so their edges may differ by a
//! bounded amount while their shape must agree. Shades are dithered instead
//! of Ghostty's uniform translucency and are checked on their own.

use super::*;
use bevy::math::{IVec2, UVec2};

/// (cell width, ascent, descent, box thickness), as Ghostty's test draws them.
const METRICS: [(u32, u32, u32, u32); 4] = [
    (18, 30, 6, 4),
    (12, 20, 4, 3),
    (11, 19, 2, 2),
    (9, 15, 2, 1),
];

/// Whether Ghostty draws `codepoint` with anti-aliased paths.
fn uses_paths(codepoint: u32) -> bool {
    matches!(
        codepoint,
        0x256d..=0x2573
            | 0x25e2..=0x25ff
            | 0xe0b0..=0xe0d4
            | 0xf5d6..=0xf5ed
            | 0xf5ee..=0xf60d
            | 0x1fb3c..=0x1fb6f
            | 0x1fb98..=0x1fbae
            | 0x1fbbd..=0x1fbbf
            | 0x1fbd0..=0x1fbe3
            | 0x1fbe8..=0x1fbef
            | 0x1cc30..=0x1cc3f
            | 0x1ce00..=0x1ce0c
    )
}

/// Whether `codepoint` is a shade that deliberately differs from Ghostty.
fn dithered(codepoint: u32) -> bool {
    matches!(codepoint, 0x2591..=0x2593)
}

fn reference(block: u32, (width, height, thickness): (u32, u32, u32)) -> Option<(u32, Vec<u8>)> {
    let path = format!(
        "{}/src/render/batch/sprite/testdata/U+{block:X}...U+{:X}-{width}x{height}+{thickness}.png",
        env!("CARGO_MANIFEST_DIR"),
        block + 0xff
    );
    let file = std::fs::File::open(path).ok()?;
    let mut reader = png::Decoder::new(std::io::BufReader::new(file))
        .read_info()
        .unwrap();
    let mut buffer = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buffer).unwrap();
    let channels = info.color_type.samples();
    let gray = buffer[..info.buffer_size()]
        .iter()
        .step_by(channels)
        .copied()
        .collect();
    Some((info.width, gray))
}

#[test]
fn sprites_match_ghostty_reference_atlases() {
    let mut exact_failures = Vec::new();
    let mut path_failures = Vec::new();
    let mut compared = 0;
    for (width, ascent, descent, thickness) in METRICS {
        let height = ascent + descent;
        let metrics = Metrics {
            cell_width: width,
            cell_height: height,
            box_thickness: thickness,
        };
        let padding = UVec2::new(width / 4, height / 4);
        let stride = UVec2::new(width, height) + 2 * padding;
        let mut blocks: Vec<u32> = (0x2500..=0x1ceaf_u32)
            .filter(|cp| is_sprite(*cp))
            .map(|cp| cp & !0xff)
            .collect();
        blocks.dedup();
        for block in blocks {
            let (atlas_width, expected) = reference(block, (width, height, thickness))
                .unwrap_or_else(|| panic!("reference atlas for U+{block:X}"));
            for codepoint in (block..block + 0x100).filter(|cp| is_sprite(*cp) && !dithered(*cp)) {
                compared += 1;
                let index = codepoint - block;
                let tile = UVec2::new(index % 16, index / 16) * stride;
                let mut actual = vec![0u8; (stride.x * stride.y) as usize];
                if let Some(sprite) = draw(codepoint, width, height, metrics) {
                    let origin = sprite.offset + padding.as_ivec2();
                    for y in 0..sprite.size.y {
                        for x in 0..sprite.size.x {
                            let p = origin + IVec2::new(x as i32, y as i32);
                            actual[(p.y as u32 * stride.x + p.x as u32) as usize] =
                                sprite.alpha[(y * sprite.size.x + x) as usize];
                        }
                    }
                }
                let expected: Vec<u8> = (0..stride.y)
                    .flat_map(|y| {
                        let start = ((tile.y + y) * atlas_width + tile.x) as usize;
                        expected[start..start + stride.x as usize].iter().copied()
                    })
                    .collect();
                let differences: Vec<u8> = expected
                    .iter()
                    .zip(&actual)
                    .map(|(e, a)| e.abs_diff(*a))
                    .collect();
                let worst = differences.iter().copied().max().unwrap_or(0);
                let total: u32 = differences.iter().map(|d| u32::from(*d)).sum();
                let ink: u32 = expected.iter().map(|e| u32::from(*e)).sum::<u32>().max(255);
                let label = format!("U+{codepoint:X} at {width}x{height}+{thickness}");
                if uses_paths(codepoint) {
                    // Edges may differ; the silhouette may not: at most a
                    // tenth of the ink (one-pixel strokes are mostly edge),
                    // and hardly any pixel flipped between on and off.
                    let flipped = expected
                        .iter()
                        .zip(&actual)
                        .filter(|(e, a)| (**e >= 224 && **a < 32) || (**e < 32 && **a >= 224))
                        .count();
                    if total * 10 > ink || flipped > (stride.x + stride.y) as usize / 8 {
                        path_failures.push(format!(
                            "{label}: {total} total difference over {ink} ink, {flipped} flipped"
                        ));
                    }
                } else if worst != 0 {
                    let first = differences.iter().position(|d| *d != 0).unwrap() as u32;
                    exact_failures.push(format!(
                        "{label}: {} pixels differ, first at ({},{})",
                        differences.iter().filter(|d| **d != 0).count(),
                        first % stride.x,
                        first / stride.x
                    ));
                }
            }
        }
    }
    assert_eq!(compared, 4 * 869, "every undithered sprite at every size");
    assert!(
        exact_failures.is_empty(),
        "{} exact failures:\n{}",
        exact_failures.len(),
        exact_failures.join("\n")
    );
    assert!(
        path_failures.is_empty(),
        "{} path failures:\n{}",
        path_failures.len(),
        path_failures.join("\n")
    );
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
            let sprite = draw(codepoint, width, height, metrics).expect("block element");
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
    let sprite = draw(codepoint, width, height, metrics).expect("shade");
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
