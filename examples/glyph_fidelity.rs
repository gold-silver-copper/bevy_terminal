//! Glyph fidelity harness: compares rendered text with unclipped font bitmaps
//! and checks explicit cropping and block/box continuity.
//!
//! Every cell sits on a contrasting checkerboard background so a clipped or
//! bleeding pixel is visible against its neighbor, and every row group is
//! fenced by a guard column of `│` on both sides. Row groups: full printable
//! ASCII in the four faces; Latin-1 Supplement and Latin Extended-A; Greek and
//! Cyrillic; combining-mark stacks; all of box drawing U+2500–257F; block
//! elements U+2580–259F with braille, geometric shapes and arrows; a wide
//! CJK/emoji/full-width row; and tile panels (solid blocks, half blocks across
//! cell boundaries, shades, single/heavy/double lines) where a seam shows as
//! a line.
//!
//! Flags:
//! - `--font <index|dir|all>` selects the vendored family (see `render_test`),
//!   `all` renders every family;
//! - `--scale <f|all>` selects the raster scale (`all` = 1, 1.5, 2, 3);
//! - `--from-font <px>` derives the cell from that logical font size instead
//!   of fitting the font to the fixed 11×20 logical-pixel test cell;
//! - `--tiles-only` limits `--check` to the block/box seam panels;
//! - `--export` writes PNGs to `target/glyph-fidelity/<family>/<scale>x/`
//!   headlessly;
//! - `--fixed-cell <width>x<height>` with `--font-size <px>` selects explicit
//!   geometry; `--line-height <ratio>` configures `--from-font` cells;
//! - `--output <dir>` selects the check results and diagnostic image directory;
//! - `--check` uses only bundled fonts and a separate raw Bevy glyph-atlas
//!   reference. Every content cell of a checked row must match (within a few
//!   sRGB code values of blend rounding) the row the oracle composes from
//!   Ghostty's rules:
//!   ordinary text at its rasterized size on the terminal's shared baseline,
//!   symbols scaled down only as needed to fit the cells they may occupy, ink
//!   confined to its row, wider runs overflowing their neighbours. Missing font
//!   coverage is recorded separately. Solid, half-block, and line tiles are
//!   strict. Results are TSV plus native and 8x diagnostic PNGs, by default
//!   under `target/glyph-fidelity-check`.
//!
//! Without `--export`/`--check` a window shows one family; `Space`/`Tab`
//! cycle families.

#[allow(dead_code)]
mod common;
#[allow(dead_code)]
#[path = "common/fidelity_oracle.rs"]
mod fidelity_oracle;

use std::sync::{Arc, Mutex};

use bevy::{
    app::ScheduleRunnerPlugin,
    prelude::*,
    render::{
        RenderPlugin,
        gpu_readback::{Readback, ReadbackComplete},
    },
    window::{PrimaryWindow, WindowResolution},
    winit::WinitPlugin,
};
use bevy_image_export::{ImageExport, ImageExportPlugin, ImageExportSettings, ImageExportSource};
use bevy_terminal_ratatui::prelude::{
    CursorConfig, FontFaces, RasterConfig, TerminalPlugin, TerminalRenderConfig, TerminalSizing,
    TerminalSnapshot, TerminalSystems, TerminalTexture,
};
use bevy_terminal_ratatui::{RatatuiTerminal, TerminalRenderer};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

/// (display name, directory, regular, bold, italic, bold-italic).
const FAMILIES: [(&str, &str, &str, &str, &str, &str); 6] = [
    (
        "Iosevka Fixed",
        "iosevka-fixed",
        "IosevkaFixed-Regular.ttf",
        "IosevkaFixed-Bold.ttf",
        "IosevkaFixed-Italic.ttf",
        "IosevkaFixed-BoldItalic.ttf",
    ),
    (
        "JetBrains Mono",
        "jetbrains-mono",
        "JetBrainsMono-Regular.ttf",
        "JetBrainsMono-Bold.ttf",
        "JetBrainsMono-Italic.ttf",
        "JetBrainsMono-BoldItalic.ttf",
    ),
    (
        "Cascadia Mono",
        "cascadia-mono",
        "CascadiaMono-Regular.ttf",
        "CascadiaMono-Bold.ttf",
        "CascadiaMono-Italic.ttf",
        "CascadiaMono-BoldItalic.ttf",
    ),
    (
        "Hack",
        "hack",
        "Hack-Regular.ttf",
        "Hack-Bold.ttf",
        "Hack-Italic.ttf",
        "Hack-BoldItalic.ttf",
    ),
    (
        "DejaVu Sans Mono",
        "dejavu-sans-mono",
        "DejaVuSansMono.ttf",
        "DejaVuSansMono-Bold.ttf",
        "DejaVuSansMono-Oblique.ttf",
        "DejaVuSansMono-BoldOblique.ttf",
    ),
    (
        "Source Code Pro",
        "source-code-pro",
        "SourceCodePro-Regular.ttf",
        "SourceCodePro-Bold.ttf",
        "SourceCodePro-It.ttf",
        "SourceCodePro-BoldIt.ttf",
    ),
];

const SCALES: [f32; 4] = [1.0, 1.5, 2.0, 3.0];

/// Guard columns sit at 0 and `COLUMNS - 1`; content spans 1..=96.
const COLUMNS: u16 = 98;
const ROWS: u16 = 30;
const CONTENT: u16 = COLUMNS - 2;
const CELL: Vec2 = Vec2::new(11.0, 20.0);
const MARGIN: f32 = 12.0;

/// Checkerboard backgrounds (must both contrast with white ink and each other).
const CHECKER: [Color; 2] = [Color::Rgb(40, 44, 64), Color::Rgb(84, 56, 44)];
const INK: Color = Color::White;
const INK_RGB: [u8; 3] = [255, 255, 255];

/// Rows and their groups. Text groups use raw bitmap comparison; tiles use geometry checks.
const ROW_TITLE: u16 = 0;
const ROWS_ASCII: [u16; 4] = [2, 3, 4, 5];
const ROWS_LATIN: [u16; 2] = [7, 8];
const ROW_GREEK: u16 = 9;
const ROW_CYRILLIC: u16 = 10;
const ROW_MARKS: u16 = 11;
const ROWS_BOX: [u16; 2] = [13, 14];
const ROW_BLOCKS: u16 = 15;
const ROW_ARROWS: u16 = 16;
const ROW_WIDE: u16 = 18;
const TILE_ROWS_A: u16 = 20;
const TILE_ROWS_B: u16 = 25;
const TILE_WIDTH: u16 = 8;
const TILE_HEIGHT: u16 = 4;

const LATIN_1: &str = "ÀÁÂÃÄÅÆÇÈÉÊËÌÍÎÏÐÑÒÓÔÕÖØÙÚÛÜÝÞß àáâãäåæçèéêëìíîïðñòóôõöøùúûüýþÿ ĀāĂăĄąĆćĈĉĊċČčĎďĐđĒēĔĕĖėĘęĚěĜĝ";
const LATIN_EXT: &str = "ĠġĢģĤĥĦħĨĩĪīĬĭĮįİıĲĳĴĵĶķĸĹĺĻļĽľĿŀŁłŃńŅņŇňŉŊŋŌōŎŏŐőŒœŔŕŖŗŘřŚśŜŝŞşŠšŢţŤťŦŧŨũŪūŬŭŮůŰűŲųŴŵŶŷŸŹźŻżŽž";
const GREEK: &str = "ΑΒΓΔΕΖΗΘΙΚΛΜΝΞΟΠΡΣΤΥΦΧΨΩ αβγδεζηθικλμνξοπρςστυφχψω ΆΈΉΊΌΎΏάέήίόύώϊϋΐΰ ῃῳῷ";
const CYRILLIC: &str = "АБВГДЕЖЗИЙКЛМНОПРСТУФХЦЧШЩЪЫЬЭЮЯ абвгдежзийклмнопрстуфхцчшщъыьэюя ЁёЂђЃѓЄєЅѕІіЇїЈјЉљЊњЋћЌќЎўЏџ";
/// Grapheme clusters with stacked/combining marks, one per cell.
const MARKS: [&str; 20] = [
    "Ẫ",
    "ǻ",
    "e\u{30a}",
    "a\u{328}\u{308}",
    "Ǻ",
    "ṩ",
    "ấ",
    "ệ",
    "ǟ",
    "ȫ",
    "x\u{302}",
    "ỹ",
    "Ệ",
    "ǭ",
    "n\u{303}\u{301}",
    "o\u{308}\u{304}",
    "u\u{30c}\u{307}",
    "i\u{323}\u{302}",
    "E\u{300}\u{306}",
    "Ǚ",
];
const BRAILLE: &str = "⠁⠃⠇⠏⠟⠿⡿⣿⣀⣤⣶⣿⢸⡇⠉⠛";
const SHAPES: &str = "■□▢▣▤▥▦▧▨▩▪▫▬▭▮▯▰▱▲△▴▵▶▷▸▹►▻▼▽▾▿◀◁◂◃◄◅◆◇◈◉◊○◌◍◎●◐◑◒◓◔◕";
const ARROWS: &str =
    "←↑→↓↔↕↖↗↘↙↚↛↜↝↞↟↠↡↢↣↤↥↦↧↨↩↪↫↬↭↮↯↰↱↲↳↴↵↶↷↸↹↺↻↼↽↾↿⇀⇁⇂⇃⇄⇅⇆⇇⇈⇉⇊⇋⇌⇍⇎⇏⇐⇑⇒⇓⇔⇕⇖⇗⇘⇙⇚⇛⇜⇝⇞⇟⇠⇡⇢⇣⇤⇥⇦⇧⇨⇩";
const WIDE: &str = "汉字日本語한글 🙂🚀🎉👍🏽🇺🇸 ｜ｆｕｌｌｗｉｄｔｈ ｜";

/// Tile panels: (label, rows of the 8×4 rectangle as symbol patterns).
#[derive(Clone, Copy)]
enum Tile {
    /// Full blocks; every pixel of the panel must be ink.
    Solid,
    /// `▐▌` pairs: the join across each cell boundary must be solid.
    HalfColumns,
    /// `▄` over `▀`: the join across each row boundary must be solid.
    HalfRows,
    /// Shades: visual only.
    Shade(char),
    /// Horizontal lines: continuous through every pixel column.
    Horizontal(char),
    /// Vertical lines: continuous through every pixel row.
    Vertical(char),
}

impl Tile {
    fn symbol(self, column: u16, row: u16) -> char {
        match self {
            Tile::Solid => '█',
            Tile::HalfColumns => {
                if column.is_multiple_of(2) {
                    '▐'
                } else {
                    '▌'
                }
            }
            Tile::HalfRows => {
                if row.is_multiple_of(2) {
                    '▄'
                } else {
                    '▀'
                }
            }
            Tile::Shade(c) | Tile::Horizontal(c) | Tile::Vertical(c) => c,
        }
    }

    fn label(self) -> String {
        match self {
            Tile::Solid => "solid".into(),
            Tile::HalfColumns => "half columns".into(),
            Tile::HalfRows => "half rows".into(),
            Tile::Shade(c) => format!("shade {c}"),
            Tile::Horizontal(c) => format!("horizontal {c}"),
            Tile::Vertical(c) => format!("vertical {c}"),
        }
    }
}

const TILES_A: [Tile; 6] = [
    Tile::Solid,
    Tile::HalfColumns,
    Tile::HalfRows,
    Tile::Shade('░'),
    Tile::Shade('▒'),
    Tile::Shade('▓'),
];
const TILES_B: [Tile; 6] = [
    Tile::Horizontal('─'),
    Tile::Horizontal('━'),
    Tile::Horizontal('═'),
    Tile::Vertical('│'),
    Tile::Vertical('┃'),
    Tile::Vertical('║'),
];

/// Left column of tile `index` in a panel row (one blank column between tiles).
fn tile_column(index: usize) -> u16 {
    1 + index as u16 * (TILE_WIDTH + 1)
}

#[derive(Clone)]
struct LoadedFamily {
    name: &'static str,
    dir: &'static str,
    faces: FontFaces,
}

fn load_families(app: &mut App, wanted: &[usize]) -> Vec<LoadedFamily> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/fonts");
    let mut fonts = app.world_mut().resource_mut::<Assets<Font>>();
    wanted
        .iter()
        .filter_map(|index| {
            let (name, dir, regular, bold, italic, bold_italic) = FAMILIES[*index];
            let mut load = |file: &str| {
                std::fs::read(root.join(dir).join(file))
                    .ok()
                    .map(|bytes| fonts.add(Font::from_bytes(bytes)))
            };
            let faces = FontFaces {
                regular: load(regular)?.into(),
                bold: Some(load(bold)?.into()),
                italic: Some(load(italic)?.into()),
                bold_italic: Some(load(bold_italic)?.into()),
                synthesize: true,
            };
            Some(LoadedFamily { name, dir, faces })
        })
        .collect()
}

/// One rendered terminal of the harness.
#[derive(Component, Clone)]
struct Case {
    family: &'static str,
    dir: &'static str,
    scale: f32,
}

#[derive(Resource)]
struct Options {
    export: bool,
    check: bool,
    tiles_only: bool,
    output: String,
}

/// Measured textures queued for exporters; render-world preparation sizes buffers.
#[derive(Resource, Default)]
struct PendingExports(Vec<(Handle<Image>, String)>);

/// One readback: the entity, the RGBA8 bytes (rows may be padded) and the size.
type Capture = (Entity, Vec<u8>, UVec2);

/// Readback results keyed by entity.
#[derive(Resource, Default, Clone)]
struct Captures(Arc<Mutex<Vec<Capture>>>);

fn parse_arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|argument| argument == name)
        .and_then(|index| args.get(index + 1).cloned())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let export = args.iter().any(|argument| argument == "--export");
    let check = args.iter().any(|argument| argument == "--check");
    let tiles_only = args.iter().any(|argument| argument == "--tiles-only");
    let headless = export || check;
    let font_argument = parse_arg(&args, "--font");
    let scale_argument = parse_arg(&args, "--scale");
    let from_font =
        parse_arg(&args, "--from-font").map(|value| value.parse::<f32>().unwrap_or(18.0).max(1.0));

    let line_height = parse_arg(&args, "--line-height")
        .map(|s| s.parse::<f32>().expect("numeric line height"))
        .unwrap_or(1.0);
    let fixed = parse_arg(&args, "--fixed-cell").map(|s| {
        let (width, height) = s.split_once('x').expect("--fixed-cell WIDTHxHEIGHT");
        Vec2::new(
            width.parse().expect("cell width"),
            height.parse().expect("cell height"),
        )
    });
    let fixed_font_size = parse_arg(&args, "--font-size")
        .map(|s| s.parse::<f32>().expect("numeric font size"))
        .unwrap_or(18.0);
    assert!(line_height.is_finite() && line_height > 0.0);
    assert!(fixed_font_size.is_finite() && fixed_font_size > 0.0);
    if let Some(cell) = fixed {
        assert!(cell.is_finite() && cell.cmpgt(Vec2::ZERO).all());
    }
    let sizing = fixed.map_or_else(
        || {
            from_font.map_or(TerminalSizing::FitCellWidth(CELL), |font_size| {
                TerminalSizing::FromFont {
                    font_size,
                    line_height,
                }
            })
        },
        |cell_size| TerminalSizing::Fixed {
            cell_size,
            font_size: fixed_font_size,
        },
    );

    let wanted: Vec<usize> = match font_argument.as_deref() {
        Some("all") => (0..FAMILIES.len()).collect(),
        Some(value) => vec![
            value
                .parse::<usize>()
                .ok()
                .or_else(|| {
                    FAMILIES.iter().position(|family| {
                        family.1 == value
                            || family.0.to_lowercase().starts_with(&value.to_lowercase())
                    })
                })
                .unwrap_or(0)
                .min(FAMILIES.len() - 1),
        ],
        None => vec![0],
    };
    let scales: Vec<Option<f32>> = match scale_argument.as_deref() {
        Some("all") => SCALES.iter().copied().map(Some).collect(),
        Some(value) => vec![Some(value.parse::<f32>().unwrap_or(1.0).max(0.25))],
        None => vec![if headless { Some(1.0) } else { None }],
    };

    let mut app = App::new();
    let export_plugin = ImageExportPlugin::default();
    let export_threads = export_plugin.threads.clone();
    if headless {
        app.add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: bevy::window::ExitCondition::DontExit,
                    close_when_requested: false,
                    ..default()
                })
                .set(RenderPlugin {
                    synchronous_pipeline_compilation: true,
                    ..default()
                })
                .disable::<WinitPlugin>(),
        )
        .add_plugins(ScheduleRunnerPlugin::run_loop(
            std::time::Duration::from_millis(1),
        ));
    } else {
        // Resized to the measured terminal once its font is measured.
        let width = (f32::from(COLUMNS) * CELL.x + 2.0 * MARGIN) as u32;
        let height = (f32::from(ROWS) * CELL.y + 2.0 * MARGIN) as u32;
        app.add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "bevy_terminal_ratatui · glyph fidelity".into(),
                resolution: WindowResolution::new(width, height),
                ..default()
            }),
            ..default()
        }));
    }
    if check {
        app.world_mut()
            .resource_mut::<bevy::text::FontCx>()
            .collection = fontique::Collection::new(fontique::CollectionOptions {
            system_fonts: false,
            ..default()
        });
        let fallback = Font::from_bytes(
            include_bytes!("../assets/fonts/fidelity/NotoEmoji-Regular.ttf").to_vec(),
        );
        let mut fonts = app.world_mut().resource_mut::<bevy::text::FontCx>();
        fonts.collection.register_fonts(fallback.data, None);
        fonts
            .set_emoji_family("Noto Emoji")
            .expect("bundled emoji family");
    }
    let families = load_families(&mut app, &wanted);
    if families.len() != wanted.len() {
        eprintln!("no vendored font family found under assets/fonts");
        std::process::exit(2);
    }
    app.add_plugins(TerminalPlugin)
        .add_plugins(common::app::presentation)
        .insert_resource(Options {
            export,
            check,
            tiles_only,
            output: parse_arg(&args, "--output")
                .unwrap_or_else(|| "target/glyph-fidelity-check".into()),
        })
        .init_resource::<PendingExports>()
        .init_resource::<Captures>()
        .init_resource::<Frame>()
        .add_systems(Update, on_ready.after(TerminalSystems::Sync))
        .add_systems(Update, (refresh_titles, tick));
    if export {
        app.add_plugins(export_plugin)
            .add_systems(Update, spawn_pending_exports);
        common::export::gpu::install(&mut app);
    }

    // Spawn one primary terminal per family × scale.
    let mut cases = Vec::new();
    for family in &families {
        for scale in &scales {
            cases.push((family.clone(), *scale));
        }
    }
    let windowed_family = families[0].name;
    app.add_systems(Startup, move |mut commands: Commands| {
        for (index, (family, scale)) in cases.iter().enumerate() {
            let (mut terminal, renderer) = RatatuiTerminal::new(COLUMNS, ROWS).with_renderer();
            draw_harness(&mut terminal, family.name, scale.unwrap_or(1.0), None);
            let config = TerminalRenderConfig {
                sizing,
                font: family.faces.clone(),
                raster: RasterConfig {
                    scale: scale.unwrap_or(1.0),
                    ..default()
                },
                cursor: CursorConfig {
                    blink_hz: None,
                    ..default()
                },
                ..default()
            };
            let case = Case {
                family: family.name,
                dir: family.dir,
                scale: scale.unwrap_or(1.0),
            };
            if headless {
                commands.spawn((
                    common::app::headless_terminal(renderer, config),
                    case,
                    Drawn(terminal),
                    TitledWith::default(),
                ));
            } else if index == 0 {
                commands.spawn(Camera2d);
                commands.spawn((
                    common::app::ui_terminal(renderer, config, Vec2::splat(MARGIN)),
                    case,
                    Drawn(terminal),
                    TitledWith::default(),
                ));
            }
        }
    });
    if !headless {
        if scale_argument.is_none() {
            app.add_plugins(common::app::window_scale);
        }
        app.insert_resource(FontCycle {
            families: families.clone(),
            current: 0,
        })
        .add_systems(
            Update,
            (cycle_fonts, fit_to_window).before(TerminalSystems::Sync),
        );
        info!("showing {windowed_family}; Space/Tab cycle families");
    }
    app.run();
    export_threads.finish();

    if check {
        std::process::exit(RESULT.load(std::sync::atomic::Ordering::SeqCst));
    }
}

/// Process exit status of `--check`.
static RESULT: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// The Ratatui terminal that draws a case (kept to redraw the title once metrics are known).
#[derive(Component)]
struct Drawn(RatatuiTerminal);

/// The metrics the title was last drawn with, so it is redrawn only when they
/// actually change.
#[derive(Component, Default)]
struct TitledWith(Option<(Vec2, f32)>);

#[derive(Resource, Default)]
struct Frame(u32);

#[derive(Resource)]
struct FontCycle {
    families: Vec<LoadedFamily>,
    current: usize,
}

/// Starts readbacks once geometry is measured. The oracle reads raw font
/// bitmaps independently after the rendered scenes have reached the GPU.
fn on_ready(
    mut commands: Commands,
    options: Res<Options>,
    cases: Query<(Entity, &Case, &TerminalTexture), Without<CaptureStarted>>,
    mut pending: ResMut<PendingExports>,
) {
    for (entity, case, texture) in &cases {
        if texture.measured().is_none() {
            continue;
        }
        commands.entity(entity).insert(CaptureStarted);
        if options.export {
            let dir = format!("target/glyph-fidelity/{}/{}x", case.dir, case.scale);
            pending.0.push((texture.image.clone(), dir));
        }
        if options.check {
            commands
                .spawn(Readback::texture(texture.image.clone()))
                .observe(
                    move |done: On<ReadbackComplete>,
                          textures: Query<&TerminalTexture>,
                          captures: Res<Captures>,
                          frame: Res<Frame>,
                          mut commands: Commands| {
                        if frame.0 < 8 {
                            return;
                        }
                        let size = textures
                            .get(entity)
                            .ok()
                            .and_then(TerminalTexture::measured)
                            .map(|geometry| geometry.size())
                            .unwrap_or_default();
                        let mut captures = captures.0.lock().unwrap();
                        captures.retain(|(e, ..)| *e != entity);
                        captures.push((entity, done.data.clone(), size));
                        if frame.0 >= 24 {
                            commands.entity(done.entity).despawn();
                        }
                    },
                );
        }
    }
}

#[derive(Component)]
struct CaptureStarted;

/// Redraws a primary terminal's title with its measured metrics whenever they
/// change (first measurement, or a font change in the windowed mode).
fn refresh_titles(
    mut cases: Query<
        (&Case, &TerminalTexture, &mut Drawn, &mut TitledWith),
        Changed<TerminalTexture>,
    >,
) {
    for (case, texture, mut drawn, mut titled) in &mut cases {
        let Some(geometry) = texture.measured() else {
            continue;
        };
        let metrics = Some((geometry.cell_size(), geometry.font_size()));
        if titled.0 == metrics {
            continue;
        }
        titled.0 = metrics;
        draw_harness(&mut drawn.0, case.family, case.scale, metrics);
    }
}

fn spawn_pending_exports(
    mut commands: Commands,
    mut pending: ResMut<PendingExports>,
    mut sources: ResMut<Assets<ImageExportSource>>,
) {
    for (handle, dir) in pending.0.drain(..) {
        commands.spawn((
            ImageExport(sources.add(handle)),
            ImageExportSettings {
                output_dir: dir,
                extension: "png".into(),
            },
        ));
    }
}

/// Drives the headless run: exits after the exports/readbacks are done and
/// runs the checks.
#[allow(clippy::too_many_arguments)]
fn tick(
    mut frame: ResMut<Frame>,
    options: Res<Options>,
    captures: Res<Captures>,
    cases: Query<(
        Entity,
        &Case,
        &TerminalRenderer,
        &TerminalTexture,
        &TerminalRenderConfig,
    )>,
    mut oracle: fidelity_oracle::Rasterizer,
    mut exit: MessageWriter<AppExit>,
) {
    frame.0 += 1;
    if !(options.export || options.check) {
        return;
    }
    if options.check {
        let expected = cases.iter().count();
        let ready = captures.0.lock().unwrap().len();
        // Every terminal must have a capture; wait for the startup scenes and
        // title redraws to reach the GPU before comparing them.
        if frame.0 < 30 || ready < expected {
            if frame.0 > 600 {
                eprintln!("timed out waiting for readbacks ({ready}/{expected})");
                RESULT.store(1, std::sync::atomic::Ordering::SeqCst);
                exit.write(AppExit::Success);
            }
            return;
        }
        let captures = captures.0.lock().unwrap();
        let failures = run_checks(
            &captures,
            &cases,
            options.tiles_only,
            &mut oracle,
            &options.output,
        );
        RESULT.store(i32::from(failures > 0), std::sync::atomic::Ordering::SeqCst);
        exit.write(AppExit::Success);
    } else if frame.0 >= 8 {
        exit.write(AppExit::Success);
    }
}

/// Windowed mode: the grid follows the (resizable) window.
fn fit_to_window(
    mut cases: Query<(&Case, &mut Drawn)>,
    textures: Query<&TerminalTexture>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    for (case, mut drawn) in &mut cases {
        if common::app::fit_grid_to_window(&mut drawn.0, &textures, &windows, MARGIN) {
            let metrics = textures
                .single()
                .ok()
                .and_then(TerminalTexture::measured)
                .map(|geometry| (geometry.cell_size(), geometry.font_size()));
            draw_harness(&mut drawn.0, case.family, case.scale, metrics);
        }
    }
}

fn cycle_fonts(
    keys: Res<ButtonInput<KeyCode>>,
    mut cycle: ResMut<FontCycle>,
    mut cases: Query<(&mut Case, &mut TerminalRenderConfig)>,
) {
    let count = cycle.families.len();
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let next = if keys.just_pressed(KeyCode::Space) || (keys.just_pressed(KeyCode::Tab) && !shift) {
        (cycle.current + 1) % count
    } else if keys.just_pressed(KeyCode::Backspace) || (keys.just_pressed(KeyCode::Tab) && shift) {
        (cycle.current + count - 1) % count
    } else {
        return;
    };
    cycle.current = next;
    let family = cycle.families[next].clone();
    for (mut case, mut config) in &mut cases {
        case.family = family.name;
        case.dir = family.dir;
        config.font = family.faces.clone();
    }
}

fn checker(column: u16, row: u16) -> Color {
    CHECKER[usize::from((column + row) % 2)]
}

/// Draws the whole harness into `terminal`.
fn draw_harness(
    terminal: &mut RatatuiTerminal,
    family: &str,
    scale: f32,
    metrics: Option<(Vec2, f32)>,
) {
    terminal.draw(|frame| {
        let mut lines: Vec<Line> = vec![Line::raw(""); usize::from(ROWS)];
        let title = match metrics {
            Some((cell, font)) => format!(
                "glyph fidelity · {family} · {scale}x · cell {}×{} · font {font:.2} px",
                cell.x, cell.y
            ),
            None => format!("glyph fidelity · {family} · {scale}x · measuring…"),
        };
        lines[usize::from(ROW_TITLE)] = Line::raw(title);
        let ascii: String = (0x20u8..=0x7e).map(char::from).collect();
        for (row, modifier) in ROWS_ASCII.into_iter().zip([
            Modifier::empty(),
            Modifier::BOLD,
            Modifier::ITALIC,
            Modifier::BOLD | Modifier::ITALIC,
        ]) {
            lines[usize::from(row)] = Line::from(Span::styled(
                ascii.clone(),
                Style::new().add_modifier(modifier),
            ));
        }
        lines[usize::from(ROWS_LATIN[0])] = Line::raw(LATIN_1);
        lines[usize::from(ROWS_LATIN[1])] = Line::raw(LATIN_EXT);
        lines[usize::from(ROW_GREEK)] = Line::raw(GREEK);
        lines[usize::from(ROW_CYRILLIC)] = Line::raw(CYRILLIC);
        let marks: Vec<Span> = MARKS
            .iter()
            .flat_map(|mark| [Span::raw(*mark), Span::raw(" ")])
            .collect();
        lines[usize::from(ROW_MARKS)] = Line::from(marks);
        let boxes: String = (0x2500u32..=0x257f).filter_map(char::from_u32).collect();
        let (first, second) = boxes.split_at(boxes.chars().take(64).map(char::len_utf8).sum());
        lines[usize::from(ROWS_BOX[0])] = Line::raw(first.to_owned());
        lines[usize::from(ROWS_BOX[1])] = Line::raw(second.to_owned());
        let blocks: String = (0x2580u32..=0x259f).filter_map(char::from_u32).collect();
        lines[usize::from(ROW_BLOCKS)] = Line::raw(format!("{blocks} {BRAILLE} {SHAPES}"));
        lines[usize::from(ROW_ARROWS)] = Line::raw(ARROWS);
        lines[usize::from(ROW_WIDE)] = Line::raw(WIDE);
        for (top, tiles) in [(TILE_ROWS_A, TILES_A), (TILE_ROWS_B, TILES_B)] {
            for row in 0..TILE_HEIGHT {
                let mut text = String::new();
                for (index, tile) in tiles.iter().enumerate() {
                    for column in 0..TILE_WIDTH {
                        text.push(tile.symbol(column, row));
                    }
                    if index + 1 < tiles.len() {
                        text.push(' ');
                    }
                }
                lines[usize::from(top + row)] = Line::raw(text);
            }
        }
        // Content starts at column 1 (guard column at 0); a smaller grid (a
        // resized window) crops it.
        let area = frame.area();
        let content = ratatui::layout::Rect::new(
            1,
            0,
            CONTENT.min(area.width.saturating_sub(2)),
            ROWS.min(area.height),
        );
        frame.render_widget(Paragraph::new(lines), content);
        let buffer = frame.buffer_mut();
        for row in 0..area.height {
            for column in 0..area.width {
                let cell = &mut buffer[(column, row)];
                if column == 0 || column == area.width - 1 {
                    cell.set_symbol("│");
                }
                cell.set_fg(INK).set_bg(checker(column, row));
            }
        }
    });
}

/// Reads one texel of a padded readback.
fn texel(data: &[u8], size: UVec2, x: u32, y: u32) -> [u8; 3] {
    let stride = data.len() / size.y as usize;
    let start = y as usize * stride + x as usize * 4;
    [data[start], data[start + 1], data[start + 2]]
}

fn is_ink(pixel: [u8; 3], background: [u8; 3]) -> bool {
    pixel
        .iter()
        .zip(background)
        .any(|(p, b)| p.abs_diff(b) > 12)
}

fn checker_rgb(column: u16, row: u16) -> [u8; 3] {
    match checker(column, row) {
        Color::Rgb(r, g, b) => [r, g, b],
        _ => [0, 0, 0],
    }
}

struct Group {
    name: &'static str,
    rows: Vec<u16>,
}

fn groups() -> Vec<Group> {
    vec![
        Group {
            name: "ascii",
            rows: ROWS_ASCII.to_vec(),
        },
        Group {
            name: "latin",
            rows: ROWS_LATIN.to_vec(),
        },
        Group {
            name: "greek/cyrillic",
            rows: vec![ROW_GREEK, ROW_CYRILLIC],
        },
        Group {
            name: "marks",
            rows: vec![ROW_MARKS],
        },
        Group {
            name: "wide/fallback",
            rows: vec![ROW_WIDE],
        },
        Group {
            name: "symbols",
            rows: vec![ROW_ARROWS],
        },
        Group {
            name: "shapes/braille",
            rows: vec![ROW_BLOCKS],
        },
    ]
}

/// Block elements are drawn as geometry, never as glyphs (shades excluded).
fn is_block_element(symbol: &str) -> bool {
    let mut chars = symbol.chars();
    matches!(
        (chars.next(), chars.next()),
        (Some('\u{2580}'..='\u{2590}' | '\u{2594}'..='\u{259f}'), None)
    )
}

/// Compares each capture with raw glyph coverage and checks the procedural tiles.
/// Returns the number of failed (family, scale, group) combinations.
fn run_checks(
    captures: &[Capture],
    cases: &Query<(
        Entity,
        &Case,
        &TerminalRenderer,
        &TerminalTexture,
        &TerminalRenderConfig,
    )>,
    tiles_only: bool,
    oracle: &mut fidelity_oracle::Rasterizer,
    output: &str,
) -> usize {
    let capture_of = |entity: Entity| captures.iter().find(|(e, ..)| *e == entity);
    let mut failures = 0;
    let output = std::path::Path::new(output);
    std::fs::create_dir_all(output).expect("create fidelity output directory");
    let mut summary =
        String::from("family\tscale\tgroup\tchecked\tunavailable\tfailures\tconstrained\n");
    let mut diagnostics = String::from("family\tscale\tgroup\tclassification\tdiagnostic\n");
    println!("family            scale  group                 result");
    let mut primaries: Vec<_> = cases.iter().collect();
    primaries.sort_by(|a, b| (a.1.dir, a.1.scale.to_bits()).cmp(&(b.1.dir, b.1.scale.to_bits())));
    for (entity, case, renderer, texture, config) in primaries {
        let Some((_, data, size)) = capture_of(entity) else {
            println!(
                "{:<17} {:<6} {:<21} MISSING CAPTURE",
                case.family, case.scale, "-"
            );
            failures += 1;
            continue;
        };
        let directory = output.join(case.dir).join(format!("{}x", case.scale));
        std::fs::create_dir_all(&directory).expect("create case directory");
        let stride = data.len() / size.y as usize;
        let pixels: Vec<u8> = data
            .chunks_exact(stride)
            .flat_map(|row| row[..size.x as usize * 4].iter().copied())
            .collect();
        fidelity_oracle::save_png(&directory.join("actual.png"), *size, pixels);
        let snapshot = renderer.surface().snapshot();
        let cell = UVec2::new(
            (texture.measured().unwrap().cell_size().x * texture.measured().unwrap().raster_scale())
                .round() as u32,
            (texture.measured().unwrap().cell_size().y * texture.measured().unwrap().raster_scale())
                .round() as u32,
        );
        let font_size = texture.measured().unwrap().physical_font_size();
        let expected_baseline = oracle
            .baseline(config, font_size, cell.y as f32)
            .expect("configured baseline reference");
        for group in groups().into_iter().filter(|_| !tiles_only) {
            // Every content cell of a row, blank or not, must show exactly the
            // row the oracle composes: ordinary text at its rasterized size,
            // symbols fitted, ink confined to the row, wider runs overflowing
            // their neighbours with later cells drawn on top.
            let mut problems: Vec<String> = Vec::new();
            let mut constrained: Vec<String> = Vec::new();
            let mut checked = 0;
            let mut unsupported = 0;
            let mut saved = 0;
            for row in &group.rows {
                let cells = snapshot.row(*row);
                let row_size = UVec2::new(size.x, cell.y);
                // A wide glyph's continuation cells carry their anchor's background.
                let anchors: Vec<u16> = (0..cells.len())
                    .map(|column| {
                        let mut anchor = column;
                        while anchor > 0 && cells[anchor].is_continuation() {
                            anchor -= 1;
                        }
                        anchor as u16
                    })
                    .collect();
                let mut expected: Vec<[u8; 3]> = (0..row_size.y)
                    .flat_map(|_| {
                        (0..row_size.x).map(|x| checker_rgb(anchors[(x / cell.x) as usize], *row))
                    })
                    .collect();
                let mut layers = vec![0u8; expected.len()];
                let mut skipped = vec![false; cells.len()];
                // Compose the whole row (edge columns can reach the interior);
                // compare only the interior.
                let mut column = 0;
                while column < cells.len() {
                    let source_cell = &cells[column];
                    let span = fidelity_oracle::span(cells, column) as usize;
                    let Some(symbol) = glyph_at(&snapshot, column as u16, *row) else {
                        column += 1;
                        continue;
                    };
                    if is_block_element(&symbol) {
                        // Procedural geometry, drawn over text; checked by the tiles.
                        skipped[column..column + span].fill(true);
                        column += span;
                        continue;
                    }
                    let interior = column > 0 && column + 1 < usize::from(COLUMNS);
                    if interior {
                        checked += 1;
                    }
                    let columns = fidelity_oracle::visual_columns(cells, column);
                    let placement = match oracle.place(
                        source_cell,
                        config,
                        font_size,
                        cell,
                        columns,
                        expected_baseline,
                    ) {
                        Ok(placement) => placement,
                        Err(error) => {
                            if interior {
                                problems.push(format!("{symbol:?}: oracle error: {error}"));
                            }
                            skipped[column..column + span].fill(true);
                            column += span;
                            continue;
                        }
                    };
                    if !placement.reference.supported {
                        // A font-coverage gap: the cell itself is not judged, but
                        // its `.notdef` raster is drawn and may reach its neighbours.
                        if interior {
                            unsupported += 1;
                            diagnostics.push_str(&format!(
                                "{}\t{}\t{}\tunavailable\t{symbol:?} at ({column},{row})\n",
                                case.family, case.scale, group.name
                            ));
                            if group.name == "ascii" {
                                problems.push(format!("{symbol:?}: missing required ASCII glyph"));
                            }
                        }
                        skipped[column..column + span].fill(true);
                    }
                    let Some((min, max)) = placement.ink() else {
                        if interior {
                            problems.push(format!("{symbol:?}: empty reference"));
                        }
                        column += span;
                        continue;
                    };
                    let x0 = (column as u32 * cell.x) as i32;
                    let origin = IVec2::new(x0, 0);
                    if fidelity_oracle::is_graphics(&symbol) {
                        placement.reference.composite_within(
                            &mut expected,
                            &mut layers,
                            row_size,
                            origin + placement.shift,
                            x0..x0 + (cell.x * columns) as i32,
                        );
                    } else {
                        placement.reference.composite(
                            &mut expected,
                            row_size,
                            origin + placement.shift,
                        );
                    }
                    if !interior || !placement.reference.supported {
                        column += span;
                        continue;
                    }
                    // A rescaled symbol must be the unconstrained raster shrunk
                    // uniformly by about the ratio that makes it fit: never
                    // larger, and no more than 15% (a whole-pixel font-size
                    // step plus hinting) and two pixels smaller per axis. This
                    // is not derived from the renderer's or the oracle's
                    // fitting arithmetic.
                    if placement.scaled {
                        let fitted = (max - min).as_vec2();
                        let full = placement.unconstrained.as_vec2();
                        let bounds = Vec2::new((cell.x * columns) as f32, cell.y as f32);
                        let expected_size = full * (bounds / full).min_element().min(1.0);
                        let too_large = (fitted - expected_size).max_element() > 2.0;
                        let too_small = (expected_size * 0.85 - fitted).max_element() > 2.0;
                        if too_large || too_small {
                            problems.push(format!(
                                "{symbol:?} at ({column},{row}): rescaled ink {fitted:?} from {full:?} in {bounds:?}, expected about {expected_size:?}"
                            ));
                        }
                    }
                    let note = if placement.scaled {
                        Some("fitted")
                    } else if max.x > (cell.x * columns) as i32 || min.x < 0 {
                        Some("overflow")
                    } else if columns as usize > span {
                        Some("spread")
                    } else if min.y < 0 || max.y > cell.y as i32 {
                        Some("clipped")
                    } else {
                        None
                    };
                    if let Some(note) = note {
                        let message = format!(
                            "{symbol:?} at ({column},{row}): {note}; ink {min:?}..{max:?} over {columns} cell(s) of {cell:?}"
                        );
                        diagnostics.push_str(&format!(
                            "{}\t{}\t{}\t{note}\t{message}\n",
                            case.family, case.scale, group.name
                        ));
                        constrained.push(message);
                    }
                    column += span;
                }
                for column in 1..usize::from(COLUMNS) - 1 {
                    if skipped[column] {
                        continue;
                    }
                    let symbol = glyph_at(&snapshot, column as u16, *row)
                        .unwrap_or_else(|| cells[column].symbol().to_owned());
                    let x0 = column as u32 * cell.x;
                    let region = |pixels: &dyn Fn(u32, u32) -> [u8; 3]| -> Vec<[u8; 3]> {
                        (0..cell.y)
                            .flat_map(|y| (0..cell.x).map(move |x| pixels(x0 + x, y)))
                            .collect()
                    };
                    let expected_cell = region(&|x, y| expected[(y * row_size.x + x) as usize]);
                    let actual_cell =
                        region(&|x, y| texel(data, *size, x, u32::from(*row) * cell.y + y));
                    let layers = &layers;
                    let tolerance: Vec<u8> = (0..cell.y)
                        .flat_map(|y| {
                            (0..cell.x).map(move |x| layers[(y * row_size.x + x0 + x) as usize])
                        })
                        .collect();
                    let differences = fidelity_oracle::differing_pixels_within(
                        &expected_cell,
                        &actual_cell,
                        &tolerance,
                    );
                    if differences != 0 || (saved < 3 && symbol == "W") {
                        let rgba = |pixels: &[[u8; 3]]| -> Vec<u8> {
                            pixels
                                .iter()
                                .flat_map(|rgb| [rgb[0], rgb[1], rgb[2], 255])
                                .collect()
                        };
                        fidelity_oracle::save_detail(
                            &directory.join(format!("reference-{row}-{column}.png")),
                            cell,
                            rgba(&expected_cell),
                        );
                        fidelity_oracle::save_detail(
                            &directory.join(format!("actual-{row}-{column}.png")),
                            cell,
                            rgba(&actual_cell),
                        );
                        saved += 1;
                    }
                    if differences == 0 {
                        continue;
                    }
                    let worst = expected_cell
                        .iter()
                        .zip(&actual_cell)
                        .enumerate()
                        .map(|(i, (e, a))| (fidelity_oracle::pixel_difference(e, a), i, e, a))
                        .max()
                        .unwrap();
                    let message = format!(
                        "{symbol:?} at ({column},{row}): {differences} differing pixels in the cell; cell {cell:?}; worst at ({},{}) expected {:?} actual {:?}",
                        worst.1 as u32 % cell.x,
                        worst.1 as u32 / cell.x,
                        worst.2,
                        worst.3
                    );
                    diagnostics.push_str(&format!(
                        "{}\t{}\t{}\tfailure\t{message}\n",
                        case.family, case.scale, group.name
                    ));
                    problems.push(message);
                }
            }
            if group.name == "wide/fallback" && checked - unsupported < 5 {
                problems.push(
                    "bundled emoji fallback must exercise at least five supported wide symbols"
                        .into(),
                );
            }
            summary.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                case.family,
                case.scale,
                group.name,
                checked,
                unsupported,
                problems.len(),
                constrained.len()
            ));
            println!(
                "    coverage: {} supported, {unsupported} unavailable in bundled fonts",
                checked - unsupported
            );
            let result = if problems.is_empty() {
                format!(
                    "pass ({} supported glyphs, {unsupported} unavailable, {} fitted/overflowing/clipped runs verified)",
                    checked - unsupported,
                    constrained.len()
                )
            } else {
                failures += 1;
                format!(
                    "FAIL ({} cells differ from the composed row; {} constrained runs)",
                    problems.len(),
                    constrained.len()
                )
            };
            println!(
                "{:<17} {:<6} {:<21} {result}",
                case.family, case.scale, group.name
            );
            for problem in problems.iter().take(8) {
                println!("    {problem}");
            }
            if problems.len() > 8 {
                println!("    … {} more", problems.len() - 8);
            }
            for note in constrained.iter().take(3) {
                println!("    (constrained) {note}");
            }
            if constrained.len() > 3 {
                println!("    (constrained) … {} more", constrained.len() - 3);
            }
        }
        // Block elements are procedural geometry, including half-block joins.
        // Every solid/block/line tile is strict, independent of the font's outlines.
        let mut problems = Vec::new();
        for (top, tiles) in [(TILE_ROWS_A, TILES_A), (TILE_ROWS_B, TILES_B)] {
            for (index, tile) in tiles.iter().enumerate() {
                let left = tile_column(index);
                if let Some(problem) = check_tile(data, *size, cell, *tile, left, top) {
                    problems.push(format!("{}: {problem}", tile.label()));
                }
            }
        }
        summary.push_str(&format!(
            "{}\t{}\ttiles\t{}\t0\t{}\t{}\n",
            case.family,
            case.scale,
            TILES_A.len() + TILES_B.len(),
            problems.len(),
            0
        ));
        let result = if problems.is_empty() {
            "pass".to_owned()
        } else {
            failures += 1;
            format!("FAIL ({} tile defects)", problems.len())
        };
        println!(
            "{:<17} {:<6} {:<21} {result}",
            case.family, case.scale, "tiles"
        );
        for problem in &problems {
            println!("    {problem}");
        }
    }
    std::fs::write(output.join("results.tsv"), summary).expect("write fidelity summary");
    std::fs::write(output.join("diagnostics.tsv"), diagnostics).expect("write glyph diagnostics");
    if failures == 0 {
        println!("all checks passed");
    } else {
        println!("{failures} check(s) failed");
    }
    failures
}

/// The symbol drawn at a cell, if it is a non-blank glyph anchor.
fn glyph_at(snapshot: &TerminalSnapshot, column: u16, row: u16) -> Option<String> {
    let cell = snapshot.cell((column, row))?;
    if cell.is_continuation() {
        return None;
    }
    let symbol = cell.symbol();
    (!symbol.trim().is_empty()).then(|| symbol.to_owned())
}

/// Checks one 8×4 tile; returns a description of the first defect.
fn check_tile(
    data: &[u8],
    size: UVec2,
    cell: UVec2,
    tile: Tile,
    left: u16,
    top: u16,
) -> Option<String> {
    let x0 = u32::from(left) * cell.x;
    let y0 = u32::from(top) * cell.y;
    let width = u32::from(TILE_WIDTH) * cell.x;
    let height = u32::from(TILE_HEIGHT) * cell.y;
    let solid = |x: u32, y: u32| {
        let p = texel(data, size, x, y);
        p.iter().zip(INK_RGB).all(|(a, b)| a.abs_diff(b) <= 1)
    };
    let inked = |x: u32, y: u32| is_ink(texel(data, size, x, y), checker_rgb(left, top));
    match tile {
        Tile::Solid => {
            for y in y0..y0 + height {
                for x in x0..x0 + width {
                    if !solid(x, y) {
                        return Some(format!(
                            "seam: pixel ({},{}) in the solid panel is {:?}",
                            x - x0,
                            y - y0,
                            texel(data, size, x, y)
                        ));
                    }
                }
            }
            None
        }
        Tile::HalfColumns => {
            // Each `▐▌` pair forms a block across the cell boundary; the join must be
            // solid in the two pixel columns on either side of the join.
            for pair in 0..u32::from(TILE_WIDTH) / 2 {
                let boundary = x0 + (pair * 2 + 1) * cell.x;
                for y in y0 + 1..y0 + height - 1 {
                    for x in boundary.saturating_sub(2)..boundary + 2 {
                        if !solid(x, y) {
                            return Some(format!(
                                "seam: pixel ({},{}) at the join of pair {pair} is {:?}",
                                x - x0,
                                y - y0,
                                texel(data, size, x, y)
                            ));
                        }
                    }
                }
            }
            None
        }
        Tile::HalfRows => {
            for pair in 0..u32::from(TILE_HEIGHT) / 2 {
                let boundary = y0 + (pair * 2 + 1) * cell.y;
                for y in boundary.saturating_sub(2)..boundary + 2 {
                    for x in x0 + 1..x0 + width - 1 {
                        if !solid(x, y) {
                            return Some(format!(
                                "seam: pixel ({},{}) at the join of pair {pair} is {:?}",
                                x - x0,
                                y - y0,
                                texel(data, size, x, y)
                            ));
                        }
                    }
                }
            }
            None
        }
        Tile::Shade(_) => None,
        Tile::Horizontal(_) => {
            for x in x0..x0 + width {
                if !(y0..y0 + height).any(|y| inked(x, y)) {
                    return Some(format!("gap: no ink in pixel column {}", x - x0));
                }
            }
            None
        }
        Tile::Vertical(_) => {
            for y in y0..y0 + height {
                if !(x0..x0 + width).any(|x| inked(x, y)) {
                    return Some(format!("gap: no ink in pixel row {}", y - y0));
                }
            }
            None
        }
    }
}
