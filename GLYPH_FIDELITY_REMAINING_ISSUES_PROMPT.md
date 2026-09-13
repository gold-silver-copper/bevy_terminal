# Finish fidelity verification and fix the remaining issues

Audit and fix the remaining demonstrated fidelity issues in `bevy_terminal` and `bevy_terminal_ratatui`. Implement and verify the changes, rather than stopping at a plan. Do not commit, push, comment on GitHub, or create/update PRs for this task.

Keep `bevy_terminal` low level. Applications own terminal state, layout, and presentation; the renderer consumes cells, fonts, and explicit raster geometry. Ratty is a consumer, and `bevy_ratatui` will migrate to the adapter. Backwards compatibility and semver breaks are not concerns, but prefer small coherent internal changes over new APIs or dependencies.

## Establish what is actually unfinished

Read repository instructions, `GLYPH_FIDELITY_COVERAGE_PROMPT.md`, both glyph implementation reports, available verification manifests, the current diff, and the fidelity harnesses. Preserve existing uncommitted fixes and unrelated work. Treat reports as claims to verify against source and fresh evidence, not proof by themselves.

The coverage report currently describes passing primary matrices and fallback/color/lifecycle checks, a corrected fallback baseline, and an outstanding VS15 text-presentation fallback problem. Confirm these observations. Do not assume the historical 66 failures still exist, confuse intentionally unsupported primary-font samples with renderer failures, or claim that the remaining verification manifest exists without checking it.

Complete missing verification/report artifacts from the preceding work. Record exact commands, exit codes, source hashes, pinned font provenance, backend details, and diagnostic paths. Temporary evidence may be absent; regenerate anything necessary. Correct unsupported claims in the reports.

## Diagnose and fix text-presentation selection

Reproduce the strict probe:

```sh
cargo run --all-features --locked --example glyph_coverage -- --scale 1 --text-presentation
```

Trace `❤︎`, `♥︎`, and `☕︎` through the configured font collection, shaping, resolved face, raw atlas, and terminal output. Establish whether the defect is in library configuration, Bevy, Parley, or another dependency. Verify the reported Parley 0.9.0 diagnosis in actual source, including whether VS15 is ignored when selecting emoji fallback. Distinguish presentation selection from missing font support or raster-placement errors.

Check the current upstream implementation and any relevant fixes before choosing a solution. Prefer a verified upstream fix and compatible dependency update. If a narrowly scoped configuration or library correction can honor presentation semantics without distorting other fallback behavior, demonstrate and test it. Do not force a text font for all emoji, strip selectors, mutate shared font state per cell, introduce a second shaping engine, or silently accept the wrong face.

If the fix belongs upstream and cannot be consumed coherently in this workspace, prepare a minimal reproducer and a concrete proposed upstream patch locally. Explain the exact dependency blocker and remaining integration work. Do not publish it without authorization or declare the capability fixed merely because a separate patch exists.

Once supported, make text-presentation cases required in the normal coverage suite and CI. Retain explicit VS16 color counterparts, joined emoji, modifiers, flags, keycaps, combining sequences, and ordinary fallback text. Verify actual resolved faces and full raster output. Missing required glyphs or incorrect presentation must fail.

## Use Ghostty appropriately

You may use Ghostty and ghostty-vt as references. Inspect actual source and record the revision. Use VT code for terminal/Unicode semantics and trace font selection and pixel placement into Ghostty's font/rendering code. Do not treat VT state as a raster oracle or require pixel equality between different rasterization engines. Compare equivalent fonts, physical sizes, styles, and geometry; document intentional differences. Respect licenses and do not add Ghostty as a runtime dependency.

## Preserve and strengthen the contracts

Keep the independent raw, unclipped Bevy atlas oracle. Preserve glyph colors, linear blending, the one-sRGB-code-value tolerance, shared typographic baselines, ordinary bearings, deterministic oversized clipping, wide-cell occupancy, and guard-region checks. Do not derive expected placement from the captured pixels being tested or duplicate production fitting logic in the oracle.

For each newly confirmed defect, retain a minimal failing regression, establish the root cause, and implement the smallest coherent fix. Check style and fallback metrics, fractional rounding, compact/fixed cells, font switches, same-physical-size scale changes, stale pixels, stable image handles, and idle behavior. Keep caches bounded and measurement independent of cell content. Avoid adding production complexity merely to expand test-font coverage.

Maintain separate reporting for required successes, intentional clipping, primary-font coverage gaps, unsupported stack capabilities, and rendering failures. Do not lower coverage, add font-specific exemptions, widen tolerances, or remove failing cases to obtain a green result.

## Verify the complete result

Run targeted regressions first, then repository-required formatting, locked workspace all-target/all-feature checks, strict Clippy, tests, doctests, and documentation with warnings denied. Verify default and minimal feature configurations and the independent oracle regressions.

Run GPU readback tests, required coverage/lifecycle cases, and all four complete primary matrices on Metal and software Vulkan where available:

```sh
cargo run --all-features --locked --example glyph_fidelity -- --check --font all --scale all
cargo run --all-features --locked --example glyph_fidelity -- --check --font all --scale all --from-font 23
cargo run --all-features --locked --example glyph_fidelity -- --check --font all --scale all --from-font 23 --line-height 0.85
cargo run --all-features --locked --example glyph_fidelity -- --check --font all --scale all --fixed-cell 9x18 --font-size 18
cargo run --all-features --locked --example glyph_coverage
```

Inspect representative native and enlarged images, including text/color presentation pairs, fallback baselines, and deliberate crops. Rebuild Ratty's headless consumer in an isolated worktree using local dependency overrides, run its smoke tests, and rerun the real `bevy_ratatui` context fixture when affected. Preserve consumer worktrees and PR state. Use isolated build directories when comparing revisions so stale binaries cannot invalidate evidence.

Perform a separate full-diff review, validate findings against current code, fix confirmed in-scope problems, and rerun affected checks. Do not broaden into unrelated cleanup or claim remote CI passed when only local checks ran.

Save a concise implementation report and machine-readable verification manifest. Include before/after failures, renderer versus harness versus upstream changes, exact verification results, source/font provenance, inspected artifacts, and a requirement-by-requirement completion audit. State every remaining blocker explicitly. Completion requires all agreed required cases to pass; passing a finite suite does not prove support for every font, Unicode sequence, or geometry.
