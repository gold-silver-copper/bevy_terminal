# `bevy_terminal`

`bevy_terminal` renders terminal-style character grids with Bevy's own text
shaping, font fallback, glyph atlases, render device, and render world. Its only
normal dependency is `bevy`. It knows nothing about any particular terminal UI
library: producers write a renderer-neutral scene of cells, styles, colors and
cursor state into a `TerminalSurface`, and the renderer turns that scene into a
renderer-owned texture.

The Ratatui backend lives in the companion crate `bevy_terminal_ratatui`, which
depends on this crate and on `ratatui`:

```text
bevy_terminal ──> bevy
bevy_terminal_ratatui ──> bevy_terminal
                         └> ratatui
```

## Scene model

- `GridSize` and `CellPosition`: small integer terminal coordinates.
- `TerminalCell`: a `CellSymbol` grapheme (inline up to 22 bytes, heap beyond),
  a `TerminalStyle`, and a `CellOccupancy`.
- `CellOccupancy::{Single, Wide { columns }, Continuation}`: an ordinary glyph,
  the anchor of a glyph spanning several columns, or a column covered by the
  anchor to its left. Wide glyphs are always drawn inside their declared span
  and never overwrite the following cell.
- `TerminalStyle`: foreground, background and underline `TerminalColor`s plus a
  `StyleFlags` bit set for bold, dim, italic, underline, slow/rapid blink,
  reverse, hidden and crossed-out.
- `TerminalColor::{Default, Indexed(u8), Rgb(u8, u8, u8)}`. `Default` resolves
  contextually against the `TerminalTheme`: theme foreground for text, theme
  background for backgrounds, and the resolved foreground for underlines.
- `TerminalSnapshot`: an owned copy of the grid, cursor and revision.

## Writing a scene

```no_run
use bevy::prelude::*;
use bevy_terminal::prelude::*;

let surface = TerminalSurface::new(40, 10);
{
    // One lock, many cells, at most one published revision.
    let mut update = surface.begin_update();
    let title = TerminalStyle::new()
        .fg(TerminalColor::BLACK)
        .bg(TerminalColor::LIGHT_CYAN)
        .with(StyleFlags::BOLD);
    for (column, symbol) in "hello".chars().enumerate() {
        let cell = TerminalCell { symbol: symbol.into(), style: title, ..TerminalCell::EMPTY };
        update.set_cell(column as u16, 0, &cell);
    }
    update.set_cell(0, 2, &TerminalCell::wide("界", 2)); // writes its continuation cell too
    update.set_cell(3, 2, &TerminalCell::new("┌"));
    update.set_cursor_position(5, 0);
    update.set_cursor_visible(true);
    update.commit(); // or drop the guard
}

App::new()
    .add_plugins((DefaultPlugins, BevyTerminalPlugin::new(surface)))
    .add_systems(Startup, |mut commands: Commands| {
        commands.spawn(Camera2d);
    })
    .run();
```

`SurfaceUpdate` also offers `set_cells`, `clear`, `clear_row`, `clear_from`,
`clear_through`, `clear_row_from`, `resize`, `scroll_up` and `scroll_down`.
Every mutation compares against the retained cell and marks only real changes
dirty, so redrawing identical content publishes nothing and the renderer's
unchanged-frame fast path stays intact. A snapshot can be brought up to date
incrementally with `TerminalSurface::update_snapshot`, which copies only dirty
cells and reports the changed rows.

## Rendering

`BevyTerminalPlugin::new(surface)` installs the compact renderer. Each plugin
instance creates its own entity carrying `TerminalBatch`, `TerminalBatchOutput`
(the renderer-owned `Handle<Image>`, its physical size, logical size and raster
scale) and `TerminalBatchStats`. Instances share the GPU pipeline and scratch
buffers but keep independent surfaces, configurations, textures and statistics.
Add the plugin several times for several terminals.

- `TerminalBatchPresentation::Ui` (default) spawns one `ImageNode` marked with
  `TerminalBatchRoot` that presents the texture at `TerminalRenderConfig::origin`.
- `.headless()` renders only the texture, for custom composition, image export
  or benchmarks. Read the handle from `TerminalBatchOutput`; it changes when
  the grid dimensions or raster scale change.
- `TerminalRenderConfig` sets the explicit `cell_size`, the `FontSizing`
  (`FitCellWidth` by default: the regular font's advance is measured and the
  font sized so one advance equals the cell width; `Explicit` uses
  `font_size` as given), the
  regular `font` plus optional `bold_font`, `italic_font` and
  `bold_italic_font` faces, the `TerminalTheme`, `CursorStyle`, blink rates and
  `TerminalRenderScale` (`Automatic` follows the primary window in UI mode and
  resolves to 1.0 headless; `Fixed(scale)` rasterizes at a known scale).
- The renderer reads the surface once per changed frame, copies only dirty
  cells into its retained snapshot, and rebuilds only changed rows.
- Every glyph, including box-drawing, block, shade and braille characters, is
  shaped and rasterized from the configured font by Bevy text, cached in one
  renderer-owned atlas and clipped to its cell; nothing is drawn procedurally.
- `RetainedBevyTerminalPlugin` keeps the original one-Bevy-UI-entity-per-run
  path as a reference implementation.

Resizing a surface (`SurfaceUpdate::resize`) preserves the overlapping cells;
the renderer reallocates its texture with a new asset identity so custom
presentation code should observe `TerminalBatchOutput` for changes.
`TerminalSurface::metrics` reports the logical pixel size the renderer derived
from its configuration, so producers can expose window metrics without knowing
the renderer's internals.

## Examples

```text
cargo run -p bevy_terminal --example scene
cargo run -p bevy_terminal --example scene_export
```

`scene` renders two independent surfaces (styles, box drawing, blocks, a wide
glyph, colors and a moving cursor) written directly through the surface API,
using the vendored JetBrains Mono Regular/Bold/Italic/BoldItalic faces under
`assets/fonts/jetbrains-mono` (OFL). `scene_export` writes the same textures
headlessly under `target/bevy-terminal-qa/`.

## License

Licensed under the MIT license.
