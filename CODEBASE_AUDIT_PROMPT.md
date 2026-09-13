# Audit bevy_terminal and bevy_terminal_ratatui

Audit both libraries for opportunities to make them cleaner, smaller, more legible, more idiomatic, and easier to consume. Return concrete suggested improvements backed by the code. This is an audit: do not implement the recommendations.

Backwards compatibility and breaking semver are not concerns. Recommend the best coherent API without compatibility aliases or deprecated wrappers. Preserve useful capabilities, correctness, rendering quality, and justified performance optimizations.

## Establish the baseline

Follow applicable AGENTS.md instructions. Inspect the working tree and repository layout, identify the upstream repositories for both libraries, and fetch their latest upstream `main`. Audit those revisions and record their commit hashes. If local changes exist, preserve them and use isolated checkouts for the upstream audit. Do not reset, discard, or overwrite existing work. Do not commit, push, create PRs, or comment on GitHub.

Read the architecture, public APIs, implementation, tests, examples, documentation, feature flags, and benchmark adapters. Run appropriate baseline checks and distinguish existing failures from proposed improvements. Do not treat existing audit or implementation documents as established findings: validate their claims against the selected revisions.

## Understand the consumers first

Inspect `../ratty` and `../bevy_ratatui`, locating their repositories if those paths differ.

- Ratty is an existing consumer. Trace how it owns terminal content, renders, resizes, handles fonts and scale, obtains measured geometry, and presents the output. Identify workarounds or duplicated state caused by the library API.
- bevy_ratatui will soon migrate to bevy_terminal_ratatui. Inspect its actual context abstraction and current windowed rendering integration. Determine the API it needs to replace its current rendering path, including resource ownership, plain `ratatui::Terminal` access, initialization, resizing, and feature isolation.
- Keep application responsibilities in the consumers. Avoid moving PTY management, terminal emulation, input policy, window management, or other application-specific behavior into these libraries.

Ground API recommendations in these real integrations. Clearly label any consumer assumptions you cannot verify.

## Audit scope

Review the following areas, following the actual code rather than assuming particular defects exist:

1. **Public API and crate boundaries:** naming, constructors, ownership, wrapper necessity, access to underlying Ratatui APIs, return values, resize semantics, renderer attachment, exports, preludes, defaults, and feature boundaries. Look for redundant ways to perform the same operation and awkward integration patterns.
2. **Retained content and synchronization:** revision tracking, independent readers, surface replacement, snapshot costs, lock scope, panic behavior, no-op updates, cursor metadata, and separation of shared content from presentation-specific geometry.
3. **Bevy integration and lifecycle:** component/resource ownership, scheduling, change detection, idle work, late presentation attachment, stable image handles, world-quad updates, asset loading, initialization, readiness, remeasurement, and teardown.
4. **Rendering correctness and resource lifetime:** partial updates, pending scene coalescing, extraction, delayed assets, rapid resizing, GPU submission ordering, cache invalidation, bounded queues, bind groups, glyph atlases, and long-lived memory growth.
5. **Sizing, fonts, and measured geometry:** meaningful configuration modes, invalid combinations, logical versus physical pixels, DPI, font fallback and registration, font failure/recovery, numeric and allocation limits, and authoritative measurements that consumers can query after initialization.
6. **Structure and idiomatic Rust:** oversized modules/functions, repeated resource plumbing, visibility, borrowing and cloning, error handling, unnecessary abstraction, duplicated logic, dead state, handwritten utilities better served by established types, and stale comments.
7. **Performance and dependencies:** avoidable allocations, repeated scanning or shaping, idle rendering work, unnecessary production features, and cache costs. Distinguish measured problems from hypotheses. Do not propose removing intentional optimizations merely to reduce line count.
8. **Tests, examples, and documentation:** missing behavioral coverage, misleading contracts, examples that hide integration difficulties, unsupported feature combinations, and migration guidance.

Prefer deleting unnecessary concepts, consolidating duplicated logic, and making ownership explicit. Propose abstractions only when they simplify demonstrated use cases. Avoid speculative extensibility or a wholesale rewrite without compelling evidence.

## Evidence and validation

For each substantive finding, inspect the relevant callers and lifecycle paths. Reproduce suspected correctness bugs with focused tests or small experiments where practical, keeping audit artifacts isolated. Use existing benchmarks for performance claims when feasible. Clearly distinguish confirmed defects, design tradeoffs, and unmeasured optimization opportunities.

Check supported feature combinations and the repository's prescribed validation where practical. Report the commands and results, including anything unavailable or blocked. Do not claim consumer compatibility or rendering fidelity solely from reading the code.

## Deliverable

Save the completed audit to `CODEBASE_AUDIT.md` and summarize the highest-priority recommendations in your final response. Record the audited upstream commit hashes and consumer revisions so the findings can be reproduced. Identify any relevant local changes separately from the upstream findings.

Provide a prioritized report with the highest-value improvements first. For each recommendation, include:

- The concrete problem and relevant file/symbol references, with line numbers where useful.
- Evidence or a representative usage that demonstrates the problem.
- The smallest coherent proposed change, with a short before/after API sketch when helpful.
- Expected benefits, tradeoffs, and effects on Ratty and the upcoming bevy_ratatui migration.
- The tests or measurements needed to validate implementation.

Separate correctness and lifecycle defects from API/design improvements and optional performance work. Group findings that share one underlying cause, avoid duplicate recommendations, and explain which existing designs should be retained and why.

Finish with a practical implementation sequence, the recommended final API shape for both consumers, and outstanding uncertainties. Keep the report specific enough to become an implementation brief, while leaving the code unchanged.
