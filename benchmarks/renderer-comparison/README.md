# Headless Ratatui renderer comparison

This suite compares Ratatui renderers as complete render paths inside Bevy. It
does not use Criterion microbenchmarks and it does not measure a window's
vsync-limited frame rate. Each adapter is a separate executable driven by the
same deterministic workload protocol, so incompatible renderer dependency
graphs can be isolated without changing the controller.

The enabled adapters are:

| Adapter | Measured path |
|---|---|
| `bevy_grid` | Ratatui diff → compact Bevy text-atlas batch → renderer-owned texture |
| `soft_ratatui` | Ratatui diff → CPU raster → RGBA conversion → Bevy image upload/render |
| `egui_ratatui` | soft raster → egui texture/tessellation → Bevy image upload/render |
| `parley_ratatui` | Ratatui buffer → Parley/Vello → texture on Bevy's WGPU device |
| `bevy_tui_texture` | Ratatui diff → Bevy render-world WGPU terminal texture |

`ratatui-wgpu` is tracked, but its 0.5.0 public API cannot be run by this
windowless harness: the `RenderSurface` trait is sealed and the offscreen
`HeadlessSurface`/builder are compiled only under `cfg(test)`. The controller
reports that limitation rather than opening an incomparable hidden window.
`adapters.toml` also records related projects that are not Bevy texture
renderers.

## Run

From this directory:

```text
./run.sh --profile quick
./run.sh --profile standard --repeat 3
./run.sh --adapters bevy_grid,parley_ratatui --sizes 80x24,120x40
./run.sh --profile quick --font-size 16 # explicit native-metric experiment
./run.sh --list
```

Profiles:

- `quick`: 80×24, four workloads, 5 warmup + 20 measured frames.
- `standard`: 80×24 and 120×40, all workloads, 30 + 180 frames.
- `full`: adds 240×54, 120 + 1000 frames.

All builds use the workspace release profile (thin LTO, one codegen unit). The
controller writes a timestamped directory beneath `results/` containing:

- `results.jsonl`: full reports and raw per-frame samples;
- `summary.csv`: one comparable row per adapter/workload/size/repeat, including
  p50 draw, preparation, submission, Bevy-update, GPU-wait, and total timings;
- `aggregate.csv`: nearest-rank percentiles recomputed from every raw frame
  across repetitions;
- `summary.md`: per-process and aggregate human-readable tables;
- `parity.md`: aggregate `bevy_grid`/`bevy_tui_texture` ratios and gate status;
- `captures/`: one PNG captured after timed frames for every successful case;
- `run.json`: source fingerprint, Git state, controller, platform, adapter
  status, and failures.

Use `--output PATH` for a stable destination. The controller continues after an
individual adapter failure so the run record is complete, but exits non-zero by
default; `--allow-failures` changes only that final exit status.

Adapter order rotates deterministically between cases and advances again for
each repetition, so every case samples multiple process positions. `--order-offset N`
changes the initial position for an independent rotated run. Release binaries
are rejected when any of their relevant adapter, SDK, patch, or library sources
are newer; `--no-build` therefore cannot silently benchmark stale code. Use
`--no-captures` only for exploratory iterations—the PNG readback and encoding
are already outside the measured loop.

## What is measured

Each retained sample contains six timings:

| Field | Scope |
|---|---|
| `draw_ns` | Canonical workload closure, Ratatui buffer diff, backend draw/flush |
| `prepare_ns` | Renderer-specific CPU preparation (RGBA conversion, egui, Vello scene) |
| `submit_ns` | Main-thread texture update or direct GPU submission |
| `bevy_update_ns` | Complete Bevy main-world + render-world update |
| `gpu_wait_ns` | Explicit wait for every command submitted to Bevy's WGPU device |
| `total_ns` | End-to-end wall time enclosing all of the above |

The headline comparison is synchronized `total_ns`. Without the WGPU wait, a
renderer that queues more work asynchronously would appear artificially fast.
Disable it only for queue-pressure experiments with `--no-gpu-sync`; such
results should not be compared with synchronized runs.

There is no swapchain, OS window, compositor, vsync, screenshot readback, or PNG
encoding in a sample. Post-timing captures use Bevy GPU readback (or the
renderer-owned CPU pixels where applicable). Bevy renders to offscreen images. The direct Vello adapter
uses Bevy's own WGPU device and queue, so its explicit completion wait covers its
commands too.

## Comparable settings and caveats

The controller supplies every adapter with identical:

- Ratatui cell content and animation index;
- columns, rows, warmup, measured frames, and process boundaries;
- system monospace font bytes and a calibrated effective font size;
- requested logical cell size and Bevy offscreen target;
- synchronous Bevy pipeline compilation before sampling;
- per-frame WGPU completion policy.

The default comparison uses 10×20 physical cells for every backend. The
registry selects 18 px for `bevy_grid`, `soft_ratatui`, `egui_ratatui`, and
`parley_ratatui`, and 20 px for `bevy_tui_texture`. The soft renderers pad their
native 10×18 Courier New metrics to 10×20 before allocating their pixmaps;
Parley applies metric offsets; `bevy_grid` uses the requested cell directly;
and `bevy_tui_texture` natively produces 10×20 at its calibrated size. The
controller rejects a default run whose actual output does not equal the
requested pixel dimensions, and every raw/CSV/Markdown result records the
effective cell size, font size, and output size.

The calibration is font- and platform-specific. Passing `--font-size` opts out
of the strict matched-resolution check for metric-probing or renderer-native
experiments. If the shared system font changes, probe sizes first and update
the `matched_font_size` values in `adapters.toml`; never change logical columns
or rows or scale a completed image to make a comparison appear matched.

The common font is deliberately discovered at runtime rather than vendored.
This makes all adapters on one machine consume the exact same bytes, while
results from different machines remain explicitly labeled with the selected
font and GPU. Unicode fallback coverage can still differ because some renderers
support system fallback and others only receive the primary fixture.

For stable numbers, use an otherwise idle machine, release builds, a fixed
power/performance mode, the same WGPU backend, and several repeats. Do not mix
software-WGPU and hardware-GPU reports.

## Workloads

- `static`: identical dashboard after the first frame; measures retained/diff
  no-op behavior.
- `sparse`: dashboard with a counter and one moving marker; exercises small
  dirty regions.
- `dense_ascii`: every cell changes its printable ASCII glyph and foreground.
- `dense_styled`: every cell changes true colors, block symbols, and modifiers.
- `unicode`: wide CJK, combining sequences, emoji/ZWJ, box drawing, blocks,
  braille, RTL, and Indic text.

The workload generator is in `sdk/src/lib.rs` and is covered by deterministic
tests, including static stability and sparse change detection.

## Adding an adapter

For a Bevy 0.19 renderer in this workspace:

1. Add a binary package under `adapters/`.
2. Implement `renderer_bench_sdk::RendererAdapter`.
3. Render `render_workload` inside the backend's ordinary Ratatui draw call.
4. Put CPU preparation/submission into the matching `AdapterFrame` phases; let
   renderer Bevy systems run during the shared update.
5. Implement `capture_rgba` so visual validation covers the renderer's completed
   native-size output outside timing.
6. Add the package/binary to `adapters.toml` and the workspace members.
7. Run SDK tests, `cargo check --workspace`, then at least the quick profile.

For an incompatible Bevy/Ratatui version, use a standalone Cargo workspace and
emit the same JSON schema as documented in `PROTOCOL.md`; extend the registry
controller with its build/binary command. Process isolation is the compatibility
boundary—do not coerce two renderers onto ABI-incompatible WGPU types in one
binary.

## Compatibility patches

The root `bevy_grid` crate enables Ratatui 0.30.2's `scrolling-regions` feature.
The current releases of `soft_ratatui`, `egui_ratatui`, `parley_ratatui`, and
`bevy_tui_texture` do not implement the two backend methods exposed by that
feature. The benchmark workspace enables the same feature consistently, and
the `patches/` directory contains source pinned to each upstream release/commit
with only those methods added. Every patch has a `PATCH.md` recording its exact
scope. The canonical benchmark workloads do not invoke backend scrolling.
