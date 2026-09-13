Refactor `bevy_terminal` and `bevy_terminal_ratatui` to be smaller, lower-level, and easier to understand.

The core contract should be:

**Terminal cells + rendering configuration → image + validated geometry + status.**

Applications own presentation, layout, window selection, and DPI policy. Backwards compatibility and breaking semver are not concerns.

Inspect the current implementation and consumer integrations before editing. Ratty consumes these libraries, and `bevy_ratatui` will soon migrate to `bevy_terminal_ratatui`. Validate each proposed removal against actual usage.

Complete the following:

1. **Remove library-owned 3D presentation.**
   - Remove `TerminalWorldQuad`, its synchronization systems, and the `3d` feature from both crates.
   - Keep a minimal example showing how an application binds the terminal image to its own mesh and material.
   - Applications should retain full access to terminal textures and measured dimensions.

2. **Separate UI presentation from core rendering.**
   - Stop core rendering from modifying `Node` dimensions or `ImageNode.image`.
   - Move useful presentation code into examples initially. Introduce a separate optional presentation plugin only if concrete consumer needs justify it.
   - Remove obsolete UI dependencies, feature branches, and default features where feasible.

3. **Make raster scale explicit.**
   - Remove core dependence on `PrimaryWindow`, `UiScale`, and presentation-component detection.
   - Use one explicit raster-scale contract for headless, UI, and world-space rendering.
   - Relocate necessary window/DPI calculations into consumer code or presentation examples. Preserve Ratty’s framebuffer-ratio behavior and fractional logical geometry.

4. **Use persistent output as the single readiness contract.**
   - Remove `TerminalReady`, `TerminalRemeasured`, and event-history bookkeeping.
   - Migrate integrations to `TerminalTexture::measured()`, status, and scheduling after `TerminalSystems::Sync`.
   - Preserve late-consumer support, stale-geometry rejection, and retained layout while replacement measurements are pending or fail.
   - Keep the distinction between measured geometry and completed GPU rendering/readback explicit.

5. **Reduce convenience APIs and duplicated entry points.**
   - Remove `TerminalFonts` font-discovery wrappers; demonstrate direct Bevy font-context usage where needed.
   - Preserve font loading, supplied font sources, and system fallback.
   - Remove trivial window-specific wrappers from core.
   - Replace broad adapter re-exports with a deliberate public API and prelude.
   - Evaluate optional timing instrumentation without weakening useful renderer diagnostics.

Preserve the substantive low-level capabilities: transactional surface updates, dirty tracking, validated geometry, Unicode and wide-cell occupancy, terminal styles, cursor rendering, font measurement, useful sizing modes, bounded caches, and stable image handles.

Keep both the raw Ratatui backend and the convenient terminal wrapper. Ensure `bevy_ratatui` can own a normal `ratatui::Terminal` without adopting the wrapper.

Implement the cleanup completely, updating documentation, examples, tests, benchmarks, and migration fixtures. Update Ratty’s integration in an isolated worktree, preserving unrelated changes. Avoid replacement abstractions that merely move complexity around, speculative extension points, and unrelated cleanup.

Verify minimal-feature builds, formatting, strict Clippy, tests, documentation, consumer integration, and relevant GPU rendering/readback behavior. Add or adapt tests for meaningful behavioral risks. Distinguish pre-existing failures from regressions.

Finish with a concise report of what was removed, the resulting public API, consumer migration changes, verification results, and remaining limitations. Do not commit, push, or modify GitHub unless explicitly requested.
