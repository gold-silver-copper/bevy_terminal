//! Byte-exact dumps of fixed scenes for before/after comparisons
//! (`tools/pixel-baseline.sh`).
//!
//! For every case, each update's scene is written exactly as the main world
//! hands it over (instance bytes, draw batches, atlas uploads, clear) and the
//! CPU replay's canvas after it. Refactors that must not change output are
//! checked by running this on both revisions and comparing the files byte
//! for byte. The dump format must stay the same across revisions; the code
//! that reaches into the renderer follows its internals.

use super::replay::{Atlas, Canvas};
use super::*;
use crate::render::TerminalSizing;
use crate::scene::{StyleFlags, TerminalCell, TerminalColor, TerminalStyle};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

const ROWS: [(&str, StyleFlags); 10] = [
    (
        "ÅÉẪ Ǻǻ Z\u{302}\u{303}\u{304} q\u{307}\u{328} ṩ Ệ gjpqy",
        StyleFlags::empty(),
    ),
    (
        "|a\u{301}\u{323}| |x\u{336}| café Ångström ǟ ȫ ǭ Ǚ",
        StyleFlags::BOLD,
    ),
    (
        "Καλημέρα κόσμε — Привет, мир |مرحبا| |שלום|",
        StyleFlags::empty(),
    ),
    ("fifj WMW Wy fx ƒ @ AT ʃ & || gjpqy ÅÉ", StyleFlags::ITALIC),
    (
        "←↑→↓ ∀∂∑√∞≈≠≤≥ ◆◇○●★☆ ✔ ⚡ ☀ \u{f408} \u{e60b}",
        StyleFlags::empty(),
    ),
    (
        "|😀|🚀|❤\u{fe0f}|👍🏽|🇯🇵| |日本語|한글|",
        StyleFlags::UNDERLINED,
    ),
    (
        "┌─┬─┐ ╭╮╰╯ ⣿⡿⠿⢿ ░▒▓█▀▄▌▐▙ \u{e0b0}\u{e0b2} ━┃╋",
        StyleFlags::empty(),
    ),
    ("dim reversed hidden struck", StyleFlags::DIM),
    ("slow blink rapid blink", StyleFlags::REVERSED),
    ("gjpqy ÅÉ q\u{307}\u{328} ع ح Ẫ", StyleFlags::CROSSED_OUT),
];

fn write_row(surface: &TerminalSurface, row: u16, text: &str, style: TerminalStyle) {
    surface.update(|update| {
        let mut column = 0u16;
        for grapheme in text.graphemes(true) {
            let width = grapheme.width().max(1) as u16;
            let cell = if width > 1 {
                TerminalCell::wide(grapheme, width)
            } else {
                TerminalCell::new(grapheme)
            };
            update.set_cell((column, row), &cell.with_style(style));
            column += width;
        }
    });
}

/// Colours and styles cycling over rows, so backgrounds, the palette, true
/// colours, decorations and dimming all appear.
fn style(row: usize, flags: StyleFlags) -> TerminalStyle {
    let colors = [
        TerminalColor::Default,
        TerminalColor::Indexed(3),
        TerminalColor::Rgb(40, 200, 120),
        TerminalColor::Indexed(200),
        TerminalColor::Indexed(244),
    ];
    TerminalStyle::new()
        .fg(colors[row % colors.len()])
        .bg(colors[(row + 2) % colors.len()])
        .underline_color(colors[(row + 1) % colors.len()])
        .with(flags)
}

/// Runs one update; appends its scene to `out` and returns the canvas.
fn step(
    app: &mut App,
    entity: Entity,
    canvas: &mut Canvas,
    atlas: &mut Atlas,
    out: &mut Vec<u8>,
) -> bool {
    app.update();
    let Some(scene) = take_queued(app.world_mut(), entity) else {
        return false;
    };
    out.extend_from_slice(&(scene.instances.len() as u32).to_le_bytes());
    for instance in &scene.instances {
        let values = [instance.rect, instance.uv, instance.color];
        for value in values.as_flattened().iter().chain([&instance.background]) {
            out.extend_from_slice(&value.to_bits().to_le_bytes());
        }
    }
    out.extend_from_slice(&(scene.batches.len() as u32).to_le_bytes());
    for batch in &scene.batches {
        out.extend_from_slice(&batch.start.to_le_bytes());
        out.extend_from_slice(&batch.count.to_le_bytes());
        out.push(u8::from(batch.replace));
        out.push(u8::from(batch.texture == scene.atlas));
    }
    out.extend_from_slice(&(scene.atlas_uploads.len() as u32).to_le_bytes());
    for upload in &scene.atlas_uploads {
        for value in [
            upload.origin.x,
            upload.origin.y,
            upload.size.x,
            upload.size.y,
        ] {
            out.extend_from_slice(&value.to_le_bytes());
        }
        out.extend_from_slice(&upload.pixels);
    }
    out.push(u8::from(scene.clear));
    for value in scene.clear_color.to_linear().to_f32_array() {
        out.extend_from_slice(&value.to_bits().to_le_bytes());
    }
    if scene.destination_size != canvas.size {
        *canvas = Canvas::new(scene.destination_size);
    }
    atlas.upload(&scene);
    let images = app.world().resource::<Assets<Image>>();
    canvas.apply(&scene, atlas, images);
    true
}

#[test]
#[ignore = "writes byte-exact scene dumps; see tools/pixel-baseline.sh"]
fn pixel_baseline_dump() {
    let directory = std::env::var("BEVY_TERMINAL_BASELINE")
        .expect("BEVY_TERMINAL_BASELINE names the output directory");
    std::fs::create_dir_all(&directory).unwrap();
    let sizings = [
        ("font18", TerminalSizing::font(18.0), 1.0),
        (
            "font24-lh085-2x",
            TerminalSizing::FromFont {
                font_size: 24.0,
                line_height: 0.85,
            },
            2.0,
        ),
        (
            "fixed9x18-1.5x",
            TerminalSizing::Fixed {
                cell_size: Vec2::new(9.0, 18.0),
                font_size: 18.0,
            },
            1.5,
        ),
    ];
    for (family, _) in super::probe::BUNDLED {
        for (label, sizing, scale) in sizings {
            let name = format!("{family}-{label}");
            let replay = super::replay::Replay::new(family, (48, 12), sizing, scale);
            let (mut app, entity, surface) = (replay.app, replay.entity, replay.surface);
            let mut canvas = replay.canvas;
            let mut atlas = replay.atlas;
            let mut scenes = Vec::new();
            let mut frames = 0;
            let mut record = |app: &mut App, canvas: &mut Canvas, atlas: &mut Atlas| {
                assert!(
                    step(app, entity, canvas, atlas, &mut scenes),
                    "{name}: no scene"
                );
                let pixels: Vec<u8> = canvas.pixels.as_flattened().to_vec();
                std::fs::write(format!("{directory}/{name}-{frames}.rgba"), pixels).unwrap();
                frames += 1;
            };
            for (row, (text, flags)) in ROWS.iter().enumerate() {
                write_row(&surface, row as u16, text, style(row, *flags));
            }
            record(&mut app, &mut canvas, &mut atlas);
            // A partial repaint with a new background, a cursor, and a scroll.
            write_row(&surface, 4, "Ẫ ع partial ▓▓ 🚀", style(7, StyleFlags::BOLD));
            record(&mut app, &mut canvas, &mut atlas);
            surface.update(|update| {
                update.set_cursor_position((5, 6));
                update.set_cursor_visible(true);
            });
            record(&mut app, &mut canvas, &mut atlas);
            surface.update(|update| {
                update.scroll_up(0..12, 3);
            });
            record(&mut app, &mut canvas, &mut atlas);
            std::fs::write(format!("{directory}/{name}.scenes"), &scenes).unwrap();
        }
    }
}
