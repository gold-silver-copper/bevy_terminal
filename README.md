# `bevy_terminal_ratatui`

A Ratatui backend for the low-level `bevy_terminal` GPU renderer.

The renderer turns terminal cells and configuration into an image, validated
geometry, and status. Applications own UI layout, windows, cameras, raster scale,
and 3D materials. The adapter translates Ratatui buffers into the neutral surface;
it does not emulate a terminal, manage a PTY, or forward input.

| Crate | Responsibility |
| --- | --- |
| `bevy_terminal` | Shared terminal surface, font measurement, rendering, output geometry |
| `bevy_terminal_ratatui` | Ratatui backend and optional ergonomic terminal wrapper |

This checkout targets Bevy 0.19, Ratatui 0.30.2, and Rust 1.95 or newer. The API
below describes this checkout; a matching published release is required before
using it from crates.io.

## Drawing and rendering

```rust,no_run
use bevy::prelude::*;
use bevy_terminal_ratatui::prelude::*;

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, TerminalPlugin))
        .add_systems(Startup, setup)
        .add_systems(Update, present.after(TerminalSystems::Sync))
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
    let terminal = RatatuiTerminal::drawn(80, 24, |frame| {
        frame.render_widget("Hello from Ratatui", frame.area());
    });
    commands.spawn((
        terminal.with_renderer(),
        TerminalRenderConfig {
            sizing: TerminalSizing::font(18.0),
            raster: RasterConfig { scale: 1.0, ..default() },
            ..default()
        },
        ImageNode::default(),
    ));
}

// This application chooses to size each UI node to its measured terminal.
fn present(mut terminals: Query<(&TerminalTexture, &mut ImageNode, &mut Node)>) {
    for (texture, mut image, mut node) in &mut terminals {
        let Some(geometry) = texture.measured() else { continue; };
        let size = geometry.logical_size();
        if image.image != texture.image { image.image = texture.image.clone(); }
        if node.width != px(size.x) { node.width = px(size.x); }
        if node.height != px(size.y) { node.height = px(size.y); }
    }
}
```

The default font is Bevy's generic monospace source. Enable system font discovery
or supply a loaded `Font` asset. The runnable examples use bundled font faces.
Each terminal has an independent surface and texture. Keep the wrapper as a
component or resource and draw again when application content changes.

## Owning a normal Ratatui terminal

The wrapper is optional. Libraries such as `bevy_ratatui` can own an ordinary
`ratatui::Terminal<RatatuiBackend>` and retain Ratatui's native viewport APIs:

```rust
use bevy_terminal_ratatui::prelude::*;

let backend = RatatuiBackend::new(80, 24);
let renderer = TerminalRenderer::new(backend.surface());
let mut terminal = ratatui::Terminal::new(backend).unwrap();
terminal.draw(|frame| frame.render_widget("raw backend", frame.area())).unwrap();
assert_eq!(renderer.surface().snapshot().row_text(0).trim_end(), "raw backend");
```

See `integration/bevy-ratatui-context` for a fixture implementing the actual
consumer's `TerminalContext` trait without requiring the wrapper.

## Output and resizing

Read `TerminalTexture::measured()` after `TerminalSystems::Sync`. It returns
`None` while fonts load, measurement fails, or a shared surface resize makes the
old geometry stale. Status remains queryable. A late consumer can read current
output without having observed any earlier frames.

`TerminalGeometry` exposes logical cell/font sizes, logical output dimensions,
physical image dimensions, raster scale, and the measured grid. Geometry is bound
to its source surface and resize generation. Its image handle remains stable
across remeasurement; applications retain their last useful presentation while
replacement geometry is unavailable.

The wrapper's `fit_to(&texture, available_logical_size)` uses valid geometry from its own surface and
resizes the grid and Ratatui buffers together. With a raw backend, compute
`geometry.grid_for(available_logical_size)`, resize the backend, and call
`Terminal::autoresize()` for fullscreen/inline viewports. Fixed viewports require
an explicit `Terminal::resize()` for the application-owned area. Compare sizes
before resizing to avoid a feedback loop.

For `Backend::window_size().pixels`, explicitly pass measured geometry to
`backend_mut().set_geometry(geometry)`. Foreign or stale geometry is rejected.
A resize invalidates the backend's pixel measurement until fresh geometry arrives.

Measurement readiness is **not GPU completion**. Readback consumers must wait
for their render-world readback result and establish that it represents the
intended frame. The static export example checks its expected dimensions and
nonzero alpha; that is not a general completion signal for changing content.

## Presentation and DPI

`RasterConfig.scale` is an explicit physical-to-logical pixel ratio, defaulting to
`1.0`. The renderer never inspects windows, UI nodes, or cameras. Applications
choose scale for their target and update configuration before renderer sync.
Non-finite/non-positive scales fall back to `1.0`; positive scales clamp to `1..=8`.
Fractional logical cells remain available through measured geometry.

`examples/common/app.rs` demonstrates single-window UI layout and DPI policy.
`examples/world_quad.rs` binds the image to an application-owned material and
sizes a rectangle to its aspect ratio. No library presentation feature is needed;
enable the appropriate Bevy features in the application itself.

## Sizing, colors, and fonts

- `TerminalSizing::FromFont` derives cells from font size and line height.
- `TerminalSizing::FitCellWidth` fits the font to a cell width and grows height
  to contain its line box.
- `TerminalSizing::Fixed` uses explicit cell and font sizes, fitting/clipping
  glyphs to that grid.

Cells snap to physical pixels. Wide glyphs occupy explicit continuation cells,
and shaping stays anchored to grid columns. The renderer supports ANSI/indexed/RGB
colors, underline colors, style flags, cursor shapes, and blinking. Unicode
coverage depends on the selected fonts and fallback; inspect the fidelity harness
for the fonts your application ships.

The output is `Rgba8UnormSrgb` with straight alpha, suitable for Bevy UI, sprites,
and materials. Transparent backgrounds are supported.

`FontFaces` accepts explicit regular/bold/italic/bold-italic assets or Bevy font
sources. `font_family("JetBrains Mono")` constructs a named source. Query Bevy's
font collection directly when implementing an application font selector:

```rust,no_run
use bevy::{prelude::*, text::FontCx};
use bevy_terminal_ratatui::prelude::*;

fn choose_font(mut fonts: ResMut<FontCx>, mut config: Single<&mut TerminalRenderConfig>) {
    if fonts.collection.family_by_name("JetBrains Mono").is_some() {
        config.font = FontFaces::regular(font_family("JetBrains Mono"));
    }
}
```

Font assets are registered by Bevy asynchronously across updates. Family lookup
should run after registration, or be repeated when the font collection changes.

## Features and diagnostics

- `system_fonts` (default): discover host fonts. Disable defaults when supplying
  explicit assets and avoiding host font discovery.
- `timings` (opt-in): populate CPU snapshot and scene timing fields.
- `scrolling-regions` (adapter only, opt-in): enable Ratatui's extended backend
  trait. Leave disabled when coexisting with backends implementing the standard
  trait, such as `soft_ratatui`.

`TerminalStats` counters remain available without timing instrumentation and are
zero on idle frames; timing fields are zero when `timings` is disabled. Benchmarks
that report those timings enable the feature explicitly.

The adapter exports `RatatuiBackend`, `RatatuiTerminal`, `TerminalRenderer`, the
`bevy_terminal` crate, and a curated `prelude`. Advanced renderer modules are
accessed through `bevy_terminal_ratatui::bevy_terminal`, rather than mirrored
across both crate roots.

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

The interactive UI examples are resizable: their grids follow the window at the
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

In `image_export`, early frames contain the complete 72×22 scene. Later frames shrink the backend
to 60×18 so the exported sequence also catches row-stride errors and stale
texture pixels after a resize.

`high_dpi_export` writes the renderer-owned terminal texture at 2× scale under
`target/render-qa-2x/`, exercising native 2× font rasterization without a
camera resampling stage.

`multiple_terminals_export` writes a before/after sequence under
`target/multiple-terminals-qa/`; the second texture grows while the first
retains its dimensions. Measured sizes depend on the selected font.


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

## Migration from the previous presentation API

| Previous API | Replacement |
| --- | --- |
| `TerminalWorldQuad`, `3d` feature | Application mesh/material binding; `world_quad` example |
| `ui` feature, automatic `ImageNode` sizing | Application presentation system |
| `TerminalRenderScale` | Numeric `RasterConfig.scale` |
| `TerminalReady`, `TerminalRemeasured` | Persistent measured output after `TerminalSystems::Sync` |
| `TerminalFonts` | Direct Bevy `FontCx` access |
| Window-specific grid/scale helpers | Application window policy; `TerminalGeometry::grid_for` |
| Adapter root `render`, `scene`, `surface`, `bevy` modules | Explicit `bevy_terminal` crate re-export |

## License

MIT.
