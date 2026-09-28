# Ghostty parity: implementation report

This report covers the work on branch `ghostty-parity` (PR #6), from `main` at `9281c92` (the released 0.7.7 source). The goal was to stop `bevy_terminal` cropping glyphs, place glyphs the way Ghostty does as far as Bevy's text stack allows, and improve rendering quality and performance.

- Ghostty reference: `ghostty-org/ghostty@b40acce58dcf77df52231c3798ea58e924647c89` (2026-09-26), MIT. Logic and generated tables are ported with attribution. Ghostty is not a dependency.
- Machine: Apple M2 Max, 64 GiB, macOS, Metal. Rust 1.98.1 stable.
- Software Vulkan (Mesa lavapipe) runs in CI only. It is not available locally.
- The machine-readable record is [`verification.json`](verification.json).

## Summary

| Workstream | Status | Main commits |
| --- | --- | --- |
| 0. `release-fast` profile | done | `b16dc35` |
| 1. Clipping and placement probe | done | `8118046`, `3d46323` |
| 2. Vertical overflow with correct partial repaint | done | `5e8e8cd` |
| 3. Horizontal placement, emoji, Nerd Fonts, cell width | done | `8c2c2b9` |
| 4. Rendering quality (linear-corrected blending and others) | done. Synthetic bold/italic is a documented divergence; `minimum-contrast` (optional) was not done | `958d22d`, `32e65c7` |
| 5. Procedural grid graphics | done | `0392a2c` |
| 6. Performance | done for what the measurements confirmed; the other candidates were evaluated and are listed with reasons | `21a7f82`, `c656b77`, `f80e9f9` |
| 7. Run shaping | assessed, not implemented; design and blockers in [`RUN_SHAPING.md`](RUN_SHAPING.md) | `e994799` |
| Independent review | 5 findings, all confirmed and fixed | `feb0e42` |

## Probe: before and after

`crates/bevy_terminal/src/render/batch/probe.rs` drives the real sync system headlessly. It measures every emitted glyph quad against the glyph's full raster. The matrix covers the six bundled families at 18 and 24 px, raster scale 1 and 2, and line height 1.0 and 0.85: 48 configurations with host-independent fallback fonts.

- The files are `probe-baseline-*.tsv` (before) and `probe-after-*.tsv` (after).
- After the independent review, the probe measures the pieces the scene actually emits. Before, it compared against a recorded clip rectangle that could never fail.
- Re-running the probe with the corrected measurement reproduced `probe-after-summary.tsv` byte for byte.
- Reintroducing a row clip makes `glyph_ink_is_clipped_only_at_the_texture_edges` fail.

| Family | Graphemes clipped inside the texture, per configuration (before → after) | Pixels lost inside the texture, all configurations (before → after) | Graphemes shifted horizontally, per configuration (before → after) | Graphemes cut at the texture edge, per configuration (before → after) |
| --- | --- | --- | --- | --- |
| cascadia-mono | 19–143 → 0 | 15375 → 0 | 6–12 → 3 | 7–18 → 5–15 |
| dejavu-sans-mono | 12–157 → 0 | 12196 → 0 | 15–20 → 1 | 1–16 → 0–15 |
| hack | 22–156 → 0 | 14009 → 0 | 7–11 → 1 | 6–16 → 5–15 |
| iosevka-fixed | 6–96 → 0 | 8836 → 0 | 21–32 → 4 | 6–18 → 4–15 |
| jetbrains-mono | 14–49 → 0 | 7675 → 0 | 4–8 → 1 | 3–10 → 3–9 |
| source-code-pro | 15–122 → 0 | 9901 → 0 | 22–23 → 2 | 0–10 → 0–10 |

- **Vertical ink loss** now happens only at the texture's first and last rows: accents on row 0 and descenders on the last row. The texture's edges cut them, and the probe reports them separately.
- **Horizontal shifts** now happen only at column 0 and the last column. That is the documented edge rule: ink crossing the texture's outer edge is pushed back inside.
- Graphics are excluded from these counts. Sprites are drawn at cell size and cannot lose ink.
- **Cell heights.** Source Code Pro's cell height changed between the two runs. The line box now follows Ghostty's metrics (ascent + descent + line gap), so some `cell` columns differ.
- **Sample rows.** The samples gained a Nerd Fonts row, so row numbers after row 11 are one higher in the after files.

## Ghostty rules adopted

| Rule | Ghostty source (at `b40acce`) | Here |
| --- | --- | --- |
| Text glyphs are never clipped; the glyph box sits at cell position plus bearings | `src/renderer/shaders/shaders.metal` `cell_text_vertex` (l. 556); `src/renderer/generic.zig` `addGlyph` (l. 3437) | `scene.rs`: only the texture's edges clip; rows record their ink reach (`RowReach`), and partial repaints redraw reaching glyphs in full-scene order |
| Unconstrained text keeps its bearings | `generic.zig` `addGlyph` | `scene.rs`: `edge_shift` applies only at texture edges |
| Cells wider than the face centre the glyph by `(cell_width − face_width) / 2` | `src/font/Metrics.zig` (l. 233–265), `src/font/face/freetype.zig` `renderGlyph` | `face_dx` for `Fixed` cells wider than the face's advance |
| `isSymbol`, `constraintWidth` | `src/renderer/cell.zig` (l. 244, 250) | unchanged; they already matched |
| Constraint engine: `fit`, `cover`, `fit_cover1`, `stretch`, alignment `start`/`end`/`center`/`center1`, padding, icon height, `max_constraint_width`, relative groups | `src/font/Glyph.zig` `Constraint` (l. 83, `constrain` l. 180) | `constraint.rs`, a port that includes Ghostty's `Constraints` unit test |
| Colour (emoji-presentation) glyphs cover the box, centred, with 2.5% side padding, whatever the codepoint | `src/font/SharedGrid.zig` (l. 416–430) | `shaping.rs`, `Constraint::EMOJI` |
| Nerd Fonts per-codepoint attributes | `src/font/nerd_font_attributes.zig`, `nerd_font_codegen.py` | `nerd_font.rs`, generated by `gen_nerd_font_table.py` from a Ghostty checkout |
| Sprite font for box drawing, blocks, shades, Braille, geometric shapes, Powerline, branch drawing, legacy computing and its supplement, with line thickness from the underline thickness | `src/font/sprite/draw/*.zig`, `src/font/Metrics.zig` `box_thickness` | `sprite/`. Rectangle-built glyphs equal Ghostty's reference atlases (`src/font/sprite/testdata`, copied with Ghostty's license) pixel for pixel at all four test sizes. Anti-aliased paths agree within 10% of their ink |
| `alpha-blending = linear-corrected` | `src/config/Config.zig` (l. 390–412); `shaders.metal` `use_linear_correction` (l. 712) | the `gpu.rs` shader; each glyph piece carries its background's luminance, and glyphs are split where the background changes |
| Shades are uniform translucent fills, not dither patterns | `src/font/sprite/draw/block.zig` | `sprite/shapes.rs` |

## Intentional divergences from Ghostty

- **Whole-pixel glyph positions.** Bevy floors glyph positions (`bevy_text` 0.19.1 `pipeline.rs:391`), so Ghostty's fractional x offsets are not available.
- **Cell width: refit kept.** The font size is still refit so the advance fills the rounded cell for font-driven and width-fitted sizing.
  - Ghostty keeps the size and centres, which relies on those fractional offsets. With floored positions, keep-and-centre would either put up to half a pixel of error on every glyph or need a gutter per cell.
  - Only explicit `Fixed` cells wider than the face are centred (`face_dx`), as in Ghostty's FreeType path.
- **Glyph boxes for constraints come from rasterized ink**, or the bitmap box for colour glyphs. Bevy exposes rasters, not outlines. The constraint box is the primary face's advance × (ascent + descent + line gap), with cap-height icon heights.
  - Uniform scales re-rasterize at whole-pixel font sizes, so a scaled glyph can be up to one font pixel smaller than Ghostty's.
  - This bounds the sizes each glyph is rasterized at: one fitted size per glyph and cell geometry, reached in at most `SYMBOL_RESCALES` (2) re-measurements.
- **Stretched Nerd Font glyphs** are resampled with a premultiplied box filter instead of being re-rendered from outlines.
- **Texture edges.** Ghostty has window padding; the texture has none.
  - Text crossing the texture's outer left or right edge is pushed back inside.
  - Vertical overflow past the first or last row is clipped by the texture. The probe reports it separately.
  - A gutter would change every consumer's geometry and pointer mapping, so the push-in was kept.
- **Anti-aliased sprite paths** use analytic coverage rather than z2d's supersampling. They agree within 10% of their ink.
- **Blending.** Ghostty defaults to `native` on macOS. Here `linear-corrected` is used on every platform, which is Ghostty's default elsewhere. Colour glyphs blend unchanged in both.
- **Synthetic bold and italic.** Bevy 0.19 does not synthesize styles: a missing face falls back to a face with the requested weight and style. So there is no synthetic-bold box to widen.
- **Hinting stays disabled** by default. The cell-width refit needs continuous advances, and hinting makes ink extents discontinuous; the constraint fitter already re-measures after hinting.
- **Bitmap (sbix/CBDT) emoji** are whole-pixel rasters placed at whole pixels, at whole-pixel font sizes. That is the quantization Ghostty applies, so nothing changed.
- **Shaping** is per grapheme. Ghostty shapes runs, which gives it ligatures, `font-feature`, Arabic joining and shaper offsets across cells. See [`RUN_SHAPING.md`](RUN_SHAPING.md).
- **`minimum-contrast`** was optional and was not implemented.

## Performance

Measured before changing anything, with `benchmarks/audit-workloads` (new workloads in `21a7f82`) and `benchmarks/renderer-comparison`. Each variant ran in its own worktree with its benchmark workspace's own profile, interleaved and in rotating order. Details and raw tables are in [`benchmarks/`](benchmarks/README.md).

**Load caveat.** Unrelated work kept the machine heavily loaded throughout, with a load average of 16–60 on 12 cores. Wall-clock medians moved by up to 2× between identical runs. The tables therefore also give low percentiles (`p10`) and the fastest of many updates, which other load can only inflate. Allocation counts, bytes and cache misses are deterministic.

What the measurements confirmed, and what changed. Numbers are from `audit-release-run3.txt`, which compares `main`, `pre` and `post` in one batch:

| Measurement | `main` | after |
| --- | --- | --- |
| CPU image memory retained by 8 terminals showing a line of text (`text_8`) | 135,266,304 B | 1,048,576 B |
| Adding a glyph | mutates the 16 MiB atlas `Image`, which Bevy re-uploads whole | writes a sub-rectangle in the render world |
| `glyph_churn` allocated / peak live | 43.5 MB / 22.4 MB | 28.5 MB / 8.4 MB |
| `alternating_working_sets` shape misses | 8640 | 7423 (the cache retires a generation instead of dropping everything) |
| Full atlas | falls back to Bevy's source atlases (extra batches) | cleared and rebuilt for the current frame |
| Fastest 120×40 dense styled update | 1.25 ms | 1.32 ms (1.95 ms before the scene work; overflow and blending made scenes costlier) |
| Fastest 120×40 dense ASCII update | 0.41 ms | 0.47 ms |

**Final round** (`f80e9f9` against `main`, interleaved, load average 55–93):

- `renderer-comparison` (thin LTO): 5 runs × 180 frames per case. Raw table: [`benchmarks/final-renderer.txt`](benchmarks/final-renderer.txt).
- `audit-workloads` (release): 3 runs. Raw table: [`benchmarks/final-audit.txt`](benchmarks/final-audit.txt).

| Workload | Size | Total p50 (ms), main → final | Total p10 (ms), main → final |
| --- | --- | --- | --- |
| static | 80×24 | 1.611 → 1.777 | 1.139 → 1.136 |
| static | 120×40 | 1.715 → 1.738 | 1.184 → 1.221 |
| sparse | 80×24 | 1.807 → 2.071 | 1.294 → 1.274 |
| sparse | 120×40 | 1.825 → 2.054 | 1.362 → 1.346 |
| dense_ascii | 80×24 | 1.990 → 2.228 | 1.447 → 1.507 |
| dense_ascii | 120×40 | 2.637 → 2.698 | 2.000 → 2.046 |
| dense_styled | 80×24 | 2.209 → 2.406 | 1.712 → 1.747 |
| dense_styled | 120×40 | 3.450 → 3.631 | 2.614 → 2.608 |
| unicode | 80×24 | 1.998 → 2.121 | 1.424 → 1.414 |
| unicode | 120×40 | 2.215 → 2.269 | 1.695 → 1.676 |

- **Frame times.** The low percentiles are within ±4% of `main` in every case. Medians are 1–15% higher, but under this load medians swung by up to 2× between identical runs in earlier rounds, so they cannot separate the variants.
- **Wall time is about unchanged.** The renderer now does more per changed row: overflow bookkeeping, splitting glyphs per background for linear correction, and procedural sprites. The scene work in `c656b77` bought that back; before it, dense styled updates were 56% slower (`pre` in `renderer-batch4-p10.txt`).
- **Memory, allocations and cache misses are deterministic, and all improved:**
  - `text_8` retained CPU image bytes: 135,266,304 → 1,048,576;
  - `glyph_churn` allocated 43.5 → 28.7 MB, peak 22.4 → 8.5 MB;
  - `resize_font_scale_churn` peak 3.8 → 1.1 MB;
  - `alternating_working_sets` misses 8640 → 7423.
- `unicode_cache_saturation` peak rose from 49.2 to 51.2 MB. The retired shape-cache generation stays within its caps.

Candidates evaluated and not done:

- **A single-channel coverage atlas with a separate colour atlas** would save GPU memory: 16 MiB of RGBA8 per terminal would become 4 MiB of R8 plus a colour page. It would cost a second texture binding or extra batches per scene. The measured costs were CPU copies and whole-texture uploads, and both are gone, so this was left out.
- **Bevy's font atlases still hold the glyphs** the unified atlas copies from. They belong to Bevy's text pipeline, which rasterizes for us. The CPU copy of the unified atlas is gone.
- **Cheaper cache misses.** A miss still runs Bevy/Parley layout for one grapheme. Run shaping would change this path, so it is left to that design.

Idle terminals do no per-frame work (`idle_output_is_stable_and_application_ui_is_untouched`, `idle_terminals_do_not_rescan_fonts_or_publish_statistics`). Memory is bounded:

- the shape cache has two generations under entry and byte caps;
- each terminal has one atlas, cleared when full;
- queued uploads never exceed one atlas's worth (`f80e9f9`).

## Ratty captures

- **Setup.**
  - Isolated detached worktree `/tmp/ratty-verify` of `orhun/ratty` at `226453e`; the user's checkouts were not touched. Its `render_test` sample lines equal `demo/font-cycling-render-test`.
  - Before: crates.io `bevy_terminal_ratatui` 0.7.7.
  - After: `[patch.crates-io]` to this branch. `cargo tree -i bevy_terminal` confirmed the local path.
  - Captured with `headless_snapshot`, 104×32, raster scale 1.
- **Configurations.**
  - The user's configuration: JetBrainsMono Nerd Font Mono, 18 pt, opacity 1, cursor hidden.
  - Bundled DejaVu Sans Mono, 14 pt, line height 0.85.
  - Three views each: top, scrolled 22 lines, and End.
- **Crops.** Enlarged before/after crops are in [`ratty-captures/`](ratty-captures/). Full captures were inspected but not committed.

Observations:

- **Stacked marks.** The upper marks of Ẑ̃̄, cut off before, now draw in full and reach into the row above, as Ghostty draws them.
- **Emoji** now cover their two cells with 2.5% padding. `1️⃣` and the flags are as large as the others.
- **Shades** `░▒▓` are Ghostty's uniform fills instead of the font's dither patterns.
- **Braille, box drawing and Powerline** are drawn procedurally.
  - The bundled DejaVu has no Powerline glyphs, so they were tofu before and are now drawn.
  - Box corners and joins meet at the cell edges.
- **Hebrew, Devanagari and Thai** keep their bearings. Letters that `fit_horizontally` shifted by a pixel or two now sit where the font puts them.
- **Italics and other faces** keep their shapes and positions at this size. Only anti-aliased edges change, by up to 42 of 255, from linear-corrected blending. Light text on the dark background is slightly lighter-weight (mean brightness of the faces rows 65.4 → 63.2), which is the thickening of plain linear blending that the correction removes.
- **Font issue with DejaVu Sans Mono (unchanged).** Its combining marks draw in the following cell (`ạ́`, `x̶`). DejaVu Sans Mono's combining marks have ink inside a full advance, and its `mark` lookups do not attach them to these bases. Shaping the same strings with `harfrust` 0.6.2 (the HarfBuzz port Parley uses) gives the marks a zero advance and zero offset, so HarfBuzz, and so Ghostty, place them identically. This is a property of the font, not a renderer defect.
- **Presentation.**
  - `bevy_terminal`'s output uses nearest sampling and is sized at logical size × raster scale.
  - Ratty's flat 2D view presents it 1:1 with `textureLoad` (`src/present.rs`), so nothing blurs or crops.
  - Ratty's 3D plane samples the texture on transformable geometry, where sampling is inherent.

## Ghostty comparison

- **What was compared.** The rules, not screenshots:
  - Ghostty's `Constraints` unit test runs against the port.
  - Sprites are compared with Ghostty's reference atlases.
  - The Nerd Fonts table is generated from Ghostty's source.
  - Oracle expectations restate Ghostty's formulas independently: cover and fit, linear correction, and sprite rectangles.
- **Not done: a live comparison with Ghostty's GUI.** Ghostty.app is installed, but a capture would open windows on the user's active desktop and need screen-recording permission. Ghostty has no headless renderer. Pixel equality between different rasterizers (CoreText vs. swash) was not a goal.

## Independent review

`codex exec` (read-only) reviewed `git diff origin/main...HEAD`. It reported five findings, each checked against the code, confirmed and fixed with a regression in `feb0e42`:

1. **High: a render device reset lost the atlas.** The main world kept UVs into a blank texture. Scenes now record whether their uploads hold every entry. The render world flags a lost atlas, and the terminal rebuilds it.
2. **Medium: a sprite without atlas room was cached as empty.** It is no longer cached.
3. **Medium: sprites in wide cells drew at one column's width.** They now span the cell.
4. **Medium: combined quadrants at odd sizes blended overlapping solid rectangles twice.** Only single-rectangle blocks use solid quads now.
5. **Medium: the probe's interior-clip check could not fail.** It now measures emitted pieces.

A follow-up found while writing the report: unextracted scenes kept uploads from before an atlas clear. They are dropped now (`f80e9f9`), with a regression that fails without the fix.

## Verification

All commands ran on the final code (`f80e9f9`) with `cargo +stable` on Metal. The later commits change only documentation under `ghostty-parity/` and `.gitignore`.

| Check | Command | Exit | Time | Notes |
| --- | --- | --- | --- | --- |
| fmt | `cargo fmt --all -- --check` | 0 | 0 s |  |
| check-all | `cargo check --workspace --all-features --all-targets --locked` | 0 | 17 s |  |
| clippy | `cargo clippy --workspace --all-features --all-targets --locked -- -D warnings` | 0 | 2 s |  |
| check-default | `cargo check --workspace --locked` | 0 | 11 s |  |
| check-minimal-core | `cargo check -p bevy_terminal --no-default-features --locked` | 0 | 5 s |  |
| check-minimal-adapter | `cargo check -p bevy_terminal_ratatui --no-default-features --locked` | 0 | 4 s |  |
| tests | `cargo test --workspace --all-features --lib --tests --locked` | 0 | 39 s | bevy_terminal lib 101 passed (1 ignored: the probe report), 17 + 2 integration |
| gpu-readback | `cargo test --workspace --all-features --test gpu_readback --test export_readiness --locked -- --ignored --test-threads=1` | 0 | 132 s | 6 `gpu_readback` + 1 `export_readiness` |
| oracle-tests | `cargo test --all-features --example glyph_fidelity --locked` | 0 | 33 s | 9 oracle unit tests |
| fidelity-fit | `cargo run --all-features --locked --example glyph_fidelity -- --check --font all --scale all --output /tmp/verify-final/gf/fit` | 0 | 346 s | 193 font × scale × group checks passed |
| fidelity-natural | `cargo run --all-features --locked --example glyph_fidelity -- --check --font all --scale all --from-font 23 --output /tmp/verify-final/gf/natural` | 0 | 242 s | 193 passed |
| fidelity-compact | `cargo run --all-features --locked --example glyph_fidelity -- --check --font all --scale all --from-font 23 --line-height 0.85 --output /tmp/verify-final/gf/compact` | 0 | 212 s | 193 passed |
| fidelity-fixed | `cargo run --all-features --locked --example glyph_fidelity -- --check --font all --scale all --fixed-cell 9x18 --font-size 18 --output /tmp/verify-final/gf/fixed` | 0 | 108 s | 193 passed |
| fidelity-roomy | `cargo run --all-features --locked --example glyph_fidelity -- --check --font all --scale all --fixed-cell 14x24 --font-size 16 --output /tmp/verify-final/gf/roomy` | 0 | 198 s | 193 passed |
| coverage | `cargo run --all-features --locked --example glyph_coverage -- --output /tmp/verify-final/gc` | 0 | 76 s | all required coverage checks passed (same-pixels, fractional, compact, font-switch, content-replace, idle, …) |
| doctests | `cargo test --workspace --all-features --doc --locked` | 0 | 9 s | 5 + 4 |
| docs | `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps --locked` | 0 | 5 s |  |
| probe | `cargo test -p bevy_terminal --lib --locked probe -- --ignored && diff target/glyph-placement-probe/summary.tsv ghostty-parity/probe-after-summary.tsv` | 0 | 42 s | regenerated summary identical to `probe-after-summary.tsv` |
| consumer-fixture | `CARGO_TARGET_DIR=/tmp/fixture-target cargo test --manifest-path integration/bevy-ratatui-context/Cargo.toml --locked` | 0 | 8 s | resource-owned raw terminal integration |
| consumer-windowed | `CARGO_TARGET_DIR=/tmp/fixture-target cargo check --manifest-path integration/bevy-ratatui-context/Cargo.toml --features consumer-windowed --locked` | 0 | 2 s |  |
| consumer-native | `CARGO_TARGET_DIR=/tmp/fixture-target cargo check --manifest-path ../bevy_ratatui/Cargo.toml --lib --no-default-features --features std,crossterm --locked` | 0 | 61 s | pinned `bevy_ratatui` at `f1118a9` |

- **Disk.** The run was interrupted once when the disk filled; the machine's volume reached 100%. The interrupted steps were rerun after freeing this work's scratch builds; the table records the reruns.
- **Coverage flake.** Earlier in the work, one `glyph_coverage` run under extreme load failed in `content-replace` with stale content across all terminals. More than 20 runs since have passed, including this one. The harness waits a fixed 30 ticks for readback; waiting on a capture sequence number would make it load-independent.
- **CI.** The software Vulkan job and the `bevy_ratatui` consumer job passed on `f80e9f9`: [run 36367934180](https://github.com/gold-silver-copper/bevy_terminal/actions/runs/36367934180).

## Font provenance

- Required checks use only bundled fonts, listed with their licenses in `assets/fonts/README.md`:
  - `assets/fonts/` holds DejaVu Sans Mono, JetBrains Mono, Hack, Iosevka Fixed, Source Code Pro and Cascadia Mono, plus the fidelity fallback subsets.
  - `crates/bevy_terminal/assets/fonts/` holds the crate's test fonts.
- `assets/fonts/fidelity/subset-sources.json` and `subset-hashes.json` record the subset sources and hashes.
- New fixture: `crates/bevy_terminal/assets/fonts/fidelity/FidelityNerdSymbols.ttf`, subset from Ghostty's vendored JetBrains Mono Nerd Font (OFL 1.1).
- The Ratty captures used the host's JetBrainsMono Nerd Font Mono from `~/Library/Fonts`. It is an optional diagnostic, not a required check.

## Requirement audit

| Requirement | Status | Evidence |
| --- | --- | --- |
| Single normal PR, commits per step, pushed, CI checked, description kept current | met | PR #6; CI green on every final push |
| Prompt, handoff, scratch and large captures not committed | met | `git status`; only small crops are committed |
| No merge, publish, version bump, tag, or comments elsewhere; Ratty branches and PR #155 untouched | met | Ratty work in a detached `/tmp` worktree only |
| Findings re-established with fresh evidence before changing code | met | `probe-baseline-*.tsv` at `9281c92` |
| 0: `release-fast` profile, README, timings | met | `b16dc35`; PR description: 353 s from scratch, 5.7 s per edit (`release`: 348 s, 8.9 s) |
| 1: checked-in probe with baseline; system fonts optional | met | `probe.rs`, `BEVY_TERMINAL_PROBE_FONTS` |
| 2: no row clip for text and colour glyphs; partial repaints pixel-identical; any overflow depth; cursor, background, blink, scroll, resize, style-only and stale-ink tests; paint order | met | `replay.rs` tests (`overflowing_ink_reaches_neighbours_and_partial_repaints_match_full_scenes` and others); GPU readback |
| 3: text keeps bearings; edge rule chosen and tested; symbols fit the face box; emoji cover every colour glyph at a bounded number of raster sizes; Nerd Fonts table with generator; cell width evaluated | met | `8c2c2b9`; fidelity oracle; `constraint.rs` tests |
| 4: linear-corrected blending with per-piece backgrounds; oracle updated from the formula; hinting, bitmap emoji and synthetic styles evaluated; presentation checked | met (synthetic styles: divergence; `minimum-contrast`: optional, not done) | `958d22d`, `32e65c7`; Ratty `present.rs` |
| 5: procedural graphics for all listed ranges; `trim_overshoot`, box offset and block probe removed; seamless tiling | met | `0392a2c`; reference-atlas tests; fixed and fractional fidelity configs |
| 6: measured first; confirmed items addressed; idle does nothing; memory bounded; no unintended visual change | met; single-channel atlas and cheaper misses evaluated and left out | `benchmarks/`; oracles unchanged by `c656b77` |
| 7: run shaping assessed; design and blockers written | met (not implemented) | `RUN_SHAPING.md` |
| Verification: fmt, check, clippy, tests, doctests, docs, feature configurations | met | table above |
| GPU readback, fidelity and coverage on Metal | met | table above |
| Software Vulkan | met in CI only | CI job "Workspace and software Vulkan" |
| Probe rerun, same matrix | met | `probe-after-*.tsv` |
| Ratty captures, native and enlarged | met | `ratty-captures/` |
| Compare with Ghostty at the same font and size | partial: rule-level only, no GUI capture | see above |
| Benchmarks after the changes against the baseline | met | `benchmarks/`, table above |
| Independent review; confirmed findings fixed; checks rerun | met | `feb0e42`, `f80e9f9` |
| Report and machine-readable manifest | met | this file, `verification.json` |

## Coverage limits

A passing finite suite does not prove support for every font, script or geometry.

- **Checked:**
  - six bundled monospace families plus fallback subsets;
  - font sizes 18, 23 and 24 px and fixed 9×18 and 14×24 cells;
  - raster scales 1, 1.5, 2 and 3;
  - line heights 1.0 and 0.85;
  - the sample strings in the probe and fidelity suites: Latin with accents and stacked marks, Greek, Cyrillic, Arabic, Hebrew, Devanagari, Thai, CJK, emoji and ZWJ sequences, symbols, Nerd Fonts and grid graphics.
- **Not covered:**
  - proportional fonts;
  - variable-font axes beyond weight and style;
  - scripts needing run shaping (Arabic joining across cells, ligatures);
  - vertical text;
  - Ghostty's GUI output;
  - software Vulkan on this machine.
