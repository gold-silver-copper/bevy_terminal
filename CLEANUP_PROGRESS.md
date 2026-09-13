# Cleanup implementation and verification

Requirements: [CODEBASE_CLEANUP_PROMPT.md](CODEBASE_CLEANUP_PROMPT.md).

Implementation, verification, and the separate final review are complete. This report supersedes the earlier chronological execution notes. No commits, pushes, PRs, or GitHub comments were made.

## Revisions and consumers

- Core and adapter baseline: upstream main `fcc164b54399fd123eb9ae98b01181e4609638be`.
- Ratty: `aafa0eb9a6345055cd564290fc48fea6bf81d0b8`, branch `perf/ratty-benchmarks-rust-tooling`, with six focused integration files modified. Existing unrelated untracked files were preserved.
- bevy_ratatui: `f1118a98ab8aaacbed8f9a03e1f278871f39e84e`, unchanged. The standalone integration fixture compiles its actual context trait against this local adapter.
- The upstream baseline was checked before implementation. Final source remains an uncommitted worktree diff against that baseline. Original prompt files are preserved.

## Resulting API

`TerminalSurface` owns content and coherent `SurfaceInfo`; each `TerminalRenderer` owns its measurement, image, snapshot, and caches. `TerminalTexture::measured()` exposes persistent authoritative geometry. Logical cell/font/texture dimensions, physical image size, and raster scale travel together. `TerminalStatus` distinguishes loading, missing resources, failed fonts/shaping, invalid sizing, oversized textures, and measured readiness. Ready does not promise GPU completion.

`TerminalSizing` expresses three meaningful configurations: `FromFont { font_size, line_height }`, `FitCellWidth(cell_size)`, and `Fixed { cell_size, font_size }`. `TerminalSizing::font(size)` is the concise natural-line-height configuration used by Ratty. Paint-only changes preserve measurements and shape caches.

`RatatuiTerminal::new`, `from_backend`, and `drawn` return `Self`; `.with_renderer()` explicitly pairs the wrapper with its renderer. `draw` preserves `CompletedFrame`. Deref/DerefMut preserve access to Ratatui, including fallible drawing, without duplicating its methods. The tuple field is private; conversion from an existing configured Ratatui terminal is explicit.

Plain `ratatui::Terminal<RatatuiBackend>` is supported. Resize its backend, then call `autoresize` for fullscreen/inline viewports; fixed viewports use explicit `Terminal::resize`. The wrapper supplies `resize_grid`. Backend pixel metrics are explicitly supplied by the owner from its selected measured renderer and are invalidated on backend resize. Different presentations cannot overwrite shared surface metrics.

Ratty now polls persistent measured output after renderer sync, compares before adopting, and retries a minimized-window update on subsequent frames. Event markers and pending flags were removed. Requested scale remains separate from effective geometry. PTY, input, emulation, selection, and presentation policy remain in Ratty. Its main and xtask manifests intentionally use local paths to the modified library.

The [bevy_ratatui fixture and migration guide](integration/bevy-ratatui-context/README.md) cover resource ownership, the real context trait, drawing, measurement, buffer resizing, stable renderer attachment, feature isolation, and elimination of CPU image copying. The full consumer migration remains outside scope.

## Requirement coverage

| Requirement | Implementation and evidence |
| --- | --- |
| 1. Independent readers | Per-row revisions are compared with each reader's retained revision; snapshots never consume global dirty flags. Tests cover readers advancing at different rates, revision wrap, equal-revision surface replacement, and shared content with distinct renderer geometry. |
| 1. Panic/no-op/metadata | An update guard publishes completed writes during unwinding; poisoned locks recover consistently. Tests verify panic visibility and blank/redundant scroll no-ops. `info()` reads metadata under one lock without cloning cells. |
| 2. Real Bevy change detection | Output uses compare-before-write. UI helpers preserve `Mut` until an actual field changes. Tests cover idle output/UI ticks, late attachment, resize/configuration, and stable handles. World-quad tests verify geometry/material changes independently and preserve application material settings. |
| 3. Bounded pending ownership | One pending scene per destination; unacknowledged replacements are full scenes so intermediate dirty rows survive coalescing. Generation acknowledgement occurs after queue submission. Extraction prunes missing/failed owners; bind groups require live source ownership and a GPU asset. Tests cover delayed extraction, two queued row edits, rapid resize, renderer removal, and repeated spawn/despawn of owned images. |
| 4. Persistent measurement lifecycle | Readiness and failures are queryable after initialization. Tests cover missing resources, loading handles, malformed fonts and recovery, late registration, DPI/scale, zoom-related geometry, and transient shaping/probe errors. Ratty's integration tests validate adoption/reflow against the local renderer. |
| 5. Ratatui APIs | Constructors, CompletedFrame, explicit pairing, plain backend resizing, fullscreen/inline/fixed viewport tests, snapshot assertions, and the real bevy_ratatui context fixture cover both component and resource ownership. |
| 6. Coherent sizing | Invalid enum cross-products removed. Finite positive sizing validation, a surface allocation cap, saturating fit helpers, and pre-allocation device texture limits are covered by boundary/recovery tests. Paint invalidation preserves caches; selected assets and newly registered family fonts invalidate shaping. Existing glyph fitting and procedural tile geometry are retained. |
| 7. Focused organization | Public terminal components are in `render/terminal.rs`; metrics, shaping/atlas, scene generation, GPU submission/shader, and tests are separate private modules under `render/batch/`. Synchronization remains in `batch.rs`. Named `TextResources` and `TextContext` replace repeated resource argument lists. UI and world-quad presentation remain narrowly scoped. |
| 8. Duplication and memory | Unused foreground buffer removed; FontFaces selection delegates to one resolver; bitflags replaces manual flags; snapshot formatting streams without row-vector intermediates; all-widgets is dev-only. Shape entries and payload bytes are bounded; excess runs still render without being retained. Compact cells, ASCII lookup, fixed UVs, and batching remain. |

### Cost and design decisions

- Row revisions cost 8 bytes per row (192 bytes for 80x24), versus roughly one byte per cell for the old dirty array (1,920 bytes), in addition to the existing retained snapshots. A per-cell revision would cost 15,360 bytes at that size; a journal needs retention/reader coordination. The selected design scans changed rows and clones only cells whose values actually differ. It adds comparisons within those rows to keep readers independent without journal management.
- Surface updates publish at most one revision for a mutation batch; writes restored later in the same closure can conservatively publish because a real intermediate mutation occurred. Blank scrolling and individually unchanged writes do not publish.
- Shape caches retain at most 4,096 entries and 4 MiB of key/glyph payload, plus container overhead. Uncached overflow is used for the current draw. Tests cover both budgets and retained-entry stability.
- The unified 2048x2048 RGBA atlas retains 16 MiB on the CPU and another 16 MiB on the GPU per terminal. Fixed dimensions preserve stable UVs and batching. Full atlases fall back to Bevy source atlases. Packing now commits cursor/row state only on successful insertion and rejects overflowing dimensions; regression tests cover failed wrapping and u32 limits.
- Horizontal-placement caching was evaluated rather than added speculatively. A release microbenchmark of the actual fitting functions measured approximately 4 ns ASCII, 39 ns italic, 219 ns wide fallback, and 6.7 microseconds for a pathological 72-column run clipped into 20 pixels. These were directional measurements under compilation load. Common-case cost and the full renderer comparison do not justify another cache and invalidation mechanism.
- Bevy owns its global source-font atlases. Repeated system-font reload identities can retain historical entries there; terminal-owned output/unified images are released. The ownership regression uses a persistent bundled font to isolate library state. Clearing the shared Bevy cache could invalidate unrelated text and was rejected. README documents the boundary and persistent-font recommendation.
- Bevy 0.19 registers previously unseen font asset IDs. Replacing bytes under an already registered ID does not reliably replace its font collection entry. Recovery uses a fresh Font asset/handle; this limitation is documented rather than worked around with a duplicate registry.

## Verification

Commands run against the modified library:

```sh
cargo fmt --all -- --check
cargo check --workspace --all-features --all-targets --locked
cargo clippy --workspace --all-features --all-targets --locked -- -D warnings
cargo test --workspace --all-features --lib --tests --locked
cargo test --workspace --all-features --doc --locked
cargo test --workspace --all-features --test gpu_readback --locked -- --ignored --test-threads=1
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps --locked
cargo check --workspace --lib --no-default-features --locked
cargo check --workspace --lib --no-default-features --features ui --locked
cargo check --workspace --lib --no-default-features --features 3d --locked
```

The final verification matrix passed after the advance-retry fix: 79 core + 16 adapter tests, 7 core + 9 adapter doctests (one documented example ignored), five real GPU readback tests, strict docs, Clippy, and the three feature configurations. GPU tests ran on Apple M2 Max / Metal; Linux software Vulkan CI was inspected but not executed locally. Repeated global-logger diagnostics in the GPU harness did not fail its assertions.

Final review added `failed_advance_discards_previous_measurement_and_pending_content`. It failed at the stale-metrics assertion before the three-line invalidation fix, then passed. The full library checks, documentation, feature matrix, and GPU readback reruns passed after this fix.

Ratty verification uses its modified local path dependencies:

```sh
cargo fmt --all -- --check
cargo check --workspace --all-features --all-targets --locked
cargo test --workspace --all-features --lib --locked
```

Ratty passed 107 tests, ratty-vt 109 (3 ignored), and ratty-vte 36: 252 passed in total, including the final fix rerun. Root library resolution is Bevy 0.19.1; Ratty's existing lock resolves Bevy 0.19.0. Both are tested. The bevy_ratatui fixture passed its real integration test again after the final fix:

```sh
cargo test --manifest-path integration/bevy-ratatui-context/Cargo.toml --locked
```

### Glyph fidelity

The full all-font/all-scale command was run in both the isolated upstream worktree and the modified repository:

```sh
RUST_LOG=error cargo run --quiet --example glyph_fidelity -- --check --font all --scale all
RUST_LOG=error cargo run --quiet --example glyph_fidelity -- --check --font all --scale all --from-font 23.0 --tiles-only
```

The first command exits 1 in both versions with 66 pre-existing clipping-failure groups. Their complete textual reports are identical (157,044 characters); this is preserved baseline behavior, not a passing clipping suite. The separate font-driven tiles-only command passes across all six vendored families and four scales. Reports are `/tmp/bevy-terminal-glyph-baseline.log`, `/tmp/bevy-terminal-glyph-current.log`, and `/tmp/bevy-terminal-glyph-from-font.log`.

### Renderer benchmarks

Same release quick workload, 80x24 cells, 800x480 output, CourierNewPSMT, Apple M2 Max / Metal, GPU synchronization enabled, 5 warmup + 20 measured frames per repetition, three repetitions. Baseline and modified runs were sequential with compilation idle:

```sh
benchmarks/renderer-comparison/run.sh --profile quick --adapters bevy_terminal_ratatui --repeat 3 --no-build --output /tmp/bevy-terminal-cleanup-benchmark-baseline-final
benchmarks/renderer-comparison/run.sh --profile quick --adapters bevy_terminal_ratatui --repeat 3 --no-build --output /tmp/bevy-terminal-cleanup-benchmark-current-final
```

The baseline command ran in `/tmp/bevy-terminal-cleanup-baseline`, not the modified worktree. Each directory includes run metadata, raw results, aggregate CSV, and captures.

| Workload | Upstream median ms | Modified median ms | Change |
| --- | ---: | ---: | ---: |
| Static | 0.647542 | 0.633291 | -2.2% |
| Sparse | 0.740667 | 0.707333 | -4.5% |
| Dense ASCII | 0.778584 | 0.769459 | -1.2% |
| Unicode | 0.748291 | 0.775916 | +3.7% |

This short sample supports comparable performance, not a universal speedup. The Unicode slowdown is reported rather than hidden. Eleven of twelve paired captures are pixel-identical. The remaining Unicode capture differs at 1,158 pixels, all in Braille glyphs; upstream itself varies by up to 1,401 pixels between repetitions. Every pixel in the modified capture matches one of the three upstream variants. Two complete Unicode captures match upstream exactly. After the final advance-failure fix, the release adapter was rebuilt and the same baseline/current commands rerun into `/tmp/bevy-terminal-cleanup-benchmark-baseline-verified` and `/tmp/bevy-terminal-cleanup-benchmark-current-verified`:

| Workload | Upstream median ms | Final modified median ms | Change |
| --- | ---: | ---: | ---: |
| Static | 0.627958 | 0.646292 | +2.9% |
| Sparse | 0.709625 | 0.709375 | -0.04% |
| Dense ASCII | 0.764750 | 0.806416 | +5.4% |
| Unicode | 0.748333 | 0.757667 | +1.2% |

Both samples are retained to expose run-to-run variance rather than select the most favorable result. The final short run shows a modest dense-ASCII cost; no broad performance improvement is claimed. Ten of twelve final paired captures are pixel-identical. The two Unicode differences are again confined to Braille glyphs, with zero pixels outside the observed upstream variants. The complete all-font fidelity report comparison and GPU regression results remain as recorded above.

## Separate final review

The review covered the full core/adapter diff, moved private modules and tests, public APIs, examples, README contracts, manifests/lockfiles, the integration fixture, and Ratty's six changed files. It examined synchronization, lifecycle transitions, partial-scene replacement, render submission ordering, ownership, feature boundaries, and consumer resize semantics as a separate pass after implementation. No independent agent was used; delegation was not authorized.

Confirmed review findings fixed include atlas row-wrap rollback, fallback bind-group lifetime after owner removal, failed measurement retry, idle UI change ticks, numeric bounds, and the final advance-measurement stale-state edge case. Tests were added for the concrete failure paths. Rejected changes and dependency-owned limitations are documented above. The final pass checked the advance-failure invalidation and its before/after regression, consumer diffs, and artifact scope. The standalone fixture now ignores its build directory. Formatting, whitespace checks, required library checks, all affected consumer checks, GPU tests, feature combinations, and the rebuilt benchmark comparison completed. No confirmed in-scope review finding remains unresolved. Existing clipping failures and the shared Bevy font-cache/registration limits above remain explicit limitations; Linux Vulkan CI and a full bevy_ratatui migration were not performed.
