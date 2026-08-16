# `bevy_grid`

`bevy_grid` is a Ratatui backend that renders terminal cells with Bevy's own UI
nodes and text pipeline. There is no software framebuffer, texture upload, egui,
or custom graphics pipeline in the runtime path.

The backend retains a Ratatui `Buffer` behind a small thread-safe surface handle.
A Bevy system reads that surface and creates exact UI rectangles for backgrounds
plus absolutely positioned Bevy text runs. Ordinary unit-width cells with the
same style are batched. Wide graphemes get their own run anchored at their exact
column, so font fallback cannot shift the content that follows them. Combining
marks remain part of Ratatui's original cell string and are shaped together by
Bevy. Common Ratatui box-drawing sets and full or fractional block elements are
emitted as exact Bevy UI geometry, which avoids the hairline seams that font
glyph bearings can introduce.

```no_run
use bevy::prelude::*;
use bevy_grid::prelude::*;
use ratatui::{Terminal, widgets::{Block, Borders, Paragraph}};

let backend = BevyBackend::new(60, 20);
let surface = backend.surface();
let mut terminal = Terminal::new(backend)?;

terminal.draw(|frame| {
    frame.render_widget(
        Paragraph::new("Hello from Ratatui")
            .block(Block::new().borders(Borders::ALL)),
        frame.area(),
    );
})?;

App::new()
    .add_plugins((DefaultPlugins, BevyGridPlugin::new(surface)))
    .add_systems(Startup, |mut commands: Commands| {
        commands.spawn(Camera2d);
    })
    .run();
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Cell geometry and Unicode

`TerminalRenderConfig::cell_size` is explicit because a shaped text run can use
several fallback fonts and there is no single metric valid for every Unicode
glyph. Pick a primary monospace font and a cell width matching its unit advance.
The default uses Bevy's generic system monospace family with a cell size suited
to an 18 px font. For deterministic production rendering, supply a font family
or loaded font asset and configure its measured cell dimensions.

Wide Ratatui cells are anchored to two columns regardless of the fallback
glyph's natural advance. This prevents column drift, but a poorly matched font
can still make the glyph look too narrow or too wide inside those two columns.
Mixed-weight and dashed box-drawing characters, shaded blocks, and braille
glyphs require a font designed to fill its advance and a line height matching
`cell_size.y`; otherwise the font itself may introduce visible seams. The
standard Ratatui single, heavy, and double border sets plus common full,
fractional, and quadrant blocks are rendered as UI geometry and do not depend
on glyph bearings. Rounded corners retain their font shape while their
adjoining geometry overlaps the cell boundary to prevent seams.

All Ratatui colors are supported, including the ANSI 256-color cube and true
color. Bold, dim, italic, underline, reverse, hidden, crossed-out, slow-blink,
and rapid-blink modifiers are mapped to Bevy text behavior. Bold and italic
depend on the selected font family exposing suitable faces or variable axes.

## Resizing

Resize the backend and then ask `ratatui::Terminal` to adopt the backend size:

```ignore
terminal.backend_mut().resize(columns, rows);
terminal.autoresize()?;
```

## Render QA

The `image_export` example uses the Git development dependency
`bevy_image_export` to write frames under `target/render-qa/`:

```text
cargo run --example image_export
```

Early frames contain the complete 72×22 scene. Later frames shrink the backend
to 60×18 so the exported sequence also catches row-stride errors and stale UI
entities after a resize.

The normal dependency list is deliberately limited to `bevy` and `ratatui`.

## Renderer performance comparison

`benchmarks/renderer-comparison` is a separate workspace containing a
windowless, synchronized Bevy harness for comparing `bevy_grid`,
`soft_ratatui`, `egui_ratatui`, `parley_ratatui`, and `bevy_tui_texture` with
identical Ratatui workloads. It reports raw JSON samples plus CSV/Markdown
summaries and keeps all third-party renderer dependencies outside this library's
runtime manifest. See
[`benchmarks/renderer-comparison/README.md`](benchmarks/renderer-comparison/README.md).

## Ratatui upstream example ports

The project also contains deterministic Bevy ports of all 43 runnable targets
from Ratatui 0.30.2: 32 application examples and 11 state-pattern binaries.
Run one port by slug or export the complete visual suite:

```text
cargo run --example ratatui_examples -- --list
cargo run --example ratatui_examples -- chart
cargo run --example ratatui_examples_export
```

The exporter writes stable frames beneath `target/ratatui-examples/<slug>/`.
Network responses, randomness, clocks, tracing subscribers, panic hooks, and
terminal-only input or escape behavior use documented deterministic fixtures.
See [RATATUI_EXAMPLES.md](RATATUI_EXAMPLES.md) for the pinned upstream commit,
complete inventory, and adaptation policy.

## Current scope

One `BevyGridPlugin` terminal surface is supported per Bevy `App`. The backend
models the visible grid only: rows scrolled past the top are discarded rather
than retained as host-side scrollback.

## License

Licensed under the MIT license.
