# Prompt: pixel-perfect glyph rendering (no clipped descenders, no seams)

You are working in the `bevy_terminal` repository (workspace root =
`bevy_terminal_ratatui`, lower crate = `crates/bevy_terminal`). Glyph
rendering is not yet perfect: with the current cell metrics, glyphs with
descenders (`g`, `j`, `p`, `q`, `y`, `,`, `;`, `_`, `ç`, `ą`, `ῃ`…), tall
glyphs (`Å`, `É`, `|`, `[`, `{`, `⎡`, `⎣`), combining marks above capitals
(`Ẫ`, `Ǻ`), and wide fallback glyphs can be clipped by the cell boundary. This
task makes rendering correct for every glyph and proves it with a visual
harness. Do not commit, push or publish unless explicitly asked.

## 1. Diagnose before changing anything

1. Reproduce: run `cargo run --example render_test -- --export --font
   iosevka-fixed` and `--font jetbrains-mono`, crop the ASCII rows (section 9)
   at 400 % and confirm which glyphs lose pixels (descenders at the bottom,
   accents/ascenders at the top, wide glyphs at the sides).
2. Read the pipeline: `crates/bevy_terminal/src/render/batch.rs`
   (`cached_shape` → `clip_glyph_to_cell` → `glyph_quad`), `RasterMetrics`,
   `physical_config`, `resolve_metrics`/`measure_advance` in `render/mod.rs`,
   and the `LineHeight::Px(cell_height)` passed to Bevy's `TextPipeline`.
   Establish precisely *why* pixels are lost. Expected causes to confirm or
   rule out, each with a one-line finding in the report:
   - the glyph's rasterized bitmap extends outside the cell rectangle and
     `clip_glyph_to_cell` discards it (vertical: descender below the cell
     because the baseline sits too low for the cell height / line height;
     horizontal: side bearings or fallback fonts with a wider advance);
   - the baseline placement Bevy chooses for `LineHeight::Px(cell_height)`
     when the font's natural ascent+descent exceeds `cell_height` (Bevy centers
     the line box; part of the glyph box falls outside);
   - the font size chosen by `FontSizing::FitCellWidth` producing a glyph box
     taller than the cell for the configured `cell_size` (e.g. 11×20 with an
     18.33 px Iosevka has descent + ascent ≈ 22 px);
   - `snap_geometry` rounding pushing a 1-px row out of the cell;
   - `TerminalRenderScale`/HiDPI paths differing from 1×.

## 2. Fix

Implement the fix in the renderer, not by tweaking example configs:

1. **Never lose glyph pixels vertically inside a terminal.** Cells must be
   able to show the full glyph box of the primary font. Two-part fix:
   - When `FontSizing::FitCellWidth` (or `CellSizing::FromFont`) picks the
     font size, also validate the vertical fit: measure the primary font's
     glyph box for a probe run containing ascender, cap, descender and mark
     glyphs (e.g. `Ẫgjqy|[]{}ÅÉ`) via `TextPipeline::update_text_layout_info`
     (glyph rects), and if `top < 0` or `bottom > cell_height` at the chosen
     size, reduce the font size until the box fits (keeping the width fit
     as a maximum). Expose the result in `TerminalTexture::font_size` and
     record in `TerminalStats` (or a `debug!`) when the vertical fit
     constrained the size, so a user knows why the font is smaller than the
     width alone would allow.
   - Position glyphs so the *font's* line box is centered/anchored in the
     cell in a way that keeps descenders inside: compute the vertical offset
     from the measured glyph-box extents (not from `LineHeight::Px` centering
     alone) and apply it uniformly per font face, so `g` and `Å` are both
     fully inside a cell whose height ≥ their combined extent.
   - Clipping to the cell (`clip_glyph_to_cell`) stays as the last resort for
     genuinely oversized fallback glyphs (emoji, CJK from a fallback family
     with a bigger box) — but a primary-font glyph must never reach it.
     Widen the clip for wide cells to the full declared span (already true) and
     make sure `snap_geometry` cannot round a glyph out of a cell that it
     fits into mathematically (snap the cell rectangle and the glyph
     consistently: floor for left/top, ceil for right/bottom, then clip).
2. **Horizontal**: glyphs whose bitmap is wider than the cell (fallback
   families with a larger advance, or side bearings) must be centered in the
   cell (or span) before clipping, so a `W` from a fallback font is symmetric,
   and single-column glyphs never bleed into the neighbor.
3. **Seams**: adjacent block/box glyphs (`█`, `─`, `│`, `▀▄`, `▌▐`,
   `░▒▓`) must tile with no visible seam at 1×, 1.5×, 2× and 3× raster
   scales. Preserve `FontSizing::FitCellWidth` behavior (advance = cell width)
   and unhinted rasterization; if a seam appears at fractional scales because
   the physical cell rounds while the glyph advance does not, size the
   physical font from the *rounded physical cell* (fit the advance to the
   physical cell width), not from the logical cell.
4. Keep the incremental renderer, stable texture handle, sRGB target and
   transparent-background behavior intact; keep performance (see §5).

## 3. Visual testing harness

Add an example `glyph_fidelity` in `bevy_terminal_ratatui` (headless export
via `--export`, windowed otherwise, `--font <index|dir>` like `render_test`)
that renders, in cells with a **contrasting per-cell background checkerboard**
(so a clipped or bleeding pixel is visible against the neighbor), each of the
following on its own row group with a guard column of `│` on both sides:

- full printable ASCII (`0x20..=0x7E`) in regular, bold, italic, bold-italic;
- Latin-1 Supplement and Latin Extended-A letters with descenders and accents
  (`ÀÁÂÃÄÅÆÇÈÉÊËÌÍÎÏÐÑÒÓÔÕÖØÙÚÛÜÝÞß àáâãäåæçèéêëìíîïðñòóôõöøùúûüýþÿ ĀāĂăĄąĆćĈĉ…`),
  Greek and Cyrillic samples, combining-mark stacks (`Ẫ`, `ǻ`, `e̊`, `ą̈`);
- box drawing (all of U+2500–257F), block elements (U+2580–259F), quadrants,
  shades, braille samples, geometric shapes and arrows;
- a wide/CJK/emoji row (`汉字日本語한글`, `🙂🚀🎉👍🏽🇺🇸`, `｜ｆｕｌｌ`);
- a "tile" panel: solid blocks and single/heavy/double lines drawn as
  8×4-cell rectangles so seams show as lines.

Export at raster scales 1.0, 1.5, 2.0 and 3.0 (`--scale`), and for each
vendored family (`--font all` loops through them). Write PNGs under
`target/glyph-fidelity/<family>/<scale>x/`.

Then add an **automated check** in the same example (`--check`) or in a
`tests/glyph_fidelity.rs` integration test (`#[ignore]`, GPU) that reads the
texture back (`bevy::render::gpu_readback`, as `tests/gpu_readback.rs` does)
and asserts, per glyph cell in the ASCII/Latin rows:
- the glyph's ink bounding box (pixels differing from the cell background) is
  strictly inside the cell for the primary font — no ink on the cell's first
  or last pixel row/column that would indicate clipping, *unless* the glyph
  is a block/box element intended to touch the edge (whitelist those ranges);
- for the tile panel: no interior pixel differs from the fill color by more
  than 1/255 (no seams), at every tested scale.
Report failures with the glyph, family, scale and the offending pixel row or
column so they can be fixed, then make them all pass.

Finally, inspect the exported PNGs by eye at 400 % for both Iosevka Fixed and
JetBrains Mono at 1× and 2× (crop and view them; do not just check file
existence), and describe what you saw.

## 4. Docs and tests

- Document the vertical fit rule and the "primary glyphs are never clipped;
  fallback glyphs are centered then clipped" policy in both READMEs and on
  `FontSizing`/`CellSizing`.
- Unit tests for the vertical fit calculation (given a measured glyph box,
  the resulting font size and offset), the consistent snapping/clipping math,
  and the horizontal centering of an over-wide glyph.

## 5. Verification and performance gate

```text
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace --all-targets
cargo test -p bevy_terminal --test gpu_readback -- --ignored
cargo test -p bevy_terminal --test glyph_fidelity -- --ignored   # if implemented as a test
cargo clippy --workspace --all-targets -- -D warnings
cargo doc --workspace --no-deps
```

Run every existing exporter and inspect; run the glyph-fidelity harness for
all fonts and scales; run the quick benchmark profile and a paired A/B
(previous binary vs new, ≥3 reps) — investigate any repeatable p50 regression
> 5 %. The extra measurement happens only when fonts/config change, never per
frame.

## 6. Report

Root cause(s) found, the fix, the harness results (a table: family × scale ×
row group → pass/fail, all pass), the by-eye inspection notes, before/after
crops of `g`/`Å`/`|` and a tile panel, test/clippy results, and the A/B table.
