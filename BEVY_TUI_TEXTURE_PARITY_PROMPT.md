# Prompt: Make `bevy_grid` Competitive with `bevy_tui_texture`

You are working in the `bevy_grid` Rust repository. Continue profiling,
redesigning, implementing, testing, visually validating, and benchmarking the
renderer until `bevy_grid` is genuinely competitive with
`bevy_tui_texture` in the repository's headless renderer-comparison suite.

This is an implementation task, not a request for recommendations. Do not stop
after identifying a bottleneck, making one optimization, or writing a future
work section. Run an evidence-driven optimization loop, retain the best correct
implementation, and escalate to a different architecture when the current one
cannot reach the performance gate.

## Non-negotiable dependency constraint

The root library's normal dependencies must remain exactly:

```toml
[dependencies]
bevy = { ... }
ratatui = { ... }
```

- Do not add any other normal, optional, target-specific, build, or proc-macro
  dependency to the root crate.
- Retain the existing `bevy_image_export` development dependency for render
  QA, but do not add more root dev dependencies.
- The separate benchmark workspace may retain the dependencies needed to build
  its comparison adapters. None of those implementations may leak into or be
  linked by the `bevy_grid` library.
- Standard-library code, public Bevy APIs, Bevy's re-exported render types, and
  original WGSL embedded in this repository are allowed.
- Do not vendor, copy, or disguise another crate as local source merely to
  satisfy the manifest check. In particular, do not copy
  `bevy_tui_texture`, a font rasterizer, a shaping engine, or a software
  renderer into `bevy_grid`.
- Keep `unsafe_code = "forbid"`.

At the end of every substantial iteration, prove the dependency constraint
with `cargo metadata --no-deps` and inspect every dependency kind, not just the
human-readable `Cargo.toml`.

## Architectural freedom

Backwards compatibility and breaking semver are not concerns. Public and
internal APIs, component types, resources, plugins, retained state, scheduling,
and render integration may all be redesigned.

The output must remain entirely Bevy-native, but preserving the current
one-UI-entity-per-primitive representation is not a requirement. Start by
measuring the current Bevy UI/text implementation, then replace it if its
layout, entity, extraction, or text-preparation model prevents parity. It is
acceptable to implement a custom terminal batch inside Bevy's `RenderApp`
using only Bevy APIs, GPU buffers, render commands, pipelines, meshes, shaders,
font assets, and glyph-atlas/text infrastructure. A thin Bevy UI-facing wrapper
may position and clip the terminal while the grid itself uses a lower-level
Bevy render representation.

Do not switch to `soft_ratatui`, Egui, Parley, Vello, another renderer, or a
full CPU-rasterized framebuffer. Do not make `bevy_grid` a wrapper around
`bevy_tui_texture`. Study comparison implementations to understand cost models
and correctness requirements, but implement an original Bevy-only design.

## Current measured gap

Use the raw run in:

`benchmarks/renderer-comparison/results/standard-rerun-20260816`

That run used an Apple M2 Max, Metal, Courier New, 30 warmup frames, 180
measured frames, and three repetitions. Every renderer used native 10×20 cells:
80×24 produced 800×480 pixels and 120×40 produced 1200×800 pixels. The values
below are aggregate synchronized end-to-end p50 / p95 milliseconds over 540
samples per row.

| Grid | Workload | `bevy_grid` | `bevy_tui_texture` | Current p50 ratio |
| --- | --- | ---: | ---: | ---: |
| 80×24 | static | 2.107 / 6.012 | 0.793 / 2.650 | 2.66× |
| 80×24 | sparse | 2.470 / 11.487 | 1.483 / 2.912 | 1.67× |
| 80×24 | dense ASCII | 5.477 / 6.836 | 1.052 / 2.352 | 5.21× |
| 80×24 | dense styled | 3.880 / 4.226 | 0.939 / 2.183 | 4.13× |
| 80×24 | Unicode | 9.931 / 10.632 | 1.000 / 3.087 | 9.93× |
| 120×40 | static | 2.794 / 7.551 | 1.271 / 12.657 | 2.20× |
| 120×40 | sparse | 2.966 / 7.059 | 0.897 / 2.217 | 3.31× |
| 120×40 | dense ASCII | 11.782 / 13.745 | 1.282 / 1.451 | 9.19× |
| 120×40 | dense styled | 8.955 / 9.620 | 1.471 / 1.685 | 6.09× |
| 120×40 | Unicode | 11.699 / 21.282 | 1.541 / 4.844 | 7.59× |

These numbers are a starting point, not a frozen target. Run
`bevy_tui_texture` contemporaneously with every serious candidate because
system load, thermal state, drivers, and Bevy pipeline caches affect absolute
times.

## Definition of competitive

Do not declare completion until all of the following hold in a fresh standard
run on the same machine:

1. For every one of the ten workload/grid combinations, `bevy_grid` aggregate
   p50 synchronized frame time is no more than 1.10× the contemporaneous
   `bevy_tui_texture` p50.
2. For every combination, `bevy_grid` aggregate p95 is no more than 1.25× the
   contemporaneous `bevy_tui_texture` p95. Investigate isolated OS or shader
   compilation outliers rather than exploiting an accidentally inflated
   comparison p95.
3. The geometric mean of the ten `bevy_grid / bevy_tui_texture` p50 ratios is
   at most 1.00. This prevents barely missing every case while calling the
   result competitive.
4. The result persists across at least three repetitions and is not carried by
   one favorable process run. Report each repetition as well as the aggregate.
5. All comparisons use identical logical buffers, native 10×20 physical cells,
   identical output resolution, the shared font fixture, GPU synchronization,
   and the same warmup/measured-frame counts.
6. Behavioral, Unicode, layout, and visual gates in this prompt all pass.
7. The only normal root dependencies are still `bevy` and `ratatui`.

These are minimum gates. If `bevy_grid` becomes faster than
`bevy_tui_texture`, retain the faster correct design rather than adding work to
make the numbers look similar.

## Benchmark integrity before optimization

Audit and strengthen the harness before using small differences to make
architecture decisions:

- Confirm each adapter renders the intended workload rather than merely
  accepting buffer updates.
- Confirm GPU synchronization waits for every device and queue used by each
  adapter.
- Capture an image outside the timed region for every adapter, workload, and
  size. Validate dimensions and that static, sparse, dense, and Unicode scenes
  contain the expected content.
- Record buffer columns/rows, cell width/height, font size, actual output
  width/height, GPU, font, commit, renderer revision, and Bevy version in every
  report.
- Add a minimal empty-Bevy/offscreen-target measurement to establish the frame
  overhead floor. Do not subtract it from headline results; use it to determine
  how much renderer work remains.
- Rotate or deterministically interleave adapter order across repetitions so
  one backend does not always run at the same thermal or background-load
  position.
- Prebuild release binaries before a timed comparison and reject stale
  binaries using a source/build fingerprint.
- Keep pipeline initialization, font loading, atlas priming, and readiness in
  warmup. Do not move ordinary per-frame work out of the measured region.
- Keep screenshots, readback, PNG encoding, logging, and report serialization
  outside measured frames for all adapters.
- Aggregate raw frame samples using the harness's nearest-rank percentile
  definition. Do not average per-process percentiles.
- Save failures; never silently omit a slow or failed run.

Do not tune the benchmark specifically for `bevy_grid`. Any harness correction
must apply comparable semantics to every adapter and be documented.

## Required optimization loop

Repeat the following loop until the competitive gate passes:

1. **Measure the current best implementation.** Run focused paired benchmarks
   against `bevy_tui_texture`, capture phase timings and renderer counters, and
   identify the largest remaining term.
2. **Form one falsifiable hypothesis.** Examples: UI layout dominates dense
   ASCII, text shaping dominates Unicode, extraction copies too much retained
   state, or GPU buffer allocation causes p95 spikes.
3. **Build the smallest measurement or prototype that can disprove it.** Use a
   representative 120×40 workload and an empty-render baseline before making a
   repository-wide rewrite.
4. **Implement the winning change completely.** Handle normal updates, sparse
   updates, resize, config/font invalidation, cursor, blink, clipping, and
   teardown; do not leave a fast benchmark-only path disconnected from the
   public plugin.
5. **Run targeted correctness and render tests.** Fix every regression before
   considering its timing.
6. **Run paired quick benchmarks.** Compare raw samples, p50, p95, phase time,
   entity count, draw-call count, bytes uploaded, and changed components.
7. **Keep only a correct improvement.** If it trades one workload against
   another, calculate all ten ratios. Retain it only if it advances the parity
   gate or is a necessary, measured foundation for the next architecture.
8. **Checkpoint the best result.** Save the code state, exact commands, raw
   results, counters, visual hashes, and a concise conclusion in a new result
   directory. Never overwrite the starting run or the current best run.
9. **Escalate when a layer reaches its ceiling.** Do not spend many iterations
   polishing a representation whose measured lower bound cannot pass. Replace
   the entity/layout architecture and resume the loop.

Do not stop because the remaining work is difficult or because one workload
improved dramatically. If a Bevy public API blocks one design, record the exact
API limitation, test the next viable Bevy-only design, and continue. Stop only
for the completed gate or a genuine external blocker requiring user authority.

## Instrumentation and profiling requirements

Extend `TerminalRenderStats` or add dependency-free internal diagnostics that
can attribute at least:

- Ratatui draw/diff time;
- surface locking, comparison, dirty tracking, and snapshot time;
- scene/run generation time;
- ECS command generation and deferred-command application;
- Bevy UI layout time where applicable;
- text shaping/layout and glyph-atlas preparation time;
- main-world to render-world extraction time and bytes copied;
- render preparation/queue time;
- GPU submission and explicit wait time;
- active and retained entities/components;
- text spans, glyphs, solid rectangles, line segments, and batches;
- component insertions/mutations/removals per frame;
- spawned/despawned entities per frame;
- CPU vectors/strings whose capacity grows in a measured frame;
- GPU buffers, reallocations, write calls, and bytes written per frame;
- render passes, draw calls, pipeline switches, atlas bindings, and vertices or
  instances submitted.

Use `std::time::Instant`, counters, Bevy system ordering, and render-world
resources rather than adding a profiler dependency. Keep expensive diagnostic
collection behind a feature, resource, or benchmark configuration so the
measurement itself does not dominate production results. Verify conclusions
with diagnostics disabled in headline runs.

## Architecture work to evaluate

### Stage 1: remove remaining overhead in the retained implementation

Measure and implement the applicable changes, but do not assume they will be
enough:

- Replace per-update `Commands::insert` traffic with direct, type-stable
  component mutation where doing so avoids archetype/deferred-command work.
- Make dirty tracking cell-precise at write time when possible instead of
  rescanning the entire retained buffer after every revision.
- Separate glyph, foreground, background, modifier, geometry, cursor, and
  visibility dirtiness so color-only changes cannot dirty text layout.
- Cache resolved styles, cell geometry, row descriptors, glyph classification,
  and run boundaries. State exactly which config fields invalidate each cache.
- Reuse `Vec`, `String`, bitset, and descriptor capacity; measure capacity
  growth and eliminate hot-frame allocation.
- Precompute row/column pixel positions and line/block geometry.
- Prevent blink and cursor systems from taking mutable component access when
  the resulting value is unchanged.
- Eliminate all terminal-specific work on truly static frames, including
  extraction and render-buffer writes, not just main-world synchronization.
- Avoid full hierarchy rebuilds except for changes that truly invalidate every
  primitive.

After this stage, benchmark the theoretical lower bound. If thousands of UI or
text entities, spans, layouts, or extracted components remain on dense frames,
move to Stage 2 instead of endlessly tuning the pool.

### Stage 2: collapse primitive and entity count

Prototype representations whose terminal cost is O(rows), O(atlases), or a
small constant rather than O(cells):

- Batch all backgrounds, block elements, box-drawing segments, underlines, and
  cross-outs into one or a few retained meshes or instance buffers with
  per-vertex/per-instance color.
- Preserve overlap at line joins so single, heavy, and double borders remain
  continuous without pixel gaps.
- Test row-level Bevy text, `TextSpan` sections, `Text2d`, meshes, and hybrid
  representations. Measure the complete extraction and preparation path, not
  only scene construction.
- Implement a fast path for common monospace ASCII whose glyph advance is
  known to match the configured cell. Keep an exact anchored fallback for
  wide, combining, fallback-font, emoji, RTL, Indic, and otherwise complex
  cells.
- Make style/color data an instance or vertex attribute where possible rather
  than a separate ECS entity.
- Keep only one positioning/clipping object in the main world when the internal
  grid can be represented as retained render data.
- Update fixed-size dirty ranges in place. A dense frame may rewrite a complete
  compact instance buffer, but it must not recreate thousands of assets or ECS
  entities.
- Compare a complete compact-buffer rewrite against fine-grained dirty writes;
  small contiguous writes can be slower than one bounded upload.

Reject a batching scheme if it obtains speed by allowing proportional-font
drift, merging cells with incompatible shaping, dropping styles, or changing
paint order.

### Stage 3: Bevy-only render-world terminal batch

If Bevy UI/text entity processing still prevents parity, implement a dedicated
Bevy render-world path without adding dependencies:

- Extract one compact retained terminal scene only when its revision changes.
- Own persistent GPU buffers and grow them geometrically; do not allocate them
  every frame.
- Batch solid geometry and glyph quads into the minimum practical number of
  pipelines and draw calls.
- Use Bevy's font assets, font system, text layout, glyph atlas, and atlas image
  information through supported public APIs. Investigate the exact Bevy 0.19
  text pipeline rather than rerasterizing fonts independently.
- Cache shaped glyph sequences by `(cell text, font face/style, font size,
  cell width class)` and resolved atlas entries by glyph identity.
- Handle atlas growth or eviction without stale UVs. Make atlas invalidation
  explicit and tested.
- Put foreground/background color and terminal-cell position in compact
  per-instance data so dense color changes do not reshape text.
- Make cursor position, blink phase, and other uniform state a small uniform or
  instance update instead of a scene rebuild.
- Use a scissor rectangle or equivalent clipping derived from the terminal's
  Bevy placement.
- Preserve deterministic terminal paint order for backgrounds, glyphs,
  geometry, decorations, and cursor.
- Keep the plugin integrated with normal Bevy cameras/offscreen targets and
  lifecycle. Ensure multiple terminal instances can be represented cleanly if
  the redesign makes that practical.

The goal is not to imitate `bevy_tui_texture` line for line. The goal is to
remove the same classes of overhead—per-cell entities, UI layout, redundant
shaping, extraction churn, and allocation—while retaining `bevy_grid`'s exact
Ratatui semantics and dependency boundary.

## Correctness requirements

Preserve all currently supported behavior:

- Ratatui diff, clear, resize, append-line, and scrolling-region semantics;
- partial updates and cleanup when wide glyphs are overwritten;
- unit-width, double-width, combining, and zero-width cell content;
- CJK, emoji and ZWJ sequences, RTL, Indic text, braille, and font fallback;
- ANSI, indexed, true-color, reset, foreground, background, and underline
  colors;
- bold, dim, italic, reverse, hidden, underlined, crossed-out, slow blink, and
  rapid blink modifiers;
- exact backgrounds with no cell gaps;
- continuous light, heavy, and double box drawing;
- full, fractional, and quadrant blocks;
- cursor visibility, bounds, shape, movement, color, and blinking;
- clipping, origin, cell size, font size/source, theme, and resize behavior;
- deterministic output and stable unchanged frames.

Maintain exact terminal cell anchoring. Font fallback and wide glyphs must not
shift later columns. Never assume one Unicode scalar equals one glyph or one
terminal cell.

## Automated and visual testing

Add tests at the representation and integration levels for:

- a completely static frame producing zero scene/ECS/render-buffer changes;
- one-cell glyph-only, color-only, background-only, and modifier-only updates;
- one-row sparse updates and full dense updates;
- transitions between text, line geometry, blocks, quadrants, and empty cells;
- pool or buffer growth followed by shrink/reuse without stale content;
- resize, font, cell-metric, theme, origin, and clipping invalidation;
- glyph-atlas growth/invalidation if a custom batch is used;
- several terminals if the architecture supports them;
- ASCII, mixed styles, CJK, combining marks, emoji/ZWJ, RTL, Indic, braille,
  box drawing, blocks, and zero-width sequences;
- wide-character overwrite and right-edge clipping;
- cursor and blink state that cannot reveal inactive primitives;
- deterministic paint order across representation transitions.

Use deterministic exhaustive/property-style loops written with `std`; do not
add a random-testing dependency.

Run both image exporters against the current best pre-redesign reference:

```text
cargo run --example image_export
cargo run --example ratatui_examples_export
```

Compare all focused frames and all 43 example ports at native resolution and
high zoom. Retain hashes and difference images outside timed regions. Require
pixel identity where the rendering path should be identical. If a lower-level
Bevy path changes antialiasing or rasterization, inspect every difference and
accept it only if terminal fidelity is equal or better. Specifically reject:

- gaps in horizontal or vertical lines;
- broken corners or intersections;
- one-pixel holes between cell backgrounds;
- baseline or column drift;
- wide glyphs occupying the wrong cells;
- clipped combining marks or emoji;
- incorrect overlapping geometry, decorations, or cursor order;
- missing fallback glyphs, colors, or modifiers;
- stale pixels after sparse updates or resize.

## Benchmark cadence

During exploration, use focused paired runs such as:

```text
cd benchmarks/renderer-comparison
./run.sh \
  --adapters bevy_grid,bevy_tui_texture \
  --sizes 80x24,120x40 \
  --workloads static,sparse,dense_ascii,dense_styled,unicode \
  --warmup 30 \
  --frames 180 \
  --repeat 3 \
  --output results/parity-iteration-<N>
```

Use shorter runs only to reject clearly bad prototypes. A candidate cannot
become the new best from a short run alone.

Once all parity gates appear to pass, run the complete suite:

```text
./run.sh --profile standard --repeat 3 \
  --output results/parity-final-<timestamp>
```

Run it once more with the adapter order rotated. If either run fails the gate,
continue the optimization loop.

## Required verification

For the root crate, run:

```text
cargo fmt --all -- --check
cargo check --all-targets
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
cargo package --allow-dirty
```

For `benchmarks/renderer-comparison`, run:

```text
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
python3 -c 'compile(open("run.py", encoding="utf-8").read(), "run.py", "exec")'
```

Then inspect the complete local change set, including staged, unstaged,
untracked, generated, and ignored result artifacts. Review it separately for:

- stale cache or atlas references;
- incomplete config/resize/font invalidation;
- wide/combining/fallback shaping mistakes;
- buffer capacity and index overflow;
- render-world resource lifetime or device-loss issues;
- unnecessary Bevy change detection or extraction;
- hidden per-frame allocation or buffer recreation;
- benchmark work moved outside the measured region;
- different buffer size, font, cell geometry, output resolution, or GPU wait;
- source copied from a comparison backend;
- any root dependency other than Bevy and Ratatui.

Fix every confirmed high-severity finding and repeat affected checks,
screenshots, and benchmarks.

## Iteration records and final report

Every retained iteration must save:

- exact commands and source revision;
- raw JSONL, CSV, Markdown, and run metadata;
- per-repetition and aggregate p50/p95;
- ratios against the contemporaneous `bevy_tui_texture` samples;
- buffer grid, cell size, font size, and actual resolution;
- phase timings and renderer counters;
- relevant image hashes/diffs;
- the hypothesis, result, and keep/reject decision.

The final report must include:

- the complete architectural progression and why each discarded design hit a
  measured ceiling;
- the final main-world and render-world data flow;
- cache ownership and every invalidation rule;
- before/after entity, span, batch, draw-call, allocation, extraction, and GPU
  upload counters;
- fresh `bevy_grid` and `bevy_tui_texture` p50/p95 for every workload and both
  grids, per repetition and aggregate;
- p50/p95 ratios and geometric-mean ratio;
- exact buffer grids, font/cell metrics, and output resolutions;
- visual comparison results and artifact links;
- all verification commands and outcomes;
- dependency proof from `cargo metadata`;
- remaining risks, without using them to waive a failed parity gate.

Lead the final report with whether every competitive gate passed. Do not call
the task complete while any gate remains unmet.
