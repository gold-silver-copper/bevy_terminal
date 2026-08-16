# Implementation Prompt: `bevy_grid`

Create a new Rust library project named `bevy_grid` in this directory. The
library must provide a Ratatui backend and renderer that displays a Ratatui
terminal entirely with Bevy's built-in UI and text-rendering capabilities. It
should occupy the same design space as `soft_ratatui` and `egui_ratatui`, but it
must not use either project or any non-Bevy rendering layer at runtime.

## Hard constraints

- The library's only normal dependencies must be `bevy` and `ratatui`.
- Use `https://github.com/paulkre/bevy_image_export` only as a development
  dependency for render verification.
- Backwards compatibility and breaking-semver concerns do not apply.
- The result must be a reusable library, not merely a demo application.
- Rendering must use Bevy UI nodes and Bevy text. Do not implement a custom
  wgpu pipeline, use egui, render terminal cells to a software bitmap at
  runtime, or add helper crates.
- Keep the public API small, documented, and idiomatic for both Bevy and
  Ratatui users.

## Research and design work

Before committing to an implementation, inspect:

- the local sibling project at `../learnchinese`, especially the code that
  lays text out in a monospace grid using Bevy UI;
- `soft_ratatui` and `egui_ratatui` for useful backend/lifecycle API ideas;
- the current `ratatui::backend::Backend` contract and buffer/cell semantics;
- the current Bevy UI and text APIs; and
- `bevy_image_export`'s current setup and headless/export workflow.

Choose the renderer architecture based on terminal correctness. In particular,
do not assume that one Bevy text entity per terminal cell is correct or fast
enough without validating it. Prefer batching cells into styled text spans or
runs where possible, while using Bevy UI nodes for cell backgrounds and other
geometry. Explain important architectural choices in crate documentation.

## Required terminal behavior

Implement the Ratatui backend operations needed to construct and draw through a
`ratatui::Terminal`, including querying the terminal size, drawing changed
cells, clearing, cursor positioning and visibility, appending lines or an
explicitly documented equivalent, flushing updates into Bevy-visible state,
and resizing.

The renderer must preserve terminal-cell geometry and Ratatui styling:

- fixed-width columns and fixed-height rows;
- foreground and background colors, including reset/default colors and all
  Ratatui color variants;
- supported text modifiers such as bold, italic, underlined, crossed out,
  reversed, hidden, and dim, with documented behavior where Bevy/font support
  imposes a limitation;
- cursor location and visibility;
- Unicode grapheme content and Ratatui's cell-width semantics;
- double-width characters occupying exactly two terminal columns without
  shifting following content;
- zero-width/combining content remaining attached to the intended cell;
- box-drawing, block, and braille characters aligning without visible seams;
  and
- resizing without stale entities, stale styles, or out-of-bounds updates.

Use a monospace font whose glyph coverage is suitable for terminal rendering in
tests/examples. Derive the cell size from actual text metrics when Bevy exposes
reliable metrics; otherwise make cell size an explicit configurable property
and document the requirement that the chosen font and size agree with it.

## Bevy integration

Provide an idiomatic Bevy plugin and components/resources/events or systems that
bridge the Ratatui backend's buffered updates into the Bevy world. The API must
make ownership and scheduling clear: Ratatui may render through its `Terminal`
without requiring mutable access to the Bevy `World`, while a Bevy system later
applies the latest frame. Avoid global state. Make the bridge deterministic and
safe to use across Bevy schedules/threads.

Include at least one example that launches a Bevy window and renders a
representative Ratatui UI. The example should demonstrate borders, styled text,
background colors, cursor behavior, Unicode (including CJK and combining text),
double-width cells, block/braille content, and resizing or configurable grid
dimensions.

## Verification

Add focused tests for backend semantics and frame conversion. Include tests for
partial draws, clears, cursor operations, resize behavior, color conversion,
modifiers, Unicode graphemes, double-width cells, combining content, and invalid
or clipped coordinates.

Use `bevy_image_export` in a development-only render harness to save deterministic
PNG images of representative terminal scenes. Inspect those images at native
resolution. Pay special attention to:

- gaps or overlaps in connected box-drawing and block characters;
- column drift after CJK, emoji, or other wide graphemes;
- combining marks being split from their base glyph;
- background rectangles leaving seams;
- text baselines, clipping, and cursor alignment; and
- correctness after resize.

Where exact pixel rendering is platform/font dependent, test invariant geometry
and state in Rust and use exported images as explicit visual QA artifacts rather
than brittle golden snapshots. Keep generated QA images out of the published
crate unless a small checked-in reference image adds clear value.

## Quality gate

- Format with `cargo fmt`.
- Pass `cargo check --all-targets` and `cargo test --all-targets`.
- Run Clippy across all targets and features, treating warnings as errors unless
  an upstream dependency makes that impossible and the exception is documented.
- Build crate documentation with warnings denied.
- Inspect the full final change set for correctness, regressions, unsafe edge
  cases, accidental extra dependencies, and missing tests.
- Verify from `Cargo.toml` and `cargo metadata` that normal dependencies are
  exactly `bevy` and `ratatui`, and that `bevy_image_export` is development-only.

Deliver the complete library source, examples, tests, concise documentation,
the image-export QA harness, and a summary of verification and any known Bevy or
font-rendering limitations.
