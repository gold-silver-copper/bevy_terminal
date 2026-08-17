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
- `TerminalCell`: a `CellSymbol` grapheme (opaque; 24 bytes, no allocation up
  to 22 UTF-8 bytes), a `TerminalStyle`, and a read-only `CellOccupancy` set by
  `TerminalCell::new`, `wide` or `continuation_of`.
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
// One lock, many cells, at most one published revision.
surface.update(|update| {
    let title = TerminalStyle::new()
        .fg(TerminalColor::BLACK)
        .bg(TerminalColor::LIGHT_CYAN)
        .with(StyleFlags::BOLD);
    for (column, symbol) in "hello".chars().enumerate() {
        update.set_cell((column as u16, 0), &TerminalCell::from(symbol).with_style(title));
    }
    update.set_cell((0, 2), &TerminalCell::wide("界", 2)); // writes its continuation cell too
    update.set_cell((3, 2), &TerminalCell::new("┌"));
    update.set_cursor_position((5, 0));
    update.set_cursor_visible(true);
});

App::new()
    .add_plugins((DefaultPlugins, TerminalPlugin))
    .add_systems(Startup, move |mut commands: Commands| {
        commands.spawn(Camera2d);
        commands.spawn(Terminal::new(surface.clone()));
    })
    .run();
```

`TerminalSurface::update` takes the lock once, applies the closure and
publishes at most one revision (none if nothing changed);
`TerminalSurface::begin_update` returns the same guard for producers that
need to interleave other work, and must not be held across frames. Positions
are anything `Into<CellPosition>` — `(x, y)` tuples or `CellPosition`.
`SurfaceUpdate` offers `set_cell`, `set_cells`, `set_cursor_position`,
`set_cursor_visible`, `clear`, `clear_row`, `clear_range` (row-major,
inclusive), `resize`, `scroll_up` and `scroll_down`. Every mutation compares
against the retained cell and marks only real changes dirty, so redrawing
identical content publishes nothing and the renderer's unchanged-frame fast
path stays intact.

## Rendering

Add `TerminalPlugin` once, then spawn a `Terminal` component per terminal:

```no_run
# use bevy::prelude::*;
# use bevy_terminal::prelude::*;
# let surface = TerminalSurface::new(80, 24);
# let mut app = App::new();
app.add_plugins(TerminalPlugin);
app.world_mut().spawn(
    Terminal::new(surface)
        .with_config(TerminalRenderConfig {
            cell_size: Vec2::new(11.0, 20.0),
            font: FontFaces::regular(bevy::text::FontSource::Monospace),
            font_size: FontSizing::FitCellWidth,
            ..default()
        })
        .with_presentation(Presentation::Ui { origin: Vec2::splat(16.0) }),
);
```

The plugin attaches to every `Terminal` entity:

- `TerminalTexture` — the renderer-owned `Handle<Image>`, its physical `size`,
  `logical_size`, `raster_scale`, physical `cell_size` and the effective
  `font_size`. The handle changes identity when the grid or raster scale
  changes and a `TerminalResized` entity event is triggered on the terminal.
- `TerminalStats` — per-frame counters (changed rows, snapshot cells, quads,
  draw batches, shape-cache hits/misses, timings).
- For `Presentation::Ui { origin }`, an `ImageNode` marked with
  `TerminalNode { terminal }`; `Presentation::Headless` renders only the
  texture, for custom composition, image export or benchmarks.

Terminals can be spawned and despawned at any time; despawning removes the
node and images. Editing `Terminal::config_mut()` rebuilds only that terminal.
Terminals share the GPU pipeline and scratch buffers but keep independent
surfaces, configurations, textures and statistics.

`TerminalRenderConfig` holds the explicit `cell_size`, the `FontFaces`
(regular plus optional bold/italic/bold-italic; missing faces fall back
bold-italic → bold → italic → regular), the `FontSizing` (`FitCellWidth` by
default: the regular font's advance is measured and the font sized so one
advance equals the cell width; `Px(size)` uses an explicit size),
`font_hinting` (unhinted by default so measured metrics stay exact), the
`TerminalTheme`, `CursorConfig { style, color, blink_hz }`, `BlinkConfig {
slow_hz, rapid_hz }` and `TerminalRenderScale` (`Automatic` follows the
primary window in UI mode and resolves to 1.0 headless; `Fixed(scale)`
rasterizes at a known scale).

The renderer reads the surface once per changed frame, copies only dirty cells
into its retained snapshot, and rebuilds only changed rows. Every glyph,
including box-drawing, block, shade and braille characters, is shaped and
rasterized from the configured font by Bevy text, cached in one renderer-owned
atlas and clipped to its cell; nothing is drawn procedurally.

Resizing a surface (`SurfaceUpdate::resize`) preserves the overlapping cells.
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
using the repository's Iosevka Fixed faces when run from a checkout and the
vendored JetBrains Mono Regular/Bold/Italic/BoldItalic faces under
`assets/fonts/jetbrains-mono` (OFL) otherwise. `scene_export` writes the same textures
headlessly under `target/bevy-terminal-qa/`.

## License

Licensed under the MIT license.
