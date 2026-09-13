# Close the remaining glyph fidelity coverage gaps

Implement and verify the next fidelity improvements in `bevy_terminal` and `bevy_terminal_ratatui`. Do the work, rather than stopping at an audit or plan. Do not commit, push, create/update PRs, or otherwise modify GitHub unless explicitly requested for this work.

The workspace root is `bevy_terminal_ratatui`; the low-level renderer is in `crates/bevy_terminal`. Ratty consumes these libraries, and `bevy_ratatui` will migrate to the adapter. Keep the renderer low-level: applications supply cells, fonts, explicit geometry, and raster configuration; the library produces an image, validated geometry, and status. Backwards compatibility and breaking semver are not concerns.

## 1. Establish the current baseline

Read repository instructions, `CODEBASE_IMPLEMENTATION_GLYPH_FIDELITY_REPORT.md`, `CODEBASE_IMPLEMENTATION_GLYPH_FIDELITY_VERIFICATION.json`, `examples/glyph_fidelity.rs`, and `examples/common/fidelity_oracle.rs`. Inspect current source and worktree changes before editing. Preserve the existing fidelity fixes and unrelated work, including uncommitted changes.

The preceding verification recorded zero strict failures across eight Metal/software-Vulkan matrices: six font families, four scales, and width-fitted, natural, compact, and fixed sizing. Each full matrix covered 144 groups and 17,904 supported text samples, with 528 unavailable sample instances reported separately. Treat these as historical observations to reproduce, not hard-coded expectations. The unavailable count includes repeated samples across fonts/scales, not 528 unique characters.

Reproduce the current strict checks and save fresh structured results. Use separate Cargo target directories when comparing source revisions: a previous shared target directory caused an old example executable to be mistaken for the current build. Record source revisions or working-tree hashes, commands, resolved fonts, physical metrics, and backend details. Historical `/tmp` evidence may be absent; new evidence must stand on its own.

## 2. Make missing font coverage testable

Inventory every unavailable character or sequence and distinguish intentional primary-font gaps from broken fallback resolution. Add suitable, pinned, redistributable test fonts to exercise CJK, Hangul, full-width characters, and missing Latin/Greek/combining sequences. Preserve licenses and attribution; keep assets reasonably small without invalidating shaping or licensing through careless subsetting.

Keep two distinct cases:

- Primary-face tests that accurately record what each configured font supports.
- Explicit fallback tests whose required characters must resolve and render successfully. A missing required glyph must fail these tests rather than silently reduce coverage.

Disable incidental host font discovery for strict tests. Verify the actual resolved face and that the intended fallback path is exercised. Do not replace primary-font tests with a universal fallback font or change unrelated font-family selection just to obtain a green result. Keep bundled coverage assets and special collection setup in tests/examples; do not make the production renderer depend on them.

## 3. Strengthen the independent oracle

Preserve the raw, unclipped Bevy glyph-atlas reference and complete pixel comparison. Do not reintroduce a reference terminal that shares production fitting/clipping, infer font metrics from procedural blocks, or rely on ink counts alone.

Add assertions for placement contracts that unrestricted alignment could otherwise hide:

- Ordinary runs whose ink already fits retain their bearings.
- Runs that fit after an integer horizontal translation preserve all source coverage without scaling.
- Oversized runs use the documented deterministic clipping policy. Checking that some crop matches is insufficient to prove the selected crop is correct; add independent expectations for asymmetric coverage and tie cases.
- Ordinary text uses a stable baseline policy across styles and fallback faces. Test mixed-face runs and combining sequences; distinguish typographic baseline alignment from aligning ink tops. Do not derive every expected vertical position solely from the same rendered output being checked.
- Wide anchors and continuation cells retain coherent occupancy, background, style, and clipping. Verify that neighboring cells and guard regions are not painted unexpectedly.

Use explicit synthetic fixtures with independently derived expected results where they clarify a rule. Add real-font GPU cases for confirmed defects. Do not duplicate the production fitter inside the oracle, fit individual letters vertically to make them pass, or add font-specific exemptions. Preserve the existing one-sRGB-code-value tolerance unless a separately demonstrated conversion requirement justifies a different, narrowly scoped rule.

## 4. Add color-glyph fidelity coverage

Extend the oracle to handle color glyph atlas data as well as alpha masks. Confirm the actual atlas format, channel order, color space, alpha representation, and GPU blend behavior before defining reference composition. Preserve original glyph colors and compare full pixels over contrasting backgrounds; do not reduce color glyphs to white silhouettes.

Use deterministic bundled assets and explicit supported sequences to cover color emoji, variation selectors, joined emoji, modifiers, wide spans, and transparency. Verify shaping and occupancy separately from raster fidelity. Do not assume every selected font supports every emoji sequence or every color-font format.

Where the underlying Bevy/font stack does not support a requested format or sequence, provide evidence, separate that capability limitation from a library defect, and leave the coverage explicitly outstanding. Do not silently substitute a monochrome glyph and claim color coverage. Keep scope to the existing rendering stack; do not add a second shaping/rasterization engine merely to make a test pass.

## 5. Expand geometry and lifecycle coverage

Keep the existing six-family, four-scale matrices passing. Add representative cases for fractional font sizes and raster scales around physical-pixel rounding boundaries, mixed configured faces with differing metrics, natural line heights, compact rows, and explicit fixed cells whose dimensions differ from the chosen font's advance and line box.

Test changes in configuration as well as static snapshots: switching fonts, changing scale while snapped physical dimensions remain equal, changing line height, and updating text containing fallback/wide/combining runs. Verify correct invalidation, stable image handles where promised, updated logical geometry, no stale pixels, and no content-dependent cell measurement.

Define expected behavior before changing assertions. Fixed or compact cells may intentionally crop; they must still preserve the documented placement, occupancy, and crop policy. Arbitrary fixed widths do not currently promise seamless font-drawn box lines when the advance is narrower than the cell. Characterize this explicitly rather than stretching ordinary text, growing explicit cells, broadly weakening seam checks, or introducing a new presentation/fitting API. Procedural blocks must remain cell-sized, and existing supported box-drawing joins must remain intact.

## 6. Make only demonstrated production fixes

For every newly exposed defect, save a reproducer, establish the root cause, add a regression, and implement the smallest coherent internal fix. Preserve transactional surfaces, Unicode occupancy, styles, cursors, font loading/fallback, bounded caches, dirty tracking, and persistent output validation.

Avoid per-frame font scans, redundant shaping, unbounded caches, application/window inference, library-owned UI/3D presentation, new public APIs without a demonstrated necessity, and duplicated fitting implementations. Prefer improving the harness when the problem is missing evidence. Missing test-font coverage alone is not justification for changing renderer behavior.

Ghostty/ghostty-vt may be used as a reference. Inspect actual source and pin the revision. Trace pixel-placement behavior into Ghostty's font/rendering code rather than treating VT semantics as a raster oracle. Compare equivalent fonts, styles, physical sizes, and sizing policies; document intentional differences. Do not add Ghostty as a runtime dependency or copy its architecture wholesale. Respect licenses for any reused assets or code.

## 7. Verify and report

Run targeted regressions first, then required formatting, locked workspace all-target/all-feature checking, strict Clippy, unit/integration tests, doctests, and documentation with warnings denied. Verify affected default/minimal feature configurations; use an isolated minimal consumer when dev-feature unification would otherwise obscure the result.

Run the GPU readback regressions and the existing fidelity matrices, including full font-driven text checks rather than only tiles:

```sh
cargo run --all-features --locked --example glyph_fidelity -- --check --font all --scale all
cargo run --all-features --locked --example glyph_fidelity -- --check --font all --scale all --from-font 23
cargo run --all-features --locked --example glyph_fidelity -- --check --font all --scale all --from-font 23 --line-height 0.85
cargo run --all-features --locked --example glyph_fidelity -- --check --font all --scale all --fixed-cell 9x18 --font-size 18
```

Run new required fallback, color, placement, and geometry cases on Metal and software Vulkan where available. If either backend or a required capability cannot run, state the exact limitation and leave that verification outstanding. Inspect representative native and enlarged captures, including failures and deliberately clipped cases.

Rebuild and run Ratty's existing headless smoke cases against the changed library in an isolated worktree with a local dependency override. Preserve unrelated consumer changes and existing PR state. Re-run the real `bevy_ratatui` context fixture when geometry or backend behavior changes.

Add deterministic, bounded new coverage to CI after validating its oracle. Required fallback/color cases must fail on missing coverage. Keep intentional primary-font coverage gaps separate, and make aggregate reports distinguish required successes, unexpected missing glyphs, capability limits, expected clipping, and rendering failures. Upload useful diagnostics on failure without depending on host fonts or unpinned downloads.

Perform a separate final full-diff review and validate any findings against current source. Fix confirmed in-scope defects and rerun affected checks. Save a concise implementation report and machine-readable verification manifest containing before/after coverage, root causes, harness versus renderer changes, exact commands/results, source/font provenance, representative artifacts, and remaining limitations.

Completion means the agreed required cases pass, intentional clipping is explicitly asserted, and every newly confirmed defect is fixed. Do not claim that passing these cases proves every possible font, Unicode sequence, or geometry works. Do not declare completion with unresolved required failures or disguise unsupported required cases as successful checks. Publication and updating Ratty's PR remain separate tasks requiring explicit authorization.
