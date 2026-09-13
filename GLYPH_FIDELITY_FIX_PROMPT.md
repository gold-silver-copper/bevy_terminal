Fix the glyph fidelity failures in `bevy_terminal` by correcting the test oracle and then repairing confirmed rendering defects.

The workspace root is `bevy_terminal_ratatui`; the low-level renderer is in `crates/bevy_terminal`. Ratty consumes these libraries, and `bevy_ratatui` will migrate to the adapter. Preserve the low-level boundary: terminal cells and explicit rendering configuration produce an image, validated geometry, and status. Applications own presentation and DPI policy. Backwards compatibility and breaking semver are not concerns, but this work should not require new public APIs or presentation features.

Implement and verify the fixes completely. Do not merely write an audit or make the existing failure count disappear. Do not commit, push, or modify GitHub unless explicitly requested for this work.

## 1. Reproduce and classify the baseline

Inspect current code and repository instructions before editing. Run:

```sh
cargo run --example glyph_fidelity -- --check --font all --scale all
cargo run --example glyph_fidelity -- --check --font all --scale all --from-font 23.0 --tiles-only
```

The previously recorded first command had 66 failing font/scale/group combinations: 24 ASCII, 23 Latin, and 19 Greek/Cyrillic. These were text-fidelity failures, not tile-seam failures. The font-driven tile run passed all 24 groups. Treat these as historical observations to reproduce, not expected results to hard-code. The first command uses `FitCellWidth`; do not confuse its preset cell dimensions with `TerminalSizing::Fixed`.

Save structured results and representative primary/reference captures. Distinguish actual lost ink, placement differences, reference errors, genuinely oversized glyphs, and unsupported or fallback font coverage. Record the resolved font face and physical metrics when needed to make a diagnosis reproducible. Historical `/tmp` logs may be absent; fresh evidence must be sufficient.

Use Ghostty/ghostty-vt as a reference where useful; the user explicitly authorizes this. Inspect the actual source and pin the revision used. Distinguish VT semantics (cell width, continuation cells, combining sequences, and styles) from font measurement, glyph placement, clipping, and GPU rendering. If the relevant behavior is outside ghostty-vt, trace it into Ghostty's font/rendering implementation rather than treating a VT-only result as a pixel-rendering oracle. Compare equivalent fonts, sizes, styles, scales, and sizing policies, and document intentional differences. Use this reference to inform small internal rules and targeted tests; do not add Ghostty as a runtime dependency or copy its architecture wholesale. Record source paths/revisions and respect applicable licensing if reusing code.

## 2. Repair the harness reference calculation

Inspect `examples/glyph_fidelity.rs`, particularly `run_checks`, `block_rows`, reference-terminal construction, and ink comparison.

The harness currently infers the font's line box from the rendered full-block character. The renderer draws block elements procedurally to fill the cell, so that output is not an independent measurement of font line metrics. Validate this mismatch against current code and replace the invalid inference.

Establish a reference using actual resolved font metrics and rasterized glyph bounds. Primary and reference samples must use matching font faces, physical font size, hinting, shaping, and rasterization phase. Account explicitly for baseline and coordinate differences when reference cells are larger. Prove that the reference itself is not clipped. Avoid changing shaping or fitting policy merely by changing reference-cell dimensions without accounting for that difference.

Specify what the comparison proves. Ink-pixel counts alone cannot prove that the same pixels, coverage, and shape survived. Compare appropriately aligned coverage or image regions where exact preservation is required, and explain any justified tolerance. Keep the oracle sufficiently independent that it cannot hide the production placement defect it is intended to detect.

Keep separate assertions for ordinary text fidelity, cell occupancy/clipping, and procedural block or box-drawing continuity. Genuinely oversized ink and intentionally compact line heights require an explicit documented expectation, not a blanket no-clipping promise. Do not broadly suppress failures, weaken tolerances, or add font-specific exemptions to obtain green results.

## 3. Fix confirmed horizontal placement defects

Inspect `fit_horizontally`, `trim_overshoot`, glyph ink bounds, snapping, and clipping in `crates/bevy_terminal/src/render/batch/scene.rs` and `shaping.rs`.

The faint-column overshoot heuristic exists to preserve box-drawing alignment but currently also applies to ordinary text. Investigate whether it unnecessarily discards ordinary glyph pixels that could fit through an integer horizontal translation. A recorded example is Cascadia Mono's `W`: an 11-pixel-wide reference glyph in an 11-pixel-wide cell was drawn in only 10 columns. Reproduce the exact face, scale, and placement before treating the heuristic as the confirmed cause.

Preserve all ink of an ordinary glyph run when it fits inside its allocated cell span. Preserve ordinary bearings when they already fit. Maintain box-drawing joins and procedural block geometry through a deliberate internal policy appropriate to those symbols. Keep combining sequences and wide-cell spans coherent. Retain deterministic, documented clipping for genuinely oversized runs; do not stretch ordinary glyph bitmaps or allow uncontrolled painting into neighboring cells.

Add focused regression coverage for the confirmed defect and for box-drawing alignment that the original heuristic was meant to protect.

## 4. Fix confirmed vertical placement defects

After correcting the oracle, investigate the remaining top/bottom clipping in accented characters, ascenders, descenders, and styled faces. Inspect font metrics, probe measurements, `vertical_offset`, physical pixel rounding, and final glyph/UV clipping in `metrics.rs`, `shaping.rs`, and `scene.rs`.

Use a consistent baseline across ordinary text. Do not move individual letters vertically just to fit a test. Check all configured faces and the supported sizing modes: `FromFont`, `FitCellWidth`, and explicit `Fixed` geometry. Respect fixed dimensions and intentionally compact line heights. Do not grow cells based on the current text content or introduce unstable geometry when content changes.

Resolve avoidable clipping where the intended font metrics and cell geometry provide sufficient room. Clearly separate that from ink that cannot fit under the requested sizing policy. Test fractional raster scales and cases where snapped physical dimensions remain equal while logical metrics change.

## 5. Preserve behavior and keep the implementation small

Keep transactional surfaces, dirty tracking, Unicode/wide-cell occupancy, styles, cursor rendering, font loading/fallback, bounded caches, stable image handles, and validated persistent output intact. Avoid per-frame font scans, repeated shaping, new unbounded caches, or content-dependent cell measurement.

Do not reintroduce library-owned UI/3D presentation, window inference, readiness events, speculative extension points, or duplicated fitting implementations. Prefer a small number of explicit internal rules supported by regression tests. Update documentation where the actual sizing or clipping contract needs clarification.

## 6. Verify the corrected contract

Add meaningful tests for oracle correctness and the confirmed placement defects. Cover regular, bold, italic, and bold-italic text; ASCII, accented Latin, Greek/Cyrillic, combining sequences, and wide/fallback glyphs; and raster scales 1, 1.5, 2, and 3. Keep strict block and line continuity checks passing. Include representative sizing and line-height cases relevant to Ratty.

Run formatting, locked workspace all-target/all-feature checking, strict Clippy, unit/integration tests, doctests, and documentation with warnings denied. Verify minimal/default feature configurations where affected. Run the GPU readback regressions and both fidelity commands above, plus broader font-driven text checks so a tile-only pass is not presented as proof of text fidelity.

Inspect representative rendered images at native resolution and enlarged views. Verify Ratty's existing headless smoke captures against the changed library using an isolated worktree/local dependency override if necessary; preserve unrelated consumer changes. Re-run the real `bevy_ratatui` context fixture when shared geometry or backend behavior changes.

Exercise Metal and software Vulkan when available. If a backend cannot run in the available environment, identify the exact limitation and leave that verification explicitly outstanding rather than claiming cross-backend success. Add the corrected deterministic strict fidelity checks to CI once their oracle is validated. Use bundled fonts and explicit configuration for strict cases; do not make CI depend on incidental host fallback fonts.

Finish with a concise report of confirmed root causes, renderer fixes versus harness fixes, before/after results, representative artifacts, verification commands/results, and remaining limitations. Every remaining strict failure must be resolved or reported as unfinished work. Expected clipping must have a defensible contract and dedicated assertions; do not rename defects as expected behavior to declare completion.
