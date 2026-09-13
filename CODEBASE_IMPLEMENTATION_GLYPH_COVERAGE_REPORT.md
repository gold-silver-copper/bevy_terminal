# Glyph fidelity coverage implementation

The required fallback, color, placement, and lifecycle suite passes on Metal and software Vulkan. It exposed and fixed a renderer baseline defect. **VS15 text-presentation fallback remains an upstream capability limitation**, with an explicit failing probe; it is not claimed as fixed. No commits, pushes, or GitHub changes were made.

Evidence is under `/tmp/glyph-coverage`. Exact commands, results, source/font hashes, and the requirement audit are in `CODEBASE_IMPLEMENTATION_GLYPH_COVERAGE_VERIFICATION.json`. The previous fidelity report remains a historical snapshot.

## Renderer fix

Each terminal cell is shaped independently. Bevy centers that cell's resolved font metrics within its line, so applying the same vertical translation to every cell did not establish a shared baseline. The new CJK fixture reproduced this in an otherwise roomy 14×40 cell at an 18 px font: the primary baseline was 26 px, while CJK fallback used 28 px.

The renderer now normalizes ordinary shaped runs to the primary face's snapped baseline before applying the terminal's text offset. Configured style metrics are measured relative to that same baseline. Box drawing retains its separate alignment policy. The correction is an integer translation; it does not rescale glyphs, change rasterization phase, grow cells based on fallback content, or add public APIs. It runs at existing measurement/cache boundaries. Existing bounded caches, dirty tracking, transactional surfaces, and image-handle behavior remain intact.

[CJK before, 8×](/tmp/glyph-coverage/first/cjk-1-actual.8x.png) and [after, 8×](/tmp/glyph-coverage/final-metal-coverage/roomy/cjk-1x-1-actual.8x.png) show the two-pixel correction. The initial failure is recorded in [first.log](/tmp/glyph-coverage/first.log).

## Stronger oracle and coverage

- Baseline expectations now come from raw configured font metrics, independently of captured ink and production fitting code. Fitting runs must keep their bearings or use the minimum translation that preserves their complete ink. The obsolete threshold-based text alignment scan was removed.
- Oversized runs must match a permitted unchanged crop. Focused production tests independently specify asymmetric coverage, even/odd centering, tie resolution, and multi-glyph-run placement. The oracle does not reproduce the production coverage-scoring algorithm.
- The raw reference now composes premultiplied linear RGBA from Bevy's RGBA8 sRGB atlases, then blends onto the test background. CBDT glyph colors remain intrinsic; mask glyphs retain their coverage. Full RGB comparisons still permit only one sRGB code value. A synthetic red-over-blue regression checks color preservation and linear blending.
- All 34 distinct missing sequences in the preceding matrix inventory have required fallback cases: 16 CJK/Hangul/full-width sequences and 18 Latin/Greek/combining sequences. A deliberately ASCII-only primary fixture forces fallback; required face IDs, support, anchor symbols, and continuation occupancy/style are asserted. Missing required samples cannot silently lower the checked count.
- Twelve color sequences exercise emoji, VS16, modifiers, a country flag, a keycap, and joined sequences. Each is checked on two backgrounds with a nonwhite text foreground, so tinting an intrinsic-color glyph fails. The cases require one shaped color glyph and partial alpha. Opaque cell backgrounds remain opaque, and every blank/neighbor region is checked for unwanted ink.
- Mixed configured faces exercise regular, bold, italic, and bold-italic baseline alignment. Procedural blocks must fill the cell even when fixed dimensions differ from font advance.

The three renamed subsets total 175,820 bytes. Sources are pinned in [subset-sources.json](assets/fonts/fidelity/subset-sources.json); licenses and reproduction instructions are in [the font README](assets/fonts/fidelity/README.md). Full-font versus subset comparison passed for every selected sequence at 18 and 23.25 px, checking support, glyph counts, color format, metrics, complete bounds, and raw raster pixels. The primary-family suite remains separate and still reports its original 528 unsupported sample instances; these are intentional coverage records, not successful renders. The new required fallback suite closes their test coverage without altering those families or relying on host fonts.

## Results

A fresh pre-change Metal baseline reproduced all four existing matrices with zero failures, plus the font-driven tile-only check. Working-tree hashes and a binary diff were saved before editing.

| Final check | Metal | Software Vulkan |
| --- | --- | --- |
| Width-fitted primary matrix | 144/144 groups pass | 144/144 groups pass |
| Natural 23 px primary matrix | 144/144 | 144/144 |
| Compact 23 px, line height 0.85 | 144/144 | 144/144 |
| Fixed 9×18, font 18 px | 144/144 | 144/144 |
| Required coverage/lifecycle | 192/192 cases pass | 192/192 cases pass |
| GPU readback/export regressions | 6/6 tests pass | 6/6 tests pass |

Each coverage run has 24 cases across eight stages: roomy fixed cells; scale changes retaining identical physical dimensions while logical geometry changes; fractional natural sizing; compact rows; fractional fixed dimensions; font switching; replacing wide/color content with narrow combining text; and idle rendering. Image handles remain stable, content-only changes preserve geometry, and idle pixels/counters remain unchanged. Readbacks must match current dimensions, and idle comparisons exclude GPU row padding.

Each backend verifies 1,856 text sample instances (1,020 fully preserved and 836 checked crops), plus 24 procedural-fill checks. Metal and Vulkan agree on these aggregate results. Cropping remains explicit behavior for insufficient geometry; neither arbitrary fixed widths nor fallback content can silently change the sizing contract. Arbitrary fixed widths do not promise seamless font-drawn box joins when the font advance is narrower than the cell. The tested existing joins remain intact, and procedural blocks remain cell-sized; no glyph stretching or automatic growth of explicit cells was added.

Representative inspected artifacts include [color emoji on the alternate background](/tmp/glyph-coverage/final-metal-coverage/roomy/color-alt-1x.png), [compact CJK](/tmp/glyph-coverage/final-metal-coverage/compact/cjk-1x.png), native and 8× actual/reference pairs, and full unclipped raw references. Every stage has a `results.tsv` and `failures.txt`.

## Verification and downstream consumers

All required formatting, locked workspace all-target/all-feature checks, strict Clippy, and documentation checks passed. Unit/integration tests passed: 84 core tests and 18 adapter tests; the six oracle regressions and ten doctests also passed. Default/minimal feature checks passed, including a standalone dependency with default features disabled producing a real Metal GPU readback. The full-font subset maintenance test passed with its pinned source files.

The real `bevy_ratatui` context fixture at `f1118a98ab8aaacbed8f9a03e1f278871f39e84e` passed its integration test and windowed build. Ratty was tested in the isolated worktree at `8e0c5a04d8e93c30c7b4e7a5f64dc40d1ce28df0` with a local dependency override: the rebuilt headless snapshot, all seven smoke cases, and 106 library tests passed. Smoke cases report zero stale cells. Its generated local lockfile was saved as evidence and the worktree restored to clean state; existing consumer source worktrees and PRs were preserved.

A separate full-diff review covered the renderer correction, shared oracle, both harnesses, synthetic regressions, fixture provenance, docs, and CI. It removed obsolete alignment code, made the new harness explicitly reject placements outside its allowed crop range, and corrected idle comparison/readback readiness so padding or old dimensions cannot masquerade as image defects. No confirmed in-scope renderer findings remain. CI now runs the required coverage suite and includes its diagnostic outputs in the existing artifact upload; these workflow changes have not been published or run on GitHub.

## Explicit upstream limitation and reference scope

`glyph_coverage --scale 1 --text-presentation` is an additional strict capability probe. It requires `❤︎`, `♥︎`, and `☕︎` to resolve through the configured text fallback. The raw Bevy oracle instead resolves the color-emoji font. The probe fails rather than substituting a monochrome reference or treating that selection as correct. Its evidence is separate from the required supported VS16/color suite: [Metal probe](/tmp/glyph-coverage/presentation.log) and [Vulkan probe](/tmp/glyph-coverage/vs15-vulkan.log).

The cause is upstream of terminal placement in [Parley 0.9.0 font selection](https://docs.rs/crate/parley/0.9.0/source/src/shape/mod.rs): cluster emoji classification uses the pictograph property without honoring VS15, and font selection appends the emoji generic family before script fallback. The failing face IDs come directly from raw Bevy shaping, not terminal fitting or GPU clipping. Fixing this capability requires a presentation-aware upstream selection change and dependency update; no global font-context mutation, vendored shaping fork, or second rendering engine was introduced here. VS15 fallback and untested color-font formats remain outstanding, as permitted by the prompt's unsupported-stack clause. Passing the selected CBDT fixtures does not establish every Unicode/presentation or color-font capability.

Ghostty was consulted at `7aab0a0392369613472bd5dcfd66bef58e78c3ec`, specifically `src/font/face/coretext.zig`'s conversion from baseline-relative glyph bounds to a cell baseline, alongside the previously inspected ordinary-text/symbol constraint separation. This informs the internal rule, not a claim of CoreText/Bevy pixel equivalence. No Ghostty renderer code or runtime dependency was added.
