# `bevy_terminal_ratatui`

A Ratatui backend for the low-level `bevy_terminal` GPU renderer.

The renderer turns terminal cells and configuration into an image, validated
geometry, and status. Applications own UI layout, windows, cameras, raster scale,
and 3D materials. The adapter translates Ratatui buffers into the neutral cell
grid; it does not emulate a terminal, manage a PTY, or forward input.

| Crate | Responsibility |
| --- | --- |
| `bevy_terminal` | Terminal grid component, font measurement, rendering, output geometry |
| `bevy_terminal_ratatui` | Ratatui backend and optional ergonomic terminal wrapper |

Version 0.10.1 targets Bevy 0.20, Ratatui 0.30.2, and Rust 1.97.1 or newer (Bevy's minimum).

```toml
[dependencies]
bevy_terminal_ratatui = "0.10.1"
```

## Drawing and rendering

```rust,no_run
use bevy::prelude::*;
use bevy_terminal_ratatui::prelude::*;

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, RatatuiPlugin))
        .add_systems(Startup, setup)
        .add_systems(PostUpdate, present.after(TerminalSystems::Sync))
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
    let mut terminal = RatatuiTerminal::new(80, 24);
    terminal.draw(|frame| {
        frame.render_widget("Hello from Ratatui", frame.area());
    });
    commands.spawn((
        terminal,
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
        if image.image != *texture.image() { image.image = texture.image().clone(); }
        if node.width != px(size.x) { node.width = px(size.x); }
        if node.height != px(size.y) { node.height = px(size.y); }
    }
}
```

The default font is Bevy's generic monospace source. Enable system font discovery
or supply a loaded `Font` asset. The runnable examples use bundled font faces.
Each terminal entity has its own grid and texture. Query the `RatatuiTerminal`
component and draw again when application content changes.

## Owning a normal Ratatui terminal

The wrapper is optional. Code that owns an ordinary
`ratatui::Terminal<RatatuiBackend>` keeps Ratatui's native viewport APIs. The
backend owns its grid:

```rust
use bevy_terminal_ratatui::prelude::*;

let mut terminal = ratatui::Terminal::new(RatatuiBackend::new(80, 24)).unwrap();
terminal.draw(|frame| frame.render_widget("raw backend", frame.area())).unwrap();
assert_eq!(terminal.backend().grid().row_text(0).trim_end(), "raw backend");
```

Terminals are drawn from entities. To render one owned by your own component,
implement `TerminalSource` for it (its grid is `self.backend().grid()`) and add
`TerminalSourcePlugin::<YourComponent>`.

## Output and resizing

Read `TerminalTexture::measured()` after `TerminalSystems::Sync`. It returns the
geometry while the status is `Ready`, and `None` while fonts load or after a
failure; `texture.status()` says which. A late consumer can read current output
without having observed any earlier frames.

`TerminalGeometry` exposes logical cell/font sizes, logical output dimensions,
physical image dimensions, raster scale, and the measured grid. It always
describes the image exactly. The image handle remains stable across
remeasurement; applications retain their last useful presentation while
replacement geometry is unavailable.

Applications choose available space and when to resize. Compute
`grid_for(available_logical_size, geometry.cell_size())` and use the wrapper's
`resize_grid` when dimensions change. With a raw backend, resize it and call
`Terminal::autoresize()` for fullscreen/inline viewports. Fixed viewports require
an explicit `Terminal::resize()` for the application-owned area. Compare sizes
before resizing to avoid a feedback loop; a geometry whose `grid()` differs from
the terminal's size was measured before the last resize.

`RatatuiPlugin` (and `RatatuiSourcePlugin` for your own `RatatuiSource`) hands
each backend its texture's measured geometry, so `Backend::window_size().pixels`
reports it. The pixel size is zero while the terminal loads or has failed, and
a resize hides it until geometry for the new size arrives.

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
- `TerminalSizing::Fixed` uses explicit cell and font sizes; taller ink
  overflows into the neighbouring rows.

Cells snap to physical pixels. Wide glyphs occupy explicit continuation cells,
and shaping stays anchored to grid columns. The renderer supports ANSI/indexed/RGB
colors, underline colors, style flags, cursor shapes, and blinking. Unicode
coverage depends on the selected fonts and fallback; the `render_test` example
shows the coverage of the fonts your application ships.

The output is `Rgba8UnormSrgb` with straight alpha, suitable for Bevy UI, sprites,
and materials. The alpha is exact over an opaque theme background (the default).
A translucent theme background is supported, but glyph edges and translucent
foregrounds over it are blended into its color and present darker than straight
alpha.

`FontFaces` accepts explicit regular/bold/italic/bold-italic assets or Bevy font
sources. `FontSource::Family("JetBrains Mono".into())` constructs a named source. Query Bevy's
font collection directly when implementing an application font selector:

```rust,no_run
use bevy::{prelude::*, text::FontCx};
use bevy_terminal_ratatui::prelude::*;

fn choose_font(mut fonts: ResMut<FontCx>, mut config: Single<&mut TerminalRenderConfig>) {
    if fonts.collection.family_by_name("JetBrains Mono").is_some() {
        config.font = FontFaces::regular(FontSource::Family("JetBrains Mono".into()));
    }
}
```

Font assets are registered by Bevy asynchronously across updates. Family lookup
should run after registration, or be repeated when the font collection changes.
To replace a font, give the configuration a new handle. Bevy does not register
again an asset removed and re-inserted under the same id before its next
update, which leaves the terminal `Loading`.

## Features and diagnostics

- `system_fonts` (default): discover host fonts. Disable defaults when supplying
  explicit assets and avoiding host font discovery.
- `timings` (opt-in): measure `TerminalStats::scene_ns`, which is zero otherwise.
- `scrolling-regions` (adapter only, opt-in): enable Ratatui's extended backend
  trait. Its methods become required of every backend in the build, so enable
  this whenever anything else enables `ratatui/scrolling-regions`, and leave it
  disabled when coexisting with backends implementing only the standard trait,
  such as `soft_ratatui`.

`TerminalStats` counters remain available without timing instrumentation and are
zero on idle frames.

The adapter exports `RatatuiBackend`, `RatatuiTerminal`, `RatatuiPlugin`,
`RatatuiSource`, `RatatuiSourcePlugin`, the `bevy_terminal` crate, and a
`prelude` that also re-exports the renderer's (including `grid_for`).
Advanced renderer modules are accessed through `bevy_terminal_ratatui::bevy_terminal`,
rather than mirrored across both crate roots.

## Migrating from 0.9 to 0.10

- **Open enums:** `TerminalStatus`, `TerminalFailure`, `TerminalSizing` and
  `CursorStyle` are `#[non_exhaustive]`: add a wildcard arm when matching them.
  `TerminalStats` is too, so it is read rather than constructed; its `scene_ns`
  field now always exists and is zero without the `timings` feature.
- **Geometry hand-off:** `RatatuiBackend::set_geometry` is removed. The plugin
  keeps every registered source's backend in step with its texture, including
  after the component is replaced or the terminal fails.
- **Raster scale:** `RasterConfig::scale` is clamped to `0.25..=8.0` (it was
  `1.0..=8.0`), so a terminal presented below its logical size can be
  rasterized at the size it is shown.
- **Placeholder image:** a terminal's image is 1×1 until its first scene, which
  sizes it together with the geometry it publishes.
- **Plugin order and Bevy features:** `TerminalPlugin` may be added before
  Bevy's render plugin. `bevy_terminal` no longer enables Bevy's
  `multi_threaded` and `async_executor` features; applications choose them.
- **Cell symbols:** the Ratatui backend drops control characters and escape
  sequences (such as OSC 8 hyperlinks) from cell symbols instead of drawing
  them as text.

0.10.1 declares the Rust version Bevy 0.20 already required (1.97.1), marks
`TerminalSystems` `#[non_exhaustive]`, and no longer panics when a terminal
entity is despawned while it is being added.

## Migrating from 0.8 to 0.9

- **Bevy 0.20:** `0.9.0` targets Bevy `0.20` (while `0.8.1` targeted Bevy `0.19`).
- **Entity-only terminals:** `TerminalSurface`, `TerminalRenderer`, and
  `TerminalSnapshot` are removed. Spawn `TerminalGrid` or `RatatuiTerminal`
  directly as a component (with `TerminalPlugin` or `RatatuiPlugin`), or
  implement `TerminalSource` / `RatatuiSource` on your own component and read
  cells via `terminal.grid()`.
- **Accessor & prelude:** Read the image handle via `texture.image()` instead of
  the `texture.image` field; `grid_for` is now in `prelude`.
- **Schedule:** `TerminalSystems::Sync` runs in `PostUpdate` (after Bevy font
  registration), so `Update` draw systems no longer need `.before(TerminalSystems::Sync)`
  and presentation systems run in `PostUpdate` `.after(TerminalSystems::Sync)`.

## Render QA

The examples live in the repository, not in the published crate; run them from
a checkout. The export examples use `bevy_image_export`.

```text
cargo run --example render_test                     # one window with every style/color/glyph check
cargo run --example render_test -- --export         # same scene to target/render-test/<family>/
cargo run --example render_test -- --font hack      # start with another vendored family
cargo run --example colors_rgb                      # live full-grid true-color stress test, vsync off
cargo run --example image_export                    # frames under target/render-qa/
cargo run --example high_dpi_export                 # 2x raster under target/render-qa-2x/
cargo run --example multiple_terminals_export       # target/multiple-terminals-qa/
```

For optimized builds that still rebuild quickly after an edit, use the
`release-fast` profile (no LTO, 16 codegen units, incremental):
`cargo run --profile release-fast --example render_test`.

`render_test` shows all 512 modifier combinations, the four font faces, ANSI 16
as foreground/background/reversed, the 256-color cube and grayscale ramp, RGB
gradients, underline colors, every box-drawing weight and junction,
block/quadrant/shade/braille elements, wide CJK/emoji cells with guard columns,
combining marks, RTL/Indic text, and the cursor. `Space`/`Tab` (`Shift+Tab`
back) cycle through the vendored font families. `colors_rgb` ports Ratatui's
animated RGB example and redraws every cell on every frame; its title bar
reports the grid size and per-frame renderer statistics.

The interactive examples are resizable: their grids follow the window at the
renderer's measured cell size instead of the window being sized from a fixed
column × row count. `image_export` shrinks the grid partway through, and
`multiple_terminals_export` grows one of two terminals, so the exported frames
also catch stale pixels after a resize.

Glyph placement is checked by rules rather than reference images: library tests
verify that ink is clipped only at the texture's edges, that box, block and
shade tiles meet without seams, and that symbols fit their cells. The GPU
readback tests need a GPU:

```text
cargo test --workspace --all-features --test gpu_readback --test export_readiness -- --ignored --test-threads=1
```

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
See [RATATUI_EXAMPLES.md](https://github.com/gold-silver-copper/bevy_terminal/blob/main/RATATUI_EXAMPLES.md) for the pinned upstream commit,
complete inventory, and adaptation policy.

## License

MIT.
