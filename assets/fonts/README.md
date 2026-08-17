# Vendored fonts

Every family here is used only by the executable examples and visual-QA
tools. Only `jetbrains-mono` is included in the published
`bevy_terminal_ratatui` package (it is embedded with `include_bytes!` by the
examples); the other families are loaded from disk by the `render_test`
example so they can be compared without bloating the crate.

| directory | family | version | license | notable coverage (Regular face) |
|---|---|---|---|---|
| `jetbrains-mono` | JetBrains Mono | 2.304 | OFL 1.1 | box drawing, blocks, powerline; no braille, partial geometric shapes |
| `cascadia-mono` | Cascadia Mono | 2407.24 | OFL 1.1 | box drawing, blocks, braille, geometric shapes |
| `hack` | Hack | 3.003 | MIT + Bitstream Vera | box drawing, blocks, arrows, geometric shapes; no braille |
| `dejavu-sans-mono` | DejaVu Sans Mono | 2.37 | Bitstream Vera / public domain | box drawing, blocks, arrows, geometric and misc symbols; no braille |
| `iosevka-fixed` | Iosevka Fixed | 34.8.0 | OFL 1.1 | box drawing, blocks, braille, arrows, geometric shapes, powerline (largest coverage) |
| `source-code-pro` | Source Code Pro | 2.042 | OFL 1.1 | box drawing, blocks, powerline |

None of them contain CJK or color emoji; those glyphs always come from Bevy's
system-font fallback.
