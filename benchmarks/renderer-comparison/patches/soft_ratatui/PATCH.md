# `soft_ratatui` compatibility patch

This directory is the source of `soft_ratatui` 0.2.0 at upstream commit
`36a833bd9932d119473a930a469ac8d0ca215331`, with only the following change:

- implement `Backend::scroll_region_up` and `Backend::scroll_region_down`, which
  are exposed by `ratatui-core` 0.1.2's `scrolling-regions` feature. The
  implementations move the retained cell rows, clear exposed rows, and call the
  existing full redraw method.

The published crate specifies `ratatui-core = "0.1.0"`, which permits Cargo to
select 0.1.2 but does not implement those feature-gated methods. Without the
patch, `soft_ratatui` and its wrapper `egui_ratatui` do not compile when the
root crate and benchmark enable `scrolling-regions`. No rasterization code or
measured hot path used by the canonical workloads was otherwise changed.

Upstream: <https://github.com/gold-silver-copper/soft_ratatui>
