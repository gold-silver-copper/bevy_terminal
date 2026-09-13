# Glyph fidelity fixes

The renderer and reference harness are fixed. No commits, pushes, or GitHub changes were made. Verification artifacts are under `/tmp/glyph-fidelity-fix`; machine-readable results and hashes are in `CODEBASE_IMPLEMENTATION_GLYPH_FIDELITY_VERIFICATION.json`.

## Confirmed defects and changes

- **Invalid reference:** the old harness treated a procedurally filled full-block cell as a measurement of the font line box. Its second terminal also applied terminal fitting and clipping. The new reference composes complete raw Bevy glyph-atlas rectangles, including negative bearings, and reads resolved ascent/descent and baseline directly. It matches physical font size, faces, hinting, and rasterization phase without using terminal measurement/fitting/clipping functions.
- **Lost horizontal ink:** Cascadia Mono italic `W`, font 18.77 px at 1×, has 11 ink columns at x=1..12 in an 11 px cell. The box-drawing overshoot heuristic discarded the last faint column. Ordinary text now translates to preserve every column when the run fits; only single box-drawing characters use the alignment-preserving overshoot allowance. Existing oversized-run coverage selection remains intact.
- **Avoidable vertical clipping:** the old block/core/accent probes did not enclose the configured faces' typographic metrics in whole pixels. Cascadia's baseline 17, ascent 17.42, and descent 4.40 require a 23 px enclosure, rather than the old 22 px row. Natural and width-fitted sizing now measure all four configured styles and apply a uniform text offset; box drawing keeps its separate grid alignment. Fixed dimensions and compact line heights remain explicit cropping requests.
- **Reference background and coverage:** wide-cell continuations inherit their anchor's style. The oracle now uses that background across the whole span. Strict runs disable host font discovery and register a bundled monochrome Noto Emoji fallback. Missing glyphs are recorded separately; wide cases must exercise at least five supported symbols.

Full comparisons check every RGB pixel, with one sRGB code value allowed for CPU/GPU conversion rounding. An ink threshold only locates alignment; it is not the fidelity assertion. Bounds checks detect missing source pixels even when every remaining pixel matches. Oversized/out-of-metric ink and compact cells must match an unchanged crop at the same vertical translation as preserved text. Combining sequences and wide spans are compared as whole runs. Procedural half-block joins are now strict too.

Production changes are confined to three internal renderer modules. There are no new public APIs, presentation features, runtime dependencies, or content-dependent measurements. The extra face measurements run at the existing configuration/font invalidation boundary, not every frame. Existing cache, dirty-row, stable-image, transactional-surface, and geometry regression tests pass; no performance benchmark claim is made.

## Before and after

Baseline revision: `12b035ceb0b317ce8c529485b80c6b0317cf67ec`.

| Check | Before | After |
| --- | --- | --- |
| Historical width-fitted strict text groups | 66 failures reproduced: 24 ASCII, 23 Latin, 19 Greek/Cyrillic | No strict failures with the corrected oracle |
| Font-driven 23 px tile-only matrix | 24/24 pass | 24/24 pass |
| Corrected oracle against old Cascadia renderer, 1× | All five text groups fail; tiles pass | All six groups pass |
| Width-fitted full matrix | Old oracle is not a valid direct pixel baseline | 144/144 groups pass on each backend |
| Natural 23 px full matrix | — | 144/144 on each backend |
| Compact 23 px, line height 0.85 | — | 144/144 on each backend |
| Fixed 9×18 cells, 18 px font | — | 144/144 on each backend |

Each full matrix covers six families, four scales (1, 1.5, 2, 3), five text groups, and a tile group: **17,904 supported text samples** plus tiles. ASCII exercises regular, bold, italic, and bold-italic faces. The expanded oracle also makes combining marks and supported wide/fallback runs strict. The old 66 failures therefore must not be described as 66 independently confirmed renderer bugs.

Metal and software Vulkan agree on aggregate results, including verified crop counts: 1,151 width-fitted, 1,098 natural, 4,532 compact, and 9,537 fixed per matrix. These crops are checked for preserved pixel values and uniform vertical placement, not silently exempted.

## Representative evidence

Fresh baseline logs and classification: [baseline JSON](/tmp/glyph-fidelity-fix/baseline.json), [width-fitted baseline log](/tmp/glyph-fidelity-fix/baseline-fixed.log), [font-driven tile baseline](/tmp/glyph-fidelity-fix/baseline-from-font.log).

The before images use the corrected harness with unchanged production code from the baseline revision in an isolated worktree. This demonstrates that the new oracle rejects the original defects:

- Cascadia italic W: [before, 8×](/tmp/glyph-fidelity-fix/before-cascadia/cascadia-mono/1x/actual-4-56.8x.png), [after, 8×](/tmp/glyph-fidelity-fix/strict-fit/cascadia-mono/1x/actual-4-56.8x.png), [unclipped reference, 8×](/tmp/glyph-fidelity-fix/strict-fit/cascadia-mono/1x/reference-4-56.8x.png).
- [Natural full capture](/tmp/glyph-fidelity-fix/metal-natural/cascadia-mono/1x/actual.png), [fixed full capture](/tmp/glyph-fidelity-fix/metal-fixed/cascadia-mono/1x/actual.png).
- Ratty: [compact text](/tmp/glyph-fidelity-fix/ratty-smoke/font-compact.png), [inline document graphics](/tmp/glyph-fidelity-fix/ratty-smoke/document-inline.png), [smoke results](/tmp/glyph-fidelity-fix/ratty-smoke/results.json).

Native captures and enlarged glyph details were inspected. Each matrix directory contains `results.tsv`, `diagnostics.tsv`, full captures, and selected native/8× reference/actual glyph pairs. Failures record resolved font blob IDs/index and physical metrics; the verification manifest records bundled font file hashes. Blob IDs are process-local identifiers, not portable font names.

## Verification

All commands below passed. Cargo checks use locked dependencies.

- `cargo fmt --all -- --check`; `git diff --check`.
- `cargo check --workspace --all-features --all-targets --locked`.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`.
- `cargo test --workspace --all-features --lib --tests --locked`: 83 core and 18 adapter tests. GPU tests are separately run rather than counted as passed while ignored.
- `cargo test --all-features --example glyph_fidelity --locked`: five oracle tests, including changed shapes with equal ink counts, faint pixels, negative bearings, shared crop translation, one-code-value tolerance, and raw font metrics independent of procedural blocks.
- `cargo test --workspace --all-features --test gpu_readback --test export_readiness --locked -- --ignored --test-threads=1`: six tests on Metal and six on software Vulkan.
- `cargo test --workspace --all-features --doc --locked`: ten doctests.
- `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps --locked`.
- Core default/no-default-feature all-target checks; additionally a standalone dependency with `default-features = false` produced a real Metal GPU readback, avoiding dev-feature unification as the only minimal-feature evidence.
- The two requested fidelity commands, plus full natural, compact, and fixed matrices described above. Vulkan used Rust 1.95.0 and Mesa 22.3.6 llvmpipe in a Debian container; macOS used Rust 1.96.0-nightly and Metal. Exact invocations are in the manifest and `/tmp/glyph-fidelity-fix/*matrix.json`.
- Real `bevy_ratatui` pinned context fixture: one integration test and the `consumer-windowed` build passed.
- Isolated Ratty worktree at `8e0c5a04d8e93c30c7b4e7a5f64dc40d1ce28df0`, locally patched to this library: headless build, seven smoke cases with zero stale cells, and 106 library tests passed. Existing consumer source worktrees and PR were preserved.

The existing snapped-geometry regression also passes: changing scale from 1 to 1.001 can change logical metrics while keeping physical dimensions identical. A separate full-diff review covered production changes, the new oracle, regression tests, font provenance, documentation, and CI; obsolete comments and unused ink-count machinery were removed. No confirmed correctness findings remain.

CI now runs the oracle regressions and four deterministic full matrices, then uploads diagnostics even on failure. These workflow changes are local and have not run on GitHub. A late Metal rerun initially executed the old baseline example binary because the temporary worktree shared its Cargo target directory. That result is preserved in `fit-final.log`; explicitly rebuilding the current renderer and example resolves the artifact collision. Final current-source evidence is `fit-final-rebuilt/`. The other Metal/Vulkan matrices preceded the baseline build or used a separate Linux target volume.

## Ghostty reference and remaining limits

Reference revision: [`7aab0a0392369613472bd5dcfd66bef58e78c3ec`](https://github.com/ghostty-org/ghostty/tree/7aab0a0392369613472bd5dcfd66bef58e78c3ec).

- [`src/renderer/generic.zig`](https://github.com/ghostty-org/ghostty/blob/7aab0a0392369613472bd5dcfd66bef58e78c3ec/src/renderer/generic.zig): ordinary glyphs use no fitting constraint; designated symbols can be fitted.
- [`src/font/Glyph.zig`](https://github.com/ghostty-org/ghostty/blob/7aab0a0392369613472bd5dcfd66bef58e78c3ec/src/font/Glyph.zig), [`src/font/face/coretext.zig`](https://github.com/ghostty-org/ghostty/blob/7aab0a0392369613472bd5dcfd66bef58e78c3ec/src/font/face/coretext.zig), and [`src/font/SharedGrid.zig`](https://github.com/ghostty-org/ghostty/blob/7aab0a0392369613472bd5dcfd66bef58e78c3ec/src/font/SharedGrid.zig): explicit constraints, baseline/bearing placement, and emoji fitting inform the separation of policies.

VT cell occupancy and continuation semantics alone cannot establish pixel fidelity. Relevant placement behavior is in Ghostty's renderer/font code, outside ghostty-vt. This change does not claim CoreText/Bevy pixel equivalence: bevy_terminal intentionally translates fitting ordinary runs and clips oversized runs to their allocated spans. No Ghostty renderer code or architecture was copied and no runtime integration was added. The unmodified Noto Emoji test font comes from `src/font/res/NotoEmoji-Regular.ttf`; its OFL-1.1 license and attribution accompany the asset.

The deterministic font set still lacks the CJK/Hangul/full-width samples and some family-specific Latin, Greek, and combining coverage. Each matrix reports 528 unavailable sample instances separately; these are not claimed as successful glyph renders. Five supported wide emoji sequences per family provide actual fallback/continuation coverage. Color emoji raster comparison is outside this monochrome oracle. Explicit fixed cells can intentionally clip substantially, and arbitrary cell widths do not promise seamless font-drawn box lines when the chosen font advance is narrower than the cell. Tested fixed geometry is 9×18 with an 18 px font; procedural blocks remain cell-sized.
