# Ratatui example ports

This suite covers every runnable target under Ratatui 0.30.2's `examples/`
directory at upstream commit
`e665c36cb14752a61cd777fbd06dbef8474f2add`:

- 32 application examples under `examples/apps/`.
- 11 binaries under `examples/concepts/state/src/bin/`.
- 43 Bevy-rendered scenes in one interactive gallery.

The ports preserve each example's primary widgets, layout, colors, controls,
and visual purpose. Crossterm/Termion/Termwiz setup and event loops are replaced
by Bevy keyboard and mouse systems feeding the `RatatuiBackend`. Data sources that
would require dependencies beyond Bevy and Ratatui use local fixtures:

- GitHub and weather responses are local sample data.
- Random charts, temperatures, people, and market data are fixed.
- Animation, selection, scrolling, mouse drawing, forms, and Unicode text input
  are interactive in the gallery.
- Panic-hook, tracing-subscriber, and inline-terminal behavior is represented
  by the UI state it produces.
- OSC-8 hyperlinks retain their underlined visual affordance; Bevy UI does not
  provide a terminal hyperlink target.

The source catalog in
`examples/ratatui_examples/mod.rs` stores an adaptation note and upstream path
for every entry.

## Running and exporting

```text
# list every slug
cargo run --example ratatui_examples -- --list

# open the gallery at its first example
cargo run --example ratatui_examples

# optionally choose the gallery's starting example
cargo run --example ratatui_examples -- volatility-surface

# export several stable frames of every scene
cargo run --example ratatui_examples_export
```

Exported PNGs are written to `target/ratatui-examples/<slug>/`. The exporter
uses the Git development dependency `bevy_image_export`; it is not a normal
library dependency. It creates a fresh canonical state for every capture, so
exports stay deterministic even though the gallery is interactive.

Both the gallery and exporter load the vendored Iosevka Fixed 34.8.0 family
(falling back to the embedded JetBrains Mono faces). Regular, bold, italic, and
bold-italic Ratatui text is bound directly to
the corresponding face, so rendering does not depend on fonts installed on the
host or accidentally select an emoji presentation for supported symbols.

## Gallery controls

Global controls are deliberately assigned to function/page keys so they do not
steal the arrows, Vim keys, text, or punctuation used by an example:

| Key | Action |
|---|---|
| `PageDown` or `F6` | Next example, wrapping at the end |
| `PageUp` or `Shift+F6` | Previous example, wrapping at the beginning |
| `F1` | Contextual help for the current example |
| `F2` | Reset only the current example |
| `F10` | Exit the gallery |

The window title always shows the current slug and catalog position. State is
preserved when switching away from an example. Press `F2` when a clean state is
preferred.

The interactive gallery window is resizable. Its Ratatui backend keeps crisp
10×18 cells and changes its column/row count to fill the available window area;
the current example redraws into the new grid. The headless exporter remains
fixed at 100×62 so visual-regression output stays deterministic.

The contextual `F1` panel is the authoritative control reference. Interactive
ports use these upstream-equivalent controls:

| Example | Controls |
|---|---|
| `async-github` | `j/k` or `Up/Down` scroll pull requests |
| `calendar-explorer` | arrows or `h/j/k/l` move by day/week; `n/p` or `Tab/Shift+Tab` move by month; `s` changes style |
| `canvas` | arrows or `h/j/k/l` move; `Enter` changes marker; drag draws/moves |
| `constraint-explorer` | arrows select/edit; `1`–`6` change type; `a/x` add/delete; `+/-` spacing |
| `constraints` | arrows or `h/j/k/l` select the constraint tab/item; `Home/End` jump |
| `custom-widget` | `Left/Right` or `h/l` select; `Space`, `Enter`, or left click toggles |
| `demo` | `h/l` changes tabs; `j/k` changes selection; `t` toggles the chart |
| `demo2` | `h/l` or `Tab` changes tabs; `j/k` controls the active tab; `d/Delete` starts destroy mode |
| `flex` | arrows or `h/j/k/l` select mode/row; `+/-` spacing; `Home/End` jump |
| `gauge` | `Space/Enter` restarts the animation |
| `input-form` | `Tab/Shift+Tab` changes fields; typing/Backspace edits; arrows change age; `Enter` submits; `Esc` cancels |
| `mouse-drawing` | left-drag draws; `Space` changes color; `c` clears |
| `panic` | `p/e/h` safely render the panic/error/disabled-hook states without crashing Bevy |
| `popup` | `p` toggles the popup |
| `scrollbar` | arrows or `h/j/k/l` scroll; mouse wheel scrolls vertically |
| `table` | `j/k` changes row; `h/l` changes column; `Shift+Left/Right` changes highlight color |
| `todo-list` | `j/k`, `Home/End` select; `h` clears; `l/Right/Enter` toggles completion |
| `user-input` | normal mode: `e` edits and `q` quits; editing: Unicode text, arrows, Backspace/Delete, `Enter`, `Esc` |
| `volatility-surface` | arrows or `h/j/k/l` rotate; `z/x` zoom; `p` palette; `Space` pause; `Ctrl+R` reset |

Time-driven examples animate automatically. The 11 state-pattern examples
increment their counters on gallery ticks, mirroring the upstream examples'
render-mutation loops. Examples without mutable upstream behavior remain visual
demos; `q` and the gallery-wide `F10` close the window. This replaces the
terminal examples' various single-key exit loops without stealing navigation.

## Application inventory

| Port slug | Upstream example |
|---|---|
| `advanced-widget-impl` | `examples/apps/advanced-widget-impl` |
| `async-github` | `examples/apps/async-github` |
| `calendar-explorer` | `examples/apps/calendar-explorer` |
| `canvas` | `examples/apps/canvas` |
| `chart` | `examples/apps/chart` |
| `color-explorer` | `examples/apps/color-explorer` |
| `colors-rgb` | `examples/apps/colors-rgb` |
| `constraint-explorer` | `examples/apps/constraint-explorer` |
| `constraints` | `examples/apps/constraints` |
| `custom-widget` | `examples/apps/custom-widget` |
| `demo` | `examples/apps/demo` |
| `demo2` | `examples/apps/demo2` |
| `flex` | `examples/apps/flex` |
| `gauge` | `examples/apps/gauge` |
| `hello-world` | `examples/apps/hello-world` |
| `hyperlink` | `examples/apps/hyperlink` |
| `inline` | `examples/apps/inline` |
| `input-form` | `examples/apps/input-form` |
| `minimal` | `examples/apps/minimal` |
| `modifiers` | `examples/apps/modifiers` |
| `mouse-drawing` | `examples/apps/mouse-drawing` |
| `panic` | `examples/apps/panic` |
| `popup` | `examples/apps/popup` |
| `release-header` | `examples/apps/release-header` |
| `scrollbar` | `examples/apps/scrollbar` |
| `table` | `examples/apps/table` |
| `todo-list` | `examples/apps/todo-list` |
| `tracing` | `examples/apps/tracing` |
| `user-input` | `examples/apps/user-input` |
| `volatility-surface` | `examples/apps/volatility-surface` |
| `weather` | `examples/apps/weather` |
| `widget-ref-container` | `examples/apps/widget-ref-container` |

## State-pattern inventory

| Port slug | Upstream binary |
|---|---|
| `state-component-trait` | `component-trait.rs` |
| `state-immutable-consuming` | `immutable-consuming.rs` |
| `state-immutable-function` | `immutable-function.rs` |
| `state-immutable-shared-ref` | `immutable-shared-ref.rs` |
| `state-mutable-function` | `mutable-function.rs` |
| `state-mutable-widget` | `mutable-widget.rs` |
| `state-nested-mutable-widget` | `nested-mutable-widget.rs` |
| `state-nested-stateful-widget` | `nested-stateful-widget.rs` |
| `state-refcell` | `refcell.rs` |
| `state-stateful-widget` | `stateful-widget.rs` |
| `state-widget-with-mutable-ref` | `widget-with-mutable-ref.rs` |

The state examples intentionally produce similar pixels because their upstream
distinction is Rust ownership and widget API structure rather than layout. Each
is still cataloged, rendered, exported, and checked independently.
