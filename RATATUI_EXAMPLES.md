# Ratatui example ports

This suite covers every runnable target under Ratatui 0.30.2's `examples/`
directory at upstream commit
`e665c36cb14752a61cd777fbd06dbef8474f2add`:

- 32 application examples under `examples/apps/`.
- 11 binaries under `examples/concepts/state/src/bin/`.
- 43 deterministic Bevy-rendered scenes in total.

The ports preserve each example's primary widgets, layout, colors, and visual
purpose. Crossterm/Termion/Termwiz setup and event loops are replaced by the
`BevyBackend`. Inputs that would make screenshots nondeterministic or require
dependencies beyond Bevy and Ratatui are replaced by fixed fixtures:

- GitHub and weather responses are local sample data.
- Random charts, temperatures, people, and market data are fixed.
- Time, animation, selection, mouse, and form state are frozen.
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

# open one scene in a Bevy window
cargo run --example ratatui_examples -- volatility-surface

# export several stable frames of every scene
cargo run --example ratatui_examples_export
```

Exported PNGs are written to `target/ratatui-examples/<slug>/`. The exporter
uses the Git development dependency `bevy_image_export`; it is not a normal
library dependency.

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
