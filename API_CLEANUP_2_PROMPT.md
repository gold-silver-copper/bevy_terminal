# Prompt: `bevy_terminal` / `bevy_terminal_ratatui` 0.3.0 API refinement

You are working in the `bevy_terminal` repository (workspace root =
`bevy_terminal_ratatui`, lower crate = `crates/bevy_terminal`). Both crates are
published at 0.2.0. Implement the following as one coherent breaking change and
bump both crates to **0.3.0**. This is an implementation task: make every change,
migrate examples, docs, tests and the benchmark adapter, verify, and report. Do
not keep deprecated aliases. Do not commit, push or publish unless explicitly
asked afterwards.

Preserve everything that works today: neutral scene model, transactional
surface, incremental dirty-cell snapshots, font-glyph-only rendering, measured
font sizing, unhinted rasterization, multiple independent terminals, headless
textures, high-DPI raster scale, wide-cell anchoring, and the dependency graph
(`bevy_terminal → bevy` only; `bevy_terminal_ratatui → bevy_terminal + ratatui`).
Performance must not regress (gate in §7).

## 1. Split the terminal component; user-owned UI presentation

1. Replace the single `Terminal { surface, config, presentation }` component
   with three ordinary components on the same entity:
   - `Terminal { surface }` — identity; construct with `Terminal::new(surface)`
     (or `Terminal::from(surface)`); expose `surface()`. Immutable after spawn.
   - `TerminalRenderConfig` — a `Component`, `#[require]`d by `Terminal`
     (default config inserted automatically). Change detection on *this*
     component (not on `Terminal`) drives re-measurement/rebuild, so touching
     unrelated data on the entity no longer clears the shape cache.
   - The UI presentation is no longer created by the plugin. Remove
     `Presentation`, `TerminalNode`, `TerminalRegistry` and the origin field.
     Instead: if the terminal entity has an `ImageNode` (and `Node`), the
     plugin keeps `ImageNode.image` pointing at the current texture and sets
     `Node.width/height` to the logical size (respecting a user-set
     `Node.position_type/left/top` and any parent layout). If it has none, the
     terminal is headless. Provide `Terminal::ui_node(&TerminalRenderConfig?)`
     or a documented pattern (`commands.spawn((Terminal::new(s), config,
     ImageNode::default(), Node { position_type: Absolute, left, top, ..}))`)
     and a helper `TerminalUiBundle`/constructor if it makes examples shorter.
     Also support the `ImageNode` living on a *child* entity via a marker
     `TerminalImage { terminal: Entity }` only if a use case in the examples
     needs it; otherwise keep it to the terminal entity.
   - `TerminalRenderScale::Automatic` still follows the primary window when
     the entity has an `ImageNode`, and resolves to 1.0 otherwise.
2. `TerminalTexture` and `TerminalStats` remain attached by the plugin.
   Despawning a terminal entity releases its images (no separate node to
   despawn any more).

## 2. Texture handle stability and readiness

1. Try to keep `TerminalTexture::image` **stable** across resizes by
   reallocating the `Image` in place (`Assets::insert(id, new_image)` /
   `get_mut` + `resize`) instead of `images.add`. Verify with an export test
   that a resize (larger and smaller) renders correctly on the next frames
   with no stale texels and no panics; check `ImageNode` picks up the new
   size. If it holds, delete `TerminalResized`. If it does not (document what
   went wrong), keep the identity change but still emit the event.
2. Add a readiness signal: trigger `TerminalReady { entity, image, size }`
   (entity event) once, the frame the texture is first allocated, so systems
   can react instead of polling `Query<&TerminalTexture>` with a `Local<bool>`.
   Convert the exporters/examples to use it (or an `Added<TerminalTexture>`
   query — pick the one that reads best and use it consistently).

## 3. Cruft and consistency

1. Remove `TerminalTheme::cursor` (dead since `CursorConfig::color`).
2. Make `TerminalTheme::{foreground, background, resolve}` `pub(crate)`.
3. `TerminalSnapshot::cell` and `GridSize::contains` take
   `impl Into<CellPosition>`; `TerminalSurface::new` and `SurfaceUpdate::resize`
   take `impl Into<GridSize>` (keep `(u16, u16)` working). Add
   `From<(u16, u16)>` where missing.
4. Document the `StyleFlags` bit layout as a stable contract (matches
   Ratatui's `Modifier` bits) on the type; keep the API as is.
5. `TerminalStats`: add a `Display` impl printing a one-line summary; make the
   `Instant`-based timers conditional on `TerminalPlugin { collect_timings:
   bool }` (default `true`) so they can be switched off; document that.
6. `SurfaceUpdate::commit`/`Drop`: document that `TerminalSurface::update`
   returns the same "published a revision" bool; keep both.
7. In the sync system, `warn_once!` when Bevy's text resources are missing
   instead of silently rendering nothing.
8. Font measurement: re-measure only when the terminal's *own* font assets
   change (track the `AssetId<Font>`s from `FontFaces` and use
   `AssetEvent<Font>` / `Assets::get` presence) or when `TerminalRenderConfig`
   changes; do not re-measure and clear caches whenever any unrelated font is
   added. Keep the "retry until registered" behavior for handle fonts.
9. `FontFaces`: document synthetic bold/italic behavior; add
   `FontFaces::with_synthesis(bool)` (default true) that controls whether
   `FontWeight::BOLD`/`FontStyle::Italic` are requested when falling back to
   another face.

## 4. Adapter DX (`bevy_terminal_ratatui`)

1. Add a one-call constructor for the common case, e.g.
   `RatatuiBackend::with_terminal(columns, rows) -> (RatatuiBackend, Terminal)`
   or `RatatuiBackend::terminal(&self) -> Terminal`. Use it in the README
   hello-world and examples.
2. Address the `ratatui::Terminal` name clash: re-export the component from
   the adapter prelude under a non-colliding alias (`TerminalRenderer` — pick
   one name, use it in every adapter example and doc) while `bevy_terminal`
   keeps `Terminal`. Do not rename the lower-crate type.
3. Keep `RatatuiTerminalExt::resize_grid`; also add `RatatuiTerminalExt::surface(&self) -> TerminalSurface`
   for symmetry if trivial.

## 5. Docs and examples

- Update both READMEs, `RATATUI_EXAMPLES.md` and crate docs. Add a short
  "Concepts" table at the top of each README (surface / cell / snapshot /
  terminal entity / texture / config). Fix the dangling "`TerminalRenderConfig`
  then contains" fragment. Extend the "Migrating" section for 0.2 → 0.3.
- Factor the shared example boilerplate (fonts, camera, spawning a terminal
  with an `ImageNode` at an origin, headless export registration) into
  `examples/common/` so each example shrinks; do the same for
  `crates/bevy_terminal/examples/common`.
- Every example must place its terminal(s) through ordinary Bevy UI nodes
  (absolute-positioned or in a layout) — `multiple_terminals` should show two
  terminals inside a flex row to prove layout participation.

## 6. Tests

Update existing tests and add: `Terminal` + `TerminalRenderConfig` split
(config change rebuilds, unrelated component change does not clear the shape
cache), `ImageNode` on the entity gets image/size updates and a resize keeps
the handle stable (or fires the event), headless entity without `ImageNode`
resolves `Automatic` scale to 1.0, `TerminalReady` fires exactly once,
`Into<GridSize>`/`Into<CellPosition>` call sites, `TerminalStats` `Display`,
`FontFaces::with_synthesis(false)`, measurement not repeated when an unrelated
font is added, adapter one-call constructor, prelude alias compiles alongside
`ratatui::Terminal` in a doctest.

## 7. Verification and performance gate

```text
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo doc --workspace --no-deps
cargo tree -p bevy_terminal --edges all -i ratatui      # must not match
cargo package --workspace --allow-dirty
```

Migrate `benchmarks/renderer-comparison/adapters/bevy-terminal-ratatui` (headless
entity, `FontSizing::Px`), run the bounded quick profile and the standard
three-repeat profile, and run a paired A/B (pre-change binary vs post-change,
interleaved, ≥3 reps) — investigate any repeatable p50 regression > 5 % on
`static/sparse/dense_ascii/dense_styled/unicode` at 80x24 or 120x40, and confirm
renderer counters are unchanged. Re-run `render_test --export` (Iosevka Fixed
and JetBrains Mono), `image_export`, `high_dpi_export`, `multiple_terminals_export`,
`ratatui_examples_export` and `scene_export`; inspect the PNGs, including the
resize frames.

## 8. Report

Final public API of both crates, 0.2 → 0.3 name mapping, what was deleted,
whether the stable texture handle worked, test/clippy/doc/package results,
benchmark before/after tables, captures inspected, and any intentional
behavior changes.
