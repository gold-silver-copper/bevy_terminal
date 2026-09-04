# `bevy_terminal_ratatui`

`bevy_terminal_ratatui` is a Ratatui backend that renders a terminal texture
with Bevy's own text shaping, font fallback, glyph atlases, render device, and
render world. It is a thin adapter over [`bevy_terminal`](crates/bevy_terminal),
which owns the renderer-neutral scene model, the retained surface and the
compact Bevy renderer:

| concept | type | role |
|---|---|---|
| surface | `TerminalSurface` | thread-safe retained grid the backend writes into |
| cell | `TerminalCell` (`CellSymbol` + `TerminalStyle` + occupancy) | one grid cell |
| snapshot | `TerminalSnapshot` | owned copy of the grid the renderer reads incrementally |
| terminal entity | `TerminalRenderer` (`bevy_terminal::Terminal`) + `TerminalRenderConfig` | one rendered terminal; add an `ImageNode` to show it |
| texture | `TerminalTexture` | the renderer-owned `Rgba8UnormSrgb` image (stable handle), its size, cell size and font size |
| config | `TerminalRenderConfig` | cell sizing (`CellSizing::Logical`/`FromFont`), fonts, theme, cursor, blink, raster |
| features | `ui`, `system_fonts` (both default), `3d` | UI `ImageNode` presentation; system font discovery; `TerminalWorldQuad` 3D presentation |

## Compatibility

| `bevy_terminal_ratatui` | `bevy_terminal` | `bevy` | `ratatui` |
|---|---|---|---|
| 0.7 | 0.7 | 0.19 (any patch) | 0.30 |
| 0.6 | 0.6 | 0.19 (any patch) | 0.30 |
| 0.5 | 0.5 | 0.19.1 | 0.30 |
| 0.3 | 0.3 | 0.19 | 0.30 |

```text
bevy_terminal ──> bevy
bevy_terminal_ratatui ──> bevy_terminal
                         └> ratatui
```

The normal dependency list of the two crates together is only `bevy` and
`ratatui`; there is no software framebuffer, Egui bridge, or external
font/rendering engine in the runtime path.

`RatatuiBackend` implements `ratatui::backend::Backend`. Each `draw` acquires
the surface once, translates only the cells Ratatui submitted into neutral
`TerminalCell`s (named colors become ANSI indices, `Indexed`/`Rgb` are kept,
`Reset` becomes the contextual default; every modifier and the underline color
are preserved), synthesizes the wide-glyph continuation cells that Ratatui
omits from its diff, and publishes at most one new surface revision. The
`bevy_terminal` renderer then copies only dirty cells, anchors each cell string
to its exact column, builds a compact quad scene and draws it into a
renderer-owned `Image`. Put an `ImageNode` on the terminal entity to show that
texture through Bevy UI (the renderer keeps its image and size in sync, your
layout places it); without one the terminal is headless and only the texture is
produced. All renderer types are re-exported here, so a Ratatui application
only installs this crate. The renderer component is exported as
`TerminalRenderer` so it does not collide with `ratatui::Terminal`.

```no_run
use bevy::prelude::*;
use bevy_terminal_ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

let (mut terminal, renderer) = RatatuiTerminal::new(60, 20);

terminal.draw(|frame| {
    frame.render_widget(
        Paragraph::new("Hello from Ratatui")
            .block(Block::new().borders(Borders::ALL)),
        frame.area(),
    );
});

App::new()
    .add_plugins((DefaultPlugins, TerminalPlugin))
    .add_systems(Startup, move |mut commands: Commands| {
        commands.spawn(Camera2d);
        commands.spawn((
            renderer.clone(),
            ImageNode::default(),
            Node {
                position_type: PositionType::Absolute,
                left: px(20.0),
                top: px(20.0),
                ..default()
            },
        ));
    })
    .run();
# Ok::<(), Box<dyn std::error::Error>>(())
```

`TerminalPlugin` is added once. Every `TerminalRenderer` component you spawn
becomes an independently rendered terminal; it requires a
`TerminalRenderConfig` (a default is inserted automatically — insert your own
to configure, mutate it to rebuild only that terminal). The plugin attaches
`TerminalTexture` (the renderer-owned image with a stable handle, its
physical/logical size, raster scale, cell size and effective font size) and
`TerminalStats`, and triggers `TerminalReady` on the entity when the texture
exists. Terminals can be spawned and despawned at any time.

`TerminalReady` fires exactly once. If the texture changes size later — a grid
resize, a configuration or raster-scale change, or a font that finished
loading and re-measured the cell — the entity gets a `TerminalRemeasured`
event (`previous_size`, `size`, `cell_size`). The image handle never changes,
so UI presentation and `TerminalWorldQuad` follow along automatically;
custom presentation only needs to observe the event when something it built
depends on the size.

### Drawing the first frame

The first presented frame shows whatever the surface holds when the renderer
first syncs it — the empty theme background if nothing has been drawn yet.
`RatatuiTerminal::drawn` draws one frame before handing the pair back, so the
terminal has content from frame one:

```no_run
# use bevy::prelude::*;
# use bevy_terminal_ratatui::prelude::*;
# use ratatui::widgets::Paragraph;
# fn setup(mut commands: Commands) {
commands.spawn((
    RatatuiTerminal::drawn(60, 20, |frame| {
        frame.render_widget(Paragraph::new("Loading..."), frame.area());
    }),
    ImageNode::default(),
    Node::default(),
));
# }
```

`RatatuiTerminal` is a component wrapping `ratatui::Terminal<RatatuiBackend>`
(`Deref` to the inner terminal); `new`/`drawn` return it paired with the
renderer as a bundle, so the pair spawns as-is or destructures for a
resource-based setup. Its `draw` returns `()` — the backend is infallible —
and `resize_grid`/`fit_to`/`surface`/`snapshot` are inherent methods.

Applications that do not use Ratatui can write the same surface directly; see
the [`bevy_terminal` README](crates/bevy_terminal/README.md) for the scene API,
texture-only use, multiple surfaces and font-face configuration.

## Multiple independent terminals

Each spawned terminal entity owns its renderer-owned texture, configuration and
statistics. Terminals share the render pipeline and reusable GPU storage, but
updates and resizes remain independent, and because presentation is an
ordinary `ImageNode` they take part in Bevy UI layout — here a flex row:

```no_run
# use bevy::prelude::*;
# use bevy_terminal_ratatui::prelude::*;
App::new()
    .add_plugins((DefaultPlugins, TerminalPlugin))
    .add_systems(Startup, |mut commands: Commands| {
        commands.spawn(Camera2d);
        commands
            .spawn(Node {
                position_type: PositionType::Absolute,
                left: px(24.0),
                top: px(56.0),
                column_gap: px(52.0),
                ..default()
            })
            .with_children(|row| {
                row.spawn((RatatuiTerminal::new(42, 16), ImageNode::default(), Node::default()));
                row.spawn((RatatuiTerminal::new(34, 12), ImageNode::default(), Node::default()));
            });
    });
```

`TerminalRenderer`, `TerminalRenderConfig`, `TerminalTexture`, and
`TerminalStats` are components on the same entity, so you already hold the
`Entity` when you spawn it. Mutate `TerminalRenderConfig` to change one
terminal's configuration; change detection rebuilds only that terminal. The
texture handle is stable for the terminal's lifetime (resizes reallocate the
image in place), so custom presentation code can bind it once — from the
`TerminalReady` event or `TerminalTexture`.

Run `cargo run --example multiple_terminals` for a complete scene where both
terminals update independently and one resizes while the other remains unchanged.

## World-space presentation (3D)

With the `3d` feature, `TerminalWorldQuad` presents a terminal on an unlit,
alpha-blended rectangle in a 3D scene. Its width follows the measured texture
aspect and is rebuilt on every re-measure, so a terminal on a wall stays in
proportion when its grid or font changes:

```no_run
# use bevy::prelude::*;
# use bevy_terminal_ratatui::prelude::*;
# let mut app = App::new();
app.add_systems(Startup, |mut commands: Commands| {
    commands.spawn(Camera3d::default());
    commands.spawn((
        RatatuiTerminal::new(80, 24),
        TerminalWorldQuad::new(3.0), // 3 world units tall, width from the aspect
        Transform::from_xyz(0.0, 1.5, -2.0),
    ));
});
```

The material contract: the plugin writes only `base_color_texture`, `unlit`,
`alpha_mode`, `double_sided` and `cull_mode` (the fields the component
mirrors; `TerminalWorldQuad::two_sided()` makes a screen readable from
behind). Every other `StandardMaterial` field — emissive texture and tint,
roughness, base color — is yours and survives re-syncs, so a "power state"
system can dim the plugin-created material safely. See
`cargo run --example world_quad --features 3d`.

For your own mesh (an imported screen model), skip the component: bind
`TerminalTexture::image` in your material once on `TerminalReady` — the
handle is stable — and observe `TerminalRemeasured` if your UV mapping depends
on the size. `cargo run --example imported_screen --features 3d` walks
through it with a curved screen mesh, an emissive material and a power toggle.

## Terminal emulator setup

Terminal emulators work "font size in → cell size out": pick a family and a
size, derive the cell from the font, and fit as many cells as the window holds.

```no_run
# use bevy::prelude::*;
# use bevy_terminal_ratatui::prelude::*;
# use bevy_terminal_ratatui::render::raster_scale_for_window;
fn setup(mut commands: Commands, window: Query<&Window>) {
    let window = window.single().unwrap();
    commands.spawn((
        RatatuiTerminal::new(80, 24),
        TerminalRenderConfig {
            cell_size: CellSizing::FROM_FONT,     // width and height measured from the font
            font_size: FontSizing::Px(16.0),      // zoom by mutating this
            font: FontFaces::regular(font_family("JetBrains Mono")),
            raster: RasterConfig {
                scale: TerminalRenderScale::Fixed(raster_scale_for_window(window)),
                ..default()
            },
            ..default()
        },
        ImageNode::default(),
        Node::default(),
    ));
}

// Each frame (or on WindowResized): fit the grid to the window at the current
// cell size (`TerminalTexture::cell_size` is the logical cell the renderer
// settled on), then draw as usual.
fn fit(mut tui: Single<(&mut RatatuiTerminal, &TerminalTexture)>, window: Query<&Window>) {
    let (terminal, texture) = &mut *tui;
    if let Ok(window) = window.single() {
        terminal.fit_to(texture, window.resolution.size());
    }
}
```

`RatatuiTerminal::fit_to` resizes both the surface and Ratatui's
buffers and returns whether the grid changed; `TerminalTexture::grid_for` and
`render::grid_for_window` give the same computation without resizing.

## Windowed TUI setup

For a `bevy_ratatui`-style windowed app (fixed cell size, grid follows the
window): create the backend anywhere (no `App` needed), spawn the terminal on
startup, and resize on `WindowResized`:

```no_run
# use bevy::{prelude::*, window::WindowResized};
# use bevy_terminal_ratatui::prelude::*;
fn on_resize(
    mut resized: MessageReader<WindowResized>,
    mut tui: Single<(&mut RatatuiTerminal, &TerminalTexture)>,
) {
    let (terminal, texture) = &mut *tui;
    for event in resized.read() {
        terminal.fit_to(texture, Vec2::new(event.width, event.height));
    }
}
```

The default `TerminalRenderConfig` (11×20 cells, `FontSizing::FitCellWidth`,
`FontSource::Monospace`) needs the `system_fonts` feature; without it supply
a font asset (`FontFaces::regular(handle)`), or use
`FontSource::Handle(Handle::default())` for Bevy's built-in FiraMono subset
when `bevy/default_font` is enabled (monospace, but limited box-drawing
coverage).

## Cell geometry and Unicode

`CellSizing::Logical(Vec2)` is the default: a shaped text run can use several
fallback fonts and there is no single metric valid for every Unicode glyph, so
an explicit cell keeps every glyph anchored to its column. Pick a primary
monospace font; with `FontSizing::FitCellWidth` (default) the font is measured
and sized so its advance fills the cell. `CellSizing::FROM_FONT` derives the cell
from a `FontSizing::Px` size instead (see above). Integer logical cell
dimensions are recommended. The compact renderer snaps the physical cell to
whole pixels so cell edges cannot accumulate subpixel drift.

### Vertical fit: primary glyphs are never clipped, fallback glyphs are fitted

After the font size is chosen the renderer measures the primary font (once per
font/config change, never per frame): the rows its full block `█` covers
completely are the *line box*; the ink of an ASCII probe (`gjpqy|[]{}()_`) in
all four faces and of an accented-capital probe are measured as well. Every
glyph is then shifted by one uniform whole-pixel offset per terminal so that,
in priority order, the block keeps covering the cell (block/box tiles have no
seams), the ASCII probe is fully inside the cell (descenders included), and
accented capitals fit when the font leaves room.

- With `FitCellWidth`, the configured cell height is a minimum and grows to the
  measured line box. With `CellSizing::FROM_FONT`, both dimensions come from the
  font. The legacy `FromFont { line_height }` payload is ignored. A primary-font
  glyph inside the line box is therefore never clipped.
  Iosevka Fixed sized to an 11 px column is a 22 px font whose line box is 27
  px, so a requested 11×20 cell becomes 11×27; read the effective size from
  `TerminalTexture::cell_size`. To keep an exact grid, choose `FontSizing::Px`
  in a logical cell (glyphs beyond it are clipped after the same fitting).
- The font size is fitted to the *physical* cell width, so fractional raster
  scales (1.5×) do not open seams between advances.
- Horizontally, a run wider than its cell (a fallback family with a larger
  advance, a wide italic) is placed so the faintest columns are clipped;
  a run that fits but overhangs (italic, negative bearing) is pushed inside.
  A sub-pixel overshoot — box-drawing and block glyphs drawn a fraction past
  their advance so strokes overlap (JetBrains Mono, Cascadia) — is not
  overhang: the run keeps its bearings and the cell clips the faint column,
  so `┌` stays on the same pixel column as `│`.
- Everything else — accents a font draws above its own line box (Iosevka's
  `Ẫ`, Cascadia's `À`), emoji/CJK from a taller fallback family — is centered
  and clipped to its cell as a last resort.

`TerminalRenderConfig::raster.scale` defaults to
`TerminalRenderScale::Automatic`. An on-screen batch renderer then shapes text
and builds its terminal texture at the primary window's physical-to-logical
scale factor. For example, an 18 logical-pixel font is rasterized at 36 physical
pixels on a 2× display and the terminal image is presented at half its physical
dimensions. A non-default Bevy `UiScale` is included in this calculation. The
terminal image uses nearest sampling, so that final presentation does not blur
glyph atlas texels. Its UI origin is also snapped to
the physical pixel grid.

Headless mode deliberately resolves `Automatic` to `1.0`, keeping benchmarks
and image exports deterministic. Use `TerminalRenderScale::Fixed(value)` when a
custom camera or output target has a known scale; that target should present the
texture at the same scale. `TerminalTexture` reports `size` in physical pixels,
`logical_size` in Bevy UI pixels, the active `raster_scale` and the effective
`font_size`.

Wide Ratatui cells are anchored to their declared columns regardless of the fallback
glyph's natural advance. This prevents column drift, but a poorly matched font
can still make the glyph look too narrow or too wide inside those two columns.
Every glyph, including box-drawing, block, shade, quadrant and braille
characters, is rendered from the configured font: nothing is drawn
procedurally. By default (`FontSizing::FitCellWidth`) the renderer measures the
regular font's advance by shaping it and picks the font size at which one
advance equals `cell_size.x`, so glyphs designed to fill their advance tile the
grid without seams regardless of which font is used. Set `FontSizing::Px(size)`
to use an explicit size. Glyphs are rasterized unhinted by default
(`raster.hinting`), because hinting snaps the font to whole-pixel sizes and would
reintroduce fractional seams on displays whose scale factor makes the physical
font size fractional.

All Ratatui colors are supported, including the ANSI 256-color cube and true
color. Bold, dim, italic, underline, reverse, hidden, crossed-out, slow-blink,
and rapid-blink modifiers are mapped to Bevy text behavior. Bold and italic
depend on the selected font family exposing suitable faces or variable axes.
For deterministic static-face selection, fill in `FontFaces { regular, bold,
italic, bold_italic, synthesize }` (missing faces fall back bold-italic → bold →
italic → regular; `synthesize` controls whether the bold weight / italic style
is still requested from the fallback face). Every executable example
does this with the vendored Iosevka Fixed 34.8.0 family (`assets/fonts/iosevka-fixed`,
OFL), whose Regular/Bold/Italic/BoldItalic faces cover box drawing, blocks,
braille, arrows and geometric shapes so terminal symbols rarely need a system
fallback font; the faces are read from the checkout at runtime and the
embedded JetBrains Mono faces are used if they are absent. See
`assets/fonts/README.md` for every vendored family and its coverage.

## Texture format and transparency

`TerminalTexture::image` is `Rgba8UnormSrgb` with straight (non-premultiplied)
alpha: display-ready for `ImageNode`, sprites and `StandardMaterial`
textures. `TerminalTheme::background` alpha is honored end to end (the clear
color, default-background cells and partial-row repaints all *replace* rather
than accumulate), so a translucent terminal composites correctly over a
`ClearColor` or a 3D scene: `cargo run --example render_test -- --transparent`.

## Font families

`FontSource` is Bevy's type; its `Family` variant holds a `SmolStr`. Use
`font_family("JetBrains Mono")` (any string type) instead of spelling the
conversion out, or the re-exported `SmolStr`. To choose between installed
families — or warn when a preferred one is missing — query `TerminalFonts`, a
system parameter over Bevy's font collection (registered font assets plus,
with `system_fonts`, the installed system fonts):

```no_run
# use bevy::prelude::*;
# use bevy_terminal_ratatui::prelude::*;
fn choose_font(mut fonts: TerminalFonts, mut config: Single<&mut TerminalRenderConfig>) {
    match fonts.resolve_family(&["JetBrainsMono Nerd Font Mono", "JetBrains Mono"]) {
        Some(family) => config.font = FontFaces::regular(family),
        None => warn!("JetBrains Mono is not installed: {:?}", fonts.families()),
    }
}
```

Glyphs the configured faces lack (CJK, emoji, scripts) come from Bevy's
system-wide font fallback; Bevy 0.19 does not expose a per-text fallback family
list, so there is no per-terminal fallback setting. Choose a primary family
with the symbol coverage you need (see `assets/fonts/README.md`).

## Resizing

`RatatuiTerminal::resize_grid` resizes the backend grid and Ratatui's own
double buffers together:

```ignore
terminal.resize_grid(columns, rows);
```

## Testing drawn content

`RatatuiTerminal::snapshot()` (or `surface().snapshot()`) returns the
`TerminalSnapshot` the renderer would draw, so tests assert against the real
backend instead of a parallel `TestBackend` draw path. `to_text()` /
`Display` give the screen as plain rows (wide glyphs once, styles dropped);
`iter()` yields `(CellPosition, &TerminalCell)` for style assertions:

```
# use bevy_terminal_ratatui::prelude::*;
# use ratatui::widgets::Paragraph;
let (terminal, _renderer) = RatatuiTerminal::drawn(12, 2, |frame| {
    frame.render_widget(Paragraph::new("Loading..."), frame.area());
});
let snapshot = terminal.snapshot();
assert!(snapshot.to_text().contains("Loading..."));
assert_eq!(snapshot.row_text(1).trim(), "");
assert!(snapshot.iter().all(|(_, cell)| cell.style.background == TerminalColor::Default));
```

## Render QA

The `image_export` example uses the Git development dependency
`bevy_image_export` to write frames under `target/render-qa/`:

```text
cargo run --example colors_rgb                      # live full-grid true-color stress test, vsync off
cargo run --example render_test                     # one window with every style/color/glyph check
cargo run --example render_test -- --export         # same scene to target/render-test/<family>/
cargo run --example render_test -- --font iosevka-fixed
cargo run --example image_export
cargo run --example high_dpi_export
cargo run --example multiple_terminals_export
```

`colors_rgb` ports Ratatui's animated RGB example and redraws every cell on
every frame, making it a live renderer-throughput stress test. Its title bar
reports the fitted grid size and per-frame renderer statistics.

`glyph_fidelity` is the clipping/seam harness: full printable ASCII in four
faces, Latin-1/Extended-A, Greek, Cyrillic, combining-mark stacks, all box
drawing (U+2500–257F), block elements, braille, shapes, arrows, a wide
CJK/emoji row and block/line tile panels on a per-cell checkerboard with `│`
guard columns:

```text
cargo run --example glyph_fidelity                                   # window; Space/Tab cycle fonts
cargo run --example glyph_fidelity -- --export --font all --scale all # target/glyph-fidelity/<family>/<scale>x/
cargo run --example glyph_fidelity -- --check --font all --scale all  # GPU readback assertions, exit 1 on failure
cargo test --test glyph_fidelity -- --ignored                         # the same check as an integration test
```

`--check` renders each terminal twice — once at the real cell and once in a
6 px wider, 10 px taller reference cell at the same font size — and asserts
that every ASCII/Latin/Greek/Cyrillic glyph inside the font's line box keeps
exactly the same ink pixels (nothing clipped), that the solid-block tile has
no pixel off the fill color and the line tiles are continuous, at 1×, 1.5×,
2× and 3×.

Every windowed example is resizable: the grid follows the window at the
renderer's measured cell size (`RatatuiTerminal::fit_to`, see
`examples/common/app.rs::fit_grid_to_window`) instead of the window being sized
from a fixed column × row count.

`render_test` is the single all-in-one check (press `Space`/`Tab` to cycle
through the vendored font families under `assets/fonts/`, `Shift+Tab` for the
previous one; `--font <index|dir>` or `RENDER_TEST_FONT` picks the initial
family, `--export` or `RENDER_TEST_EXPORT=1` exports headlessly): all
512 modifier combinations,
the four font faces, ANSI 16 as foreground/background/reversed, the 256-color
cube and grayscale ramp, RGB gradients, underline colors, every box-drawing
weight and junction, block/quadrant/shade/braille elements, wide CJK/emoji
cells with guard columns, combining marks, RTL/Indic text, and the cursor.

Early frames contain the complete 72×22 scene. Later frames shrink the backend
to 60×18 so the exported sequence also catches row-stride errors and stale
texture pixels after a resize.

`high_dpi_export` writes the renderer-owned 1584×1188 terminal texture under
`target/render-qa-2x/`, exercising native 2× font rasterization without a
camera resampling stage.

`multiple_terminals_export` writes a before/after sequence under
`target/multiple-terminals-qa/`; the second texture grows from 320×240 to
360×288 while the first stays at 370×288 (10 px Iosevka columns measure a
10×24 cell).

The normal dependency list is deliberately limited to `bevy_terminal` and
`ratatui`; `bevy_terminal` itself depends only on `bevy`.

## Renderer performance comparison

`benchmarks/renderer-comparison` is a separate workspace containing a
windowless, synchronized Bevy harness for comparing `bevy_terminal_ratatui`,
`soft_ratatui`, `egui_ratatui`, `parley_ratatui`, and `bevy_tui_texture` with
identical Ratatui workloads. It reports raw JSON samples plus CSV/Markdown
summaries and keeps all third-party renderer dependencies outside this library's
runtime manifest. See
[`benchmarks/renderer-comparison/README.md`](benchmarks/renderer-comparison/README.md).

## Ratatui upstream example ports

The project also contains an interactive Bevy gallery of all 43 runnable
targets from Ratatui 0.30.2: 32 application examples and 11 state-pattern
binaries. Run the gallery, optionally choose its starting slug, or export the
complete deterministic visual suite:

```text
cargo run --example ratatui_examples -- --list
cargo run --example ratatui_examples
cargo run --example ratatui_examples -- chart
cargo run --example ratatui_examples_export
```

Use `PageUp`/`PageDown` to switch examples, `F1` for the current example's
controls, `F2` to reset it, and `F10` to exit. Bevy keyboard/mouse input drives
each port's selection, scrolling, forms, text editing, drawing, and animation.
The gallery window is resizable and adjusts the Ratatui buffer grid while
keeping fixed, crisp cell dimensions; deterministic exports remain 100×62.

The exporter writes stable frames beneath `target/ratatui-examples/<slug>/`.
Network responses, randomness, clocks, tracing subscribers, panic hooks, and
terminal-only input or escape behavior use documented deterministic fixtures.
See [RATATUI_EXAMPLES.md](RATATUI_EXAMPLES.md) for the pinned upstream commit,
complete inventory, and adaptation policy.

## Current scope

Both crates enable `unsafe_code = "forbid"`. The backend models each terminal's visible grid only: rows scrolled past the top
are discarded rather than retained as host-side scrollback.

## Migrating from 0.1

0.2 replaces the per-instance plugin with a component and tidies the API:

| 0.1 | 0.2 |
|---|---|
| `app.add_plugins(BevyTerminalPlugin::new(surface).with_config(c).headless())` | `app.add_plugins(TerminalPlugin)` once, then `commands.spawn(Terminal::new(surface).with_config(c).with_presentation(Presentation::Headless))` |
| `TerminalBatch` / `TerminalBatchOutput` / `TerminalBatchRoot` / `TerminalBatchStats` | `Terminal` / `TerminalTexture` / `TerminalNode` / `TerminalStats` |
| `TerminalBatchPresentation::{Ui, Headless}` + `TerminalRenderConfig::origin` | `Presentation::{Ui { origin }, Headless}` |
| `TerminalRenderConfig { font, bold_font, italic_font, bold_italic_font }` | `TerminalRenderConfig { font: FontFaces { regular, bold, italic, bold_italic } }` |
| `font_size: f32` + `font_sizing: FontSizing` | `font_size: FontSizing::{FitCellWidth, Px(f32)}` |
| `cursor_style`, `theme.cursor`, `cursor_blink_hz` | `cursor: CursorConfig { style, color, blink_hz }` |
| `slow_blink_hz`, `rapid_blink_hz` (0 = off) | `blink: BlinkConfig { slow_hz: Option, rapid_hz: Option }` |
| `RetainedBevyTerminalPlugin`, `TerminalRenderStats`, `TerminalRoot` | removed |
| `RatatuiBackend::snapshot()` | `backend.surface().snapshot()` |
| `backend.resize(c, r); terminal.autoresize()?` | `terminal.resize_grid(c, r)` |
| `update.set_cell(x, y, &cell)`, `set_cursor_position(x, y)` | `set_cell((x, y), &cell)`, `set_cursor_position((x, y))` (any `Into<CellPosition>`) |
| `clear_from` / `clear_through` / `clear_row_from` | `clear_range(start, end)` |
| `TerminalCell::occupancy` field, `with_occupancy` | `occupancy()`; construct with `new`/`wide`/`continuation_of` |
| `CellSymbol::{Ascii, Inline, Heap}` variants | opaque; use `new`/`as_str`/`Deref` |
| `SurfaceMetrics { cell_size: Option<(f32,f32)>, pixel_size: (u16,u16) }` | `Option<Vec2>`, `UVec2` |
| `TerminalSurface::begin_update()` | still available; prefer `surface.update(|u| { .. })` |

## Migrating from 0.2

0.3 splits the terminal into plain components and hands UI presentation to you:

| 0.2 | 0.3 |
|---|---|
| `Terminal::new(s).with_config(c).with_presentation(Presentation::Ui { origin })` | `(TerminalRenderer::new(s), c, ImageNode::default(), Node { position_type: Absolute, left, top, .. })` — any UI placement works |
| `Presentation::Headless` | spawn without an `ImageNode` |
| `Presentation`, `TerminalNode`, `TerminalResized` | removed; texture handle is now stable; `TerminalReady` fires once when the texture exists |
| `terminal.config()` / `config_mut()` | `TerminalRenderConfig` is its own (required) component; query/mutate it directly |
| `TerminalPlugin` (unit) | `TerminalPlugin` (`collect_timings` field) |
| `bevy_terminal::Terminal` in the Ratatui prelude | `TerminalRenderer` (alias; the lower crate still calls it `Terminal`) |
| `RatatuiBackend::new(c, r)` + `Terminal::new(backend.surface())` | `RatatuiTerminal::new(c, r)` → `(terminal, renderer)` |
| `TerminalSurface::new(c, r)`, `update.resize(c, r)` | `new((c, r))`, `resize((c, r))` (any `Into<GridSize>`) |
| `snapshot.cell(x, y)`, `GridSize::contains(pos)` | `cell((x, y))`, `contains((x, y))` (any `Into<CellPosition>`) |
| `TerminalTheme::cursor` | removed (use `CursorConfig::color`) |
| `TerminalTheme::{foreground, background, resolve}` | crate-private |
| `FontFaces { .. }` | new field `synthesize: bool` (default `true`; `FontFaces::regular(..)`/`From` set it) |

## Migrating from 0.4

| 0.4 | 0.5 |
|---|---|
| `cell_size: Vec2` | `cell_size: CellSizing` (`Vec2::into()` / `CellSizing::Logical`, or `CellSizing::FROM_FONT` with `FontSizing::Px`) |
| — | `TerminalTexture::cell_size` (logical), `TerminalTexture::grid_for`, `render::{grid_for, grid_for_window, raster_scale_for_window}`, `RatatuiTerminalExt::fit_to` |
| `Rgba8Unorm` texture (linear bytes) | `Rgba8UnormSrgb` (display-ready) |
| always-on `bevy/2d`, `bevy/ui`, `bevy/system_font_discovery` | minimal Bevy features; crate features `ui` and `system_fonts` (both default) |
| — | `bevy_terminal::bevy` re-export |
| `FitCellWidth` clipped glyphs taller than the cell | the cell height grows to the font's line box (`TerminalTexture::cell_size`); use `FontSizing::Px` for an exact grid |
| `TerminalReady` on allocation (before fonts were measured) | `TerminalReady` once fonts are registered and the texture has its measured size |

## Migrating from 0.3

0.4 removes redundant ways of doing things:

| 0.3 | 0.4 |
|---|---|
| `bevy_terminal::{Terminal, TerminalSurface, ..}` (flat root) | `bevy_terminal::prelude::*` or the public modules `bevy_terminal::{render, scene, surface}` |
| `surface.begin_update()` / `SurfaceUpdate::commit` / `has_changes` | `surface.update(\|u\| { .. }) -> bool` only |
| `SurfaceUpdate::set_cells`, `cell`, `contains` | loop with `set_cell`; out-of-range positions are ignored |
| `TerminalSurface::metrics()` / `SurfaceMetrics` | `surface.size()` and `surface.pixel_size() -> Option<UVec2>` |
| `TerminalRenderConfig { render_scale, font_hinting, .. }` | `raster: RasterConfig { scale, hinting }` |
| `TerminalPlugin { collect_timings }` | `TerminalPlugin` (unit; timings always collected) |
| `TerminalSystems::{Setup, Sync}` | `TerminalSystems::Sync` |
| `TerminalStats::{sync_frames, unchanged_frames, cached_shapes, gpu_bytes_written}` | removed (unchanged frames report zeros) |
| `TerminalTexture::cell_size` | removed (`size` ÷ grid) |
| `TerminalReady { entity, image, size }` | `TerminalReady { entity }`; read `TerminalTexture` |
| `GridSize::ZERO`, `GridSize::area`, `CellPosition::ORIGIN`, `StyleFlags::{difference, set}`, `CellOccupancy::spanning`, `TerminalSnapshot::empty` | removed |
| `RatatuiBackend::terminal()`, `RatatuiBackend::resize` (public), `RatatuiTerminalExt::surface` | `RatatuiTerminal::{new, resize_grid, surface}` |

## Migrating from 0.6

| 0.6 | 0.7 |
|---|---|
| `RatatuiBackend::with_terminal(c, r)` → `(backend, renderer)` + `ratatui::Terminal::new(backend)?` | `RatatuiTerminal::new(c, r)` → `(RatatuiTerminal, renderer)`, a spawnable bundle |
| `RatatuiBackend::with_terminal_drawn(c, r, f)` | `RatatuiTerminal::drawn(c, r, f)` |
| `RatatuiTerminalExt::{resize_grid, fit_to}` on `ratatui::Terminal<RatatuiBackend>` | inherent methods on `RatatuiTerminal` (which derefs to the inner terminal) |
| `terminal.draw(f)?` / `let Ok(_) = terminal.draw(f)` | `terminal.draw(f)` returns `()` |
| `terminal.backend().surface()` | `terminal.surface()`; `terminal.snapshot()` for assertions |
| — | `TerminalSnapshot::{iter, row_text, to_text}` + `Display` |
| `TerminalWorldQuad { height, unlit, alpha_mode }` | + `double_sided`, `cull_mode`, `two_sided()`, and a documented material write-set contract |

## License

Licensed under the MIT license.
