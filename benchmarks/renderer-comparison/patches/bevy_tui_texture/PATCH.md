# `bevy_tui_texture` compatibility patch

This directory is `bevy_tui_texture` 0.3.4 at upstream commit
`f26c3a253f904f8164ce3df090f0fca6461572d0`, with only the following change:

- implement `Backend::scroll_region_up` and `Backend::scroll_region_down`,
  which are exposed when this workspace enables Ratatui 0.30.2's
  `scrolling-regions` feature. The implementations move/reset retained cells and
  invalidate row geometry so the next payload is a correct full redraw.

The shaping, rasterization, atlas, render-world integration, shaders, texture
management, and all measured hot paths are otherwise upstream source.

Upstream: <https://github.com/tt-toe/bevy_tui_texture>
