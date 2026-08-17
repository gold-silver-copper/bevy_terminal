# Prompt: `bevy_terminal` / `bevy_terminal_ratatui` 0.2.0 API cleanup

You are working in the `bevy_terminal` repository (workspace root =
`bevy_terminal_ratatui`, lower crate = `crates/bevy_terminal`). Both crates are
published at 0.1.1. Implement the following API cleanup as one coherent
breaking change and bump both crates to **0.2.0**. This is an implementation
task: make every change below, migrate all examples, docs, tests and the
benchmark adapter, verify, and report. Do not leave deprecated aliases for the
old names; backwards compatibility is not a goal. Do not commit, push or
publish unless explicitly asked afterwards.

Preserve everything that already works: the neutral scene model, the
transactional surface (one lock, ≤1 revision per transaction, incremental
dirty-cell snapshots), font-glyph-only rendering, measured font sizing,
unhinted rasterization, multiple independent terminals, headless/texture
output, high-DPI raster scale, wide-cell anchoring, and the dependency graph
(`bevy_terminal → bevy` only; `bevy_terminal_ratatui → bevy_terminal + ratatui`).
Performance must not regress (see the gate at the end).

## 1. Remove cruft

1. **Delete the retained renderer.** Remove `RetainedBevyTerminalPlugin`,
   `TerminalRenderStats`, the `RenderedEntities`/primitive-pool code, the
   `sync_terminal`/`animate_blinks` systems and every helper that exists only
   for it (`TextPrimitive`, `SolidPrimitive`, `PrimitivePool`, `TerminalBlink*`,
   `TerminalCursor`, `text_node`, `solid_node`, `spawn_*`, `update_*`,
   `sync_*_pool`, `push_decoration`, `text_runs`, `background_runs`,
   `foreground_z_index`, `cursor_node`, `cursor_visibility`, and their tests).
   Keep the shared pieces the compact renderer still needs (`ResolvedStyle`,
   `text_font`, `blink_hidden`, `cursor_should_be_visible`, `PixelGeometry`,
   `cell_span`, font measurement) and move them next to the compact renderer.
   Drop `#[derive(Resource)]` from `TerminalSurface` and `TerminalRenderConfig`;
   they only existed for the retained path.
2. **Collapse font size into one field.** Replace `font_size: f32` +
   `font_sizing: FontSizing` with `font_size: FontSize` where
   `enum FontSize { FitCellWidth, Px(f32) }` (`FitCellWidth` default). Remove
   `FontSizing`. Keep the measurement logic (probe run of `0` glyphs at a probe
   size, retry until the font asset is registered, re-measure on font/config
   change) and expose the resulting size (see §3.2).
3. **`RatatuiBackend`:** remove `snapshot()` (use `surface().snapshot()`).
   Add a helper that resizes a `ratatui::Terminal<RatatuiBackend>` in one call
   (backend resize + `terminal.autoresize()`), e.g. an extension trait
   `RatatuiTerminalExt::resize_grid(&mut self, columns, rows) -> io::Result<()>`
   or a free function; update the docs that currently tell users to call both.
4. **Hide renderer back-channels.** Make `TerminalSurface::set_cell_size`
   `pub(crate)`. Keep `metrics()` public (the adapter needs it for
   `window_size`). Keep `update_snapshot`/`SnapshotDelta` public only if
   clearly documented as the incremental API for renderers; otherwise make them
   `pub(crate)`.
5. **Trim stats.** Reduce the per-terminal stats component to actionable
   counters: `sync_frames`, `unchanged_frames`, `changed_rows`, `snapshot_cells`,
   `solid_quads`, `glyph_quads`, `draw_batches`, `cached_shapes`, `shape_misses`,
   `snapshot_ns`, `scene_ns`, `gpu_bytes_written`. Remove
   `pipeline_switches`, `atlas_bindings`, `render_passes`, `draw_calls`,
   `gpu_write_calls`, `gpu_buffer_reallocations`, `extracted_bytes` (or fold
   them into a doc note). Mark the struct `#[non_exhaustive]`. Update the
   benchmark adapter's counter log line accordingly.
6. **Slim the Ratatui prelude.** `bevy_terminal_ratatui::prelude` should export
   only what a Ratatui user needs: `RatatuiBackend`, the resize helper,
   `TerminalPlugin`, `Terminal`, `TerminalTexture`, `TerminalStats`,
   `TerminalNode`, `TerminalRenderConfig`, `FontFaces`, `FontSize`,
   `TerminalRenderScale`, `Presentation`, `CursorStyle`, `TerminalTheme`,
   `TerminalSurface`. The full `bevy_terminal` API stays reachable via the
   `bevy_terminal` re-export and top-level re-exports.

## 2. Rename and re-place abstractions

1. **Drop "Batch" from public names** (the retained renderer is gone, so there
   is no clash): `TerminalBatch → Terminal`, `TerminalBatchOutput →
   TerminalTexture`, `TerminalBatchRoot → TerminalNode`, `TerminalBatchStats →
   TerminalStats`, `TerminalBatchPresentation → Presentation`,
   `BevyTerminalPlugin → TerminalPlugin` (see §3.1 for its new role). Remove the
   bare `TerminalRoot` marker; `TerminalNode { terminal: Entity }` is the only
   UI marker.
2. **Presentation owns placement.** Move `origin` out of
   `TerminalRenderConfig` into `Presentation::Ui { origin: Vec2 }`;
   `Presentation::Headless` has no fields. `TerminalRenderConfig` then contains
   only rendering inputs.
3. **Group cursor and blink settings.**
   `cursor: CursorConfig { style: CursorStyle, color: Color, blink_hz: Option<f32> }`
   (move `TerminalTheme::cursor` here) and
   `blink: BlinkConfig { slow_hz: Option<f32>, rapid_hz: Option<f32> }`
   (`None` = disabled; replace the implicit "0 means off").
4. **Font faces as one value.** `font: FontFaces { regular: FontSource,
   bold: Option<FontSource>, italic: Option<FontSource>, bold_italic:
   Option<FontSource> }` with `impl From<FontSource> for FontFaces` and
   `FontFaces::regular(source)`; the "bold_italic falls back to bold, then
   italic, then regular" rule lives in one method on `FontFaces`.
5. **Re-export `bevy::text::FontHinting`** from `bevy_terminal` (and through
   `bevy_terminal_ratatui`) so users never spell a `bevy::text` path.
6. **Make `CellSymbol` opaque.** Keep the three-way storage (ASCII byte /
   inline ≤22 bytes / heap) but hide the variants behind a private
   representation; public API is `new`, `as_str`, `Deref<Target = str>`,
   `From<&str>`, `From<char>`, `Default`, `Display`, `Debug`, `Eq`, `Hash`.
   Assert the 24-byte size in a test.
7. **Make `TerminalCell::occupancy` read-only.** Keep the field private with
   `occupancy()`/`columns()`/`is_continuation()` accessors and the constructors
   `new`, `wide`, `continuation_of`, `with_style`. `symbol` and `style` may stay
   public fields.
8. **Consistent coordinates.** All `SurfaceUpdate` methods that take a cell
   position accept `impl Into<CellPosition>` (`(u16, u16)` and `CellPosition`
   both work); `GridSize` and `CellPosition` gain `From<(u16, u16)>` if missing.
   Replace `SurfaceMetrics.cell_size: Option<(f32, f32)>` with `Option<Vec2>`
   and `pixel_size: (u16, u16)` with `UVec2`.
9. **Simplify clears.** Replace `clear_from`, `clear_through`, `clear_row_from`
   with `clear_range(start: impl Into<CellPosition>, end: impl Into<CellPosition>)`
   (row-major, inclusive, clamped) and keep `clear()`/`clear_row(row)`. The
   Ratatui adapter maps every `ClearType` onto these.
10. **Add a closure form of updates.** `TerminalSurface::update(|u: &mut
    SurfaceUpdate| { .. }) -> bool` as the recommended API; keep
    `begin_update()` for producers that need the guard (the adapter). Document
    that a `SurfaceUpdate` holds the surface lock and must not be stored across
    frames.

## 3. Component-based terminals (DX)

1. **`TerminalPlugin` is added once; terminals are entities the user spawns.**
   Replace the per-instance `BevyTerminalPlugin::new(surface)` with:
   ```rust
   app.add_plugins(TerminalPlugin);
   let entity = commands.spawn(Terminal::new(surface).with_config(config).with_presentation(Presentation::Headless)).id();
   ```
   `Terminal` is a component (surface + config + presentation). Use required
   components / a component hook or an `Added<Terminal>` system to attach
   `TerminalTexture`, `TerminalStats` and the internal render state, allocate
   the output image, and spawn the `TerminalNode` `ImageNode` for
   `Presentation::Ui`. Despawning the terminal entity must clean up its node
   and images. Terminals can therefore be added or removed at runtime, and the
   user has the `Entity` directly (no `renders_surface`/`shares_state_with`
   matching needed; keep `shares_state_with` on the surface anyway, it is
   cheap and useful).
2. **`TerminalTexture`** keeps `image`, `size` (physical), `logical_size`,
   `raster_scale`, and adds `font_size: f32` (the measured/effective logical
   font size) and `cell_size` (physical). Keep the image `Handle<Image>` stable
   across resizes if Bevy's render-asset caching allows reallocation in place;
   if it must change identity, fire an observer event
   (`Trigger<TerminalResized { entity }>`) and document it. Prefer the stable
   handle.
3. **Config changes.** Editing `Terminal::config_mut()` (or replacing the
   `Terminal` component) is detected via change detection and rebuilds only
   that terminal, as today. Document this in one place.
4. **Systems.** Keep `TerminalSystems::Sync` (and `Blink` if still separate)
   as the public ordering hook.

## 4. Documentation and examples

- Update both READMEs, `RATATUI_EXAMPLES.md`, crate docs and every example
  (`demo`, `image_export`, `high_dpi_export`, `multiple_terminals[_export]`,
  `ratatui_examples[_export]`, `render_test`, and `crates/bevy_terminal`
  `scene`/`scene_export`) to the new API. Examples that add terminals should
  demonstrate spawning at runtime where natural (`multiple_terminals` can add
  the second terminal a few frames in).
- Convert `render_test`'s env-var knobs to CLI args (`--export`, `--font
  <index|dir>`) while keeping the env vars working, or replace them; document
  in the README.
- Every public item keeps `missing_docs = "warn"` clean; both crates keep
  `unsafe_code = "forbid"`.
- Add a short "Migrating from 0.1" section to the root README mapping old
  names to new ones.

## 5. Tests

Update existing tests to the new API and add coverage for: `FontSize::Px` vs
`FitCellWidth` selection, `FontFaces` fallback order, `Presentation::Ui`
origin placement, spawning two `Terminal` entities at runtime with independent
textures/stats and despawning one (node and images removed), `clear_range`
clamping, `Into<CellPosition>` call sites, `CellSymbol` opacity (size,
round-trip), `TerminalStats` non-exhaustive construction via `Default`, and the
Ratatui resize helper through a real `ratatui::Terminal`.

## 6. Verification and performance gate

Run and report:

```text
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo doc --workspace --no-deps
cargo tree -p bevy_terminal --edges all -i ratatui      # must not match
cargo package --workspace --allow-dirty
```

Update `benchmarks/renderer-comparison/adapters/bevy-terminal-ratatui` to the
new API (it should spawn one `Terminal` entity with `Presentation::Headless`
and `FontSize::Px(config.font_size)`), then run the bounded quick profile and
the standard three-repeat profile. Compare against the most recent post-split
standard result in `benchmarks/renderer-comparison/results/`; investigate any
repeatable p50 regression greater than 5 % on `static`, `sparse`, `dense_ascii`,
`dense_styled` or `unicode` at 80x24 or 120x40, and confirm the renderer
counters (`snapshot_cells`, `glyph_quads`, `solid_quads`) are unchanged. Also
re-run `render_test --export` for at least Iosevka Fixed and JetBrains Mono,
`image_export`, `high_dpi_export`, `multiple_terminals_export` and
`ratatui_examples_export`, and inspect the PNGs.

## 7. Report

Finish with: the final public API of both crates (grouped by module), the
old→new name mapping, what was deleted, test/clippy/doc/package results,
benchmark before/after tables, captures inspected, and any behavior that
intentionally changed.
