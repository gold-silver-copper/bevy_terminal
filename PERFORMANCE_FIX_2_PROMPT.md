# Prompt: Fix the verified `bevy_terminal` renderer hot spots

You are working in the `bevy_terminal` workspace (crate
`crates/bevy_terminal`, batch renderer in
`crates/bevy_terminal/src/render/batch.rs`). A measured profiling pass on
2026-08-24 identified six concrete inefficiencies. Implement all of them,
keep the visual output identical, extend regression coverage, and rerun the
renderer-comparison benchmarks to confirm the improvement.

## Hard constraints

- Normal dependencies must remain exactly `bevy` and `ratatui`; dev
  dependencies may be used for testing/benchmarking.
- Keep rendering on the existing compact quad-instance batch path drawn into
  the renderer-owned texture. Do not switch to a CPU raster, Egui, Vello,
  Parley, or a different rendering library.
- Preserve Ratatui cell semantics (colors, modifiers, wide/combining
  characters, clipping, clearing, cursor) and exact monospace-grid alignment.
- Do not gain speed by skipping required work, lowering resolution, changing
  logical terminal dimensions, or producing visually different output.
- Breaking internal or public APIs is allowed when it produces a cleaner,
  faster implementation.

## Measured baseline (2026-08-24)

Harness: `benchmarks/renderer-comparison`, results in
`results/2026-08-24-heavy` (180 frames, warmup 30, 2 reps, Apple M2 Max,
Metal, Courier New, GPU sync on). p50 end-to-end frame times:

| workload | grid | draw | bevy_update | p50 total |
| --- | --- | ---: | ---: | ---: |
| static | 120x40 | 0.17 | 1.61 | 2.00 ms |
| dense_ascii | 120x40 | 0.29 | 2.13 | 2.49 ms |
| dense_styled | 120x40 | 0.55 | 3.03 | 3.72 ms |
| dense_ascii | 240x54 | 0.78 | 3.68 | 4.65 ms |
| dense_styled | 240x54 | 1.23 | 4.66 | 6.36 ms |

Internal counters for 240x54 dense_styled: 12,960 glyph quads, 19,494 solid
quads, `scene_ns` ≈ 1.35–1.60 ms. The `static` workload shows a ~1.5–1.9 ms
Bevy app-loop floor with zero terminal work; that floor is out of scope.
Roughly 3 ms per dense frame is attributable to this crate: ~1.5 ms scene
build, ~1.5 ms serialization/upload/submission.

## Verified fixes to implement

Line numbers refer to `crates/bevy_terminal/src/render/batch.rs` at commit
`f01767a`.

### 1. Eliminate the instance serialization double-copy (largest win)

`build_scene` collects quads into scratch `Vec<QuadInstance>`s, copies them
into a fresh per-scene `Vec<QuadInstance>` (line 1663), and then
`instance_bytes` (line 2109) copies everything again into a fresh `Vec<u8>`
four bytes at a time (per-scalar `to_ne_bytes` + `extend_from_slice`; ~390k
tiny appends and ~1.5 MB of allocation churn per dense 240x54 frame).

- Serialize each instance as a whole (`[f32; 12]` / `bytemuck`-style manual
  byte copy — `unsafe_code = "forbid"` stands, so copy via safe slices of
  `f32::to_ne_bytes` chunks or restructure `QuadInstance` so a plain loop
  writes 48-byte chunks), not per scalar.
- Reuse persistent buffers: keep the byte (or instance) buffer alive across
  frames (in `BatchMainState` and/or `BatchGpuState`) with `clear()` +
  reuse instead of reallocating. Ideally the scene stores one flat
  reusable buffer and no second copy exists at all.
- Remove the dead `BatchMainState.vertex_capacity` field (written at line
  1441, never read; `BatchGpuState.vertex_capacity` is the live one).

### 2. Fast path for the shape cache (ASCII direct indexing)

`ShapeCaches` (line 963) keys `HashMap<String, usize>` per style class;
`cached_shape` (line 1689) hashes a string per non-space cell (13k/frame at
240x54) and allocates a `String` per insert.

- Add a direct-indexed fast path for single-byte ASCII symbols: e.g. a
  `[u32; 128]` (or 95-entry printable range) per style class mapping byte →
  cache slot, with a sentinel for "not cached". Fall back to the map only
  for multi-byte/combining symbols.
- The insert path must not allocate a `String` for the ASCII case.

### 3. Scope blink invalidation to blinking content

`BlinkPhases::at` runs every frame (line 1327); any phase flip sets
`blink_changed`, defeating the revision early-out (line 1330) and dirtying
every row (line 1350) even when no cell blinks. With the default
`CursorConfig { blink_hz: Some(1.0) }` an *idle* terminal rebuilds its full
scene twice per second.

- Track whether the current snapshot contains any `SLOW_BLINK` /
  `RAPID_BLINK` cells (cheaply, e.g. maintained during the snapshot diff or
  scene build) and ignore text-blink phase changes when none do.
- When only the cursor phase changed, dirty only the cursor's row (and
  render the cursor overlay), not all rows.
- Keep the existing correct behavior when blinking content *is* present;
  cover both cases with tests (idle terminal produces no rebuild across a
  blink phase flip; a terminal with a SLOW_BLINK cell still toggles).

### 4. Single encoder and submit for all pending scenes

`render_batch_scenes` (line 2125) creates one `CommandEncoder` and calls
`queue.submit` per scene, and every scene writes the shared vertex buffer at
offset 0, so multiple terminals serialize into N submits.

- Pack all renderable pending scenes into the shared vertex buffer at
  distinct offsets (one `write_buffer` or a few contiguous ones), record all
  render passes into one encoder, and submit once per frame.
- Preserve the existing deferral semantics: scenes whose destination or
  glyph textures are not yet in `RenderAssets<GpuImage>`, or whose
  destination size mismatches, must stay pending without blocking the
  others.

### 5. Remove per-frame allocations in `sync_batch_terminals`

- `changed_fonts: Vec<AssetId<Font>>` (line 1174) is built every frame even
  with no font events; early-out to an empty slice when no events fired.
- `font_asset_ids` (line 1246) allocates a `Vec` per terminal per frame; use
  a fixed-size array/`ArrayVec`-style structure (max 4 faces) or an
  iterator.
- Replace the linear `changed_fonts.contains` scan appropriately (fine once
  the list is empty in the common case).

### 6. Merge background quads vertically

Backgrounds merge into horizontal runs per row only (build_scene row loop,
line 1506): 19,494 solid quads for 240x54 dense_styled. When a full rebuild
emits identical horizontal runs on adjacent rows (same x-range and color),
merge them into one taller quad. Preserve paint order guarantees (the
`replace` background pass must still cover exactly the repainted region) and
partial-row repaint behavior. The synthetic per-cell-random workload will
not improve much — real block-colored UIs are the target; do not regress
the dense workloads.

## Required validation

1. `cargo test --workspace` passes; add the regression tests named above
   (blink scoping, ASCII fast path hit/fallback equivalence, vertical
   background merge correctness, multi-terminal single-submit ordering).
2. Visual validation: rerun the existing `bevy_image_export` snapshot
   tests; captured PNGs must be pixel-identical for the covered scenes.
3. Rerun benchmarks and compare against the baseline directory:

   ```
   cd benchmarks/renderer-comparison
   ./run.sh --adapters bevy_terminal_ratatui --sizes 120x40,240x54 \
     --workloads static,dense_ascii,dense_styled,unicode \
     --warmup 30 --frames 180 --repeat 2 \
     --output results/2026-08-24-perf-fix --no-captures
   ```

   Compare `summary.csv` / `aggregate.csv` to `results/2026-08-24-heavy`.
   Expected: dense 240x54 p50 improves noticeably (target ~1–2 ms off
   bevy_update from fixes 1+2+4); no workload regresses beyond noise.
   Also run the quick cross-renderer profile
   (`./run.sh --profile quick --no-captures`) to confirm parity ratios did
   not regress against `bevy_tui_texture`.
4. Report before/after tables for both runs, and note the idle-terminal
   improvement from fix 3 separately (it is invisible to the benchmark
   because the adapter sets `BlinkConfig::NONE` — demonstrate it via
   `TerminalStats` counters in a test or small harness instead).
