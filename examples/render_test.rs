//! A single comprehensive render test scene.
//!
//! One window shows everything the renderer must get right: every one of the
//! 512 modifier combinations, the four font faces, the 16 ANSI colors as
//! foreground and background, the full 256-color cube and grayscale ramp, RGB
//! gradients, underline colors, box drawing in every weight, block/quadrant/
//! shade/braille elements, wide CJK/emoji cells, combining marks, RTL/Indic
//! text, and a blinking cursor. Pass `--export` (or set `RENDER_TEST_EXPORT=1`)
//! to write the renderer-owned texture to `target/render-test/<family>/`
//! headlessly instead, and `--font <index|dir>` (or `RENDER_TEST_FONT`) to
//! pick the initial font family.
//!
//! Pass `--transparent` to render the terminal with a 60 % translucent
//! background over a colored window clear color (a check for straight-alpha
//! compositing of the sRGB texture).
//!
//! Press `Space`/`Tab` (or `Shift+Tab` to go back) to cycle through the vendored
//! font families under `assets/fonts/`, so glyph coverage and metrics can be
//! compared per font; the current family is shown in the first line and the
//! window title.

#[allow(dead_code)]
mod common;

use bevy::{
    prelude::*,
    render::RenderPlugin,
    window::{PrimaryWindow, WindowResolution},
};
use bevy_image_export::ImageExportPlugin;
use bevy_terminal_ratatui::RatatuiTerminal;
use bevy_terminal_ratatui::prelude::{
    CursorConfig, FontFaces, RasterConfig, TerminalPlugin, TerminalRenderConfig,
    TerminalRenderScale, TerminalSystems, TerminalTexture, TerminalTheme,
};
use ratatui::{
    layout::Position,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

/// Font families vendored under `assets/fonts/`, loaded from disk at runtime so
/// the large optional families stay out of the published crate. Each entry is
/// (display name, directory, regular, bold, italic, bold-italic).
const FAMILIES: [(&str, &str, &str, &str, &str, &str); 6] = [
    (
        "Iosevka Fixed 34.8.0",
        "iosevka-fixed",
        "IosevkaFixed-Regular.ttf",
        "IosevkaFixed-Bold.ttf",
        "IosevkaFixed-Italic.ttf",
        "IosevkaFixed-BoldItalic.ttf",
    ),
    (
        "JetBrains Mono 2.304",
        "jetbrains-mono",
        "JetBrainsMono-Regular.ttf",
        "JetBrainsMono-Bold.ttf",
        "JetBrainsMono-Italic.ttf",
        "JetBrainsMono-BoldItalic.ttf",
    ),
    (
        "Cascadia Mono 2407.24",
        "cascadia-mono",
        "CascadiaMono-Regular.ttf",
        "CascadiaMono-Bold.ttf",
        "CascadiaMono-Italic.ttf",
        "CascadiaMono-BoldItalic.ttf",
    ),
    (
        "Hack 3.003",
        "hack",
        "Hack-Regular.ttf",
        "Hack-Bold.ttf",
        "Hack-Italic.ttf",
        "Hack-BoldItalic.ttf",
    ),
    (
        "DejaVu Sans Mono 2.37",
        "dejavu-sans-mono",
        "DejaVuSansMono.ttf",
        "DejaVuSansMono-Bold.ttf",
        "DejaVuSansMono-Oblique.ttf",
        "DejaVuSansMono-BoldOblique.ttf",
    ),
    (
        "Source Code Pro 2.042",
        "source-code-pro",
        "SourceCodePro-Regular.ttf",
        "SourceCodePro-Bold.ttf",
        "SourceCodePro-It.ttf",
        "SourceCodePro-BoldIt.ttf",
    ),
];

const COLUMNS: u16 = 132;
const ROWS: u16 = 62;
const CELL: Vec2 = Vec2::new(11.0, 20.0);
const MARGIN: f32 = 12.0;

const MODIFIERS: [(Modifier, &str); 9] = [
    (Modifier::BOLD, "B"),
    (Modifier::DIM, "D"),
    (Modifier::ITALIC, "I"),
    (Modifier::UNDERLINED, "U"),
    (Modifier::SLOW_BLINK, "S"),
    (Modifier::RAPID_BLINK, "R"),
    (Modifier::REVERSED, "V"),
    (Modifier::HIDDEN, "H"),
    (Modifier::CROSSED_OUT, "X"),
];

const ANSI: [(Color, &str); 16] = [
    (Color::Black, "Blk"),
    (Color::Red, "Red"),
    (Color::Green, "Grn"),
    (Color::Yellow, "Yel"),
    (Color::Blue, "Blu"),
    (Color::Magenta, "Mag"),
    (Color::Cyan, "Cyn"),
    (Color::Gray, "Gry"),
    (Color::DarkGray, "DGy"),
    (Color::LightRed, "LRd"),
    (Color::LightGreen, "LGn"),
    (Color::LightYellow, "LYl"),
    (Color::LightBlue, "LBl"),
    (Color::LightMagenta, "LMg"),
    (Color::LightCyan, "LCy"),
    (Color::White, "Wht"),
];

#[derive(Clone)]
struct LoadedFamily {
    name: &'static str,
    regular: Handle<Font>,
    bold: Handle<Font>,
    italic: Handle<Font>,
    bold_italic: Handle<Font>,
}

impl LoadedFamily {
    fn apply(&self, config: &mut TerminalRenderConfig) {
        config.font = FontFaces {
            regular: self.regular.clone().into(),
            bold: Some(self.bold.clone().into()),
            italic: Some(self.italic.clone().into()),
            bold_italic: Some(self.bold_italic.clone().into()),
            synthesize: true,
        };
    }
}

#[derive(Resource)]
struct FontCycle {
    families: Vec<LoadedFamily>,
    current: usize,
    terminal: RatatuiTerminal,
}

/// Loads every vendored family that is present on disk; missing files are skipped.
fn load_families(app: &mut App) -> Vec<LoadedFamily> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/fonts");
    let mut fonts = app.world_mut().resource_mut::<Assets<Font>>();
    FAMILIES
        .iter()
        .filter_map(|(name, dir, regular, bold, italic, bold_italic)| {
            let mut load = |file: &str| {
                std::fs::read(root.join(dir).join(file))
                    .ok()
                    .map(|bytes| fonts.add(Font::from_bytes(bytes)))
            };
            let regular = load(regular)?;
            let bold = load(bold)?;
            let italic = load(italic)?;
            let bold_italic = load(bold_italic)?;
            Some(LoadedFamily {
                name,
                regular,
                bold,
                italic,
                bold_italic,
            })
        })
        .collect()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let export = std::env::var_os("RENDER_TEST_EXPORT").is_some()
        || args.iter().any(|argument| argument == "--export");
    let transparent = args.iter().any(|argument| argument == "--transparent");
    let font_argument = args
        .iter()
        .position(|argument| argument == "--font")
        .and_then(|index| args.get(index + 1).cloned())
        .or_else(|| std::env::var("RENDER_TEST_FONT").ok());
    let (mut terminal, renderer) = RatatuiTerminal::new(COLUMNS, ROWS);
    draw_render_test(&mut terminal, FAMILIES[0].0);
    let theme = TerminalTheme {
        background: if transparent {
            bevy::color::Color::srgba(0.05, 0.05, 0.1, 0.6)
        } else {
            TerminalTheme::default().background
        },
        ..default()
    };
    let config = TerminalRenderConfig {
        theme,
        cell_size: CELL.into(),
        raster: RasterConfig {
            scale: if export {
                TerminalRenderScale::Fixed(1.0)
            } else {
                TerminalRenderScale::Automatic
            },
            ..default()
        },
        cursor: CursorConfig {
            blink_hz: if export { None } else { Some(1.0) },
            ..default()
        },
        ..default()
    };
    let width = (f32::from(COLUMNS) * CELL.x + 2.0 * MARGIN) as u32;
    let height = (f32::from(ROWS) * CELL.y + 2.0 * MARGIN) as u32;

    let mut app = App::new();
    let export_plugin = ImageExportPlugin::default();
    let export_threads = export_plugin.threads.clone();
    if export {
        app.add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        resolution: WindowResolution::new(1, 1).with_scale_factor_override(1.0),
                        visible: false,
                        ..default()
                    }),
                    ..default()
                })
                .set(RenderPlugin {
                    synchronous_pipeline_compilation: true,
                    ..default()
                }),
        );
    } else {
        app.add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "bevy_terminal_ratatui · render test".into(),
                resolution: WindowResolution::new(width, height),
                ..default()
            }),
            ..default()
        }));
    }
    let families = load_families(&mut app);
    // `--font` / `RENDER_TEST_FONT` selects the initial family by index or directory name.
    let initial = font_argument
        .and_then(|value| {
            value.parse::<usize>().ok().or_else(|| {
                FAMILIES
                    .iter()
                    .position(|family| family.1 == value || family.0.starts_with(&value))
            })
        })
        .unwrap_or(0)
        .min(families.len().saturating_sub(1));
    let mut config = config;
    families[initial].apply(&mut config);
    draw_render_test(&mut terminal, families[initial].name);
    let output_dir = format!("target/render-test/{}", FAMILIES[initial].1);
    app.add_plugins(TerminalPlugin).insert_resource(FontCycle {
        families,
        current: initial,
        terminal,
    });
    if export {
        common::export::export_terminals_on_ready(&mut app, output_dir);
        app.add_plugins(export_plugin)
            .add_systems(Startup, move |mut commands: Commands| {
                commands.spawn(common::app::headless_terminal(
                    renderer.clone(),
                    config.clone(),
                ));
            })
            .add_systems(Update, common::export::exit_after(10))
            .run();
        export_threads.finish();
    } else {
        if transparent {
            app.insert_resource(ClearColor(bevy::color::Color::srgb(0.35, 0.1, 0.4)));
        }
        app.add_systems(Startup, move |mut commands: Commands| {
            commands.spawn(Camera2d);
            commands.spawn(common::app::ui_terminal(
                renderer.clone(),
                config.clone(),
                Vec2::splat(MARGIN),
            ));
        })
        .add_systems(
            Update,
            (cycle_fonts, fit_to_window).before(TerminalSystems::Sync),
        )
        .run();
    }
}

/// Refits the grid to the (resizable) window; the scene is laid out for
/// 132×62 cells and is cropped in smaller windows.
fn fit_to_window(
    mut cycle: ResMut<FontCycle>,
    textures: Query<&TerminalTexture>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let FontCycle {
        families,
        current,
        terminal,
    } = &mut *cycle;
    if common::app::fit_grid_to_window(terminal, &textures, &windows, MARGIN) {
        draw_render_test(terminal, families[*current].name);
    }
}

/// Space/Tab select the next family, Shift+Tab or Backspace the previous one.
fn cycle_fonts(
    keys: Res<ButtonInput<KeyCode>>,
    mut cycle: ResMut<FontCycle>,
    mut configs: Query<&mut TerminalRenderConfig>,
    mut windows: Query<&mut Window>,
) {
    let count = cycle.families.len();
    if count == 0 {
        return;
    }
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
    for mut config in &mut configs {
        family.apply(&mut config);
    }
    for mut window in &mut windows {
        window.title = format!("bevy_terminal_ratatui · render test · {}", family.name);
    }
    let FontCycle { terminal, .. } = &mut *cycle;
    draw_render_test(terminal, family.name);
}

fn heading(text: &str) -> Line<'static> {
    Line::from(Span::styled(
        text.to_owned(),
        Style::new()
            .fg(Color::Black)
            .bg(Color::LightCyan)
            .add_modifier(Modifier::BOLD),
    ))
}

fn label(text: &str) -> Span<'static> {
    Span::styled(text.to_owned(), Style::new().fg(Color::DarkGray))
}

fn modifier_combination(bits: u16) -> Modifier {
    MODIFIERS
        .iter()
        .enumerate()
        .filter(|(index, _)| bits & (1 << index) != 0)
        .fold(Modifier::empty(), |set, (_, (modifier, _))| set | *modifier)
}

fn draw_render_test(terminal: &mut RatatuiTerminal, font_name: &str) {
    terminal
        .draw(|frame| {
            let mut lines: Vec<Line> = Vec::new();

            // 1. Font faces and named modifiers.
            lines.push(heading(&format!(
                " 1. Faces and modifiers  (regular / bold / italic / bold+italic must be four distinct faces)   font: {font_name}   Space/Tab = next font, Shift+Tab = previous "
            )));
            let mut faces = vec![label("faces:    ")];
            for (text, modifier) in [
                ("Regular", Modifier::empty()),
                ("Bold", Modifier::BOLD),
                ("Italic", Modifier::ITALIC),
                ("BoldItalic", Modifier::BOLD | Modifier::ITALIC),
            ] {
                faces.push(Span::styled(text, Style::new().add_modifier(modifier)));
                faces.push(Span::raw("  "));
            }
            faces.push(label(" | single: "));
            for (modifier, _) in MODIFIERS {
                faces.push(Span::styled(
                    format!("{modifier:?}").replace("Modifier(", "").replace(')', ""),
                    Style::new().fg(Color::White).bg(Color::Indexed(236)).add_modifier(modifier),
                ));
                faces.push(Span::raw(" "));
            }
            lines.push(Line::from(faces));

            // 2. All 512 modifier combinations. Bit i of the column index maps to MODIFIERS[i].
            let mut legend = String::from("bits: ");
            for (index, (_, code)) in MODIFIERS.iter().enumerate() {
                legend.push_str(&format!("{index}={code} "));
            }
            lines.push(heading(&format!(
                " 2. All 512 modifier combinations ({legend}); glyph 'A' on Rgb(230,200,60)/Rgb(30,30,60), underline color LightGreen; hidden cells (bit 7) show background only "
            )));
            for row in 0..8_u16 {
                let mut spans = vec![label(&format!("{:>3} ", row * 64))];
                for column in 0..64_u16 {
                    let bits = row * 64 + column;
                    let style = Style::new()
                        .fg(Color::Rgb(230, 200, 60))
                        .bg(Color::Rgb(30, 30, 60))
                        .underline_color(Color::LightGreen)
                        .add_modifier(modifier_combination(bits));
                    spans.push(Span::styled("A", style));
                    spans.push(Span::styled(
                        " ",
                        Style::new().bg(if bits % 2 == 0 { Color::Reset } else { Color::Indexed(234) }),
                    ));
                }
                lines.push(Line::from(spans));
            }

            // 3. ANSI 16 as foreground and background.
            lines.push(heading(" 3. ANSI 16 colors: row A foreground on default, row B default text on background, row C fg on inverse bg, row D REVERSED "));
            let mut fg = vec![label("A ")];
            let mut bg = vec![label("B ")];
            let mut both = vec![label("C ")];
            let mut rev = vec![label("D ")];
            for (index, (color, name)) in ANSI.iter().enumerate() {
                let inverse = ANSI[15 - index].0;
                fg.push(Span::styled(format!(" {name} "), Style::new().fg(*color)));
                bg.push(Span::styled(format!(" {name} "), Style::new().bg(*color)));
                both.push(Span::styled(format!(" {name} "), Style::new().fg(*color).bg(inverse)));
                rev.push(Span::styled(
                    format!(" {name} "),
                    Style::new().fg(*color).add_modifier(Modifier::REVERSED),
                ));
                for spans in [&mut fg, &mut bg, &mut both, &mut rev] {
                    spans.push(Span::raw(" "));
                }
            }
            lines.extend([Line::from(fg), Line::from(bg), Line::from(both), Line::from(rev)]);

            // 4. 256-color cube and grayscale.
            lines.push(heading(" 4. Indexed 16..231 cube (6 rows of 36) and 232..255 grayscale ramp, as backgrounds; digits show the index mod 10 "));
            for row in 0..6_u8 {
                let mut spans = vec![label(&format!("{:>3} ", 16 + row * 36))];
                for column in 0..36_u8 {
                    let index = 16 + row * 36 + column;
                    spans.push(Span::styled(
                        format!("{}", index % 10),
                        Style::new().bg(Color::Indexed(index)).fg(Color::Black),
                    ));
                    spans.push(Span::styled("  ", Style::new().bg(Color::Indexed(index))));
                }
                lines.push(Line::from(spans));
            }
            let mut gray = vec![label("232 ")];
            for index in 232..=255_u8 {
                gray.push(Span::styled(
                    format!(" {} ", index % 10),
                    Style::new().bg(Color::Indexed(index)).fg(if index < 244 { Color::White } else { Color::Black }),
                ));
            }
            lines.push(Line::from(gray));

            // 5. RGB gradients.
            lines.push(heading(" 5. RGB: 128-step gradients as background (R, G, B, hue) and as foreground text; must be smooth without banding artifacts from palette quantization "));
            for (name, f) in [
                ("R ", (|t: f32| (t, 0.0, 0.0)) as fn(f32) -> (f32, f32, f32)),
                ("G ", |t: f32| (0.0, t, 0.0)),
                ("B ", |t: f32| (0.0, 0.0, t)),
                ("H ", |t: f32| hue(t)),
            ] {
                let mut spans = vec![label(name)];
                for step in 0..128 {
                    let (r, g, b) = f(step as f32 / 127.0);
                    spans.push(Span::styled(
                        " ",
                        Style::new().bg(Color::Rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)),
                    ));
                }
                lines.push(Line::from(spans));
            }
            let mut text = vec![label("fg ")];
            for step in 0..128 {
                let (r, g, b) = hue(step as f32 / 127.0);
                text.push(Span::styled(
                    "█",
                    Style::new().fg(Color::Rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)),
                ));
            }
            lines.push(Line::from(text));
            let mut text = vec![label("tx ")];
            for step in 0..128 {
                let (r, g, b) = hue(step as f32 / 127.0);
                text.push(Span::styled(
                    ["R", "G", "B"][step % 3],
                    Style::new().fg(Color::Rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)),
                ));
            }
            lines.push(Line::from(text));

            // 6. Underline colors.
            lines.push(heading(" 6. Underline colors: each word underlined in a different color than its text; last two also DIM / REVERSED "));
            let mut ul = vec![label("   ")];
            for (index, (color, name)) in ANSI.iter().enumerate() {
                let mut style = Style::new()
                    .fg(Color::White)
                    .underline_color(*color)
                    .add_modifier(Modifier::UNDERLINED);
                if index == 14 {
                    style = style.add_modifier(Modifier::DIM);
                }
                if index == 15 {
                    style = style.add_modifier(Modifier::REVERSED);
                }
                ul.push(Span::styled(name.to_string(), style));
                ul.push(Span::raw("  "));
            }
            ul.push(Span::styled(
                "Rgb(255,128,0) underline",
                Style::new().underline_color(Color::Rgb(255, 128, 0)).add_modifier(Modifier::UNDERLINED),
            ));
            lines.push(Line::from(ul));

            // 7. Box drawing.
            lines.push(heading(" 7. Box drawing: light, heavy, double, rounded, dashed, mixed junctions — every line must be continuous with no gaps or overlaps "));
            for text in [
                "┌─┬─┐ ┏━┳━┓ ╔═╦═╗ ╭─┬─╮ ┌╌╌┬╌╌┐ ┍━┯━┑ ┎─┰─┒ ╒═╤═╕ ╓─╥─╖  ╱╲╳  ┄┄┄┄ ┅┅┅┅ ┆┆ ┇┇ ┈┈┈┈ ┉┉┉┉ ┊┊ ┋┋ ╌╌╌╌ ╍╍╍╍ ╎╎ ╏╏",
                "│ │ │ ┃ ┃ ┃ ║ ║ ║ │ │ │ ┆  ┆  ┆ │ │ │ ┃ ┃ ┃ │ │ │ ║ ║ ║   ╲╱   ╴╵╶╷ ╸╹╺╻ ╼╽╾╿  ┝┞┟┠┡┢┣┤┥┦┧┨┩┪┫┬┭┮┯┰┱┲┳┴┵┶┷┸┹┺┻",
                "├─┼─┤ ┣━╋━┫ ╠═╬═╣ ├─┼─┤ ├╌╌┼╌╌┤ ┝━┿━┥ ┠─╂─┨ ╞═╪═╡ ╟─╫─╢        ┼┽┾┿╀╁╂╃╄╅╆╇╈╉╊╋ ╪╫╬ ╭╮╯╰ ┏┓┗┛ ╔╗╚╝",
                "└─┴─┘ ┗━┻━┛ ╚═╩═╝ ╰─┴─╯ └╌╌┴╌╌┘ ┕━┷━┙ ┖─┸─┚ ╘═╧═╛ ╙─╨─╜  ┌──────────────────────────────────┐",
                "                                                            │ ┏━━━━━━━━━┓  ╔═════════╗  ╭───╮ │",
                "█▉▊▋▌▍▎▏ ▁▂▃▄▅▆▇█ ▀▄ ▐▌ ▖▗▘▙▚▛▜▝▞▟ ░▒▓█ ▔▕ blocks & quads          │ ┃ ╔═════╗ ┃  ║ ┌─────┐ ║  │ ● │ │",
                "⠁⠃⠇⡇⣇⣧⣷⣿ ⣿⣷⣯⣟⡿⢿⣻⣽⣼⣧ ⠉⠛⠿⣿ braille  ◢◣◤◥ ▲▼◀▶ ►◄ ▴▾◂▸ ●○◐◑◒◓ ■□▪▫ ◆◇  │ ┃ ║  ⋯  ║ ┃  ║ │ ─┼─ │ ║  ╰───╯ │",
                "─────────────────────────────────────────────────────────── ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━",
            ] {
                lines.push(Line::from(text));
            }
            {
                let mut spans = vec![label("═══════════════════════════════════════════════════════════ ")];
                spans.push(Span::styled(
                    "│ ┗━━━━━━━━━┛  ╚═════════╝  ",
                    Style::new().fg(Color::LightGreen),
                ));
                spans.push(Span::styled("└──────────────────────────────────┘", Style::new().fg(Color::LightBlue)));
                lines.push(Line::from(spans));
            }
            {
                let mut spans = vec![label("colored blocks: ")];
                for (index, (color, _)) in ANSI.iter().enumerate() {
                    spans.push(Span::styled("█▓▒░ ", Style::new().fg(*color).bg(ANSI[(index + 8) % 16].0)));
                }
                spans.push(Span::styled("▀▄▀▄▀▄", Style::new().fg(Color::Rgb(255, 0, 128)).bg(Color::Rgb(0, 128, 255))));
                spans.push(Span::styled(" ▛▜▙▟ ", Style::new().fg(Color::Yellow).bg(Color::Blue)));
                spans.push(Span::styled("█ hidden", Style::new().fg(Color::Red).add_modifier(Modifier::HIDDEN)));
                spans.push(Span::styled(" █ dim", Style::new().fg(Color::Red).add_modifier(Modifier::DIM)));
                spans.push(Span::styled(" █ underlined", Style::new().fg(Color::Red).add_modifier(Modifier::UNDERLINED)));
                spans.push(Span::styled(" █ crossed", Style::new().fg(Color::Red).add_modifier(Modifier::CROSSED_OUT)));
                spans.push(Span::styled(" █ ul-color", Style::new().fg(Color::Red).underline_color(Color::Cyan).add_modifier(Modifier::UNDERLINED)));
                lines.push(Line::from(spans));
            }

            // 8. Unicode.
            lines.push(heading(" 8. Unicode: wide cells must occupy exactly two columns and never overlap the '|' guard that follows; combining marks stay one cell "));
            for text in [
                "|CJK      |汉字|日本語|한글|中文测试|  |ｆｕｌｌｗｉｄｔｈ|  guard columns: |0123456789|",
                "|Emoji    |🙂|🚀|🎉|👍🏽|🇺🇸|👨‍👩‍👧‍👦|❤️|✅|⚠️|  ascii after |A|B|C|",
                "|Combining|e\u{301}|A\u{30a}|n\u{303}|o\u{308}\u{304}|Z\u{330}\u{301}| |é|Å|ñ| precomposed",
                "|RTL      |שלום עולם|مرحبا بالعالم| |Indic|नमस्ते|தமிழ்|ไทย| |Greek|αβγδε| |Cyrillic|привет|",
                "|Symbols  |→←↑↓↔⇒⇐| |±×÷≠≤≥∞∑∏√∫| |©®™°µ¶§| |€£¥¢| |«»‹›“”‘’| |…–—| |☐☑☒☺☻♠♣♥♦| |αβγ|ᚠᚢᚦ|",
                "|Mixed    |a汉b字c日d本e語f| |x🙂y🚀z| |narrow|wide|narrow| the guards | must | stay | aligned |",
            ] {
                lines.push(Line::from(text));
            }
            {
                let mut spans = vec![Span::raw("|Styled   |")];
                for (text, style) in [
                    ("汉字", Style::new().fg(Color::Black).bg(Color::LightYellow).add_modifier(Modifier::BOLD)),
                    ("日本", Style::new().fg(Color::LightRed).add_modifier(Modifier::UNDERLINED | Modifier::ITALIC)),
                    ("🚀🙂", Style::new().bg(Color::Indexed(238))),
                    ("한글", Style::new().add_modifier(Modifier::REVERSED)),
                    ("中文", Style::new().add_modifier(Modifier::CROSSED_OUT)),
                    ("測試", Style::new().add_modifier(Modifier::DIM)),
                ] {
                    spans.push(Span::styled(text, style));
                    spans.push(Span::raw("|"));
                }
                spans.push(Span::raw(" wide + styles"));
                lines.push(Line::from(spans));
            }

            // 9. Cursor and text.
            lines.push(heading(" 9. Cursor blinks at the end of this line (block); ASCII coverage below must have even spacing "));
            lines.push(Line::from("cursor here > "));
            lines.push(Line::from(" !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~"));
            lines.push(Line::from(Span::styled(
                " !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~",
                Style::new().add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(Span::styled(
                " !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~",
                Style::new().add_modifier(Modifier::ITALIC),
            )));
            lines.push(Line::from(Span::styled(
                " !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~",
                Style::new().add_modifier(Modifier::BOLD | Modifier::ITALIC),
            )));
            lines.push(Line::from(vec![
                label("checker: "),
                Span::styled("▀▄".repeat(30), Style::new().fg(Color::White).bg(Color::Black)),
                Span::styled("▚▞".repeat(30), Style::new().fg(Color::White).bg(Color::Black)),
                Span::styled("░▒▓█".repeat(8), Style::new().fg(Color::White).bg(Color::Black)),
            ]));

            frame.render_widget(Paragraph::new(lines), frame.area());
            frame.set_cursor_position(Position::new(14, cursor_row_of_cursor_line()));
        });
}

/// Row index of the "cursor here >" line, derived from the fixed layout above.
const fn cursor_row_of_cursor_line() -> u16 {
    // headings + content rows counted in order of construction
    1 + 1 // section 1
    + 1 + 8 // section 2
    + 1 + 4 // section 3
    + 1 + 6 + 1 // section 4
    + 1 + 4 + 2 // section 5
    + 1 + 1 // section 6
    + 1 + 8 + 1 + 1 // section 7
    + 1 + 6 + 1 // section 8
    + 1 // section 9 heading
}

fn hue(t: f32) -> (f32, f32, f32) {
    let h = t * 6.0;
    let x = 1.0 - (h % 2.0 - 1.0).abs();
    match h as u32 {
        0 => (1.0, x, 0.0),
        1 => (x, 1.0, 0.0),
        2 => (0.0, 1.0, x),
        3 => (0.0, x, 1.0),
        4 => (x, 0.0, 1.0),
        _ => (1.0, 0.0, x),
    }
}
