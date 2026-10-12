# Vendored fonts

Fonts for the tests and examples of this repository. They are not published:
the renderer uses only the fonts an application loads.

| directory | family | version | license | used by |
|---|---|---|---|---|
| `jetbrains-mono` | JetBrains Mono (Regular, Bold, Italic, BoldItalic) | 2.304 | OFL 1.1 | examples, tests |
| `cascadia-mono` | Cascadia Mono | 2407.24 | OFL 1.1 | placement and replay tests, `render_test` |
| `hack` | Hack | 3.003 | MIT + Bitstream Vera | placement tests, `render_test` |
| `dejavu-sans-mono` | DejaVu Sans Mono | 2.37 | Bitstream Vera / public domain | GPU readback, placement and replay tests |
| `fidelity` | small subsets, see [`fidelity/README.md`](fidelity/README.md) | | OFL 1.1 | placement and shaping tests |
