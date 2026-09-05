//! Headless GPU tests: render a terminal and read the texture back.
//!
//! These need a GPU (Metal/Vulkan/DX12) and are ignored by default; run them
//! with `cargo test -p bevy_terminal --test gpu_readback -- --ignored`.

use std::sync::{Arc, Mutex};

use bevy::{
    app::ScheduleRunnerPlugin,
    prelude::*,
    render::{
        RenderPlugin,
        gpu_readback::{Readback, ReadbackComplete},
        settings::RenderCreation,
    },
    winit::WinitPlugin,
};
use bevy_terminal::prelude::*;

#[derive(Resource, Clone)]
struct Captured(Arc<Mutex<Option<Capture>>>);

type Capture = (Vec<u8>, UVec2);

/// Renders `surface` headlessly with `config` and returns the texture's RGBA8
/// bytes (sRGB-encoded, straight alpha) and its size.
fn render_headless(surface: TerminalSurface, config: TerminalRenderConfig) -> (Vec<u8>, UVec2) {
    render_headless_with(surface, move |_| config)
}

/// Like [`render_headless`], but builds the configuration inside the app so it
/// can reference font assets registered there.
fn render_headless_with(
    surface: TerminalSurface,
    config: impl FnOnce(&mut Assets<Font>) -> TerminalRenderConfig + Send + Sync + 'static,
) -> (Vec<u8>, UVec2) {
    let mut config = Some(config);
    let captured = Captured(Arc::new(Mutex::new(None)));
    let sink = captured.clone();
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                close_when_requested: false,
                ..default()
            })
            .set(RenderPlugin {
                render_creation: RenderCreation::Automatic(Box::default()),
                synchronous_pipeline_compilation: true,
                ..default()
            })
            .disable::<WinitPlugin>(),
    )
    .add_plugins((
        ScheduleRunnerPlugin::run_loop(std::time::Duration::from_millis(1)),
        TerminalPlugin,
    ))
    .insert_resource(captured)
    .add_systems(
        Startup,
        move |mut commands: Commands, mut fonts: ResMut<Assets<Font>>| {
            let config = config.take().expect("startup runs once")(&mut fonts);
            commands.spawn((Terminal::new(surface.clone()), config));
        },
    )
    .add_observer(
        |ready: On<TerminalReady>, mut commands: Commands, textures: Query<&TerminalTexture>| {
            let texture = textures.get(ready.entity).unwrap();
            commands
                .spawn(Readback::texture(texture.image.clone()))
                .observe(
                    |done: On<ReadbackComplete>,
                     sink: Res<Captured>,
                     textures: Query<&TerminalTexture>,
                     mut frames: Local<u32>,
                     mut exit: MessageWriter<AppExit>| {
                        // Skip the first readbacks: the scene reaches the GPU a couple of
                        // frames after the texture exists.
                        *frames += 1;
                        if *frames < 6 {
                            return;
                        }
                        let size = textures.single().map(|t| t.size).unwrap_or_default();
                        *sink.0.lock().unwrap() = Some((done.data.clone(), size));
                        exit.write(AppExit::Success);
                    },
                );
        },
    );
    app.run();
    let captured = sink.0.lock().unwrap().take();
    captured.expect("a readback completed")
}

fn texel(data: &[u8], size: UVec2, x: u32, y: u32) -> [u8; 4] {
    // Readback rows may be padded to 256 bytes.
    let unpadded = size.x as usize * 4;
    let stride = data.len() / size.y as usize;
    assert!(stride >= unpadded);
    let start = y as usize * stride + x as usize * 4;
    [
        data[start],
        data[start + 1],
        data[start + 2],
        data[start + 3],
    ]
}

#[test]
#[ignore = "requires a GPU"]
fn translucent_background_and_srgb_texture() {
    let surface = TerminalSurface::new((4, 2));
    surface.update(|u| {
        u.set_cell((0, 0), &TerminalCell::new("█"));
        u.set_cell(
            (1, 0),
            &TerminalCell::new("█")
                .with_style(TerminalStyle::new().fg(TerminalColor::Rgb(255, 0, 0))),
        );
    });
    let theme = TerminalTheme {
        background: Color::srgba(0.0, 0.0, 0.0, 0.5),
        ..default()
    };
    let config = TerminalRenderConfig {
        cell_size: Vec2::new(10.0, 20.0).into(),
        font_size: FontSizing::Px(18.0),
        theme,
        cursor: CursorConfig {
            blink_hz: None,
            ..default()
        },
        blink: BlinkConfig::NONE,
        ..default()
    };
    let (data, size) = render_headless(surface, config);
    assert_eq!(size, UVec2::new(40, 40));
    // An empty cell shows the translucent background.
    let empty = texel(&data, size, 35, 30);
    assert!(
        (empty[3] as i32 - 128).abs() <= 2,
        "empty cell alpha {empty:?}"
    );
    // A full block is opaque, and red stays red through the sRGB encoding.
    let block = texel(&data, size, 5, 10);
    assert_eq!(block[3], 255, "block alpha {block:?}");
    let red = texel(&data, size, 15, 10);
    assert!(
        red[0] > 240 && red[1] < 16 && red[2] < 16 && red[3] == 255,
        "red block {red:?}"
    );
}

/// Returns the inked columns (alpha above half) of the texture row `y` within
/// the cell column `[x0, x0 + width)`.
fn inked_columns(data: &[u8], size: UVec2, y: u32, x0: u32, width: u32) -> Vec<u32> {
    (x0..x0 + width)
        .filter(|&x| texel(data, size, x, y)[3] > 128 && texel(data, size, x, y)[0] > 128)
        .collect()
}

/// JetBrains Mono draws its box-drawing bars 20 units past the 600-unit
/// advance so joins overlap. That overshoot must not push `┌` sideways: its
/// stem has to land on the same columns as the `│` below it.
#[test]
#[ignore = "requires a GPU"]
fn box_drawing_overshoot_keeps_stems_aligned() {
    let surface = TerminalSurface::new((3, 2));
    surface.update(|u| {
        u.set_cell((0, 0), &TerminalCell::new("┌"));
        u.set_cell((1, 0), &TerminalCell::new("─"));
        u.set_cell((2, 0), &TerminalCell::new("┐"));
        u.set_cell((0, 1), &TerminalCell::new("│"));
        u.set_cell((2, 1), &TerminalCell::new("│"));
    });
    let (data, size) = render_headless_with(surface, |fonts| {
        let bytes =
            include_bytes!("../../../assets/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf");
        let handle = fonts.add(Font::from_bytes(bytes.to_vec()));
        TerminalRenderConfig {
            cell_size: CellSizing::FROM_FONT,
            font: FontFaces::regular(FontSource::Handle(handle)),
            font_size: FontSizing::Px(24.0),
            theme: TerminalTheme {
                foreground: Color::WHITE,
                background: Color::BLACK,
                ..default()
            },
            cursor: CursorConfig {
                blink_hz: None,
                ..default()
            },
            blink: BlinkConfig::NONE,
            ..default()
        }
    });
    let cell_w = size.x / 3;
    let cell_h = size.y / 2;
    // Scan a row below the corner's bar (the stem only) and a row in the
    // middle of the `│` cell.
    let corner_row = cell_h * 3 / 4;
    let stem_row = cell_h + cell_h / 2;
    for cell in [0, 2] {
        let x0 = cell * cell_w;
        let corner = inked_columns(&data, size, corner_row, x0, cell_w);
        let stem = inked_columns(&data, size, stem_row, x0, cell_w);
        assert!(
            !corner.is_empty() && !stem.is_empty(),
            "no ink in cell {cell}"
        );
        assert_eq!(
            corner, stem,
            "corner stem columns {corner:?} differ from `│` columns {stem:?} in cell {cell}"
        );
    }
}

/// Builds the font-driven config used by the metrics tests.
fn font_driven_config(handle: Handle<Font>, font_size: f32) -> TerminalRenderConfig {
    TerminalRenderConfig {
        cell_size: CellSizing::FROM_FONT,
        font: FontFaces::regular(FontSource::Handle(handle)),
        font_size: FontSizing::Px(font_size),
        theme: TerminalTheme {
            foreground: Color::WHITE,
            background: Color::BLACK,
            ..default()
        },
        cursor: CursorConfig {
            blink_hz: None,
            ..default()
        },
        blink: BlinkConfig::NONE,
        ..default()
    }
}

/// DejaVu Sans Mono (like Menlo, which derives from it) draws its full block
/// 1.02 em tall inside a 1.164 em line box. Sizing the cell from the block
/// collapsed rows onto each other; the cell must be at least the line box.
#[test]
#[ignore = "requires a GPU"]
fn font_driven_cell_is_at_least_the_font_line_box() {
    let surface = TerminalSurface::new((2, 2));
    surface.update(|u| {
        u.set_cell((0, 0), &TerminalCell::new("A"));
        u.set_cell((1, 0), &TerminalCell::new("g"));
        u.set_cell((0, 1), &TerminalCell::new("A"));
        u.set_cell((1, 1), &TerminalCell::new("g"));
    });
    let (data, size) = render_headless_with(surface, |fonts| {
        let bytes = include_bytes!("../../../assets/fonts/dejavu-sans-mono/DejaVuSansMono.ttf");
        font_driven_config(fonts.add(Font::from_bytes(bytes.to_vec())), 24.0)
    });
    // 24px * (1901 + 483) / 2048 = 27.9px, so rows are at least 28px tall.
    assert!(
        size.y / 2 >= 28,
        "cell height {} is below the line box",
        size.y / 2
    );
    // The rows do not touch: at least one blank raster row separates the
    // descender of `g` from the cap of the `A` below it.
    let cell_h = size.y / 2;
    let blank_rows = (cell_h.saturating_sub(4)..cell_h + 4)
        .filter(|&y| (0..size.x).all(|x| texel(&data, size, x, y)[0] < 32))
        .count();
    assert!(blank_rows >= 1, "no blank row between the two text rows");
}

/// Block elements are drawn from geometry, so a font whose `█` is shorter than
/// its line box still tiles a column of blocks without a seam, and halves meet
/// exactly at the cell's midline.
#[test]
#[ignore = "requires a GPU"]
fn block_elements_tile_the_cell_from_geometry() {
    let surface = TerminalSurface::new((3, 2));
    surface.update(|u| {
        u.set_cell((0, 0), &TerminalCell::new("█"));
        u.set_cell((0, 1), &TerminalCell::new("█"));
        u.set_cell((1, 0), &TerminalCell::new("▄"));
        u.set_cell((1, 1), &TerminalCell::new("▀"));
        u.set_cell((2, 0), &TerminalCell::new("▐"));
        u.set_cell((2, 1), &TerminalCell::new("▌"));
    });
    let (data, size) = render_headless_with(surface, |fonts| {
        let bytes = include_bytes!("../../../assets/fonts/dejavu-sans-mono/DejaVuSansMono.ttf");
        font_driven_config(fonts.add(Font::from_bytes(bytes.to_vec())), 24.0)
    });
    let cell_w = size.x / 3;
    let cell_h = size.y / 2;
    let opaque = |x: u32, y: u32| texel(&data, size, x, y)[0] > 240;
    // Column 0: every row of both stacked blocks is opaque.
    for y in 0..size.y {
        assert!(opaque(cell_w / 2, y), "seam in the block column at row {y}");
    }
    // Column 1: lower half of row 0 and upper half of row 1 are opaque, the
    // rest dark, so the two halves form one seamless block around the cell
    // boundary.
    let x = cell_w + cell_w / 2;
    for y in 0..size.y {
        let expect = (y >= cell_h / 2) && (y < cell_h + cell_h / 2);
        assert_eq!(opaque(x, y), expect, "half block mismatch at row {y}");
    }
    // Column 2: right half above left half, split at the cell's midline.
    let y = cell_h / 2;
    assert!(!opaque(2 * cell_w + cell_w / 4, y) && opaque(2 * cell_w + 3 * cell_w / 4, y));
    let y = cell_h + cell_h / 2;
    assert!(opaque(2 * cell_w + cell_w / 4, y) && !opaque(2 * cell_w + 3 * cell_w / 4, y));
}

/// `FromFont { line_height }` scales the font's line box: DejaVu Sans Mono's
/// 27.06 px box at 23.25 px becomes 22 rows at 0.8 and 34 rows at 1.25.
#[test]
#[ignore = "requires a GPU"]
fn line_height_scales_the_font_driven_cell() {
    for (line_height, expected_rows) in [(0.8_f32, 22_u32), (1.0, 28), (1.25, 34)] {
        let surface = TerminalSurface::new((2, 2));
        surface.update(|u| {
            u.set_cell((0, 0), &TerminalCell::new("A"));
            u.set_cell((1, 1), &TerminalCell::new("█"));
        });
        let (data, size) = render_headless_with(surface, move |fonts| {
            let bytes = include_bytes!("../../../assets/fonts/dejavu-sans-mono/DejaVuSansMono.ttf");
            let mut config = font_driven_config(fonts.add(Font::from_bytes(bytes.to_vec())), 24.0);
            config.cell_size = CellSizing::FromFont { line_height };
            config
        });
        assert_eq!(size.y / 2, expected_rows, "line_height {line_height}");
        // The block still fills its whole (shorter or taller) cell.
        let cell_w = size.x / 2;
        let cell_h = size.y / 2;
        for y in cell_h..size.y {
            assert!(
                texel(&data, size, cell_w + cell_w / 2, y)[0] > 240,
                "block row {y} not filled at line_height {line_height}"
            );
        }
    }
}
