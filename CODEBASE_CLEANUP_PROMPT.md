# Clean up bevy_terminal and bevy_terminal_ratatui

Implement a thorough cleanup of both crates. Make the code smaller, clearer, more idiomatic, easier to maintain, and easier for consumers to integrate. Fix the correctness issues below before reorganizing their implementations.

Work in this repository. Ratty is an existing consumer at `../ratty`. `../bevy_ratatui` will soon migrate its windowed backend from `soft_ratatui` to `bevy_terminal_ratatui`. Inspect both consumers before deciding on public APIs. Their local code is the concrete integration reference; do not assume the audit's descriptions still match current code.

Backwards compatibility and breaking semver are not concerns. Prefer a coherent final API over compatibility aliases, deprecated wrappers, or parallel ways to do the same thing. Preserve useful capabilities and rendering quality.

## Scope and working approach

- Implement the changes, update documentation and examples, and verify the result. Do not stop at a plan or another audit.
- Follow applicable AGENTS.md instructions. Do not commit, push, create a PR, or comment on GitHub unless separately requested.
- Preserve unrelated work. Inspect the starting working tree and current revision before editing.
- Treat the findings below as hypotheses to validate against current code. Reproduce correctness issues with focused regression tests. If a finding is no longer applicable or a proposed solution would make the design worse, explain the evidence and choose the better solution.
- Keep the retained surface, renderer-owned stable texture, and thin Ratatui adapter architecture. Avoid a wholesale renderer rewrite, a generic backend framework, speculative abstractions, or unnecessary new crates.
- Implement consumer adaptations in `../ratty` when required by the new API, preserving its existing behavior. Keep those changes focused on integration. For `../bevy_ratatui`, validate the migration path with a representative context integration fixture/example and document the exact migration; implementing its full migration is outside this task.
- Work in coherent increments: correctness and lifecycle, consumer APIs, structural cleanup, then final verification. Keep user updates concise and regular.

## 1. Correct surface synchronization and ownership

Inspect `crates/bevy_terminal/src/surface.rs` and the renderer's retained snapshot state.

- Incremental snapshot synchronization currently clears shared dirty flags. One renderer can consume another renderer's changes. Support independent readers correctly, preferably with a simple per-reader revision and per-row change tracking design. Compare its memory and copying costs with alternatives before choosing.
- Track surface identity as well as revisions. Replacing a renderer entity's surface must invalidate the appropriate retained state even if the old and new surfaces have identical grid sizes and revision numbers.
- Clarify who owns pixel metrics when several renderer entities share one surface but use different configurations. Do not allow their measurements to silently overwrite one another as an implicit last-writer contract. Keep content state and presentation-specific measurements appropriately separated, while giving the Ratatui backend a clear pixel-size reporting contract.
- Define panic behavior for `TerminalSurface::update`. A panic can currently mutate cells without publishing a revision, after which poisoned locks are recovered. Use a guard or another small, explicit mechanism to keep state and revision publication consistent during unwinding. Do not introduce expensive transactional rollback without a demonstrated need.
- Align no-op revision behavior and documentation. Scrolling blank content currently publishes a revision despite unchanged-state claims. Distinguish actual content changes from conservative invalidation and document the chosen contract precisely.
- Add cheap metadata accessors so reading the cursor or dimensions does not clone all cells. Keep locking consistent and avoid separately locking fields that must form one coherent observation.

Acceptance: two readers observe every relevant update; replacing a surface refreshes content; panic recovery cannot leave changed state indefinitely invisible; metadata reads do not allocate a full snapshot; multi-renderer metric ownership is explicit.

## 2. Correct Bevy change detection and presentation updates

Inspect `sync_batch_terminals`, `sync_batch_terminal`, UI presentation, and `world_quad.rs`.

- Passing Bevy's `Mut<TerminalTexture>` as `&mut TerminalTexture` marks the component changed before any actual field write. Preserve change detection until a real change occurs, or compute and compare the next value before updating it.
- Idle terminals must not continuously trigger `Changed<TerminalTexture>` consumers or rebuild world-quad meshes/materials.
- Synchronize presentation independently of content dirtiness. Adding an `ImageNode` to an idle terminal must attach and size its texture without requiring a text edit or blink.
- Keep UI layout ownership explicit. Avoid surprising writes to application-owned presentation settings beyond the documented contract.
- Preserve application-owned material properties and stable image handles through resize and remeasurement.

Acceptance: tests cover idle change detection, late presentation attachment, configuration changes, stable handles, and world-quad updates only when relevant geometry or presentation settings change.

## 3. Bound GPU resource and pending-scene lifetimes

Inspect `BatchGpuState`, `PendingBatchScenes`, extraction, and GPU submission.

- Prune bind groups and other cached resources when their source assets or terminal owners disappear.
- Discard obsolete pending scenes when a terminal is removed or a resize/configuration generation supersedes them. Scenes waiting for dimensions that will never return must not remain queued forever.
- Preserve partial-update correctness when coalescing scenes. Replacing a queued payload must not lose dirty rows from earlier payloads. Use generation tracking or complete replacements where necessary.
- Preserve correct asset preparation ordering and the existing early-submission optimization where justified.
- Handle delayed assets and failed preparation without unbounded queues or stale content being submitted later.

Acceptance: repeated spawn/despawn, delayed asset availability, rapid resizes, and multiple queued partial updates have bounded retained state and produce the newest complete image.

## 4. Make readiness and measured geometry a clear consumer API

Ratty uses measured cell geometry to resize its PTY and synchronize custom presentation. Inspect `../ratty/src/terminal.rs`, `systems.rs`, `plugin.rs`, and relevant scene code.

- Provide persistent, queryable authoritative measurement state. A consumer joining after a one-shot event must be able to determine whether geometry is usable.
- Distinguish provisional allocation, measured geometry, and any promise of completed GPU rendering. Do not call a texture ready for export merely because main-world measurement finished.
- Missing text resources currently still lead to a readiness event. Replace this with an honest, observable state or clear initialization failure.
- Make font loading/shaping failures diagnosable. Avoid indefinite unexplained waiting and permanently caching a transient shaping failure as a valid empty glyph run.
- Define what happens when a ready terminal changes to a font that is still loading or fails to load.
- Give consumers a coherent geometry value containing the dimensions and scale they need, with an explicit logical/physical pixel distinction. Avoid duplicated, independently derived geometry in Ratty.
- Keep notifications where useful, but avoid forcing consumers to reconstruct persistent state from several events, marker components, and pending flags.

Acceptance: startup, late font registration, font failure, zoom, DPI change, and resize use one documented measurement lifecycle; Ratty reflows from authoritative geometry without feedback loops or redundant reflows.

## 5. Simplify the Ratatui-facing API

Inspect `src/backend.rs`, `src/lib.rs`, and `../bevy_ratatui/src/context_trait.rs` and `windowed_context/`.

- Prefer conventional constructors returning `Self`. `RatatuiTerminal::new` and `from_backend` currently return a terminal/renderer tuple, which resource-based consumers often destructure and partially discard. Provide an explicitly named bundle/pairing operation if it materially improves ECS spawning.
- Keep a plain `ratatui::Terminal<RatatuiBackend>` a first-class integration path. The Bevy component wrapper must be optional convenience, not the only place necessary operations are available.
- Provide an explicit supported resize path for plain backend users and for the wrapper. Keep the backend grid and Ratatui's buffers synchronized. Test applicable viewport/autoresize behavior rather than assuming every terminal uses the default viewport.
- Preserve Ratatui's useful API semantics. The infallible `draw` convenience should retain `CompletedFrame` rather than unnecessarily discard it. Support fallible drawing where appropriate without adding redundant wrappers for the entire Ratatui API.
- Decide deliberately which inner-terminal access is public. Avoid accidental escape hatches that undermine invariants, while retaining the access that `bevy_ratatui`'s context abstraction needs.
- Use one consistent renderer component name across the two crates. Reduce manual prelude duplication and ambiguous imports.

Acceptance: concise component-owned and resource-owned examples; a context fixture using `Terminal<RatatuiBackend>`; resizing, drawing, snapshot assertions, and renderer attachment work through documented public APIs.

## 6. Make sizing configuration coherent and invalidation precise

Inspect `CellSizing`, `FontSizing`, `TerminalRenderConfig`, and measurement/cache invalidation.

- Replace the cross-product of sizing enums if a single enum can directly express the meaningful modes: derive cells from a font size and line height; fit a font to explicit cells; or explicitly specify both font and cells.
- Eliminate the invalid `FromFont + FitCellWidth` combination and its fallback warning from the normal API.
- Centralize numeric validation and logical/physical conversions. Handle non-finite values, tiny cells, excessive dimensions, allocation arithmetic, and GPU texture limits deliberately.
- Keep requested settings distinct from effective measured geometry. Preserve font-driven zoom, line-height control, DPI snapping, wide glyph clipping, and seamless block geometry.
- Separate shaping/measurement invalidation from repaint invalidation. Theme, cursor appearance, and blink-rate changes should not discard font measurements and glyph caches unnecessarily.
- Track changes to fonts selected by family name as well as explicit asset handles. Newly registered family fonts must not leave previously shaped fallback output cached indefinitely.

Acceptance: invalid combinations are unrepresentable or explicitly rejected; the common Ratty configuration is concise; paint-only changes preserve shape caches; relevant font and geometry changes invalidate them correctly.

## 7. Restructure the renderer around focused responsibilities

`render/batch.rs` currently combines public components, Bevy scheduling, font measurement, shaping, glyph atlas management, CPU scene generation, GPU submission, shaders, and extensive tests.

- Extract focused modules with narrow interfaces for configuration/metrics, shaping and atlas management, scene construction, GPU submission, and presentation. Choose the smallest useful decomposition rather than a prescribed file count.
- Introduce a named Bevy `SystemParam` for repeated text resources and a small internal context where it reduces the long argument lists passed through shaping and measurement.
- Keep most implementation details private. Put public types where consumers can find them without exposing module organization as unnecessary API.
- Preserve comments explaining font and GPU invariants. Remove stale commentary and implementation narration that merely restates code.
- Move tests alongside their responsibility or into focused test modules as appropriate.

Acceptance: a reader can understand synchronization without reading atlas packing or shader construction; repeated resource plumbing and `too_many_arguments` suppressions are substantially reduced without hiding control flow.

## 8. Remove concrete duplication and unnecessary work

- Remove the unused `foregrounds` scene buffer and its plumbing after confirming it has no producer.
- Consolidate `FontFaces::select` and the internal font resolver so fallback order has one implementation.
- Prefer `bitflags` over handwritten `StyleFlags` operators, masks, and debug formatting if it preserves the compact representation and adapter mapping.
- Share snapshot text formatting logic without avoidable intermediate row/vector allocations.
- Remove unnecessary production widget features, such as `all-widgets`, if they are only needed by examples. Keep example-only dependencies/features in development configuration.
- Consider caching horizontal placement by shaped symbol/style/span so cached glyphs do not repeatedly scan coverage data. Measure this before expanding cache complexity.
- Review per-terminal atlas allocation and shape-cache growth for long-lived terminals. Bound or reduce retained memory where evidence supports it.
- Do not replace compact `CellSymbol` storage, ASCII lookup tables, or useful batching merely because a generic implementation has fewer lines. Retain measured optimizations unless an alternative has acceptable performance and materially improves maintainability.

## Consumer boundaries

Ratty owns PTYs, terminal emulation, input, selection, custom presentation, and application policy. This library supplies retained content, rendering, and authoritative geometry. Do not move Ratty-specific behavior into the library.

For `bevy_ratatui`, preserve its context abstraction and make the forthcoming windowed migration replace software rasterization/image copying with a surface-backed renderer entity. Window creation, input forwarding, terminal restoration, and its non-windowed backend remain its responsibilities. Ensure graphics dependencies can remain confined to its windowed feature.

## Verification and completion

The audit baseline was upstream commit `fcc164b`, with formatting, all-feature/all-target checking, and Clippy passing. Re-establish the current baseline rather than assuming it still holds. The audit also reproduced shared-reader lost updates, panic-time revision loss, and revisions from blank scrolling in an isolated harness; add real repository regression coverage for the chosen fixes.

Start with focused tests, then run the repository's required checks, including:

```sh
cargo fmt --all -- --check
cargo check --workspace --all-features --all-targets --locked
cargo clippy --workspace --all-features --all-targets --locked -- -D warnings
cargo test --workspace --all-features --lib --tests --locked
cargo test --workspace --all-features --doc --locked
cargo test --workspace --all-features --test gpu_readback --locked -- --ignored --test-threads=1
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps --locked
```

Update lockfiles through Cargo when dependencies change, then verify with `--locked`. Check supported feature combinations, especially no-default-features, UI, and 3D. Follow current CI instructions if they differ from this list. Run glyph-fidelity checks when changing measurement, fitting, atlas handling, blending, or glyph placement. Use the renderer benchmarks for changes to retained-data copying, shaping, allocation, or batching; compare the same workload and environment against a recorded baseline.

Verify all affected Ratty targets and tests against the modified local library, not an unchanged registry release. Keep any local dependency wiring intentional and reviewable. Validate the representative `bevy_ratatui` context fixture against the same modified library. Update this repository's examples, benchmark adapter, and docs for API changes.

Perform a separate final review of the complete diff and relevant consumer changes. Check correctness, resource ownership, lifecycle transitions, feature combinations, rendering fidelity, and missing tests. Fix confirmed in-scope issues and rerun affected verification. Do not broaden the task to unrelated consumer failures.

In the final handoff, explain the resulting API, major simplifications, consumer adaptations, findings fixed or rejected with evidence, verification commands/results, benchmark evidence, and any remaining limitations. Distinguish checks actually run from checks blocked by the environment. Do not claim GPU or consumer compatibility without testing it.
