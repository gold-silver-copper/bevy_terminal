//! Glyph fidelity harness: proves that no glyph of the primary font is clipped
//! and that block/box glyphs tile without seams.
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
//! - `--check` reads every texture back from the GPU and asserts, per glyph
//!   cell of the ASCII/Latin/Greek/Cyrillic rows, that the glyph has exactly
//!   as many ink pixels as the same glyph rendered in a roomier reference cell
//!   (so nothing was clipped), and that the tile panels have no seams; the
//!   process exits with status 1 when a check fails.
//!
//! Without `--export`/`--check` a window shows one family; `Space`/`Tab`
//! cycle families.

#[allow(dead_code)]
mod common;

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
    CellSizing, CursorConfig, FontFaces, FontSizing, RasterConfig, TerminalPlugin, TerminalReady,
    TerminalRenderConfig, TerminalRenderScale, TerminalSnapshot, TerminalSystems, TerminalTexture,
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

/// Rows and their groups. Rows checked for clipping are those with a `strict` group.
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
    /// The roomy reference terminal for `--check`, if this is a primary case.
    reference: Option<Entity>,
    /// Whether this is a reference terminal.
    is_reference: bool,
}

#[derive(Resource)]
struct Options {
    export: bool,
    check: bool,
    tiles_only: bool,
}

/// Textures waiting for exporters (spawned a frame after `TerminalReady`).
#[derive(Resource, Default)]
struct PendingExports(Vec<(Handle<Image>, String, u32)>);

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
    let families = load_families(&mut app, &wanted);
    if families.is_empty() {
        eprintln!("no vendored font family found under assets/fonts");
        std::process::exit(2);
    }
    app.add_plugins(TerminalPlugin)
        .insert_resource(Options {
            export,
            check,
            tiles_only,
        })
        .init_resource::<PendingExports>()
        .init_resource::<Captures>()
        .init_resource::<Frame>()
        .add_observer(on_ready)
        .add_systems(Update, (refresh_titles, tick));
    if export {
        app.add_plugins(export_plugin)
            .add_systems(Update, spawn_pending_exports);
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
            let (mut terminal, renderer) = RatatuiTerminal::new(COLUMNS, ROWS);
            draw_harness(&mut terminal, family.name, scale.unwrap_or(1.0), None);
            let config = TerminalRenderConfig {
                cell_size: from_font.map_or(CELL.into(), |_| CellSizing::FROM_FONT),
                font: family.faces.clone(),
                font_size: from_font.map_or(FontSizing::FitCellWidth, FontSizing::Px),
                raster: RasterConfig {
                    scale: scale.map_or(TerminalRenderScale::Automatic, TerminalRenderScale::Fixed),
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
                reference: None,
                is_reference: false,
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
/// actually change (the texture component is written on every sync).
#[derive(Component, Default)]
struct TitledWith(Option<(Vec2, f32)>);

#[derive(Resource, Default)]
struct Frame(u32);

#[derive(Resource)]
struct FontCycle {
    families: Vec<LoadedFamily>,
    current: usize,
}

/// Once a primary terminal is measured: redraw its title with the metrics,
/// queue its exporter, and (for `--check`) spawn its roomy reference twin.
fn on_ready(
    ready: On<TerminalReady>,
    mut commands: Commands,
    options: Res<Options>,
    mut cases: Query<(&mut Case, &TerminalTexture, &TerminalRenderConfig)>,
    mut pending: ResMut<PendingExports>,
) {
    let Ok((mut case, texture, config)) = cases.get_mut(ready.entity) else {
        return;
    };
    let scale = case.scale;
    if options.export && !case.is_reference {
        let dir = format!("target/glyph-fidelity/{}/{}x", case.dir, scale);
        pending.0.push((texture.image.clone(), dir, 1));
    }
    if options.check {
        if !case.is_reference && case.reference.is_none() {
            // Same font size and content in a cell 6 px wider and 10 px taller: the
            // oracle for "was anything clipped".
            let (mut terminal, renderer) = RatatuiTerminal::new(COLUMNS, ROWS);
            draw_harness(&mut terminal, case.family, scale, None);
            let faces = config.font.clone();
            let reference = commands
                .spawn((
                    common::app::headless_terminal(
                        renderer,
                        TerminalRenderConfig {
                            cell_size: CellSizing::Logical(
                                texture.cell_size + Vec2::new(6.0, 10.0),
                            ),
                            font_size: FontSizing::Px(texture.font_size),
                            font: faces,
                            raster: RasterConfig {
                                scale: TerminalRenderScale::Fixed(scale),
                                ..default()
                            },
                            cursor: CursorConfig {
                                blink_hz: None,
                                ..default()
                            },
                            ..default()
                        },
                    ),
                    Case {
                        family: case.family,
                        dir: case.dir,
                        scale,
                        reference: None,
                        is_reference: true,
                    },
                    Drawn(terminal),
                    TitledWith::default(),
                ))
                .id();
            case.reference = Some(reference);
        }
        let entity = ready.entity;
        commands
            .spawn(Readback::texture(texture.image.clone()))
            .observe(
                move |done: On<ReadbackComplete>,
                      textures: Query<&TerminalTexture>,
                      captures: Res<Captures>,
                      frame: Res<Frame>,
                      mut commands: Commands| {
                    // Give the scene a few frames to reach the GPU, keep the latest
                    // readback for a while, then stop reading back.
                    if frame.0 < 8 {
                        return;
                    }
                    let size = textures.get(entity).map(|t| t.size).unwrap_or_default();
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

/// Redraws a primary terminal's title with its measured metrics whenever they
/// change (first measurement, or a font change in the windowed mode).
fn refresh_titles(
    mut cases: Query<
        (&Case, &TerminalTexture, &mut Drawn, &mut TitledWith),
        Changed<TerminalTexture>,
    >,
) {
    for (case, texture, mut drawn, mut titled) in &mut cases {
        let metrics = Some((texture.cell_size, texture.font_size));
        if case.is_reference || titled.0 == metrics {
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
    pending.0.retain_mut(|(handle, dir, frames)| {
        if *frames > 0 {
            *frames -= 1;
            return true;
        }
        commands.spawn((
            ImageExport(sources.add(handle.clone())),
            ImageExportSettings {
                output_dir: dir.clone(),
                extension: "png".into(),
            },
        ));
        false
    });
}

/// Drives the headless run: exits after the exports/readbacks are done and
/// runs the checks.
#[allow(clippy::too_many_arguments)]
fn tick(
    mut frame: ResMut<Frame>,
    options: Res<Options>,
    captures: Res<Captures>,
    cases: Query<(Entity, &Case, &TerminalRenderer, &TerminalTexture)>,
    mut exit: MessageWriter<AppExit>,
) {
    frame.0 += 1;
    if !(options.export || options.check) {
        return;
    }
    if options.check {
        let expected = cases.iter().count();
        let ready = captures.0.lock().unwrap().len();
        // Every primary and reference terminal must have a capture; wait a little
        // longer so all readbacks reflect the final scene.
        if frame.0 < 30 || ready < expected {
            if frame.0 > 600 {
                eprintln!("timed out waiting for readbacks ({ready}/{expected})");
                RESULT.store(1, std::sync::atomic::Ordering::SeqCst);
                exit.write(AppExit::Success);
            }
            return;
        }
        let captures = captures.0.lock().unwrap();
        let failures = run_checks(&captures, &cases, options.tiles_only);
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
            let metrics = textures.single().ok().map(|t| (t.cell_size, t.font_size));
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

/// Ink statistics of one cell (or wide span) of a capture.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Ink {
    /// Number of ink pixels.
    count: u32,
    /// Ink bounding box size in pixels (zero when there is no ink).
    bbox: UVec2,
    /// Ink bounding box origin relative to the cell.
    origin: UVec2,
}

/// Measures the ink of a cell in a capture with a physical cell size.
fn ink_of(data: &[u8], size: UVec2, cell: UVec2, column: u16, row: u16, span: u16) -> Ink {
    let background = checker_rgb(column, row);
    let x0 = u32::from(column) * cell.x;
    let y0 = u32::from(row) * cell.y;
    let mut count = 0;
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (u32::MAX, u32::MAX, 0, 0);
    for y in y0..(y0 + cell.y).min(size.y) {
        for x in x0..(x0 + cell.x * u32::from(span)).min(size.x) {
            // A wide cell's continuation column has the other checker color.
            let bg = if x >= x0 + cell.x {
                checker_rgb(column + 1, row)
            } else {
                background
            };
            if is_ink(texel(data, size, x, y), bg) {
                count += 1;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }
    let (bbox, origin) = if count == 0 {
        (UVec2::ZERO, UVec2::ZERO)
    } else {
        (
            UVec2::new(max_x - min_x + 1, max_y - min_y + 1),
            UVec2::new(min_x - x0, min_y - y0),
        )
    };
    Ink {
        count,
        bbox,
        origin,
    }
}

struct Group {
    name: &'static str,
    rows: Vec<u16>,
    strict: bool,
}

fn groups() -> Vec<Group> {
    vec![
        Group {
            name: "ascii",
            rows: ROWS_ASCII.to_vec(),
            strict: true,
        },
        Group {
            name: "latin",
            rows: ROWS_LATIN.to_vec(),
            strict: true,
        },
        Group {
            name: "greek/cyrillic",
            rows: vec![ROW_GREEK, ROW_CYRILLIC],
            strict: true,
        },
        Group {
            name: "marks",
            rows: vec![ROW_MARKS],
            strict: false,
        },
        Group {
            name: "box drawing",
            rows: ROWS_BOX.to_vec(),
            strict: false,
        },
        Group {
            name: "blocks/shapes/arrows",
            rows: vec![ROW_BLOCKS, ROW_ARROWS],
            strict: false,
        },
    ]
}

/// Compares every primary capture with its reference and checks the tiles.
/// Returns the number of failed (family, scale, group) combinations.
fn run_checks(
    captures: &[Capture],
    cases: &Query<(Entity, &Case, &TerminalRenderer, &TerminalTexture)>,
    tiles_only: bool,
) -> usize {
    let capture_of = |entity: Entity| captures.iter().find(|(e, ..)| *e == entity);
    let mut failures = 0;
    println!("family            scale  group                 result");
    let mut primaries: Vec<_> = cases
        .iter()
        .filter(|(_, case, ..)| !case.is_reference)
        .collect();
    primaries.sort_by(|a, b| (a.1.dir, a.1.scale.to_bits()).cmp(&(b.1.dir, b.1.scale.to_bits())));
    for (entity, case, renderer, texture) in primaries {
        let Some((_, data, size)) = capture_of(entity) else {
            println!(
                "{:<17} {:<6} {:<21} MISSING CAPTURE",
                case.family, case.scale, "-"
            );
            failures += 1;
            continue;
        };
        let snapshot = renderer.surface().snapshot();
        let cell = UVec2::new(
            (texture.cell_size.x * case.scale).round() as u32,
            (texture.cell_size.y * case.scale).round() as u32,
        );
        let reference = case
            .reference
            .and_then(|reference| cases.get(reference).ok())
            .and_then(|(entity, _, _, texture)| {
                capture_of(entity).map(|(_, data, size)| (data, size, texture))
            });
        // The font's line box in reference-cell coordinates: the rows a full block
        // covers completely (measured on the solid tile's first cell).
        let line_box = reference.and_then(|(ref_data, ref_size, ref_texture)| {
            let ref_cell = UVec2::new(
                (ref_texture.cell_size.x * case.scale).round() as u32,
                (ref_texture.cell_size.y * case.scale).round() as u32,
            );
            block_rows(ref_data, *ref_size, ref_cell, tile_column(0), TILE_ROWS_A)
        });
        for group in groups().into_iter().filter(|_| !tiles_only) {
            // A glyph whose unclipped ink lies inside the font's line box and is no
            // wider than its cell must keep every pixel. A glyph the font designed
            // beyond the line box or wider than the cell (accents that overshoot,
            // italic overhang, fallback families) is reported for information
            // only — clipping it (after fitting) is the documented policy.
            let mut problems: Vec<String> = Vec::new();
            let mut oversize: Vec<String> = Vec::new();
            let mut checked = 0;
            for row in &group.rows {
                for column in 1..COLUMNS - 1 {
                    let Some(symbol) = glyph_at(&snapshot, column, *row) else {
                        continue;
                    };
                    let span = snapshot.cell((column, *row)).map_or(1, |c| c.columns());
                    checked += 1;
                    let ink = ink_of(data, *size, cell, column, *row, span);
                    let Some((ref_data, ref_size, ref_texture)) = reference else {
                        if ink.count == 0 {
                            problems.push(format!("{symbol:?} at ({column},{row}): no ink"));
                        }
                        continue;
                    };
                    let ref_cell = UVec2::new(
                        (ref_texture.cell_size.x * case.scale).round() as u32,
                        (ref_texture.cell_size.y * case.scale).round() as u32,
                    );
                    let expected = ink_of(ref_data, *ref_size, ref_cell, column, *row, span);
                    if ink.count == expected.count {
                        continue;
                    }
                    let (line_top, line_bottom) = line_box.unwrap_or((0, cell.y));
                    let fits = expected.bbox.x <= cell.x * u32::from(span)
                        && expected.origin.y >= line_top
                        && expected.origin.y + expected.bbox.y <= line_bottom;
                    let message = format!(
                        "{symbol:?} at ({column},{row}): {} ink px vs {} unclipped ({} px {}; glyph {}×{} in a {}×{} cell, drawn {}×{} at {},{})",
                        ink.count,
                        expected.count,
                        expected.count.abs_diff(ink.count),
                        if ink.count < expected.count {
                            "lost"
                        } else {
                            "extra"
                        },
                        expected.bbox.x,
                        expected.bbox.y,
                        cell.x * u32::from(span),
                        cell.y,
                        ink.bbox.x,
                        ink.bbox.y,
                        ink.origin.x,
                        ink.origin.y
                    );
                    if fits {
                        problems.push(message);
                    } else {
                        oversize.push(message);
                    }
                }
            }
            let result = if problems.is_empty() {
                format!(
                    "pass ({checked} glyphs, {} outside the line box or wider than the cell)",
                    oversize.len()
                )
            } else if group.strict {
                failures += 1;
                format!(
                    "FAIL ({} of {checked} glyphs clipped, {} outside the line box or wider than the cell)",
                    problems.len(),
                    oversize.len()
                )
            } else {
                format!(
                    "info ({} of {checked} glyphs clipped, {} outside the line box or wider than the cell)",
                    problems.len(),
                    oversize.len()
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
            for problem in oversize.iter().take(3) {
                println!("    (oversize) {problem}");
            }
            if oversize.len() > 3 {
                println!("    (oversize) … {} more", oversize.len() - 3);
            }
        }
        // Tiles. Solid blocks and lines are strict; the half-block joins depend on
        // the font drawing its half blocks exactly to the cell edge (DejaVu Sans
        // Mono's `▐` stops short of it), so they are reported for information.
        let mut problems = Vec::new();
        let mut notes = Vec::new();
        for (top, tiles) in [(TILE_ROWS_A, TILES_A), (TILE_ROWS_B, TILES_B)] {
            for (index, tile) in tiles.iter().enumerate() {
                let left = tile_column(index);
                if let Some(problem) = check_tile(data, *size, cell, *tile, left, top) {
                    if matches!(tile, Tile::HalfColumns | Tile::HalfRows) {
                        notes.push(format!("{}: {problem}", tile.label()));
                    } else {
                        problems.push(format!("{}: {problem}", tile.label()));
                    }
                }
            }
        }
        let result = if problems.is_empty() {
            format!("pass ({} font-side half-block joins noted)", notes.len())
        } else {
            failures += 1;
            format!(
                "FAIL ({}, {} half-block joins noted)",
                problems.len(),
                notes.len()
            )
        };
        println!(
            "{:<17} {:<6} {:<21} {result}",
            case.family, case.scale, "tiles"
        );
        for problem in &problems {
            println!("    {problem}");
        }
        for note in &notes {
            println!("    (font) {note}");
        }
    }
    if failures == 0 {
        println!("all checks passed");
    } else {
        println!("{failures} check(s) failed");
    }
    failures
}

/// Rows `[top, bottom)` of a cell that a full block glyph covers completely.
fn block_rows(data: &[u8], size: UVec2, cell: UVec2, column: u16, row: u16) -> Option<(u32, u32)> {
    // The block may be narrower than a roomy reference cell: sample the columns
    // that are white in the cell's middle row.
    let x0 = u32::from(column) * cell.x;
    let y0 = u32::from(row) * cell.y;
    let white = |x: u32, y: u32| {
        texel(data, size, x, y)
            .iter()
            .zip(INK_RGB)
            .all(|(a, b)| a.abs_diff(b) <= 1)
    };
    let middle = y0 + cell.y / 2;
    let columns: Vec<u32> = (x0..x0 + cell.x).filter(|x| white(*x, middle)).collect();
    if columns.is_empty() {
        return None;
    }
    let full = |y: u32| columns.iter().all(|x| white(*x, y0 + y));
    let top = (0..cell.y).find(|y| full(*y))?;
    let bottom = (top..cell.y).take_while(|y| full(*y)).last()? + 1;
    Some((top, bottom))
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
            // solid (two pixel columns on either side, away from the panel's own edge
            // rows where a font's half blocks may end differently from its full block).
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
