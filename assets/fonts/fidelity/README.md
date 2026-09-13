# Deterministic glyph fidelity fallback

`NotoEmoji-Regular.ttf` is an unmodified monochrome Noto Emoji font from
Ghostty `7aab0a0392369613472bd5dcfd66bef58e78c3ec`,
`src/font/res/NotoEmoji-Regular.ttf`. Ghostty records Copyright 2013 Google LLC
and OFL-1.1 for this font; the accompanying license is in `OFL.txt`.

The fidelity harness uses it for reproducible wide emoji/fallback coverage
without host fonts. It is a test/example asset, not a renderer dependency.

## Required coverage subsets

`glyph_coverage` uses the following renamed subsets. Full source files are
revision/hash pinned in `subset-sources.json`; generated hashes and sizes are
in `subset-hashes.json`. Together the subsets occupy about 172 KiB.

| Fixture | Source | License |
| --- | --- | --- |
| FidelityASCII.ttf | Bundled Cascadia Mono regular, with ASCII and the block measurement glyph retained | [Cascadia OFL](../cascadia-mono/LICENSE), Copyright Microsoft Corporation |
| FidelityCJK.otf | Noto Sans CJK SC regular at `f8d157532fbfaeda587e826d4cd5b21a49186f7c` | [OFL](Noto-OFL.txt), Copyright 2014–2021 Adobe |
| FidelityColorEmoji.ttf | Noto Color Emoji at `8998f5dd683424a73e2314a8c1f1e359c19e8742` | [OFL](Noto-OFL.txt), Copyright 2022 Google Inc. |

The ASCII fixture forces non-ASCII script/emoji samples through explicit
fallbacks. The CJK fixture retains the CJK, Hangul, and full-width samples.
The CBDT/CBLC color fixture retains selected emoji, variation selectors,
modifiers, flags, and joined sequences, including the full layout closure.
The existing DejaVu Sans Mono asset supplies required Latin/Greek/combining
fallback coverage. Original copyright and license name-table records are kept;
modified fonts use distinct family and PostScript names.

To regenerate, install `fonttools==4.63.0`, then run:

```sh
python3 assets/fonts/fidelity/build_subsets.py /tmp/fidelity-font-sources
FIDELITY_FONT_SOURCES=/tmp/fidelity-font-sources cargo test --all-features --locked --example glyph_coverage -- --ignored
```

The maintenance-only test compares each selected sequence against its full
source font at 18 and 23.25 px: support, glyph count, color format, baseline,
ascent/descent, complete bounds, and raw composed raster pixels must agree.
CI uses the checked-in subsets and makes no font downloads. This is coverage
of these fixtures and the CBDT color path, not every color-font format.
