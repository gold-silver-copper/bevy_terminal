# `egui_ratatui` compatibility patch

This directory is `egui_ratatui` 2.2.0, with one compatibility change: its
backend delegates the `scroll_region_up` and `scroll_region_down` methods
exposed by `ratatui-core` 0.1.2's `scrolling-regions` feature to the wrapped
`soft_ratatui` backend.

The egui widget, image conversion, texture loading, and measured presentation
path are unchanged.

Upstream: <https://github.com/gold-silver-copper/egui_ratatui>
