//! Fuzzes the renderer's main-world pipeline in a headless Bevy app (no GPU):
//! surface writes, resizes, configuration changes (including invalid values),
//! font swaps and removals, terminal spawns/despawns and shared surfaces, with
//! frames advanced in between. After the input ends the app settles and the
//! documented guarantees are checked: status per configuration, measured
//! geometry consistent with the surface and image, and idle terminals doing no
//! work.

#![no_main]

use std::collections::HashMap;
use std::time::Duration;

use arbitrary::Arbitrary;
use bevy::prelude::*;
use bevy::text::{FontHinting, FontSource};
use bevy_terminal::bevy;
use bevy_terminal::prelude::*;
use bevy_terminal_ratatui::bevy_terminal;
use libfuzzer_sys::fuzz_target;

const MAX_OPS: usize = 32;
const MAX_FRAMES: u32 = 20;
const MAX_TERMINALS: usize = 4;
/// The texture limit without a render device.
const TEXTURE_LIMIT: u32 = 8192;

macro_rules! font {
    ($path:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../crates/bevy_terminal/assets/fonts/",
            $path
        ))
    };
}

#[derive(Arbitrary, Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum FontChoice {
    JetBrainsRegular,
    JetBrainsBold,
    JetBrainsItalic,
    JetBrainsBoldItalic,
    Hack,
    Cascadia,
    DejaVu,
    Cjk,
    Emoji,
    Nerd,
    Wide,
    /// A handle whose asset never arrives.
    Reserved,
    /// Bytes that are not a font.
    Garbage,
}

impl FontChoice {
    fn bytes(self) -> Option<&'static [u8]> {
        Some(match self {
            Self::JetBrainsRegular => font!("jetbrains-mono/JetBrainsMono-Regular.ttf"),
            Self::JetBrainsBold => font!("jetbrains-mono/JetBrainsMono-Bold.ttf"),
            Self::JetBrainsItalic => font!("jetbrains-mono/JetBrainsMono-Italic.ttf"),
            Self::JetBrainsBoldItalic => font!("jetbrains-mono/JetBrainsMono-BoldItalic.ttf"),
            Self::Hack => font!("hack/Hack-Regular.ttf"),
            Self::Cascadia => font!("cascadia-mono/CascadiaMono-Regular.ttf"),
            Self::DejaVu => font!("dejavu-sans-mono/DejaVuSansMono.ttf"),
            Self::Cjk => font!("fidelity/FidelityCJK.otf"),
            Self::Emoji => font!("fidelity/FidelityColorEmoji.ttf"),
            Self::Nerd => font!("fidelity/FidelityNerdSymbols.ttf"),
            Self::Wide => font!("fidelity/FidelityWideSymbols.ttf"),
            Self::Garbage => b"\x00\x01\x00\x00garbage, not a font at all",
            Self::Reserved => return None,
        })
    }
}

#[derive(Arbitrary, Clone, Debug)]
enum FaceSource {
    Font(FontChoice),
    Family(u8),
    Monospace,
}

#[derive(Arbitrary, Clone, Copy, Debug)]
enum Float {
    Small(u8),
    Typical(u8),
    Big(u16),
    Zero,
    Negative(u8),
    Nan,
    Inf,
    NegInf,
    Tiny,
    Huge,
    Raw(f32),
}

impl Float {
    fn value(self) -> f32 {
        match self {
            Self::Small(v) => 0.25 + f32::from(v % 16) * 0.25,
            Self::Typical(v) => 4.0 + f32::from(v) / 8.0,
            Self::Big(v) => 32.0 + f32::from(v % 4000),
            Self::Zero => 0.0,
            Self::Negative(v) => -1.0 - f32::from(v),
            Self::Nan => f32::NAN,
            Self::Inf => f32::INFINITY,
            Self::NegInf => f32::NEG_INFINITY,
            Self::Tiny => f32::MIN_POSITIVE,
            Self::Huge => 1.0e9,
            Self::Raw(v) => v,
        }
    }
}

#[derive(Arbitrary, Clone, Copy, Debug)]
enum Sizing {
    FromFont {
        font_size: Float,
        line_height: Float,
    },
    FitCellWidth(Float, Float),
    Fixed {
        cell: (Float, Float),
        font_size: Float,
    },
    Default,
}

impl Sizing {
    fn value(self) -> TerminalSizing {
        match self {
            Self::FromFont {
                font_size,
                line_height,
            } => TerminalSizing::FromFont {
                font_size: font_size.value(),
                line_height: line_height.value(),
            },
            Self::FitCellWidth(x, y) => {
                TerminalSizing::FitCellWidth(Vec2::new(x.value(), y.value()))
            }
            Self::Fixed { cell, font_size } => TerminalSizing::Fixed {
                cell_size: Vec2::new(cell.0.value(), cell.1.value()),
                font_size: font_size.value(),
            },
            Self::Default => TerminalSizing::default(),
        }
    }
}

#[derive(Arbitrary, Clone, Copy, Debug)]
enum Hz {
    Off,
    On(Float),
}

impl Hz {
    fn value(self) -> Option<f32> {
        match self {
            Self::Off => None,
            Self::On(value) => Some(value.value()),
        }
    }
}

#[derive(Arbitrary, Clone, Copy, Debug)]
enum ColorSpec {
    Srgb(u8, u8, u8, u8),
    Weird(Float, Float, Float, Float),
}

impl ColorSpec {
    fn value(self) -> Color {
        match self {
            Self::Srgb(r, g, b, a) => Color::srgba_u8(r, g, b, a),
            Self::Weird(r, g, b, a) => {
                Color::linear_rgba(r.value(), g.value(), b.value(), a.value())
            }
        }
    }
}

#[derive(Arbitrary, Clone, Debug)]
enum ConfigChange {
    Sizing(Sizing),
    Scale(Float),
    Hinting(bool),
    Cursor {
        style: u8,
        color: ColorSpec,
        blink: Hz,
    },
    Blink {
        slow: Hz,
        rapid: Hz,
    },
    Theme {
        slot: u8,
        color: ColorSpec,
    },
    Synthesize(bool),
    Face {
        slot: u8,
        source: Option<FaceSource>,
    },
    /// Touch the component without changing it.
    Touch,
}

#[derive(Arbitrary, Clone, Copy, Debug)]
enum Dim {
    Small(u8),
    Zero,
    One,
    Large(u16),
}

impl Dim {
    fn value(self, small_max: u16) -> u16 {
        match self {
            Self::Small(v) => u16::from(v) % (small_max + 1),
            Self::Zero => 0,
            Self::One => 1,
            Self::Large(v) => v % 600,
        }
    }
}

#[derive(Arbitrary, Clone, Copy, Debug)]
enum Glyph {
    Ascii(u8),
    Cjk(u16),
    Emoji(u8),
    Combining(u8, u8, u8),
    Grid(u8),
    Braille(u8),
    Legacy(u8),
    Nerd(u16),
    PrivateUse(u16),
    Unassigned(u8),
    Raw(u32),
    Empty,
}

impl Glyph {
    fn text(self) -> String {
        let ch = |code: u32| char::from_u32(code).unwrap_or('?');
        match self {
            Self::Ascii(v) => ch(u32::from(v % 0x7f).max(0x20)).to_string(),
            Self::Cjk(v) => match v % 4 {
                0 => ch(0x4E00 + u32::from(v) % 0x5200).to_string(),
                1 => ["界", "中", "한", "あ", "カ", "語"][usize::from(v / 4) % 6].to_string(),
                2 => ch(0xAC00 + u32::from(v) % 0x2BA4).to_string(),
                _ => ch(0xFF01 + u32::from(v) % 0x5E).to_string(),
            },
            Self::Emoji(v) => [
                "😀",
                "👍🏽",
                "👨‍👩‍👧",
                "🏳️‍🌈",
                "❤️",
                "🇺🇸",
                "🧑‍💻",
                "☺",
                "✅",
                "🔥",
                "#️⃣",
                "👩🏾‍🚀",
            ][usize::from(v) % 12]
                .to_string(),
            Self::Combining(base, a, count) => {
                let mut text = ch(u32::from(base % 26) + 0x61).to_string();
                for i in 0..(count % 6) {
                    text.push(ch(0x300 + (u32::from(a) + u32::from(i) * 7) % 0x70));
                }
                text
            }
            Self::Grid(v) => ch(0x2500 + u32::from(v) % 0xA0).to_string(),
            Self::Braille(v) => ch(0x2800 + u32::from(v)).to_string(),
            Self::Legacy(v) => ch(0x1FB00 + u32::from(v)).to_string(),
            Self::Nerd(v) => match v % 3 {
                0 => ch(0xE0A0 + u32::from(v) % 0x60),
                1 => ch(0xF000 + u32::from(v) % 0x900),
                _ => ch(0xF0001 + u32::from(v) % 0x1000),
            }
            .to_string(),
            Self::PrivateUse(v) => ch(0xE000 + u32::from(v) % 0x1900).to_string(),
            Self::Unassigned(v) => {
                ch([0x0378, 0x0530, 0x2FFFF, 0xE0080, 0x10FFFF, 0xFFFE][usize::from(v) % 6])
                    .to_string()
            }
            Self::Raw(v) => ch(v % 0x110000).to_string(),
            Self::Empty => String::new(),
        }
    }
}

#[derive(Arbitrary, Clone, Copy, Debug)]
enum ColorPick {
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

impl ColorPick {
    fn value(self) -> TerminalColor {
        match self {
            Self::Default => TerminalColor::Default,
            Self::Indexed(i) => TerminalColor::Indexed(i),
            Self::Rgb(r, g, b) => TerminalColor::Rgb(r, g, b),
        }
    }
}

#[derive(Arbitrary, Clone, Copy, Debug)]
struct Style {
    fg: ColorPick,
    bg: ColorPick,
    underline: ColorPick,
    flags: u16,
}

impl Style {
    fn value(self) -> TerminalStyle {
        let mut style = TerminalStyle::new()
            .fg(self.fg.value())
            .bg(self.bg.value())
            .underline_color(self.underline.value());
        style.flags = StyleFlags::from_bits_truncate(self.flags);
        style
    }
}

#[derive(Arbitrary, Clone, Debug)]
enum Op {
    Write {
        surface: u8,
        x: u8,
        y: u8,
        glyphs: Vec<(Glyph, u8, Style)>,
    },
    Fill {
        surface: u8,
        glyph: Glyph,
        style: Style,
    },
    Clear {
        surface: u8,
        row: Option<u8>,
    },
    Scroll {
        surface: u8,
        up: bool,
        start: u8,
        end: u8,
        count: u8,
    },
    Cursor {
        surface: u8,
        x: u8,
        y: u8,
        visible: bool,
    },
    Resize {
        surface: u8,
        width: Dim,
        height: Dim,
    },
    Config {
        terminal: u8,
        change: ConfigChange,
    },
    RemoveFont(FontChoice),
    RestoreFont(FontChoice),
    Spawn {
        surface: u8,
        changes: Vec<ConfigChange>,
    },
    Despawn {
        terminal: u8,
    },
    RemoveRenderer {
        terminal: u8,
    },
    SetSurface {
        terminal: u8,
        surface: u8,
    },
    NewSurface {
        width: Dim,
        height: Dim,
    },
    Frames {
        count: u8,
        millis: u16,
    },
}

#[derive(Arbitrary, Debug)]
struct Input {
    width: Dim,
    height: Dim,
    regular: FontChoice,
    initial: Vec<ConfigChange>,
    ops: Vec<Op>,
}

#[derive(Resource, Default)]
struct Changes {
    textures: usize,
    stats: usize,
}

fn count_changes(
    textures: Query<(), Changed<TerminalTexture>>,
    stats: Query<(), Changed<TerminalStats>>,
    mut changes: ResMut<Changes>,
) {
    changes.textures += textures.iter().count();
    changes.stats += stats.iter().count();
}

struct Harness {
    app: App,
    fonts: HashMap<FontChoice, Handle<Font>>,
    /// Font assets removed from `Assets<Font>` (their handles stay alive).
    removed: Vec<FontChoice>,
    surfaces: Vec<TerminalSurface>,
    /// Live entities with their renderer and the image handle id they got.
    terminals: Vec<(Entity, AssetId<Image>)>,
    /// Images of terminals that stopped rendering; they must be released.
    released: Vec<AssetId<Image>>,
    frames: u32,
}

impl Harness {
    fn new() -> Self {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins.build().disable::<bevy::time::TimePlugin>(),
            bevy::asset::AssetPlugin::default(),
            bevy::text::TextPlugin,
        ))
        .init_asset::<Image>()
        .init_resource::<Time>()
        .init_resource::<Changes>()
        .add_plugins(TerminalPlugin)
        .add_systems(Update, count_changes.after(TerminalSystems::Sync));
        Self {
            app,
            fonts: HashMap::new(),
            removed: Vec::new(),
            surfaces: Vec::new(),
            terminals: Vec::new(),
            released: Vec::new(),
            frames: 0,
        }
    }

    fn font(&mut self, choice: FontChoice) -> Handle<Font> {
        if let Some(handle) = self.fonts.get(&choice) {
            return handle.clone();
        }
        let mut assets = self.app.world_mut().resource_mut::<Assets<Font>>();
        let handle = match choice.bytes() {
            Some(bytes) => assets.add(Font::from_bytes(bytes.to_vec())),
            None => assets.reserve_handle(),
        };
        self.fonts.insert(choice, handle.clone());
        handle
    }

    fn face(&mut self, source: &FaceSource) -> FontSource {
        match source {
            FaceSource::Font(choice) => FontSource::Handle(self.font(*choice)),
            FaceSource::Family(v) => FontSource::family(
                ["JetBrains Mono", "Hack", "DejaVu Sans Mono", "Nope", ""][usize::from(*v) % 5],
            ),
            FaceSource::Monospace => FontSource::monospace(),
        }
    }

    fn surface(&self, index: u8) -> Option<TerminalSurface> {
        if self.surfaces.is_empty() {
            return None;
        }
        self.surfaces
            .get(usize::from(index) % self.surfaces.len())
            .cloned()
    }

    fn terminal(&self, index: u8) -> Option<Entity> {
        if self.terminals.is_empty() {
            return None;
        }
        self.terminals
            .get(usize::from(index) % self.terminals.len())
            .map(|(entity, _)| *entity)
    }

    fn apply_change(&mut self, config: &mut TerminalRenderConfig, change: &ConfigChange) {
        match change {
            ConfigChange::Sizing(sizing) => config.sizing = sizing.value(),
            ConfigChange::Scale(scale) => config.raster.scale = scale.value(),
            ConfigChange::Hinting(on) => {
                config.raster.hinting = if *on {
                    FontHinting::Enabled
                } else {
                    FontHinting::Disabled
                }
            }
            ConfigChange::Cursor {
                style,
                color,
                blink,
            } => {
                config.cursor.style = match style % 3 {
                    0 => CursorStyle::Block,
                    1 => CursorStyle::Bar,
                    _ => CursorStyle::Underline,
                };
                config.cursor.color = color.value();
                config.cursor.blink_hz = blink.value();
            }
            ConfigChange::Blink { slow, rapid } => {
                config.blink = BlinkConfig {
                    slow_hz: slow.value(),
                    rapid_hz: rapid.value(),
                }
            }
            ConfigChange::Theme { slot, color } => match slot % 18 {
                16 => config.theme.foreground = color.value(),
                17 => config.theme.background = color.value(),
                i => config.theme.ansi[usize::from(i)] = color.value(),
            },
            ConfigChange::Synthesize(on) => config.font.synthesize = *on,
            ConfigChange::Face { slot, source } => {
                let source = source.as_ref().map(|source| self.face(source));
                match (slot % 4, source) {
                    (0, Some(source)) => config.font.regular = source,
                    (0, None) => {}
                    (1, source) => config.font.bold = source,
                    (2, source) => config.font.italic = source,
                    (_, source) => config.font.bold_italic = source,
                }
            }
            ConfigChange::Touch => {}
        }
    }

    fn spawn(&mut self, surface: TerminalSurface, regular: FontChoice, changes: &[ConfigChange]) {
        if self.terminals.len() >= MAX_TERMINALS {
            return;
        }
        let mut config = TerminalRenderConfig {
            font: FontFaces::regular(self.font(regular)),
            blink: BlinkConfig::NONE,
            ..default()
        };
        config.cursor.blink_hz = None;
        for change in changes.iter().take(8) {
            self.apply_change(&mut config, change);
        }
        let entity = self
            .app
            .world_mut()
            .spawn((TerminalRenderer::new(surface), config))
            .id();
        let image = self.image_of(entity);
        self.terminals.push((entity, image));
    }

    fn image_of(&self, entity: Entity) -> AssetId<Image> {
        // The texture is attached as soon as the renderer is added.
        let texture = self
            .app
            .world()
            .get::<TerminalTexture>(entity)
            .expect("TerminalTexture attached when the renderer is added");
        texture.image().id()
    }

    fn stop(&mut self, index: u8, despawn: bool) {
        let Some(entity) = self.terminal(index) else {
            return;
        };
        let position = self
            .terminals
            .iter()
            .position(|(e, _)| *e == entity)
            .unwrap();
        let (entity, image) = self.terminals.remove(position);
        if despawn {
            self.app.world_mut().despawn(entity);
        } else {
            self.app
                .world_mut()
                .entity_mut(entity)
                .remove::<TerminalRenderer>();
            let world = self.app.world();
            assert!(world.get::<TerminalTexture>(entity).is_none());
            assert!(world.get::<TerminalStats>(entity).is_none());
        }
        self.released.push(image);
    }

    fn update(&mut self, millis: u16) {
        self.app
            .world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_millis(u64::from(millis)));
        self.app.update();
        self.frames += 1;
        self.check_frame();
    }

    /// Checks that hold after every update.
    fn check_frame(&mut self) {
        let world = self.app.world();
        let images = world.resource::<Assets<Image>>();
        for &(entity, image) in &self.terminals {
            let renderer = world.get::<TerminalRenderer>(entity).unwrap();
            let texture = world
                .get::<TerminalTexture>(entity)
                .expect("texture on a live renderer");
            assert!(world.get::<TerminalStats>(entity).is_some());
            let config = world.get::<TerminalRenderConfig>(entity).unwrap();
            // The handle is stable for the terminal's lifetime.
            assert_eq!(texture.image().id(), image, "texture handle changed");
            assert!(images.get(image).is_some(), "terminal image missing");
            let Some(geometry) = texture.measured() else {
                assert!(texture.status() != TerminalStatus::Ready);
                continue;
            };
            check_geometry(renderer.surface(), config, geometry);
            let image_size = images.get(image).unwrap().size();
            assert_eq!(
                image_size,
                geometry.size(),
                "image asset size differs from measured size"
            );
        }
    }

    fn run(&mut self, input: &Input) {
        let size = (input.width.value(40), input.height.value(12));
        self.surfaces.push(TerminalSurface::new(size));
        let surface = self.surfaces[0].clone();
        self.spawn(surface, input.regular, &input.initial);
        for op in input.ops.iter().take(MAX_OPS) {
            if self.frames >= MAX_FRAMES {
                break;
            }
            self.apply(op);
        }
        self.settle();
    }

    fn apply(&mut self, op: &Op) {
        match op {
            Op::Write {
                surface,
                x,
                y,
                glyphs,
            } => {
                let Some(surface) = self.surface(*surface) else {
                    return;
                };
                surface.update(|update| {
                    let mut x = u16::from(*x);
                    let y = u16::from(*y) % update.size().height.max(1);
                    for (glyph, columns, style) in glyphs.iter().take(64) {
                        let text = glyph.text();
                        let columns = u16::from(*columns % 5);
                        let cell = if columns > 1 {
                            TerminalCell::wide(&text, columns)
                        } else {
                            TerminalCell::new(&text)
                        }
                        .with_style(style.value());
                        update.set_cell((x % update.size().width.max(1), y), &cell);
                        x = x.wrapping_add(columns.max(1));
                    }
                });
            }
            Op::Fill {
                surface,
                glyph,
                style,
            } => {
                let Some(surface) = self.surface(*surface) else {
                    return;
                };
                let cell = TerminalCell::new(&glyph.text()).with_style(style.value());
                surface.update(|update| {
                    let size = update.size();
                    if size.width as usize * size.height as usize > 4096 {
                        return;
                    }
                    for y in 0..size.height {
                        for x in 0..size.width {
                            update.set_cell((x, y), &cell);
                        }
                    }
                });
            }
            Op::Clear { surface, row } => {
                let Some(surface) = self.surface(*surface) else {
                    return;
                };
                surface.update(|update| {
                    match row {
                        Some(row) => update.clear_row(u16::from(*row)),
                        None => update.clear(),
                    };
                });
            }
            Op::Scroll {
                surface,
                up,
                start,
                end,
                count,
            } => {
                let Some(surface) = self.surface(*surface) else {
                    return;
                };
                let region = u16::from(*start)..u16::from(*end);
                surface.update(|update| {
                    if *up {
                        update.scroll_up(region, u16::from(*count));
                    } else {
                        update.scroll_down(region, u16::from(*count));
                    }
                });
            }
            Op::Cursor {
                surface,
                x,
                y,
                visible,
            } => {
                let Some(surface) = self.surface(*surface) else {
                    return;
                };
                surface.update(|update| {
                    update.set_cursor_position((u16::from(*x), u16::from(*y)));
                    update.set_cursor_visible(*visible);
                });
            }
            Op::Resize {
                surface,
                width,
                height,
            } => {
                let Some(surface) = self.surface(*surface) else {
                    return;
                };
                surface.update(|update| {
                    update.resize((width.value(40), height.value(12)));
                });
            }
            Op::Config { terminal, change } => {
                let Some(entity) = self.terminal(*terminal) else {
                    return;
                };
                let mut config = self
                    .app
                    .world()
                    .get::<TerminalRenderConfig>(entity)
                    .unwrap()
                    .clone();
                self.apply_change(&mut config, change);
                *self
                    .app
                    .world_mut()
                    .get_mut::<TerminalRenderConfig>(entity)
                    .unwrap() = config;
            }
            Op::RemoveFont(choice) => {
                if let Some(handle) = self.fonts.get(choice).cloned()
                    && self
                        .app
                        .world_mut()
                        .resource_mut::<Assets<Font>>()
                        .remove(&handle)
                        .is_some()
                {
                    self.removed.push(*choice);
                }
            }
            Op::RestoreFont(choice) => {
                if let Some(position) = self.removed.iter().position(|c| c == choice) {
                    self.removed.remove(position);
                    let handle = self.fonts[choice].clone();
                    let bytes = choice.bytes().unwrap();
                    self.app
                        .world_mut()
                        .resource_mut::<Assets<Font>>()
                        .insert(handle.id(), Font::from_bytes(bytes.to_vec()))
                        .unwrap();
                }
            }
            Op::Spawn { surface, changes } => {
                let Some(surface) = self.surface(*surface) else {
                    return;
                };
                self.spawn(surface, FontChoice::JetBrainsRegular, changes);
            }
            Op::Despawn { terminal } => self.stop(*terminal, true),
            Op::RemoveRenderer { terminal } => self.stop(*terminal, false),
            Op::SetSurface { terminal, surface } => {
                let (Some(entity), Some(surface)) =
                    (self.terminal(*terminal), self.surface(*surface))
                else {
                    return;
                };
                self.app
                    .world_mut()
                    .entity_mut(entity)
                    .insert(TerminalRenderer::new(surface));
                // Replacing the renderer must keep (or at least publish) a texture.
                let image = self.image_of(entity);
                if let Some(slot) = self.terminals.iter_mut().find(|(e, _)| *e == entity) {
                    if slot.1 != image {
                        self.released.push(slot.1);
                    }
                    slot.1 = image;
                }
            }
            Op::NewSurface { width, height } => {
                if self.surfaces.len() < 4 {
                    self.surfaces
                        .push(TerminalSurface::new((width.value(40), height.value(12))));
                }
            }
            Op::Frames { count, millis } => {
                for _ in 0..(count % 4 + 1) {
                    if self.frames >= MAX_FRAMES {
                        break;
                    }
                    self.update(*millis % 2000);
                }
            }
        }
    }

    /// Lets the frozen-clock app settle, then checks per-terminal status and
    /// that idle terminals do no work.
    fn settle(&mut self) {
        for _ in 0..3 {
            self.update(0);
        }
        let statuses: Vec<TerminalStatus> = self
            .terminals
            .iter()
            .map(|&(entity, _)| {
                let status = self
                    .app
                    .world()
                    .get::<TerminalTexture>(entity)
                    .unwrap()
                    .status();
                self.check_status(entity, status);
                status
            })
            .collect();
        if std::env::var_os("RENDERER_FUZZ_STATS").is_some() {
            eprintln!("settled: {statuses:?}");
        }
        *self.app.world_mut().resource_mut::<Changes>() = Changes::default();
        for _ in 0..3 {
            self.update(0);
            for (&(entity, _), status) in self.terminals.iter().zip(&statuses) {
                let world = self.app.world();
                let texture = world.get::<TerminalTexture>(entity).unwrap();
                assert_eq!(texture.status(), *status, "status changed while idle");
                // Shaping failures are documented to retry every update.
                if *status != TerminalStatus::ShapingFailed {
                    assert_eq!(
                        *world.get::<TerminalStats>(entity).unwrap(),
                        TerminalStats::default(),
                        "idle terminal did work ({status:?})"
                    );
                }
            }
        }
        if !statuses.contains(&TerminalStatus::ShapingFailed) {
            let changes = self.app.world().resource::<Changes>();
            assert_eq!(changes.stats, 0, "TerminalStats changed while idle");
            assert_eq!(changes.textures, 0, "TerminalTexture changed while idle");
        }
        // Images of terminals that stopped rendering are released.
        let images = self.app.world().resource::<Assets<Image>>();
        for image in &self.released {
            assert!(
                images.get(*image).is_none(),
                "released terminal image leaked"
            );
        }
    }

    /// Checks the settled status against the configuration.
    fn check_status(&self, entity: Entity, status: TerminalStatus) {
        let world = self.app.world();
        let config = world.get::<TerminalRenderConfig>(entity).unwrap();
        let surface = world.get::<TerminalRenderer>(entity).unwrap().surface();
        if !config.sizing.is_valid() {
            assert_eq!(status, TerminalStatus::InvalidSizing);
            return;
        }
        let fonts = world.resource::<Assets<Font>>();
        let garbage = self.fonts.get(&FontChoice::Garbage).map(Handle::id);
        // The fidelity subsets hold only symbols (no "0" to measure), so
        // shaping with them as a face may legitimately fail.
        let subset_ids: Vec<_> = [
            FontChoice::Cjk,
            FontChoice::Emoji,
            FontChoice::Nerd,
            FontChoice::Wide,
        ]
        .iter()
        .filter_map(|choice| self.fonts.get(choice).map(Handle::id))
        .collect();
        let faces = std::iter::once(&config.font.regular)
            .chain(config.font.bold.iter())
            .chain(config.font.italic.iter())
            .chain(config.font.bold_italic.iter());
        let (mut missing, mut invalid, mut may_fail_shaping) = (false, false, false);
        for face in faces {
            match face {
                FontSource::Handle(handle) => match fonts.get(handle) {
                    None => missing = true,
                    // Bundled fonts register; only the garbage bytes cannot.
                    Some(_) => {
                        invalid |= Some(handle.id()) == garbage;
                        may_fail_shaping |= subset_ids.contains(&handle.id());
                    }
                },
                _ => may_fail_shaping = true,
            }
        }
        if invalid {
            assert_eq!(status, TerminalStatus::FontFailed);
            return;
        }
        if missing {
            assert_eq!(status, TerminalStatus::Loading);
            return;
        }
        match status {
            TerminalStatus::Ready => {}
            TerminalStatus::TextureTooLarge => {
                // Only plausible when the request is large.
                let scale = resolved_scale(config.raster.scale);
                let grid = surface.info().size;
                let (cell, font_size) = match config.sizing {
                    TerminalSizing::Fixed {
                        cell_size,
                        font_size,
                    } => (cell_size, font_size),
                    TerminalSizing::FitCellWidth(cell) => (cell, 0.0),
                    TerminalSizing::FromFont { font_size, .. } => (Vec2::ZERO, font_size),
                };
                let need = (cell * scale).round().max(Vec2::ONE)
                    * Vec2::new(f32::from(grid.width), f32::from(grid.height));
                let bound = need.max_element().max(font_size * scale);
                // FromFont cells and FitCellWidth heights derive from the
                // font, so only demand a large request somewhere.
                let font_driven = !matches!(config.sizing, TerminalSizing::Fixed { .. });
                assert!(
                    font_driven || bound > TEXTURE_LIMIT as f32,
                    "TextureTooLarge for a small request: {bound}"
                );
            }
            TerminalStatus::ShapingFailed if may_fail_shaping => {}
            other => panic!("unexpected settled status {other:?} for loaded fonts"),
        }
    }
}

fn resolved_scale(scale: f32) -> f32 {
    if scale.is_finite() && scale > 0.0 {
        scale.clamp(1.0, 8.0)
    } else {
        1.0
    }
}

/// Documented properties of a measured geometry.
fn check_geometry(
    surface: &TerminalSurface,
    config: &TerminalRenderConfig,
    geometry: &TerminalGeometry,
) {
    let grid = geometry.grid();
    assert_eq!(
        grid,
        surface.info().size,
        "measured grid differs from surface"
    );
    assert!(
        geometry.matches_surface(surface),
        "geometry does not match its surface"
    );
    let scale = resolved_scale(config.raster.scale);
    assert_eq!(geometry.raster_scale(), scale);
    let cell = geometry.physical_cell_size();
    assert!(
        cell.is_finite() && cell.cmpge(Vec2::ONE).all(),
        "bad physical cell {cell}"
    );
    assert_eq!(
        cell,
        cell.round(),
        "physical cell size not whole pixels: {cell}"
    );
    let logical = geometry.cell_size();
    assert!(logical.is_finite() && logical.cmpgt(Vec2::ZERO).all());
    let font = geometry.physical_font_size();
    assert!(font.is_finite() && font > 0.0, "bad font size {font}");
    assert!(geometry.font_size().is_finite() && geometry.font_size() > 0.0);
    let size = geometry.size();
    let expected = UVec2::new(
        (f32::from(grid.width) * cell.x).round().max(1.0) as u32,
        (f32::from(grid.height) * cell.y).round().max(1.0) as u32,
    );
    assert_eq!(
        size, expected,
        "size is not grid × physical cell ({grid:?}, {cell})"
    );
    assert!(
        size.max_element() <= TEXTURE_LIMIT,
        "texture exceeds limit: {size}"
    );
    let logical_size = geometry.logical_size();
    assert!(logical_size.is_finite());
    match config.sizing {
        TerminalSizing::Fixed { cell_size, .. } => {
            assert_eq!(
                cell,
                (cell_size * scale).round().max(Vec2::ONE),
                "fixed cells must snap, never grow"
            );
        }
        TerminalSizing::FitCellWidth(cell_size) => {
            let requested = (cell_size * scale).round().max(Vec2::ONE);
            assert_eq!(
                cell.x, requested.x,
                "fitted width must be the requested width"
            );
            assert!(
                cell.y >= requested.y,
                "fitted height must be at least the request"
            );
        }
        TerminalSizing::FromFont { .. } => {}
    }
}

fuzz_target!(|input: Input| {
    let mut harness = Harness::new();
    harness.run(&input);
});
