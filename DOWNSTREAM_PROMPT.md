# Prompt: `bevy_terminal` / `bevy_terminal_ratatui` 0.5.0 — downstream readiness

You are working in the `bevy_terminal` repository (workspace root =
`bevy_terminal_ratatui`, lower crate = `crates/bevy_terminal`). Both crates are
published at 0.4.0. Implement everything below as one release and bump both to
**0.5.0**. The goal is to make these crates a drop-in native backend for
`bevy_ratatui` (its `windowed` feature) and `ratty` (a GPU terminal emulator).
Preserve every existing capability and the dependency graph
(`bevy_terminal → bevy` only; `bevy_terminal_ratatui → bevy_terminal +
ratatui`); performance must not regress (gate in §7). Migrate examples, docs,
tests and the benchmark adapter; verify; report. Do not commit, push or publish
unless explicitly asked.

## 1. Font-driven cell metrics

Today the config is "cell size in → font size out" (`FontSizing::FitCellWidth`)
or fully explicit. Terminal emulators work the other way: "font size in → cell
size out", and they recompute columns/rows from window size ÷ cell size on
every resize/zoom.

- Change `TerminalRenderConfig::cell_size: Vec2` to
  `cell_size: CellSizing` with
  `enum CellSizing { Logical(Vec2), FromFont }` (keep a `Vec2` `From` impl so
  `cell_size: Vec2::new(11.0, 20.0).into()` / `CellSizing::Logical(..)` both
  work; default stays `Logical(11×20)`).
- `CellSizing::FromFont` requires `FontSizing::Px(size)` (with
  `FitCellWidth` it is a configuration error: `warn_once!` and fall back to
  `Logical(11×20)`); the renderer measures the regular face at that size —
  advance width (already done via the probe run) **and line height** (from the
  probe layout height / the font's ascent+descent+line-gap as Bevy exposes it)
  — and uses ceil-to-whole-physical-pixel cell dimensions. Re-measure exactly
  as today (font asset registered, config changed, own font assets changed).
- Expose the effective logical cell size: bring back
  `TerminalTexture::cell_size: Vec2` (**logical** pixels this time; document
  that physical = `cell_size * raster_scale`) and keep `font_size`.
- Add `TerminalTexture::grid_for(&self, logical_size: Vec2) -> GridSize`
  (columns/rows that fit, floor, min 1×1) and a free helper
  `render::grid_for_window(window: &Window, cell_size: Vec2) -> GridSize`;
  add `render::raster_scale_for_window(window: &Window) -> f32` (physical ÷
  logical from the window's actual sizes, ≥ 1.0) so a caller can compute the
  scale ratty passes as `TerminalRenderScale::Fixed`. Provide a
  `RatatuiTerminalExt::fit_to(&mut self, texture: &TerminalTexture,
  logical_size: Vec2) -> bool` (or equivalent) that resizes the grid to fit
  and reports whether it changed.
- Zoom = mutating `font_size` (`FontSizing::Px`) on the config; with
  `CellSizing::FromFont` that changes the cell size and hence the texture; the
  handle stays stable.

## 2. sRGB render target

- Render into `Rgba8UnormSrgb` instead of `Rgba8Unorm`: keep the shader
  emitting linear colors and let the render target's sRGB format encode. The
  texture is then display-ready for `ImageNode`, sprites, and 3D materials
  (`StandardMaterial::base_color_texture`) without a manual copy, and dark
  tones no longer band in 8-bit linear storage.
- Verify the UI presentation looks identical (compare exports before/after
  with `compare -metric AE`; small rounding differences are expected, no
  visible change), and update the benchmark adapter's capture path
  (`linear_rgba8_to_srgb` conversion is no longer needed — remove it, and
  confirm the harness's resolution/format validation still passes).
- Document the texture format on `TerminalTexture`.

## 3. Transparent backgrounds

- `TerminalTheme::background` alpha must be honored end to end: the clear
  color uses it, backgrounds painted with `TerminalColor::Default` inherit it,
  glyphs/decorations composite over it with straight (non-premultiplied)
  alpha, and the resulting texture is usable over a Bevy `ClearColor`/3D
  scene. Add a headless test that renders a terminal with a 50 %-alpha
  background and asserts an empty cell's texel alpha ≈ 128 and a full-block
  cell's alpha 255 (read the image back via `bevy_image_export`-style copy or
  the render app; if reading back is impractical in a unit test, do it in an
  example export and assert on the PNG in a `#[test]`-tagged integration test
  under `tests/`).
- Add an example (or extend `render_test`) that shows a translucent terminal
  over a colored background.

## 4. Feature footprint

- Trim `bevy_terminal`'s `bevy` dependency to the minimum:
  `bevy_asset`, `bevy_render`, `bevy_core_pipeline`, `bevy_image`,
  `bevy_text`, `bevy_ui` (feature-gated), `bevy_window`/`bevy_winit` only if
  actually referenced (the primary-window scale query), plus `std`,
  `async_executor`/`multi_threaded` as needed by Bevy — no `2d`/sprite.
- Cargo features on `bevy_terminal`: `ui` (default; `ImageNode`
  presentation + `UiScale`), `system_fonts` (default; `bevy/system_font_discovery`).
  Without `ui`, terminals are headless-only and `TerminalRenderScale::Automatic`
  resolves to 1.0. Without `system_fonts`, `FontSource::Monospace` etc. still
  compile but users must supply font assets; document that
  `FontSource::Handle(Handle::default())` uses Bevy's built-in FiraMono when
  `bevy/default_font` is enabled (monospace, ASCII/box-drawing coverage
  limited — say so).
- `bevy_terminal_ratatui` forwards both features (default on). Add CI-style
  checks to the verification: `cargo check -p bevy_terminal --no-default-features`
  and `--no-default-features --features ui` must compile.
- Keep `unsafe_code = "forbid"` and `missing_docs = "warn"`.

## 5. Fallback control (small)

- `FontFaces::fallback: Vec<FontSource>` — additional families/handles tried
  by Bevy's font system after the primary faces. Implement via the font family
  list Bevy/parley expose for a `TextFont` if available in 0.19; if there is
  no such API, document that fallback is system-wide and drop the field (do
  not fake it). Either way, document how fallback works in both READMEs.

## 6. Docs, examples, tests

- READMEs: concepts table rows for `CellSizing`, texture format, transparency,
  features; a "Terminal emulator setup" section (font-driven cells, fit to
  window, render scale from window, zoom) and a "Windowed TUI setup" section
  (`bevy_ratatui`-style: create backend without an `App`, spawn on
  `PostStartup`, resize on `WindowResized`); migration table 0.4 → 0.5.
- Examples: extend `multiple_terminals`/gallery to use `fit_to`/`grid_for_window`
  on window resize instead of hand-computed cells; a `terminal_emulator_like`
  example (font-driven cells, zoom with `+`/`-`, translucent background over a
  gradient) is welcome but keep it small.
- Tests: `CellSizing::FromFont` measurement (advance and line height at a known
  size for the embedded JetBrains Mono → cell ≈ 0.6·size × line-height),
  `grid_for`/`grid_for_window`, `raster_scale_for_window`, `fit_to` changes the
  grid exactly once, sRGB target format on the created image, transparent
  background alpha, features compile matrix, fallback behavior (or its
  documented absence).

## 7. Verification and performance gate

```text
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo check -p bevy_terminal --no-default-features
cargo check -p bevy_terminal --no-default-features --features ui
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo doc --workspace --no-deps
cargo tree -p bevy_terminal --edges all -i ratatui      # must not match
cargo tree -p bevy_terminal --edges normal --depth 1    # bevy only
cargo package --workspace --allow-dirty
```

Update the benchmark adapter (sRGB capture path), run the quick profile and the
standard three-repeat profile, and a paired A/B (0.4.0 binary vs 0.5.0,
interleaved, ≥3 reps); investigate any repeatable p50 regression > 5 % and
confirm renderer counters are unchanged. Re-run every exporter (`render_test
--export` for Iosevka Fixed and JetBrains Mono, `image_export`,
`high_dpi_export`, `multiple_terminals_export`, `ratatui_examples_export`,
`scene_export`), inspect the PNGs, and pixel-compare pre/post sRGB exports.

## 8. Report

Final public API delta, 0.4 → 0.5 migration mapping, feature matrix, what
`bevy_ratatui` and `ratty` integrations now look like (code sketch each),
test/clippy/doc/package results, A/B and harness tables, captures inspected,
and any intentional behavior changes.
