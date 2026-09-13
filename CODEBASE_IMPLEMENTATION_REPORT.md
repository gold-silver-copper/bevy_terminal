# Audit implementation report

This document records the implementation verification checkpoint before the subsequent user-authorized publication. Git history and CI runs track publication separately.

Implemented locally and verified September 13, 2026. All ten audit findings are addressed below. Fixed-sizing glyph fidelity retains the baseline’s 66 failing groups; both complete printed fidelity reports are identical to baseline. No required implementation or verification work remains.

## Scope and baseline

Implemented the audit against library revision `0042ac5b8fbaa3871a1cf61c659769c739123d79`, the fetched upstream main at the start of the work. Core code lives in `crates/bevy_terminal`; the root package is the Ratatui adapter. The historical [audit](CODEBASE_AUDIT.md) describes that baseline, not the modified tree.

Ratty changes are local to `../ratty-terminal-main`, based on `9da4be069e92b9be9843caf0c38024299e4282b2`. The unrelated dirty `../ratty` checkout was preserved. The real bevy_ratatui context is checked against `f1118a98ab8aaacbed8f9a03e1f278871f39e84e`. Its complete windowed migration remains the explicitly separate follow-up described in the implementation prompt. No commits, pushes, PRs, or GitHub comments were made.

## Findings and resulting design

| Audit finding | Result |
|---|---|
| 1. Invalid scale redraws indefinitely | Bevy change detection gates effective configuration comparison. Fixed scales use their fallback/clamped value; disabled/invalid blink frequencies compare as disabled. Whole-system tests cover settling and repeated component writes. |
| 2. Shared resize resurrects old pixels | Every actual grid resize advances a separate generation, including away-and-back within one update. Snapshots and measurements carry that generation; text/cursor changes preserve it. |
| 3. Stale work counters on early exits | Outer validation and missing-resource exits reset statistics. Successful synchronization publishes a locally built value with change-aware assignment. Idle tests check Bevy notifications as well as counters. |
| 4. Incoherent public measurements | `TerminalGeometry` has private fields and derived logical dimensions. `TerminalTexture::measured()` validates status and source resize generation. Geometry uses weak source identity, so retaining old layout does not retain cell storage. |
| 5. Raw terminal integration | `RatatuiBackend::set_geometry` explicitly adopts the owner's chosen presentation and rejects foreign/stale geometry. Raw Ratatui terminals remain first-class; `RatatuiTerminal` remains optional convenience. No new terminal trait or application policy moved into the libraries. |
| 6. Lifecycle and module structure | `SyncInput` replaces the long positional argument list. A small `Result` outcome and one suspension boundary consolidate failure handling. Measurement helpers live in the metrics module, imports are narrower, and failure diagnostics retain phase/font context without repeating identical warnings. |
| 7. Idle font catalog rebuilding | Registration refresh is driven by font asset change detection, including delayed Bevy alias assignment. Event storage is reused, empty populations skip work, and repopulation refreshes the catalog. Idle scan tests and process measurements validate the change. |
| 8. Atlas and aggregate upload limits | Glyph-free/loading renderers retain a 4-byte atlas placeholder, growing the same image handle to the existing fixed atlas on first use. GPU upload planning splits scenes against device/host limits, preserves instance order, clears once, and acknowledges only after final submission. |
| 9. Scrolling and saturated caches | Disjoint slice borrowing removes temporary heap-symbol clones. Measured saturation justified resetting a full cache to admit the next working set; oversized individual runs remain uncached. Existing ASCII lookup and fixed atlas behavior remain. |
| 10. Consumer and export contracts | Pinned consumer CI covers the actual context trait, native-only configuration, and current windowed feature coexistence. Root export examples refresh readback buffers after GPU preparation; delayed insertion, growth, and shrinkage have a GPU regression. The static core export uses Bevy readback and waits for actual nonempty output. |

The real windowed consumer exposed an additional feature conflict: unconditionally enabling Ratatui's extended scrolling trait breaks soft_ratatui. The adapter now makes `scrolling-regions` opt-in. Direct surface scrolling is always available.

The implementation deliberately keeps independent snapshot readers, batched/no-op writes, completed-write publication during unwinding, stable image handles, pending-scene coalescing, delayed-asset retention, separate invalidation causes, and submission acknowledgment. A general outcome hierarchy, LRU bookkeeping, globally shared glyph storage, and dynamic atlas growth were unnecessary for the demonstrated improvements. Automatic DPI selection still follows the documented primary window; explicit scale remains available for other presentations.

## Consumer API

`TerminalSurface` owns content. Each renderer owns an image and its measurements. The backend owner selects which presentation supplies Ratatui pixel metrics. Query after `TerminalSystems::Sync`; measured readiness is separate from GPU completion.

For Ratty-style ownership, retain the last accepted geometry independently of the image handle:

```rust
if let Some(geometry) = texture.measured()
    && terminal.backend_mut().set_geometry(geometry)
{
    last_geometry = Some(geometry.clone());
    image_handle = Some(texture.image.clone());
}
```

Use `geometry.grid_for(available_logical_size)` to request a new grid. That request invalidates old measurements immediately; wait for the new geometry before reporting its physical pixels to Ratatui. An application can continue using the retained cell metrics for layout while font/DPI/reflow work is pending. Ratty's tests exercise that behavior, including failure states.

For bevy_ratatui, the existing context can continue to dereference to the raw terminal:

```rust
#[derive(Resource, Deref, DerefMut)]
pub struct WindowedContext(ratatui::Terminal<RatatuiBackend>);

let terminal = ratatui::Terminal::new(RatatuiBackend::new(80, 24))?;
let renderer = TerminalRenderer::new(terminal.backend().surface());
// Spawn renderer + TerminalRenderConfig + ImageNode; retain terminal as a resource.
```

Draw through ordinary Ratatui APIs. Resize the backend and call `autoresize()` for fullscreen/inline viewports; fixed viewports keep their application's explicit `Terminal::resize` policy. The [executable context fixture](integration/bevy-ratatui-context/src/lib.rs) implements the actual trait, draws, resizes, adopts geometry, and verifies stable image presentation without CPU image copying. Its [migration guide](integration/bevy-ratatui-context/README.md) defines the remaining consumer-owned window/input work.

## Measurements

See [workload methodology and raw results](benchmarks/audit-workloads/results/RESULTS.md). The standalone benchmark covers ASCII/heap scrolling, 32 idle terminals with 16 registered font assets, resize/font/scale churn, Unicode saturation, and a subsequent ASCII working set. It measures main-world elapsed time and process allocation calls using an instrumented allocator in an unpublished workspace. Production crates contain no unsafe allocator code.

The final three-run medians show substantially fewer heap scrolling allocations and lower observed idle time. Saturation followed by changing ASCII frames drops from 9,600 shape misses to 16. CPU image data for 32 blank terminals drops from 537,919,488 to 1,048,704 bytes: 32 eager 16 MiB atlases become 32 four-byte placeholders. These are CPU image allocations, not RSS or measured GPU memory.

Lazy allocation moves work to the first glyph. The churn workload includes that first 16 MiB allocation on the changed implementation while baseline setup paid it earlier; its slower cold-path timing is an explicit tradeoff. Debug-profile timings with atomic allocation accounting are comparisons of these workloads, not production FPS predictions. Final sampling includes the weak-reference review fix. Other compiler activity overlapped part of the sampling window; `sampling.json` records it. Treat elapsed times as exploratory shared-machine measurements, with allocation counts and shape misses providing the stronger evidence.

## Review

A separate full-diff review and a final pass after fixes covered library source, public API, manifests, tests, examples, documentation, consumer fixture/CI/lockfile changes, benchmark instrumentation, and Ratty's three modified source files. The implementation prompt calls for a separate review pass; this was performed locally rather than claiming an unavailable independent review.

Confirmed findings addressed during review:

- Retained geometry held a strong surface reference and extended cell storage lifetime. Replaced it with weak identity; the destruction regression passes.
- The export helper crossed the core crate's package boundary. The core static export now uses built-in readback and removes its exporter dev-dependency; the core package was inspected, both PNGs were saved successfully, and both captures were visually reviewed.
- The existing renderer-comparison workspace enables Ratatui scrolling regions directly. Its adapter now explicitly enables the matching library feature; its targeted build passed.
- Repeated GPU instance stride literals and stale example/API documentation were consolidated/corrected.

A suspected stale GPU atlas under upload throttling was rejected after checking Bevy 0.19.1's `prepare_assets`: it removes the old GPU asset before deferring preparation. The renderer retains scenes with missing assets. No extra atlas generation or readiness API was added on that unsupported hypothesis.

Small injected upload limits test planning, instance coverage/order, single clears/completions, and capacity arithmetic. Ordinary real GPU rendering is checked separately. Forced chunk boundaries have not been executed on a GPU; that is a test-scope limitation, not evidence from the normal GPU tests.

## Verification

- Workspace formatting, all-features/all-target check, strict Clippy, doctests, and warning-free docs passed after the geometry ownership fix.
- Workspace library/integration tests: 88 core and 18 adapter passed. GPU/fidelity tests are ignored in the ordinary invocation and run separately.
- Minimal builds passed with no defaults, UI only, 3D only, system fonts only, and scrolling regions only.
- Real context fixture runtime test and current windowed coexistence check passed; actual native consumer `std,crossterm` check passed with default features disabled.
- Ratty all-target check and formatting passed. Full library tests passed: 105 application tests plus 105 VT tests (3 existing VT tests ignored).
- Ratty's headless smoke passed all seven cases with zero stale cells: natural/compact fonts, missing family, short-lived child output, inline document images, style samples, and thirty scrolling inputs. PNGs were visually reviewed. Compact rows changed height from 660 to 570 while width remained 1210.
- Fixed-sizing glyph fidelity: both baseline and changed return exit 1 with the same 66 failed groups. The entire printed report, including clipping counts and advisory output, is identical. Font-driven tiles at size 23.0: all 24 family/scale cases pass in both checkouts, with identical printed reports. This preserves baseline behavior; it does not resolve its existing clipping failures.
- All six explicitly invoked GPU tests passed on Metal. The static core exporter saved both expected PNGs and exited; both were visually inspected. The core archive contains the standalone example without an out-of-crate helper or exporter dev-dependency.
- The existing renderer-comparison adapter compiles with its extended scrolling feature enabled. The new benchmark executable builds and passes strict Clippy.

The persistent [verification record](CODEBASE_IMPLEMENTATION_VERIFICATION.json) contains exact commands and results. Detailed logs are in `/tmp/terminal-verified-final`, `/tmp/terminal-final-verify`, `/tmp/terminal-baseline-fidelity`, `/tmp/terminal-export-final`, and `/tmp/terminal-ratty-smoke`. An intermediate shared-target build reused baseline artifacts and failed compilation; the two local packages were cleaned and the final root matrix rerun without concurrent baseline builds. Those intermediate compile failures are not used as fidelity evidence. CI configuration has been updated locally; no remote CI run has been created or claimed.

Representative commands (run from the library root unless specified):

```sh
cargo fmt --all -- --check
cargo check --workspace --all-features --all-targets --locked
cargo clippy --workspace --all-features --all-targets --locked -- -D warnings
cargo test --workspace --all-features --lib --tests --locked
cargo test --workspace --all-features --doc --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked
cargo test --workspace --all-features --test gpu_readback --test export_readiness --locked -- --ignored --test-threads=1
cargo run -p bevy_terminal --all-features --locked --example scene_export
cargo run --all-features --locked --example glyph_fidelity -- --check --font all --scale all
cargo run --all-features --locked --example glyph_fidelity -- --check --font all --scale all --from-font 23.0 --tiles-only
cargo test --manifest-path integration/bevy-ratatui-context/Cargo.toml --locked
cargo check --manifest-path integration/bevy-ratatui-context/Cargo.toml --features consumer-windowed --locked
cargo check --manifest-path benchmarks/renderer-comparison/Cargo.toml -p bench-bevy-terminal-ratatui --locked
```

All commands above pass except the explicitly baseline-matched fixed-sizing fidelity invocation. The verification record includes each no-default-feature build, native consumer command, Ratty check/test command, package check, and smoke results.

Ratty was verified with this local Cargo patch:

```text
patch."https://github.com/gold-silver-copper/bevy_terminal".bevy_terminal_ratatui.path="/Users/kisaczka/Desktop/code/bevy_grid"
```

It was supplied through `--config` to Cargo. Ratty's original upstream lockfile is restored; `/tmp/terminal-ratty-smoke/Cargo.local.lock` retains the local-patch validation lockfile. A new local patch run must first allow Cargo to regenerate its lockfile, then can use `--locked`. The three consumer source changes require this library API when it is published; the consumer manifest was not silently repointed.

## Completion audit and limits

- Baselines, working trees, preserved unrelated work, and consumer revisions are recorded. No external changes were made.
- The three reproduced defects have behavioral regression tests, including idle change detection and shared resizes within one update.
- Geometry ownership, source/generation validation, raw terminals, Ratty's pending layout, and the actual context trait are implemented and exercised.
- Renderer outcomes, diagnostics, module boundaries, scrolling, font registration, atlas allocation, and bounded uploads are implemented, reviewed, and checked.
- Representative allocation/CPU workloads and cache-admission evidence are saved with raw samples and explicit timing limitations.
- Examples, API docs, feature boundaries, benchmark adapters, and pinned consumer CI are updated. Audit reports and benchmark artifacts are excluded from the published adapter package.
- Formatting, linting, tests, docs, feature builds, consumer checks, GPU smoke, exact fidelity comparisons, package inspection, and final full-diff review are complete. Review findings were fixed and affected checks rerun.

Remaining limits are the unchanged baseline clipping failures, CPU-only forced-chunk planning coverage, and shared-machine benchmark timing. Automatic scale still uses the primary window as documented. The complete bevy_ratatui windowed migration and publication/remote CI are separate work, explicitly outside this local implementation task.
