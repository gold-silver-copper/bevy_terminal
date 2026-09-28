# Ghostty parity evidence

Probe results, reports and verification records for the Ghostty glyph-placement parity work.

## Glyph placement probe

`crates/bevy_terminal/src/render/batch/probe.rs` drives the real sync system headlessly and records every glyph quad of the last full scene before clipping. For each grapheme it measures:

- ink lost to a clip inside the texture;
- ink lost past the texture's edges;
- how far the ink reaches outside the grapheme's cell box;
- the horizontal shift the scene applied.

The matrix covers the six bundled families at 18 and 24 px, raster scale 1 and 2, and line height 1.0 and 0.85. Fallback fonts are bundled, so results do not depend on installed fonts. Regenerate with:

```sh
cargo test -p bevy_terminal --lib --locked probe -- --ignored
```

Output goes to `target/glyph-placement-probe/{summary,entries}.tsv`. `BEVY_TERMINAL_PROBE_FONTS="Label=/path/font.ttf;Menlo"` adds optional host fonts.

- `probe-baseline-summary.tsv`: per configuration, before any change (`main` at `9281c92`).
- `probe-baseline-defects.tsv`: the non-graphics graphemes that lost ink or were shifted at 24 px, scale 2.
- `probe-after-summary.tsv` and `probe-after-defects.tsv`: the same after this work (sprites are excluded instead of graphics). The samples gained a Nerd Fonts row, so row numbers after row 11 moved down by one.

  No grapheme loses ink to a clip inside the texture in any configuration (before: 12–157 per configuration). The remaining losses are accents on the first row and descenders on the last row, cut by the texture's edges. Every remaining horizontal shift is at the first or last column.

Other files:

- `REPORT.md` is the implementation report, and `verification.json` its machine-readable verification manifest.
- `ratty-captures/` holds enlarged before/after crops of Ratty's render test.
- `gen_nerd_font_table.py` generates `crates/bevy_terminal/src/render/batch/nerd_font.rs` from a Ghostty checkout.
- `RUN_SHAPING.md` assesses run shaping.
- `benchmarks/` holds the benchmark results.
