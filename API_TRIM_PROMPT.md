# Prompt: `bevy_terminal` / `bevy_terminal_ratatui` 0.4.0 API trim

You are working in the `bevy_terminal` repository (workspace root =
`bevy_terminal_ratatui`, lower crate = `crates/bevy_terminal`). Both crates are
published at 0.3.0. Apply the following cuts and simplifications as one breaking
change and bump both crates to **0.4.0**. Every capability must survive — this
pass removes redundant *ways* of doing things, not things that can be done.
Migrate examples, docs, tests and the benchmark adapter; verify; report. No
deprecated aliases. Do not commit, push or publish unless explicitly asked.

Keep: the neutral scene model, transactional surface, incremental snapshots,
font-glyph rendering, measured font sizing, unhinted rasterization, multiple
terminals as plain components, user-owned `ImageNode` presentation, stable
texture handles, `TerminalReady`, the dependency graph (`bevy_terminal → bevy`
only), and performance (gate in §5).

## 1. Cuts (remove without replacement)

`bevy_terminal`
- `TerminalSurface::begin_update` (public), `SurfaceUpdate::commit`,
  `SurfaceUpdate::has_changes`: `TerminalSurface::update(|u| ..) -> bool` is
  the only public write path. `SurfaceUpdate` stays public as the closure
  parameter type but cannot be obtained or stored otherwise (its guard never
  escapes; drop the "do not hold across frames" docs). The Ratatui adapter's
  `draw`/`clear_region`/`append_lines` are rewritten as closures.
- `SurfaceUpdate::set_cells`, `SurfaceUpdate::cell`, `SurfaceUpdate::contains`
  (positions outside the grid are ignored; callers need no pre-check). Keep
  `size`, `cursor_position`, `cursor_visible`, `set_cell`,
  `set_cursor_position`, `set_cursor_visible`, `clear`, `clear_row`,
  `clear_range`, `resize`, `scroll_up`, `scroll_down`.
- `GridSize::ZERO`, `GridSize::area` (make `pub(crate)` if used internally),
  `CellPosition::ORIGIN`.
- `StyleFlags::difference`, `StyleFlags::set`. Keep `from_bits`/`bits`
  (documented adapter contract), `contains`, `is_empty`, `union`, `insert`,
  `remove`, `BitOr`.
- `CellOccupancy::spanning` → `pub(crate)` (only `TerminalCell::wide` uses it).
- `TerminalSnapshot::empty`.
- `SurfaceMetrics` and `TerminalSurface::metrics`: replace with
  `TerminalSurface::pixel_size() -> Option<UVec2>` (logical pixel size once a
  renderer has configured the cell size) next to the existing `size()`.
- `TerminalTexture::cell_size` (derivable from `size` and the grid).
- `TerminalReady::{image, size}` — the event becomes `TerminalReady { entity }`;
  observers read `TerminalTexture` from the entity.
- `TerminalPlugin::collect_timings` — back to a unit struct `TerminalPlugin`;
  timings are always collected (two `Instant::now()` per changed frame).
- `TerminalSystems::Setup` — a single public set `TerminalSystems::Sync`
  remains (initialization runs before it, unnamed).
- `TerminalStats`: drop `sync_frames`, `unchanged_frames`, `cached_shapes`,
  `gpu_bytes_written`. Keep `changed_rows`, `snapshot_cells`, `solid_quads`,
  `glyph_quads`, `draw_batches`, `shape_misses`, `snapshot_ns`, `scene_ns`,
  `Display`, `#[non_exhaustive]`. Any test that used the removed counters
  asserts through the remaining ones (e.g. an unchanged frame has
  `changed_rows == 0` and `snapshot_cells == 0`).

`bevy_terminal_ratatui`
- `RatatuiBackend::terminal()` (keep `with_terminal`).
- `RatatuiBackend::resize` → `pub(crate)`; `RatatuiTerminalExt::resize_grid`
  is the only resize entry point.
- `RatatuiTerminalExt::surface` (use `terminal.backend().surface()`).
- The explicit re-export list in `src/lib.rs` → `pub use bevy_terminal::*;`
  plus the `TerminalRenderer` alias and the prelude.

## 2. Structural simplifications

1. **Public modules instead of a flat root.** In `bevy_terminal`, make
   `scene`, `surface` and `render` (rename `renderer` → `render`) public
   modules and stop re-exporting their items at the crate root. Keep
   `bevy_terminal::prelude` as the flat, glob-importable view of everything a
   user needs (`TerminalPlugin`, `Terminal`, `TerminalRenderConfig`,
   `TerminalTexture`, `TerminalStats`, `TerminalReady`, `TerminalSystems`,
   `TerminalSurface`, `SurfaceUpdate`, `TerminalCell`, `TerminalStyle`,
   `TerminalColor`, `StyleFlags`, `CellOccupancy`, `CellPosition`, `GridSize`,
   `TerminalSnapshot`, `FontFaces`, `FontSizing`, `CursorConfig`, `CursorStyle`,
   `BlinkConfig`, `TerminalRenderScale`, `TerminalTheme`, `FontHinting`,
   `FontSource`). `color.rs` merges into `render` (the theme is a render
   concern) or `scene` — pick one and document. Update all paths in docs,
   examples, tests, the adapter and the benchmark adapter.
2. **Group advanced raster settings.** `TerminalRenderConfig { cell_size,
   font: FontFaces, font_size: FontSizing, theme, cursor: CursorConfig,
   blink: BlinkConfig, raster: RasterConfig { scale: TerminalRenderScale,
   hinting: FontHinting } }`. `RasterConfig` implements `Default`
   (`Automatic`, `Disabled`).
3. **Adapter `draw` through `update`.** Rewrite `RatatuiBackend`'s `Backend`
   impl to use `surface.update(|u| ..)` everywhere; it must still take the lock
   once per Ratatui call and publish at most one revision (existing tests
   cover this).
4. **`TerminalReady` handling in examples**: exporters keep using the event
   (now reading `TerminalTexture` in the observer); nothing polls.

## 3. Docs

- Update both READMEs (concepts tables, code samples, migration section for
  0.3 → 0.4 listing every removed/moved item and its replacement),
  `RATATUI_EXAMPLES.md`, `assets/fonts/README.md` if referenced, and all
  crate/module docs. Module docs for `scene`, `surface`, `render` should each
  open with a two-sentence description.
- `examples/common` and `crates/bevy_terminal/examples/common` shrink
  accordingly (no more `set_cells`, no `begin_update`).

## 4. Tests

Update existing tests; add: `update` is the only write path and still yields
one revision per call and none for identical content; adapter `draw` through
`update` preserves continuation synthesis and stale-continuation clearing;
`pixel_size()` is `None` before a renderer configures the surface and
`Some(..)` after; `TerminalReady { entity }` fires once; `RasterConfig`
defaults; the `bevy_terminal::prelude` glob compiles alongside
`bevy::prelude::*` and (in the adapter) `ratatui::Terminal` in doctests; the
benchmark adapter's counter line uses only remaining stats.

## 5. Verification and performance gate

```text
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo doc --workspace --no-deps
cargo tree -p bevy_terminal --edges all -i ratatui      # must not match
cargo package --workspace --allow-dirty
```

Migrate `benchmarks/renderer-comparison/adapters/bevy-terminal-ratatui`, run the
quick profile and the standard three-repeat profile, and a paired A/B
(0.3.0 binary vs 0.4.0, interleaved, ≥3 reps); investigate any repeatable p50
regression > 5 % and confirm renderer counters (rows/cells/quads) are
unchanged. Re-run `render_test --export` (Iosevka Fixed, JetBrains Mono),
`image_export`, `high_dpi_export`, `multiple_terminals_export`,
`ratatui_examples_export`, `scene_export`; inspect the PNGs.

## 6. Report

Final public API of both crates grouped by module, the 0.3 → 0.4 removal/
move mapping, line-count delta, test/clippy/doc/package results, A/B and
harness tables, captures inspected, intentional behavior changes.
