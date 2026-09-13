# `bevy_terminal`

A low-level terminal cell renderer built on Bevy text and the render world.
It has no Ratatui dependency and owns no windows, UI layout, cameras, meshes,
or materials.

**Terminal cells + configuration → image + validated geometry + status.**

## Surface model

`TerminalSurface` is a cloneable shared handle. `update` applies a transaction and
publishes a revision only when content changes. Dirty tracking supports partial
row updates. `TerminalSnapshot` provides read-only cells, rows, cursor state,
and text extraction.

Cells carry symbols, colors, style flags, and explicit wide-cell occupancy.
Unicode shaping and fallback remain aligned to the terminal grid. The surface
also supports clearing and scrolling; it does not parse escape sequences or
manage a PTY.

```rust
use bevy_terminal::prelude::*;

let surface = TerminalSurface::new((80, 24));
surface.update(|update| {
    update.set_cell((0, 0), &TerminalCell::new("A")
        .with_style(TerminalStyle::new().fg(TerminalColor::CYAN)));
    update.set_cursor_position((1, 0));
    update.set_cursor_visible(true);
});
assert_eq!(surface.snapshot().row_text(0).trim_end(), "A");
```

## Rendering and output

Add `TerminalPlugin` after Bevy's asset, text, and render plugins, then spawn a
`TerminalRenderer` for each surface. A default `TerminalRenderConfig` is required
and inserted automatically. Supply your own sizing, fonts, theme, cursor, blink,
and raster settings when needed.

```rust,no_run
use bevy::prelude::*;
use bevy_terminal::prelude::*;

fn setup(mut commands: Commands) {
    let surface = TerminalSurface::new((80, 24));
    commands.spawn((
        TerminalRenderer::new(surface),
        TerminalRenderConfig {
            sizing: TerminalSizing::font(18.0),
            raster: RasterConfig { scale: 2.0, ..default() },
            ..default()
        },
    ));
}
```

The plugin attaches `TerminalTexture` and `TerminalStats`. Read output after
`TerminalSystems::Sync`. `texture.measured()` returns validated geometry only
when the selected fonts and cell metrics are usable and its source surface has
not been resized since measurement. Status exposes loading, invalid sizing,
font/shaping failures, missing text resources, and device texture limits.

`TerminalGeometry` provides:

- `grid()`: measured columns and rows.
- `size()`: physical image dimensions.
- `logical_size()`, `cell_size()`, `font_size()`: logical pixel measurements.
- `raster_scale()`: physical-to-logical pixel ratio.
- `grid_for(available_size)`: a bounded grid fitting a logical rectangle.
- `is_current()` and `matches_surface(...)`: source/generation validation.

Readiness is persistent, so late consumers need no event history. Retain the
last useful application layout while new geometry is unavailable. Geometry
validation rejects old grid measurements immediately after shared resizing.

The image handle is stable throughout the renderer's lifetime, including
resizes. Output uses `Rgba8UnormSrgb`, straight alpha, and nearest sampling.
Applications bind it to their own UI, sprites, materials, or export targets.
Rendering has bounded caches and reuses assets and GPU resources for idle and
partial-update workloads.

**Measured geometry does not prove GPU completion.** Image exports must observe
render-world readback and verify the intended content. The static scene export
example uses known content to distinguish initial zeroed images from rendered
output; that check does not apply to arbitrary animated scenes.

## Application-owned presentation

The renderer does not modify `Node`, `ImageNode`, `Transform`, or material assets.
An application presentation system can query `TerminalTexture`, bind its image,
and size its own UI or mesh from measured logical dimensions.

`RasterConfig.scale` is always explicit and defaults to `1.0`. The application
chooses its window/camera/UI policy and updates configuration before sync.
Non-finite/non-positive values fall back to `1.0`; positive values clamp to
`1..=8`. Physical cells snap to pixels; logical geometry may remain fractional.
There is no presentation-dependent or primary-window-dependent scale behavior.

## Sizing and fonts

`TerminalSizing` supports font-driven cells (`FromFont`), fitting font advance
to a requested cell width (`FitCellWidth`), and explicit cells/font size (`Fixed`).
Natural and width-fitted rows enclose the configured faces' typographic metrics
in whole pixels. Text and box-drawing alignment are measured separately; faint
text-edge pixels are preserved whenever the run fits its cell span.
Styled and fallback runs share the configured text baseline. Fallback glyphs
do not change cell measurements when the displayed content changes.
Font-driven sizing accepts a line-height multiplier; values below one can clip
outer ink intentionally. Fixed geometry fits/clips glyphs to the specified cells.

`FontFaces` selects regular, bold, italic, and bold-italic sources. Missing faces
can request weight/style from fallback faces. Supply Bevy `Font` assets, generic
font sources, or a named `FontSource::Family`. Bevy performs font
registration and fallback. Applications implementing family selection should
query `FontCx.collection` directly after registration.

`TerminalTheme`, `CursorConfig`, and `BlinkConfig` control terminal rendering
semantics. These do not prescribe application presentation or input behavior.

## Features

- `system_fonts` (default): discover host fonts. Disable defaults and supply
  explicit assets for deterministic font selection.
- `timings` (opt-in): record CPU snapshot/scene durations. Work counters remain
  available without this feature; timing fields are then zero.

There are no UI or 3D presentation features. Applications enable the Bevy
features their own presentation requires. This checkout targets Bevy 0.19 and
Rust 1.95 or newer.

## Examples and verification

```sh
cargo run -p bevy_terminal --example scene_export
cargo test -p bevy_terminal
cargo test -p bevy_terminal --test gpu_readback -- --ignored --test-threads=1
```

`scene_export` reads two scenes written through the neutral cell API back into
images without a window. The repository's adapter examples demonstrate UI layout
and DPI policy (`demo`) and application-owned 3D material binding (`world_quad`).

## License

MIT.
