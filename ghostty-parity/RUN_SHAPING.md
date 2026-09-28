# Run shaping: assessment and design

**Status:** assessed, not implemented. The renderer still shapes one grapheme per cell. This note records what was verified, a design that fits Bevy 0.19 and Parley 0.9, and what remains before it could land.

## What Ghostty does

Ghostty shapes runs of cells that share a font and style (`font/shaper/run.zig`, HarfBuzz or CoreText). It then places every glyph at its cell by cluster, plus the shaper's x offset (`renderer/generic.zig`, `addGlyph`). This gives:

- programming-font ligatures (`->`, `!=`), which `font-feature = -calt, -liga` can turn off;
- contextual forms, e.g. Arabic joining;
- shaper mark positioning across cells.

Ghostty keeps logical order; it does not reorder bidirectional text.

## What was verified here

A spike shaped JetBrains Mono through Bevy's `TextPipeline` at 40 px (not committed; the code is described here):

| Input | Glyphs (`section_index`, position, atlas size) |
| --- | --- |
| `"->"` as one section | `(0, x 0, empty spacer)`, `(0, x 24, 42 × 24 arrow)` |
| `"-"`, `">"` as two sections | `(0, x 0, empty spacer)`, `(1, x 24, 42 × 24 arrow)` |
| `"-"` alone, then `">"` alone | a 14 × 4 hyphen; a 18 × 22 greater-than |

So:

- Parley shapes across Bevy sections that share a font; the `calt` ligature formed.
- Each glyph reports the section it came from.
- Making each cell its own section therefore maps glyphs back to cells, with no cluster API needed.

Bevy's `PositionedGlyph` (bevy_text 0.19.1, `glyph.rs`) has only `position`, `atlas_info`, `section_index` and `line_index`, with no byte or cluster index. Per-cell sections are therefore the only mapping available.

## Design

1. **Runs.** Split each row into runs of consecutive cells with the same resolved face (style flags that select a face) and no sprite, block element or blank break. Each cell is one section of one Bevy text block.
2. **Placement.** For each glyph, find its cell by `section_index`. Place it at that cell's origin plus the glyph's offset from its cluster's own pen position, as in Ghostty's `x_offset`. Ignoring the shaped pen position keeps the grid exact, prevents drift, and keeps logical order even where Parley reorders RTL text visually. Contextual (joined) forms are still the shaped ones.
3. **Ligatures spanning cells.** The spacer glyph stays in its cell and the wide glyph is drawn from its own cell leftward, as the fonts design them. This already works with overflow, since ink may cover neighbouring cells.
4. **Cache.** Key shaped runs by (face, run text with cell boundaries, column spans, and whether each symbol's neighbour is blank). Keep the current per-grapheme cache as the fast path for runs of one cell and for ASCII without ligature-forming sequences. A row edit reshapes only the runs it touches. Rows remain the repaint unit, so the partial-repaint machinery is unaffected.
5. **Opt-out.** Add a `TerminalRenderConfig` field listing OpenType features to disable, like Ghostty's `font-feature`, forwarded to Bevy's `TextFont::font_features`.
6. **Constraints.** Symbols, emoji and Nerd Fonts icons remain single-cell runs, so their constraint logic is unchanged.

## What remains before implementing

- **Performance.** A changed cell reshapes its whole run instead of hitting a per-grapheme cache entry. Dense-update workloads (`dense_ascii`, `dense_styled`) need benchmarks. Ghostty needs a run cache for the same reason.
- **Bidi.** The spike covered LTR ligatures only. Placing glyphs by cell should neutralise Parley's visual reordering, but mark attachment and joining for RTL runs split across sections still have to be verified against Ghostty's output.
- **API.** The feature opt-out is new public configuration and needs agreement on its shape.
- **Oracles.** The fidelity oracle shapes one cell at a time. It would need to shape runs itself to state the expectation independently.

These are design and validation work rather than hard blockers in Bevy or Parley. They were deferred because they add public API and a new cache architecture, which is out of proportion to this PR's cropping and placement fixes.
