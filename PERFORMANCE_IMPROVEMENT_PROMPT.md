# Prompt: Improve `bevy_grid` Performance

You are working in the `bevy_grid` Rust library. Improve its runtime rendering
performance substantially while preserving terminal layout, Ratatui semantics,
and visual output.

Do not stop at profiling or recommendations: identify the actual bottlenecks,
implement the optimizations, add regression coverage, visually validate the
result, and rerun the renderer-comparison benchmarks.

## Hard constraints

- The library's normal dependencies must remain exactly `bevy` and `ratatui`.
- Continue to render entirely with Bevy UI and Bevy text. Do not replace the
  renderer with a CPU-rasterized texture, custom WGPU renderer, Egui, Vello,
  Parley, or another rendering library.
- Dev dependencies may be used for testing and benchmarking. Retain
  `bevy_image_export` for visual render tests.
- Backwards compatibility and breaking semver are not concerns, so internal and
  public APIs may be redesigned when doing so produces a cleaner and faster
  implementation.
- Preserve Ratatui cell semantics, including foreground/background colors,
  modifiers, Unicode and double-width characters, combining characters,
  line-drawing glyphs, clipping, clearing, and cursor behavior.
- Preserve exact monospace-grid alignment. Do not obtain speed by accepting
  visible gaps between adjoining box-drawing glyphs, misplaced wide glyphs,
  incorrect backgrounds, overlap, or unstable row/cell spacing.
- Do not optimize benchmarks by skipping required work, lowering the rendered
  resolution, changing the logical terminal dimensions, or producing a
  visually different terminal.

## Existing benchmark and baseline

Use the pluggable benchmark harness in:

`benchmarks/renderer-comparison`

The detailed baseline is in:

`benchmarks/renderer-comparison/results/standard-20260816`

That run used 30 warmup frames, 180 measured frames, three repetitions, and the
`static`, `sparse`, `dense_ascii`, `dense_styled`, and `unicode` workloads. The
machine was an Apple M2 Max using Metal and Courier New. Other background Rust
jobs were active, so the absolute numbers are contention-affected, but the
results still establish the current order of magnitude.

Important `bevy_grid` frame-time baselines, reported as p50 / p95:

| Grid | Workload | Baseline |
| --- | --- | ---: |
| 80x24 | static | 3.304 / 13.414 ms |
| 80x24 | sparse | 3.284 / 12.247 ms |
| 80x24 | dense ASCII | 15.955 / 43.446 ms |
| 80x24 | dense styled | 37.149 / 63.765 ms |
| 80x24 | Unicode | 13.718 / 25.648 ms |
| 120x40 | static | 4.896 / 15.491 ms |
| 120x40 | sparse | 5.330 / 26.268 ms |
| 120x40 | dense ASCII | 52.552 / 129.075 ms |
| 120x40 | dense styled | 152.997 / 297.363 ms |
| 120x40 | Unicode | 22.992 / 37.108 ms |

The 120x40 dense-ASCII median currently consists of approximately 0.114 ms of
Ratatui drawing, 52.375 ms inside Bevy, and 0.064 ms of GPU waiting. The
dense-styled case is worse. This strongly suggests that Bevy UI/text update,
layout, text shaping, extraction, entity/component change detection, or render
preparation is the primary bottleneck rather than Ratatui buffer generation or
GPU execution. Confirm this through measurement instead of treating it as a
proven diagnosis.

## Required investigation

Profile and instrument enough of the renderer to attribute frame time and
allocation/entity costs. In particular, investigate:

- UI layout recalculation and propagation;
- Bevy text shaping and glyph layout;
- entity creation, destruction, reparenting, and component insertion;
- unnecessary component mutations that trigger Bevy change detection;
- render extraction and preparation caused by changed UI/text data;
- the number of UI nodes, text entities, text spans/sections, and styled runs;
- reconstruction of unchanged rows or cells;
- transient `String`, `Vec`, style, and run allocations;
- work repeated independently for text, backgrounds, decorations, and geometry;
- how style fragmentation affects dense-styled workloads;
- whether static frames perform meaningful renderer work after the first frame.

Record before-and-after measurements for the important counters and phases. Add
focused microbenchmarks or tracing/instrumentation if the existing harness does
not reveal the cause clearly, but keep the end-to-end harness as the source of
truth.

## Optimization direction

Choose the design based on evidence, but at minimum evaluate and implement the
applicable parts of the following:

1. Track dirty cells and rows precisely by comparing the new Ratatui buffer with
   retained renderer state. Do no text, style, layout, or ECS mutation for
   unchanged content.
2. Cache row layout and styled runs. Rebuild only the rows or runs affected by a
   buffer change, resize, font change, or configuration change.
3. Split content, geometry, foreground/style, background, and decoration
   dirtiness so a color-only change does not unnecessarily rebuild text or
   layout data.
4. Avoid assigning an unchanged value to a Bevy component. Compare retained
   values before taking a mutable component reference so Bevy change detection
   remains clean.
5. Reuse and pool entities, text spans, background nodes, and other retained
   render objects. Avoid despawning and respawning the terminal hierarchy during
   ordinary frame updates.
6. Reduce UI node and text-span counts where possible without losing exact cell
   anchoring, independent cell backgrounds, Unicode width behavior, or terminal
   styling. Test the tradeoff between row-based text, styled runs, and per-cell
   entities rather than assuming one representation is always fastest.
7. Precompute and cache cell geometry, row/column positions, resolved styles,
   reusable strings, and other stable data. Invalidate caches only for the
   settings that actually affect them.
8. Reuse allocation capacity and buffers on hot paths. Avoid per-frame clones,
   formatting, temporary collections, and repeated Unicode/style conversion.
9. Ensure a completely static terminal reaches a minimal fast path. Ideally,
   once initialized, it should require only the normal Bevy frame overhead and
   no terminal-specific ECS or text mutations.
10. Keep resize and font-metric changes correct. They may take a slower rebuild
    path, but ordinary updates must remain incremental.

Be willing to redesign the current retained representation if profiling shows
that smaller local changes cannot address the dominant cost. Keep the code
clear: make cache ownership and invalidation rules explicit, document the few
non-obvious performance invariants, and avoid unsafe code unless it is both
necessary and convincingly justified.

## Correctness and visual testing

Add or strengthen automated tests for:

- unchanged-frame fast paths and lack of unnecessary mutations;
- one-cell, one-row, sparse, and dense updates;
- style-only and glyph-only changes;
- resize and full invalidation behavior;
- mixed styled runs;
- ASCII, box drawing, block elements, combining marks, CJK/double-width text,
  emoji, and zero-width sequences;
- correct cleanup when wide glyphs are overwritten or clipped;
- foregrounds, backgrounds, modifiers, and cursor state.

Use the existing image-export examples/tests to render representative scenes
before and after the change. Inspect exported images at native resolution and at
high zoom. Specifically check:

- continuous horizontal and vertical box-drawing lines;
- corners and intersections without seams;
- background rectangles without one-pixel gaps;
- consistent baselines and cell advances;
- correct wide-character occupancy and following-cell alignment;
- stable output across unchanged frames.

Compare new images against existing references where available. Any intentional
pixel difference must be explained and must not weaken terminal fidelity.

## Fair benchmark configuration

Keep every backend on the same logical cell buffer for comparisons. For the
current font on this machine, use a 10x20 physical-cell target when performing a
resolution-matched comparison:

- `bevy_grid`: 18 px glyph size with an explicit 10x20 cell;
- `parley_ratatui`: 18 px font size with explicit 10x20 cell metrics;
- `soft_ratatui` and `egui_ratatui`: 18 px font, whose native metrics are 10x18,
  padded to an explicit 10x20 cell before resizing;
- `bevy_tui_texture`: 20 px font, whose native metrics are 10x20.

Auto-probe actual font metrics before warmup when possible, because these values
are font- and platform-specific. Record logical columns/rows, effective font
size, cell width/height, and output pixel dimensions in every result. Do not
change terminal columns/rows per backend and do not scale the completed image as
a substitute for matching native cell geometry.

The Parley adapter currently needs its font size set explicitly from the
benchmark configuration (for example,
`base_options.size = config.font_size as f32`) instead of relying on its default.
Fix that benchmark-adapter issue if it is still present before using the
resolution-matched results for comparison.

## Performance acceptance targets

On the same machine and under similarly quiet conditions:

- reduce `bevy_grid` p50 frame time by at least 40% for both 120x40 dense ASCII
  and 120x40 dense styled;
- introduce no statistically meaningful regression greater than 10% in the
  static, sparse, or Unicode workloads at either tested grid size;
- reduce p95 spikes as well as median time;
- demonstrate that improvements persist across at least three repetitions, not
  only one favorable run;
- preserve visual and behavioral correctness.

Treat these as minimum targets, not a reason to stop if the identified dominant
cost has a clear additional fix. If a target cannot be met within the Bevy UI
and text constraints, report the measured limiting subsystem and provide hard
evidence, but still land every sound optimization found.

## Verification

Run all relevant project checks, including at least:

```sh
cargo fmt --all -- --check
cargo check --all-targets
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
```

Run the benchmark workspace's formatting, tests, and Clippy checks too. Then run
the complete comparison suite with the same profile and at least three
repetitions. Prefer a quiet machine; record unavoidable background contention.
Save new raw JSONL, CSV, Markdown summary, run metadata, and visual render
artifacts under a new timestamped result directory rather than overwriting the
baseline.

Verify at the end that the root crate's normal dependency set is still exactly
`bevy` and `ratatui`.

## Final review and report

Review the full diff after implementation, including unstaged and untracked
files. Check specifically for stale-cache bugs, incomplete invalidation,
wide-character edge cases, component mutations that still bypass the fast path,
benchmark configuration drift, and optimizations that accidentally change
output.

In the final report, include:

- the bottlenecks found and the evidence for each;
- the design changes and their cache/invalidation rules;
- before/after p50 and p95 results for every workload and both terminal sizes;
- relevant phase timings and entity/allocation counters;
- percentage improvements and any regressions;
- links to the new benchmark summaries and visual artifacts;
- tests and validation commands run, with outcomes;
- remaining performance limits and the next highest-value optimization, if any.
