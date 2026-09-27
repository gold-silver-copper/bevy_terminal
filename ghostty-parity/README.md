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
