# Prompt: Split `bevy_grid` into `bevy_terminal` and `bevy_terminal_ratatui`

You are working in the existing `bevy_grid` Rust repository. Implement a clean
two-crate architecture and rename the project as follows:

- `bevy_terminal`: a reusable Bevy-only terminal scene model and renderer;
- `bevy_terminal_ratatui`: a Ratatui backend/adaptor built on top of
  `bevy_terminal`, plus the convenient facade most Ratatui users will install.

This is an implementation task, not a request for an architectural proposal.
Inspect the current implementation, perform the split, migrate the examples and
benchmark harness, test it, visually validate it, and benchmark the completed
result. Do not stop after adding empty crates, moving files without removing
the dependency leak, or writing a future-work plan.

Backwards compatibility and breaking semver are not concerns. Prefer a clear,
small public API over aliases for old `bevy_grid` names. Do not commit, push,
publish crates, or mutate GitHub unless explicitly asked after the work is
complete. Preserve unrelated user changes in the dirty worktree.

## Two non-negotiable outcomes

Everything below serves two outcomes. If a design choice trades one against the
other, stop and choose the option that satisfies both; do not silently pick one.

1. **A real crate boundary.** `bevy_terminal` compiles, tests, documents and
   packages with zero knowledge of Ratatui, in any dependency kind, feature, or
   target. Ratatui semantics stop at the `Backend` implementation in the upper
   crate. This must be *proved mechanically* (see "Boundary proof"), not just
   asserted in a report.
2. **No performance regression.** The split must not add a copy, allocation,
   lock, or translation pass on any hot path: the Ratatui `draw` diff, the
   per-frame renderer snapshot update, or the render-world extraction. The
   existing benchmark protocol is the gate (see "Performance gate"), and it must
   pass without loosening.

## Required dependency graph

The normal dependency graph must be exactly:

```text
bevy_terminal ──> bevy

bevy_terminal_ratatui ──> bevy_terminal
                         └> ratatui
```

The only external normal dependencies across the two library crates must remain
Bevy and Ratatui.

- `bevy_terminal` must not depend on Ratatui directly, optionally, through a
  feature, as a target-specific dependency, as a build dependency, or as a dev
  dependency. Its tests and examples may not use Ratatui either.
- `bevy_terminal_ratatui` may normally depend only on `bevy_terminal` and
  `ratatui`. Add a direct Bevy dev dependency only where its examples/tests need
  to construct a Bevy application. If its public API needs Bevy types (it will:
  the plugin, `Handle<Image>`, components), obtain them via `bevy_terminal`
  re-exports or a normal `bevy` dependency with the *same* feature set as the
  lower crate — do not let feature unification differ between the crates.
- Retain `bevy_image_export` as a development dependency for visual QA, but do
  not add other rendering, shaping, font, UI, synchronization, bitflag, string,
  or data structure crates.
- Keep `unsafe_code = "forbid"` and `missing_docs = "warn"` in both crates.
- Continue to use only Bevy's public text, UI, asset, render-device, render-world,
  image, and WGPU APIs for rendering.
- Do not copy or vendor another renderer to satisfy the dependency constraint.

## Workspace layout

Use the current repository root as the `bevy_terminal_ratatui` package and also
as the Cargo workspace root. Add the lower-level renderer beneath `crates/`:

```text
Cargo.toml                         # package bevy_terminal_ratatui + workspace
src/                               # Ratatui adapter/facade only
examples/                          # Ratatui examples and visual QA
assets/                            # fonts needed by packaged examples
crates/
└── bevy_terminal/
    ├── Cargo.toml
    ├── README.md
    ├── assets/                    # fonts needed by its own packaged examples
    ├── src/
    │   ├── lib.rs
    │   ├── scene.rs               # neutral cell/style/color/snapshot model
    │   ├── surface.rs             # retained surface + update transactions
    │   ├── color.rs               # theme + default color resolution
    │   └── renderer/
    └── examples/                  # at least one Ratatui-free scene example
```

The exact source module grouping may change when a better cohesive layout
becomes apparent, but do not create a third library crate. Keep the separate
renderer-comparison workspace isolated as it is today.

The root manifest should use a publishable path-plus-version dependency:

```toml
bevy_terminal = { version = "0.1.0", path = "crates/bevy_terminal" }
```

Ensure `cargo package --list` and `cargo package` work independently for both
crates. Files referenced with `include_bytes!`, including the JetBrains Mono
example fonts and OFL license, must reside within the package that includes
them; do not rely on files outside a crate's publishable package boundary
(`../../assets/...` from `crates/bevy_terminal` is not acceptable).

## Architectural boundary

`bevy_terminal` must consume a renderer-neutral terminal scene, not a
`ratatui::Buffer`, `ratatui::buffer::Cell`, `CellWidth`, `CellDiffOption`,
`Modifier`, `Color`, `Style`, `Position`, `Size`, or `Rect`. The lower crate owns
terminal semantics needed for rendering; the upper crate owns Ratatui semantics
and converts them at the backend boundary.

Do not merely move the current `renderer.rs`, `renderer/batch.rs`, `color.rs`
and `backend.rs`. Today:

- `TerminalSnapshot` wraps a `ratatui::Buffer` and the renderer walks
  `snapshot.buffer().content`;
- `renderer/batch.rs` imports `CellWidth`/`CellDiffOption` to detect wide and
  continuation cells;
- `color.rs` resolves `ratatui::style::Color`;
- renderer tests build scenes with `ratatui::buffer::Cell` and `Modifier`, and
  assert on `ratatui::layout::Size`.

Every one of those must be replaced by the neutral model *first*, then the
renderer ported onto it. Ratatui-shaped names, docs and comments must go too;
the lower crate's docs should read naturally to a producer that has never heard
of Ratatui.

### Neutral scene model

Design documented equivalents of these concepts in `bevy_terminal`:

- grid size and cell position using small integer terminal coordinates;
- `TerminalCell`, containing a symbol/grapheme, style, and occupancy/span;
- an explicit distinction between an ordinary glyph cell, a wide glyph anchor,
  and a continuation cell;
- `TerminalStyle`, including foreground, background, underline color, bold,
  italic, dim, reversed, hidden, underline, crossed-out, slow blink, and rapid
  blink;
- `TerminalColor`, supporting default/reset, indexed ANSI/256 colors, and RGB;
- cursor position and visibility;
- `TerminalTheme` and contextual resolution of default foreground/background;
- an owned `TerminalSnapshot` containing everything the renderer needs.

A possible shape is:

```rust
pub struct TerminalCell {
    pub symbol: String,
    pub occupancy: CellOccupancy,
    pub style: TerminalStyle,
}

pub enum CellOccupancy {
    Single,
    Wide { columns: u16 },
    Continuation,
}

pub enum TerminalColor {
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}
```

This is illustrative rather than mandatory. Choose invariants that make invalid
wide-cell states difficult to express and keep the hot path efficient. Use the
standard library rather than adding `bitflags`, compact-string, or locking
dependencies.

Hot-path guidance for the model:

- The cell type is stored once per grid cell and copied on every incremental
  snapshot update, so keep it small and cheap to clone/compare. A heap
  `String` per cell is acceptable only if the common ASCII/BMP case avoids a
  heap allocation (e.g. a small inline symbol buffer with `String` spill, or
  interning); measure `snapshot_ns` before deciding. Do not add a dependency for
  this.
- Modifiers should be a `Copy` bit set built from `u16`/`u8` with `const`
  helpers, not a struct of eleven `bool`s that defeats fast equality.
- `TerminalStyle`, `TerminalColor`, and occupancy must be `Copy + Eq` so the
  dirty-cell diff and the batch's style comparison remain a few integer compares.
- Wide-glyph anchoring: the renderer must be able to tell "anchor spanning N
  columns" from "continuation" from the cell alone, without re-measuring symbol
  width. Width measurement is the *producer's* job (Ratatui does it); the lower
  crate only needs to trust and clip.

### Surface and update transactions

Move the thread-safe retained surface, complete scene state, dirty-cell bits,
revision tracking, incremental snapshot updates, resizing, overlap preservation,
scrolling, and cursor state into `bevy_terminal`.

Expose a batched/transactional update API so an adapter can acquire the surface
once, update many cells, and publish no more than one new revision:

```rust
let mut update = surface.begin_update();
update.set_cell(column, row, cell);
update.set_cursor_position(column, row);
update.set_cursor_visible(true);
// Commit explicitly or publish on drop only when something actually changed.
```

Rules for the update path:

- One lock acquisition per Ratatui `draw`/`flush`, `clear`, `scroll`, or
  `resize` call, held for the whole transaction. Do not lock once per cell.
- At most one revision increment per transaction, and none if nothing changed
  (compare cell-by-cell; a redraw of identical content is a no-op).
- Dirty tracking stays per-cell (or per-cell with a per-row summary), so the
  renderer's `update_snapshot` still copies only changed cells and rebuilds only
  changed rows.
- The renderer must never lock the surface while shaping glyphs or building the
  batch; it takes the lock only to copy dirty cells into its retained snapshot,
  then releases it.
- Do not copy or translate the complete grid during every Bevy frame. Preserve
  the existing changed-row/cell behavior and the "revision unchanged, text
  assets unchanged, blink unchanged ⇒ skip" fast path exactly.
- Ratatui→neutral conversion happens exactly once, inside the adapter's `draw`,
  for the cells Ratatui submitted. It must not be re-run on the render side and
  must not require the adapter to keep a second full grid in Ratatui form (the
  Ratatui `Terminal` already retains its own buffers).

Expose renderer-derived logical/physical surface metrics in a neutral form so
the upper crate can implement Ratatui's `window_size` correctly without placing
Ratatui types in the lower crate.

### Renderer ownership

Move the optimized Bevy renderer into `bevy_terminal`, including:

- the compact render-world batch and renderer-owned texture;
- optional Bevy UI `ImageNode` presentation and texture-only/headless mode;
- multiple independent terminal surfaces and textures;
- terminal output image handles, logical and physical dimensions, and scale;
- render configuration, font sources and regular/bold/italic/bold-italic face
  overrides;
- glyph shaping and caching through Bevy text;
- glyph atlas management and cell clipping;
- exact/procedural box drawing, block, quadrant, and shaded-cell geometry;
- backgrounds, decorations, cursor and blink handling;
- high-DPI raster scaling and nearest-sampled presentation;
- per-terminal performance statistics (`snapshot_cells`, `snapshot_ns`, and the
  rest — keep them, they are the regression instrument);
- the retained Bevy UI renderer if it remains useful as a reference path.

Use names that describe the new package rather than `BevyGrid`. Prefer a clear
default such as `BevyTerminalPlugin`, with consistent associated component and
output names. Document how a Bevy application can render one or more terminal
scenes without installing or mentioning Ratatui.

## `bevy_terminal_ratatui` responsibilities

Reduce the root crate to a focused Ratatui adapter plus facade. It should own:

- the type implementing `ratatui::backend::Backend`, preferably named
  `RatatuiBackend`;
- conversion from Ratatui cells, colors, modifiers, positions, sizes, clear
  operations, cursor calls, appending, resizing, and scrolling into
  `bevy_terminal` updates;
- Ratatui-specific tests and documentation;
- re-exports of the commonly used `bevy_terminal` renderer API so an ordinary
  user can install only `bevy_terminal_ratatui`.

Keep the conversion local, for example with private `translate_cell`,
`translate_color`, and modifier helpers. Do not attempt an orphan-rule-invalid
`From<&ratatui::Cell> for bevy_terminal::TerminalCell` implementation. Keep the
translation branch-light and allocation-free where the neutral model allows it;
it runs for every changed cell of every frame.

The adapter must translate only the cells Ratatui submits through its draw
iterator. Preserve the current important behavior where explicit continuation
markers are synthesized for wide symbols because Ratatui can omit skip cells
from its diff iterator. A wide or double-width glyph must remain anchored to its
declared column span and must never overwrite the following terminal cell. When
Ratatui later overwrites the anchor with a narrow cell, the stale continuation
must be cleared in the same transaction (this is the "wide and continuation
cells cannot become stale" invariant; test it).

Map named Ratatui colors to the corresponding indexed ANSI values, retain
`Color::Indexed` and `Color::Rgb` exactly, and map reset colors to the lower
crate's contextual default color. Preserve underline colors and every modifier
currently supported.

The facade should make the common path concise:

```rust
use bevy_terminal_ratatui::prelude::*;

let backend = RatatuiBackend::new(80, 24);
let surface = backend.surface();
let terminal = ratatui::Terminal::new(backend)?;

app.add_plugins(BevyTerminalPlugin::new(surface));
```

Direct users of `bevy_terminal` must be able to construct and update the same
surface without Ratatui.

## Correctness requirements

The split and renaming must preserve all current functionality and visual
quality:

- exact fixed cell geometry and background coverage;
- ASCII, combining sequences, CJK/double-width glyphs, RTL/Indic text and emoji
  fallback behavior;
- explicit wide-cell continuations and glyph clipping;
- no overlap between selector arrows and following numbers;
- continuous single, heavy, double, rounded, dashed and mixed box drawing;
- no hairline gaps in block, quadrant or shaded elements;
- ANSI 16/256 and RGB colors, reset colors and underline colors;
- bold, italic and bold-italic using the correct configured font faces;
- dim, reverse, hidden, underline, crossed-out and blink behavior;
- cursor style, position, visibility and blinking;
- partial draws, clears, scrolls, appends and resizes;
- no flicker or transient clear frame while switching/redrawing examples;
- multiple independent textures and optional Bevy UI presentation;
- resizable interactive gallery behavior;
- deterministic headless output and high-DPI sharpness.

Do not make screenshots look correct by scaling a completed terminal image to a
different resolution or changing the logical grid.

## Examples and documentation

Keep all existing Ratatui examples under `bevy_terminal_ratatui`, including the
interactive all-in-one gallery, every keybinding, multiple terminals, resizable
window, image exports, and JetBrains Mono setup.

Add at least one `bevy_terminal` example that:

- has no Ratatui dependency or imports;
- writes a representative scene through the neutral surface API;
- exercises styles, box drawing, a wide glyph and cursor state;
- renders to a Bevy UI node or exposes its texture;
- demonstrates multiple independent surfaces if that does not make the example
  unnecessarily large.

Write separate crate-level READMEs:

- `bevy_terminal`: Bevy terminal scene renderer usable by any producer;
- `bevy_terminal_ratatui`: Ratatui backend and convenient Bevy integration.

Explain the dependency direction, direct-scene use, Ratatui use, texture-only
use, multiple surfaces, resizing and font-face configuration. Update every old
`bevy_grid` package name, import, title, documentation link, benchmark label and
example command. Search the complete repository for stale names and retain an
old occurrence only when it is intentionally historical and documented.

## Testing strategy

Split tests according to ownership.

`bevy_terminal` tests must directly construct neutral cells/scenes and cover:

- snapshot and dirty-row/cell updates;
- no-op revisions, including a transaction that rewrites identical cells;
- one revision per multi-cell transaction;
- resizing and two-dimensional overlap preservation;
- scrolling and cursor state;
- single, wide and continuation occupancy;
- combining symbols as one cell string;
- style/theme resolution;
- explicit font-face selection;
- exact geometry and glyph clipping;
- renderer output ownership for multiple plugin instances;
- the renderer fast path: unchanged revision performs no snapshot copy and
  rebuilds no rows (assert via the stats).

`bevy_terminal_ratatui` tests must cover:

- complete Ratatui cell/color/modifier conversion;
- wide-symbol continuation synthesis;
- Ratatui diff replacement of previous wide cells clears stale continuations;
- clear and cursor semantics;
- append/scroll/resize behavior;
- one surface revision per `draw`+`flush`, none for an identical redraw;
- operation through a real `ratatui::Terminal`;
- the complete gallery interaction and rendering tests.

Avoid duplicating renderer implementation tests in the adapter crate.

## Boundary proof

At the end, run and paste the output of all of the following; each must be
clean:

```sh
# No ratatui reachable from bevy_terminal in ANY dependency kind.
cargo tree -p bevy_terminal --edges all -i ratatui   # must report "nothing to print" / error: not found
cargo tree -p bevy_terminal --edges normal
cargo tree -p bevy_terminal_ratatui --edges normal --depth 1
cargo metadata --format-version 1 | jq '.packages[] | select(.name=="bevy_terminal") | .dependencies[] | {name, kind, optional, target}'

# No Ratatui identifiers in the lower crate's source, tests, examples, or docs.
grep -rniE 'ratatui|CellWidth|CellDiffOption|CompactString' crates/bevy_terminal   # must be empty
grep -rn 'bevy_grid\|BevyGrid\|BevyBackend' --include='*.rs' --include='*.md' --include='*.toml' . \
  | grep -v target/   # only intentionally historical, documented hits

# Independent packaging.
cargo package --allow-dirty --manifest-path crates/bevy_terminal/Cargo.toml --list
cargo package --allow-dirty --list
```

The `cargo metadata` output for `bevy_terminal` must show only `bevy` (normal)
and `bevy_image_export` (dev). Anything else is a boundary failure.

## Visual validation

Run the complete visual-QA workflow after the split. Use
`bevy_image_export` outside measured benchmark frames and inspect the generated
PNGs, not merely their existence.

At minimum inspect:

- the representative Unicode/style scene;
- the Ratatui `constraints` scene, especially the selector triangle and number;
- the `modifiers` scene for distinct regular/bold/italic/bold-italic faces;
- dense line and box drawing for gaps;
- CJK, combining marks, emoji and wide-cell alignment;
- the multiple-terminal scene;
- the new Ratatui-free `bevy_terminal` example;
- a fixed 2× output for sharp native rasterization.

Where a pre-split export of the same scene exists, compare pixel-for-pixel
(`cmp` or an image diff) and explain any difference; identical output is the
expectation for scenes whose content did not change.

Retain the complete JetBrains Mono 2.304 static family and its OFL license for
examples. JetBrains Mono contains the selector triangle used by the constraints
example; ensure it is selected through the explicit regular font handle rather
than a platform color-emoji fallback.

## Performance gate

Update `benchmarks/renderer-comparison` to build and report the new adapter as
`bevy_terminal_ratatui` while measuring the same complete path:

```text
Ratatui diff -> RatatuiBackend translate -> bevy_terminal surface transaction
-> incremental snapshot -> compact Bevy terminal renderer -> renderer-owned texture
```

Do not benchmark only the lower renderer while excluding Ratatui conversion.
Keep process isolation, synchronized GPU completion, captures, matched 10x20
cells, calibrated font sizes and native output validation.

Procedure:

1. Before touching the renderer, record the current per-terminal stats
   (`snapshot_cells`, `snapshot_ns`, batch/upload timings) from a headless run
   of the gallery `dense_styled` and `unicode` scenes; keep the numbers.
2. Run a bounded quick profile to catch protocol failures.
3. Run the standard profile with three repetitions. Do not start any stress,
   soak, full, or unbounded background job.
4. Re-record the per-terminal stats from step 1 on the split build.

Use the existing standard three-repeat result as the pre-split baseline:

`benchmarks/renderer-comparison/results/2026-08-16-standard-r3-jetbrains`

Acceptance:

- No repeatable regression greater than 5% in synchronized aggregate p50 for
  `static`, `sparse`, `dense_ascii`, `dense_styled`, or `unicode` at either
  80x24/800x480 or 120x40/1200x800. Anything above that must be investigated
  and fixed, not explained away.
- `snapshot_ns` and `snapshot_cells` for an unchanged frame must remain zero
  (fast path intact) and for changed frames must be within noise of the
  pre-split numbers.
- Preserve the current overall gate against `bevy_tui_texture`; do not hide
  conversion cost, move work into unmeasured frames, or loosen the benchmark
  protocol to pass it.
- If a regression is found, the usual suspects are: per-cell `String`
  allocation in translation, per-cell locking, a lost no-op path, a struct-of-
  bools modifier comparison, or extra cloning of the neutral snapshot. Check
  those first.

## Required verification

Run, fix, and report at least:

```text
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo doc --workspace --no-deps
cargo tree -p bevy_terminal --edges all -i ratatui
cargo tree -p bevy_terminal --edges normal
cargo tree -p bevy_terminal_ratatui --edges normal
cargo package --allow-dirty --manifest-path crates/bevy_terminal/Cargo.toml
cargo package --allow-dirty
```

Also run the benchmark workspace checks, the bounded quick comparison, the
three-repeat standard comparison, and all visual exporters relevant to affected
code.

Inspect the full final diff independently after implementation. Confirm that:

- the dependency boundary is real (Boundary proof section is clean);
- there is no Ratatui type, name, or comment in `bevy_terminal`;
- no full-grid per-frame translation was introduced and the fast path survives;
- one lock and at most one revision per adapter transaction;
- wide and continuation cells cannot become stale;
- multiple terminals retain independent configuration/output/state;
- packaged examples do not reference files outside their crate;
- every old import and command was migrated consistently;
- generated benchmark reports contain no adapter failures or resolution
  mismatches.

## Completion report

Finish with a concise report containing:

- the final workspace and dependency graph, with the boundary-proof output;
- the neutral scene and transactional surface API (types, sizes in bytes of
  `TerminalCell`/`TerminalStyle`, allocation behavior);
- which APIs moved into each crate;
- example migration and direct `bevy_terminal` usage;
- tests, Clippy, docs and package verification results;
- visual captures inspected, pixel-diff results, and any limitations found;
- benchmark tables before and after the split, including grid and output
  resolution, plus the per-terminal stats before/after;
- any remaining risks or intentionally unsupported behavior.

Do not declare completion while required checks fail, captures visibly regress,
the core crate still depends on Ratatui in any dependency kind, benchmark
resolution validation fails, or the split adds an unexplained material
performance regression.
