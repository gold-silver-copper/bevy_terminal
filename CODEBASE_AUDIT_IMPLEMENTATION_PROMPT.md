# Implement the codebase audit

Implement the justified improvements in `CODEBASE_AUDIT.md` for `bevy_terminal` and `bevy_terminal_ratatui`. Make the code cleaner, smaller, more legible, more idiomatic, and easier to consume. Remove duplication and unnecessary concepts while preserving rendering quality, correctness, and justified performance optimizations. Backwards compatibility and breaking semver are not concerns; do not retain compatibility aliases or deprecated wrappers merely to preserve the old API.

This is an implementation task. Carry the work through code, tests, documentation, examples, and consumer validation. Do not stop at another audit or a proposed plan. Treat the audit as evidence to revalidate, not an unquestionable specification: implement the smallest coherent solution to each confirmed problem, and explain any recommendation you reject or defer.

## Establish the current baseline

Read applicable `AGENTS.md` instructions and the full `CODEBASE_AUDIT.md`. Inspect repository status, remotes, branches, and relevant consumer changes before editing. Fetch upstream and compare current main with the audited revision `0042ac5b8fbaa3871a1cf61c659769c739123d79`. Revalidate findings against the actual implementation baseline; do not reintroduce resolved issues or discard newer work.

Both libraries live in this repository: the core is `crates/bevy_terminal`, and the Ratatui adapter is the root package. Ratty is an existing consumer in `../ratty`; its audited integration worktree was `../ratty-terminal-main` at `9da4be069e92b9be9843caf0c38024299e4282b2`. bevy_ratatui is in `../bevy_ratatui`, audited at `f1118a98ab8aaacbed8f9a03e1f278871f39e84e`. Locate current equivalents if those worktrees or revisions have changed.

Preserve existing staged, unstaged, and untracked work. Use isolated worktrees when needed. Do not reset, overwrite, or stash unrelated changes. Record the baseline revisions and distinguish existing failures from regressions. This prompt authorizes local implementation and verification, not commits, pushes, PR creation, or GitHub comments.

## Fix the confirmed defects first

1. **Canonical configuration comparison.** A supported `Fixed(NaN)` raster scale falls back to 1.0 but compares unequal every frame, causing continuous idle redraws. Make validation, effective values, and change comparison agree. Examine related floating-point configuration fields without assuming they have the same defect. Add whole-system regression tests that verify idle generations, counters, and Bevy change detection after settling.
2. **Measurement validity across shared resizes.** Backend pixel metrics currently match only grid dimensions, allowing resize-away-and-back through a shared surface to resurrect an old measurement. Track resize validity independently of content revisions. Ensure ordinary content edits do not invalidate geometry, while old measurements cannot be adopted after a newer resize or for a different surface.
3. **Statistics on early exits.** Reset per-frame work counters on loading, failure, invalid-sizing, and missing-resource paths as well as normal rendering. Preserve idle change-detection behavior. Test recovery and status transitions together with counters.

The audit reproductions were saved in `/tmp/bevy-terminal-audit-0042ac5` and `/tmp/bevy-terminal-audit-0042ac5.patch`. They assert the defective behavior, so convert them into tests of the desired contract. Recreate them from the report if the temporary artifacts no longer exist.

## Simplify the geometry and consumer APIs

Replace loosely related public measurement fields with a small coherent measured-geometry value where that reduces invalid states and consumer bookkeeping. Define ownership and validity explicitly: measured grid, resize generation, physical dimensions, logical dimensions, cell metrics, and raster scale must agree. Derive redundant quantities where practical. Keep the stable image handle and persistent loading/ready/failure status easy to query.

Give the backend an explicit way to adopt geometry from the presentation chosen by its owner. Validate that it belongs to the correct surface and resize generation. Multiple renderers may legitimately measure one surface differently; the renderer must not silently overwrite shared backend geometry. Keep fitting a requested grid separate from accepting a completed measurement. Never associate an old image size with a newly requested grid.

Keep plain `ratatui::Terminal<RatatuiBackend>` fully supported. bevy_ratatui's actual `TerminalContext` requires that dereference target and resource ownership. Put shared operations on the backend or use one small helper if demonstrated duplication warrants it. Keep `RatatuiTerminal` only for meaningful convenience; avoid forwarding the whole Ratatui API or introducing another terminal abstraction. Preserve application ownership of fixed viewports.

Update Ratty's integration for the resulting API and preserve its behavior during pending font/scale/resize measurements, including retaining last accepted geometry when appropriate. Keep PTY management, terminal emulation, input, and window policy in the consumer.

Update the real bevy_ratatui context fixture and migration guide against its actual trait. Establish that its windowed integration can use the renderer's image without CPU pixel copying, while native-only features remain isolated. A complete bevy_ratatui windowed migration is a subsequent task unless separately requested; do not silently broaden this library cleanup into a full consumer rewrite.

## Simplify renderer internals

Consolidate repeated status transitions, cancellation, snapshot invalidation, statistics handling, and event publication at a clear update boundary. Use small private resolved-input/outcome types only where they reduce argument lists and duplicated policy. Preserve separate invalidation causes for content, shaping, and presentation.

Tighten the existing metrics, shaping, scene, and GPU module boundaries. Move measurement helpers with measurement code, prefer narrow imports, and make dependencies explicit. Do not add public traits or split files merely to reduce line counts.

Preserve actionable measurement/shaping failure causes internally and expose useful bounded diagnostics. Avoid repeating identical logs every frame, caching failures as successful empty results, or losing recovery behavior.

Retain independent surface readers, batched/no-op writes, panic publication semantics, stable image handles, pending-scene coalescing, delayed-asset retention, and render acknowledgment. Main-world measured readiness must remain distinct from GPU completion.

## Address performance and resource limits with evidence

- Eliminate avoidable per-frame font catalog reconstruction and intermediate allocations. Account for delayed Bevy font registration, named/generic faces, removals, and empty-to-populated terminal queries. Measure idle CPU work and allocations; zero scene counters alone are insufficient.
- Evaluate lazy glyph-atlas allocation for loading and glyph-free terminals. Preserve valid solid-only rendering and existing atlas behavior once needed. Implement it if the memory savings justify a simple design; avoid speculative global glyph sharing or dynamic atlas machinery.
- Bound aggregate GPU instance uploads against device buffer limits using checked arithmetic. Preserve scene ordering and acknowledgment if uploads are chunked. Test with small injected limits instead of allocating excessive memory.
- Remove unnecessary scrolling clones where safe borrowing keeps the code legible. Preserve overlap direction, wide-cell behavior, and row revision semantics.
- Add representative workloads for heap-backed symbols, scrolling, resize/font/scale churn, cache saturation followed by a changed working set, and many idle terminals. Change shaping-cache admission or eviction only when measurements demonstrate a worthwhile improvement.

Report measured results separately from hypotheses. Retain fast paths and bounded caches that serve a demonstrated purpose. For a recommendation left unchanged, provide concrete reasoning and any benchmark evidence rather than merely labeling it optional.

## Complete verification and documentation

Update all affected examples, docs, exports/preludes, benchmark adapters, and consumer call sites. Remove superseded APIs and stale documentation. Add consumer CI that checks the actual bevy_ratatui context against a pinned checkout and supported feature configurations without depending on an undocumented local directory layout.

Inspect the export examples' fixed-frame readiness assumptions. Use a generation-aware preparation/readback condition if needed, or document and test the constrained scheduling contract. Do not expand public readiness APIs without establishing what the exporter actually requires.

Run repository-prescribed checks, starting with focused behavioral tests and broadening to affected feature combinations and consumers. At minimum, run:

```sh
cargo fmt --all -- --check
cargo check --workspace --all-features --all-targets --locked
cargo clippy --workspace --all-features --all-targets --locked -- -D warnings
cargo test --workspace --all-features --lib --tests --locked
```

Also run relevant doctests, supported minimal-feature builds, the real context fixture, and Ratty's affected tests. Run GPU smoke and glyph-fidelity checks for changes touching rendering, metrics, atlases, or submission. Compare failures with the untouched baseline before attributing them to the changes. Do not claim rendering correctness or consumer compatibility from compilation alone.

Perform a separate full-diff review after implementation, including relevant untracked files and consumer changes. Validate findings against current code, fix confirmed in-scope issues, and rerun affected checks. Do not fix unrelated failures; report the exact blockers and remaining verification if an environment or external dependency prevents completion.

## Final handoff

Summarize the resulting API and the concrete improvements, supported by a short usage example for both Ratty-style ownership and bevy_ratatui's raw-terminal context. List audit findings fixed, revised, or deferred with reasons. Report verification commands and results, performance/memory measurements, review findings and resolutions, and remaining risks or blockers. Keep the final implementation focused and reviewable, with production changes distinguished from diagnostic artifacts.
