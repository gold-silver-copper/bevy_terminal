//! Shared scene and font setup for the `bevy_terminal` examples.
//!
//! Everything here writes cells directly through the neutral surface API; no
//! terminal UI library is involved.

#![allow(dead_code)]

use bevy::prelude::*;
use bevy_terminal::{
    FontFaces, StyleFlags, Terminal, TerminalCell, TerminalColor, TerminalRenderConfig,
    TerminalStyle, TerminalSurface,
};

const REGULAR: &[u8] =
    include_bytes!("../../assets/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf");
const BOLD: &[u8] = include_bytes!("../../assets/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf");
const ITALIC: &[u8] = include_bytes!("../../assets/fonts/jetbrains-mono/JetBrainsMono-Italic.ttf");
const BOLD_ITALIC: &[u8] =
    include_bytes!("../../assets/fonts/jetbrains-mono/JetBrainsMono-BoldItalic.ttf");

/// A terminal presented through a UI image node absolutely positioned at `origin`.
pub fn ui_terminal(
    surface: TerminalSurface,
    config: TerminalRenderConfig,
    origin: Vec2,
) -> impl Bundle {
    (
        Terminal::new(surface),
        config,
        ImageNode::default(),
        Node {
            position_type: PositionType::Absolute,
            left: px(origin.x),
            top: px(origin.y),
            ..default()
        },
    )
}

pub const COLUMNS: u16 = 48;
pub const ROWS: u16 = 14;
pub const CELL_SIZE: Vec2 = Vec2::new(11.0, 20.0);

/// Directory holding the Iosevka Fixed faces in the repository checkout.
const IOSEVKA_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/fonts/iosevka-fixed"
);

/// Registers four faces and applies them to `config`. Iosevka Fixed is read
/// from the repository checkout when present (it is too large to package);
/// otherwise the JetBrains Mono faces embedded in this crate are used. The
/// renderer measures the regular face and sizes it to the cell width itself.
pub fn configure_fonts(app: &mut App, mut config: TerminalRenderConfig) -> TerminalRenderConfig {
    let iosevka: Option<Vec<Vec<u8>>> = [
        "IosevkaFixed-Regular.ttf",
        "IosevkaFixed-Bold.ttf",
        "IosevkaFixed-Italic.ttf",
        "IosevkaFixed-BoldItalic.ttf",
    ]
    .iter()
    .map(|face| std::fs::read(std::path::Path::new(IOSEVKA_DIR).join(face)).ok())
    .collect();
    let faces = iosevka.unwrap_or_else(|| {
        [REGULAR, BOLD, ITALIC, BOLD_ITALIC]
            .iter()
            .map(|bytes| bytes.to_vec())
            .collect()
    });
    let mut fonts = app.world_mut().resource_mut::<Assets<Font>>();
    let mut handles = faces
        .into_iter()
        .map(|bytes| fonts.add(Font::from_bytes(bytes)));
    config.font = FontFaces {
        regular: handles.next().expect("four faces").into(),
        bold: Some(handles.next().expect("four faces").into()),
        italic: Some(handles.next().expect("four faces").into()),
        bold_italic: Some(handles.next().expect("four faces").into()),
        synthesize: true,
    };
    config
}

/// Builds a representative scene: a box, styled text, a wide glyph, colors and a cursor.
pub fn scene_surface() -> TerminalSurface {
    let surface = TerminalSurface::new((COLUMNS, ROWS));
    let mut update = surface.begin_update();

    let border = TerminalStyle::new().fg(TerminalColor::CYAN);
    draw_box(&mut update, 0, 0, COLUMNS, ROWS, border);

    let title = TerminalStyle::new()
        .fg(TerminalColor::BLACK)
        .bg(TerminalColor::LIGHT_CYAN)
        .with(StyleFlags::BOLD);
    write_text(&mut update, 2, 0, " bevy_terminal ", title);
    write_text(
        &mut update,
        18,
        0,
        " direct scene, no TUI lib ",
        TerminalStyle::new().fg(TerminalColor::LIGHT_YELLOW),
    );

    let plain = TerminalStyle::new();
    write_text(&mut update, 2, 2, "regular", plain);
    write_text(&mut update, 12, 2, "bold", plain.with(StyleFlags::BOLD));
    write_text(&mut update, 19, 2, "italic", plain.with(StyleFlags::ITALIC));
    write_text(
        &mut update,
        28,
        2,
        "bold italic",
        plain.with(StyleFlags::BOLD | StyleFlags::ITALIC),
    );

    write_text(
        &mut update,
        2,
        4,
        "underline",
        plain
            .with(StyleFlags::UNDERLINED)
            .underline_color(TerminalColor::LIGHT_RED),
    );
    write_text(
        &mut update,
        14,
        4,
        "crossed",
        plain.with(StyleFlags::CROSSED_OUT),
    );
    write_text(&mut update, 24, 4, "dim", plain.with(StyleFlags::DIM));
    write_text(
        &mut update,
        30,
        4,
        "reversed",
        plain.fg(TerminalColor::GREEN).with(StyleFlags::REVERSED),
    );

    write_text(&mut update, 2, 6, "wide:", plain);
    let wide = TerminalCell::wide("界", 2).with_style(
        plain
            .fg(TerminalColor::LIGHT_MAGENTA)
            .bg(TerminalColor::Indexed(236)),
    );
    update.set_cell((8, 6), &wide);
    update.set_cell(
        (10, 6),
        &TerminalCell::wide("😀", 2).with_style(plain.bg(TerminalColor::Indexed(236))),
    );
    write_text(&mut update, 13, 6, "| next column stays put", plain);

    for (index, column) in (2..COLUMNS - 2).step_by(3).enumerate() {
        let index = index as u8;
        let color = TerminalColor::Indexed(16 + index * 6 % 216);
        write_text(&mut update, column, 8, "██▓", plain.fg(color));
    }
    for (index, column) in (2..COLUMNS - 2).step_by(3).enumerate() {
        let level = (index * 12).min(255) as u8;
        write_text(
            &mut update,
            column,
            9,
            "▀▄▚",
            plain.fg(TerminalColor::Rgb(level, 128, 255 - level)),
        );
    }

    write_text(
        &mut update,
        2,
        11,
        "┌──┬──┐  ┏━━┳━━┓  ╔══╦══╗",
        TerminalStyle::new(),
    );
    write_text(
        &mut update,
        2,
        12,
        "└──┴──┘  ┗━━┻━━┛  ╚══╩══╝",
        TerminalStyle::new(),
    );
    write_text(
        &mut update,
        30,
        11,
        "cursor >",
        plain.fg(TerminalColor::LIGHT_GREEN),
    );
    update.set_cursor_position((39, 11));
    update.set_cursor_visible(true);
    update.commit();
    surface
}

/// A second, smaller scene demonstrating an independent surface.
pub fn status_surface() -> TerminalSurface {
    let surface = TerminalSurface::new((24, 5));
    let mut update = surface.begin_update();
    draw_box(
        &mut update,
        0,
        0,
        24,
        5,
        TerminalStyle::new().fg(TerminalColor::YELLOW),
    );
    write_text(
        &mut update,
        2,
        1,
        "second surface",
        TerminalStyle::new().with(StyleFlags::BOLD),
    );
    write_text(
        &mut update,
        2,
        2,
        "own texture + config",
        TerminalStyle::new().fg(TerminalColor::Rgb(180, 220, 255)),
    );
    write_text(
        &mut update,
        2,
        3,
        "▁▂▃▄▅▆▇█ ░▒▓█",
        TerminalStyle::new().fg(TerminalColor::LIGHT_BLUE),
    );
    update.commit();
    surface
}

/// Writes ASCII/BMP text one cell per `char`.
pub fn write_text(
    update: &mut bevy_terminal::SurfaceUpdate<'_>,
    x: u16,
    y: u16,
    text: &str,
    style: TerminalStyle,
) {
    for (offset, symbol) in text.chars().enumerate() {
        let cell = TerminalCell::from(symbol).with_style(style);
        update.set_cell((x + offset as u16, y), &cell);
    }
}

/// Draws a light box-drawing frame.
pub fn draw_box(
    update: &mut bevy_terminal::SurfaceUpdate<'_>,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
    style: TerminalStyle,
) {
    let right = x + width - 1;
    let bottom = y + height - 1;
    for column in x + 1..right {
        update.set_cell((column, y), &cell("─", style));
        update.set_cell((column, bottom), &cell("─", style));
    }
    for row in y + 1..bottom {
        update.set_cell((x, row), &cell("│", style));
        update.set_cell((right, row), &cell("│", style));
    }
    update.set_cell((x, y), &cell("┌", style));
    update.set_cell((right, y), &cell("┐", style));
    update.set_cell((x, bottom), &cell("└", style));
    update.set_cell((right, bottom), &cell("┘", style));
}

fn cell(symbol: &str, style: TerminalStyle) -> TerminalCell {
    TerminalCell::new(symbol).with_style(style)
}
