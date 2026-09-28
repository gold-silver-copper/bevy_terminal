# Ratty render test crops

Enlarged crops (nearest-neighbour) of Ratty's `widget/examples/render_test.rs` sample screen, captured with Ratty's `headless_snapshot` at 104×32 cells and raster scale 1.

- `*-before.png`: crates.io `bevy_terminal_ratatui` 0.7.7 (`main` at `9281c92`).
- `*-after.png`: this branch, through `[patch.crates-io]` in an isolated Ratty worktree.

| Crop | Configuration | Zoom |
| --- | --- | --- |
| `marks` | JetBrainsMono Nerd Font Mono 18 pt: precomposed, decomposed and stacked marks | 4× |
| `scripts` | same: Arabic, Hebrew, Devanagari, Thai | 3× |
| `emoji-pua` | same: emoji, symbols, Powerline and Nerd Font icons | 3× |
| `blocks` | same: eighths, halves, shades, quadrants | 3× |
| `braille` | same: Braille | 3× |
| `box` | same: light, heavy, double and mixed box drawing | 2× |
| `faces` | same: font faces and decorations | 3× |
| `compact-pua` | DejaVu Sans Mono 14 pt, line height 0.85: emoji, Powerline, scripts | 3× |
| `compact-marks` | same: marks and ZWJ sequences | 4× |

See `../REPORT.md` for what changed and why.
