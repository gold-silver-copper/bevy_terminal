# Trim the public terminal APIs

Audit and trim the public APIs of `bevy_terminal` and `bevy_terminal_ratatui`. Implement the justified changes, update consumers, and verify the result. Backwards compatibility and semver breaks are not concerns.

Keep `bevy_terminal` low level: applications supply cells, fonts, and explicit geometry; the renderer produces an image, validated geometry, and status. Ratty is an existing consumer, and `bevy_ratatui` will migrate to the adapter.

Read repository instructions and inspect the current worktrees before editing. Preserve unrelated changes. Treat the candidates below as hypotheses to validate against implementation and actual usage.

Investigate these changes:

- Make `CellSymbol` and `TerminalCell`’s symbol storage private. Keep a clear text accessor and preserve allocation behavior. Add a mutation API only if a demonstrated consumer needs it.
- Make `TerminalCell::continuation_of()` internal. Producers should submit anchors and declared widths; surface writes should maintain continuation cells. Preserve wide-cell overwrite, clipping, clearing, and scrolling semantics.
- Remove public `FontFaces::select()` if consumers do not need it. Keep production resolution internal. Adapt the fidelity oracle without making it depend on production resolution or duplicating the complete synthesis policy.
- Remove `FontFaces::with_synthesis()` in favor of its existing public field.
- Remove `font_family()` in favor of `FontSource::Family`.
- Remove `RatatuiTerminal::drawn()` and `snapshot()` where ordinary composition remains clear.
- Move `RatatuiTerminal::fit_to()` into application/example code. Preserve measurement validation, fractional geometry, resize behavior, and backend pixel-size updates when moving that logic.

Inspect the remaining public surface for similarly redundant APIs or exposed implementation details. Prioritize meaningful simplification; do not remove useful conveniences solely to reduce method counts or replace them with equally large abstractions.

Preserve:

- Independent use of `RatatuiBackend` with a raw `ratatui::Terminal`.
- `RatatuiTerminal` unless concrete evidence justifies removing it; Ratty currently uses it.
- Geometry validation and stale/foreign-surface rejection.
- Surface identity, coherent metadata reads, transactional updates, bounded caches, and diagnostics.
- The distinct sizing modes and both geometry-based and explicit-cell-size grid calculations.
- Existing glyph fidelity, shared baselines, color preservation, deterministic clipping, and Unicode occupancy.

Update examples, documentation, exports, tests, and benchmarks affected by each removal. Update Ratty’s existing renderer-migration worktree without mixing in unrelated work. Verify the real `bevy_ratatui` context fixture still works with a raw Ratatui terminal. Use local dependency overrides for consumer verification where necessary.

Run targeted regressions, then required formatting, locked checks, strict Clippy, tests, doctests, and documentation checks. Verify default/minimal feature configurations. Run affected GPU regressions, fidelity coverage, and Ratty’s headless smoke tests. Any oracle changes must retain independent expectations and complete pixel comparisons; do not weaken assertions to accommodate the cleanup.

Perform a separate full-diff review, fix confirmed in-scope findings, and rerun affected checks.

Report the APIs removed or made private, replacements at consumer call sites, candidates retained with reasons, verification results, and remaining limitations. Do not commit, push, create/update PRs, or modify GitHub unless explicitly requested.
