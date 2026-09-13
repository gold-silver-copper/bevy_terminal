# bevy_ratatui context migration fixture

This fixture implements the actual `bevy_ratatui::context::TerminalContext`
trait using a resource-owned `ratatui::Terminal<RatatuiBackend>`. It compiles
against the sibling `../../../bevy_ratatui` checkout and this repository's
adapter. It does not start a native terminal or perform the full consumer
migration.

Run from the repository root:

```sh
cargo test --manifest-path integration/bevy-ratatui-context/Cargo.toml --locked
```

The test checks drawing, measured geometry, UI texture attachment, backend and
Ratatui buffer resizing, a stable image handle, and absence of a main-world CPU
image copy. It does not test GPU completion, window events, or input forwarding.

## Migration steps

1. In bevy_ratatui's windowed feature, replace the optional `soft_ratatui`
   dependency with `bevy_terminal_ratatui` using `default-features = false` and
   `features = ["ui"]`. Activate it only from the windowed feature. Keep the
   native backend and its terminal restoration path unchanged. Select the
   `system_fonts` feature only if the application intends to use host fonts;
   otherwise load an explicit bundled Bevy `Font` asset.
2. In `src/windowed_context/context.rs`, change the inner terminal and trait
   parameter from `SoftBackend<EmbeddedGraphics>` to `RatatuiBackend`. Keep
   `Deref<Target = ratatui::Terminal<RatatuiBackend>>` and `DerefMut`. Initialize
   with `ratatui::Terminal::new(RatatuiBackend::new(100, 50))`. Remove the bitmap
   atlas setup. The existing windowed `restore()` remains a no-op.
3. Add `TerminalPlugin` alongside the existing windowed plugin, after Bevy's
   asset/text/render plugins are available. Keep window creation, `Camera2d`,
   input forwarding, and application scheduling in bevy_ratatui.
4. Replace `TerminalRender(Handle<Image>)`, CPU image creation, and
   `render_terminal_to_handle` with one renderer entity. Obtain the context's
   `backend().surface()` and spawn `TerminalRenderer::new(surface)` with
   `TerminalRenderConfig`, `ImageNode::default()`, and the desired layout `Node`.
   Configure `FontFaces` and `TerminalSizing` explicitly to select the intended
   appearance. The renderer supplies and retains the image handle.
5. Replace bitmap `char_width`/`char_height` resizing with a query of that
   entity's `TerminalTexture`. Use `texture.measured()` before fitting. Its
   `cell_size` and `logical_size` are logical pixels; `size` is physical pixels
   and `raster_scale` converts between those spaces. Compute
   `texture.grid_for(available_logical_size)`, resize the backend when the grid
   changes, then call the context's `autoresize()` to synchronize Ratatui's
   buffers. Fullscreen and inline viewports support this path; fixed viewports
   require an explicit `Terminal::resize` for their application-owned area.
6. Reevaluate fitting after window size, DPI, or measured geometry changes.
   Query the persistent state rather than relying exclusively on the initial
   `TerminalReady` event. A system running after `TerminalSystems::Sync` sees
   that update's measurements. A resize it requests is rendered on the next
   sync; compare the grid before resizing to avoid a feedback loop.
7. If Ratatui callers need `Backend::window_size().pixels`, pass the selected
   renderer's measured physical `size` to `backend_mut().set_pixel_size(...)`
   after the resized output is measured. Shared content can have several
   presentations, so the backend owner chooses which one's metrics it reports.

`TerminalStatus::Ready` means measurement and main-world scene construction
succeeded. It does not mean the GPU has completed rendering or readback.
Loading and failure statuses are queryable even after a terminal was previously
ready; retain the last usable presentation while waiting, and surface failures
through the application's diagnostic UI. Export code must use the renderer's
GPU readback path and its completion signal.

The fixture enables the consumer's `std` and `crossterm` features to compile its
existing default context declarations. It never initializes that context. The
production migration must separately check bevy_ratatui's native-only and
windowed feature combinations.
