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

| concept | type | role |
|---|---|---|
| surface | `TerminalSurface` | thread-safe retained grid producers write into |
| cell | `TerminalCell` (`CellSymbol` + `TerminalStyle` + occupancy) | one grid cell |
| snapshot | `TerminalSnapshot` | owned copy of the grid the renderer reads incrementally |
| terminal entity | `TerminalRenderer` + `TerminalRenderConfig` (+ your `ImageNode`) | one rendered terminal |
| texture | `TerminalTexture` | the renderer-owned `Rgba8UnormSrgb` image (stable handle), its size, cell size and font size |
| config | `TerminalRenderConfig` | sizing (`TerminalSizing`), fonts, theme, cursor, blink, raster |
| features | `ui`, `system_fonts` (both default) | UI `ImageNode` presentation; system font discovery |

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

let surface = TerminalSurface::new((40, 10));
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
        commands.spawn((
            TerminalRenderer::new(surface.clone()),
            ImageNode::default(),
            Node { position_type: PositionType::Absolute, left: px(16.0), top: px(16.0), ..default() },
        ));
    })
    .run();
```

`TerminalSurface::update` is the only way to write: it takes the lock once,
applies the closure and publishes at most one revision (none if nothing
changed), returning whether it did. Positions are anything
`Into<CellPosition>` — `(x, y)` tuples or `CellPosition`; sizes anything
`Into<GridSize>`. `SurfaceUpdate` offers `set_cell`, `set_cursor_position`,
`set_cursor_visible`, `clear`, `clear_row`, `clear_range` (row-major,
inclusive), `resize`, `scroll_up` and `scroll_down`. Every mutation compares
against the retained cell and marks only real changes dirty, so redrawing
identical content publishes nothing and the renderer's unchanged-frame fast
path stays intact.

## Rendering

Add `TerminalPlugin` once, then spawn a `TerminalRenderer` component per terminal
together with a `TerminalRenderConfig` (required; a default is inserted if you
omit it) and, to show it, an `ImageNode`:

```no_run
# use bevy::prelude::*;
# use bevy_terminal::prelude::*;
# let surface = TerminalSurface::new((80, 24));
# let mut app = App::new();
app.add_plugins(TerminalPlugin);
app.world_mut().spawn((
    TerminalRenderer::new(surface),
    TerminalRenderConfig {
        sizing: TerminalSizing::FitCellWidth(Vec2::new(11.0, 20.0)),
        font: FontFaces::regular(bevy::text::FontSource::Monospace),
        ..default()
    },
    ImageNode::default(),
    Node { position_type: PositionType::Absolute, left: px(16.0), top: px(16.0), ..default() },
));
```

The plugin attaches to every `TerminalRenderer` entity:

- `TerminalTexture` — the renderer-owned `Handle<Image>` (stable for the
  terminal's lifetime; resizes reallocate the image in place), its physical
  `size`, `logical_size`, `raster_scale` and the effective `font_size`.
  `TerminalReady { entity }` is triggered on the entity once the texture
  has measured geometry. Query `TerminalTexture::measured()` for persistent
  readiness; this does not promise GPU completion.
- `TerminalStats` — per-frame counters (changed rows, snapshot cells, quads,
  draw batches, shape-cache misses, timings; `Display` prints a one-line
  summary).

If the entity has an `ImageNode`, the plugin keeps its image and its `Node`
width/height in sync with the texture; where the node goes (absolute position,
a flex/grid parent, …) is ordinary Bevy UI layout. Without an `ImageNode` the
terminal is headless — only the texture is produced, for custom composition,
image export or benchmarks — and `TerminalRenderScale::Automatic` resolves to
1.0.

Terminals can be spawned and despawned at any time. The renderer releases its
image handles with the entity; application-held clones keep their assets alive.
Mutating `TerminalRenderConfig` rebuilds only that terminal.
Terminals share the GPU pipeline and scratch buffers but keep independent
surfaces, configurations, textures and statistics.

`TerminalRenderConfig` holds `TerminalSizing`, the `FontFaces`
(regular plus optional bold/italic/bold-italic; missing faces fall back
bold-italic → bold → italic → regular, and `synthesize` decides whether the
fallback face is asked for the bold weight / italic style),
the `TerminalTheme`, `CursorConfig { style, color, blink_hz }`, `BlinkConfig {
slow_hz, rapid_hz }` and `RasterConfig { scale, hinting }` (`scale`:
`TerminalRenderScale::Automatic` follows the primary window when presented and
resolves to 1.0 headless, `Fixed(scale)` rasterizes at a known scale;
`hinting` is `FontHinting::Disabled` by default so measured metrics stay
exact).

The renderer reads the surface once per changed frame, copies only dirty cells
into its retained snapshot, and rebuilds only changed rows. Each renderer has an independent snapshot
reader, so several renderers can share content without consuming one another's
changes. Bevy shapes and rasterizes text into a renderer-owned atlas; Block
Elements (U+2580..U+259F, shades excepted) use solid geometry to tile exactly.

Resizing a surface (`SurfaceUpdate::resize`) preserves overlapping cells.
The surface owns content, cursor, and revision metadata (`surface.info()`);
presentation metrics belong to each renderer's `TerminalTexture`. Use its
`measured()` accessor before driving layout. It returns a `TerminalGeometry`
with coherent logical/physical dimensions, scale, and source grid generation.
Its accessors are `size()`, `logical_size()`, `cell_size()`, `font_size()`,
`raster_scale()`, and `grid_for(available)`. A shared resize immediately makes
old output unmeasured, including a resize back to the original dimensions. `status` identifies loading,
missing resources, invalid sizing, font/shaping failures, and texture limits.
Readiness is a measurement contract, not a GPU completion signal.

## Cell sizing, texture format, features

- `TerminalSizing::FitCellWidth(Vec2)` (default: 11×20) fits a font to an
  explicit cell width, growing height as needed. `FromFont { font_size,
  line_height }` derives cells from the measured advance and line box;
  `TerminalSizing::font(size)` uses line height 1. `Fixed { cell_size,
  font_size }` specifies both sizes and clips glyphs beyond the cell.
  Values must be finite and positive. Surface grids are bounded by
  `TerminalSurface::MAX_CELLS`, and texture allocation respects device limits.
  `TerminalGeometry::cell_size` reports the
  logical cell in use, `TerminalGeometry::grid_for` / `render::grid_for` /
  `render::grid_for_window` compute the grid that fits a size, and
  `render::raster_scale_for_window` the physical/logical ratio for
  `TerminalRenderScale::Fixed`.
- Vertical fit: the renderer measures the primary font's line box (its full
  block's fully covered rows) and the ink of ASCII and accented-capital probes
  in every face, then shifts all glyphs by one whole-pixel offset so blocks
  keep covering the cell, ASCII (descenders included) is never clipped and
  accents fit when there is room. With `FitCellWidth`/`FromFont` the cell
  height is the font's line box (ascent + descent + leading from its metrics
  tables, grown to the block glyph when that is taller); with `TerminalSizing::Fixed` the
  configured sizes are exact and glyphs beyond the cell are clipped. Runs
  wider than their cell (fallback families, wide italics) drop their faintest
  columns; overhanging runs are pushed inside, but a sub-pixel overshoot (the
  fraction of a column that box-drawing glyphs are drawn past their advance so
  strokes overlap) is clipped in place so corners stay aligned with stems.
  Unicode Block Elements (U+2580..U+259F, shades excepted) are drawn as
  geometry rather than glyphs, as Ghostty does, so halves, eighths and
  quadrants tile the cell exactly with any font. The
  font is sized to the
  physical cell width so fractional raster scales do not open seams.
- The texture is `Rgba8UnormSrgb` with straight alpha; `TerminalTheme::background`
  alpha is honored (backgrounds replace, glyphs blend), so translucent
  terminals composite correctly.
- Cargo features: `ui` (default) enables `ImageNode` presentation and following
  the UI scale — without it terminals are headless and `Automatic` scale is
  1.0; `system_fonts` (default) enables system font discovery — without it
  supply font assets (or `FontSource::Handle(Handle::default())` with
  `bevy/default_font`); `3d` enables `TerminalWorldQuad`, an unlit world-space
  rectangle whose width follows the texture aspect. Font fallback for missing
  glyphs is Bevy's system-wide fallback; there is no per-terminal fallback
  list in Bevy 0.19.
- Events: `TerminalReady` fires once per terminal when its texture has been
  measured; `TerminalRemeasured` fires on every later size change (resize,
  config change, late font) with the previous and new size, for presentation
  the plugin does not manage itself.
- Font families: `font_family("Name")` builds a `FontSource::Family` from any
  string (`SmolStr` is re-exported); the `TerminalFonts` system parameter
  answers `has_family`, `resolve_family(&[preferred...])` and `families()`
  from Bevy's font collection.
- Bevy: any `0.19.x` (see the compatibility table in the workspace README).

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
headlessly under `target/bevy-terminal-qa/`. It exits after saving one PNG for
each static scene. The example waits for correctly sized, nonempty GPU readbacks;
it does not assume a fixed number of frames is enough for font/image preparation.

## License

Licensed under the MIT license.
