# bevy_terminal and bevy_terminal_ratatui audit

Audited September 12, 2026. This is a recommendation report; production code is unchanged.

The architecture is sound enough to improve incrementally. Keep shared retained content, independent renderer entities, and a thin Ratatui adapter. The highest-value work is making measurement coherent across the renderer/backend boundary and making renderer updates easier to reason about. A rewrite or a broader application framework would add complexity without addressing the demonstrated problems.

## Baseline and scope

Both libraries live in `gold-silver-copper/bevy_terminal`: core in `crates/bevy_terminal`, adapter at the repository root. Fetched upstream `main`; audited `0042ac5b8fbaa3871a1cf61c659769c739123d79`, also the local clean HEAD at the start of this audit.

Consumer references:

- Ratty main: `05308cd6703be24441f6913a537a16d35ed2c117`. Its upstream-main integration is `9da4be069e92b9be9843caf0c38024299e4282b2`, inspected in `../ratty-terminal-main`, and pins the audited library commit. The original `../ratty` checkout has unrelated/preexisting local changes; these were preserved and are not attributed to upstream.
- bevy_ratatui main: `f1118a98ab8aaacbed8f9a03e1f278871f39e84e`. Inspected its actual `TerminalContext` contract and software-rendered windowed implementation, plus this repository's `integration/bevy-ratatui-context` fixture.

Reviewed public types, surface mutation/snapshots, adapter translation and resizing, renderer scheduling/state, shaping/metrics/scene/GPU paths, features, examples, tests, and benchmark workload coverage. Consumer compatibility conclusions below distinguish source inspection from executed tests.

## Confirmed defects

### 1. Normalize configuration before comparing it — P2

**Evidence:** [`sync_batch_terminals`](crates/bevy_terminal/src/render/batch.rs#L538) compares the previous and current raw `TerminalRenderConfig` with derived floating-point equality. [`resolve_raster_scale`](crates/bevy_terminal/src/render/batch.rs#L331) separately sanitizes invalid scale values. `Fixed(NaN)` is explicitly supported by the documented fallback, but NaN never equals itself.

A focused test produced a ready terminal at effective scale 1.0, then observed increasing scene generations and every row marked changed on three subsequent idle updates. The existing scale-helper test verifies the fallback value but misses the whole-system consequence.

**Change:** compare a canonical effective configuration, or normalize accepted fallback values at the configuration boundary before storing the comparison state. Keep the documented fallback or explicitly reject invalid values; do not silently support them in one layer and treat them as a perpetual change in another. Inspect other floating-point configuration fields while doing this; only the scale case was reproduced here.

**Benefit / cost:** restores idle behavior for supported input and makes validation/comparison semantics agree. A private resolved configuration introduces a small type, but removes repeated interpretation of raw fields. Ratty and bevy_ratatui retain their existing sizing choices.

**Validation:** run real app updates for NaN, infinities, zero, negative, and clamped scale values; after settling, require unchanged generation, zero work counters, and no spurious output change detection. Preserve actual scale-change and blink behavior.

### 2. Shared resize can resurrect invalid backend pixel metrics — P2

**Evidence:** [`RatatuiBackend::set_pixel_size`](src/backend.rs#L49) stores `(GridSize, pixels)`; [`window_size`](src/backend.rs#L331) accepts the stored pixels whenever dimensions match. The documentation says resizing makes the measurement unknown until supplied again.

Reproduced sequence: backend 4×2, record 40×40 pixels, resize its shared surface to 8×2, then back to 4×2. Pixel metrics become unknown at 8×2 but return to 40×40 at 4×2 without a new measurement. `backend.resize` clears them correctly; mutation through the public shared surface bypasses that clearing.

**Change:** add a grid-resize generation to coherent surface metadata and bind pixel measurements to it. Do not use the general content revision: ordinary text edits must not invalidate geometry. A stronger follow-up API should accept a measured geometry value carrying the source/grid generation, rather than independently sampling the current grid when accepting a bare pixel size.

**Benefit / cost:** accurately represents measurement validity for shared writers and asynchronous resize flows. Adds one narrowly defined generation counter. The inspected Ratty path uses backend resizing and explicit measurement adoption, so this reproduction does not demonstrate a bug in that path.

**Validation:** cover resize-away-and-back, both resize entry points, text-only edits, two backends sharing a surface with distinct presentation metrics, and rejecting an old measurement after a newer resize.

### 3. Reset per-frame statistics before early exits — P3

**Evidence:** [`TerminalStats`](crates/bevy_terminal/src/render/terminal.rs#L189) promises zero counters on frames producing no terminal work. Counters reset only in [`sync_batch_terminal`](crates/bevy_terminal/src/render/batch.rs#L694), after outer validation and font-readiness branches.

Reproduced: render changed content, replace the configured font with a reserved unloaded handle, then update three times. Status is `Loading` and no scene is pending, but `changed_rows` retains its previous positive value. Missing-text and invalid-sizing branches have the same reset-placement problem by inspection.

**Change:** reset statistics once at the outer per-terminal update boundary, including the missing-text-resources path. Use the existing default value rather than a repeated list of field assignments, while avoiding unnecessary Bevy change notifications when already zero.

**Benefit / cost:** trustworthy diagnostics and benchmark attribution; very small implementation change, no consumer API expansion.

**Validation:** ready-to-loading, failed, invalid-sizing, missing-resource, and recovery transitions; assert counters and output status together.

## API and structural improvements

### 4. Make measured geometry one coherent value — highest API priority

**Evidence:** [`TerminalTexture`](crates/bevy_terminal/src/render/terminal.rs#L76) exposes status plus separately mutable physical size, logical size, cell size, font size, and scale. `measured()` returns the entire component; `grid_for()` is also callable directly on provisional fields. Consumers then manually transfer only some of this state to their backend.

Ratty's `src/terminal.rs` measurement adoption around lines 378–398 combines readiness, cell geometry, backend pixels, and separately selected window scale. The integration fixture similarly checks readiness, resizes a raw terminal, and supplies pixels in separate steps. The current code handles important cases, but the library makes every consumer preserve the same relationships.

**Change:** introduce a small validated `TerminalGeometry` value with private fields and accessors. Include its measured grid and resize generation; derive logical dimensions from physical dimensions and scale. Keep image ownership and status in `TerminalTexture`. Return geometry only while authoritative. Associate backend pixel metrics with this value's source/grid generation; the application still selects which renderer supplies them.

Illustrative direction, not an implemented API:

```rust
// Today
if let Some(output) = output.measured() {
    let grid = output.grid_for(available);
    backend.set_pixel_size(output.size);
}

// Proposed
if let Some(geometry) = output.geometry() {
    let grid = geometry.grid_for(available);
    backend.set_geometry(geometry)?; // validates source and resize generation
}
```

Grid fitting and adopting a measurement remain distinct: after resizing, consumers must wait for the matching new measurement. A helper must not label old pixels as belonging to a newly requested grid. Define this ordering explicitly in the docs.

**Benefit / tradeoff:** fewer partially valid tuples and less repeated consumer bookkeeping, at the cost of replacing field access with a few meaningful getters. Do not add a general layout framework or put PTY reflow into the library.

**Validation:** fractional DPI, scale changes that preserve physical dimensions, font remeasurement, rapid resizes, stale measurement rejection, and multiple renderers per surface. Preserve Ratty's policy of retaining its last accepted geometry while a new measurement is pending.

### 5. Keep raw `ratatui::Terminal<RatatuiBackend>` first-class

The real bevy_ratatui `TerminalContext` requires dereferencing to `ratatui::Terminal<T>`. The existing raw backend supports this and the integration fixture implements the actual trait. Retain that ownership model: resource-owned terminal, shared surface attached to an independent renderer entity.

[`RatatuiTerminal::fit_to`](src/backend.rs#L157) currently puts a geometry convenience exclusively on the wrapper. Make any revised geometry adoption operation a backend operation so both consumers can use it. Keep `backend.resize` plus Ratatui's `autoresize` explicit unless repeated real integration code justifies one small extension helper. Fixed viewports must remain explicitly owned by the application.

Do not force bevy_ratatui into an entity-owned wrapper, introduce another terminal trait, or duplicate the complete Ratatui API. Keep the wrapper only as useful optional convenience; remove redundant forwarding methods if the geometry redesign makes them unnecessary.

For bevy_ratatui's migration, replace the windowed software backend and RGB-to-RGBA image copying with the renderer's stable image handle. Keep input/window policy and native terminal restoration in bevy_ratatui. Make the new dependency part of its windowed feature so native-only builds retain their feature isolation.

**Validation:** compile the actual context trait, draw/resize through its dereference target, run the resource-owned integration fixture, and check native-only and windowed consumer configurations. The inspected fixture demonstrates the intended integration shape; it is not a completed consumer migration.

### 6. Consolidate renderer outcomes and tighten module dependencies

[`sync_batch_terminals`](crates/bevy_terminal/src/render/batch.rs#L465) repeats status assignment, pending cancellation, and snapshot invalidation across branches. Its inner update takes 13 arguments, including several related invalidation booleans. The statistics defect is one concrete consequence of distributing lifecycle policy between these layers.

**Change:** introduce a small private resolved-input value and an explicit update outcome such as unchanged, waiting, failed, or scene produced. Apply status, statistics, invalidation, and events at one boundary. Preserve distinctions between content changes, shaping changes, and presentation changes; merging all invalidation into one flag would lose justified optimizations.

The existing metrics/shaping/scene/GPU file split is useful, but broad parent imports and helpers in `render/mod.rs` still couple those files. Move measurement helpers with metrics; have scene construction accept explicit measured inputs; keep GPU code consuming completed scene data. Prefer narrow imports and types over another layer of public traits. Do not split files further solely to reduce line counts.

Also preserve causes of measurement/shaping failures. `measure_advance` converts failure to `None`, while shaping uses a failure flag and debug logging. Return an internal error with phase and relevant font identity, then expose or log a bounded diagnostic on transition. Keep the simple public status enum; consumers need actionable failure context without receiving Bevy internals or repeated logs every frame.

**Validation:** existing loading, recovery, surface replacement, pending-scene, idle change-detection, and remeasurement tests must retain their behavior. Add outcome-transition tests tied to the three reproduced defects. This refactor should follow the small fixes, not obscure them in one large patch.

## Performance and resource improvements

These are source-backed opportunities or risks, not benchmarked speedup claims.

### 7. Stop rebuilding the font catalog on every update

[`sync_batch_terminals`](crates/bevy_terminal/src/render/batch.rs#L501) rebuilds a `HashSet` by scanning all font assets and looking up their registered families every frame, even with no terminal entities. Font events also pass through two collected vectors. Each terminal then compares its full raw configuration before reaching the idle path.

**Change:** maintain font registration state with an explicit generation and update it when relevant inputs change. Account for Bevy's delayed registration: an asset event alone is not proof that its family is registered, so retain pending registrations until resolved. Avoid catalog work when no terminals need it, ensuring newly added terminals still get a correct initial catalog. Use Bevy change detection as an initial filter for configuration work, followed by effective-value comparison when needed.

**Benefit / tradeoff:** less idle allocation/scanning and clearer dependency tracking. A small registration-state resource adds state, so verify that it replaces rather than duplicates the current set and polling logic.

**Validation:** late handle fonts, family/generic fonts, asset replacement/removal, empty-to-nonempty terminal populations, and hundreds of idle terminals. Measure whole-system CPU time and allocations; zero scene counters alone do not establish zero idle overhead.

### 8. Allocate glyph storage lazily; bound aggregate GPU uploads

[`initialize_batch_terminals`](crates/bevy_terminal/src/render/batch.rs#L224) allocates a 2048×2048 RGBA atlas for every renderer before fonts or sizing are ready. That is 16 MiB of CPU image data per terminal, plus GPU storage when prepared, excluding target images and other caches. It is bounded per renderer, not a demonstrated leak.

**Change:** consider allocating the atlas on the first actual glyph. Solid-only scenes need a valid binding, so use a shared small placeholder or another minimal binding arrangement. Retain the existing fixed atlas once needed; dynamic atlas growth and global glyph sharing introduce substantially more complexity and should require measurements.

Separately, [`render_batch_scenes`](crates/bevy_terminal/src/render/batch/gpu.rs#L269) combines all scenes into one staging buffer and rounds GPU capacity up with `next_power_of_two`. Per-terminal texture limits do not bound this aggregate allocation against the device's maximum buffer size. This is a static resource-limit gap; no oversized GPU allocation was attempted during the audit.

Bound upload batches by device limits, use checked capacity arithmetic, and either chunk oversized scenes or return a controlled resource-limit outcome. Test arithmetic with small injected limits rather than allocating gigabytes. Preserve scene ordering and acknowledgment semantics when splitting submissions.

**Consumer impact:** lazy storage helps loading, hidden, or numerous terminals; upload limits matter to large/multiple rendered surfaces. Measure memory before adding more elaborate cache machinery.

### 9. Target cloning and cache churn with representative benchmarks

[`SurfaceState::scroll`](crates/bevy_terminal/src/surface.rs#L281) clones a source cell before checking whether the destination differs. Long symbols use heap storage, so even equal source/destination cells can allocate a temporary clone. Changed heap-backed cells may then incur another clone through the write path.

Compare source/destination before cloning and use disjoint borrowing where it stays legible. Preserve overlap direction and per-row no-op semantics. Avoid replacing this with an allocation-heavy snapshot or a complicated unsafe move algorithm.

The bounded shaping cache in [`shaping.rs`](crates/bevy_terminal/src/render/batch/shaping.rs#L316) stops admitting entries when full and retains earlier entries. That bounds memory, but a long-lived terminal can repeatedly shape newly common content after an earlier Unicode-heavy phase. This is a workload hypothesis, not an observed Ratty regression. Benchmark before choosing eviction, reserved ASCII capacity, or periodic cache replacement.

The comparison SDK currently covers static, sparse, dense ASCII/styled, and Unicode workloads. Add scrolling with inline and heap-backed symbols, repeated resize/font/scale changes, cache saturation followed by a different working set, and many idle terminals. Measure allocations and retained memory as well as frame time. Do not infer raw backend scrolling is Ratty's dominant path simply because terminals commonly scroll.

## Coverage and examples

### 10. Make consumer integration and asynchronous readiness executable contracts

The bevy_ratatui fixture uses a sibling checkout path and is outside the root workspace. Add a CI job that obtains a pinned consumer revision and runs it, plus the relevant native/windowed feature checks. Treat updating that pin as a deliberate compatibility check. Retain Ratty's measured-geometry and failure-transition tests alongside the library tests; they exercise the actual ownership boundary.

[`examples/common/export.rs`](examples/common/export.rs#L10) waits a fixed frame after `TerminalReady` before attaching an exporter. Public docs correctly distinguish measured geometry from GPU completion, but a fixed delay is a scheduling assumption. Prefer a render-preparation/readback condition for the relevant image generation, or explicitly constrain the example and test delayed preparation. No export failure was reproduced in this audit. Avoid expanding the public readiness API until the exporter integration establishes which signal it actually needs.

`Automatic` scale explicitly means the primary window, so secondary-window behavior is a documented limitation rather than a discovered contract bug. Keep explicit scale selection available and test the consumer's actual window. Add target-aware automatic scale only when the migrating windowed integration needs it.

## Designs to retain

- Shared `TerminalSurface` with independent readers and per-row revisions; an independently consumed dirty list would reintroduce reader interference.
- Batched writes, no-op detection, and publication of completed mutations during panic unwinding.
- Content separate from renderer-specific geometry; two renderers can legitimately measure one surface differently.
- Stable image handles, persistent queryable readiness, and distinct logical/physical units. Existing readiness events remain useful conveniences; deleting them alone does not simplify consumer ownership.
- Pending-scene coalescing, delayed-asset retention, and render acknowledgment. Do not simplify asynchronous state by assuming one update equals one completed GPU frame.
- Inline symbols, ASCII shaping fast paths, bounded caches, and retained GPU buffers where they have clear purpose.
- Current thin adapter and optional UI/3D presentation. Avoid moving terminal emulation, PTY management, input, or window policy into either library.

## Validation performed

Fresh checks against the audited production baseline passed:

```sh
cargo fmt --all -- --check
cargo check --workspace --all-features --all-targets --locked
cargo clippy --workspace --all-features --all-targets --locked -- -D warnings
cargo test --workspace --all-features --lib --tests --locked
```

The test run passed 79 core and 16 adapter tests. Five GPU tests and one glyph-fidelity test were ignored by that command. This audit did not rerun the ignored GPU/fidelity tests, doctests, the complete feature matrix, or full consumer test suites. All-features checking is not evidence that every minimal feature combination works. No rendering-fidelity or whole-application performance claim is based solely on these checks.

Three isolated reproduction tests passed while asserting the observed defective behavior:

- `audit_nan_scale_rebuilds_every_idle_frame`
- `audit_stats_remain_stale_when_font_starts_loading`
- `audit_shared_resize_away_and_back_revives_old_pixels`

They live in `/tmp/bevy-terminal-audit-0042ac5`, a detached checkout of the audited commit. A patch is saved at `/tmp/bevy-terminal-audit-0042ac5.patch`. These are diagnostic reproductions, not regression tests demonstrating fixes. Run from that checkout:

```sh
CARGO_TARGET_DIR=/Users/kisaczka/Desktop/code/bevy_grid/target \
  cargo test --workspace --all-features --lib --locked audit_ -- --test-threads=1
```

## Suggested implementation sequence

1. Fix canonical scale comparison, statistics reset placement, and resize-generation invalidation with focused regression tests.
2. Introduce coherent measured geometry and backend adoption. Update Ratty and the real bevy_ratatui fixture together; retain raw-terminal access and application-owned resize policy.
3. Consolidate renderer update outcomes and internal error context, then narrow module dependencies. Preserve all lifecycle and idle-work behavior.
4. Add consumer CI and missing workloads. Use results to decide font-catalog tracking, lazy atlas allocation, scrolling clones, and cache policy. Independently add checked GPU upload limits.
5. Complete bevy_ratatui's windowed migration against this API and validate feature isolation and actual GPU output before declaring it compatible.

The intended final public shape is small: `TerminalSurface` owns content; `TerminalRenderer` plus `TerminalRenderConfig` select rendering; `TerminalTexture` exposes an image, status, and optional coherent `TerminalGeometry`; `RatatuiBackend` provides the standard backend and explicit geometry adoption; raw Ratatui terminals and the optional convenience wrapper both remain usable. Ratty owns PTY behavior. bevy_ratatui owns its context, windows, and input.

No P0/P1 defect was established by the audit. Remaining uncertainty is concentrated in GPU stress limits, export scheduling, long-lived cache behavior, and the complete consumer migration. The three reproduced defects can be fixed immediately; the structural work should be guided by those contracts, and performance changes by the additional measurements above.
