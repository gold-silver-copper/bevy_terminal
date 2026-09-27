//! A CPU replay of batch scenes, to prove that partial repaints produce the
//! same pixels as full ones.
//!
//! The replay follows the GPU path: pixel-aligned quads sampled with nearest
//! filtering, the unified shader's colour rules, straight-alpha blending in
//! linear light and an 8-bit sRGB target, so each draw rounds once as it does
//! on the GPU. It is a reference for scene *equivalence*, not for GPU pixels.

use super::probe::{BUNDLED, ProbeFont, isolate_fonts, load_faces};
use super::*;
use crate::render::{BlinkConfig, CursorConfig, RasterConfig, TerminalSizing};
use crate::scene::{TerminalCell, TerminalColor, TerminalStyle};
use bevy::ecs::system::SystemState;

/// The shader's linear-corrected coverage (Ghostty's `alpha-blending =
/// linear-corrected`).
fn corrected(coverage: f32, foreground: Vec3, background: f32) -> f32 {
    let linearize = |v: f32| {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let unlinearize = |v: f32| {
        if v <= 0.003_130_8 {
            v * 12.92
        } else {
            v.powf(1.0 / 2.4) * 1.055 - 0.055
        }
    };
    let fg = foreground.dot(Vec3::new(0.2126, 0.7152, 0.0722));
    if background < 0.0 || (fg - background).abs() <= 0.001 {
        return coverage;
    }
    let blend = linearize(unlinearize(fg) * coverage + unlinearize(background) * (1.0 - coverage));
    ((blend - background) / (fg - background)).clamp(0.0, 1.0)
}

/// An 8-bit sRGB RGBA image.
#[derive(Clone, PartialEq)]
pub(super) struct Canvas {
    pub(super) size: UVec2,
    pub(super) pixels: Vec<[u8; 4]>,
}

impl std::fmt::Debug for Canvas {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Canvas({}x{})", self.size.x, self.size.y)
    }
}

fn encode(linear: Vec4) -> [u8; 4] {
    let srgb = Srgba::from(LinearRgba::new(linear.x, linear.y, linear.z, linear.w));
    srgb.to_u8_array()
}

fn decode(pixel: [u8; 4]) -> Vec4 {
    let linear = LinearRgba::from(Srgba::from_u8_array(pixel));
    Vec4::new(linear.red, linear.green, linear.blue, linear.alpha)
}

impl Canvas {
    pub(super) fn new(size: UVec2) -> Self {
        Self {
            size,
            pixels: vec![[0; 4]; (size.x * size.y) as usize],
        }
    }

    /// Draws `scene` over the canvas.
    pub(super) fn apply(&mut self, scene: &BatchScene, images: &Assets<Image>) {
        assert_eq!(scene.destination_size, self.size, "scene for another size");
        if scene.clear {
            let clear = encode(Vec4::from_array(
                scene.clear_color.to_linear().to_f32_array(),
            ));
            self.pixels.fill(clear);
        }
        for batch in &scene.batches {
            let atlas = images.get(batch.texture);
            for instance in
                &scene.instances[batch.start as usize..(batch.start + batch.count) as usize]
            {
                self.draw(instance, atlas, batch.replace);
            }
        }
    }

    fn draw(&mut self, quad: &QuadInstance, atlas: Option<&Image>, replace: bool) {
        let size = self.size.as_vec2();
        let x0 = ((quad.rect.x + 1.0) * 0.5 * size.x).round();
        let x1 = ((quad.rect.z + 1.0) * 0.5 * size.x).round();
        let y0 = ((1.0 - quad.rect.y) * 0.5 * size.y).round();
        let y1 = ((1.0 - quad.rect.w) * 0.5 * size.y).round();
        let solid = quad.uv.w < 0.0;
        for y in y0.max(0.0) as u32..y1.min(size.y) as u32 {
            for x in x0.max(0.0) as u32..x1.min(size.x) as u32 {
                let source = if solid {
                    quad.color
                } else {
                    let atlas = atlas.expect("glyph atlas");
                    let u = quad.uv.x + (x as f32 + 0.5 - x0) / (x1 - x0) * (quad.uv.z - quad.uv.x);
                    let v = quad.uv.y + (y as f32 + 0.5 - y0) / (y1 - y0) * (quad.uv.w - quad.uv.y);
                    let tx = ((u * atlas.width() as f32).floor() as u32).min(atlas.width() - 1);
                    let ty = ((v * atlas.height() as f32).floor() as u32).min(atlas.height() - 1);
                    let data = atlas.data.as_ref().expect("readable atlas");
                    let offset = ((ty * atlas.width() + tx) * 4) as usize;
                    let sample = decode(data[offset..offset + 4].try_into().unwrap());
                    if quad.color.w >= 0.0 {
                        let coverage = corrected(sample.w, quad.color.truncate(), quad.background);
                        quad.color.truncate().extend(quad.color.w * coverage)
                    } else {
                        sample
                    }
                };
                let index = (y * self.size.x + x) as usize;
                let destination = decode(self.pixels[index]);
                let blended = if replace {
                    source
                } else {
                    let a = source.w;
                    (source.truncate() * a + destination.truncate() * (1.0 - a))
                        .extend(a + destination.w * (1.0 - a))
                };
                self.pixels[index] = encode(blended);
            }
        }
    }

    /// Number of differing pixels and the first difference.
    pub(super) fn diff(&self, other: &Canvas) -> (usize, Option<PixelDifference>) {
        assert_eq!(self.size, other.size);
        let mut count = 0;
        let mut first = None;
        for (index, (a, b)) in self.pixels.iter().zip(&other.pixels).enumerate() {
            if a != b {
                count += 1;
                first.get_or_insert((
                    UVec2::new(index as u32 % self.size.x, index as u32 / self.size.x),
                    *a,
                    *b,
                ));
            }
        }
        (count, first)
    }
}

/// A pixel position with its value in two canvases.
pub(super) type PixelDifference = (UVec2, [u8; 4], [u8; 4]);

/// A headless terminal whose scenes are replayed onto a persistent canvas.
pub(super) struct Replay {
    pub(super) app: App,
    pub(super) entity: Entity,
    pub(super) surface: TerminalSurface,
    pub(super) canvas: Canvas,
    pub(super) scenes: usize,
    pub(super) partial_scenes: usize,
}

impl Replay {
    /// A terminal using a bundled family with the isolated fallback fonts.
    pub(super) fn new(family: &str, grid: (u16, u16), sizing: TerminalSizing, scale: f32) -> Self {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            bevy::text::TextPlugin,
            TerminalPlugin,
        ))
        .init_asset::<Image>();
        isolate_fonts(&mut app);
        let (dir, files) = BUNDLED
            .iter()
            .find(|(dir, _)| *dir == family)
            .expect("bundled family");
        let faces = load_faces(&mut app, &ProbeFont::Bundled(dir, *files));
        let surface = TerminalSurface::new(grid);
        let entity = app
            .world_mut()
            .spawn((
                TerminalRenderer::new(surface.clone()),
                TerminalRenderConfig {
                    sizing,
                    font: faces,
                    raster: RasterConfig { scale, ..default() },
                    blink: BlinkConfig::NONE,
                    cursor: CursorConfig {
                        blink_hz: None,
                        ..default()
                    },
                    ..default()
                },
            ))
            .id();
        let mut replay = Self {
            app,
            entity,
            surface,
            canvas: Canvas::new(UVec2::ONE),
            scenes: 0,
            partial_scenes: 0,
        };
        for _ in 0..6 {
            replay.step();
        }
        assert!(
            replay.texture().measured().is_some(),
            "terminal never became ready"
        );
        replay
    }

    pub(super) fn texture(&self) -> &TerminalTexture {
        self.app
            .world()
            .get::<TerminalTexture>(self.entity)
            .unwrap()
    }

    /// Runs one update and replays its scene (acknowledging it as the GPU would).
    pub(super) fn step(&mut self) {
        self.app.update();
        let mut state = self
            .app
            .world_mut()
            .get_mut::<BatchMainState>(self.entity)
            .unwrap();
        let Some(scene) = state.pending.take() else {
            return;
        };
        let generation = state.generation;
        state.submitted.store(generation, Ordering::Release);
        if scene.destination_size != self.canvas.size {
            assert!(scene.clear, "a resized texture starts with a full scene");
            self.canvas = Canvas::new(scene.destination_size);
        }
        self.scenes += 1;
        self.partial_scenes += usize::from(!scene.clear);
        let images = self.app.world().resource::<Assets<Image>>();
        self.canvas.apply(&scene, images);
    }

    /// The canvas a full scene of the current snapshot produces, built with
    /// the terminal's own shapes, atlas and metrics but fresh scene state.
    pub(super) fn full_reference(&mut self) -> Canvas {
        let mut system = SystemState::<(
            TextResources,
            ResMut<Assets<Image>>,
            Query<(&mut BatchMainState, &TerminalRenderConfig)>,
        )>::new(self.app.world_mut());
        let (mut text, mut images, mut states) = system.get_mut(self.app.world_mut()).unwrap();
        let (mut state, config) = states.get_mut(self.entity).unwrap();
        let config = config.clone();
        let snapshot = state.last_snapshot.clone().expect("retained snapshot");
        let rows: Vec<u16> = (0..snapshot.size().height).collect();
        let destination = state.output.id();
        let blink = state.blink;
        let BatchMainState {
            raster_config,
            shapes,
            glyph_atlas,
            ..
        } = &mut *state;
        let mut scratch = SceneScratch::default();
        let mut reach = Vec::new();
        let mut stats = TerminalStats::default();
        let mut cx = text.context(&mut images);
        let scene = build_scene(
            &snapshot,
            &config,
            *raster_config,
            &rows,
            true,
            destination,
            &mut cx,
            shapes,
            glyph_atlas,
            &mut scratch,
            &mut reach,
            &mut stats,
            blink,
        );
        assert!(cx.failure.is_none());
        let mut canvas = Canvas::new(scene.destination_size);
        canvas.apply(&scene, &images);
        canvas
    }

    /// Updates, replays, and asserts the canvas equals a full repaint.
    pub(super) fn assert_matches_full(&mut self, step: &str) {
        self.step();
        let reference = self.full_reference();
        let (count, first) = self.canvas.diff(&reference);
        assert_eq!(
            count, 0,
            "{step}: partial repaints differ from a full scene in {count} pixels; first {first:?}"
        );
    }

    pub(super) fn write(&self, row: u16, text: &str, style: TerminalStyle) {
        use unicode_segmentation::UnicodeSegmentation;
        use unicode_width::UnicodeWidthStr;
        self.surface.update(|update| {
            update.clear_row(row);
            let mut column = 0;
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
}

fn background(r: u8, g: u8, b: u8) -> TerminalStyle {
    TerminalStyle::new().bg(TerminalColor::Rgb(r, g, b))
}

#[test]
fn overflowing_ink_reaches_neighbours_and_partial_repaints_match_full_scenes() {
    let mut replay = Replay::new(
        "cascadia-mono",
        (14, 7),
        TerminalSizing::FromFont {
            font_size: 24.0,
            line_height: 0.85,
        },
        1.0,
    );
    let plain = TerminalStyle::new();
    for row in 0..7 {
        replay.write(
            row,
            "abcdefghijklmn",
            background(30 + row as u8 * 20, 40, 60),
        );
    }
    replay.assert_matches_full("initial");
    let tall = "Ẫ\u{302}\u{303}ǺZ\u{302}\u{303}\u{304}ع ح q\u{307}\u{328}";

    // Ink overflowing a row reaches its neighbours in a full scene.
    replay.write(3, tall, background(90, 30, 30));
    replay.assert_matches_full("overflowing content written");
    let reach = replay
        .app
        .world()
        .get::<BatchMainState>(replay.entity)
        .unwrap()
        .reach[3];
    assert!(
        reach.up >= 1 && reach.down >= 1,
        "the stacked marks and the Arabic descenders leave the row: {reach:?}"
    );

    // A neighbour changes under the overflow, including only its background.
    replay.write(2, "ABCDEFGHIJKLMN", background(20, 90, 20));
    replay.assert_matches_full("row above rewritten");
    replay.write(4, "abcdefghijklmn", background(20, 20, 90));
    replay.assert_matches_full("row below background changed");
    replay.write(4, "abcdefghijklmn", background(20, 20, 91));
    replay.assert_matches_full("background-only change");

    // The cursor moves through overflowed rows.
    for row in [2, 3, 4, 0] {
        replay.surface.update(|update| {
            update.set_cursor_visible(true);
            update.set_cursor_position((5, row));
        });
        replay.assert_matches_full("cursor moved");
    }

    // Removing overflowing content leaves no stale ink behind.
    replay.write(3, "plain text", plain);
    replay.assert_matches_full("overflowing content removed");

    // Ink that reaches two rows, and overflow at the texture's edges.
    replay.write(
        3,
        "a\u{301}\u{302}\u{303}\u{304}\u{306}\u{307}\u{308}\u{30a}\u{30b}\u{30c}",
        plain,
    );
    replay.write(0, tall, plain);
    replay.write(6, tall, plain);
    replay.assert_matches_full("multi-row reach and edges");
    let reach = replay
        .app
        .world()
        .get::<BatchMainState>(replay.entity)
        .unwrap()
        .reach[3];
    assert!(
        reach.up >= 2,
        "ten stacked marks reach two rows up: {reach:?}"
    );
    replay.write(1, "x", plain);
    replay.assert_matches_full("row between a two-row reach rewritten");

    // Scrolling moves overflowing rows; style-only changes repaint.
    replay.surface.update(|update| {
        update.scroll_up(0..7, 1);
    });
    replay.assert_matches_full("scrolled");
    replay.write(
        2,
        tall,
        TerminalStyle::new().fg(TerminalColor::Rgb(255, 200, 0)),
    );
    replay.assert_matches_full("style change");

    // Blinking text hides and shows overflowing ink.
    replay
        .app
        .world_mut()
        .get_mut::<TerminalRenderConfig>(replay.entity)
        .unwrap()
        .blink = BlinkConfig {
        slow_hz: Some(40.0),
        rapid_hz: None,
    };
    replay.write(
        5,
        tall,
        TerminalStyle::new().with(crate::scene::StyleFlags::SLOW_BLINK),
    );
    for _ in 0..12 {
        replay.assert_matches_full("blinking");
        std::thread::sleep(std::time::Duration::from_millis(7));
    }
    assert!(
        replay.partial_scenes >= 10,
        "the sequence must exercise partial scenes ({} of {})",
        replay.partial_scenes,
        replay.scenes
    );
}

#[test]
fn rows_without_overflow_repaint_only_themselves() {
    let mut replay = Replay::new("jetbrains-mono", (10, 5), TerminalSizing::font(18.0), 1.0);
    for row in 0..5 {
        replay.write(row, "plain text", TerminalStyle::new());
    }
    replay.assert_matches_full("initial");
    replay.write(2, "other text", TerminalStyle::new());
    replay.step();
    let stats = *replay
        .app
        .world()
        .get::<TerminalStats>(replay.entity)
        .unwrap();
    assert_eq!(stats.changed_rows, 1, "ordinary text repaints one row");
    let state = replay
        .app
        .world()
        .get::<BatchMainState>(replay.entity)
        .unwrap();
    assert!(
        state
            .reach
            .iter()
            .all(|reach| *reach == RowReach::default())
    );
}

#[test]
fn overflowing_coverage_is_corrected_against_each_background_it_covers() {
    let mut replay = Replay::new("cascadia-mono", (3, 3), TerminalSizing::font(24.0), 1.0);
    let (above, own) = (background(200, 30, 30), background(20, 20, 120));
    replay.write(0, "   ", above);
    replay.write(1, " Z\u{302}\u{303}\u{304}\u{306}\u{307} ", own);
    replay.write(2, "   ", above);
    replay.app.update();
    let state = replay
        .app
        .world()
        .get::<BatchMainState>(replay.entity)
        .unwrap();
    let scene = state.pending.as_ref().expect("a scene");
    let luminance = |style: TerminalStyle| {
        super::scene::luminance(
            crate::render::TerminalTheme::default().background(style.background),
        )
    };
    let backgrounds: Vec<f32> = scene
        .batches
        .iter()
        .filter(|batch| !batch.replace)
        .flat_map(|batch| {
            &scene.instances[batch.start as usize..(batch.start + batch.count) as usize]
        })
        .filter(|quad| quad.uv.w >= 0.0 && quad.color.w >= 0.0)
        .map(|quad| quad.background)
        .collect();
    assert!(
        backgrounds
            .iter()
            .any(|b| (b - luminance(own)).abs() < 1e-6)
            && backgrounds
                .iter()
                .any(|b| (b - luminance(above)).abs() < 1e-6),
        "the stacked marks over the row above blend against its background: {backgrounds:?}"
    );
}
