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
//! The checks use the bundled families with a host-independent font
//! collection.

use super::shaping::is_symbol;
use super::*;
use crate::render::pixel_rect;
use crate::render::{BlinkConfig, CursorConfig, FontFaces, RasterConfig, TerminalSizing};
use crate::scene::{StyleFlags, TerminalCell};
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
    pub(super) geometry: Rect,
    /// Index of the glyph among the scene's placed glyphs.
    pub(super) placed: usize,
    /// Horizontal shift applied on top of the run's own placement.
    pub(super) shift: f32,
}

/// What the probe reads from the last full scene: the glyphs it placed,
/// the quads it emitted for each placed glyph (in placement order), and all
/// of its quads.
#[derive(Clone, Default)]
pub(super) struct ProbeRecord {
    pub(super) glyphs: Vec<ProbeGlyph>,
    pub(super) emitted: Vec<std::ops::Range<usize>>,
    pub(super) quads: Vec<QuadInstance>,
    pub(super) target: Vec2,
}

impl ProbeRecord {
    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }

    /// The pixel rectangles of the quads the scene emitted for `glyph`,
    /// after clipping.
    fn pieces(&self, glyph: &ProbeGlyph) -> Vec<Rect> {
        let size = self.target;
        self.emitted[glyph.placed]
            .clone()
            .map(|index| {
                let [left, top, right, bottom] = self.quads[index].rect();
                pixel_rect(
                    (left + 1.0) / 2.0 * size.x,
                    (1.0 - top) / 2.0 * size.y,
                    (right - left) / 2.0 * size.x,
                    (top - bottom) / 2.0 * size.y,
                )
            })
            .collect()
    }
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

/// The bundled families and their regular, bold, italic and bold italic faces.
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

/// A bundled font file, from the crate's own fonts or the repository's.
pub(super) fn asset(path: &str) -> Vec<u8> {
    let crate_fonts = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/fonts/");
    let repository_fonts = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fonts/");
    std::fs::read(format!("{crate_fonts}{path}"))
        .or_else(|_| std::fs::read(format!("{repository_fonts}{path}")))
        .unwrap_or_else(|error| panic!("{path}: {error}"))
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
        let font = Font::from_bytes(asset(path));
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

/// The faces of the bundled `family`.
pub(super) fn load_faces(app: &mut App, family: &str) -> FontFaces {
    let (dir, [regular, bold, italic, bold_italic]) = BUNDLED
        .into_iter()
        .find(|(dir, _)| *dir == family)
        .unwrap_or_else(|| panic!("bundled family {family}"));
    let mut fonts = app.world_mut().resource_mut::<Assets<Font>>();
    let mut load = |file: &str| -> FontSource {
        fonts
            .add(Font::from_bytes(asset(&format!("{dir}/{file}"))))
            .into()
    };
    FontFaces {
        regular: load(regular),
        bold: Some(load(bold)),
        italic: Some(load(italic)),
        bold_italic: Some(load(bold_italic)),
        synthesize: true,
    }
}

/// One probe configuration: a bundled family at a size, scale and line height.
#[derive(Clone, Debug)]
struct ProbeCase {
    family: &'static str,
    font_size: f32,
    scale: f32,
    line_height: f32,
}

impl ProbeCase {
    fn label(&self) -> String {
        format!(
            "{} {}px {}x lh {}",
            self.family, self.font_size, self.scale, self.line_height
        )
    }
}

/// Measurements of one grapheme.
#[derive(Clone, Debug, Default)]
struct ProbeEntry {
    symbol: String,
    row: u16,
    column: u16,
    class: &'static str,
    /// Inked texels dropped by a clip inside the texture.
    lost_clip: u32,
    /// How far ink reaches past the cell box: top, bottom, left, right.
    outside: [i32; 4],
    /// Ink bounds relative to the cell origin: left, top, right, bottom.
    bounds: [i32; 4],
    shift: f32,
}

struct ProbeResult {
    cell: Vec2,
    entries: Vec<ProbeEntry>,
}

fn run(case: &ProbeCase) -> ProbeResult {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        bevy::asset::AssetPlugin::default(),
        bevy::text::TextPlugin,
        TerminalPlugin,
    ))
    .init_asset::<Image>();
    isolate_fonts(&mut app);
    let faces = load_faces(&mut app, case.family);
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
        .probe = Some(ProbeRecord::default());
    for _ in 0..8 {
        app.update();
    }
    let size = app
        .world()
        .get::<TerminalTexture>(entity)
        .and_then(TerminalTexture::measured)
        .unwrap_or_else(|| panic!("{}: terminal never became ready", case.label()))
        .size();
    let state = app.world().get::<BatchMainState>(entity).unwrap();
    let record = state.scratch.probe.clone().unwrap_or_default();
    let glyphs = &record.glyphs;
    assert!(
        !glyphs.is_empty(),
        "{}: no full scene recorded",
        case.label()
    );
    let cell = state.raster().cell_size;
    let images = app.world().resource::<Assets<Image>>();
    let mut entries: Vec<ProbeEntry> = Vec::new();
    for glyph in glyphs {
        let pieces = record.pieces(glyph);
        let index = match entries.last() {
            Some(last) if last.row == glyph.row && last.column == glyph.column => entries.len() - 1,
            _ => {
                entries.push(ProbeEntry {
                    symbol: glyph.symbol.clone(),
                    row: glyph.row,
                    column: glyph.column,
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
        // The terminal's own atlas lives in the render world; tests see its shadow.
        let (data, width, height) = if glyph.texture == state.glyph_atlas.id {
            (
                &state.glyph_atlas.shadow[..],
                GLYPH_ATLAS_SIZE,
                GLYPH_ATLAS_SIZE,
            )
        } else {
            let image = images.get(glyph.texture).expect("glyph atlas");
            (
                &image.data.as_ref().expect("readable atlas")[..],
                image.width(),
                image.height(),
            )
        };
        let atlas = Vec2::new(width as f32, height as f32);
        let texels = (glyph.uv * Vec4::new(atlas.x, atlas.y, atlas.x, atlas.y)).round();
        let cell_box = pixel_rect(
            f32::from(glyph.column) * cell.x,
            f32::from(glyph.row) * cell.y,
            f32::from(glyph.columns) * cell.x,
            cell.y,
        );
        for ty in texels.y as u32..texels.w as u32 {
            for tx in texels.x as u32..texels.z as u32 {
                let alpha = data[((ty * width + tx) * 4 + 3) as usize];
                if alpha == 0 {
                    continue;
                }
                let x = glyph.geometry.min.x + (tx as f32 - texels.x);
                let y = glyph.geometry.min.y + (ty as f32 - texels.y);
                let in_texture = x >= 0.0 && y >= 0.0 && x < size.x as f32 && y < size.y as f32;
                let in_clip = pieces.iter().any(|piece| {
                    x >= piece.min.x - 0.01
                        && y >= piece.min.y - 0.01
                        && x + 1.0 <= piece.max.x + 0.01
                        && y + 1.0 <= piece.max.y + 0.01
                });
                if in_texture && !in_clip {
                    entry.lost_clip += 1;
                }
                let over = [
                    cell_box.min.y - y,
                    y + 1.0 - cell_box.max.y,
                    cell_box.min.x - x,
                    x + 1.0 - cell_box.max.x,
                ];
                for (side, over) in entry.outside.iter_mut().zip(over) {
                    *side = (*side).max(over as i32);
                }
                let (cx, cy) = ((x - cell_box.min.x) as i32, (y - cell_box.min.y) as i32);
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

/// A few families, sizes, scales and line heights, including a compact row.
#[test]
fn glyph_ink_is_clipped_only_at_the_texture_edges() {
    let cases = [
        ("cascadia-mono", 24.0, 2.0, 0.85),
        ("iosevka-fixed", 18.0, 1.0, 1.0),
        ("hack", 24.0, 1.0, 1.0),
    ];
    for (family, font_size, scale, line_height) in cases {
        let case = ProbeCase {
            family,
            font_size,
            scale,
            line_height,
        };
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
            case.label()
        );
        assert!(
            result
                .entries
                .iter()
                .any(|entry| entry.class == "text" && entry.outside[0] > 0),
            "{}: the samples must include ink above its row",
            case.label()
        );
    }
}

#[test]
fn placement_follows_ghostty_rules() {
    let case = ProbeCase {
        family: "jetbrains-mono",
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
