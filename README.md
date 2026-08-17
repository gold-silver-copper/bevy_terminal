# `bevy_terminal_ratatui`

`bevy_terminal_ratatui` is a Ratatui backend that renders a terminal texture
with Bevy's own text shaping, font fallback, glyph atlases, render device, and
render world. It is a thin adapter over [`bevy_terminal`](crates/bevy_terminal),
which owns the renderer-neutral scene model, the retained surface and the
compact Bevy renderer:

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
renderer-owned `Image`. `Presentation::Ui` shows that texture through one
`ImageNode`; `Presentation::Headless` exposes it without a camera or UI node.
All renderer types are re-exported here, so a Ratatui application only
installs this crate.

```no_run
use bevy::prelude::*;
use bevy_terminal_ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

let backend = RatatuiBackend::new(60, 20);
let surface = backend.surface();
let mut terminal = ratatui::Terminal::new(backend)?;

terminal.draw(|frame| {
    frame.render_widget(
        Paragraph::new("Hello from Ratatui")
            .block(Block::new().borders(Borders::ALL)),
        frame.area(),
    );
})?;

App::new()
    .add_plugins((DefaultPlugins, TerminalPlugin))
    .add_systems(Startup, move |mut commands: Commands| {
        commands.spawn(Camera2d);
        commands.spawn(Terminal::new(surface.clone()));
    })
    .run();
# Ok::<(), Box<dyn std::error::Error>>(())
```

`TerminalPlugin` is added once. Every `Terminal` component you spawn becomes an
independently rendered terminal; the plugin attaches `TerminalTexture` (the
renderer-owned image, its physical/logical size, raster scale and effective
font size), `TerminalStats`, and, for `Presentation::Ui { origin }`, a
`TerminalNode` image node. Terminals can be spawned and despawned at any time.

Applications that do not use Ratatui can write the same surface directly; see
the [`bevy_terminal` README](crates/bevy_terminal/README.md) for the scene API,
texture-only use, multiple surfaces and font-face configuration.

## Multiple independent terminals

Each spawned `Terminal` entity owns its renderer-owned texture, configuration,
statistics, and optional UI image node. Terminals share the render pipeline and
reusable GPU storage, but updates and resizes remain independent:

```no_run
# use bevy::prelude::*;
# use bevy_terminal_ratatui::prelude::*;
let left = RatatuiBackend::new(42, 16).surface();
let right = RatatuiBackend::new(34, 12).surface();

App::new()
    .add_plugins((DefaultPlugins, TerminalPlugin))
    .add_systems(Startup, move |mut commands: Commands| {
        commands.spawn(Camera2d);
        commands.spawn(Terminal::new(left.clone()).with_presentation(Presentation::Ui {
            origin: Vec2::new(24.0, 56.0),
        }));
        commands.spawn(Terminal::new(right.clone()).with_presentation(Presentation::Ui {
            origin: Vec2::new(496.0, 56.0),
        }));
    });
```

`Terminal`, `TerminalTexture`, and `TerminalStats` are components on the same
entity, so you already hold the `Entity` when you spawn it. Edit
`Terminal::config_mut()` to change one terminal's configuration; change
detection rebuilds only that terminal. A terminal's texture handle changes
identity when its grid dimensions or raster scale change; a `TerminalResized`
entity event is triggered on the terminal entity so custom presentation code
can rebind (`app.add_observer(|resized: On<TerminalResized>, ..| ..)`).
`TerminalNode::terminal` links the optional UI presentation node back to its
terminal.

Run `cargo run --example multiple_terminals` for a complete scene where both
terminals update independently and one resizes while the other remains unchanged.

## Cell geometry and Unicode

`TerminalRenderConfig::cell_size` is explicit because a shaped text run can use
several fallback fonts and there is no single metric valid for every Unicode
glyph. Pick a primary monospace font and a cell width matching its unit advance.
The default uses Bevy's generic system monospace family with a cell size suited
to an 18 px font. Integer logical cell dimensions are recommended. The compact
renderer snaps the corresponding physical cell and font dimensions to whole
pixels so cell edges cannot accumulate subpixel drift. For deterministic
production rendering, supply a font family or loaded font asset and configure
its measured cell dimensions.

`TerminalRenderConfig::render_scale` defaults to
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
`logical_size` in Bevy UI pixels, the active `raster_scale`, the physical
`cell_size` and the effective `font_size`.

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
(`font_hinting`), because hinting snaps the font to whole-pixel sizes and would
reintroduce fractional seams on displays whose scale factor makes the physical
font size fractional.

All Ratatui colors are supported, including the ANSI 256-color cube and true
color. Bold, dim, italic, underline, reverse, hidden, crossed-out, slow-blink,
and rapid-blink modifiers are mapped to Bevy text behavior. Bold and italic
depend on the selected font family exposing suitable faces or variable axes.
For deterministic static-face selection, fill in `FontFaces { regular, bold,
italic, bold_italic }` (missing faces fall back bold-italic → bold → italic →
regular). Every executable example
does this with the vendored Iosevka Fixed 34.8.0 family (`assets/fonts/iosevka-fixed`,
OFL), whose Regular/Bold/Italic/BoldItalic faces cover box drawing, blocks,
braille, arrows and geometric shapes so terminal symbols rarely need a system
fallback font; the faces are read from the checkout at runtime and the
embedded JetBrains Mono faces are used if they are absent. See
`assets/fonts/README.md` for every vendored family and its coverage.

## Resizing

`RatatuiTerminalExt::resize_grid` resizes the backend grid and Ratatui's own
double buffers together:

```ignore
use bevy_terminal_ratatui::RatatuiTerminalExt;
terminal.resize_grid(columns, rows);
```

## Render QA

The `image_export` example uses the Git development dependency
`bevy_image_export` to write frames under `target/render-qa/`:

```text
cargo run --example render_test                     # one window with every style/color/glyph check
cargo run --example render_test -- --export         # same scene to target/render-test/<family>/
cargo run --example render_test -- --font iosevka-fixed
cargo run --example image_export
cargo run --example high_dpi_export
cargo run --example multiple_terminals_export
```

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

`high_dpi_export` writes the renderer-owned 1584×880 terminal texture under
`target/render-qa-2x/`, exercising native 2× font rasterization without a
camera resampling stage.

`multiple_terminals_export` writes a before/after sequence under
`target/multiple-terminals-qa/`; the second texture grows from 320×180 to
360×216 while the first stays at 370×216.

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
| `backend.resize(c, r); terminal.autoresize()?` | `terminal.resize_grid(c, r)` (`RatatuiTerminalExt`) |
| `update.set_cell(x, y, &cell)`, `set_cursor_position(x, y)` | `set_cell((x, y), &cell)`, `set_cursor_position((x, y))` (any `Into<CellPosition>`) |
| `clear_from` / `clear_through` / `clear_row_from` | `clear_range(start, end)` |
| `TerminalCell::occupancy` field, `with_occupancy` | `occupancy()`; construct with `new`/`wide`/`continuation_of` |
| `CellSymbol::{Ascii, Inline, Heap}` variants | opaque; use `new`/`as_str`/`Deref` |
| `SurfaceMetrics { cell_size: Option<(f32,f32)>, pixel_size: (u16,u16) }` | `Option<Vec2>`, `UVec2` |
| `TerminalSurface::begin_update()` | still available; prefer `surface.update(|u| { .. })` |

## License

Licensed under the MIT license.
