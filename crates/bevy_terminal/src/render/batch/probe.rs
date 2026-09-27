//! Glyph clipping and placement probe.
//!
//! Drives the real sync system headlessly over sample rows (accents, stacked
//! marks, Arabic and Hebrew, italics, symbols, emoji, CJK, grid graphics and
//! glyphs at the texture's edges), records every glyph quad of the last full
//! scene before clipping, and measures for each grapheme:
//!
//! - ink lost to a clip inside the texture, and ink lost past its edges;
//! - how far its ink reaches outside its cell box;
//! - the horizontal shift the scene applied.
//!
//! Required checks use the bundled families with a host-independent font
//! collection. `cargo test -p bevy_terminal --lib probe -- --ignored` runs the
//! whole matrix and writes TSV reports to `target/glyph-placement-probe`
//! (override with `BEVY_TERMINAL_PROBE_REPORT=<dir>`).
//! `BEVY_TERMINAL_PROBE_FONTS="Label=/path/font.ttf;Menlo"` adds optional host
//! fonts: a path loads that file, a bare name selects a system family (host
//! discovery and fallback are enabled for those runs).

use super::shaping::is_symbol;
use super::*;
use crate::render::PixelGeometry;
use crate::render::{BlinkConfig, CursorConfig, FontFaces, RasterConfig, TerminalSizing};
use crate::scene::TerminalCell;
use std::fmt::Write as _;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// One glyph quad of a full scene, before clipping.
#[derive(Clone, Debug)]
pub(super) struct ProbeGlyph {
    pub(super) symbol: String,
    pub(super) row: u16,
    pub(super) column: u16,
    pub(super) columns: u16,
    pub(super) sprite: bool,
    pub(super) color: bool,
    pub(super) texture: AssetId<Image>,
    pub(super) uv: Vec4,
    /// The placed bitmap.
    pub(super) geometry: PixelGeometry,
    /// The rectangle the scene clipped the bitmap to.
    pub(super) clip: PixelGeometry,
    /// Horizontal shift applied on top of the run's own placement.
    pub(super) shift: f32,
}

impl ProbeGlyph {
    fn class(&self) -> &'static str {
        if self.sprite {
            "sprite"
        } else if self.color {
            "color"
        } else if is_symbol(&self.symbol) {
            "symbol"
        } else {
            "text"
        }
    }
}

const WIDTH: u16 = 72;
const ITALIC: StyleFlags = StyleFlags::ITALIC;
const BOLD_ITALIC: StyleFlags = StyleFlags::BOLD.union(StyleFlags::ITALIC);

/// Sample rows. Accents sit on the first and last rows as well, to separate
/// losses at the texture edge from losses to an interior clip.
fn samples() -> Vec<(String, StyleFlags)> {
    let edge = format!("j{}W", " ".repeat(usize::from(WIDTH) - 2));
    [
        (
            "ÅÉẪ Ǻǻ Z\u{302}\u{303}\u{304} q\u{307}\u{328} ṩ Ệ gjpqy",
            StyleFlags::empty(),
        ),
        (
            "|café| |Ångström| |piñata| ÅÉẪ gjpqy|[]{}()_",
            StyleFlags::empty(),
        ),
        (
            "|cafe\u{301}| |A\u{30a}ngstro\u{308}m| |pin\u{303}ata|",
            StyleFlags::empty(),
        ),
        (
            "|a\u{301}\u{323}| |Z\u{302}\u{303}\u{304}| |x\u{336}| |q\u{307}\u{328}| ǟ ȫ ǭ Ǚ",
            StyleFlags::empty(),
        ),
        ("Καλημέρα κόσμε ΆΈῷ — Привет, мир Йё", StyleFlags::empty()),
        ("|مرحبا بالعالم| |שלום עולם| ع ح م ل ر", StyleFlags::empty()),
        ("fifj WMW Wy fx ƒ @ AT ʃ & || gjpqy ÅÉ", ITALIC),
        ("fifj WMW Wy fx ƒ @ AT ʃ & || gjpqy ÅÉ", BOLD_ITALIC),
        (
            "←↑→↓ ↔↕ ⇐⇑⇒⇓ ∀∂∑√∞≈≠≤≥ ◆◇○●★☆ ♠♣♥♦ ✔x ✘ ⚡ ☀",
            StyleFlags::empty(),
        ),
        (
            "|😀|🚀|❤\u{fe0f}|👍🏽|👩\u{200d}💻|🇯🇵|1\u{fe0f}\u{20e3}| 🧑 🤔 🥰 ⭐ ⌚",
            StyleFlags::empty(),
        ),
        ("|日本語|简体中文|한글| （全角）１２３", StyleFlags::empty()),
        (
            "┌─┬─┐ ╭╮╰╯ ⣿⡿⠿⢿ ░▒▓ \u{e0b0}\u{e0b2} ━┃╋",
            StyleFlags::empty(),
        ),
        (
            "\u{f408} \u{e60b} x\u{f408}x \u{e0c0}\u{e0c0} ☰ x☰x \u{f408}\u{f408}",
            StyleFlags::empty(),
        ),
        (edge.as_str(), StyleFlags::empty()),
        (edge.as_str(), ITALIC),
        ("gjpqy ÅÉ q\u{307}\u{328} ع ح Ẫ", StyleFlags::empty()),
    ]
    .into_iter()
    .map(|(text, flags)| (text.to_owned(), flags))
    .collect()
}

fn fill(surface: &TerminalSurface) {
    let samples = samples();
    surface.update(|update| {
        update.set_cursor_visible(false);
        for (row, (text, flags)) in samples.iter().enumerate() {
            let mut column = 0u16;
            for grapheme in text.graphemes(true) {
                let width = grapheme.width().max(1) as u16;
                let mut cell = if width > 1 {
                    TerminalCell::wide(grapheme, width)
                } else {
                    TerminalCell::new(grapheme)
                };
                cell.style.flags = *flags;
                update.set_cell((column, row as u16), &cell);
                column += width;
            }
        }
    });
}

/// A font for one probe run.
#[derive(Clone, Debug)]
pub(super) enum ProbeFont {
    /// A bundled family directory with its four faces.
    Bundled(&'static str, [&'static str; 4]),
    /// A font file loaded as every face (styles synthesized).
    File(String),
    /// A system family (styles resolved by the host).
    System(String),
}

pub(super) const BUNDLED: [(&str, [&str; 4]); 6] = [
    (
        "iosevka-fixed",
        [
            "IosevkaFixed-Regular.ttf",
            "IosevkaFixed-Bold.ttf",
            "IosevkaFixed-Italic.ttf",
            "IosevkaFixed-BoldItalic.ttf",
        ],
    ),
    (
        "jetbrains-mono",
        [
            "JetBrainsMono-Regular.ttf",
            "JetBrainsMono-Bold.ttf",
            "JetBrainsMono-Italic.ttf",
            "JetBrainsMono-BoldItalic.ttf",
        ],
    ),
    (
        "cascadia-mono",
        [
            "CascadiaMono-Regular.ttf",
            "CascadiaMono-Bold.ttf",
            "CascadiaMono-Italic.ttf",
            "CascadiaMono-BoldItalic.ttf",
        ],
    ),
    (
        "hack",
        [
            "Hack-Regular.ttf",
            "Hack-Bold.ttf",
            "Hack-Italic.ttf",
            "Hack-BoldItalic.ttf",
        ],
    ),
    (
        "dejavu-sans-mono",
        [
            "DejaVuSansMono.ttf",
            "DejaVuSansMono-Bold.ttf",
            "DejaVuSansMono-Oblique.ttf",
            "DejaVuSansMono-BoldOblique.ttf",
        ],
    ),
    (
        "source-code-pro",
        [
            "SourceCodePro-Regular.ttf",
            "SourceCodePro-Bold.ttf",
            "SourceCodePro-It.ttf",
            "SourceCodePro-BoldIt.ttf",
        ],
    ),
];

pub(super) fn asset(path: &str) -> Vec<u8> {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fonts/");
    std::fs::read(format!("{root}{path}")).unwrap_or_else(|error| panic!("{path}: {error}"))
}

/// Replaces the host collection with bundled fallbacks so the probe does not
/// depend on installed fonts.
pub(super) fn isolate_fonts(app: &mut App) {
    let mut cx = app.world_mut().resource_mut::<bevy::text::FontCx>();
    cx.collection = fontique::Collection::new(fontique::CollectionOptions {
        system_fonts: false,
        ..default()
    });
    for (path, scripts) in [
        (
            "fidelity/FidelityCJK.otf",
            &["Hani", "Hang", "Hira", "Kana"][..],
        ),
        (
            "cascadia-mono/CascadiaMono-Regular.ttf",
            &["Arab", "Hebr", "Brai"][..],
        ),
        (
            "dejavu-sans-mono/DejaVuSansMono.ttf",
            &["Latn", "Grek", "Cyrl", "Zyyy", "Zinh"][..],
        ),
        ("fidelity/FidelityColorEmoji.ttf", &[][..]),
        (
            "fidelity/FidelityNerdSymbols.ttf",
            &["Latn", "Zyyy", "Zzzz"][..],
        ),
    ] {
        let font = Font::from_bytes(if path.starts_with("fidelity/FidelityNerd") {
            std::fs::read(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/assets/fonts/fidelity/FidelityNerdSymbols.ttf"
            ))
            .unwrap()
        } else {
            asset(path)
        });
        let families = cx.collection.register_fonts(font.data, None);
        for script in scripts {
            cx.collection.append_fallbacks(
                fontique::Script::from_str_unchecked(script),
                std::iter::once(families[0].0),
            );
        }
    }
    cx.set_emoji_family("Fidelity Color Emoji")
        .expect("bundled emoji family");
}

pub(super) fn load_faces(app: &mut App, font: &ProbeFont) -> FontFaces {
    let mut fonts = app.world_mut().resource_mut::<Assets<Font>>();
    match font {
        ProbeFont::Bundled(dir, files) => {
            let mut load = |file: &str| -> FontSource {
                fonts
                    .add(Font::from_bytes(asset(&format!("{dir}/{file}"))))
                    .into()
            };
            FontFaces {
                regular: load(files[0]),
                bold: Some(load(files[1])),
                italic: Some(load(files[2])),
                bold_italic: Some(load(files[3])),
                synthesize: true,
            }
        }
        ProbeFont::File(path) => FontFaces::regular(fonts.add(Font::from_bytes(
            std::fs::read(path).unwrap_or_else(|error| panic!("{path}: {error}")),
        ))),
        ProbeFont::System(name) => FontFaces::regular(FontSource::Family(name.clone().into())),
    }
}

/// One probe configuration.
#[derive(Clone, Debug)]
pub(super) struct ProbeCase {
    pub(super) label: String,
    pub(super) font: ProbeFont,
    pub(super) font_size: f32,
    pub(super) scale: f32,
    pub(super) line_height: f32,
}

/// Measurements of one grapheme.
#[derive(Clone, Debug, Default)]
pub(super) struct ProbeEntry {
    pub(super) symbol: String,
    pub(super) row: u16,
    pub(super) column: u16,
    pub(super) columns: u16,
    pub(super) class: &'static str,
    /// Inked texels.
    pub(super) ink: u32,
    /// Inked texels dropped by a clip inside the texture.
    pub(super) lost_clip: u32,
    /// Inked texels past the texture's edges.
    pub(super) lost_edge: u32,
    /// How far ink reaches past the cell box: top, bottom, left, right.
    pub(super) outside: [i32; 4],
    /// Ink bounds relative to the cell origin: left, top, right, bottom.
    pub(super) bounds: [i32; 4],
    pub(super) shift: f32,
}

pub(super) struct ProbeResult {
    pub(super) cell: Vec2,
    pub(super) entries: Vec<ProbeEntry>,
}

pub(super) fn run(case: &ProbeCase) -> ProbeResult {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        bevy::asset::AssetPlugin::default(),
        bevy::text::TextPlugin,
        TerminalPlugin,
    ))
    .init_asset::<Image>();
    if !matches!(case.font, ProbeFont::System(_)) {
        isolate_fonts(&mut app);
    }
    let faces = load_faces(&mut app, &case.font);
    let rows = samples().len() as u16;
    let surface = TerminalSurface::new((WIDTH, rows));
    fill(&surface);
    let entity = app
        .world_mut()
        .spawn((
            TerminalRenderer::new(surface),
            TerminalRenderConfig {
                sizing: TerminalSizing::FromFont {
                    font_size: case.font_size,
                    line_height: case.line_height,
                },
                font: faces,
                raster: RasterConfig {
                    scale: case.scale,
                    ..default()
                },
                blink: BlinkConfig::NONE,
                cursor: CursorConfig {
                    blink_hz: None,
                    ..default()
                },
                ..default()
            },
        ))
        .id();
    app.update();
    app.world_mut()
        .get_mut::<BatchMainState>(entity)
        .expect("initialized terminal")
        .scratch
        .probe = Some(Vec::new());
    for _ in 0..8 {
        app.update();
    }
    let size = app
        .world()
        .get::<TerminalTexture>(entity)
        .and_then(TerminalTexture::measured)
        .unwrap_or_else(|| panic!("{}: terminal never became ready", case.label))
        .size();
    let state = app.world().get::<BatchMainState>(entity).unwrap();
    let glyphs = state.scratch.probe.clone().unwrap_or_default();
    assert!(!glyphs.is_empty(), "{}: no full scene recorded", case.label);
    let cell = state.raster_config.cell_size;
    let images = app.world().resource::<Assets<Image>>();
    let mut entries: Vec<ProbeEntry> = Vec::new();
    for glyph in &glyphs {
        let index = match entries.last() {
            Some(last) if last.row == glyph.row && last.column == glyph.column => entries.len() - 1,
            _ => {
                entries.push(ProbeEntry {
                    symbol: glyph.symbol.clone(),
                    row: glyph.row,
                    column: glyph.column,
                    columns: glyph.columns,
                    class: glyph.class(),
                    outside: [i32::MIN; 4],
                    bounds: [i32::MAX, i32::MAX, i32::MIN, i32::MIN],
                    shift: glyph.shift,
                    ..default()
                });
                entries.len() - 1
            }
        };
        let entry = &mut entries[index];
        let image = images.get(glyph.texture).expect("glyph atlas");
        let data = image.data.as_ref().expect("readable atlas");
        let atlas = Vec2::new(image.width() as f32, image.height() as f32);
        let texels = (glyph.uv * Vec4::new(atlas.x, atlas.y, atlas.x, atlas.y)).round();
        let cell_box = PixelGeometry {
            x: f32::from(glyph.column) * cell.x,
            y: f32::from(glyph.row) * cell.y,
            width: f32::from(glyph.columns) * cell.x,
            height: cell.y,
        };
        for ty in texels.y as u32..texels.w as u32 {
            for tx in texels.x as u32..texels.z as u32 {
                let alpha = data[((ty * image.width() + tx) * 4 + 3) as usize];
                if alpha == 0 {
                    continue;
                }
                let x = glyph.geometry.x + (tx as f32 - texels.x);
                let y = glyph.geometry.y + (ty as f32 - texels.y);
                entry.ink += 1;
                let in_texture = x >= 0.0 && y >= 0.0 && x < size.x as f32 && y < size.y as f32;
                let clip = glyph.clip;
                let in_clip = x >= clip.x
                    && y >= clip.y
                    && x < clip.x + clip.width
                    && y < clip.y + clip.height;
                if !in_texture {
                    entry.lost_edge += 1;
                } else if !in_clip {
                    entry.lost_clip += 1;
                }
                let over = [
                    cell_box.y - y,
                    y + 1.0 - (cell_box.y + cell_box.height),
                    cell_box.x - x,
                    x + 1.0 - (cell_box.x + cell_box.width),
                ];
                for (side, over) in entry.outside.iter_mut().zip(over) {
                    *side = (*side).max(over as i32);
                }
                let (cx, cy) = ((x - cell_box.x) as i32, (y - cell_box.y) as i32);
                entry.bounds = [
                    entry.bounds[0].min(cx),
                    entry.bounds[1].min(cy),
                    entry.bounds[2].max(cx + 1),
                    entry.bounds[3].max(cy + 1),
                ];
            }
        }
    }
    for entry in &mut entries {
        for side in &mut entry.outside {
            *side = (*side).max(0);
        }
    }
    ProbeResult { cell, entries }
}

/// The full matrix: bundled families at Ratty-like sizes, plus optional
/// host fonts from `BEVY_TERMINAL_PROBE_FONTS`.
pub(super) fn matrix() -> Vec<ProbeCase> {
    let mut fonts: Vec<(String, ProbeFont)> = BUNDLED
        .iter()
        .map(|(dir, files)| (dir.to_string(), ProbeFont::Bundled(dir, *files)))
        .collect();
    if let Ok(extra) = std::env::var("BEVY_TERMINAL_PROBE_FONTS") {
        for spec in extra.split(';').filter(|spec| !spec.is_empty()) {
            fonts.push(match spec.split_once('=') {
                Some((label, path)) => (label.to_owned(), ProbeFont::File(path.to_owned())),
                None => (spec.to_owned(), ProbeFont::System(spec.to_owned())),
            });
        }
    }
    let mut cases = Vec::new();
    for (name, font) in fonts {
        for font_size in [18.0, 24.0] {
            for scale in [1.0, 2.0] {
                for line_height in [1.0, 0.85] {
                    cases.push(ProbeCase {
                        label: format!("{name}\t{font_size}\t{scale}\t{line_height}"),
                        font: font.clone(),
                        font_size,
                        scale,
                        line_height,
                    });
                }
            }
        }
    }
    cases
}

pub(super) const REPORT_HEADER: &str = "font\tsize\tscale\tline_height\tcell\tsymbol\trow\tcolumn\tcolumns\tclass\tink\tlost_clip\tlost_edge\tout_top\tout_bottom\tout_left\tout_right\tshift_x\tink_left\tink_top\tink_right\tink_bottom\n";
pub(super) const SUMMARY_HEADER: &str = "font\tsize\tscale\tline_height\tcell\tgraphemes\tclipped\tlost_clip\tedge_clipped\tlost_edge\ttext_shifted\ttext_shift_px\toverflowing\n";

/// Appends the notable entries of one run and its summary line.
pub(super) fn report(
    case: &ProbeCase,
    result: &ProbeResult,
    report: &mut String,
    summary: &mut String,
) {
    let cell = format!("{}x{}", result.cell.x, result.cell.y);
    let mut clipped = 0;
    let mut lost_clip = 0;
    let mut edge_clipped = 0;
    let mut lost_edge = 0;
    let mut shifted = 0;
    let mut shift_px = 0.0;
    let mut overflowing = 0;
    for entry in &result.entries {
        let outside = entry.outside.iter().any(|side| *side > 0);
        clipped += usize::from(entry.lost_clip > 0);
        lost_clip += entry.lost_clip;
        edge_clipped += usize::from(entry.lost_edge > 0);
        lost_edge += entry.lost_edge;
        overflowing += usize::from(outside);
        if entry.class == "text" && entry.shift != 0.0 {
            shifted += 1;
            shift_px += entry.shift.abs();
        }
        if entry.lost_clip == 0
            && entry.lost_edge == 0
            && !outside
            && entry.shift == 0.0
            && std::env::var_os("BEVY_TERMINAL_PROBE_ALL").is_none()
        {
            continue;
        }
        let [top, bottom, left, right] = entry.outside;
        writeln!(
            report,
            "{}\t{cell}\t{:?}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{top}\t{bottom}\t{left}\t{right}\t{}\t{}",
            case.label,
            entry.symbol,
            entry.row,
            entry.column,
            entry.columns,
            entry.class,
            entry.ink,
            entry.lost_clip,
            entry.lost_edge,
            entry.shift,
            entry.bounds.map(|v| v.to_string()).join("\t"),
        )
        .unwrap();
    }
    writeln!(
        summary,
        "{}\t{cell}\t{}\t{clipped}\t{lost_clip}\t{edge_clipped}\t{lost_edge}\t{shifted}\t{shift_px}\t{overflowing}",
        case.label,
        result.entries.len()
    )
    .unwrap();
}

#[test]
#[ignore = "full placement matrix; writes TSV reports"]
fn glyph_placement_probe_report() {
    let directory = std::env::var("BEVY_TERMINAL_PROBE_REPORT").unwrap_or_else(|_| {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../target/glyph-placement-probe"
        )
        .into()
    });
    std::fs::create_dir_all(&directory).unwrap();
    let mut report = String::from(REPORT_HEADER);
    let mut summary = String::from(SUMMARY_HEADER);
    for case in matrix() {
        let result = run(&case);
        self::report(&case, &result, &mut report, &mut summary);
    }
    std::fs::write(format!("{directory}/entries.tsv"), &report).unwrap();
    std::fs::write(format!("{directory}/summary.tsv"), &summary).unwrap();
    print!("{summary}");
}

/// Probe configurations every test run checks: a few families, sizes, scales
/// and line heights, including a compact row.
pub(super) fn required_cases() -> Vec<ProbeCase> {
    [
        ("cascadia-mono", 24.0, 2.0, 0.85),
        ("iosevka-fixed", 18.0, 1.0, 1.0),
        ("hack", 24.0, 1.0, 1.0),
    ]
    .into_iter()
    .map(|(dir, font_size, scale, line_height)| {
        let files = BUNDLED.iter().find(|(name, _)| *name == dir).unwrap().1;
        ProbeCase {
            label: format!("{dir} {font_size}px {scale}x lh {line_height}"),
            font: ProbeFont::Bundled(dir, files),
            font_size,
            scale,
            line_height,
        }
    })
    .collect()
}

#[test]
fn glyph_ink_is_clipped_only_at_the_texture_edges() {
    for case in required_cases() {
        let result = run(&case);
        let clipped: Vec<_> = result
            .entries
            .iter()
            .filter(|entry| entry.class != "sprite" && entry.lost_clip > 0)
            .map(|entry| format!("{:?} at ({},{})", entry.symbol, entry.column, entry.row))
            .collect();
        assert!(
            clipped.is_empty(),
            "{}: ink clipped inside the texture: {clipped:?}",
            case.label
        );
        assert!(
            result
                .entries
                .iter()
                .any(|entry| entry.class == "text" && entry.outside[0] > 0),
            "{}: the samples must include ink above its row",
            case.label
        );
    }
}

#[test]
fn placement_follows_ghostty_rules() {
    let files = BUNDLED
        .iter()
        .find(|(dir, _)| *dir == "jetbrains-mono")
        .unwrap()
        .1;
    let case = ProbeCase {
        label: "jetbrains-mono 24px 2x".into(),
        font: ProbeFont::Bundled("jetbrains-mono", files),
        font_size: 24.0,
        scale: 2.0,
        line_height: 1.0,
    };
    let result = run(&case);
    let cell = result.cell.as_ivec2();
    let last = i32::from(WIDTH) - 1;
    let find = |symbol: &str, row: u16, column: u16| {
        result
            .entries
            .iter()
            .find(|entry| entry.symbol == symbol && entry.row == row && entry.column == column)
            .unwrap_or_else(|| panic!("{symbol:?} at ({column},{row})"))
    };
    // Ordinary text keeps its bearings; only the texture's edges push it in.
    for entry in result.entries.iter().filter(|entry| entry.class == "text") {
        let at_edge = entry.column == 0 || i32::from(entry.column) == last;
        assert!(
            entry.shift == 0.0 || at_edge,
            "{:?} at ({},{}) shifted by {}",
            entry.symbol,
            entry.column,
            entry.row,
            entry.shift
        );
    }
    assert!(find("j", 13, 0).shift > 0.0 || find("j", 13, 0).bounds[0] >= 0);
    assert!(find("W", 14, WIDTH - 1).bounds[2] <= cell.x);
    // Colour emoji cover two cells with 2.5% side padding, centered.
    let rocket = find("🚀", 9, 4);
    let width = rocket.bounds[2] - rocket.bounds[0];
    assert!(
        width >= cell.x * 2 * 9 / 10 && width <= cell.x * 2,
        "🚀 is {width} px wide over two {} px cells",
        cell.x
    );
    assert!(rocket.bounds[0] > 0 && rocket.bounds[2] < cell.x * 2);
    // Nerd Fonts: `fit_cover1` fits one cell; a blank neighbour leaves the
    // size alone and left-aligns; stretched glyphs cover their cells exactly.
    let icon = find("\u{f408}", 12, 5);
    assert!(
        icon.bounds[0] >= 0 && icon.bounds[2] <= cell.x,
        "{:?}",
        icon.bounds
    );
    assert!(icon.bounds[2] - icon.bounds[0] >= cell.x - 2);
    let spread = find("\u{f408}", 12, 0);
    assert_eq!(spread.bounds[0], 0);
    assert!(spread.bounds[2] > cell.x, "{:?}", spread.bounds);
    assert_eq!(find("\u{e0c0}", 12, 8).bounds, [0, 0, cell.x, cell.y]);
    assert_eq!(find("\u{e0c0}", 12, 9).bounds, [0, 0, 2 * cell.x, cell.y]);
    // Symbols fit the face box of their cells.
    let arrow = find("←", 8, 0);
    assert!(
        arrow.bounds[0] >= 0 && arrow.bounds[2] <= cell.x,
        "{:?}",
        arrow.bounds
    );
}
